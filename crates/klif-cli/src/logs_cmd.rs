//! `logs <system> [--tail N] [--follow]`: the System's console as the engine keeps it (KLIF's own lines, then the
//! server's stdout and stderr merged in arrival order, ANSI stripped, secrets redacted; the last 200 lines). The
//! console is the engine's view model (`snapshot {focus}`), so it works the same for a local System, a System on
//! a node, and a System that is not running (the console of its last session).

use crate::args::Args;
use crate::cmds::find;
use crate::conn::{self, Conn};
use crate::out::{note, val, CliError, CliResult, Out};
use crate::outputs::{LogEvent, LogsDoc};
use crate::resolve::resolve;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::now_s;
use klif_core::klif_common::vm::{System, SystemId, SystemStatus};
use std::time::Duration;

/// Lines printed without `--tail`.
const DEFAULT_TAIL: usize = 100;
/// How often `--follow` looks at the console.
const POLL: Duration = Duration::from_millis(400);

/// The System holds a session (it is starting, running or stopping), or an external server answers.
fn running(s: &System) -> bool {
    s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Online | SystemStatus::Busy | SystemStatus::Stopping)
}

/// The lines of `now` that `prev` did not have: the console is a window that slides, so the longest suffix of
/// `prev` that starts `now` is what both share. `None`: nothing in common (the window moved on by more than its
/// size, or the console is another one).
fn new_lines<'a>(prev: &[String], now: &'a [String]) -> Option<&'a [String]> {
    if prev.is_empty() {
        return Some(now);
    }
    (1..=prev.len().min(now.len())).rev().find(|k| prev[prev.len() - k..] == now[..*k]).map(|k| &now[k..])
}

/// The console of `id` (a node's System needs a moment: the node serves the console of the focused System only).
fn console(conn: &Conn, id: &SystemId) -> CliResult<(System, Vec<String>)> {
    let attempts = if id.is_remote() { 8 } else { 1 };
    for i in 0..attempts {
        let vm = conn.snapshot(Some(id))?;
        let s = find(&vm, id).cloned().ok_or_else(|| CliError::new("not_found", format!("System {id} disappeared.")))?;
        if !vm.console.is_empty() || i + 1 == attempts {
            return Ok((s, vm.console));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    unreachable!("the last attempt returns")
}

fn tail_of(lines: &[String], n: usize) -> &[String] {
    &lines[lines.len().saturating_sub(n)..]
}

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let tail = args.opt_num::<usize>("--tail")?;
    let follow = args.flag("--follow");
    let arg = args.pos("System")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let id = resolve(&conn.snapshot(None)?, &arg)?;
    let (system, mut lines) = console(&conn, &id)?;
    let n = tail.unwrap_or(DEFAULT_TAIL);
    if !follow {
        let shown = tail_of(&lines, n).to_vec();
        let doc = LogsDoc { system: id.clone(), label: system.label.clone(), status: system.status, lines: shown.clone() };
        out.doc(val(&doc), || {
            if shown.is_empty() {
                format!("{} has no console lines yet.", system.label)
            } else {
                shown.join("\n")
            }
        });
        return Ok(());
    }

    crate::out::set_streaming();
    let print = |line: &str| {
        let ev = LogEvent::Log { at: now_s(), system: id.clone(), line: line.to_string() };
        out.event(val(&ev), || line.to_string());
    };
    for line in tail_of(&lines, n) {
        print(line);
    }
    if !running(&system) {
        note(format!("{} is not running; the lines above are the console of its last session.", system.label));
        let ev = LogEvent::End { at: now_s(), system: id.clone(), status: system.status };
        out.event(val(&ev), || format!("-- {} is {} --", system.label, system.status.as_str()));
        return Ok(());
    }
    loop {
        std::thread::sleep(POLL);
        let (s, now) = console(&conn, &id)?;
        let live = running(&s);
        // Once the session ended the console becomes the idle one; only lines that continue the live console count.
        match new_lines(&lines, &now) {
            Some(fresh) => fresh.iter().for_each(|l| print(l)),
            None if live => now.iter().for_each(|l| print(l)),
            None => {}
        }
        lines = now;
        if !live {
            let ev = LogEvent::End { at: now_s(), system: id.clone(), status: s.status };
            out.event(val(&ev), || format!("-- {} is {} --", s.label, s.status.as_str()));
            return Ok(());
        }
    }
}
