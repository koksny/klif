//! Unix `ProcessHost`: process groups. Windows + AMD stays the reference build; macOS is tested, Linux is not.
//!
//! - launch: `std::process::Command` with `process_group(0)` (the root leads a new group whose id is its
//!   pid), run directly (execve, never through a shell), stdin `/dev/null`, stdout/stderr appended to the spec's
//!   log files, the spec's environment changes. The record names the group `pgid:<n>` and keeps the root's start
//!   time. Nothing ties the group to KLIF: it keeps running when KLIF exits.
//! - adopt: the group by id, verified through the root's start time (Linux `/proc/<pid>/stat`, macOS
//!   `proc_pidinfo`). When the root has exited but the group still has members, Linux adopts the group (a live
//!   group id cannot be reused); macOS adopts it only when a member still writes to the session's log file
//!   (its stdout or stderr), so a reused id never makes KLIF adopt or stop a stranger's group.
//! - tree_pids: every non-zombie process whose group is the session's (Linux `/proc/*/stat`, macOS
//!   `proc_listpids`); elsewhere just the live root.
//! - stop: SIGTERM to the group, then SIGKILL after a grace period, bounded waits (20 s in total).
//! - port_owner: Linux = `/proc/net/tcp{,6}` listeners mapped to pids through `/proc/<pid>/fd`; macOS = the
//!   TCP listeners of the user's own processes (`proc_pidfdinfo`, one scan kept for a while), and a loopback
//!   connect probe for a port held by a process KLIF may not inspect; elsewhere the probe alone (owner
//!   unknown).
//!
//! Children that leave the group (setsid / setpgid) are not tracked: the unix host only knows groups.

use std::{
    fs::{File, OpenOptions},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use std::os::unix::process::{CommandExt, ExitStatusExt};

use anyhow::{bail, Result};
use klif_common::launch::LaunchSpec;

use crate::{Owned, PortOwner, ProcState, ProcessHost, SessionRecord, Supervisor};

/// Total stop budget, and the part of it the group gets to exit on SIGTERM before SIGKILL.
const STOP_DEADLINE: Duration = Duration::from_secs(20);
const TERM_GRACE: Duration = Duration::from_secs(5);

/// The few libc calls the basic host needs (declared here instead of adding a crate; `pid_t` and `int` are
/// 32-bit on every supported unix, and these signal numbers are the same on Linux and the BSDs/macOS).
mod sys {
    extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
        pub fn getpgid(pid: i32) -> i32;
    }
    pub const SIGKILL: i32 = 9;
    pub const SIGTERM: i32 = 15;
    pub const EPERM: i32 = 1;
}

