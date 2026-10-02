//! The engine: one session at a time on one inference GPU.
//!
//! Threads: the 2 Hz tick thread (this module), the telemetry sampler and probe threads (klif-telemetry),
//! and a short-lived stop thread per stop (TerminateJobObject + wait, up to 5 s). Host commands (`act`)
//! run on the caller's thread, take the state lock, and wake the tick thread so the result is published
//! at once.
//!
//! Lock order: `st` -> `telemetry` -> (telemetry's own lock). `vm` and `subs` are never held together with
//! `st`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, bail, Result};
use klif_catalog::{Catalog, LiveFacts, Recipes};
use klif_common::config::Config;
use klif_common::vm::{
    Availability, Backend, Endpoint, Ended, Fault, GpuMemory, HostInfo, ImageLive, LastSession, LlmLive, LoadProgress, LoadStep,
    LoadStepId, ModelRef, Phase, Recipe, RecipePatch, Session, Slot, SlotId, SlotKind, StepState, SystemStats, ViewModel,
};
use klif_common::{now_s, Secret};
use klif_supervisor::{Owned, PortOwner, ProcState, Supervisor};
use klif_telemetry::{Health, ServerSignals, Telemetry, TelemetrySnapshot, UlpsSetting, WatchSpec};

use crate::narrate::{dormant_line, wake_line};
use crate::oldstate::{self, OldState};
use crate::state::{self, PersistedLast, PersistedSession, PersistedState, SessionOrigin, STATE_VERSION};
use crate::timefmt;

const TICK: Duration = Duration::from_millis(500);
/// A fatal log line while the process stays up and the server is not ready: wait this long for it to exit.
const FATAL_GRACE_S: f64 = 10.0;
/// While idle, how often the old launcher's state file is checked for a new run to adopt.
const LEGACY_CHECK_S: f64 = 2.0;
const CONSOLE_LEN: usize = 200;
const LOG_TAIL_LEN: usize = 12;

/// A view-model subscriber (called on the engine thread).
type Subscriber = Arc<dyn Fn(&ViewModel) + Send + Sync>;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ------------------------------------------------------------------------------------------ state

struct FaultRec {
    title: String,
    exit_code: Option<i64>,
    exit_code_hex: Option<String>,
    log_tail: Vec<String>,
    at: f64,
    steps: Option<Vec<LoadStep>>,
}

/// The session KLIF owns right now.
struct Sess {
    p: PersistedSession,
    owned: Arc<Owned>,
    phase: Phase,
    fault: Option<FaultRec>,
    /// When Stop was requested (uptime ends there).
    stop_at: Option<f64>,
    stopper: Option<JoinHandle<Result<()>>>,
    /// Cleanup of what is left of a faulted session's tree.
    cleanup: Option<JoinHandle<Result<()>>>,
    fatal_since: Option<f64>,
    /// KLIF's own lines shown before the logs (launch facts) and after them (stop / fault notes).
    header: Vec<String>,
    tail: Vec<String>,
    been_live: bool,
    /// Every PID seen in the session's tree: a dying member still listed in the TCP table is not foreign.
    known_pids: BTreeSet<u32>,
    last_llm: Option<LlmLive>,
    last_image: Option<ImageLive>,
    median_tps: Option<f64>,
    /// Dormant-GPU episodes already narrated in the console (entry, wake).
    dormant_logged: u64,
    wake_logged: u64,
}

impl Sess {
    fn new(p: PersistedSession, owned: Owned, phase: Phase, header: Vec<String>) -> Sess {
        Sess {
            p,
            owned: Arc::new(owned),
            phase,
            fault: None,
            stop_at: None,
            stopper: None,
            cleanup: None,
            fatal_since: None,
            header,
            tail: Vec::new(),
            been_live: false,
            known_pids: BTreeSet::new(),
            last_llm: None,
            last_image: None,
            median_tps: None,
            dormant_logged: 0,
            wake_logged: 0,
        }
    }

    fn started_at(&self) -> f64 {
        self.p.record.started_at
    }
}

pub(crate) struct State {
    sup: Supervisor,
    recipes: Recipes,
    /// Tiers whose recipe the user tuned (only these are persisted; the others follow klif.toml / presets).
    tuned: BTreeSet<SlotId>,
    selected: SlotId,
    host: HostInfo,
    session: Option<Sess>,
    /// Bumped whenever a session begins or ends (the telemetry snapshot must then be re-taken).
    session_seq: u64,
    last: Option<PersistedLast>,
    tier_ports: BTreeMap<SlotId, u16>,
    port_owners: BTreeMap<u16, PortOwner>,
    pending_launch: Option<SlotId>,
    /// Startup notices (shown in the console while idle).
    notes: Vec<String>,
    /// KLIF lines after the last session's logs (while idle).
    idle_tail: Vec<String>,
    old_mtime: Option<SystemTime>,
    last_legacy_check: f64,
}

