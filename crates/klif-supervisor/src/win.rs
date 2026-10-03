//! Win32 helpers for the supervisor: owned handles, process times, job queries and the TCP listener table,
//! plus the Windows `ProcessHost` implementation (jobs). No process enumeration, no tree kill: a session's
//! processes are exactly its job's process list, and stopping a session is `TerminateJobObject`.

use std::{
    ffi::c_void,
    mem::size_of,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, Result};
use klif_common::launch::LaunchSpec;

use crate::{Owned, PortOwner, ProcState, ProcessHost, SessionRecord, Supervisor, STOP_EXIT_CODE};

use windows::{
    core::{BOOL, HRESULT, HSTRING, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, FILETIME, HANDLE, NO_ERROR, WAIT_TIMEOUT},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::{AF_INET, AF_INET6},
        System::{
            JobObjects::{
                IsProcessInJob, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, OpenJobObjectW,
                QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
            },
            SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE},
            Threading::{
                GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
                WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE,
            },
        },
    },
};

/// How long `stop` waits in total (job emptied, processes exited, port released). A GPU server can take
/// seconds to really die after TerminateJobObject: the driver frees many GiB, possibly after waking the
/// card from D3 first.
const STOP_DEADLINE: Duration = Duration::from_secs(20);

/// `port_owner` runs for every System on every engine tick: one listener-table read serves a whole tick.
const LISTENER_CACHE_TTL: Duration = Duration::from_millis(150);

/// An owned kernel handle, closed on drop. Kernel handles may be used from any thread.
pub(crate) struct Handle(HANDLE);

// SAFETY: a kernel object handle is a process-wide value; the APIs used on it are thread-safe.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    pub(crate) fn new(h: HANDLE) -> Handle {
        Handle(h)
    }
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Handle({:p})", self.0 .0)
    }
}

fn ft(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// Creation time (FILETIME, 100 ns since 1601) of the process behind `h`.
pub(crate) fn handle_ctime(h: HANDLE) -> Option<u64> {
    let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) }.ok()?;
    Some(ft(c))
}

pub(crate) fn open_process(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<Handle> {
    if pid == 0 {
        return None;
    }
    unsafe { OpenProcess(access, false, pid) }.ok().map(Handle)
}

/// True while the process behind `h` (opened with SYNCHRONIZE) has not exited.
pub(crate) fn is_running(h: HANDLE) -> bool {
    let r = unsafe { WaitForSingleObject(h, 0) };
    r == WAIT_TIMEOUT
}

/// Exit code once the process has exited, None while it runs or if the code cannot be read.
pub(crate) fn exit_code(h: HANDLE) -> Option<u32> {
    if is_running(h) {
        return None;
    }
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(h, &mut code) }.ok()?;
    Some(code)
}

/// Open `pid` only if it is still the same process (creation time matches). Includes
/// QUERY_LIMITED_INFORMATION + SYNCHRONIZE so `is_running`/`exit_code` work on the handle.
pub(crate) fn open_verified(pid: u32, ctime: u64, extra: PROCESS_ACCESS_RIGHTS) -> Option<Handle> {
    let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | extra)?;
    (handle_ctime(h.raw()) == Some(ctime)).then_some(h)
}

/// Executable file name of `pid` (QueryFullProcessImageNameW, microseconds). None when it cannot be opened.
pub(crate) fn image_name(pid: u32) -> Option<String> {
    match pid {
        0 => return Some("System Idle Process".into()),
        4 => return Some("System".into()),
        _ => {}
    }
    let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h.raw(), PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    full.rsplit(['\\', '/']).next().filter(|n| !n.is_empty()).map(str::to_string)
}

// ---------------------------------------------------------------- jobs

/// Reopen a session job by name (query + terminate rights only). None when no such job exists any more.
pub(crate) fn open_job(name: &str) -> Option<Handle> {
    let n = HSTRING::from(name);
    unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, false, &n) }.ok().map(Handle)
}

