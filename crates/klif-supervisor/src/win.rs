//! Win32 helpers for the supervisor: owned handles, process times, toolhelp trees, job queries,
//! deepest-first tree kill and the TCP listener table. Follows the verified platform probe.

use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    mem::size_of,
};

use windows::{
    core::{BOOL, HRESULT, HSTRING, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, FILETIME, HANDLE, NO_ERROR, WAIT_TIMEOUT},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::{AF_INET, AF_INET6},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
            },
            JobObjects::{
                IsProcessInJob, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, OpenJobObjectW,
                QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            },
            SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE},
            Threading::{
                GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
                TerminateProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            },
        },
    },
};

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

/// Creation time of `pid` (also works for an exited process that someone still holds open).
pub(crate) fn creation_time(pid: u32) -> Option<u64> {
    let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    handle_ctime(h.raw())
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

/// The process is alive and still the one identified by (pid, creation time).
pub(crate) fn alive(pid: u32, ctime: u64) -> bool {
    open_verified(pid, ctime, PROCESS_ACCESS_RIGHTS(0)).is_some_and(|h| is_running(h.raw()))
}

// ---------------------------------------------------------------- toolhelp

#[derive(Debug, Clone)]
pub(crate) struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub exe: String,
}

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub(crate) fn snapshot() -> Vec<Proc> {
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let snap = Handle(snap);
    let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut out = Vec::new();
    if unsafe { Process32FirstW(snap.raw(), &mut e) }.is_ok() {
        loop {
            out.push(Proc { pid: e.th32ProcessID, ppid: e.th32ParentProcessID, exe: wstr(&e.szExeFile) });
            if unsafe { Process32NextW(snap.raw(), &mut e) }.is_err() {
                break;
            }
        }
    }
    out
}

/// Descendants of `root` (created at `root_ct`) as (pid, creation time). A child must not be older
/// than its parent: that filters stale parent links left behind by PID reuse.
pub(crate) fn descendants(root: u32, root_ct: u64, procs: &[Proc]) -> Vec<(u32, Option<u64>)> {
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if p.pid != p.ppid && p.pid > 4 {
            kids.entry(p.ppid).or_default().push(p.pid);
        }
    }
    let mut seen = HashSet::from([root]);
    let mut out = Vec::new();
    let mut stack = vec![(root, root_ct)];
    while let Some((pid, ct)) = stack.pop() {
        for &k in kids.get(&pid).map(|v| v.as_slice()).unwrap_or(&[]) {
            let kct = creation_time(k);
            if kct.unwrap_or(u64::MAX) < ct || !seen.insert(k) {
                continue; // parent PID was reused: not really our child
            }
            out.push((k, kct));
            if let Some(kct) = kct {
                stack.push((k, kct));
            }
        }
    }
    out
}

/// The owned tree: every anchor whose PID still names the same process, plus its toolhelp
/// descendants, as pid -> creation time. Anchors that have exited are kept (their children may live).
pub(crate) fn owned_tree(anchors: &[(u32, u64)], procs: &[Proc]) -> Vec<(u32, Option<u64>)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &(root, root_ct) in anchors {
        // Only walk from a root whose PID still names the same process object; a reused PID would
        // otherwise hand us someone else's children.
        if creation_time(root) != Some(root_ct) {
            continue;
        }
        if seen.insert(root) {
            out.push((root, Some(root_ct)));
        }
        for (pid, ct) in descendants(root, root_ct, procs) {
            if seen.insert(pid) {
                out.push((pid, ct));
            }
        }
    }
    out
}

/// Live PIDs of the owned tree (exited/zombie entries filtered out).
pub(crate) fn live_tree_pids(anchors: &[(u32, u64)]) -> Vec<u32> {
    let procs = snapshot();
    owned_tree(anchors, &procs)
        .into_iter()
        .filter(|&(pid, ct)| match ct {
            Some(ct) => alive(pid, ct),
            // creation time unreadable (access denied): trust the parent link, report it
            None => procs.iter().any(|p| p.pid == pid),
        })
        .map(|(pid, _)| pid)
        .collect()
}

/// Terminate the owned tree deepest-first. Each PID is re-validated by creation time right before
/// TerminateProcess. Returns the PIDs actually terminated.
pub(crate) fn kill_tree(anchors: &[(u32, u64)]) -> Vec<u32> {
    let procs = snapshot();
    let tree = owned_tree(anchors, &procs);
    let set: HashSet<u32> = tree.iter().map(|t| t.0).collect();
    let parent: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let depth = |pid: u32| {
        let (mut d, mut cur) = (0usize, pid);
        while let Some(&pp) = parent.get(&cur) {
            if pp == cur || !set.contains(&pp) || d > 256 {
                break;
            }
            d += 1;
            cur = pp;
        }
        d
    };
    let mut list: Vec<(u32, usize, Option<u64>)> = tree.iter().map(|&(pid, ct)| (pid, depth(pid), ct)).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));
    let mut killed = Vec::new();
    for (pid, _, ct) in list {
        let Some(h) = open_process(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)
        else {
            continue; // already gone (or not ours to open)
        };
        if handle_ctime(h.raw()) != ct || !is_running(h.raw()) {
            continue; // PID reused, or already exited
        }
        if unsafe { TerminateProcess(h.raw(), 1) }.is_ok() {
            killed.push(pid);
            let _ = unsafe { WaitForSingleObject(h.raw(), 2000) };
        }
    }
    killed
}

/// Executable file name of `pid`: QueryFullProcessImageNameW (microseconds), toolhelp as fallback.
pub(crate) fn image_name(pid: u32) -> String {
    match pid {
        0 => return "System Idle Process".into(),
        4 => return "System".into(),
        _ => {}
    }
    if let Some(h) = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        if unsafe { QueryFullProcessImageNameW(h.raw(), PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok() {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            if let Some(name) = full.rsplit(['\\', '/']).next().filter(|n| !n.is_empty()) {
                return name.to_string();
            }
        }
    }
    snapshot().into_iter().find(|p| p.pid == pid).map(|p| p.exe).unwrap_or_else(|| "?".into())
}

// ---------------------------------------------------------------- jobs

pub(crate) fn open_job(name: &str) -> Option<Handle> {
    let n = HSTRING::from(name);
    unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, false, &n) }.ok().map(Handle)
}

/// PIDs currently in the job (the job list holds only processes that have not terminated).
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
        match r {
            Ok(()) => {
                let n = (l.NumberOfProcessIdsInList as usize).min(cap);
                let ids = unsafe { std::slice::from_raw_parts(l.ProcessIdList.as_ptr(), n) };
                return ids.iter().map(|&p| p as u32).collect();
            }
            Err(e) if e.code() == HRESULT::from_win32(ERROR_MORE_DATA.0) && cap < 1 << 16 => {
                cap = (cap * 2).max(l.NumberOfAssignedProcesses as usize + 16);
            }
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

/// (local port, owning PID) of every IPv4 and IPv6 TCP listener.
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
