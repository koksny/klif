//! status, diag, plan, select, launch, stop, restart, dismiss, systems, nodes, serve.

use crate::args::Args;
use crate::conn::{self, Conn};
use crate::out::{note, num, refused, table, val, CliError, CliResult, Out};
use crate::outputs::*;
use crate::resolve::{resolve, resolve_system};
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{Action, CommandView, LlmClass, Phase, System, SystemId, SystemKind, SystemStatus, ViewModel};
use klif_core::wire::{status_of, StatusJson, StatusSystem};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// Longest `--wait` for a launch.
const LAUNCH_WAIT: Duration = Duration::from_secs(15 * 60);
/// Longest wait for a stop.
const STOP_WAIT: Duration = Duration::from_secs(60);

pub fn find<'a>(vm: &'a ViewModel, id: &SystemId) -> Option<&'a System> {
    vm.systems.iter().find(|s| &s.id == id)
}

/// The System holds a session (it is starting, running or stopping).
pub fn holds(s: &System) -> bool {
    !s.external && (s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Online | SystemStatus::Busy | SystemStatus::Stopping))
}

pub fn act(conn: &Conn, action: Action) -> CliResult {
    conn.link().act(action).map_err(refused)
}

fn status_word(s: SystemStatus) -> &'static str {
    s.as_str()
}

fn sys_line(s: &System) -> String {
    let mut line = format!("{} ({}): {}", s.label, s.id, status_word(s.status));
    if let Some(r) = &s.reason {
        line.push_str(&format!(" - {r}"));
    }
    if let Some(e) = &s.endpoint {
        if matches!(s.status, SystemStatus::Online | SystemStatus::Busy) {
            line.push_str(&format!(" at {e}"));
        }
    }
    line
}

pub fn fault_error(s: &System) -> CliError {
    let f = s.session.as_ref().and_then(|x| x.fault.as_ref());
    let title = f.map(|f| f.title.clone()).or_else(|| s.reason.clone()).unwrap_or_else(|| "it faulted".into());
    let tail: Vec<String> = f.map(|f| f.log_tail.iter().rev().take(10).rev().cloned().collect()).unwrap_or_default();
    let mut msg = format!("{} failed: {title}", s.label);
    if !tail.is_empty() {
        msg.push_str(&format!("\n  | {}", tail.join("\n  | ")));
    }
    CliError::new("fault", msg)
}

/// Poll until the System is live. Fault -> code "fault"; timeout -> code "timeout" (it keeps loading).
/// `restarted_from` (a restart): the old session's uptime; until a new session shows (lower uptime, or a
/// starting / stopping status), the old one being online does not count.
pub fn wait_live(conn: &Conn, id: &SystemId, timeout: Duration, restarted_from: Option<f64>) -> CliResult<System> {
    let t0 = Instant::now();
    let mut seen_start = false;
    let mut new_session = restarted_from.is_none();
    let mut last_note = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let vm = conn.snapshot(Some(id))?;
        let s = find(&vm, id).ok_or_else(|| CliError::new("not_found", format!("System {id} disappeared.")))?;
        let phase = s.session.as_ref().map(|x| x.phase);
        if let (Some(old), Some(sess)) = (restarted_from, s.session.as_ref()) {
            if sess.uptime_s + 0.25 < old + t0.elapsed().as_secs_f64() - 1.0 || matches!(sess.phase, Phase::Starting | Phase::Loading | Phase::Stopping) {
                new_session = true;
            }
        }
        if matches!(s.status, SystemStatus::Starting | SystemStatus::Stopping) {
            new_session = true;
        }
        match s.status {
            SystemStatus::Online | SystemStatus::Busy if !new_session => {}
            SystemStatus::Online | SystemStatus::Busy if s.external || phase.is_none_or(|p| p == Phase::Live) => return Ok(s.clone()),
            SystemStatus::Fault => return Err(fault_error(s)),
            SystemStatus::Starting | SystemStatus::Stopping => seen_start = true,
            SystemStatus::Offline | SystemStatus::Invalid | SystemStatus::NotSet if seen_start || t0.elapsed() > Duration::from_secs(10) => {
                return Err(CliError::new(
                    "stopped",
                    format!("{} stopped before it was ready{}", s.label, s.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_else(|| ".".into())),
                ));
            }
            _ => {}
        }
        if t0.elapsed() > timeout {
            return Err(CliError::new(
                "timeout",
                format!("{} was not ready after {} minutes; it keeps loading.", s.label, timeout.as_secs() / 60),
            ));
        }
        if last_note.elapsed() >= Duration::from_secs(5) {
            last_note = Instant::now();
            let frac = s.session.as_ref().and_then(|x| x.loading.as_ref()).map(|l| format!(" {:.0}%", l.fraction * 100.0)).unwrap_or_default();
            note(format!("  {} is {}{frac} ({:.0} s)", s.label, status_word(s.status), t0.elapsed().as_secs_f64()));
        }
    }
}