pub(crate) struct Inner {
    pub(crate) cfg: Config,
    catalog: Catalog,
    telemetry: Mutex<Option<Telemetry>>,
    st: Mutex<State>,
    vm: Mutex<ViewModel>,
    subs: Mutex<Vec<Subscriber>>,
    wake: Mutex<bool>,
    wake_cv: Condvar,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

// ------------------------------------------------------------------------------------------ start

/// Recipes for every tier: the persisted ones, else the catalog defaults (config tiers / launcher presets
/// with the old launcher's KV, vision and mode).
pub(crate) fn initial_recipes(cfg: &Config, catalog: &Catalog, persisted: &PersistedState, old: Option<&OldState>) -> Recipes {
    let defaults = catalog.default_recipes(cfg, &old.map(|o| o.defaults.clone()).unwrap_or_default());
    SlotId::ALL
        .iter()
        .filter_map(|&slot| persisted.recipes.get(&slot).or_else(|| defaults.get(&slot)).cloned().map(|r| (slot, r)))
        .collect()
}

/// The port each tier's recipe would use (the plan's effective port; only the catalog knows pinned ports).
pub(crate) fn tier_ports(cfg: &Config, catalog: &Catalog, recipes: &Recipes) -> BTreeMap<SlotId, u16> {
    SlotId::ALL
        .iter()
        .map(|&slot| {
            let fallback = if slot.kind() == SlotKind::Image { 1234 } else { 7030 };
            let port = recipes
                .get(&slot)
                .and_then(|r| catalog.plan_unchecked(cfg, slot, r, None, timefmt::FIXED_STAMP).ok().map(|p| p.port).or(r.port))
                .unwrap_or(fallback);
            (slot, port)
        })
        .collect()
}

fn fallback_vm(host: HostInfo, selected: SlotId) -> ViewModel {
    ViewModel {
        now: now_s(),
        slots: Vec::new(),
        selected,
        session: None,
        vram: GpuMemory {
            device: String::new(),
            total_gib: 0.0,
            used_gib: 0.0,
            layers: Vec::new(),
            spill_mib: 0.0,
            history: Vec::new(),
            layer_history: None,
            baseline_gib: 0.0,
            warn_below_gib: 0.0,
            dormant: None,
        },
        system: SystemStats { ram_used_gib: 0.0, ram_total_gib: 0.0, ram_type: None, cpu_name: String::new(), cpu_pct: 0.0 },
        last_session: None,
        host,
        console: Vec::new(),
    }
}

pub(crate) fn start(cfg: Config, host: HostInfo) -> Result<Arc<Inner>> {
    let t0 = Instant::now();
    let catalog = Catalog::load(&cfg).map_err(|e| anyhow!("The launcher catalog could not be loaded: {e:#}"))?;
    log::info!("catalog loaded ({:?}) in {} ms", catalog.origin(), t0.elapsed().as_millis());
    let mut notes: Vec<String> = Vec::new();
    for w in catalog.warnings() {
        log::warn!("catalog: {w}");
    }
    if !catalog.warnings().is_empty() {
        notes.push(format!("[KLIF] catalog: {} warning(s), see the KLIF log", catalog.warnings().len()));
    }

    let state_file = cfg.launcher.state_file.clone();
    let old = oldstate::read(state_file.as_deref());
    let persisted = state::load(&cfg.state_path("state.json"));
    let recipes = initial_recipes(&cfg, &catalog, &persisted, old.as_ref());
    let selected = persisted.selected.unwrap_or(SlotId::Medium);
    let ports = tier_ports(&cfg, &catalog, &recipes);

    // Telemetry on the inference adapter (resolved by PCI id: LUIDs change every boot).
    let adapter = match cfg.gpu.inference.as_deref() {
        Some(pci) => {
            let a = klif_telemetry::find_adapter(pci);
            if a.is_none() {
                log::warn!("inference GPU {pci} not found among the DXGI adapters");
                notes.push(format!("[KLIF] inference GPU {pci} was not found; VRAM is not measured"));
            }
            a
        }
        None => {
            notes.push("[KLIF] no inference GPU configured (gpu.inference); VRAM is not measured".into());
            None
        }
    };
    let adapter = adapter.map(|mut a| {
        if let Some(name) = cfg.gpu.inference_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            a.name = name.to_string();
        }
        log::info!("inference adapter {} ({}), {} MiB", a.display_name(), a.pci_id(), a.dedicated_bytes >> 20);
        a
    });
    let telemetry = Telemetry::start(adapter, cfg.telemetry.warn_below_gib);

    let sup = Supervisor::new();
    if let Some(w) = sup.parent_job_warning() {
        log::warn!("{w}");
        notes.push(format!("[KLIF] warning: {w}"));
    }

    let st = State {
        sup,
        recipes,
        tuned: persisted.recipes.keys().copied().collect(),
        selected,
        host: host.clone(),
        session: None,
        session_seq: 0,
        last: persisted.last_session.clone(),
        tier_ports: ports,
        port_owners: BTreeMap::new(),
        pending_launch: None,
        notes,
        idle_tail: Vec::new(),
        old_mtime: oldstate::mtime(state_file.as_deref()),
        last_legacy_check: now_s(),
    };
    let inner = Arc::new(Inner {
        cfg,
        catalog,
        telemetry: Mutex::new(Some(telemetry)),
        st: Mutex::new(st),
        vm: Mutex::new(fallback_vm(host, selected)),
        subs: Mutex::new(Vec::new()),
        wake: Mutex::new(false),
        wake_cv: Condvar::new(),
        stop: AtomicBool::new(false),
        thread: Mutex::new(None),
    });

    // Adoption: a persisted KLIF session first, else the old launcher's live run. Never start anything.
    {
        let mut st = lock(&inner.st);
        let mut adopted = false;
        if let Some(ps) = persisted.session.clone() {
            adopted = inner.adopt_persisted(&mut st, ps);
        }
        if !adopted {
            if let Some(old) = &old {
                adopted = inner.adopt_legacy(&mut st, old);
            }
        }
        if !adopted && persisted.session.is_some() {
            log::info!("the persisted session is gone; forgetting it");
        }
        inner.persist(&st);
    }

    inner.tick();
    let worker = inner.clone();
    let h = std::thread::Builder::new().name("klif-engine".into()).spawn(move || worker.run())?;
    *lock(&inner.thread) = Some(h);
    log::info!("engine started in {} ms", t0.elapsed().as_millis());
    Ok(inner)
}

// ------------------------------------------------------------------------------------------ helpers

fn backend_of(s: &str) -> Option<Backend> {
    match s.trim().to_ascii_lowercase().as_str() {
        "hip" => Some(Backend::Hip),
        "vulkan" => Some(Backend::Vulkan),
        "cpu" => Some(Backend::Cpu),
        _ => None,
    }
}

fn display_host(host: &str) -> &str {
    match host.trim() {
        "" | "0.0.0.0" | "*" => "127.0.0.1",
        "::" => "::1",
        h => h,
    }
}

