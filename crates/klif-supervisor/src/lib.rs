//! klif-supervisor: starting, owning, adopting and stopping server processes. No klif-catalog dependency:
//! its input is `klif_common::launch::LaunchSpec`.
//!
//! Rules (non-negotiable):
//! - Spawn hidden (CREATE_NO_WINDOW), stdin = NUL, stdout/stderr appended to the spec's log files (NOT pipes:
//!   servers must survive KLIF), only the intended handles inherited (PROC_THREAD_ATTRIBUTE_HANDLE_LIST), the
//!   command line from `klif_common::cmdline::render` (exactly what the UI shows).
//! - Each session gets a named job `Local\KLIF-<session>` WITHOUT KILL_ON_JOB_CLOSE, so servers survive KLIF
//!   and can be adopted by name after a restart.
//! - Stop = TerminateJobObject + waits (process handles, then the port). A session without a job is never
//!   tree-killed: stop returns a sentence instead. NEVER touch a process KLIF does not own.
//! - Exit codes are captured (format faults as 0x%08X).
//!
//! Platforms: `ProcessHost` is implemented by `Supervisor` with jobs on Windows and process groups on unix
//! (basic). Frozen API: SPEC section 4 (klif-supervisor). Owner: package C.

#[cfg(windows)]
mod spawn;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

use anyhow::Result;
use klif_common::launch::LaunchSpec;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Exit code given to processes KLIF terminates.
pub const STOP_EXIT_CODE: u32 = 1;

/// What the engine persists to adopt a session after a restart (serde shape unchanged since 0.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_name: String,
    /// Windows: `Local\KLIF-<session>` (reopened by name on adopt). unix: `pgid:<n>` (the process group).
    /// None = a job-less record: adopted monitor-only, never stopped by KLIF.
    pub job_name: Option<String>,
    pub root_pid: u32,
    /// Creation time of the root process: FILETIME (100 ns since 1601) on Windows; on Linux the start time in
    /// clock ticks since boot (`/proc/<pid>/stat` field 22); on macOS microseconds since 1970 (`proc_pidinfo`);
    /// 0 where the platform gives none.
    pub root_ctime: u64,
    pub started_at: f64,
    pub out_log: PathBuf,
    pub err_log: PathBuf,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcState {
    /// Alive; all PIDs of the session (root first when alive).
    Running { pids: Vec<u32> },
    /// The root exited with this code (the rest of the session may still run).
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

/// Starting, watching and stopping sessions on one platform. Every method is cheap enough for the engine's
/// 2 Hz tick except `stop`, which waits (up to ~20 s) and runs on a stop thread.
pub trait ProcessHost: Send + Sync {
    /// Start the spec's process in a new session. Secrets go only into the child's environment block.
    fn launch(&self, spec: &LaunchSpec) -> Result<Owned>;
    /// Re-attach to a persisted session (job by name, else the root PID + creation time, monitor-only).
    fn adopt(&self, record: &SessionRecord) -> Result<Owned>;
    fn state(&self, owned: &Owned) -> ProcState;
    /// Live PIDs of the session, root first when alive.
    fn tree_pids(&self, owned: &Owned) -> Vec<u32>;
    /// Stop the session and wait until its processes and port are gone. Idempotent. Errors are sentences.
    fn stop(&self, owned: &Owned) -> Result<()>;
    /// Who listens on a TCP port right now.
    fn port_owner(&self, port: u16, ours: Option<&Owned>) -> PortOwner;
}

/// The platform's `ProcessHost` (stateless; cheap to create).
#[derive(Debug, Default, Clone, Copy)]
pub struct Supervisor {
    _private: (),
}

impl Supervisor {
    pub fn new() -> Supervisor {
        Supervisor { _private: () }
    }
}

/// A session KLIF owns. Holds the platform handles that pin it (job, root process).
pub struct Owned {
    pub record: SessionRecord,
    #[cfg(windows)]
    pub(crate) job: Option<win::Handle>,
    /// The root process handle: pins the PID against reuse and keeps the exit code readable.
    #[cfg(windows)]
    pub(crate) root: Option<win::Handle>,
    /// unix: the session's process group (= the root's pid) when KLIF may signal it; None = monitor-only.
    #[cfg(unix)]
    pub(crate) pgid: Option<i32>,
    /// unix: the root as KLIF's own child (launched in this run, not adopted): reaped and its exit code read
    /// through it.
    #[cfg(unix)]
    pub(crate) child: Option<std::sync::Mutex<std::process::Child>>,
}

impl Owned {
    /// True when the session's job is held (stop = TerminateJobObject; unix: the process group is
    /// signalled). Without it KLIF only monitors.
    pub fn has_job(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_some()
        }
        #[cfg(unix)]
        {
            self.pgid.is_some()
        }
    }
}

impl std::fmt::Debug for Owned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owned").field("record", &self.record).field("job", &self.has_job()).finish()
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

/// Executable file name of a running process, e.g. "node.exe" (None when it cannot be queried).
pub fn image_name(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        win::image_name(pid)
    }
    #[cfg(unix)]
    {
        unix::image_name(pid)
    }
}

/// A warn-level sentence when KLIF itself runs in a job that kills its children when it closes (servers
/// would die with KLIF). None when not in such a job. Meant for diag.
pub fn parent_job_warning() -> Option<String> {
    #[cfg(windows)]
    {
        win::parent_job_warning()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
