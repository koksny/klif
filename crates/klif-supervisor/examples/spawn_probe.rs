//! Scratch harness for klif-supervisor (an example, not a test suite).
//!
//! It writes two HARMLESS scripts under `<repo>/.local/supervisor-probe/` (a `.cmd` that echoes,
//! pings localhost, exits 7 and ends with `pause`; a `.ps1` that starts a child and listens on a free
//! loopback port), launches them through `Supervisor`, and only ever stops what it started itself.
//! It never starts an inference server. The foreign-port check only READS the TCP table.
//!
//! `cargo run -p klif-supervisor --example spawn_probe [-- <foreign-port>]` (default 5193, the UI dev server).

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use klif_catalog::{EnvValue, LaunchPlan};
use klif_common::{
    vm::{ModelRef, SlotId, SlotKind},
    Secret,
};
use klif_supervisor::{
    creation_filetime, format_exit_code, image_name, Owned, PortOwner, ProcState, Supervisor, DOTNET_TICKS_OFFSET,
};

const TREE_CMD: &str = r#"@echo off
echo tree.cmd start
echo KLIF_PROBE_PLAIN=%KLIF_PROBE_PLAIN%
if "%KLIF_PROBE_SECRET%"=="dummy-not-a-key" (echo secret: present and equal) else (echo secret: MISSING or different)
if defined KLIF_PROBE_REMOVED (echo removed-var: STILL PRESENT) else (echo removed-var: absent)
echo this line goes to stderr 1>&2
ping -n %1 127.0.0.1 >nul
set EXIT_CODE=7
echo tree.cmd exiting with %EXIT_CODE%
pause
exit /b %EXIT_CODE%
"#;

const LISTEN_PS1: &str = r#"param([int]$Port, [int]$Seconds = 60)
$ErrorActionPreference = 'Stop'
"listen.ps1 start pid=$PID"
$child = Start-Process -FilePath $env:ComSpec -ArgumentList '/d', '/c', "ping -n $($Seconds + 1) 127.0.0.1 >nul" -NoNewWindow -PassThru
"child cmd pid=$($child.Id)"
$l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
$l.Start()
"listening on 127.0.0.1:$Port"
[Console]::Error.WriteLine('listen.ps1 stderr line')
Start-Sleep -Seconds $Seconds
$l.Stop()
"listen.ps1 done"
exit 3
"#;

struct Checks {
    failed: usize,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        println!("  [{}] {what}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            self.failed += 1;
        }
    }
}

fn system_exe(rel: &str) -> PathBuf {
    let root = std::env::var("SystemRoot").expect("SystemRoot is set on Windows");
    Path::new(&root).join(rel)
}

