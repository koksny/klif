//! Manual probe for klif-supervisor (an example, not a test suite). Starts any program as a KLIF session and
//! walks the whole session lifecycle against the real OS:
//!
//! launch -> wait until the port is ours -> tree_pids / port_owner / state -> drop the handle (as if KLIF
//! restarted) -> adopt by the record -> stop -> every session pid and the port are gone -> adopt again
//! fails -> a second stop is a no-op.
//!
//! ```text
//! cargo run -p klif-supervisor --example spawn_probe -- --port 47123 [--extra-port 47124] [--logs <dir>]
//!     [--cwd <dir>] [--wait <s>] [--env NAME=VALUE] [--secret NAME=VALUE] [--remove NAME] -- <program> [args...]
//! ```
//!
//! The program must listen on 127.0.0.1:<port>; `--extra-port` names a port one of its CHILD processes
//! listens on (checked to be "ours" too). A bare program name is looked up on PATH here (the supervisor
//! itself only takes absolute paths). Exit code 0 = every step passed.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use klif_common::{
    launch::{EnvVal, LaunchSpec},
    secret::Secret,
};
use klif_supervisor::{format_exit_code, image_name, parent_job_warning, PortOwner, ProcState, ProcessHost, Supervisor};

struct Opts {
    port: u16,
    extra_port: Option<u16>,
    logs: PathBuf,
    cwd: Option<PathBuf>,
    wait: Duration,
    env_set: Vec<(String, EnvVal)>,
    env_remove: Vec<String>,
    program: String,
    args: Vec<String>,
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: spawn_probe --port N [--extra-port N] [--logs DIR] [--cwd DIR] [--wait S] [--env K=V] [--secret K=V] [--remove K] -- PROGRAM [ARGS...]"
    );
    ExitCode::from(2)
}

fn parse() -> Option<Opts> {
    let mut it = std::env::args().skip(1);
    let mut o = Opts {
        port: 0,
        extra_port: None,
        logs: std::env::temp_dir().join("klif-spawn-probe"),
        cwd: None,
        wait: Duration::from_secs(15),
        env_set: Vec::new(),
        env_remove: Vec::new(),
        program: String::new(),
        args: Vec::new(),
    };
    let pair = |s: String| s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => o.port = it.next()?.parse().ok()?,
            "--extra-port" => o.extra_port = Some(it.next()?.parse().ok()?),
            "--logs" => o.logs = PathBuf::from(it.next()?),
            "--cwd" => o.cwd = Some(PathBuf::from(it.next()?)),
            "--wait" => o.wait = Duration::from_secs_f64(it.next()?.parse().ok()?),
            "--env" => {
                let (k, v) = pair(it.next()?)?;
                o.env_set.push((k, EnvVal::Plain(v)));
            }
            "--secret" => {
                let (k, v) = pair(it.next()?)?;
                o.env_set.push((k, EnvVal::Secret(Secret::new(v)?)));
            }
            "--remove" => o.env_remove.push(it.next()?),
            "--" => {
                o.program = it.next()?;
                o.args = it.collect();
                break;
            }
            _ => return None,
        }
    }
    (o.port != 0 && !o.program.is_empty()).then_some(o)
}

/// Absolute path of `program`: as given when absolute, else the first PATH entry that has it.
fn resolve(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    let names: Vec<String> = if cfg!(windows) && p.extension().is_none() {
        vec![format!("{program}.exe"), program.to_string()]
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|c| c.is_file())
}

fn names(pids: &[u32]) -> String {
    pids.iter().map(|&p| format!("{p} ({})", image_name(p).unwrap_or_else(|| "?".into()))).collect::<Vec<_>>().join(", ")
}

fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(12)..].join("\n")
}

struct Checks {
    failed: usize,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        println!("[{}] {what}", if ok { " ok " } else { "FAIL" });
        if !ok {
            self.failed += 1;
        }
    }
}

