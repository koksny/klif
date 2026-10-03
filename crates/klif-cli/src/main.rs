//! klif-cli: KLIF from a terminal or a coding agent (SPEC section 8). Talks to the running engine over the
//! control channel (`klif_core::link::ControlClient`), else runs an in-process engine while holding engine.lock.
//!
//! `klif-cli [--json] <command>`: with `--json` stdout carries exactly one JSON document `{schemaVersion: 1, ...}`;
//! failures are `{schemaVersion: 1, error: {code, message}}` with exit code 1, usage errors exit 2. Nothing is
//! launched, stopped, removed or downloaded without `--yes`; a model server is never stopped because klif-cli exits.

mod args;
mod bench_cmd;
mod cmds;
mod conn;
mod keys_cmd;
mod models;
mod out;
mod presets;
mod resolve;

use args::Args;
use out::{CliError, CliResult, Out};
use std::process::ExitCode;

const USAGE: &str = "\
klif-cli - KLIF (Koksny.com LOCAL INFERENCE FORNICATOR) from a terminal or a coding agent

usage: klif-cli [--json] <command> ...

  status [<system>]                       Systems, GPUs and nodes (one System with <system>)
  diag                                    diagnostics: config location, engine, GPUs, versions (no secrets)
  plan <system>                           the exact command a System would launch now (secrets masked)
  select <system>                         select a tab
  launch <system> --yes [--stop-others] [--wait]
  stop <system> --yes | stop --all --yes
  restart <system> --yes [--wait]
  dismiss <system>                        leave a fault (back to offline)

  systems list
  systems add --kind llm|image|tts|stt|video [--class fast|deep|max] [--label L] [--id ID] [--preset P] [--node N]
  systems remove <system> --yes
  systems rename <system> <label>
  systems move <system> <index>           0-based tab index among the local Systems
  systems exclusive <system> on|off

  presets list [--node N]
  presets show <id> [--node N]
  presets use <system> <id>
  presets param <system> <name> <value>
  presets save <id> --file F.toml [--use <system>] [--node N]
  presets set <id> key=value... [--node N]
  presets delete <id> --yes [--node N]

  bench <system> [--runs N] [--prompt N] [--gen N] [--keep-running] [--allow-shared] [--yes]
  bench list [--preset ID]

  models list [--kind K]
  models download <rec-id> --yes
  models adopt <rec-id> [--system S]

  key status | key set (reads one line from stdin) | key clear --yes
  node status | node token [--create [--yes]]
  nodes list
  serve                                   run the engine headless (+ the network listener when [node] listen is set)

<system>: an id (s1, render-box/s1), a label ignoring case and spaces (system1, \"System 1\"),
          or the 0.2 names low|medium|high|krea (local s1/s2/s3/cgi).
presets set keys: command  args=[\"json\",\"array\"]  args+=TOKEN  args-=TOKEN  env.NAME=VALUE  env.NAME-
          env_remove+=NAME  env_remove-=NAME  cwd  port  host  endpoint  health  model  mmproj  ctx  gpu  kind
          adapter  name  managed  api_key  model_name  quant  backend  device  notes   (empty value = unset)
config:   KLIF_CONFIG=<klif.toml>, else .local\\klif.toml above the exe or the current folder, else %APPDATA%\\KLIF\\klif.toml.
logs:     KLIF_LOG=info|debug (stderr; trace = KLIF's own records only, dependencies capped at debug).
";

/// Engine log lines on stderr: warnings by default (KLIF_LOG=info|debug for more); bench and download progress
/// always.
struct StderrLog;

static LOGGER: StderrLog = StderrLog;
static LEVEL: std::sync::OnceLock<log::LevelFilter> = std::sync::OnceLock::new();

fn progress_target(target: &str) -> bool {
    target.starts_with("klif_core::bench") || target.starts_with("klif_core::download")
}

/// Dependencies whose trace output is wire bytes (request heads with `Authorization: Bearer <key>`, bodies, TLS
/// records): capped at debug even if the prefix rule below ever changes (as the shell's klog.rs does).
const WIRE_TARGETS: [&str; 4] = ["ureq_proto", "ureq", "native_tls", "rustls"];

fn wire_target(target: &str) -> bool {
    WIRE_TARGETS.iter().any(|t| target == *t || target.strip_prefix(t).is_some_and(|rest| rest.starts_with("::")))
}

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        // KLIF_LOG=trace covers KLIF's own records only: a dependency never logs above debug.
        if (!m.target().starts_with("klif") || wire_target(m.target())) && m.level() > log::Level::Debug {
            return false;
        }
        m.level() <= *LEVEL.get().unwrap_or(&log::LevelFilter::Warn) || (progress_target(m.target()) && m.level() <= log::Level::Info)
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        if progress_target(r.target()) && r.level() == log::Level::Info {
            eprintln!("  {}", r.args());
        } else {
            eprintln!("[{} {}] {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

fn init_log() {
    let level = match std::env::var("KLIF_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "error" => log::LevelFilter::Error,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Warn,
    };
    let _ = LEVEL.set(level);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info.max(level));
    }
}

fn main() -> ExitCode {
    init_log();
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    let json = {
        let before = raw.len();
        raw.retain(|a| a != "--json");
        raw.len() != before
    };
    let out = Out { json };
    let result = run(raw, out);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            out.error(&e);
            ExitCode::from(e.exit)
        }
    }
}

fn run(mut raw: Vec<String>, out: Out) -> CliResult {
    let Some(first) = raw.first().cloned() else {
        if !out.json {
            eprint!("{USAGE}");
        }
        return Err(CliError::usage("Missing command (klif-cli --help lists them)."));
    };
    match first.as_str() {
        "help" | "--help" | "-h" | "/?" => {
            out.doc(serde_json::json!({ "usage": USAGE }), || USAGE.to_string());
            return Ok(());
        }
        "--version" | "-V" | "version" => {
            let v = klif_core::klif_common::KLIF_VERSION;
            out.doc(serde_json::json!({ "version": v }), || format!("klif-cli {v}"));
            return Ok(());
        }
        _ => {}
    }
    raw.remove(0);
    let args = Args::new(raw);
    let loaded = klif_core::klif_common::config::load();
    match first.as_str() {
        "status" => cmds::status(args, &loaded, out),
        "diag" => cmds::diag(args, &loaded, out),
        "plan" => cmds::plan(args, &loaded, out),
        "select" => cmds::select(args, &loaded, out),
        "launch" => cmds::launch(args, &loaded, out),
        "stop" => cmds::stop(args, &loaded, out),
        "restart" => cmds::restart(args, &loaded, out),
        "dismiss" => cmds::dismiss(args, &loaded, out),
        "systems" => cmds::systems(args, &loaded, out),
        "nodes" => cmds::nodes(args, &loaded, out),
        "serve" => cmds::serve(args, &loaded, out),
        "presets" => presets::run(args, &loaded, out),
        "bench" => bench_cmd::run(args, &loaded, out),
        "models" => models::run(args, &loaded, out),
        "key" => keys_cmd::key(args, &loaded, out),
        "node" => keys_cmd::node(args, &loaded, out),
        other => Err(CliError::usage(format!("Unknown command \"{other}\" (klif-cli --help lists them)."))),
    }
}
