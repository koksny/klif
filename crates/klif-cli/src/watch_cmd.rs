//! `watch [--system S] [--types LIST] [--until SYSTEM=STATUS] [--timeout SECONDS]`: what happens in the engine, one
//! event per line (JSON lines with `--json`). It polls the engine's view model twice a second and tells the
//! differences: status changes per System, sessions that started or ended, faults, broken records and model
//! downloads. An agent waits with `watch --until s2=online` instead of sleeping and polling `status`.

use crate::args::Args;
use crate::cmds::{fault_error, find};
use crate::conn;
use crate::out::{note, val, CliError, CliResult, Out};
use crate::outputs::{WatchEvent, WatchSystem};
use crate::records_cmd::{metric_id, value_text};
use crate::resolve::resolve;
use klif_core::klif_common::now_s;
use klif_core::klif_common::vm::{DownloadState, Ended, Phase, RecordMetric, System, SystemId, SystemStatus, ViewModel};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(500);
/// A file's download progress is told at most this often (state changes at once).
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
/// Log lines carried by a `fault` event.
const FAULT_TAIL: usize = 12;
const TYPES: [&str; 6] = ["status", "fault", "launch", "stop", "record", "download"];
const STATUSES: [SystemStatus; 9] = [
    SystemStatus::NotSet,
    SystemStatus::Invalid,
    SystemStatus::Offline,
    SystemStatus::Starting,
    SystemStatus::Online,
    SystemStatus::Busy,
    SystemStatus::Stopping,
    SystemStatus::Fault,
    SystemStatus::Unreachable,
];

/// What the last poll showed of a System.
struct Seen {
    status: SystemStatus,
    has_session: bool,
    uptime_s: f64,
    fault: Option<String>,
}

impl Seen {
    fn of(s: &System) -> Seen {
        Seen {
            status: s.status,
            has_session: s.session.is_some(),
            uptime_s: s.session.as_ref().map(|x| x.uptime_s).unwrap_or(0.0),
            fault: s.session.as_ref().and_then(|x| x.fault.as_ref()).map(|f| f.title.clone()),
        }
    }
}

fn parse_types(v: Option<String>) -> CliResult<BTreeSet<&'static str>> {
    let Some(v) = v else { return Ok(TYPES.into_iter().collect()) };
    let mut set = BTreeSet::new();
    for t in v.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()) {
        match TYPES.iter().find(|x| **x == t) {
            Some(x) => {
                set.insert(*x);
            }
            None => return Err(CliError::usage(format!("Unknown event type \"{t}\" ({}).", TYPES.join(", ")))),
        }
    }
    if set.is_empty() {
        return Err(CliError::usage(format!("--types needs at least one of {}.", TYPES.join(", "))));
    }
    Ok(set)
}

/// `s2=online` -> ("s2", Online).
fn parse_until(v: &str) -> CliResult<(String, SystemStatus)> {
    let usage = || CliError::usage(format!("--until needs <system>=<status>, e.g. s2=online (statuses: {}).", STATUSES.map(|s| s.as_str()).join(", ")));
    let (system, status) = v.rsplit_once('=').ok_or_else(usage)?;
    let status = status.trim().to_ascii_lowercase();
    let status = STATUSES.iter().copied().find(|s| s.as_str() == status).ok_or_else(usage)?;
    if system.trim().is_empty() {
        return Err(usage());
    }
    Ok((system.trim().to_string(), status))
}

/// The System has reached `target`. `online` is a live server: also a busy one, and a loaded one (not while it
/// still loads), as `launch --wait` counts it.
fn reached(s: &System, target: SystemStatus) -> bool {
    match target {
        SystemStatus::Online => {
            matches!(s.status, SystemStatus::Online | SystemStatus::Busy) && (s.external || s.session.as_ref().is_none_or(|x| x.phase == Phase::Live))
        }
        t => s.status == t,
    }
}