/// `kill(target, 0)`: true while the process (target > 0) or group (target < 0) exists, even when KLIF may
/// not signal it.
fn exists(target: i32) -> bool {
    if unsafe { sys::kill(target, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(sys::EPERM)
}

/// Signal a session's group; never KLIF's own group (a corrupted record must not take KLIF down with it).
fn signal_group(pgid: i32, sig: i32) {
    if pgid > 1 && pgid != unsafe { sys::getpgid(0) } {
        unsafe {
            sys::kill(-pgid, sig);
        }
    }
}

// ---------------------------------------------------------------- libproc (macOS)

/// macOS process facts through libproc. Struct offsets below are those of `<sys/proc_info.h>` (the same on
/// arm64 and x86_64; lsof relies on them too).
#[cfg(target_os = "macos")]
mod darwin {
    use std::ffi::{c_void, CStr};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const PROC_PGRP_ONLY: u32 = 2;
    const PROC_UID_ONLY: u32 = 4;
    const PROC_PIDFDVNODEPATHINFO: i32 = 2;
    const PROC_PIDFDSOCKETINFO: i32 = 3;
    const SOCKINFO_TCP: i32 = 2;
    const TSI_S_LISTEN: i32 = 1;
    /// `sizeof(struct socket_fdinfo)` and the offsets of `psi.soi_kind`,
    /// `psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport` (network byte order) and `psi.soi_proto.pri_tcp.tcpsi_state`.
    const SOCKET_FDINFO_SIZE: usize = 792;
    const SOI_KIND: usize = 256;
    const TCP_LPORT: usize = 268;
    const TCP_STATE: usize = 344;
    /// `sizeof(struct vnode_fdinfowithpath)` and the offset of `pvip.vip_path` (MAXPATHLEN bytes).
    const VNODE_PATH_SIZE: usize = 1200;
    const VNODE_PATH: usize = 176;

    fn bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
        (n == size).then_some(info)
    }

    /// (zombie, process group, start time in microseconds since 1970).
    pub fn stat(pid: u32) -> Option<(bool, i32, u64)> {
        let i = bsdinfo(pid)?;
        Some((i.pbi_status == libc::SZOMB, i.pbi_pgid as i32, i.pbi_start_tvsec * 1_000_000 + i.pbi_start_tvusec))
    }

    /// `proc_listpids(kind, arg)`: the pids it returns (it counts in bytes).
    fn list_pids(kind: u32, arg: u32) -> Vec<u32> {
        let need = unsafe { libc::proc_listpids(kind, arg, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Vec::new();
        }
        // Room for processes started between the two calls.
        let mut buf = vec![0i32; need as usize / 4 + 64];
        let n = unsafe { libc::proc_listpids(kind, arg, buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
        buf.truncate(n.max(0) as usize / 4);
        buf.into_iter().filter(|&p| p > 0).map(|p| p as u32).collect()
    }

    /// Every process of the group (zombies included).
    pub fn group_members(pgid: i32) -> Vec<u32> {
        list_pids(PROC_PGRP_ONLY, pgid as u32)
    }

    pub fn image_name(pid: u32) -> Option<String> {
        // pid 0 is how a port owner KLIF may not inspect is reported, not the kernel.
        if pid == 0 {
            return None;
        }
        let mut buf = [0u8; 4 * 1024];
        let n = unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n > 0 {
            let path = PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned());
            if let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) {
                return Some(name.to_string());
            }
        }
        let n = unsafe { libc::proc_name(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned()).filter(|s| !s.is_empty())
    }

    /// Open descriptors of `pid` with their type (empty when KLIF may not read them: another user's process).
    fn fds(pid: u32) -> Vec<libc::proc_fdinfo> {
        let need = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return Vec::new();
        }
        let each = std::mem::size_of::<libc::proc_fdinfo>();
        let mut buf: Vec<libc::proc_fdinfo> = vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; need as usize / each + 16];
        let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, buf.as_mut_ptr().cast(), (buf.len() * each) as i32) };
        buf.truncate(n.max(0) as usize / each);
        buf
    }

    /// `proc_pidfdinfo` into an 8-byte aligned buffer of exactly `size` bytes.
    fn fd_info(pid: u32, fd: i32, flavor: i32, size: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u64; size.div_ceil(8)];
        let n = unsafe { libc::proc_pidfdinfo(pid as i32, fd, flavor, buf.as_mut_ptr().cast::<c_void>(), size as i32) };
        if n as usize != size {
            return None;
        }
        Some(buf.iter().flat_map(|w| w.to_ne_bytes()).take(size).collect())
    }

    fn i32_at(b: &[u8], off: usize) -> i32 {
        i32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
    }

    /// The ports `pid` listens on over TCP.
    fn listening_ports(pid: u32) -> Vec<u16> {
        fds(pid)
            .into_iter()
            .filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            .filter_map(|f| fd_info(pid, f.proc_fd, PROC_PIDFDSOCKETINFO, SOCKET_FDINFO_SIZE))
            .filter(|b| i32_at(b, SOI_KIND) == SOCKINFO_TCP && i32_at(b, TCP_STATE) == TSI_S_LISTEN)
            .map(|b| u16::from_be(i32_at(&b, TCP_LPORT) as u16))
            .collect()
    }

    pub fn listens_on(pid: u32, port: u16) -> bool {
        listening_ports(pid).contains(&port)
    }

    /// Who listens on a port, as far as KLIF can see.
    pub enum Listener {
        /// Nobody.
        None,
        /// A process of this user.
        Pid(u32),
        /// Someone KLIF may not inspect (another user's process, root's): a loopback connect is accepted.
        Hidden,
    }

    /// When a scan ran, and the (port, pid) of every TCP listener of the user's own processes it found.
    type Scan = Option<(Instant, Vec<(u16, u32)>)>;
    static LISTENERS: Mutex<Scan> = Mutex::new(None);
    /// Ports a scan found held by someone KLIF may not inspect, and when: they are not scanned for again while
    /// they keep accepting, for `LISTENERS_TTL`.
    static HIDDEN: Mutex<Vec<(u16, Instant)>> = Mutex::new(Vec::new());
    /// How long a scan is trusted. The engine asks about every System's port twice a second and a scan reads
    /// the descriptors of every process of the user (milliseconds), so a scan is repeated only when it is this
    /// old or when it disagrees with what is seen: its listener is gone, or a port it saw free accepts (a port
    /// already known to be held by another user's process is not rescanned for until the scan expires).
    const LISTENERS_TTL: Duration = Duration::from_secs(2);

    fn scan() -> Vec<(u16, u32)> {
        let uid = unsafe { libc::getuid() };
        list_pids(PROC_UID_ONLY, uid)
            .into_iter()
            .flat_map(|p| listening_ports(p).into_iter().map(move |port| (port, p)))
            .collect()
    }

    fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    fn loopback_accepts(port: u16) -> bool {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
    }

    /// Who listens on `port`. A known listener is never connected to; a port believed free is checked with a
    /// loopback connect (refused at once when it is free).
    pub fn listener(port: u16) -> Listener {
        let mut cache = LISTENERS.lock().unwrap_or_else(|p| p.into_inner());
        let mut hidden = HIDDEN.lock().unwrap_or_else(|p| p.into_inner());
        hidden.retain(|(_, at)| at.elapsed() < LISTENERS_TTL);
        let find = |c: &Scan| c.as_ref().and_then(|(_, all)| all.iter().find(|(p, _)| *p == port).map(|(_, pid)| *pid));
        if cache.as_ref().is_some_and(|(at, _)| at.elapsed() < LISTENERS_TTL) {
            match find(&cache) {
                Some(pid) if alive(pid) => return Listener::Pid(pid),
                Some(_) => {}
                None if !loopback_accepts(port) => return Listener::None,
                None if hidden.iter().any(|(p, _)| *p == port) => return Listener::Hidden,
                None => {}
            }
        }
        *cache = Some((Instant::now(), scan()));
        hidden.retain(|(p, _)| *p != port);
        match find(&cache) {
            Some(pid) => Listener::Pid(pid),
            None if loopback_accepts(port) => {
                hidden.push((port, Instant::now()));
                Listener::Hidden
            }
            None => Listener::None,
        }
    }

    /// The file `pid` has open as descriptor `fd`, when it is a file.
    fn fd_path(pid: u32, fd: i32) -> Option<PathBuf> {
        let b = fd_info(pid, fd, PROC_PIDFDVNODEPATHINFO, VNODE_PATH_SIZE)?;
        let path = CStr::from_bytes_until_nul(&b[VNODE_PATH..]).ok()?;
        Some(PathBuf::from(path.to_string_lossy().into_owned()))
    }

    /// True when `pid` writes its stdout or stderr into one of `logs` (a process KLIF started for that session
    /// or one of its children).
    pub fn writes_to(pid: u32, logs: &[&Path]) -> bool {
        let logs: Vec<PathBuf> = logs.iter().filter_map(|l| std::fs::canonicalize(l).ok()).collect();
        !logs.is_empty() && [1, 2].into_iter().filter_map(|fd| fd_path(pid, fd)).any(|p| logs.contains(&p))
    }
}