/// PIDs currently in the job (`JobObjectBasicProcessIdList`: only processes that have not terminated).
pub(crate) fn job_pids(job: HANDLE) -> Vec<u32> {
    let mut cap = 64usize;
    loop {
        // header (2 x u32) + `cap` ULONG_PTR ids; 2 extra usize cover the header on any pointer width
        let mut buf = vec![0usize; 2 + cap];
        let r = unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr() as *mut c_void,
                (buf.len() * size_of::<usize>()) as u32,
                None,
            )
        };
        let l = unsafe { &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
        let grow = (l.NumberOfAssignedProcesses as usize + 16).max(cap * 2);
        match r {
            // A process may join between the two counts: read again with room for everyone.
            Ok(()) if l.NumberOfProcessIdsInList < l.NumberOfAssignedProcesses && cap < 1 << 16 => cap = grow,
            Ok(()) => {
                let n = (l.NumberOfProcessIdsInList as usize).min(cap);
                let ids = unsafe { std::slice::from_raw_parts(l.ProcessIdList.as_ptr(), n) };
                return ids.iter().map(|&p| p as u32).filter(|&p| p != 0).collect();
            }
            Err(e) if e.code() == HRESULT::from_win32(ERROR_MORE_DATA.0) && cap < 1 << 16 => cap = grow,
            Err(_) => return Vec::new(),
        }
    }
}

pub(crate) fn terminate_job(job: HANDLE, code: u32) -> bool {
    unsafe { TerminateJobObject(job, code) }.is_ok()
}

pub(crate) fn process_in_job(process: HANDLE, job: HANDLE) -> bool {
    let mut r = BOOL(0);
    unsafe { IsProcessInJob(process, Some(job), &mut r) }.is_ok() && r.as_bool()
}

/// The session job named in `record`, if it still exists AND is really this session's: it lists processes,
/// and a live root (`root`, verified by creation time) is one of them.
fn reopen_session_job(record: &SessionRecord, root: Option<&Handle>) -> Option<Handle> {
    let job = record.job_name.as_deref().and_then(open_job)?;
    if job_pids(job.raw()).is_empty() {
        return None;
    }
    match root {
        Some(r) if is_running(r.raw()) && !process_in_job(r.raw(), job.raw()) => None,
        _ => Some(job),
    }
}

/// The job KLIF itself runs in: None when not in a job, else the immediate job's LimitFlags
/// (None inside when they cannot be read).
pub(crate) fn own_job_limit_flags() -> Option<Option<u32>> {
    let mut injob = BOOL(0);
    if unsafe { IsProcessInJob(GetCurrentProcess(), None, &mut injob) }.is_err() || !injob.as_bool() {
        return None;
    }
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let r = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            None,
        )
    };
    Some(r.ok().map(|_| info.BasicLimitInformation.LimitFlags.0))
}

// ---------------------------------------------------------------- TCP listeners

/// (local port, owning PID) of every IPv4 and IPv6 TCP listener (GetExtendedTcpTable, owner-pid tables).
pub(crate) fn tcp_listeners() -> Vec<(u16, u32)> {
    let mut out = Vec::new();
    for af in [AF_INET.0 as u32, AF_INET6.0 as u32] {
        let mut size = 0u32;
        let _ = unsafe { GetExtendedTcpTable(None, &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        let mut ok = None;
        for _ in 0..4 {
            // u32 buffer: the tables are 4-byte aligned; a little slack for listeners appearing meanwhile
            let mut buf = vec![0u32; (size as usize).div_ceil(4) + 64];
            size = (buf.len() * 4) as u32;
            let r = unsafe {
                GetExtendedTcpTable(Some(buf.as_mut_ptr() as *mut c_void), &mut size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0)
            };
            if r == NO_ERROR.0 {
                ok = Some(buf);
                break;
            }
            if r != ERROR_INSUFFICIENT_BUFFER.0 {
                break;
            }
        }
        let Some(buf) = ok else { continue };
        if af == AF_INET.0 as u32 {
            let t = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
            let rows = unsafe { std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize) };
            out.extend(rows.iter().map(|r| (u16::from_be(r.dwLocalPort as u16), r.dwOwningPid)));
        } else {
            let t = unsafe { &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID) };
            let rows = unsafe { std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize) };
            out.extend(rows.iter().map(|r| (u16::from_be(r.dwLocalPort as u16), r.dwOwningPid)));
        }
    }
    out
}

/// (read at, generation, rows) of the last listener-table read.
type ListenerCache = Option<(Instant, u64, Vec<(u16, u32)>)>;

static LISTENER_CACHE: Mutex<ListenerCache> = Mutex::new(None);
/// Bumped whenever ports were released or taken by KLIF itself (end of `stop`, after `launch`): a cached
/// table from an older generation is never served, so "stop X, then launch Y on X's port" never sees X's
/// listener again.
static LISTENER_GEN: AtomicU64 = AtomicU64::new(0);

