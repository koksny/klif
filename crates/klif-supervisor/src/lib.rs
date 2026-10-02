//! klif-supervisor: starting, owning, adopting and stopping server processes on Windows.
//!
//! Rules (non-negotiable):
//! - Spawn with CREATE_NO_WINDOW, stdin = NUL (a starter's trailing `pause` returns at once),
//!   stdout/stderr appended to the plan's log files (NOT pipes: servers must survive KLIF), only the
//!   intended handles inherited (PROC_THREAD_ATTRIBUTE_HANDLE_LIST), CREATE_SUSPENDED until assigned.
//! - Each session gets a named, inheritable job `Local\KLIF-<session>` WITHOUT KILL_ON_JOB_CLOSE, so
//!   servers survive KLIF and can be adopted by name after a restart.
//! - Ownership = root PID + creation time (FILETIME). Persisted in the engine's state. Old-launcher runs
//!   (RuntimeProcesses: Id + StartTimeUtcTicks) can be adopted too.
//! - Stop = TerminateJobObject when the job opens, else kill the owned tree deepest-first with a
//!   creation-time recheck per PID. NEVER touch a process KLIF does not own.
//! - Exit codes are captured (format faults as 0x%08X).

mod spawn;
mod win;

use anyhow::{bail, Result};
use klif_catalog::LaunchPlan;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows::Win32::System::{
    JobObjects::{JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK},
    Threading::PROCESS_ACCESS_RIGHTS,
};

use win::Handle;

/// .NET UTC ticks (0001-01-01) minus FILETIME (1601-01-01), in 100 ns units.
pub const DOTNET_TICKS_OFFSET: u64 = 504_911_232_000_000_000;
/// FILETIME of the Unix epoch.
const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;
/// Exit code given to processes KLIF terminates.
pub const STOP_EXIT_CODE: u32 = 1;

/// What the engine persists to adopt a session after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_name: String,
    pub job_name: Option<String>,
    pub root_pid: u32,
    /// Creation time as FILETIME (100 ns since 1601). .NET ticks = filetime + 504_911_232_000_000_000.
    pub root_ctime: u64,
    pub started_at: f64,
    pub out_log: PathBuf,
    pub err_log: PathBuf,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcState {
    /// Alive; all PIDs in the owned tree.
    Running { pids: Vec<u32> },
    /// The root exited with this code (tree may be gone too).
    Exited { code: u32 },
    /// Cannot be found any more and no exit code is known.
    Gone,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortOwner {
    Free,
    /// Held by one of our session's PIDs.
    Ours { pid: u32 },
    /// Held by someone else.
    Foreign { pid: u32, image: String },
}

pub struct Supervisor {
    _private: (),
}

pub struct Owned {
    pub record: SessionRecord,
    /// The session job, when it could be created/opened (authoritative PID list, one-call stop).
    job: Option<Handle>,
    /// The root process handle: pins the PID against reuse and keeps the exit code readable.
    root: Option<Handle>,
    /// Ownership anchors (pid, creation FILETIME) for the toolhelp fallback: the root, plus the other
    /// live old-launcher entries for a legacy adoption.
    anchors: Vec<(u32, u64)>,
}

impl Owned {
    /// True when the session's job handle is held (stop = TerminateJobObject).
    pub fn has_job(&self) -> bool {
        self.job.is_some()
    }
}

impl std::fmt::Debug for Owned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owned")
            .field("record", &self.record)
            .field("job", &self.job.is_some())
            .field("root_handle", &self.root.is_some())
            .field("anchors", &self.anchors)
            .finish()
    }
}

/// Exit code as shown to the user: NTSTATUS-style faults (high bit set) as `0x%08X`, else decimal.
pub fn format_exit_code(code: u32) -> String {
    if code & 0x8000_0000 != 0 {
        format!("0x{code:08X}")
    } else {
        code.to_string()
    }
}

/// Executable name of a running process (toolhelp), e.g. "node.exe"; "?" when not found.
pub fn image_name(pid: u32) -> String {
    win::image_name(pid)
}

/// Creation time (FILETIME) of a process, None if it cannot be opened.
pub fn creation_filetime(pid: u32) -> Option<u64> {
    win::creation_time(pid)
}

fn filetime_to_unix_s(ft: u64) -> f64 {
    (ft as f64 - UNIX_EPOCH_FILETIME as f64) / 1e7
}

impl Default for Supervisor {
    fn default() -> Self {
        Supervisor::new()
    }
}

impl Supervisor {
    pub fn new() -> Supervisor {
        Supervisor { _private: () }
    }