/// After a launch without --wait: until the engine shows the session (the spawn is recorded) or a fault.
fn wait_started(conn: &Conn, id: &SystemId) -> CliResult<System> {
    let t0 = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let vm = conn.snapshot(Some(id))?;
        let s = find(&vm, id).ok_or_else(|| CliError::new("not_found", format!("System {id} disappeared.")))?;
        if s.status == SystemStatus::Fault {
            return Err(fault_error(s));
        }
        if s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Online | SystemStatus::Busy) || t0.elapsed() > Duration::from_secs(5) {
            return Ok(s.clone());
        }
    }
}

/// Poll until none of `ids` holds a session.
fn wait_stopped(conn: &Conn, ids: &[SystemId]) -> CliResult<ViewModel> {
    let t0 = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let vm = conn.snapshot(None)?;
        let left: Vec<&System> = ids.iter().filter_map(|id| find(&vm, id)).filter(|s| holds(s) && s.status != SystemStatus::Fault).collect();
        if left.is_empty() {
            return Ok(vm);
        }
        if t0.elapsed() > STOP_WAIT {
            let names: Vec<String> = left.iter().map(|s| s.label.clone()).collect();
            return Err(CliError::new("timeout", format!("Still stopping after {} s: {}.", STOP_WAIT.as_secs(), names.join(", "))));
        }
    }
}

// ------------------------------------------------------------------------------------------ status

fn human_status(sj: &StatusJson, conn: &Conn, vm: &ViewModel) -> String {
    let mut out = String::new();
    out.push_str(&format!("KLIF {} · engine: {}\n", klif_core::klif_common::KLIF_VERSION, conn.describe()));
    if sj.systems.is_empty() {
        out.push_str("\nNo Systems yet (klif-cli systems add --kind llm).\n");
    } else {
        let mut rows = vec![["", "ID", "LABEL", "KIND", "STATUS", "PRESET", "MODEL", "ENDPOINT", "TOK/S", "VRAM"].map(String::from).to_vec()];
        for s in &sj.systems {
            rows.push(vec![
                if sj.selected.as_ref() == Some(&s.id) { "*".into() } else { "".into() },
                s.id.to_string(),
                s.label.clone(),
                s.kind.as_str().to_string(),
                s.status.as_str().to_string(),
                s.preset.clone().unwrap_or_else(|| "-".into()),
                s.model.clone().unwrap_or_else(|| "-".into()),
                s.base_url.clone().unwrap_or_else(|| "-".into()),
                num(s.decode_tps, 1),
                s.vram_gib.map(|g| format!("{g:.2} GiB")).unwrap_or_else(|| "-".into()),
            ]);
        }
        out.push('\n');
        out.push_str(&table(&rows));
        let notes: Vec<String> = sj
            .systems
            .iter()
            .filter_map(|s| s.fault.as_ref().or(s.reason.as_ref()).map(|r| format!("  {}: {r}", s.id)))
            .collect();
        if !notes.is_empty() {
            out.push_str(&notes.join("\n"));
            out.push('\n');
        }
    }
    if !sj.gpus.is_empty() {
        out.push_str("\nGPUs\n");
        let rows: Vec<Vec<String>> =
            sj.gpus.iter().map(|g| vec![format!("  {}", g.id), g.name.clone(), format!("{:.2} / {:.2} GiB", g.used_gib, g.total_gib)]).collect();
        out.push_str(&table(&rows));
    }
    if !sj.nodes.is_empty() {
        out.push_str("\nNodes\n");
        let rows: Vec<Vec<String>> = sj
            .nodes
            .iter()
            .map(|n| vec![format!("  {}", n.id), val(&n.state).as_str().unwrap_or("").to_string(), n.latency_ms.map(|l| format!("{l:.0} ms")).unwrap_or_default()])
            .collect();
        out.push_str(&table(&rows));
    }
    let errors: Vec<String> = vm.config.issues.iter().filter(|i| i.is_error()).map(|i| format!("  {}", i.text)).collect();
    if !errors.is_empty() {
        out.push_str("\nklif.toml\n");
        out.push_str(&errors.join("\n"));
        out.push('\n');
    }
    out
}