fn invalidate_listener_cache() {
    LISTENER_GEN.fetch_add(1, Ordering::SeqCst);
}

/// `tcp_listeners()` at most `LISTENER_CACHE_TTL` old and from the current generation (for `port_owner`;
/// `stop` always reads fresh).
fn tcp_listeners_cached() -> Vec<(u16, u32)> {
    let generation = LISTENER_GEN.load(Ordering::SeqCst);
    {
        let c = LISTENER_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, g, rows)) = c.as_ref() {
            if *g == generation && at.elapsed() < LISTENER_CACHE_TTL {
                return rows.clone();
            }
        }
    }
    let rows = tcp_listeners();
    // Tagged with the generation seen BEFORE the read: a read that raced an invalidation is never reused.
    *LISTENER_CACHE.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), generation, rows.clone()));
    rows
}

// ---------------------------------------------------------------- own job

/// See `crate::parent_job_warning`.
pub(crate) fn parent_job_warning() -> Option<String> {
    let flags = own_job_limit_flags()?; // None: not in a job at all
    let Some(flags) = flags else {
        return Some("KLIF runs inside a job whose limits cannot be read; servers may not survive KLIF exiting".into());
    };
    if flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0 == 0 {
        return None;
    }
    let breakaway = if flags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK.0 | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK.0) != 0 {
        "breakaway is allowed"
    } else {
        "breakaway is not allowed"
    };
    Some(format!(
        "KLIF runs inside a job with KILL_ON_JOB_CLOSE (LimitFlags=0x{flags:X}, {breakaway}): servers it starts will be killed when that job closes"
    ))
}

// ---------------------------------------------------------------- ProcessHost (Windows: jobs)

fn filetime_to_unix_s(ft: u64) -> f64 {
    /// FILETIME of the Unix epoch.
    const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;
    (ft as f64 - UNIX_EPOCH_FILETIME as f64) / 1e7
}

