//! klif-cli: inspect the KLIF core from a terminal.
//!
//!   klif-cli status [--wait <s>]     one ViewModel as JSON (after the first telemetry samples)
//!   klif-cli plan <slot> [key=value...]  the launch plan for a tier: exe, argv, cwd, logs, env NAMES (never values);
//!                                    key=value pairs patch the recipe in memory only (state.json is not written),
//!                                    e.g. `plan krea edit=on precision=medium`
//!   klif-cli watch <n>               n one-line summaries at 2 Hz
//!   klif-cli launch <slot> --yes     really launch the tier's recipe (needs --yes), then follow it until live
//!   klif-cli gpu [--secs <s>]        the inference card's power state (1 Hz for s seconds) and EnableUlps
//!
//! Slots: high, medium, low, krea. Nothing is ever launched or stopped without `launch ... --yes`, and
//! a running server is never stopped by this tool (servers survive KLIF by design).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use klif_core::klif_catalog::EnvValue;
use klif_core::klif_common::config::Config;
use klif_core::klif_common::vm::{Action, HostInfo, HostKind, Phase, RecipePatch, SlotId, ViewModel};
use klif_core::{Engine, EngineHandle};

fn usage() -> &'static str {
    "usage:\n  klif-cli status [--wait <seconds>]\n  klif-cli plan <high|medium|low|krea>\n  klif-cli watch <n>\n  klif-cli launch <high|medium|low|krea> --yes\n  klif-cli gpu [--secs <seconds>]\n"
}

fn parse_slot(s: &str) -> Result<SlotId> {
    SlotId::ALL
        .iter()
        .copied()
        .find(|x| x.as_str().eq_ignore_ascii_case(s.trim()))
        .ok_or_else(|| anyhow!("unknown slot {s:?} (high, medium, low, krea)"))
}

fn host() -> HostInfo {
    HostInfo { kind: HostKind::Browser, frameless: false, maximized: false, app_version: env!("CARGO_PKG_VERSION").into(), panel: Default::default() }
}

fn start() -> Result<EngineHandle> {
    let cfg = Config::load()?;
    eprintln!("config: {}", cfg.source.display());
    Engine::start(cfg, host())
}

/// Wait for the telemetry's first GPU/CPU samples (1 Hz) and a couple of ticks.
fn settle(e: &EngineHandle, secs: f64) {
    std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
    e.refresh();
}

fn summary(vm: &ViewModel) -> String {
    let t = vm.now;
    let secs = t.rem_euclid(86400.0);
    let clock = format!("{:02}:{:02}:{:04.1}Z", (secs / 3600.0) as u32, ((secs % 3600.0) / 60.0) as u32, secs % 60.0);
    let ready: Vec<String> = vm
        .slots
        .iter()
        .map(|s| format!("{}={}", s.id.as_str(), serde_json::to_value(s.availability).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()))
        .collect();
    let sess = match &vm.session {
        None => "idle".to_string(),
        Some(s) => {
            let mut x = format!("{}:{:?} {} {} up {:.0}s :{}", s.slot.as_str(), s.phase, s.model.name, s.model.quant, s.uptime_s, s.endpoint.port);
            if let Some(l) = &s.loading {
                x += &format!(" load {:.0}%", l.fraction * 100.0);
            }
            if let Some(l) = &s.llm {
                x += &format!(
                    " {:?} {:.1} tok/s ctx {}/{} req {}",
                    l.activity, l.decode_tps, l.context.used_tokens, l.context.total_tokens, l.totals.requests
                );
            }
            if let Some(i) = &s.image {
                x += &format!(" {:?} step {}/{} imgs {}", i.activity, i.step, i.steps, i.images_this_session);
            }
            if let Some(f) = &s.fault {
                x += &format!(" FAULT {}", f.title);
            }
            x
        }
    };
    let dormant = match &vm.vram.dormant {
        Some(d) => format!(
            " DORMANT {:.2} GiB paged out{} for {:.0}s",
            d.paged_out_gib,
            d.power_state.as_deref().map(|p| format!(" ({p})")).unwrap_or_default(),
            d.since_s
        ),
        None => String::new(),
    };
    format!(
        "{clock} sel={} [{}] {} | vram {:.2}/{:.2} GiB ({}){dormant} | ram {:.1}/{:.1} GiB | cpu {:.0}%",
        vm.selected.as_str(),
        ready.join(" "),
        sess,
        vm.vram.used_gib,
        vm.vram.total_gib,
        vm.vram.layers.iter().map(|l| format!("{}:{:.2}", serde_json::to_value(l.id).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), l.gib)).collect::<Vec<_>>().join(","),
        vm.system.ram_used_gib,
        vm.system.ram_total_gib,
        vm.system.cpu_pct
    )
}

