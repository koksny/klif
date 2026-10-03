//! Process creation (Windows): the child is born inside its session job.
//!
//! One `CreateProcessW` call with `STARTUPINFOEXW` and ONE attribute list carrying two attributes:
//! - `PROC_THREAD_ATTRIBUTE_JOB_LIST` = [the session job]: the process is a member of the job from its first
//!   instruction (no suspended start, no resume, no separate job assignment, no window in which it could
//!   spawn something outside the job).
//! - `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` = exactly the handles the child inherits: NUL as stdin, the
//!   append-only stdout/stderr log files, and a query-only duplicate of the job handle. The duplicate is
//!   what keeps the job's NAME alive after KLIF exits (a named kernel object loses its name when its last
//!   handle closes), so a restarted KLIF can reopen the job by name; it grants the child nothing but
//!   reading its own job's accounting.
//!
//! Flags: `CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT`. The command line is
//! exactly `klif_common::cmdline::render(exe, args)` (what the UI and `klif-cli plan` show). The environment
//! block is KLIF's own environment minus `env_remove`, plus `env_set`; secret values are only borrowed from
//! the spec, encoded straight into the block and the block is wiped after the call.
//!
//! The job has no limits at all (in particular no KILL_ON_JOB_CLOSE): servers outlive KLIF and are adopted
//! by job name after a restart.

use std::{
    ffi::{c_void, OsStr, OsString},
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::Path,
};

use anyhow::{bail, Context, Result};
use klif_common::launch::{EnvVal, LaunchSpec};
use windows::{
    core::{w, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            DuplicateHandle, GetLastError, SetLastError, DUPLICATE_HANDLE_OPTIONS, ERROR_ALREADY_EXISTS, HANDLE,
            WIN32_ERROR,
        },
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{
            CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, OPEN_EXISTING, SYNCHRONIZE,
        },
        System::{
            JobObjects::CreateJobObjectW,
            SystemServices::JOB_OBJECT_QUERY,
            Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, InitializeProcThreadAttributeList,
                UpdateProcThreadAttribute, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
                LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            },
        },
    },
};

use crate::win::{handle_ctime, Handle};

pub(crate) struct Spawned {
    pub pid: u32,
    pub ctime: u64,
    pub process: Handle,
    pub job: Handle,
}

/// `Local\KLIF-<session>`; characters a kernel object name cannot carry are replaced.
pub(crate) fn job_name_for(session: &str) -> String {
    let s: String = session
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .take(200)
        .collect();
    format!("Local\\KLIF-{s}")
}

fn wide_z(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// Security attributes for handles listed in PROC_THREAD_ATTRIBUTE_HANDLE_LIST (they must be inheritable;
/// the list is what limits inheritance to exactly those handles).
fn inheritable() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    }
}

/// Upper-cased name used for case-insensitive matching and block ordering.
fn env_key(name: &OsStr) -> Vec<u16> {
    name.to_string_lossy().to_uppercase().encode_utf16().collect()
}

/// An environment value: inherited from KLIF's own environment, or borrowed from the spec.
enum BlockVal<'a> {
    Os(OsString),
    Str(&'a str),
}

impl BlockVal<'_> {
    fn wide_len(&self) -> usize {
        match self {
            BlockVal::Os(v) => v.encode_wide().count(),
            BlockVal::Str(s) => s.encode_utf16().count(),
        }
    }
    fn write(&self, out: &mut Vec<u16>) {
        match self {
            BlockVal::Os(v) => out.extend(v.encode_wide()),
            BlockVal::Str(s) => out.extend(s.encode_utf16()),
        }
    }
}