    /// Warn-level diagnostic if KLIF itself runs in a KILL_ON_JOB_CLOSE job (servers would die with it).
    pub fn parent_job_warning(&self) -> Option<String> {
        let flags = win::own_job_limit_flags()?; // None: not in a job at all
        let Some(flags) = flags else {
            return Some(
                "KLIF runs inside a job whose limits cannot be read; servers may not survive KLIF exiting".into(),
            );
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

    /// Start the plan's process. Secrets go only into the child's environment block.
    pub fn launch(&mut self, plan: &LaunchPlan) -> Result<Owned> {
        if plan.session_name.trim().is_empty() {
            bail!("launch plan has no session name");
        }
        let job_name = spawn::job_name_for(&plan.session_name);
        let s = spawn::spawn(plan, &job_name)?;
        let record = SessionRecord {
            session_name: plan.session_name.clone(),
            job_name: Some(job_name),
            root_pid: s.pid,
            root_ctime: s.ctime,
            started_at: filetime_to_unix_s(s.ctime),
            out_log: plan.out_log.clone(),
            err_log: plan.err_log.clone(),
            port: plan.port,
        };
        Ok(Owned { anchors: vec![(s.pid, s.ctime)], record, job: Some(s.job), root: Some(s.process) })
    }

    /// Re-attach to a persisted session if its root (PID + creation time) is still alive.
    ///
    /// The job is tried first (`OpenJobObjectW` by name): a job that still lists processes is
    /// adopted even if the root itself has exited (its server child is what matters). Otherwise
    /// the root must be alive with the recorded creation time.
    pub fn adopt(&mut self, record: &SessionRecord) -> Option<Owned> {
        let anchors = vec![(record.root_pid, record.root_ctime)];
        let root = win::open_verified(record.root_pid, record.root_ctime, PROCESS_ACCESS_RIGHTS(0))
            .filter(|h| win::is_running(h.raw()));
        if let Some(job) = record.job_name.as_deref().and_then(win::open_job) {
            let pids = win::job_pids(job.raw());
            // A live root must really be in that job; otherwise the name is not ours.
            let root_matches = root.as_ref().is_none_or(|h| win::process_in_job(h.raw(), job.raw()));
            if !pids.is_empty() && root_matches {
                return Some(Owned { record: record.clone(), job: Some(job), root, anchors });
            }
        }
        let root = root?;
        Some(Owned { record: record.clone(), job: None, root: Some(root), anchors })
    }

    /// Adopt a run started by the old PowerShell GUI (RuntimeProcesses entries).
    ///
    /// Each entry is (PID, .NET UTC ticks of its start time). Entries whose process still has that
    /// creation time are the owned anchors; their toolhelp descendants are owned too. The root is
    /// the oldest anchor that has no other anchor among its ancestors.
    pub fn adopt_legacy(&mut self, pids_with_ticks: &[(u32, i64)], out_log: Option<PathBuf>, err_log: Option<PathBuf>, port: u16) -> Option<Owned> {
        let mut anchors: Vec<(u32, u64)> = Vec::new();
        for &(pid, ticks) in pids_with_ticks {
            if pid <= 4 || ticks <= DOTNET_TICKS_OFFSET as i64 || anchors.iter().any(|a| a.0 == pid) {
                continue;
            }
            let ft = ticks as u64 - DOTNET_TICKS_OFFSET;
            if win::alive(pid, ft) {
                anchors.push((pid, ft));
            }
        }
        if anchors.is_empty() {
            return None;
        }
        let procs = win::snapshot();
        let parent: std::collections::HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
        let has_anchor_ancestor = |pid: u32, ct: u64| {
            let (mut cur, mut cur_ct, mut steps) = (pid, ct, 0);
            while let Some(&pp) = parent.get(&cur) {
                if pp == cur || pp <= 4 || steps > 256 {
                    break;
                }
                // a parent link is only real if the parent is not younger than the child
                let Some(pct) = win::creation_time(pp) else { break };
                if pct > cur_ct {
                    break;
                }
                if anchors.iter().any(|a| a.0 == pp && a.1 == pct) {
                    return true;
                }
                cur = pp;
                cur_ct = pct;
                steps += 1;
            }
            false
        };
        let (root_pid, root_ct) = anchors
            .iter()
            .copied()
            .filter(|&(p, c)| !has_anchor_ancestor(p, c))
            .min_by_key(|a| a.1)
            .unwrap_or(anchors[0]);
        // keep the root first
        anchors.retain(|a| a.0 != root_pid);
        anchors.insert(0, (root_pid, root_ct));
        let root = win::open_verified(root_pid, root_ct, PROCESS_ACCESS_RIGHTS(0))?;

        let session_name = out_log
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .and_then(|n| n.strip_suffix(".out.log").map(str::to_string))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("legacy-{root_pid}"));
        let record = SessionRecord {
            session_name,
            job_name: None,
            root_pid,
            root_ctime: root_ct,
            started_at: filetime_to_unix_s(root_ct),
            out_log: out_log.unwrap_or_default(),
            err_log: err_log.unwrap_or_default(),
            port,
        };
        Some(Owned { record, job: None, root: Some(root), anchors })
    }

    pub fn state(&self, owned: &Owned) -> ProcState {
        if let Some(root) = &owned.root {
            if let Some(code) = win::exit_code(root.raw()) {
                return ProcState::Exited { code };
            }
            if win::is_running(root.raw()) {
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

    /// Live PIDs of the owned tree: the job's process list when the job is held, else the live
    /// anchors plus their toolhelp descendants (creation-time checked). Root first when alive.
    pub fn tree_pids(&self, owned: &Owned) -> Vec<u32> {
        let mut pids = match &owned.job {
            Some(job) => win::job_pids(job.raw()),
            None => win::live_tree_pids(&owned.anchors),
        };
        if let Some(i) = pids.iter().position(|&p| p == owned.record.root_pid) {
            pids.swap(0, i);
        }
        pids
    }

    /// Stop an owned session (job terminate, or tree kill deepest-first). Idempotent.
    pub fn stop(&mut self, owned: &Owned) -> Result<()> {
        // A GPU server can take seconds to really die after TerminateJobObject (the driver frees
        // ~16 GiB, possibly waking the card from D3 first). Its PID leaves the job's active list
        // before the process object and its listening socket are gone, so wait on real handles.
        let deadline = Instant::now() + Duration::from_secs(20);
        let reopened = match &owned.job {
            Some(_) => None,
            None => owned.record.job_name.as_deref().and_then(win::open_job),
        };
        let job = owned.job.as_ref().or(reopened.as_ref());
        let doomed_pids: Vec<u32> = match job {
            Some(job) => win::job_pids(job.raw()),
            None => win::live_tree_pids(&owned.anchors),
        };
        let doomed: Vec<Handle> = doomed_pids
            .iter()
            .filter_map(|&pid| win::open_process(pid, windows::Win32::System::Threading::PROCESS_SYNCHRONIZE))
            .collect();
        let mut terminated = false;
        if let Some(job) = job {
            if win::job_pids(job.raw()).is_empty() || win::terminate_job(job.raw(), STOP_EXIT_CODE) {
                terminated = true;
                while !win::job_pids(job.raw()).is_empty() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
        if !terminated {
            win::kill_tree(&owned.anchors);
        }
        if let Some(root) = &owned.root {
            let left = deadline.saturating_duration_since(Instant::now());
            if win::is_running(root.raw()) {
                unsafe {
                    let _ = windows::Win32::System::Threading::WaitForSingleObject(root.raw(), left.as_millis() as u32);
                }
            }
        }
        // Wait until every process that was in the session has really exited...
        for h in &doomed {
            let left = deadline.saturating_duration_since(Instant::now());
            if win::is_running(h.raw()) {
                unsafe {
                    let _ = windows::Win32::System::Threading::WaitForSingleObject(h.raw(), left.as_millis() as u32);
                }
            }
        }
        // ...and the session's port is no longer held by any of them.
        while Instant::now() < deadline
            && win::tcp_listeners().iter().any(|l| l.0 == owned.record.port && doomed_pids.contains(&l.1))
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut left: Vec<u32> = match job {
            Some(job) => win::job_pids(job.raw()),
            None => win::live_tree_pids(&owned.anchors),
        };
        left.extend(
            doomed_pids
                .iter()
                .zip(&doomed)
                .filter(|(_, h)| win::is_running(h.raw()))
                .map(|(pid, _)| *pid),
        );
        left.sort_unstable();
        left.dedup();
        if !left.is_empty() {
            bail!("could not stop session {}: still running pids {:?}", owned.record.session_name, left);
        }
        Ok(())
    }

    /// Who listens on a TCP port right now (GetExtendedTcpTable, microseconds).
    pub fn port_owner(&self, port: u16, ours: Option<&Owned>) -> PortOwner {
        let mut holders: Vec<u32> = win::tcp_listeners().into_iter().filter(|l| l.0 == port).map(|l| l.1).collect();
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
        PortOwner::Foreign { pid, image: win::image_name(pid) }
    }
}