// ---------------------------------------------------------------- /proc (Linux)

/// (state, pgrp, start time in ticks since boot) from `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
fn proc_stat(pid: u32) -> Option<(char, i32, u64)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) may contain spaces and parentheses: fields resume after the LAST ')'.
    let rest = &s[s.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // f[0] = field 3 (state), f[2] = field 5 (pgrp), f[19] = field 22 (starttime)
    Some((f.first()?.chars().next()?, f.get(2)?.parse().ok()?, f.get(19)?.parse().ok()?))
}

/// Start time of a live (non-zombie) process, the value recorded as `root_ctime`; 0 where unknown.
fn start_time(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        proc_stat(pid).filter(|s| s.0 != 'Z').map(|s| s.2)
    }
    #[cfg(target_os = "macos")]
    {
        darwin::stat(pid).filter(|s| !s.0).map(|s| s.2)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        exists(pid as i32).then_some(0)
    }
}

/// True when `pid` is alive and is the process the record describes (start time matches where known; on macOS
/// it is always known, so it must match).
fn same_process(pid: u32, ctime: u64) -> bool {
    #[cfg(target_os = "macos")]
    {
        pid > 1 && ctime != 0 && start_time(pid) == Some(ctime)
    }
    #[cfg(not(target_os = "macos"))]
    {
        pid > 1 && start_time(pid).is_some_and(|t| ctime == 0 || t == 0 || t == ctime)
    }
}