fn kind(ev: &WatchEvent) -> &'static str {
    match ev {
        WatchEvent::Watching { .. } => "watching",
        WatchEvent::Status { .. } => "status",
        WatchEvent::Launch { .. } => "launch",
        WatchEvent::Stop { .. } => "stop",
        WatchEvent::Fault { .. } => "fault",
        WatchEvent::Record { .. } => "record",
        WatchEvent::Download { .. } => "download",
        WatchEvent::Until { .. } => "until",
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.2}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// The readable line of an event; `t` = seconds since the watch began.
fn text(ev: &WatchEvent, t: f64) -> String {
    let stamp = format!("[+{t:6.1}s]");
    match ev {
        WatchEvent::Watching { systems, until, .. } => {
            let list = systems.iter().map(|s| format!("{} {}", s.id, s.status.as_str())).collect::<Vec<_>>().join(", ");
            let wait = until.as_ref().map(|u| format!(", waiting for {u}")).unwrap_or_default();
            format!("watching: {}{wait} (Ctrl-C ends it)", if list.is_empty() { "no Systems".to_string() } else { list })
        }
        WatchEvent::Status { system, label, from, to, reason, .. } => format!(
            "{stamp} {system} ({label}): {} -> {}{}",
            from.map(|f| f.as_str()).unwrap_or("new"),
            to.as_str(),
            reason.as_deref().map(|r| format!(" - {r}")).unwrap_or_default()
        ),
        WatchEvent::Launch { system, preset, model, endpoint, .. } => {
            let mut what: Vec<String> = Vec::new();
            if let Some(p) = preset {
                what.push(format!("preset {p}"));
            }
            if let Some(m) = model {
                what.push(m.clone());
            }
            if let Some(e) = endpoint {
                what.push(e.clone());
            }
            format!("{stamp} {system} started{}", if what.is_empty() { String::new() } else { format!(" ({})", what.join(", ")) })
        }
        WatchEvent::Stop { system, ended, uptime_s, .. } => format!(
            "{stamp} {system} {}{}",
            match ended {
                Some(Ended::Fault) => "ended after a fault",
                _ => "stopped",
            },
            uptime_s.map(|u| format!(" after {u:.0} s")).unwrap_or_default()
        ),
        WatchEvent::Fault { system, title, exit_code, log_tail, .. } => {
            let mut s = format!("{stamp} {system} FAULT: {title}{}", exit_code.map(|c| format!(" (exit code {c})")).unwrap_or_default());
            for l in log_tail.iter().rev().take(6).rev() {
                s.push_str(&format!("\n  | {l}"));
            }
            s
        }
        WatchEvent::Record { metric, old, new, model, quant, backend, .. } => {
            let m = metric.unwrap_or(RecordMetric::DecodeTps);
            let who = format!(
                "{}{}{}",
                model.as_deref().unwrap_or("?"),
                quant.as_deref().map(|q| format!(" {q}")).unwrap_or_default(),
                backend.as_deref().filter(|b| !b.is_empty()).map(|b| format!(" on {b}")).unwrap_or_default()
            );
            format!(
                "{stamp} record {}: {who} {}{}",
                metric_id(m),
                value_text(m, *new),
                old.map(|o| format!(" (was {})", value_text(m, o))).unwrap_or_default()
            )
        }
        WatchEvent::Download { id, file, state, done_bytes, total_bytes, error, .. } => {
            let st = serde_json::to_value(state).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            let progress = match total_bytes {
                Some(t) if *t > 0 => format!(" {:.1}% ({} / {} GiB)", *done_bytes as f64 * 100.0 / *t as f64, gib(*done_bytes), gib(*t)),
                _ => format!(" {} GiB", gib(*done_bytes)),
            };
            format!("{stamp} download {id} {file}: {st}{progress}{}", error.as_deref().map(|e| format!(" - {e}")).unwrap_or_default())
        }
        WatchEvent::Until { system, status, .. } => format!("{stamp} {system} is {}", status.as_str()),
    }
}

struct Watcher {
    out: Out,
    types: BTreeSet<&'static str>,
    t0: Instant,
    /// Only the status / launch / stop / fault events of this System.
    only: Option<SystemId>,
}

impl Watcher {
    fn emit(&self, ev: WatchEvent) {
        let k = kind(&ev);
        if !matches!(k, "watching" | "until") && !self.types.contains(k) {
            return;
        }
        if let Some(only) = &self.only {
            let system = match &ev {
                WatchEvent::Status { system, .. }
                | WatchEvent::Launch { system, .. }
                | WatchEvent::Stop { system, .. }
                | WatchEvent::Fault { system, .. } => Some(system),
                _ => None,
            };
            if system.is_some_and(|s| s != only) {
                return;
            }
        }
        let t = self.t0.elapsed().as_secs_f64();
        self.out.event(val(&ev), || text(&ev, t));
    }
}