fn plan(session: &str, exe: PathBuf, args: Vec<String>, cwd: &Path, logs: &Path, port: u16) -> LaunchPlan {
    LaunchPlan {
        slot: SlotId::High,
        kind: SlotKind::Llm,
        card_id: "probe".into(),
        profile_key: "probe|none|none|0".into(),
        model: ModelRef::default(),
        exe,
        args,
        cwd: cwd.to_path_buf(),
        // lower-case on purpose: Windows env names are case-insensitive
        env_remove: vec!["klif_probe_removed".into()],
        env_set: vec![
            ("KLIF_PROBE_PLAIN".into(), EnvValue::Plain("plain-ok".into())),
            ("KLIF_PROBE_SECRET".into(), EnvValue::Secret(Secret::new("dummy-not-a-key").unwrap())),
        ],
        session_name: session.into(),
        out_log: logs.join(format!("{session}.out.log")),
        err_log: logs.join(format!("{session}.err.log")),
        port,
        host: "127.0.0.1".into(),
        expected_layers: vec![],
        spec_mode: None,
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    l.local_addr().unwrap().port()
}

fn names(pids: &[u32]) -> String {
    pids.iter().map(|&p| format!("{p}:{}", image_name(p))).collect::<Vec<_>>().join(", ")
}

fn show(sup: &Supervisor, o: &Owned) -> ProcState {
    let s = sup.state(o);
    match &s {
        ProcState::Running { pids } => println!("  state: Running [{}]", names(pids)),
        ProcState::Exited { code } => println!("  state: Exited {} ({code})", format_exit_code(*code)),
        ProcState::Gone => println!("  state: Gone"),
    }
    s
}

fn wait_not_running(sup: &Supervisor, o: &Owned, max: Duration) -> ProcState {
    let t = Instant::now();
    loop {
        let s = sup.state(o);
        if !matches!(s, ProcState::Running { .. }) || t.elapsed() > max {
            return s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_port_ours(sup: &Supervisor, port: u16, o: &Owned, max: Duration) -> PortOwner {
    let t = Instant::now();
    loop {
        let p = sup.port_owner(port, Some(o));
        if matches!(p, PortOwner::Ours { .. }) || t.elapsed() > max {
            return p;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Independent liveness check (not via the supervisor): open + zero-timeout wait.
fn running(pid: u32) -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    let Ok(h) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else { return false };
    let r = unsafe { WaitForSingleObject(h, 0) };
    unsafe {
        let _ = CloseHandle(h);
    }
    r == WAIT_TIMEOUT
}

fn read(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).to_string()
}

fn timed_port_owner(sup: &Supervisor, port: u16, ours: Option<&Owned>) -> PortOwner {
    let t = Instant::now();
    let r = sup.port_owner(port, ours);
    println!("  port_owner({port}) = {r:?} in {} us", t.elapsed().as_micros());
    r
}

fn main() {
    let foreign_port: u16 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5193);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
    let dir = repo.join(".local").join("supervisor-probe");
    std::fs::create_dir_all(&dir).unwrap();
    let tree_cmd = dir.join("tree.cmd");
    let listen_ps1 = dir.join("listen.ps1");
    std::fs::write(&tree_cmd, TREE_CMD.replace('\n', "\r\n")).unwrap();
    std::fs::write(&listen_ps1, LISTEN_PS1.replace('\n', "\r\n")).unwrap();
    let cmd = system_exe(r"System32\cmd.exe");
    let ps = system_exe(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let stamp = klif_common::now_s() as u64;
    let base = format!("klif-probe-{stamp}");
    let logs = dir.join("logs").join(&base);
    // a variable KLIF itself has, which the plan removes from the child
    std::env::set_var("KLIF_PROBE_REMOVED", "should-not-reach-child");

    let mut c = Checks { failed: 0 };
    let mut sup = Supervisor::new();

    println!("== F. parent job / exit-code formatting");
    println!("  parent_job_warning: {:?}", sup.parent_job_warning());
    c.check(format_exit_code(0xC000_0409) == "0xC0000409", "0xC0000409 formatted as hex");
    c.check(format_exit_code(7) == "7", "7 formatted as decimal");
    c.check(format_exit_code(u32::MAX) == "0xFFFFFFFF", "-1 formatted as 0xFFFFFFFF");

    // ------------------------------------------------------------------ A
    println!("\n== A. tree.cmd: env block, append-only logs, trailing pause, exit code");
    std::fs::create_dir_all(&logs).unwrap();
    let pa = plan(&format!("{base}-a"), cmd.clone(), vec!["/d".into(), "/c".into(), tree_cmd.display().to_string(), "3".into()], &dir, &logs, 0);
    std::fs::write(&pa.out_log, "PRE-EXISTING LINE\r\n").unwrap();
    let t0 = Instant::now();
    let a = sup.launch(&pa).expect("launch A");
    println!("  record: {:?}", a.record);
    c.check(a.has_job() && a.record.job_name.as_deref() == Some(format!("Local\\KLIF-{base}-a").as_str()), "job Local\\KLIF-<session> held");
    std::thread::sleep(Duration::from_millis(500));
    let s = show(&sup, &a);
    c.check(matches!(&s, ProcState::Running { pids } if pids.len() >= 2), "running tree has the root + children");
    let s = wait_not_running(&sup, &a, Duration::from_secs(20));
    let el = t0.elapsed();
    println!("  finished after {} ms", el.as_millis());
    show(&sup, &a);
    c.check(s == ProcState::Exited { code: 7 }, "exit code 7 captured");
    c.check(el < Duration::from_secs(8), "trailing pause returned at once (stdin = NUL)");
    let out = read(&pa.out_log);
    let err = read(&pa.err_log);
    println!("  out.log:\n    {}", out.trim_end().replace("\r\n", "\n    "));
    println!("  err.log:\n    {}", err.trim_end().replace("\r\n", "\n    "));
    c.check(out.starts_with("PRE-EXISTING LINE"), "out log appended (pre-existing line kept)");
    c.check(out.contains("KLIF_PROBE_PLAIN=plain-ok"), "plain env var set");
    c.check(out.contains("secret: present and equal"), "secret env var reached the child (value not printed)");
    c.check(out.contains("removed-var: absent"), "env_remove applied case-insensitively");
    c.check(out.contains("Press any key"), "pause ran");
    c.check(err.contains("this line goes to stderr"), "stderr to err log");
    c.check(sup.stop(&a).is_ok(), "stop() on an exited session is Ok (idempotent)");

    // ------------------------------------------------------------------ B
    println!("\n== B. listen.ps1 tree: port_owner Ours, simulated restart, adopt by job name, stop via job");
    let port = free_port();
    let logs_b = logs.join("nested").join("b"); // does not exist yet: launch must create it
    let pb = plan(
        &format!("{base}-b"),
        ps.clone(),
        vec!["-NoProfile".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), listen_ps1.display().to_string(), "-Port".into(), port.to_string(), "-Seconds".into(), "60".into()],
        &dir,
        &logs_b,
        port,
    );
    let b = sup.launch(&pb).expect("launch B");
    c.check(logs_b.is_dir(), "log parent folder created");
    let po = wait_port_ours(&sup, port, &b, Duration::from_secs(25));
    println!("  port_owner with ours = {po:?}");
    c.check(matches!(po, PortOwner::Ours { .. }), "port_owner = Ours");
    let pids_b = match show(&sup, &b) {
        ProcState::Running { pids } => pids,
        _ => vec![],
    };
    c.check(pids_b.len() >= 4, "job lists powershell + conhost + cmd + ping");
    let po = timed_port_owner(&sup, port, None);
    c.check(matches!(&po, PortOwner::Foreign { image, .. } if image.eq_ignore_ascii_case("powershell.exe")), "without ours the same port is Foreign(powershell.exe)");
    let rec_b = b.record.clone();
    drop(b); // simulated KLIF restart: our job + process handles are closed
    std::thread::sleep(Duration::from_millis(300));
    let b2 = sup.adopt(&rec_b);
    c.check(b2.as_ref().is_some_and(|o| o.has_job()), "adopt() reopened the job by name");
    let b2 = b2.expect("adopt B");
    let pids_b2 = match show(&sup, &b2) {
        ProcState::Running { pids } => pids,
        _ => vec![],
    };
    let mut x = pids_b.clone();
    let mut y = pids_b2.clone();
    x.sort();
    y.sort();
    c.check(x == y, "adopted tree == original tree");
    c.check(matches!(timed_port_owner(&sup, port, Some(&b2)), PortOwner::Ours { .. }), "port_owner = Ours after adoption");
    let t = Instant::now();
    let r = sup.stop(&b2);
    println!("  stop() -> {r:?} in {} ms", t.elapsed().as_millis());
    c.check(r.is_ok(), "stop() via TerminateJobObject");
    let s = show(&sup, &b2);
    c.check(matches!(s, ProcState::Exited { .. } | ProcState::Gone), "state after stop is Exited/Gone");
    c.check(timed_port_owner(&sup, port, None) == PortOwner::Free, "port free after stop");
    c.check(!pids_b.is_empty() && pids_b.iter().all(|&p| !running(p)), "every PID of the tree is gone");
    c.check(sup.stop(&b2).is_ok(), "second stop() is Ok (idempotent)");
    c.check(sup.adopt(&rec_b).is_none(), "adopt() of the stopped session returns None");
    println!("  out.log: {:?}", read(&pb.out_log).lines().collect::<Vec<_>>());
    c.check(read(&pb.err_log).contains("listen.ps1 stderr line"), "powershell stderr in err log");

    // ------------------------------------------------------------------ C
    println!("\n== C. adopt by PID + creation time (no job): toolhelp tree + deepest-first kill");
    let port = free_port();
    let pc = plan(
        &format!("{base}-c"),
        ps.clone(),
        vec!["-NoProfile".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), listen_ps1.display().to_string(), "-Port".into(), port.to_string(), "-Seconds".into(), "60".into()],
        &dir,
        &logs,
        port,
    );
    let cc = sup.launch(&pc).expect("launch C");
    let _ = wait_port_ours(&sup, port, &cc, Duration::from_secs(25));
    let job_tree = sup.tree_pids(&cc);
    println!("  job tree: [{}]", names(&job_tree));
    let mut rec_c = cc.record.clone();
    rec_c.job_name = None; // force the PID + creation-time path
    drop(cc);
    let mut bad = rec_c.clone();
    bad.root_ctime += 1;
    c.check(sup.adopt(&bad).is_none(), "wrong creation time -> not adopted");
    let c2 = sup.adopt(&rec_c).expect("adopt C");
    c.check(!c2.has_job(), "adopted without a job");
    let th_tree = sup.tree_pids(&c2);
    println!("  toolhelp tree: [{}]", names(&th_tree));
    let (mut x, mut y) = (job_tree.clone(), th_tree.clone());
    x.sort();
    y.sort();
    c.check(x == y, "toolhelp tree == job tree");
    c.check(matches!(timed_port_owner(&sup, port, Some(&c2)), PortOwner::Ours { .. }), "port_owner = Ours via toolhelp tree");
    let t = Instant::now();
    let r = sup.stop(&c2);
    println!("  stop() -> {r:?} in {} ms", t.elapsed().as_millis());
    c.check(r.is_ok(), "stop() via kill_tree");
    let s = show(&sup, &c2);
    c.check(s == ProcState::Exited { code: 1 }, "root exit code 1 after tree kill");
    c.check(timed_port_owner(&sup, port, None) == PortOwner::Free, "port free after tree kill");
    c.check(!job_tree.is_empty() && job_tree.iter().all(|&p| !running(p)), "every PID of the tree is gone");

    // ------------------------------------------------------------------ D
    println!("\n== D. adopt_legacy (RuntimeProcesses: Id + StartTimeUtcTicks)");
    let pd = plan(&format!("{base}-d"), cmd.clone(), vec!["/d".into(), "/c".into(), tree_cmd.display().to_string(), "61".into()], &dir, &logs, 0);
    let d = sup.launch(&pd).expect("launch D");
    std::thread::sleep(Duration::from_millis(800));
    let d_tree = sup.tree_pids(&d);
    println!("  job tree: [{}]", names(&d_tree));
    let root = d.record.root_pid;
    let root_ticks = (d.record.root_ctime + DOTNET_TICKS_OFFSET) as i64;
    // the old GUI stored the whole tree; put a child first so root selection is exercised
    let mut entries: Vec<(u32, i64)> = d_tree
        .iter()
        .filter(|&&p| p != root)
        .filter_map(|&p| creation_filetime(p).map(|ft| (p, (ft + DOTNET_TICKS_OFFSET) as i64)))
        .collect();
    entries.push((root, root_ticks));
    entries.push((999_999, root_ticks)); // a dead/foreign entry is ignored
    drop(d);
    c.check(sup.adopt_legacy(&[(root, root_ticks + 1)], None, None, 0).is_none(), "wrong ticks -> not adopted");
    let d2 = sup.adopt_legacy(&entries, Some(pd.out_log.clone()), Some(pd.err_log.clone()), 0).expect("adopt_legacy");
    println!("  record: {:?}", d2.record);
    c.check(d2.record.root_pid == root, "root = the anchor with no anchor ancestor");
    c.check(d2.record.session_name == format!("{base}-d"), "session name from the out log name");
    c.check((klif_common::now_s() - d2.record.started_at).abs() < 30.0, "started_at from the creation time");
    let s = show(&sup, &d2);
    c.check(matches!(&s, ProcState::Running { pids } if pids.len() == d_tree.len()), "legacy tree == job tree size");
    let r = sup.stop(&d2);
    println!("  stop() -> {r:?}");
    c.check(r.is_ok(), "stop() legacy via kill_tree");
    c.check(show(&sup, &d2) == ProcState::Exited { code: 1 }, "root exit code 1");
    c.check(!d_tree.is_empty() && d_tree.iter().all(|&p| !running(p)), "every PID of the tree is gone");

    // ------------------------------------------------------------------ E
    println!("\n== E. foreign port (read-only; never stopped)");
    let po = timed_port_owner(&sup, foreign_port, None);
    c.check(matches!(&po, PortOwner::Foreign { .. }), "dev-server port is Foreign without ours");
    let po2 = timed_port_owner(&sup, foreign_port, Some(&d2));
    c.check(po2 == po, "dev-server port stays Foreign with an unrelated owned session");
    c.check(timed_port_owner(&sup, free_port(), None) == PortOwner::Free, "unused port is Free");

    println!("\n{} check(s) failed", c.failed);
    std::process::exit(if c.failed == 0 { 0 } else { 1 });
}