/// Current environment minus `remove`, plus `set` (names case-insensitive, as on Windows; a later `set`
/// entry wins over an earlier one), sorted by upper-cased name, as a double-NUL-terminated UTF-16 block.
/// Spec values (secrets included) are only borrowed and encoded straight into the block, which is allocated
/// once (no regrowth copies) and wiped by the caller after CreateProcessW.
fn env_block(remove: &[String], set: &[(String, EnvVal)]) -> Vec<u16> {
    let mut vars: Vec<(Vec<u16>, OsString, BlockVal<'_>)> =
        std::env::vars_os().map(|(k, v)| (env_key(&k), k, BlockVal::Os(v))).collect();
    let drop_keys: Vec<Vec<u16>> = remove
        .iter()
        .chain(set.iter().map(|(k, _)| k))
        .map(|k| env_key(OsStr::new(k)))
        .collect();
    vars.retain(|(key, _, _)| !drop_keys.contains(key));
    for (k, v) in set {
        let key = env_key(OsStr::new(k));
        let entry = (key, OsString::from(k), BlockVal::Str(v.expose()));
        match vars.iter_mut().find(|(existing, _, _)| *existing == entry.0) {
            Some(slot) => *slot = entry,
            None => vars.push(entry),
        }
    }
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    let cap: usize = vars.iter().map(|(_, k, v)| k.encode_wide().count() + 1 + v.wide_len() + 1).sum::<usize>() + 2;
    let mut block = Vec::with_capacity(cap);
    for (_, k, v) in &vars {
        block.extend(k.encode_wide());
        block.push(b'=' as u16);
        v.write(&mut block);
        block.push(0);
    }
    if vars.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

fn wipe(block: &mut [u16]) {
    for c in block.iter_mut() {
        unsafe { std::ptr::write_volatile(c, 0) };
    }
}

/// The NUL-terminated UTF-16 command line: exactly `klif_common::cmdline::render(exe, args)`, the string
/// the UI and `klif-cli plan` show.
pub(crate) fn command_line(exe: &Path, args: &[String]) -> Vec<u16> {
    let line = klif_common::cmdline::render(&exe.to_string_lossy(), args);
    line.encode_utf16().chain([0]).collect()
}

fn open_log(path: &Path, sa: &SECURITY_ATTRIBUTES) -> Result<Handle> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("Cannot create the log folder {}.", dir.display()))?;
    }
    let h = unsafe {
        CreateFileW(
            &HSTRING::from(path),
            FILE_APPEND_DATA.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            Some(sa),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| anyhow::anyhow!("Cannot open the log file {}: {}", path.display(), sentence(&e)))?;
    Ok(Handle::new(h))
}

/// The system message of a Win32 error without the HRESULT suffix, as a sentence fragment.
fn sentence(e: &windows::core::Error) -> String {
    let m = e.message();
    let m = m.trim().trim_end_matches('.');
    if m.is_empty() {
        format!("error 0x{:08X}", e.code().0 as u32)
    } else {
        format!("{m}.")
    }
}

/// An initialised attribute list; deleted on drop. The VALUES it points at (handle arrays) must outlive
/// every use of the list, which the caller guarantees by keeping them on its stack until CreateProcessW returns.
struct AttrList {
    _buf: Vec<u64>,
    ptr: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttrList {
    fn new(count: u32) -> Result<AttrList> {
        let mut size = 0usize;
        // The first call only reports the size (and "fails" with ERROR_INSUFFICIENT_BUFFER).
        let _ = unsafe { InitializeProcThreadAttributeList(None, count, None, &mut size) };
        if size == 0 {
            bail!("Cannot size the process attribute list.");
        }
        let mut buf = vec![0u64; size.div_ceil(size_of::<u64>())];
        let ptr = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr() as *mut c_void);
        unsafe { InitializeProcThreadAttributeList(Some(ptr), count, None, &mut size) }
            .map_err(|e| anyhow::anyhow!("Cannot initialise the process attribute list: {}", sentence(&e)))?;
        Ok(AttrList { _buf: buf, ptr })
    }

    /// Adds one attribute whose value is an array of handles.
    ///
    /// SAFETY: `handles` must stay alive and unmoved until the list is no longer used.
    unsafe fn handles(&mut self, attribute: u32, handles: &[HANDLE], what: &str) -> Result<()> {
        unsafe {
            UpdateProcThreadAttribute(
                self.ptr,
                0,
                attribute as usize,
                Some(handles.as_ptr() as *const c_void),
                std::mem::size_of_val(handles),
                None,
                None,
            )
        }
        .map_err(|e| anyhow::anyhow!("Cannot set the {what} process attribute: {}", sentence(&e)))
    }
}

impl Drop for AttrList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.ptr) };
    }
}

/// A duplicate of `h` in this process with only `access`, inheritable.
fn inheritable_duplicate(h: HANDLE, access: u32) -> Result<Handle> {
    let mut dup = HANDLE::default();
    let me = unsafe { GetCurrentProcess() };
    unsafe { DuplicateHandle(me, h, me, &mut dup, access, true, DUPLICATE_HANDLE_OPTIONS(0)) }
        .map_err(|e| anyhow::anyhow!("Cannot duplicate the session job handle: {}", sentence(&e)))?;
    Ok(Handle::new(dup))
}

