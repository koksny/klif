//! Process creation: STARTUPINFOEX with an explicit inherit list (NUL stdin, append-only log files,
//! the session job), CREATE_SUSPENDED until the process is in its named job, and a Unicode
//! environment block built from KLIF's own environment plus the plan's changes.

use std::{
    ffi::{c_void, OsStr, OsString},
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::Path,
};

use anyhow::{bail, Context, Result};
use klif_catalog::{EnvValue, LaunchPlan};
use windows::{
    core::{w, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::{GetLastError, SetHandleInformation, ERROR_ALREADY_EXISTS, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT},
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{
            CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, OPEN_EXISTING, SYNCHRONIZE,
        },
        System::{
            JobObjects::{AssignProcessToJobObject, CreateJobObjectW},
            Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, ResumeThread,
                TerminateProcess, UpdateProcThreadAttribute, CREATE_NO_WINDOW, CREATE_SUSPENDED,
                CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
                PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
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

/// An environment value: inherited from KLIF's own environment, or borrowed from the plan.
enum EnvVal<'a> {
    Os(OsString),
    Str(&'a str),
}

impl EnvVal<'_> {
    fn wide_len(&self) -> usize {
        match self {
            EnvVal::Os(v) => v.encode_wide().count(),
            EnvVal::Str(s) => s.encode_utf16().count(),
        }
    }
    fn write(&self, out: &mut Vec<u16>) {
        match self {
            EnvVal::Os(v) => out.extend(v.encode_wide()),
            EnvVal::Str(s) => out.extend(s.encode_utf16()),
        }
    }
}

/// Current environment minus `remove`, plus `set` (names case-insensitive, as on Windows), sorted
/// by upper-cased name, as a double-NUL-terminated UTF-16 block. Plan values (secrets included) are
/// only borrowed and encoded straight into the block, which is allocated once (no regrowth copies)
/// and wiped by the caller after CreateProcessW.
fn env_block(remove: &[String], set: &[(String, EnvValue)]) -> Vec<u16> {
    let mut vars: Vec<(Vec<u16>, OsString, EnvVal<'_>)> =
        std::env::vars_os().map(|(k, v)| (env_key(&k), k, EnvVal::Os(v))).collect();
    let drop_keys: Vec<Vec<u16>> = remove
        .iter()
        .chain(set.iter().map(|(k, _)| k))
        .map(|k| env_key(OsStr::new(k)))
        .collect();
    vars.retain(|(key, _, _)| !drop_keys.contains(key));
    for (k, v) in set {
        let value = match v {
            EnvValue::Plain(s) => s.as_str(),
            EnvValue::Secret(s) => s.expose(),
        };
        vars.push((env_key(OsStr::new(k)), OsString::from(k), EnvVal::Str(value)));
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

/// Quote one argument by the MSVC CRT / CommandLineToArgvW rules.
fn quote_arg(arg: &OsStr, out: &mut Vec<u16>) {
    let a: Vec<u16> = arg.encode_wide().collect();
    let needs = a.is_empty() || a.iter().any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == b'\n' as u16 || c == 0x0b || c == b'"' as u16);
    if !needs {
        out.extend_from_slice(&a);
        return;
    }
    out.push(b'"' as u16);
    let mut backslashes = 0usize;
    for &c in &a {
        if c == b'\\' as u16 {
            backslashes += 1;
        } else {
            if c == b'"' as u16 {
                out.extend(std::iter::repeat_n(b'\\' as u16, backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
    out.push(b'"' as u16);
}

pub(crate) fn command_line(exe: &Path, args: &[String]) -> Vec<u16> {
    let mut cl: Vec<u16> = Vec::new();
    cl.push(b'"' as u16);
    cl.extend(exe.as_os_str().encode_wide());
    cl.push(b'"' as u16);
    for a in args {
        cl.push(b' ' as u16);
        quote_arg(OsStr::new(a), &mut cl);
    }
    cl.push(0);
    cl
}

fn open_log(path: &Path, sa: &SECURITY_ATTRIBUTES) -> Result<Handle> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create log folder {}", dir.display()))?;
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
    .with_context(|| format!("cannot open log file {}", path.display()))?;
    Ok(Handle::new(h))
}

pub(crate) fn spawn(plan: &LaunchPlan, job_name: &str) -> Result<Spawned> {
    if !plan.exe.is_absolute() {
        bail!("launch exe must be an absolute path: {}", plan.exe.display());
    }
    if !plan.cwd.is_dir() {
        bail!("working folder does not exist: {}", plan.cwd.display());
    }
    let sa = inheritable();

    // Named, inheritable job WITHOUT KILL_ON_JOB_CLOSE (no limits at all).
    let jname = HSTRING::from(job_name);
    let job = unsafe { CreateJobObjectW(Some(&sa), &jname) }.with_context(|| format!("CreateJobObjectW {job_name}"))?;
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let job = Handle::new(job);
    if already {
        bail!("job {job_name} already exists (session name collision); not launching");
    }

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
    .context("cannot open NUL for stdin")?;
    let nul = Handle::new(nul);
    let out = open_log(&plan.out_log, &sa)?;
    let err = if plan.err_log == plan.out_log { None } else { Some(open_log(&plan.err_log, &sa)?) };
    let err_h = err.as_ref().map(|h| h.raw()).unwrap_or(out.raw());

    // Inherit exactly these handles (no duplicates allowed in the list).
    let mut inherit: Vec<HANDLE> = vec![nul.raw(), out.raw()];
    if let Some(e) = &err {
        inherit.push(e.raw());
    }
    inherit.push(job.raw());

    let app = wide_z(plan.exe.as_os_str());
    let mut cl = command_line(&plan.exe, &plan.args);
    let cwd = wide_z(plan.cwd.as_os_str());
    let mut env = env_block(&plan.env_remove, &plan.env_set);

    let mut pi = PROCESS_INFORMATION::default();
    let created = unsafe {
        let mut attr_size = 0usize;
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut attr_size);
        let mut attr_buf = vec![0u64; attr_size.div_ceil(8)];
        let attrs = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr() as *mut c_void);
        let r = InitializeProcThreadAttributeList(Some(attrs), 1, None, &mut attr_size)
            .context("InitializeProcThreadAttributeList")
            .and_then(|_| {
                let r = UpdateProcThreadAttribute(
                    attrs,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    Some(inherit.as_mut_ptr() as *const c_void),
                    inherit.len() * size_of::<HANDLE>(),
                    None,
                    None,
                )
                .context("UpdateProcThreadAttribute(HANDLE_LIST)");
                let r = r.and_then(|_| {
                    let mut si = STARTUPINFOEXW::default();
                    si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
                    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                    si.StartupInfo.hStdInput = nul.raw();
                    si.StartupInfo.hStdOutput = out.raw();
                    si.StartupInfo.hStdError = err_h;
                    si.lpAttributeList = attrs;
                    CreateProcessW(
                        PCWSTR(app.as_ptr()),
                        Some(PWSTR(cl.as_mut_ptr())),
                        None,
                        None,
                        true, // required, but limited to the HANDLE_LIST above
                        CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                        Some(env.as_ptr() as *const c_void),
                        PCWSTR(cwd.as_ptr()),
                        &si.StartupInfo,
                        &mut pi,
                    )
                    .with_context(|| format!("CreateProcessW {}", plan.exe.display()))
                });
                DeleteProcThreadAttributeList(attrs);
                r
            });
        r
    };
    wipe(&mut env);
    // The child holds its own copies now; KLIF's ends of NUL and the logs close here.
    drop((nul, out, err));
    created?;

    let process = Handle::new(pi.hProcess);
    let thread = Handle::new(pi.hThread);
    // KLIF's own job handle must not leak into unrelated children spawned later.
    let _ = unsafe { SetHandleInformation(job.raw(), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) };

    if let Err(e) = unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } {
        let _ = unsafe { TerminateProcess(process.raw(), 1) }; // still suspended: never ran
        bail!("AssignProcessToJobObject {job_name}: {e}");
    }
    if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
        let _ = unsafe { TerminateProcess(process.raw(), 1) };
        bail!("ResumeThread failed for pid {}", pi.dwProcessId);
    }
    drop(thread);
    let ctime = handle_ctime(process.raw()).context("GetProcessTimes on the new process")?;
    Ok(Spawned { pid: pi.dwProcessId, ctime, process, job })
}