#[cfg(target_os = "linux")]
fn all_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|d| d.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok())).collect())
        .unwrap_or_default()
}

/// Live members of a process group (zombies excluded).
fn group_pids(pgid: i32) -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut pids: Vec<u32> = all_pids()
            .into_iter()
            .filter(|&p| proc_stat(p).is_some_and(|(state, g, _)| g == pgid && state != 'Z'))
            .collect();
        pids.sort_unstable();
        pids
    }
    #[cfg(target_os = "macos")]
    {
        let mut pids: Vec<u32> = darwin::group_members(pgid)
            .into_iter()
            .filter(|&p| darwin::stat(p).is_some_and(|(zombie, g, _)| g == pgid && !zombie))
            .collect();
        pids.sort_unstable();
        pids
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        if exists(-pgid) && exists(pgid) { vec![pgid as u32] } else { Vec::new() }
    }
}

fn group_alive(pgid: i32) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        !group_pids(pgid).is_empty()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        exists(-pgid)
    }
}

/// The root has exited but its group lives on: may KLIF take the group over? Linux: yes (a live group id is
/// never reused). macOS: only when a member still writes to the session's log, which proves it descends from
/// the process KLIF started.
fn orphans_are_ours(pgid: i32, record: &SessionRecord) -> bool {
    #[cfg(target_os = "macos")]
    {
        let logs = [record.out_log.as_path(), record.err_log.as_path()];
        group_pids(pgid).into_iter().any(|p| darwin::writes_to(p, &logs))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = record;
        group_alive(pgid)
    }
}

/// Socket inodes listening on `port` (IPv4 and IPv6).
#[cfg(target_os = "linux")]
fn listening_inodes(port: u16) -> Vec<u64> {
    let mut out = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else { continue };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            // sl local_address rem_address st tx:rx tr:when retrnsmt uid timeout inode
            let (Some(local), Some(st), Some(inode)) = (f.get(1), f.get(3), f.get(9)) else { continue };
            let lport = local.rsplit(':').next().and_then(|p| u16::from_str_radix(p, 16).ok());
            if *st == "0A" && lport == Some(port) {
                if let Ok(i) = inode.parse::<u64>() {
                    if i != 0 {
                        out.push(i);
                    }
                }
            }
        }
    }
    out
}

/// True when one of `pid`'s open descriptors is one of the socket `inodes`.
#[cfg(target_os = "linux")]
fn holds_socket(pid: u32, inodes: &[u64]) -> bool {
    let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { return false };
    dir.flatten().any(|e| {
        std::fs::read_link(e.path()).ok().and_then(|t| {
            let t = t.to_string_lossy().into_owned();
            t.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')).and_then(|n| n.parse::<u64>().ok())
        })
        .is_some_and(|i| inodes.contains(&i))
    })
}

pub(crate) fn image_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(target) = std::fs::read_link(format!("/proc/{pid}/exe")) {
            if let Some(name) = target.file_name().and_then(std::ffi::OsStr::to_str) {
                // A replaced binary reads "name (deleted)".
                return Some(name.trim_end_matches(" (deleted)").to_string());
            }
        }
        std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }
    #[cfg(target_os = "macos")]
    {
        darwin::image_name(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

// ---------------------------------------------------------------- launch helpers

fn open_log(path: &Path) -> Result<File> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("Cannot create the log folder {}: {e}.", dir.display()))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| anyhow::anyhow!("Cannot open the log file {}: {e}.", path.display()))
}