fn cmd_status(args: &[String]) -> Result<()> {
    let wait = match args.iter().position(|a| a == "--wait") {
        Some(i) => args.get(i + 1).ok_or_else(|| anyhow!("--wait needs seconds"))?.parse::<f64>()?,
        None => 2.2,
    };
    let e = start()?;
    settle(&e, wait);
    let vm = e.snapshot();
    println!("{}", serde_json::to_string_pretty(&vm)?);
    let dormant = match &vm.vram.dormant {
        Some(d) => serde_json::to_string(d)?,
        None => "absent".into(),
    };
    eprintln!("vram.dormant: {dormant}");
    let cfg = Config::load()?;
    gpu_facts(&cfg, Some(&e));
    e.shutdown();
    Ok(())
}

/// The inference card's power state and EnableUlps, on stderr (read-only).
fn gpu_facts(cfg: &Config, e: Option<&EngineHandle>) {
    let Some(pci) = cfg.gpu.inference.as_deref() else {
        eprintln!("gpu: no inference card configured (gpu.inference)");
        return;
    };
    let t0 = Instant::now();
    let power = klif_core::klif_telemetry::power_state(pci);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("gpu: {pci} power state {} (read in {ms:.1} ms)", power.map(|p| p.as_str()).unwrap_or("unknown"));
    let ulps = match e {
        Some(e) => e.ulps_setting(),
        None => klif_core::klif_telemetry::ulps_setting(pci),
    };
    match ulps {
        Some(u) => eprintln!(
            "gpu: EnableUlps = {} ({}; DriverDesc {:?}; device {}){}",
            u.enable_ulps.map(|v| v.to_string()).unwrap_or_else(|| "not set".into()),
            u.key,
            u.driver_desc.clone().unwrap_or_default(),
            u.instance_id,
            if u.is_on() { " -> ULPS on: the idle card can sleep with a model loaded" } else { "" }
        ),
        None => eprintln!("gpu: EnableUlps could not be read (no matching display device or driver key)"),
    }
}

