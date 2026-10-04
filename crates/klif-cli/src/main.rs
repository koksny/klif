//! klif-cli: KLIF from a terminal or a coding agent (SPEC section 8). Talks to the running engine over the
//! control channel (`klif_core::link::ControlClient`), else runs an in-process engine while holding engine.lock.
//!
//! `klif-cli [--json] <command>`: with `--json` stdout carries exactly one JSON document `{schemaVersion: 1, ...}`;
//! failures are `{schemaVersion: 1, error: {code, message}}` with exit code 1, usage errors exit 2. Nothing is
//! launched, stopped, removed or downloaded without `--yes`; a model server is never stopped because klif-cli exits.

mod args;
mod bench_cmd;
mod catalog;
mod cmds;
mod conn;
mod hardware_cmd;
mod keys_cmd;
mod logs_cmd;
mod models;
mod out;
mod outputs;
mod presets;
mod records_cmd;
mod resolve;
mod schema_cmd;
mod settings_cmd;
mod suggest_cmd;
mod watch_cmd;

use args::Args;
use out::{CliError, CliResult, Out};
use std::process::ExitCode;

/// Set by `--json`: the stderr progress of a download is then JSON lines too.
static JSON_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
            if r.target().starts_with("klif_core::download") && JSON_MODE.load(std::sync::atomic::Ordering::Relaxed) {
                let note = outputs::DownloadEvent::Note { at: klif_core::klif_common::now_s(), message: r.args().to_string() };
                eprintln!("{}", out::json_line(out::val(&note)));
            } else {
                eprintln!("  {}", r.args());
            }
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
    JSON_MODE.store(json, std::sync::atomic::Ordering::Relaxed);
    let result = run(raw, out);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            out.error(&e);
            ExitCode::from(e.exit)
        }
    }
}

/// `help [<command>]`: the text list, or one command's details; `--json`: the catalog (`HelpDoc`).
fn help(words: Vec<String>, out: Out) -> CliResult {
    if words.is_empty() {
        let doc = catalog::help_doc(None);
        out.doc(out::val(&doc), || doc.usage.clone());
        return Ok(());
    }
    let cmds = catalog::matching(&words);
    if cmds.is_empty() {
        return Err(CliError::new("not_found", format!("There is no command {q}{}{q} (klif-cli --help lists them).", words.join(" "), q = '"')));
    }
    out.doc(out::val(&catalog::help_doc(Some(&cmds))), || catalog::command_text(&cmds));
    Ok(())
}

fn run(mut raw: Vec<String>, out: Out) -> CliResult {
    let Some(first) = raw.first().cloned() else {
        if !out.json {
            eprint!("{}", catalog::usage_text());
        }
        return Err(CliError::usage("Missing command (klif-cli --help lists them)."));
    };
    match first.as_str() {
        "help" | "--help" | "-h" | "/?" => return help(raw[1..].to_vec(), out),
        "--version" | "-V" | "version" => {
            let v = klif_core::klif_common::KLIF_VERSION;
            out.doc(out::val(&outputs::VersionDoc { version: v.to_string() }), || format!("klif-cli {v}"));
            return Ok(());
        }
        // No configuration needed: the schemas describe klif-cli itself.
        "schema" => return schema_cmd::run(Args::new(raw[1..].to_vec()), out),
        _ => {}
    }
    raw.remove(0);
    let args = Args::new(raw);
    let loaded = klif_core::klif_common::config::load();
    match first.as_str() {
        "status" => cmds::status(args, &loaded, out),
        "diag" => cmds::diag(args, &loaded, out),
        "hardware" => hardware_cmd::run(args, &loaded, out),
        "plan" => cmds::plan(args, &loaded, out),
        "select" => cmds::select(args, &loaded, out),
        "launch" => cmds::launch(args, &loaded, out),
        "stop" => cmds::stop(args, &loaded, out),
        "restart" => cmds::restart(args, &loaded, out),
        "dismiss" => cmds::dismiss(args, &loaded, out),
        "settings" => settings_cmd::run(args, &loaded, out),
        "systems" => cmds::systems(args, &loaded, out),
        "nodes" => cmds::nodes(args, &loaded, out),
        "serve" => cmds::serve(args, &loaded, out),
        "presets" => presets::run(args, &loaded, out),
        "bench" => bench_cmd::run(args, &loaded, out),
        "logs" => logs_cmd::run(args, &loaded, out),
        "watch" => watch_cmd::run(args, &loaded, out),
        "records" => records_cmd::run(args, &loaded, out),
        "models" => models::run(args, &loaded, out),
        "suggest" => suggest_cmd::run(args, &loaded, out),
        "key" => keys_cmd::key(args, &loaded, out),
        "node" => keys_cmd::node(args, &loaded, out),
        other => Err(CliError::usage(format!("Unknown command \"{other}\" (klif-cli --help lists them)."))),
    }
}