/// Exit code as a number: the process's own code, or 128 + signal (the shell convention) when it was killed.
fn code_of(status: ExitStatus) -> u32 {
    match (status.code(), status.signal()) {
        (Some(c), _) => c as u32,
        (None, Some(sig)) => 128 + sig as u32,
        (None, None) => 1,
    }
}

/// Reap the root if it is KLIF's own child; Some(code) once it has exited.
fn reap(owned: &Owned) -> Option<u32> {
    let child = owned.child.as_ref()?;
    let mut c = child.lock().unwrap_or_else(|p| p.into_inner());
    c.try_wait().ok().flatten().map(code_of)
}

/// The new root's start time for the record. Without it (0) macOS can never verify the session again, so it is
/// asked twice and a miss is logged.
fn launch_start_time(pid: u32) -> u64 {
    if let Some(t) = start_time(pid).or_else(|| {
        std::thread::sleep(Duration::from_millis(20));
        start_time(pid)
    }) {
        return t;
    }
    if cfg!(target_os = "macos") {
        log::warn!("the start time of pid {pid} could not be read: KLIF will not adopt this session after a restart");
    }
    0
}

fn now_s() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// The record's process group; never KLIF's own.
fn pgid_of(record: &SessionRecord) -> Option<i32> {
    let own = unsafe { sys::getpgid(0) };
    record.job_name.as_deref()?.strip_prefix("pgid:")?.parse::<i32>().ok().filter(|&g| g > 1 && g != own)
}