/// `klif-cli gpu [--secs N]`: power state once per second for N seconds (read-only, starts nothing).
fn cmd_gpu(args: &[String]) -> Result<()> {
    let secs = match args.iter().position(|a| a == "--secs") {
        Some(i) => args.get(i + 1).ok_or_else(|| anyhow!("--secs needs seconds"))?.parse::<u32>()?,
        None => 0,
    };
    let cfg = Config::load()?;
    gpu_facts(&cfg, None);
    let pci = cfg.gpu.inference.clone().ok_or_else(|| anyhow!("no inference card configured"))?;
    // The sampler's reader: the device handle stays open, each read is one property query.
    let mut reader = klif_core::klif_telemetry::PowerReader::for_pci(&pci);
    let t0 = Instant::now();
    let mut last: Option<&str> = None;
    for i in 0..secs {
        let t = Instant::now();
        let p = reader.read(klif_core::klif_common::now_s()).map(|p| p.as_str());
        let us = t.elapsed().as_secs_f64() * 1e6;
        let change = if i > 0 && p != last { "  <- changed" } else { "" };
        println!("{:>4}s power {} ({us:.0} us){change}", i, p.unwrap_or("unknown"));
        last = p;
        let next = t0 + Duration::from_secs(i as u64 + 1);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    Ok(())
}

/// `key=value` pairs to a Tune patch (the UI's `Partial<Recipe>`), for `plan` previews only.
fn parse_patch(pairs: &[String]) -> Result<Option<RecipePatch>> {
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut obj = serde_json::Map::new();
    for pair in pairs {
        let (k, v) = pair.split_once('=').ok_or_else(|| anyhow!("expected key=value, got {pair:?}"))?;
        let (key, value) = match k.trim().to_ascii_lowercase().as_str() {
            "edit" => ("edit", serde_json::Value::Bool(parse_bool(v)?)),
            "vision" => ("vision", serde_json::Value::Bool(parse_bool(v)?)),
            "precision" => ("precision", serde_json::Value::String(v.trim().to_ascii_lowercase())),
            "size" | "imagesize" => ("imageSize", serde_json::Value::String(v.trim().into())),
            "card" | "cardid" => ("cardId", serde_json::Value::String(v.trim().into())),
            "backend" => ("backend", serde_json::Value::String(v.trim().into())),
            "hardware" => ("hardware", serde_json::Value::String(v.trim().into())),
            "mode" => ("mode", serde_json::Value::String(v.trim().into())),
            "kv" | "kvtype" => ("kvType", serde_json::Value::String(v.trim().into())),
            "port" => ("port", serde_json::Value::from(v.trim().parse::<u16>()?)),
            "ctx" | "ctxtokens" => ("ctxTokens", serde_json::Value::from(v.trim().parse::<u32>()?)),
            other => bail!("unknown recipe key {other:?} (edit precision size card backend hardware port mode ctx kv vision)"),
        };
        obj.insert(key.into(), value);
    }
    Ok(Some(serde_json::from_value(serde_json::Value::Object(obj))?))
}

fn parse_bool(v: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => bail!("expected on/off, got {v:?}"),
    }
}

fn cmd_plan(args: &[String]) -> Result<()> {
    let slot = parse_slot(args.first().ok_or_else(|| anyhow!("plan needs a slot"))?)?;
    let patch = parse_patch(&args[1..])?;
    let cfg = Config::load()?;
    let pv = klif_core::preview_plan_with(&cfg, slot, patch.as_ref())?;
    let p = &pv.plan;
    println!("slot:        {} ({})", slot.as_str(), slot.label());
    if patch.is_some() {
        println!("recipe:      {} (patched in memory, state.json untouched)", serde_json::to_string(&pv.recipe)?);
    } else {
        println!("recipe:      {}", serde_json::to_string(&pv.recipe)?);
    }
    if let Some(mode) = &p.model.mode {
        println!("mode:        {mode}");
    }
    println!("card:        {}  profile {}", p.card_id, p.profile_key);
    println!("model:       {} {} [{}] on {}", p.model.name, p.model.quant, serde_json::to_string(&p.model.backend)?, p.model.device);
    println!("exe:         {}", p.exe.display());
    println!("argv:        {}", p.args.join(" "));
    for (i, a) in p.args.iter().enumerate() {
        println!("  [{i:2}] {a}");
    }
    println!("cwd:         {}", p.cwd.display());
    println!("session:     {}", p.session_name);
    println!("out log:     {}", p.out_log.display());
    println!("err log:     {}", p.err_log.display());
    println!("endpoint:    {}:{}", p.host, p.port);
    println!("env remove:  {}", p.env_remove.join(", "));
    let names: Vec<String> = p
        .env_set
        .iter()
        .map(|(n, v)| match v {
            EnvValue::Plain(_) => n.clone(),
            EnvValue::Secret(_) => format!("{n} (secret)"),
        })
        .collect();
    println!("env set:     {}", if names.is_empty() { "(none)".to_string() } else { names.join(", ") });
    println!("api key:     {}", if pv.api_key_present { "present (value not shown)" } else { "none" });
    if let Some(spec) = &p.spec_mode {
        println!("speculative: {spec}");
    }
    let gib: f64 = p.expected_layers.iter().map(|l| l.gib).sum();
    println!("expected:    {gib:.2} GiB ({})", p.expected_layers.iter().map(|l| l.label.clone()).collect::<Vec<_>>().join(", "));
    match &pv.refused {
        None => println!("launchable:  yes"),
        Some(r) => println!("launchable:  NO ({r})"),
    }
    Ok(())
}