/// The events between two polls, for every System.
fn system_events(seen: &mut BTreeMap<SystemId, Seen>, vm: &ViewModel, now: f64) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    for s in &vm.systems {
        let cur = Seen::of(s);
        let (system, label) = (s.id.clone(), s.label.clone());
        let status = |from: Option<SystemStatus>| WatchEvent::Status { at: now, system: system.clone(), label: label.clone(), from, to: cur.status, reason: s.reason.clone() };
        let launch = || WatchEvent::Launch {
            at: now,
            system: system.clone(),
            label: label.clone(),
            preset: s.session.as_ref().and_then(|x| x.preset.clone()).or_else(|| s.preset.clone()),
            model: Some(s.model.name.trim().to_string()).filter(|m| !m.is_empty()),
            endpoint: s.endpoint.clone(),
        };
        let stop = || {
            // The last session's own facts, when they are fresh (they belong to the session that just ended).
            let last = s.last_session.as_ref().filter(|l| l.ended_ago_s < 15.0);
            WatchEvent::Stop { at: now, system: system.clone(), label: label.clone(), ended: last.map(|l| l.ended), uptime_s: last.map(|l| l.uptime_s) }
        };
        match seen.get(&s.id) {
            None => {
                events.push(status(None));
                if cur.has_session {
                    events.push(launch());
                }
            }
            Some(old) => {
                if old.status != cur.status {
                    events.push(status(Some(old.status)));
                }
                if !old.has_session && cur.has_session {
                    events.push(launch());
                } else if old.has_session && !cur.has_session {
                    events.push(stop());
                } else if old.has_session && cur.has_session && cur.uptime_s + 1.5 < old.uptime_s {
                    // A new session between two polls.
                    events.push(stop());
                    events.push(launch());
                }
            }
        }
        let old_fault = seen.get(&s.id).and_then(|o| o.fault.clone());
        if let (Some(title), true) = (cur.fault.as_ref(), cur.fault != old_fault) {
            let f = s.session.as_ref().and_then(|x| x.fault.as_ref());
            events.push(WatchEvent::Fault {
                at: now,
                system: system.clone(),
                label: label.clone(),
                title: title.clone(),
                exit_code: f.and_then(|f| f.exit_code),
                log_tail: f.map(|f| f.log_tail.iter().rev().take(FAULT_TAIL).rev().cloned().collect()).unwrap_or_default(),
            });
        }
        seen.insert(s.id.clone(), cur);
    }
    events
}

type RecordKey = (String, Option<RecordMetric>, u64);

fn record_key(e: &klif_core::klif_common::vm::RecordEvent) -> RecordKey {
    (e.key.clone(), e.metric, e.at.to_bits())
}

fn record_events(seen: &mut BTreeSet<RecordKey>, vm: &ViewModel) -> Vec<WatchEvent> {
    let mut fresh: Vec<&klif_core::klif_common::vm::RecordEvent> = vm.record_events.iter().filter(|e| !seen.contains(&record_key(e))).collect();
    fresh.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    fresh
        .into_iter()
        .map(|e| {
            seen.insert(record_key(e));
            let entry = vm.records.iter().find(|r| r.key == e.key);
            WatchEvent::Record {
                at: e.at,
                key: e.key.clone(),
                metric: e.metric,
                old: e.old,
                new: e.new,
                model: entry.map(|r| r.model.name.clone()),
                quant: entry.and_then(|r| r.model.quant.clone()),
                backend: entry.map(|r| r.backend.clone()),
                machine: entry.map(|r| r.machine.clone()),
            }
        })
        .collect()
}

/// A download file's last told state and when.
struct Told {
    state: DownloadState,
    done: u64,
    at: Instant,
}

fn download_events(told: &mut BTreeMap<(String, String), Told>, vm: &ViewModel, now: f64) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    for d in &vm.downloads {
        let key = (d.id.clone(), d.file.clone());
        let tell = match told.get(&key) {
            None => true,
            Some(t) if t.state != d.state => true,
            Some(t) => t.done != d.done_bytes && t.at.elapsed() >= PROGRESS_EVERY && d.state != DownloadState::Done,
        };
        if !tell {
            continue;
        }
        told.insert(key, Told { state: d.state, done: d.done_bytes, at: Instant::now() });
        events.push(WatchEvent::Download {
            at: now,
            id: d.id.clone(),
            file: d.file.clone(),
            state: d.state,
            done_bytes: d.done_bytes,
            total_bytes: d.total_bytes,
            error: d.error.clone(),
        });
    }
    events
}