fn pid_list(pids: &[u32]) -> String {
    pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

impl ProcessHost for Supervisor {
    fn launch(&self, spec: &LaunchSpec) -> Result<Owned> {
        if spec.session_name.trim().is_empty() {
            bail!("The launch has no session name.");
        }
        if !spec.exe.is_absolute() {
            bail!("The program path must be absolute: {}.", spec.exe.display());
        }
        if !spec.cwd.is_dir() {
            bail!("The working folder does not exist: {}.", spec.cwd.display());
        }
        let out = open_log(&spec.out_log)?;
        let err = if spec.err_log == spec.out_log {
            out.try_clone().map_err(|e| anyhow::anyhow!("Cannot share the log file {}: {e}.", spec.out_log.display()))?
        } else {
            open_log(&spec.err_log)?
        };

        let mut cmd = Command::new(&spec.exe);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .process_group(0);
        // `env_remove` names are case-insensitive (LaunchSpec contract): drop every inherited spelling.
        for (name, _) in std::env::vars_os() {
            let n = name.to_string_lossy();
            if spec.env_remove.iter().any(|r| r.eq_ignore_ascii_case(&n)) {
                cmd.env_remove(&name);
            }
        }
        for (name, value) in &spec.env_set {
            cmd.env(name, value.expose());
        }
        let child = cmd.spawn().map_err(|e| anyhow::anyhow!("Cannot start {}: {e}.", spec.exe.display()))?;
        let pid = child.id();
        let record = SessionRecord {
            session_name: spec.session_name.clone(),
            job_name: Some(format!("pgid:{pid}")),
            root_pid: pid,
            root_ctime: launch_start_time(pid),
            started_at: now_s(),
            out_log: spec.out_log.clone(),
            err_log: spec.err_log.clone(),
            port: spec.port,
        };
        Ok(Owned { record, pgid: Some(pid as i32), child: Some(Mutex::new(child)) })
    }

    /// The group by id: ours when the root is alive and verified (and still leads it), or when the root is
    /// gone but the group still has members (a live group id is never reused). A live root without a usable
    /// group is adopted monitor-only.
    fn adopt(&self, record: &SessionRecord) -> Result<Owned> {
        let root_ok = same_process(record.root_pid, record.root_ctime);
        let root_reused = !root_ok && exists(record.root_pid as i32) && start_time(record.root_pid).is_some();
        if let Some(g) = pgid_of(record) {
            let leads = root_ok && unsafe { sys::getpgid(record.root_pid as i32) } == g;
            let orphans = !root_ok && !root_reused && orphans_are_ours(g, record);
            if leads || orphans {
                return Ok(Owned { record: record.clone(), pgid: Some(g), child: None });
            }
        }
        if root_ok {
            return Ok(Owned { record: record.clone(), pgid: None, child: None });
        }
        bail!("Session {} is no longer running.", record.session_name)
    }

    fn state(&self, owned: &Owned) -> ProcState {
        if let Some(code) = reap(owned) {
            return ProcState::Exited { code };
        }
        let pids = self.tree_pids(owned);
        if pids.is_empty() {
            ProcState::Gone
        } else {
            ProcState::Running { pids }
        }
    }

    fn tree_pids(&self, owned: &Owned) -> Vec<u32> {
        let root = owned.record.root_pid;
        let mut pids = match owned.pgid {
            Some(g) => group_pids(g),
            None => Vec::new(),
        };
        if owned.pgid.is_none() && same_process(root, owned.record.root_ctime) {
            pids.push(root);
        }
        if let Some(i) = pids.iter().position(|&p| p == root) {
            pids[..=i].rotate_right(1);
        }
        pids
    }

    fn stop(&self, owned: &Owned) -> Result<()> {
        let Some(g) = owned.pgid else {
            if same_process(owned.record.root_pid, owned.record.root_ctime) {
                bail!(
                    "Session {} has no process group KLIF owns, so KLIF cannot stop it safely; stop process {} yourself.",
                    owned.record.session_name,
                    owned.record.root_pid
                );
            }
            return Ok(());
        };
        let start = Instant::now();
        let gone = || {
            reap(owned);
            !group_alive(g)
        };
        if gone() {
            return Ok(());
        }
        let name = &owned.record.session_name;
        signal_group(g, sys::SIGTERM);
        while !gone() && start.elapsed() < TERM_GRACE {
            std::thread::sleep(Duration::from_millis(50));
        }
        if gone() {
            log::info!("session {name} (group {g}) ended on SIGTERM after {:.1} s", start.elapsed().as_secs_f64());
        } else {
            log::warn!("session {name} (group {g}) still runs {} s after SIGTERM: sending SIGKILL", TERM_GRACE.as_secs());
            signal_group(g, sys::SIGKILL);
            while !gone() && start.elapsed() < STOP_DEADLINE {
                std::thread::sleep(Duration::from_millis(50));
            }
            if gone() {
                log::info!("session {name} (group {g}) ended on SIGKILL after {:.1} s", start.elapsed().as_secs_f64());
            }
        }
        if !gone() {
            bail!(
                "Session {} did not stop within {} s; still running: pid {}.",
                owned.record.session_name,
                STOP_DEADLINE.as_secs(),
                pid_list(&group_pids(g))
            );
        }
        Ok(())
    }

    fn port_owner(&self, port: u16, ours: Option<&Owned>) -> PortOwner {
        #[cfg(target_os = "linux")]
        {
            let inodes = listening_inodes(port);
            if inodes.is_empty() {
                return PortOwner::Free;
            }
            if let Some(owned) = ours {
                if let Some(pid) = self.tree_pids(owned).into_iter().find(|&p| holds_socket(p, &inodes)) {
                    return PortOwner::Ours { pid };
                }
            }
            match all_pids().into_iter().find(|&p| holds_socket(p, &inodes)) {
                Some(pid) => PortOwner::Foreign { pid, image: image_name(pid).unwrap_or_else(|| "?".into()) },
                // Held by a process KLIF may not inspect (another user's).
                None => PortOwner::Foreign { pid: 0, image: "?".into() },
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(owned) = ours {
                if let Some(pid) = self.tree_pids(owned).into_iter().find(|&p| darwin::listens_on(p, port)) {
                    return PortOwner::Ours { pid };
                }
            }
            match darwin::listener(port) {
                darwin::Listener::None => PortOwner::Free,
                darwin::Listener::Pid(pid) => PortOwner::Foreign { pid, image: image_name(pid).unwrap_or_else(|| "?".into()) },
                darwin::Listener::Hidden => PortOwner::Foreign { pid: 0, image: "?".into() },
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err() {
                return PortOwner::Free;
            }
            match ours.map(|o| self.tree_pids(o)).and_then(|p| p.first().copied()) {
                Some(pid) => PortOwner::Ours { pid },
                None => PortOwner::Foreign { pid: 0, image: "?".into() },
            }
        }
    }
}