fn human_one(s: &StatusSystem) -> String {
    let mut rows = vec![vec!["id".to_string(), s.id.to_string()], vec!["label".into(), s.label.clone()], vec!["kind".into(), s.kind.as_str().into()]];
    if let Some(c) = s.class {
        rows.push(vec!["class".into(), c.as_str().into()]);
    }
    if let Some(n) = &s.node {
        rows.push(vec!["node".into(), n.clone()]);
    }
    rows.push(vec!["status".into(), s.status.as_str().into()]);
    for (k, v) in [("reason", &s.reason), ("fault", &s.fault), ("preset", &s.preset), ("model", &s.model), ("baseUrl", &s.base_url)] {
        if let Some(v) = v {
            rows.push(vec![k.into(), v.clone()]);
        }
    }
    if let Some(a) = s.adapter {
        rows.push(vec!["adapter".into(), a.as_str().into()]);
    }
    if let Some(k) = s.api_key {
        rows.push(vec!["api key".into(), if k { "required".into() } else { "none".into() }]);
    }
    if !s.params.is_empty() {
        rows.push(vec!["params".into(), s.params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")]);
    }
    if let Some(t) = s.decode_tps {
        rows.push(vec!["decode".into(), format!("{t:.1} tok/s")]);
    }
    if let Some(g) = s.vram_gib {
        rows.push(vec!["vram".into(), format!("{g:.2} GiB")]);
    }
    table(&rows)
}

pub fn status(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let which = args.next_pos();
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let sj = status_of(&vm);
    match which {
        None => out.doc(val(&sj), || human_status(&sj, &conn, &vm)),
        Some(arg) => {
            let id = resolve(&vm, &arg)?;
            let entry = sj.systems.iter().find(|s| s.id == id).ok_or_else(|| CliError::new("not_found", format!("There is no System \"{arg}\".")))?;
            out.doc(val(entry), || human_one(entry));
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------- diag

pub fn diag(args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    args.done()?;
    let conn = conn::connect(loaded)?;
    let engine = conn.link().diag().map_err(|e| CliError::new("control", format!("The engine did not answer: {e:#}")))?;
    let loc = klif_core::klif_common::config::locate();
    #[cfg(windows)]
    let ram_type = klif_telemetry::smbios_ram_type();
    #[cfg(not(windows))]
    let ram_type: Option<String> = None;
    let cli = DiagCli {
        version: klif_core::klif_common::KLIF_VERSION.to_string(),
        exe: std::env::current_exe().ok().map(|p| p.display().to_string()),
        engine: conn.mode().to_string(),
        engine_pid: conn.engine_pid(),
        config: DiagConfig {
            path: loc.path.display().to_string(),
            origin: val(&loc.origin).as_str().unwrap_or("").to_string(),
            exists: loc.exists,
            state_dir: loaded.cfg.state_dir.display().to_string(),
            data_dir: loaded.cfg.data_dir.display().to_string(),
        },
        config_issues: loaded.issues.clone(),
        smbios_ram_type: ram_type,
    };
    let doc = val(&DiagDoc { cli, engine });
    out.doc(doc.clone(), || serde_json::to_string_pretty(&doc).unwrap_or_default());
    Ok(())
}

// -------------------------------------------------------------------------------------------- plan

pub fn human_command(c: &CommandView) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    if let Some(ext) = &c.external {
        rows.push(vec!["external".into(), format!("{ext} (KLIF only watches it)")]);
    } else {
        rows.push(vec!["program".into(), c.program.clone()]);
        rows.push(vec!["args".into(), if c.args.is_empty() { "(none)".into() } else { c.args.join(" ") }]);
        rows.push(vec!["cwd".into(), c.cwd.clone()]);
    }
    rows.push(vec!["adapter".into(), c.adapter.as_str().into()]);
    let health = match &c.health {
        klif_core::klif_common::vm::HealthCheck::Auto => "adapter default".to_string(),
        klif_core::klif_common::vm::HealthCheck::Http { path } => format!("HTTP {path}"),
        klif_core::klif_common::vm::HealthCheck::Tcp => "TCP".to_string(),
    };
    rows.push(vec!["listen".into(), format!("{}:{} (health: {health})", c.host, c.port)]);
    for (i, e) in c.env.iter().enumerate() {
        let v = match (e.removed, e.managed, e.overridden) {
            (true, true, _) => format!("{} (removed by KLIF)", e.name),
            (true, false, _) => format!("{} (removed)", e.name),
            (false, managed, overridden) => {
                let value = e.value.as_deref().unwrap_or(klif_core::klif_common::secret::MASK);
                let tag = match (managed, overridden) {
                    (true, true) => " (KLIF's, overridden by the preset)",
                    (true, false) => " (set by KLIF)",
                    _ => "",
                };
                format!("{}={value}{tag}", e.name)
            }
        };
        rows.push(vec![if i == 0 { "env".into() } else { String::new() }, v]);
    }
    if !c.hash.is_empty() {
        rows.push(vec!["hash".into(), c.hash.clone()]);
    }
    let mut out = table(&rows);
    if c.external.is_none() && !c.display.is_empty() {
        out.push_str(&format!("\n{}\n", c.display));
    }
    if !c.issues.is_empty() {
        out.push('\n');
        for i in &c.issues {
            let lvl = if i.is_error() { "error" } else { "warn" };
            match &i.field {
                Some(f) => out.push_str(&format!("{lvl}: [{f}] {}\n", i.text)),
                None => out.push_str(&format!("{lvl}: {}\n", i.text)),
            }
        }
    }
    out
}

pub fn plan(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let arg = args.pos("System")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let s = resolve_system(&vm, &arg)?;
    let cmd = conn.link().plan(&s.id).map_err(refused)?;
    // The plan is shown even when the System cannot start (exit 0); `launchable` says whether a launch would run.
    let refusal = launch_refusal(s, &cmd);
    let doc = PlanDoc { system: s.id.clone(), label: s.label.clone(), preset: s.preset.clone(), launchable: refusal.is_none(), command: cmd.clone(), refused: refusal.clone() };
    out.doc(val(&doc), || {
        let verdict = match &refusal {
            None => "launchable: yes".to_string(),
            Some(r) => format!("launchable: NO ({r})"),
        };
        format!("{} ({}) · preset {}\n\n{}\n{verdict}\n", s.label, s.id, s.preset.as_deref().unwrap_or("-"), human_command(&cmd))
    });
    Ok(())
}

/// Why a launch of `s` with this command would be refused (None: it would run): the command's first error, else
/// the System's invalid reason (ports, a foreign holder), else an external server KLIF only watches.
fn launch_refusal(s: &System, cmd: &CommandView) -> Option<String> {
    if let Some(i) = cmd.issues.iter().find(|i| i.is_error()) {
        return Some(i.text.trim_end_matches('.').to_string());
    }
    if s.status == SystemStatus::Invalid {
        return Some(s.reason.as_deref().unwrap_or("its preset has errors").trim_end_matches('.').to_string());
    }
    if s.external || cmd.external.is_some() {
        return Some("an external server; KLIF only watches it".into());
    }
    if !s.controllable {
        return Some("its node does not let this machine launch (add \"launch\" to [node] allow on that node)".into());
    }
    None
}

// --------------------------------------------------------------------------- select / launch / stop

pub fn select(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let arg = args.pos("System")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &arg)?;
    act(&conn, Action::Select { system: id.clone() })?;
    let vm = conn.snapshot(None)?;
    let s = find(&vm, &id).cloned();
    out.doc(val(&SelectDoc { selected: vm.selected.clone(), system: s.as_ref().map(SysBrief::of) }), || {
        format!("Selected {}.", s.as_ref().map(|s| format!("{} ({})", s.label, s.id)).unwrap_or_else(|| id.to_string()))
    });
    Ok(())
}

pub fn launch(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let stop_others = args.flag("--stop-others");
    let wait = args.flag("--wait");
    let arg = args.pos("System")?;
    args.done()?;
    if !yes {
        return Err(CliError::needs_yes(format!(
            "launch starts a model server: add --yes (klif-cli plan {arg} shows exactly what runs)."
        )));
    }
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &arg)?;
    act(&conn, Action::Launch { system: Some(id.clone()), stop_others })?;
    let s = if wait {
        note(format!("launched {id}; waiting until it is ready (the server keeps running if klif-cli is interrupted)"));
        wait_live(&conn, &id, LAUNCH_WAIT, None)?
    } else {
        wait_started(&conn, &id)?
    };
    out.doc(val(&LaunchDoc { launched: true, system: SysBrief::of(&s) }), || sys_line(&s));
    Ok(())
}

pub fn stop(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let all = args.flag("--all");
    let arg = args.next_pos();
    args.done()?;
    if all == arg.is_some() {
        return Err(CliError::usage("stop needs a System or --all (not both)."));
    }
    if !yes {
        return Err(CliError::needs_yes("stop ends a running model server: add --yes."));
    }
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let ids: Vec<SystemId> = match &arg {
        Some(a) => vec![resolve(&vm, a)?],
        None => vm.systems.iter().filter(|s| s.node.is_none() && !s.id.is_remote() && holds(s)).map(|s| s.id.clone()).collect(),
    };
    if all {
        act(&conn, Action::StopAll)?;
    } else {
        act(&conn, Action::Stop { system: Some(ids[0].clone()) })?;
    }
    let vm = wait_stopped(&conn, &ids)?;
    let stopped: Vec<SysBrief> = ids.iter().filter_map(|id| find(&vm, id)).map(SysBrief::of).collect();
    out.doc(val(&StopDoc { stopped }), || {
        if ids.is_empty() {
            "Nothing was running.".to_string()
        } else {
            ids.iter().filter_map(|id| find(&vm, id)).map(sys_line).collect::<Vec<_>>().join("\n")
        }
    });
    Ok(())
}

pub fn restart(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let wait = args.flag("--wait");
    let arg = args.pos("System")?;
    args.done()?;
    if !yes {
        return Err(CliError::needs_yes("restart stops and starts a model server: add --yes."));
    }
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &arg)?;
    let old_uptime = find(&vm, &id).and_then(|s| s.session.as_ref()).map(|x| x.uptime_s).unwrap_or(0.0);
    act(&conn, Action::Restart { system: Some(id.clone()) })?;
    let s = if wait { wait_live(&conn, &id, LAUNCH_WAIT, Some(old_uptime))? } else { wait_started(&conn, &id)? };
    out.doc(val(&RestartDoc { restarted: true, system: SysBrief::of(&s) }), || sys_line(&s));
    Ok(())
}

pub fn dismiss(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let arg = args.pos("System")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &arg)?;
    act(&conn, Action::Dismiss { system: Some(id.clone()) })?;
    let vm = conn.snapshot(None)?;
    let s = find(&vm, &id).cloned();
    out.doc(val(&DismissDoc { system: s.as_ref().map(SysBrief::of) }), || s.as_ref().map(sys_line).unwrap_or_else(|| format!("Dismissed {id}.")));
    Ok(())
}