fn url_for(host: &str, port: u16) -> String {
    let h = display_host(host);
    if h.contains(':') && !h.starts_with('[') {
        format!("http://[{h}]:{port}/")
    } else {
        format!("http://{h}:{port}/")
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.retain(|x| x.is_finite() && *x > 0.0);
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn step_failed_detail(id: LoadStepId) -> &'static str {
    match id {
        LoadStepId::Process => "process did not start",
        LoadStepId::Device => "device listing failed",
        LoadStepId::Weights => "weights not loaded",
        LoadStepId::Kv => "allocation failed",
        LoadStepId::Warmup => "warm-up failed",
        LoadStepId::Ready => "never became ready",
    }
}

fn initial_steps(kind: SlotKind) -> Vec<LoadStep> {
    klif_telemetry::llama::STEP_IDS
        .iter()
        .enumerate()
        .map(|(i, id)| LoadStep {
            id: *id,
            label: klif_telemetry::llama::step_label(*id, kind == SlotKind::Image).to_string(),
            state: if i == 0 { StepState::Active } else { StepState::Pending },
            detail: None,
        })
        .collect()
}

/// The load steps at the moment the session died: the running step is marked failed.
fn fail_steps(mut steps: Vec<LoadStep>) -> Vec<LoadStep> {
    if steps.iter().any(|s| s.state == StepState::Failed) {
        return steps;
    }
    let idx = steps.iter().rposition(|s| s.state == StepState::Active).or_else(|| steps.iter().position(|s| s.state == StepState::Pending));
    if let Some(i) = idx {
        steps[i].state = StepState::Failed;
        steps[i].detail = Some(step_failed_detail(steps[i].id).to_string());
    }
    steps
}

/// Did the server get past "process spawned" (log lines, a device, an answering socket)?
fn progressed(sig: &ServerSignals) -> bool {
    sig.health != Health::Down
        || sig.load_steps.first().map(|s| s.state == StepState::Done).unwrap_or(false)
        || sig.load_steps.iter().skip(1).any(|s| s.state != StepState::Pending)
}

enum Cause {
    Exited(u32),
    Gone,
    Fatal,
}

fn build_fault(kind: SlotKind, phase: Phase, cause: &Cause, sig: Option<&ServerSignals>, console: &[String], now: f64) -> FaultRec {
    let loading = matches!(phase, Phase::Starting | Phase::Loading);
    let base = match cause {
        Cause::Exited(_) if loading => "The server exited while loading",
        Cause::Exited(_) => "The server exited unexpectedly",
        Cause::Gone if loading => "The server process disappeared while loading",
        Cause::Gone => "The server process disappeared",
        Cause::Fatal => "The server reported a fatal error and did not recover",
    };
    let hint = sig.and_then(|s| s.fatal_hint.clone());
    let title = match &hint {
        Some(h) => format!("{base}: {}.", h.trim().trim_end_matches('.')),
        None => format!("{base}."),
    };
    // The image starters print the server's own exit code; it says more than the starter's.
    let starter = sig.and_then(|s| s.starter_exit).filter(|c| *c != 0);
    let code: Option<i64> = starter.or(match cause {
        Cause::Exited(c) => Some(*c as i32 as i64),
        _ => None,
    });
    let exit_code_hex = code.map(|c| c as i32 as u32).filter(|u| u & 0x8000_0000 != 0).map(|u| format!("0x{u:08X}"));
    let err_tail: Vec<String> = sig.map(|s| s.error_tail.clone()).unwrap_or_default();
    let src: &[String] = if err_tail.len() >= 3 { &err_tail } else { console };
    let log_tail = src[src.len().saturating_sub(LOG_TAIL_LEN)..].to_vec();
    let steps = if loading {
        let steps = sig.map(|s| s.load_steps.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| initial_steps(kind));
        Some(fail_steps(steps))
    } else {
        None
    };
    FaultRec { title, exit_code: code, exit_code_hex, log_tail, at: now, steps }
}

fn spawn_stop(owned: Arc<Owned>, what: &'static str) -> Option<JoinHandle<Result<()>>> {
    std::thread::Builder::new()
        .name(format!("klif-{what}"))
        .spawn(move || {
            let mut sup = Supervisor::new();
            sup.stop(&owned)
        })
        .map_err(|e| log::error!("could not start the {what} thread: {e}"))
        .ok()
}

fn join_result(h: JoinHandle<Result<()>>) -> Result<()> {
    h.join().unwrap_or_else(|_| Err(anyhow!("the stop thread panicked")))
}

enum Outcome {
    Nothing,
    End(Ended),
    Fault(FaultRec, bool),
}

// ------------------------------------------------------------------------------------------ engine

impl Inner {
    fn tel<R>(&self, f: impl FnOnce(&Telemetry) -> R) -> Option<R> {
        lock(&self.telemetry).as_ref().map(f)
    }

    pub(crate) fn api_key(&self) -> Option<Secret> {
        oldstate::read_api_key(self.cfg.launcher.state_file.as_deref())
    }

    fn persist(&self, st: &State) {
        let ps = PersistedState {
            version: STATE_VERSION,
            recipes: st.recipes.iter().filter(|(k, _)| st.tuned.contains(k)).map(|(k, v)| (*k, v.clone())).collect(),
            selected: Some(st.selected),
            session: st.session.as_ref().filter(|s| s.phase != Phase::Fault).map(|s| s.p.clone()),
            last_session: st.last.clone(),
        };
        if let Err(e) = state::save(&self.cfg.state_path("state.json"), &ps) {
            log::warn!("could not write state.json: {e:#}");
        }
    }

    fn poke(&self) {
        *lock(&self.wake) = true;
        self.wake_cv.notify_all();
    }

    fn watch(&self, p: &PersistedSession, from_start: bool) {
        let api_key = if p.kind == SlotKind::Llm { self.api_key() } else { None };
        let spec = WatchSpec {
            kind: p.kind,
            out_log: p.record.out_log.clone(),
            err_log: p.record.err_log.clone(),
            host: display_host(&p.host).to_string(),
            port: p.record.port,
            api_key,
            started_at: p.record.started_at,
            ctx_tokens: p.ctx_tokens,
            spec_mode: p.spec_mode.clone(),
        };
        self.tel(|t| t.watch(spec, from_start));
    }

    fn begin_session(&self, st: &mut State, sess: Sess, from_start: bool) {
        self.watch(&sess.p, from_start);
        st.selected = sess.p.slot;
        st.session = Some(sess);
        st.session_seq += 1;
        st.idle_tail.clear();
        st.pending_launch = None;
    }

    // ---- adoption ---------------------------------------------------------------------------

    fn adopt_persisted(&self, st: &mut State, p: PersistedSession) -> bool {
        let Some(owned) = st.sup.adopt(&p.record) else { return false };
        log::info!(
            "adopted session {} (root pid {}, job {})",
            p.record.session_name,
            p.record.root_pid,
            if owned.has_job() { "open" } else { "none" }
        );
        let header = vec![
            format!("[KLIF] adopted {} (pid {}) after a KLIF restart", p.record.session_name, p.record.root_pid),
            format!("[KLIF] stdout: {}", p.record.out_log.display()),
            format!("[KLIF] stderr: {}", p.record.err_log.display()),
        ];
        let sess = Sess::new(p, owned, Phase::Loading, header);
        self.begin_session(st, sess, true);
        true
    }

    /// Adopt the old PowerShell GUI's live run (RuntimeProcesses), mapped to the tier its card sits in.
    fn adopt_legacy(&self, st: &mut State, old: &OldState) -> bool {
        if old.runtime.is_empty() || st.session.is_some() {
            return false;
        }
        let named = old
            .out_log
            .as_deref()
            .and_then(oldstate::parse_session_log_name)
            .or_else(|| old.err_log.as_deref().and_then(oldstate::parse_session_log_name));
        let card_id = named.as_ref().map(|n| n.0.clone());
        let log_port = named.as_ref().map(|n| n.1);
        let slot = card_id
            .as_deref()
            .and_then(|c| self.catalog.home_slot(c))
            .unwrap_or(if matches!(log_port, Some(1234) | Some(1235)) { SlotId::Krea } else { st.selected });

        // The recipe that run most likely used: the tier's, with the card, axes and context of the run.
        let base = st.recipes.get(&slot).cloned().unwrap_or_default();
        let mut patch = RecipePatch { card_id: card_id.clone(), ..RecipePatch::default() };
        patch.backend = old.backend.as_deref().and_then(backend_of);
        patch.hardware = old.hardware.clone();
        if slot.kind() == SlotKind::Image {
            patch.image_size = old.context.and_then(oldstate::image_size_label);
        } else {
            patch.ctx_tokens = old.context;
            patch.port = log_port;
        }
        let recipe = self.catalog.apply_patch(slot, &base, &patch);
        let plan = self.catalog.plan_unchecked(&self.cfg, slot, &recipe, None, timefmt::FIXED_STAMP).ok();
        let port = log_port.or(plan.as_ref().map(|p| p.port)).unwrap_or(if slot.kind() == SlotKind::Image { 1234 } else { 7030 });

        let Some(owned) = st.sup.adopt_legacy(&old.runtime, old.out_log.clone(), old.err_log.clone(), port) else {
            return false;
        };
        let record = owned.record.clone();
        let kind = slot.kind();
        let (model, host, spec_mode) = match &plan {
            Some(p) => (p.model.clone(), p.host.clone(), p.spec_mode.clone()),
            None => (
                ModelRef { name: card_id.clone().unwrap_or_else(|| "Unknown model".into()), ..ModelRef::default() },
                if kind == SlotKind::Image { self.cfg.net.image_host.clone() } else { self.cfg.net.llm_host.clone() },
                None,
            ),
        };
        let p = PersistedSession {
            record: record.clone(),
            origin: SessionOrigin::Legacy,
            slot,
            kind,
            card_id: card_id.unwrap_or_default(),
            ctx_tokens: model.ctx_tokens,
            model,
            recipe: Some(recipe),
            host,
            spec_mode,
            api_key_set: kind == SlotKind::Llm && self.api_key().is_some(),
        };
        log::info!(
            "adopted the old launcher's run {} as {} (root pid {}, port {})",
            record.session_name,
            slot.as_str(),
            record.root_pid,
            record.port
        );
        let header = vec![
            format!("[KLIF] adopted the old launcher's run {} (pid {})", record.session_name, record.root_pid),
            format!("[KLIF] stdout: {}", record.out_log.display()),
            format!("[KLIF] stderr: {}", record.err_log.display()),
        ];
        let sess = Sess::new(p, owned, Phase::Loading, header);
        self.begin_session(st, sess, true);
        true
    }

    /// While idle: if the old launcher's state file changed, look for a run it started.
    fn check_legacy(&self, st: &mut State, now: f64) {
        if st.session.is_some() || now - st.last_legacy_check < LEGACY_CHECK_S {
            return;
        }
        st.last_legacy_check = now;
        let path = self.cfg.launcher.state_file.clone();
        let m = oldstate::mtime(path.as_deref());
        if m.is_none() || m == st.old_mtime {
            return;
        }
        st.old_mtime = m;
        if let Some(old) = oldstate::read(path.as_deref()) {
            if self.adopt_legacy(st, &old) {
                self.persist(st);
            }
        }
    }

    // ---- actions ----------------------------------------------------------------------------

    pub(crate) fn act(&self, action: klif_common::vm::Action) -> Result<()> {
        let r = {
            let mut st = lock(&self.st);
            self.act_locked(&mut st, action)
        };
        self.poke();
        r
    }

    fn act_locked(&self, st: &mut State, action: klif_common::vm::Action) -> Result<()> {
        use klif_common::vm::Action;
        let current = st.session.as_ref().map(|s| (s.phase, s.p.slot));
        let running = matches!(current, Some((p, _)) if p != Phase::Fault);
        match action {
            Action::Select { slot } => {
                if running && current.map(|c| c.1) != Some(slot) {
                    bail!("Stop the running session to change the slot.");
                }
                st.selected = slot;
                self.persist(st);
                Ok(())
            }
            Action::Launch { slot } => {
                if running {
                    bail!("A session is already running. Stop it first.");
                }
                let id = slot.unwrap_or(st.selected);
                self.launch(st, id)
            }
            Action::Stop => match current {
                None => bail!("Nothing is running."),
                Some((Phase::Fault, _)) => self.dismiss(st),
                Some((Phase::Stopping, _)) => Ok(()),
                Some(_) => {
                    self.begin_stop(st);
                    Ok(())
                }
            },
            Action::Restart => match current {
                None => bail!("Nothing is running."),
                Some((Phase::Fault, slot)) => {
                    self.dismiss(st)?;
                    self.launch(st, slot)
                }
                Some((Phase::Stopping, slot)) => {
                    st.pending_launch = Some(slot);
                    Ok(())
                }
                Some((_, slot)) => {
                    self.begin_stop(st);
                    st.pending_launch = Some(slot);
                    Ok(())
                }
            },
            Action::Dismiss => match current {
                Some((Phase::Fault, _)) => self.dismiss(st),
                Some(_) => bail!("Stop the running session first."),
                None => Ok(()),
            },
            Action::SetRecipe { slot, patch } => {
                let cur = st.recipes.get(&slot).cloned().unwrap_or_default();
                let next = self.catalog.apply_patch(slot, &cur, &patch);
                st.recipes.insert(slot, next);
                st.tuned.insert(slot);
                st.tier_ports = tier_ports(&self.cfg, &self.catalog, &st.recipes);
                self.refresh_ports(st);
                self.persist(st);
                Ok(())
            }
        }
    }

    /// The slot as the UI sees it right now (fresh port facts).
    fn slot_now(&self, st: &mut State, id: SlotId) -> Option<Slot> {
        self.refresh_ports(st);
        self.slots_view(st).into_iter().find(|s| s.id == id)
    }

    /// The catalog's slots with live port facts; a busy port names the process that holds it.
    fn slots_view(&self, st: &State) -> Vec<Slot> {
        let live = Self::live_facts(st);
        let mut slots = self.catalog.slots(&st.recipes, &live);
        for s in slots.iter_mut() {
            if s.availability == Availability::Busy {
                if let Some(r) = self.busy_reason(st, s.id) {
                    s.reason = Some(r);
                }
            }
        }
        slots
    }

    fn launch(&self, st: &mut State, slot: SlotId) -> Result<()> {
        if st.session.as_ref().map(|s| s.phase == Phase::Fault).unwrap_or(false) {
            // Launching over a faulted session ends that one (it becomes the last session).
            self.dismiss(st)?;
        }
        if st.session.is_some() {
            bail!("A session is already running. Stop it first.");
        }
        let Some(view) = self.slot_now(st, slot) else { bail!("Unknown slot {}.", slot.as_str()) };
        if view.availability != Availability::Ready {
            let reason = view.reason.clone().unwrap_or_else(|| format!("{:?}.", view.availability));
            bail!("{} cannot start: {}", view.label, reason.trim_end_matches('.').to_string() + ".");
        }
        let recipe = st.recipes.get(&slot).cloned().ok_or_else(|| anyhow!("{} has no recipe.", slot.label()))?;
        let key = if slot.kind() == SlotKind::Llm { self.api_key() } else { None };
        let plan = self
            .catalog
            .plan(&self.cfg, slot, &recipe, key.as_ref(), timefmt::local_stamp())
            .map_err(|e| anyhow!("{} cannot start: {}.", slot.label(), e.to_string().trim_end_matches('.')))?;
        // One session at a time, and never on a port someone else holds.
        match st.sup.port_owner(plan.port, None) {
            PortOwner::Free => {}
            PortOwner::Ours { pid } | PortOwner::Foreign { pid, .. } => {
                let image = klif_supervisor::image_name(pid);
                bail!("Port {} is in use by {image} (pid {pid}). KLIF will not stop a process it did not start.", plan.port);
            }
        }
        let owned = st
            .sup
            .launch(&plan)
            .map_err(|e| anyhow!("{} could not be started: {}.", slot.label(), format!("{e:#}").trim_end_matches('.')))?;
        let record = owned.record.clone();
        log::info!(
            "launched {} as {} (pid {}, port {}, profile {})",
            record.session_name,
            slot.as_str(),
            record.root_pid,
            plan.port,
            plan.profile_key
        );
        let api_key_set = slot.kind() == SlotKind::Llm && key.is_some();
        drop(key);
        let header = vec![
            format!("[KLIF] started {} (powershell pid {})", record.session_name, record.root_pid),
            format!("[KLIF] stdout: {}", record.out_log.display()),
            format!("[KLIF] stderr: {}", record.err_log.display()),
        ];
        let p = PersistedSession {
            record,
            origin: SessionOrigin::Klif,
            slot,
            kind: slot.kind(),
            card_id: plan.card_id.clone(),
            ctx_tokens: plan.model.ctx_tokens,
            model: plan.model.clone(),
            recipe: Some(recipe),
            host: plan.host.clone(),
            spec_mode: plan.spec_mode.clone(),
            api_key_set,
        };
        let sess = Sess::new(p, owned, Phase::Starting, header);
        self.begin_session(st, sess, false);
        self.persist(st);
        Ok(())
    }

    fn begin_stop(&self, st: &mut State) {
        let Some(sess) = st.session.as_mut() else { return };
        if sess.phase == Phase::Stopping {
            return;
        }
        let now = now_s();
        log::info!("stopping {}", sess.p.record.session_name);
        sess.phase = Phase::Stopping;
        sess.stop_at = Some(now);
        sess.tail.push(format!("[KLIF] stopping {}", sess.p.record.session_name));
        sess.stopper = spawn_stop(sess.owned.clone(), "stop");
    }

    /// End a faulted session (it becomes the last session). Whatever is left of its tree is stopped first.
    fn dismiss(&self, st: &mut State) -> Result<()> {
        let Some(sess) = st.session.as_mut() else { return Ok(()) };
        if sess.phase != Phase::Fault {
            bail!("Stop the running session first.");
        }
        if let Some(h) = sess.cleanup.take() {
            if let Err(e) = join_result(h) {
                log::warn!("cleanup of {}: {e:#}", sess.p.record.session_name);
            }
        }
        if !st.sup.tree_pids(&sess.owned).is_empty() {
            st.sup.stop(&sess.owned).map_err(|e| anyhow!("The faulted session could not be cleaned up: {e:#}."))?;
        }
        st.pending_launch = None;
        self.end_session(st, Ended::Fault, now_s());
        Ok(())
    }

    fn end_session(&self, st: &mut State, ended: Ended, now: f64) {
        let Some(sess) = st.session.take() else { return };
        st.session_seq += 1;
        let (up_end, ended_at) = match (ended, &sess.fault) {
            (Ended::Fault, Some(f)) => (f.at, f.at),
            _ => (sess.stop_at.unwrap_or(now), now),
        };
        let llm = sess.last_llm.as_ref();
        let img = sess.last_image.as_ref();
        let decode = sess.median_tps.or_else(|| {
            median(llm.map(|l| l.requests.iter().filter(|r| r.decode_s > 0.0).map(|r| r.generated_tokens as f64 / r.decode_s).collect()).unwrap_or_default())
        });
        let summary = LastSession {
            slot: sess.p.slot,
            model: sess.p.model.clone(),
            uptime_s: (up_end - sess.started_at()).max(0.0).round(),
            ended_ago_s: 0.0,
            ended,
            requests: llm.map(|l| l.totals.requests),
            generated_tokens: llm.map(|l| l.totals.generated_tokens),
            decode_tps: decode.map(r1),
            images: img.map(|i| i.images_this_session),
            seconds_per_image: img.and_then(|i| median(i.recent.iter().map(|j| j.seconds).collect())).map(r1),
        };
        log::info!("session {} ended ({:?}) after {} s", sess.p.record.session_name, ended, summary.uptime_s);
        st.last = Some(PersistedLast { summary, ended_at });
        st.idle_tail = sess.tail.clone();
        st.idle_tail.push(format!(
            "[KLIF] {} {}",
            sess.p.record.session_name,
            if ended == Ended::Stopped { "stopped" } else { "ended with a fault" }
        ));
        self.tel(|t| t.unwatch());
        self.persist(st);
        if let Some(next) = st.pending_launch.take() {
            if let Err(e) = self.launch(st, next) {
                log::warn!("restart failed: {e:#}");
                st.idle_tail.push(format!("[KLIF] restart failed: {e}"));
            }
        }
    }

    // ---- tick -------------------------------------------------------------------------------

    fn run(self: Arc<Self>) {
        let mut next = Instant::now() + TICK;
        while !self.stop.load(Ordering::SeqCst) {
            {
                let mut woken = lock(&self.wake);
                while !*woken && !self.stop.load(Ordering::SeqCst) {
                    let left = next.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    woken = self.wake_cv.wait_timeout(woken, left).unwrap_or_else(|e| e.into_inner()).0;
                }
                *woken = false;
            }
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let t = Instant::now();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick()));
            if r.is_err() {
                log::error!("engine tick panicked");
            }
            let el = t.elapsed();
            if el > Duration::from_millis(250) {
                log::debug!("slow tick: {} ms", el.as_millis());
            }
            next = Instant::now().max(next + TICK);
            if next > Instant::now() + TICK {
                next = Instant::now() + TICK;
            }
        }
    }

    pub(crate) fn tick(&self) {
        let now = now_s();
        let vm = {
            let mut st = lock(&self.st);
            let seq = st.session_seq;
            let mut snap = self.tel(|t| t.snapshot());
            self.advance(&mut st, snap.as_ref(), now);
            self.check_legacy(&mut st, now);
            if st.session_seq != seq {
                snap = self.tel(|t| t.snapshot());
            }
            self.refresh_ports(&mut st);
            self.compose(&st, snap.as_ref(), now)
        };
        self.publish(vm);
    }

    fn publish(&self, vm: ViewModel) {
        *lock(&self.vm) = vm.clone();
        let subs: Vec<Subscriber> = lock(&self.subs).clone();
        for f in subs {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&vm))).is_err() {
                log::error!("a view-model subscriber panicked");
            }
        }
    }

    fn advance(&self, st: &mut State, snap: Option<&TelemetrySnapshot>, now: f64) {
        let outcome = {
            let State { sup, session, .. } = &mut *st;
            let Some(sess) = session.as_mut() else { return };
            let ps = sup.state(&sess.owned);
            let pids = match &ps {
                ProcState::Running { pids } => pids.clone(),
                _ => Vec::new(),
            };
            sess.known_pids.extend(pids.iter().copied());
            self.tel(|t| t.set_session_pids(pids));
            let sig = snap.and_then(|s| s.server.as_ref());
            let console: &[String] = snap.map(|s| s.console.as_slice()).unwrap_or(&[]);
            if let Some(sig) = sig {
                if matches!(sess.phase, Phase::Live | Phase::Stopping) {
                    if sig.llm.is_some() {
                        sess.last_llm = sig.llm.clone();
                    }
                    if sig.image.is_some() {
                        sess.last_image = sig.image.clone();
                    }
                    if sig.median_decode_tps.is_some() {
                        sess.median_tps = sig.median_decode_tps;
                    }
                }
            }
            if let Some(t) = snap {
                self.narrate_dormancy(sess, t);
            }
            match sess.phase {
                Phase::Stopping => {
                    if sess.stopper.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
                        match sess.stopper.take().map(join_result).unwrap_or(Ok(())) {
                            Ok(()) => Outcome::End(Ended::Stopped),
                            Err(e) => {
                                log::error!("stop failed: {e:#}");
                                let mut f = build_fault(sess.p.kind, Phase::Live, &Cause::Gone, sig, console, now);
                                f.title = format!("KLIF could not stop the session: {}.", format!("{e:#}").trim_end_matches('.'));
                                f.steps = None;
                                Outcome::Fault(f, false)
                            }
                        }
                    } else {
                        Outcome::Nothing
                    }
                }
                Phase::Fault => {
                    if sess.cleanup.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                        if let Some(h) = sess.cleanup.take() {
                            match join_result(h) {
                                Ok(()) => sess.tail.push("[KLIF] the rest of the session's processes were stopped".into()),
                                Err(e) => sess.tail.push(format!("[KLIF] cleanup failed: {e:#}")),
                            }
                        }
                    }
                    Outcome::Nothing
                }
                Phase::Starting | Phase::Loading | Phase::Live => match ps {
                    ProcState::Exited { code } => {
                        let left = !sup.tree_pids(&sess.owned).is_empty();
                        Outcome::Fault(build_fault(sess.p.kind, sess.phase, &Cause::Exited(code), sig, console, now), left)
                    }
                    ProcState::Gone => Outcome::Fault(build_fault(sess.p.kind, sess.phase, &Cause::Gone, sig, console, now), false),
                    ProcState::Running { .. } => {
                        let health = sig.map(|s| s.health).unwrap_or(Health::Down);
                        let fatal = sig.map(|s| s.fatal_hint.is_some()).unwrap_or(false);
                        let mut out = Outcome::Nothing;
                        if fatal && health != Health::Ready {
                            let since = *sess.fatal_since.get_or_insert(now);
                            if now - since >= FATAL_GRACE_S {
                                out = Outcome::Fault(build_fault(sess.p.kind, sess.phase, &Cause::Fatal, sig, console, now), true);
                            }
                        } else {
                            sess.fatal_since = None;
                        }
                        if matches!(out, Outcome::Nothing) {
                            if health == Health::Ready && sess.phase != Phase::Live {
                                log::info!("{} is live", sess.p.record.session_name);
                                sess.phase = Phase::Live;
                                sess.been_live = true;
                            } else if sess.phase == Phase::Starting && sig.map(progressed).unwrap_or(false) {
                                sess.phase = Phase::Loading;
                            }
                        }
                        out
                    }
                },
            }
        };
        match outcome {
            Outcome::Nothing => {}
            Outcome::End(ended) => self.end_session(st, ended, now),
            Outcome::Fault(f, cleanup) => {
                let Some(sess) = st.session.as_mut() else { return };
                log::warn!("{} faulted: {}", sess.p.record.session_name, f.title);
                sess.tail.push(format!("[KLIF] fault: {}", f.title));
                sess.phase = Phase::Fault;
                sess.fault = Some(f);
                sess.stopper = None;
                if cleanup {
                    sess.tail.push("[KLIF] stopping what is left of the session's processes".into());
                    sess.cleanup = spawn_stop(sess.owned.clone(), "cleanup");
                }
                st.pending_launch = None;
                self.persist(st);
            }
        }
    }

    /// The inference card's EnableUlps (read-only registry check; None without a configured card).
    pub(crate) fn ulps(&self) -> Option<UlpsSetting> {
        self.cfg.gpu.inference.as_deref().and_then(klif_telemetry::ulps_setting)
    }

    /// Console lines when the inference GPU goes dormant with the session loaded, and when it is back.
    fn narrate_dormancy(&self, sess: &mut Sess, snap: &TelemetrySnapshot) {
        let f = &snap.dormancy;
        let dev = match snap.vram.device.trim() {
            "" => "The inference GPU",
            d => d,
        };
        if let Some(e) = f.last_entry.as_ref().filter(|e| e.episode > sess.dormant_logged) {
            sess.dormant_logged = e.episode;
            // Only a live session sleeps; anything seen while stopping is the teardown.
            if sess.phase == Phase::Live {
                let ulps = self.ulps();
                log::info!(
                    "inference GPU dormant (episode {}): {:.2} GiB paged out, power {:?}, EnableUlps {:?} ({})",
                    e.episode,
                    e.paged_out_gib,
                    e.power,
                    ulps.as_ref().and_then(|u| u.enable_ulps),
                    ulps.as_ref().map(|u| u.key.as_str()).unwrap_or("no driver key")
                );
                let line = dormant_line(dev, e, ulps.as_ref().map(UlpsSetting::is_on).unwrap_or(false));
                self.tel(|t| t.note(line));
            } else {
                sess.wake_logged = e.episode;
            }
        }
        if let Some(w) = f.last_wake.as_ref().filter(|w| w.episode > sess.wake_logged) {
            sess.wake_logged = w.episode;
            log::info!("inference GPU awake (episode {}): {:.1} s, {:.2} GiB resident", w.episode, w.seconds, w.resident_gib);
            let line = wake_line(dev, w);
            self.tel(|t| t.note(line));
        }
    }

    fn refresh_ports(&self, st: &mut State) {
        let mut ports: BTreeSet<u16> = st.tier_ports.values().copied().collect();
        if let Some(s) = &st.session {
            ports.insert(s.p.record.port);
        }
        let ours = st.session.as_ref().map(|s| s.owned.clone());
        let known = st.session.as_ref().map(|s| &s.known_pids);
        let owners: BTreeMap<u16, PortOwner> = ports
            .into_iter()
            .map(|p| {
                let o = match st.sup.port_owner(p, ours.as_deref()) {
                    PortOwner::Foreign { pid, .. } if known.is_some_and(|k| k.contains(&pid)) => PortOwner::Ours { pid },
                    o => o,
                };
                (p, o)
            })
            .collect();
        st.port_owners = owners;
    }

    fn live_facts(st: &State) -> LiveFacts {
        LiveFacts {
            foreign_ports: st.port_owners.iter().filter(|(_, o)| matches!(o, PortOwner::Foreign { .. })).map(|(p, _)| *p).collect(),
        }
    }

    fn busy_reason(&self, st: &State, slot: SlotId) -> Option<String> {
        let port = *st.tier_ports.get(&slot)?;
        match st.port_owners.get(&port)? {
            PortOwner::Foreign { pid, image } => {
                Some(format!("Port {port} is in use by {image} (pid {pid}), which KLIF did not start"))
            }
            _ => None,
        }
    }

    fn compose(&self, st: &State, snap: Option<&TelemetrySnapshot>, now: f64) -> ViewModel {
        let slots = self.slots_view(st);
        let (vram, system) = match snap {
            Some(t) => (t.vram.clone(), t.system.clone()),
            None => {
                let prev = lock(&self.vm);
                (prev.vram.clone(), prev.system.clone())
            }
        };
        let sig = snap.and_then(|s| s.server.as_ref());
        let session = st.session.as_ref().map(|s| self.session_vm(s, sig, now));

        let mut console: Vec<String> = Vec::new();
        match &st.session {
            Some(s) => console.extend(s.header.iter().cloned()),
            None => console.extend(st.notes.iter().cloned()),
        }
        if let Some(t) = snap {
            console.extend(t.console.iter().cloned());
        }
        match &st.session {
            Some(s) => console.extend(s.tail.iter().cloned()),
            None => console.extend(st.idle_tail.iter().cloned()),
        }
        if console.len() > CONSOLE_LEN {
            console.drain(..console.len() - CONSOLE_LEN);
        }

        let last_session = st.last.as_ref().map(|l| {
            let mut s = l.summary.clone();
            s.ended_ago_s = r1((now - l.ended_at).max(0.0));
            s
        });
        ViewModel {
            now,
            slots,
            selected: st.session.as_ref().map(|s| s.p.slot).unwrap_or(st.selected),
            session,
            vram,
            system,
            last_session,
            host: st.host.clone(),
            console,
        }
    }

    fn session_vm(&self, s: &Sess, sig: Option<&ServerSignals>, now: f64) -> Session {
        let end = match (&s.fault, s.phase) {
            (Some(f), Phase::Fault) => f.at,
            _ => now,
        };
        let uptime = r1((end - s.started_at()).max(0.0));
        let loading = matches!(s.phase, Phase::Starting | Phase::Loading).then(|| {
            let (steps, fraction) = match sig {
                Some(g) if !g.load_steps.is_empty() => (g.load_steps.clone(), g.load_fraction),
                _ => (initial_steps(s.p.kind), 0.0),
            };
            LoadProgress { steps, fraction: (fraction * 100.0).round() / 100.0, elapsed_s: uptime }
        });
        let fault = s.fault.as_ref().filter(|_| s.phase == Phase::Fault).map(|f| Fault {
            title: f.title.clone(),
            exit_code: f.exit_code,
            exit_code_hex: f.exit_code_hex.clone(),
            log_tail: f.log_tail.clone(),
            since_s: r1((now - f.at).max(0.0)),
            steps: f.steps.clone(),
        });
        let show_live = match s.phase {
            Phase::Live | Phase::Stopping => true,
            Phase::Fault => s.been_live,
            _ => false,
        };
        let (llm, image) = if show_live {
            match s.phase {
                // A faulted or stopping server says nothing new: show what it last said.
                Phase::Fault => (s.last_llm.clone(), s.last_image.clone()),
                _ => (
                    sig.and_then(|g| g.llm.clone()).or_else(|| s.last_llm.clone()),
                    sig.and_then(|g| g.image.clone()).or_else(|| s.last_image.clone()),
                ),
            }
        } else {
            (None, None)
        };
        Session {
            slot: s.p.slot,
            model: s.p.model.clone(),
            phase: s.phase,
            uptime_s: uptime,
            endpoint: Endpoint { host: display_host(&s.p.host).to_string(), port: s.p.record.port },
            api_key_set: s.p.api_key_set,
            loading,
            fault,
            llm: if s.p.kind == SlotKind::Llm { llm } else { None },
            image: if s.p.kind == SlotKind::Image { image } else { None },
        }
    }

    // ---- host-facing ------------------------------------------------------------------------

    pub(crate) fn snapshot(&self) -> ViewModel {
        lock(&self.vm).clone()
    }

    pub(crate) fn subscribe(&self, f: Box<dyn Fn(&ViewModel) + Send + Sync>) {
        lock(&self.subs).push(Arc::from(f));
    }

    pub(crate) fn set_host(&self, host: HostInfo) {
        lock(&self.st).host = host;
        self.poke();
    }

    pub(crate) fn endpoint_url(&self) -> Option<String> {
        let st = lock(&self.st);
        let s = st.session.as_ref().filter(|s| s.phase != Phase::Fault)?;
        Some(url_for(&s.p.host, s.p.record.port))
    }

    pub(crate) fn shutdown(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        self.poke();
        if let Some(h) = lock(&self.thread).take() {
            let _ = h.join();
        }
        {
            let st = lock(&self.st);
            self.persist(&st);
        }
        // Dropping the telemetry joins its threads. Servers are left running by design.
        let t = lock(&self.telemetry).take();
        drop(t);
        log::info!("engine stopped (servers keep running)");
    }

    /// Recipes and ports as the engine sees them (for tools).
    pub(crate) fn recipe(&self, slot: SlotId) -> Option<Recipe> {
        lock(&self.st).recipes.get(&slot).cloned()
    }

    pub(crate) fn state_dir(&self) -> PathBuf {
        self.cfg.state_dir.clone()
    }
}