pub fn run(mut args: Args, loaded: &klif_core::klif_common::config::LoadedConfig, out: Out) -> CliResult {
    let system = args.opt("--system")?;
    let types = args.opt("--types")?;
    let until = args.opt("--until")?;
    let timeout = args.opt_num::<f64>("--timeout")?;
    args.done()?;
    let types = parse_types(types)?;
    let until = until.as_deref().map(parse_until).transpose()?;
    crate::out::set_streaming();
    if let Some(t) = timeout {
        if !(t.is_finite() && t > 0.0) {
            return Err(CliError::usage("--timeout needs a number of seconds above 0."));
        }
    }
    let conn = conn::connect(loaded)?;
    let mut vm = conn.snapshot(None)?;
    let only = system.as_deref().map(|s| resolve(&vm, s)).transpose()?;
    let until = until.map(|(s, st)| resolve(&vm, &s).map(|id| (id, st))).transpose()?;

    let w = Watcher { out, types: types.clone(), t0: Instant::now(), only };
    let mut seen: BTreeMap<SystemId, Seen> = vm.systems.iter().map(|s| (s.id.clone(), Seen::of(s))).collect();
    let mut records: BTreeSet<RecordKey> = vm.record_events.iter().map(record_key).collect();
    let mut told: BTreeMap<(String, String), Told> =
        vm.downloads.iter().map(|d| ((d.id.clone(), d.file.clone()), Told { state: d.state, done: d.done_bytes, at: Instant::now() })).collect();
    w.emit(WatchEvent::Watching {
        at: now_s(),
        systems: vm.systems.iter().map(|s| WatchSystem { id: s.id.clone(), label: s.label.clone(), status: s.status }).collect(),
        types: TYPES.iter().filter(|t| types.contains(*t)).map(|t| t.to_string()).collect(),
        until: until.as_ref().map(|(id, st)| format!("{id}={}", st.as_str())),
        timeout_s: timeout,
    });
    if conn.mode() == "in-process" {
        note("note: no KLIF is running, so klif-cli runs the engine while it watches (model servers keep running when it ends).");
    }

    // For `--until X=online`: a fault ends the wait at once, and a stop is an error only after a start was seen
    // while watching (before that the System is just not launched yet).
    let mut started = false;
    let mut first = true;
    loop {
        if !first {
            std::thread::sleep(POLL);
            vm = conn.snapshot(None)?;
        }
        let now = now_s();
        let before: Option<(SystemStatus, bool)> = until.as_ref().and_then(|(id, _)| seen.get(id)).map(|s| (s.status, s.has_session));
        if !first {
            for ev in system_events(&mut seen, &vm, now) {
                w.emit(ev);
            }
            for ev in record_events(&mut records, &vm) {
                w.emit(ev);
            }
            for ev in download_events(&mut told, &vm, now) {
                w.emit(ev);
            }
        }
        if let Some((id, target)) = &until {
            let s = find(&vm, id).ok_or_else(|| CliError::new("not_found", format!("System {id} disappeared.")))?;
            if reached(s, *target) {
                w.emit(WatchEvent::Until { at: now_s(), system: id.clone(), status: s.status, elapsed_s: (w.t0.elapsed().as_secs_f64() * 10.0).round() / 10.0 });
                return Ok(());
            }
            let live_target = matches!(target, SystemStatus::Online | SystemStatus::Busy);
            // A System that is faulted cannot come online by itself (dismiss it, or launch it again, first).
            if live_target && s.status == SystemStatus::Fault {
                return Err(fault_error(s));
            }
            if !first {
                if s.status == SystemStatus::Starting || (s.session.is_some() && before.is_some_and(|(_, had)| !had)) {
                    started = true;
                }
                if live_target && started && matches!(s.status, SystemStatus::Offline | SystemStatus::Invalid | SystemStatus::NotSet) {
                    return Err(CliError::new(
                        "stopped",
                        format!("{} stopped before it was ready{}", s.label, s.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_else(|| ".".into())),
                    ));
                }
            }
        }
        first = false;
        if let Some(t) = timeout {
            if w.t0.elapsed().as_secs_f64() >= t {
                return match &until {
                    None => Ok(()),
                    Some((id, target)) => {
                        let (label, now_status) = find(&vm, id).map(|s| (s.label.clone(), s.status.as_str())).unwrap_or_else(|| (id.to_string(), "unknown"));
                        Err(CliError::new(
                            "timeout",
                            format!("{label} was not {} after {t:.0} s (it is {now_status}).", target.as_str()),
                        ))
                    }
                };
            }
        }
    }
}