// ------------------------------------------------------------------------------------------ systems

fn human_systems(vm: &ViewModel) -> String {
    if vm.systems.is_empty() {
        return "No Systems yet (klif-cli systems add --kind llm).".into();
    }
    let mut rows = vec![["", "ID", "LABEL", "KIND", "CLASS", "STATUS", "PRESET", "GPU", "FLAGS"].map(String::from).to_vec()];
    for s in &vm.systems {
        let mut flags = Vec::new();
        if s.exclusive {
            flags.push("exclusive");
        }
        if s.external {
            flags.push("external");
        }
        if !s.editable {
            flags.push("read-only");
        }
        if !s.conflicts.is_empty() {
            flags.push("conflicts");
        }
        rows.push(vec![
            if vm.selected.as_ref() == Some(&s.id) { "*".into() } else { String::new() },
            s.id.to_string(),
            s.label.clone(),
            s.kind.as_str().into(),
            s.class.map(|c| c.as_str().to_string()).unwrap_or_else(|| "-".into()),
            s.status.as_str().into(),
            s.preset.clone().unwrap_or_else(|| "-".into()),
            s.gpu.clone().unwrap_or_else(|| "-".into()),
            flags.join(","),
        ]);
    }
    table(&rows)
}

fn on_off(v: &str) -> CliResult<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(CliError::usage(format!("Expected on or off, not \"{v}\"."))),
    }
}