fn cmd_watch(args: &[String]) -> Result<()> {
    let n: u32 = args.first().map(|s| s.parse()).transpose()?.unwrap_or(10);
    let e = start()?;
    settle(&e, 1.2);
    let t0 = Instant::now();
    for i in 0..n {
        println!("{}", summary(&e.snapshot()));
        let next = t0 + Duration::from_millis(500 * (i as u64 + 1));
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    e.shutdown();
    Ok(())
}

fn cmd_launch(args: &[String]) -> Result<()> {
    let slot = parse_slot(args.first().ok_or_else(|| anyhow!("launch needs a slot"))?)?;
    if !args.iter().any(|a| a == "--yes") {
        bail!("refusing to launch without --yes (see `klif-cli plan {}` for what would run)", slot.as_str());
    }
    let e = start()?;
    settle(&e, 1.2);
    e.act(Action::Launch { slot: Some(slot) }).map_err(|err| anyhow!("{err}"))?;
    eprintln!("launched {}; following it until live or fault (Ctrl+C leaves the server running)", slot.as_str());
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    loop {
        std::thread::sleep(Duration::from_millis(1000));
        let vm = e.snapshot();
        println!("{}", summary(&vm));
        match vm.session.as_ref().map(|s| s.phase) {
            Some(Phase::Live) => break,
            Some(Phase::Fault) | None => {
                if let Some(f) = vm.session.as_ref().and_then(|s| s.fault.as_ref()) {
                    for l in &f.log_tail {
                        eprintln!("  | {l}");
                    }
                }
                break;
            }
            _ => {}
        }
        if Instant::now() > deadline {
            eprintln!("still loading after 15 minutes; leaving it running");
            break;
        }
    }
    e.shutdown();
    Ok(())
}

/// Stop the session KLIF owns (adopted from state.json). Never touches processes KLIF did not start.
fn cmd_stop(args: &[String]) -> Result<()> {
    if !args.iter().any(|a| a == "--yes") {
        bail!("refusing to stop without --yes");
    }
    let e = start()?;
    settle(&e, 1.2);
    let vm = e.snapshot();
    if vm.session.is_none() {
        eprintln!("no KLIF session to stop");
        e.shutdown();
        return Ok(());
    }
    e.act(Action::Stop).map_err(|err| anyhow!("{err}"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let vm = e.snapshot();
        println!("{}", summary(&vm));
        if vm.session.is_none() || Instant::now() > deadline {
            break;
        }
    }
    e.shutdown();
    Ok(())
}

/// Engine log lines to stderr (warnings by default, everything with KLIF_LOG=info).
struct StderrLog;

static LOGGER: StderrLog = StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::max_level()
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{} {}] {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> ExitCode {
    let level = match std::env::var("KLIF_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        _ => log::LevelFilter::Warn,
    };
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("status") => cmd_status(&args[1..]),
        Some("plan") => cmd_plan(&args[1..]),
        Some("watch") => cmd_watch(&args[1..]),
        Some("launch") => cmd_launch(&args[1..]),
        Some("stop") => cmd_stop(&args[1..]),
        Some("gpu") => cmd_gpu(&args[1..]),
        _ => {
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