pub(crate) fn spawn(spec: &LaunchSpec, job_name: &str) -> Result<Spawned> {
    if !spec.exe.is_absolute() {
        bail!("The program path must be absolute: {}.", spec.exe.display());
    }
    if !spec.cwd.is_dir() {
        bail!("The working folder does not exist: {}.", spec.cwd.display());
    }

    // The session job: named, NOT inheritable (PROC_THREAD_ATTRIBUTE_JOB_LIST does not need inheritance),
    // no limits (no KILL_ON_JOB_CLOSE).
    let jname = HSTRING::from(job_name);
    let job = unsafe {
        SetLastError(WIN32_ERROR(0));
        CreateJobObjectW(None, &jname)
    }
    .map_err(|e| anyhow::anyhow!("Cannot create the session job {job_name}: {}", sentence(&e)))?;
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let job = Handle::new(job);
    if already {
        bail!("A session named {} already exists (job {job_name}); not launching a second one.", spec.session_name);
    }
    // What the child inherits of the job: a query-only handle that keeps the name alive (module docs).
    let anchor = inheritable_duplicate(job.raw(), JOB_OBJECT_QUERY)?;

    let sa = inheritable();
    let nul = unsafe {
        CreateFileW(
            w!("NUL"),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&sa),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| anyhow::anyhow!("Cannot open NUL for the server's input: {}", sentence(&e)))?;
    let nul = Handle::new(nul);
    let out = open_log(&spec.out_log, &sa)?;
    let err = if spec.err_log == spec.out_log { None } else { Some(open_log(&spec.err_log, &sa)?) };
    let err_h = err.as_ref().map(|h| h.raw()).unwrap_or(out.raw());

    // Exactly these handles are inherited (no duplicates allowed in the list).
    let mut inherit: Vec<HANDLE> = vec![nul.raw(), out.raw()];
    if let Some(e) = &err {
        inherit.push(e.raw());
    }
    inherit.push(anchor.raw());
    let jobs: [HANDLE; 1] = [job.raw()];

    let app = wide_z(spec.exe.as_os_str());
    let mut cl = command_line(&spec.exe, &spec.args);
    let cwd = wide_z(spec.cwd.as_os_str());
    let mut env = env_block(&spec.env_remove, &spec.env_set);

    let mut pi = PROCESS_INFORMATION::default();
    let created = (|| -> Result<()> {
        let mut attrs = AttrList::new(2)?;
        // SAFETY: `jobs` and `inherit` live on this stack frame until after CreateProcessW below.
        unsafe {
            attrs.handles(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs, "job list")?;
            attrs.handles(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherit, "handle list")?;
        }
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = nul.raw();
        si.StartupInfo.hStdOutput = out.raw();
        si.StartupInfo.hStdError = err_h;
        si.lpAttributeList = attrs.ptr;
        unsafe {
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cl.as_mut_ptr())),
                None,
                None,
                true, // required for the handle list, and limited to it
                CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                Some(env.as_ptr() as *const c_void),
                PCWSTR(cwd.as_ptr()),
                &si.StartupInfo,
                &mut pi,
            )
        }
        .map_err(|e| {
            let mut msg = format!("Cannot start {}: {}", spec.exe.display(), sentence(&e));
            if let Some(w) = crate::parent_job_warning() {
                msg.push_str(&format!(" ({w}.)"));
            }
            anyhow::anyhow!(msg)
        })
    })();
    wipe(&mut env);
    // The child holds its own copies now; KLIF's ends of NUL, the logs and the anchor close here.
    drop((nul, out, err, anchor));
    created?;

    let process = Handle::new(pi.hProcess);
    drop(Handle::new(pi.hThread));
    let Some(ctime) = handle_ctime(process.raw()) else {
        // Without its creation time the session could never be adopted safely: do not leave it running.
        crate::win::terminate_job(job.raw(), crate::STOP_EXIT_CODE);
        bail!("Cannot read the creation time of the new server (pid {}); it was stopped again.", pi.dwProcessId);
    };
    Ok(Spawned { pid: pi.dwProcessId, ctime, process, job })
}