pub fn systems(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.pos("systems subcommand (list, add, remove, rename, move, exclusive)")?;
    match sub.as_str() {
        "list" | "ls" => {
            args.done()?;
            let conn = conn::connect(loaded)?;
            let vm = conn.snapshot(None)?;
            out.doc(val(&SystemsListDoc { selected: vm.selected.clone(), systems: vm.systems.clone() }), || human_systems(&vm));
            Ok(())
        }
        "add" => {
            let kind = args.opt("--kind")?.ok_or_else(|| CliError::usage("systems add needs --kind llm|image|tts|stt|video|music."))?;
            let kind = SystemKind::parse(&kind).ok_or_else(|| CliError::usage(format!("Unknown kind \"{kind}\" (llm, image, tts, stt, video, music).")))?;
            let class = match args.opt("--class")? {
                None => None,
                Some(c) => Some(LlmClass::parse(&c).ok_or_else(|| CliError::usage(format!("Unknown class \"{c}\" (fast, deep, max).")))?),
            };
            let label = args.opt("--label")?;
            let id = args.opt("--id")?;
            let preset = args.opt("--preset")?;
            let node = args.opt("--node")?;
            args.done()?;
            if class.is_some() && kind != SystemKind::Llm {
                return Err(CliError::usage("--class only applies to llm Systems."));
            }
            if let Some(id) = &id {
                klif_core::klif_common::config::validate_system_id(id).map_err(CliError::usage)?;
            }
            let conn = conn::connect(loaded)?;
            let before: BTreeSet<SystemId> = conn.snapshot(None)?.systems.into_iter().map(|s| s.id).collect();
            act(&conn, Action::AddSystem { id, label, kind, class, preset, node: node.clone() })?;
            // The engine does not return the new id: find it in the next view models.
            let t0 = Instant::now();
            let added = loop {
                let vm = conn.snapshot(None)?;
                let new: Vec<System> = vm.systems.into_iter().filter(|s| !before.contains(&s.id)).collect();
                if !new.is_empty() || t0.elapsed() > Duration::from_secs(if node.is_some() { 6 } else { 3 }) {
                    break new.into_iter().next();
                }
                std::thread::sleep(Duration::from_millis(250));
            };
            out.doc(val(&SystemsAddDoc { added: added.as_ref().map(SysBrief::of) }), || match &added {
                Some(s) => format!("Added {} ({}).", s.label, s.id),
                None => "Added (the new System is not visible yet).".into(),
            });
            Ok(())
        }
        "remove" | "rm" => {
            let yes = args.flag("--yes");
            let arg = args.pos("System")?;
            args.done()?;
            if !yes {
                return Err(CliError::needs_yes(format!("systems remove deletes [systems.{arg}] from klif.toml: add --yes.")));
            }
            let conn = conn::connect(loaded)?;
            let vm = conn.snapshot(None)?;
            let s = resolve_system(&vm, &arg)?.clone();
            act(&conn, Action::RemoveSystem { system: s.id.clone() })?;
            out.doc(val(&SystemsRemoveDoc { removed: s.id.clone() }), || format!("Removed {} ({}).", s.label, s.id));
            Ok(())
        }
        "rename" => {
            let arg = args.pos("System")?;
            let label = args.rest().join(" ");
            args.done()?;
            if label.trim().is_empty() {
                return Err(CliError::usage("systems rename needs a label."));
            }
            update(loaded, out, &arg, Some(label.trim().to_string()), None, None)
        }
        "move" => {
            let arg = args.pos("System")?;
            let idx = args.pos("tab index")?;
            args.done()?;
            let idx: u32 = idx.trim().parse().map_err(|_| CliError::usage(format!("The tab index must be a number (0 = first), not \"{idx}\".")))?;
            update(loaded, out, &arg, None, Some(idx), None)
        }
        "exclusive" => {
            let arg = args.pos("System")?;
            let v = on_off(&args.pos("on or off")?)?;
            args.done()?;
            update(loaded, out, &arg, None, None, Some(v))
        }
        other => Err(CliError::usage(format!("Unknown systems subcommand \"{other}\" (list, add, remove, rename, move, exclusive)."))),
    }
}

