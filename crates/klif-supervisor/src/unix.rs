//! Unix `ProcessHost`: process groups, basic. Windows + AMD stays the tested build.
//!
//! - launch: `std::process::Command` with `process_group(0)` (the root leads a new group whose id is its
//!   pid), stdin `/dev/null`, stdout/stderr appended to the spec's log files, the spec's environment changes.
//!   The record names the group `pgid:<n>`.
//! - adopt: the group by id, verified through the root's start time on Linux (`/proc/<pid>/stat`); a group
//!   that still has members is adopted even when the root has exited (a live group id cannot be reused).
//! - tree_pids: Linux = every non-zombie process whose group is the session's (`/proc/*/stat`); elsewhere just
//!   the live root.
//! - stop: SIGTERM to the group, then SIGKILL after a grace period, bounded waits (20 s in total).
//! - port_owner: Linux = `/proc/net/tcp{,6}` listeners mapped to pids through `/proc/<pid>/fd`; elsewhere a
//!   loopback connect probe (owner unknown).
//!
//! Children that leave the group (setsid / setpgid) are not tracked: the basic unix host only knows groups.

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
    #[cfg(not(target_os = "linux"))]
    {
        exists(pid as i32).then_some(0)
    }
}

/// True when `pid` is alive and is the process the record describes (start time matches where known).
fn same_process(pid: u32, ctime: u64) -> bool {
    pid > 1 && start_time(pid).is_some_and(|t| ctime == 0 || t == 0 || t == ctime)
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
    #[cfg(not(target_os = "linux"))]
    {
        if exists(-pgid) && exists(pgid) { vec![pgid as u32] } else { Vec::new() }
    }
}

fn group_alive(pgid: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        !group_pids(pgid).is_empty()
    }
    #[cfg(not(target_os = "linux"))]
    {
        exists(-pgid)
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
    #[cfg(not(target_os = "linux"))]
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
            root_ctime: start_time(pid).unwrap_or(0),
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
            let orphans = !root_ok && !root_reused && group_alive(g);
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
        signal_group(g, sys::SIGTERM);
        while !gone() && start.elapsed() < TERM_GRACE {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !gone() {
            signal_group(g, sys::SIGKILL);
            while !gone() && start.elapsed() < STOP_DEADLINE {
                std::thread::sleep(Duration::from_millis(50));
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
        #[cfg(not(target_os = "linux"))]
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