fn pid_list(pids: &[u32]) -> String {
    pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

impl ProcessHost for Supervisor {
    fn launch(&self, spec: &LaunchSpec) -> Result<Owned> {
        if spec.session_name.trim().is_empty() {
            bail!("The launch has no session name.");
        }
        let job_name = crate::spawn::job_name_for(&spec.session_name);
        let s = crate::spawn::spawn(spec, &job_name)?;
        invalidate_listener_cache();
        let record = SessionRecord {
            session_name: spec.session_name.clone(),
            job_name: Some(job_name),
            root_pid: s.pid,
            root_ctime: s.ctime,
            started_at: filetime_to_unix_s(s.ctime),
            out_log: spec.out_log.clone(),
            err_log: spec.err_log.clone(),
            port: spec.port,
        };
        Ok(Owned { record, job: Some(s.job), root: Some(s.process) })
    }

    /// The job is reopened by name (`OpenJobObjectW`) and the root is verified by PID + creation time. A job
    /// that still lists processes is adopted even when the root itself has exited (its children still run);
    /// a live root must be a member of it, otherwise the name is not ours. Without a usable job (job-less
    /// record, or the name is gone) a live, verified root is adopted monitor-only: `stop` then answers with
    /// a sentence instead of killing anything.
    fn adopt(&self, record: &SessionRecord) -> Result<Owned> {
        let root = open_verified(record.root_pid, record.root_ctime, PROCESS_ACCESS_RIGHTS(0)).filter(|h| is_running(h.raw()));
        if let Some(job) = reopen_session_job(record, root.as_ref()) {
            return Ok(Owned { record: record.clone(), job: Some(job), root });
        }
        match root {
            Some(root) => Ok(Owned { record: record.clone(), job: None, root: Some(root) }),
            None => bail!("Session {} is no longer running.", record.session_name),
        }
    }

    fn state(&self, owned: &Owned) -> ProcState {
        if let Some(root) = &owned.root {
            if let Some(code) = exit_code(root.raw()) {
                return ProcState::Exited { code };
            }
            if is_running(root.raw()) {
                let mut pids = self.tree_pids(owned);
                if !pids.contains(&owned.record.root_pid) {
                    pids.insert(0, owned.record.root_pid);
                }
                return ProcState::Running { pids };
            }
        }
        let pids = self.tree_pids(owned);
        if pids.is_empty() {
            ProcState::Gone
        } else {
            ProcState::Running { pids }
        }
    }

    /// The job's process id list when the job is held, else just the live root. Root first when alive.
    fn tree_pids(&self, owned: &Owned) -> Vec<u32> {
        let mut pids = match &owned.job {
            Some(job) => job_pids(job.raw()),
            None => owned.root.as_ref().filter(|h| is_running(h.raw())).map(|_| vec![owned.record.root_pid]).unwrap_or_default(),
        };
        if let Some(i) = pids.iter().position(|&p| p == owned.record.root_pid) {
            pids[..=i].rotate_right(1);
        }
        pids
    }

    /// TerminateJobObject, then bounded waits (`STOP_DEADLINE` in total): the job's list empties, every
    /// process that was in it has really exited (handles opened before the kill pin their PIDs), and none
    /// of them still listens on the session's port. Idempotent.
    fn stop(&self, owned: &Owned) -> Result<()> {
        let r = self.stop_job(owned);
        invalidate_listener_cache();
        r
    }

    fn port_owner(&self, port: u16, ours: Option<&Owned>) -> PortOwner {
        let mut holders: Vec<u32> = tcp_listeners_cached().into_iter().filter(|l| l.0 == port).map(|l| l.1).collect();
        holders.sort_unstable();
        holders.dedup();
        if holders.is_empty() {
            return PortOwner::Free;
        }
        if let Some(owned) = ours {
            let tree = self.tree_pids(owned);
            if let Some(&pid) = holders.iter().find(|p| tree.contains(p)) {
                return PortOwner::Ours { pid };
            }
        }
        let pid = holders[0];
        PortOwner::Foreign { pid, image: image_name(pid).unwrap_or_else(|| "?".into()) }
    }
}

impl Supervisor {
    /// See `ProcessHost::stop` (Windows).
    fn stop_job(&self, owned: &Owned) -> Result<()> {
        let deadline = Instant::now() + STOP_DEADLINE;
        // A monitor-only adoption may find its job again (verified the same way as in `adopt`).
        let reopened = match &owned.job {
            Some(_) => None,
            None => reopen_session_job(&owned.record, owned.root.as_ref()),
        };
        let Some(job) = owned.job.as_ref().or(reopened.as_ref()) else {
            if owned.root.as_ref().is_some_and(|h| is_running(h.raw())) {
                bail!(
                    "Session {} has no job object, so KLIF cannot stop it safely; stop process {} yourself.",
                    owned.record.session_name,
                    owned.record.root_pid
                );
            }
            return Ok(());
        };
        let doomed_pids = job_pids(job.raw());
        let doomed: Vec<(u32, Handle)> =
            doomed_pids.iter().filter_map(|&pid| open_process(pid, PROCESS_SYNCHRONIZE).map(|h| (pid, h))).collect();
        if !doomed_pids.is_empty() && !terminate_job(job.raw(), STOP_EXIT_CODE) {
            bail!("Session {} could not be stopped: the system refused to terminate its job.", owned.record.session_name);
        }
        while !job_pids(job.raw()).is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        let wait = |h: &Handle| {
            let left = deadline.saturating_duration_since(Instant::now());
            if is_running(h.raw()) {
                unsafe {
                    let _ = WaitForSingleObject(h.raw(), left.as_millis().min(u32::MAX as u128 - 1) as u32);
                }
            }
        };
        if let Some(root) = &owned.root {
            wait(root);
        }
        // Wait until every process that was in the session has really exited...
        doomed.iter().for_each(|(_, h)| wait(h));
        // ...and the session's port is no longer held by any of them.
        let port = owned.record.port;
        while port != 0
            && Instant::now() < deadline
            && tcp_listeners().iter().any(|&(p, pid)| p == port && doomed_pids.contains(&pid))
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut left = job_pids(job.raw());
        left.extend(doomed.iter().filter(|(_, h)| is_running(h.raw())).map(|(pid, _)| *pid));
        left.sort_unstable();
        left.dedup();
        if !left.is_empty() {
            bail!(
                "Session {} did not stop within {} s; still running: pid {}.",
                owned.record.session_name,
                STOP_DEADLINE.as_secs(),
                pid_list(&left)
            );
        }
        Ok(())
    }
}