fn update(loaded: &LoadedConfig, out: Out, arg: &str, label: Option<String>, move_to: Option<u32>, exclusive: Option<bool>) -> CliResult {
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, arg)?;
    act(&conn, Action::UpdateSystem { system: id.clone(), label, move_to, exclusive })?;
    let vm = conn.snapshot(None)?;
    let s = find(&vm, &id).cloned();
    let pos = vm.systems.iter().filter(|x| x.node.is_none()).position(|x| x.id == id);
    out.doc(val(&SystemsUpdateDoc { system: s.as_ref().map(SysBrief::of), index: pos, exclusive: s.as_ref().map(|s| s.exclusive) }), || match &s {
        Some(s) => format!(
            "{} ({}) · tab {} · exclusive {}",
            s.label,
            s.id,
            pos.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            if s.exclusive { "on" } else { "off" }
        ),
        None => format!("Updated {id}."),
    });
    Ok(())
}

// -------------------------------------------------------------------------------------------- nodes

pub fn nodes(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.next_pos().unwrap_or_else(|| "list".into());
    args.done()?;
    if sub != "list" && sub != "ls" {
        return Err(CliError::usage(format!("Unknown nodes subcommand \"{sub}\" (list).")));
    }
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    out.doc(val(&NodesListDoc { nodes: vm.nodes.clone() }), || {
        if vm.nodes.is_empty() {
            return "No remote nodes ([nodes.<id>] in klif.toml).".into();
        }
        let mut rows = vec![["ID", "NAME", "ADDRESS", "STATE", "LATENCY", "VERSION", "ALLOW", "NOTE"].map(String::from).to_vec()];
        for n in &vm.nodes {
            rows.push(vec![
                n.id.clone(),
                n.name.clone(),
                n.address.clone(),
                val(&n.state).as_str().unwrap_or("").to_string(),
                n.latency_ms.map(|l| format!("{l:.0} ms")).unwrap_or_else(|| "-".into()),
                n.version.clone().unwrap_or_else(|| "-".into()),
                if n.allow.is_empty() { "view".into() } else { format!("view,{}", n.allow.join(",")) },
                n.error.clone().unwrap_or_default(),
            ]);
        }
        table(&rows)
    });
    Ok(())
}