fn main() -> ExitCode {
    let Some(o) = parse() else { return usage() };
    let Some(exe) = resolve(&o.program) else {
        eprintln!("spawn_probe: {} not found", o.program);
        return ExitCode::from(2);
    };
    let cwd = o.cwd.clone().or_else(|| exe.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let session = format!("klif-probe-{stamp}-p{}", o.port);
    let spec = LaunchSpec {
        exe: exe.clone(),
        args: o.args.clone(),
        cwd,
        env_remove: o.env_remove.clone(),
        env_set: o.env_set.clone(),
        session_name: session.clone(),
        out_log: o.logs.join(format!("{session}.out.log")),
        err_log: o.logs.join(format!("{session}.err.log")),
        port: o.port,
    };
    let sup = Supervisor::new();
    let mut c = Checks { failed: 0 };
    println!("parent job warning: {:?}", parent_job_warning());
    println!("command line: {}", klif_common::cmdline::render(&exe.to_string_lossy(), &o.args));
    println!("spec: {spec:?}");

    // 1. launch
    let t0 = Instant::now();
    let owned = match sup.launch(&spec) {
        Ok(owned) => owned,
        Err(e) => {
            println!("[FAIL] launch: {e:#}");
            return ExitCode::from(1);
        }
    };
    println!("launched in {:?}: {owned:?}", t0.elapsed());
    c.check(owned.has_job(), "the session holds its job / process group");

    // 2. wait until the port is ours
    let deadline = Instant::now() + o.wait;
    let mut owner = PortOwner::Free;
    while Instant::now() < deadline {
        owner = sup.port_owner(o.port, Some(&owned));
        if matches!(owner, PortOwner::Ours { .. }) {
            break;
        }
        if let ProcState::Exited { code } = sup.state(&owned) {
            println!("[FAIL] the program exited with {} before listening", format_exit_code(code));
            println!("out log:\n{}\nerr log:\n{}", tail(&spec.out_log), tail(&spec.err_log));
            return ExitCode::from(1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("port {} after {:?}: {owner:?}", o.port, t0.elapsed());
    c.check(matches!(owner, PortOwner::Ours { .. }), "port_owner(port, ours) = Ours");
    if let Some(extra) = o.extra_port {
        let deadline = Instant::now() + o.wait;
        let mut x = PortOwner::Free;
        while Instant::now() < deadline {
            x = sup.port_owner(extra, Some(&owned));
            if matches!(x, PortOwner::Ours { .. }) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("extra port {extra}: {x:?}");
        c.check(
            matches!(x, PortOwner::Ours { pid } if pid != owned.record.root_pid),
            "port_owner(extra port, ours) = Ours by a child process",
        );
    }
    let foreign_view = sup.port_owner(o.port, None);
    println!("port {} without `ours`: {foreign_view:?}", o.port);
    c.check(matches!(foreign_view, PortOwner::Foreign { .. }), "port_owner(port, None) = Foreign");

    // 3. tree and state
    let tree = sup.tree_pids(&owned);
    println!("tree_pids: {}", names(&tree));
    c.check(tree.first() == Some(&owned.record.root_pid), "tree_pids lists the root first");
    let state = sup.state(&owned);
    println!("state: {state:?}");
    c.check(matches!(state, ProcState::Running { .. }), "state = Running");

    // 4. drop the handle (KLIF restart), adopt by the record
    let record = owned.record.clone();
    drop(owned);
    std::thread::sleep(Duration::from_millis(300));
    let alive: Vec<u32> = tree.iter().copied().filter(|&p| image_name(p).is_some()).collect();
    c.check(alive.len() == tree.len(), "every session process survives dropping the handle");
    let adopted = match sup.adopt(&record) {
        Ok(a) => a,
        Err(e) => {
            println!("[FAIL] adopt: {e:#}");
            return ExitCode::from(1);
        }
    };
    println!("adopted: {adopted:?}");
    c.check(adopted.has_job(), "adopt reopened the job / process group");
    let tree2 = sup.tree_pids(&adopted);
    println!("tree_pids after adopt: {}", names(&tree2));
    let mut a = tree.clone();
    let mut b = tree2.clone();
    a.sort_unstable();
    b.sort_unstable();
    c.check(a == b, "the adopted session has the same processes");
    let owner2 = sup.port_owner(o.port, Some(&adopted));
    c.check(matches!(owner2, PortOwner::Ours { .. }), "port_owner(port, adopted) = Ours");

    // 5. stop
    let t1 = Instant::now();
    let stopped = sup.stop(&adopted);
    println!("stop: {stopped:?} in {:?}", t1.elapsed());
    c.check(stopped.is_ok(), "stop returned Ok");
    let state = sup.state(&adopted);
    println!("state after stop: {state:?}");
    c.check(matches!(state, ProcState::Gone | ProcState::Exited { .. }), "state after stop = Gone/Exited");
    let survivors: Vec<u32> = tree.iter().copied().filter(|&p| image_name(p).is_some()).collect();
    println!("survivors: {}", names(&survivors));
    c.check(survivors.is_empty(), "every process of the session is gone");
    std::thread::sleep(Duration::from_millis(200)); // past port_owner's listener cache
    let owner3 = sup.port_owner(o.port, None);
    c.check(owner3 == PortOwner::Free, "the port is free");
    if let Some(extra) = o.extra_port {
        c.check(sup.port_owner(extra, None) == PortOwner::Free, "the extra port is free");
    }

    // 6. adopt a dead session, stop twice
    match sup.adopt(&record) {
        Ok(x) => c.check(false, &format!("adopting a stopped session must fail (got {x:?})")),
        Err(e) => c.check(true, &format!("adopting a stopped session fails: {e}")),
    }
    c.check(sup.stop(&adopted).is_ok(), "a second stop is a no-op");

    println!("out log tail:\n{}", tail(&spec.out_log));
    if c.failed == 0 {
        println!("ALL CHECKS PASSED");
        ExitCode::SUCCESS
    } else {
        println!("{} CHECK(S) FAILED", c.failed);
        ExitCode::from(1)
    }
}