// -------------------------------------------------------------------------------------------- serve

pub fn serve(args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    args.done()?;
    let handle = conn::start_owned(loaded)?;
    let cfg = &loaded.cfg;
    let listen = cfg.node.as_ref().and_then(|n| n.listen_addr());
    out.doc(
        val(&ServeDoc {
            serving: true,
            pid: std::process::id(),
            config: cfg.source.as_ref().map(|p| p.display().to_string()),
            state_dir: cfg.state_dir.display().to_string(),
            network: listen.clone(),
        }),
        || {
            format!(
                "KLIF engine running headless (pid {}){}. Stop it with Ctrl+C; model servers keep running.",
                std::process::id(),
                listen.as_deref().map(|l| format!(", node listener {l}")).unwrap_or_default()
            )
        },
    );
    // Status changes on stderr until the process is ended.
    let mut last: Vec<(SystemId, SystemStatus)> = Vec::new();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let vm = handle.snapshot();
        let now: Vec<(SystemId, SystemStatus)> = vm.systems.iter().map(|s| (s.id.clone(), s.status)).collect();
        if !out.json {
            for (id, st) in &now {
                if !last.iter().any(|(i, s)| i == id && s == st) {
                    if let Some(s) = find(&vm, id) {
                        note(format!("  {}", sys_line(s)));
                    }
                }
            }
        }
        last = now;
    }
}
