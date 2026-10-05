//! The engine: N Systems on this machine, each with its own session, running concurrently, plus external Systems
//! (watched only) and the Systems of remote nodes (merged in from `crate::nodes`). SPEC section 5 + 16.
//!
//! Threads: the 2 Hz tick thread (this module), telemetry's sampler and probe threads, the node clients (E3), the
//! control servers (E3), a short-lived stop thread per stop / fault cleanup (TerminateJobObject + waits) and the
//! download threads (E2). Host commands (`act`) run on the caller's thread.
//!
//! Locks (order): `act_serial` -> `tick_lock` -> `reload_lock` -> `persist_lock` -> `st` -> `telemetry` (->
//! telemetry's own lock). `loaded`, `catalog`, `published`, `subs`, `files`, `key_info`, `bench`, `downloads`,
//! `servers`, `ulps`, `gpu_ids`, `hardware`, `nodes_sig` are leaves: nothing else is locked while one of them is held. The node
//! hub is never called while `st` is held. Slow IO (spawning, stopping, store writes, state file writes, port
//! tables, network, the catalog's file metadata probes) never happens under `st`.
//!
//! Modules: `session` (per-System session state machine: phases, faults, dormancy narration, last session),
//! `conflicts` (holders, reservations, the port / exclusive / VRAM rules), `compose` (the view model: status
//! mapping, ghosts, remote merge, conveniences, float clamping), `actions` (every `Action`, pending launches,
//! routing to nodes, store writes, downloads), `watch` (config hot reload, GPUs, external watches, API key, bench
//! cache, port owners, the network listener), `record_capture` (records from live sessions and benches, their
//! view-model part; the store is `crate::records`, whose locks are leaves too).

mod actions;
mod compose;
mod conflicts;
mod record_capture;
mod session;
mod watch;

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use klif_catalog::Catalog;
use klif_common::config::{Config, HardwareCfg, LoadedConfig, PresetCfg};
use klif_common::now_s;
use klif_common::vm::{
    ApiKeyInfo, BenchSummary, CommandView, ConfigInfo, DownloadInfo, GpuMemory, HardwareInfo, HostInfo, Issue, IssueLevel, MachineStats,
    ModelArch, PresetDetail, SystemId, SystemStatus, ViewModel, VramLayer,
};
use klif_common::Secret;
use klif_supervisor::Supervisor;
use klif_telemetry::{Telemetry, UlpsSetting};

use crate::control::{ControlServer, NetworkServer, NodeAuth};
use crate::webui::{WebAssets, WebAuth, WebServer};
use crate::download::DownloadHandle;
use crate::nodes::NodeHub;
use crate::state::{self, PersistedLast, PersistedState, STATE_VERSION};
use crate::{keys, timefmt, wire, EngineHandle, StartError};

pub(crate) use session::SessionCtx;

const TICK: Duration = Duration::from_millis(500);
/// Console lines kept per System.
pub(crate) const CONSOLE_LEN: usize = 200;

/// A view-model subscriber (called on the engine thread).
type Subscriber = Arc<dyn Fn(&ViewModel) + Send + Sync>;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ------------------------------------------------------------------------------------------ state

/// A launch that waits for other Systems to stop (stop others, restart), or is being spawned right now.
/// Pending Systems are holders: they reserve their expected VRAM, their port and their GPU.
pub(crate) struct PendingLaunch {
    /// Systems that must be gone first (stopped one after the other, in this order).
    pub(crate) waiting_for: Vec<SystemId>,
    pub(crate) at: f64,
    /// The spawn is in progress (outside the state lock).
    pub(crate) spawning: bool,
    pub(crate) label: String,
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
    pub(crate) gpus: Vec<String>,
    pub(crate) exclusive: bool,
    pub(crate) expected_gib: Option<f64>,
    /// The holders are stopped by `[launch] on_conflict = "stop"`, not by an explicit stopOthers: a holder that
    /// became busy meanwhile is never stopped (SPEC 16.8); the launch is cancelled instead.
    pub(crate) auto: bool,
}

/// A permanently watched external System (preset `endpoint`).
pub(crate) struct ExternalWatch {
    /// What the watch was made from (re-watched when it changes).
    pub(crate) signature: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    /// When it was last seen coming online (uptime of the synthetic session).
    pub(crate) online_since: Option<f64>,
}

/// One measured GPU, as the conflict rules need it.
#[derive(Debug, Clone, Default)]
pub(crate) struct GpuFact {
    pub(crate) id: String,
    pub(crate) total: f64,
    pub(crate) used: f64,
}

pub(crate) struct State {
    pub(crate) selected: Option<SystemId>,
    pub(crate) host: HostInfo,
    pub(crate) sessions: BTreeMap<SystemId, SessionCtx>,
    pub(crate) pending: BTreeMap<SystemId, PendingLaunch>,
    pub(crate) externals: BTreeMap<SystemId, ExternalWatch>,
    pub(crate) last: BTreeMap<SystemId, PersistedLast>,
    pub(crate) arches: BTreeMap<String, ModelArch>,
    pub(crate) layers: BTreeMap<String, Vec<VramLayer>>,
    /// The console of a System's ended session (shown while it is idle).
    pub(crate) idle_tails: BTreeMap<SystemId, Vec<String>>,
    /// Engine notices (shown in idle consoles).
    pub(crate) notes: Vec<String>,
    /// Ports held by processes KLIF does not own: port -> (pid, image).
    pub(crate) foreign_ports: BTreeMap<u16, (u32, String)>,
    /// (host, port) of every local System's active preset (external: its endpoint).
    pub(crate) endpoints: BTreeMap<SystemId, (String, u16)>,
    pub(crate) gpu_facts: Vec<GpuFact>,
    /// Last measured GPUs / machine (used when a snapshot is missing).
    pub(crate) gpus_mem: Vec<GpuMemory>,
    pub(crate) machine: MachineStats,
    /// Issues the engine adds to the config's (e.g. the network listener could not start).
    pub(crate) engine_issues: Vec<Issue>,
    /// The persisted part changed (written by `flush`, outside the lock).
    pub(crate) dirty: bool,
    /// Bumped whenever a session begins or ends (the telemetry snapshot is then re-taken).
    pub(crate) session_seq: u64,
}

impl State {
    fn persisted(&self) -> PersistedState {
        PersistedState {
            version: STATE_VERSION,
            selected: self.selected.clone(),
            sessions: self
                .sessions
                .iter()
                // A faulted session is kept only while processes of it are left (they must stay adoptable).
                .filter(|(_, s)| s.phase != klif_common::vm::Phase::Fault || !s.pids.is_empty())
                .map(|(k, s)| (k.clone(), s.p.clone()))
                .collect(),
            last_sessions: self.last.clone(),
            arches: self.arches.clone(),
            layers: self.layers.clone(),
        }
    }

    /// The tab label of a System (config label, else a ghost label, else the id).
    pub(crate) fn label_of(&self, cfg: &Config, id: &SystemId) -> String {
        if let Some(l) = cfg.system_label(id.as_str()) {
            return l;
        }
        if let Some(p) = self.pending.get(id) {
            return p.label.clone();
        }
        ghost_label(id)
    }
}

pub(crate) fn ghost_label(id: &SystemId) -> String {
    format!("{id} (not in klif.toml)")
}

/// The console and VRAM view of one System (the conveniences when it is selected / focused).
#[derive(Clone)]
pub(crate) struct Focus {
    pub(crate) console: Vec<String>,
    pub(crate) vram: GpuMemory,
}

struct Published {
    vm: ViewModel,
    focus: BTreeMap<SystemId, Focus>,
}

/// The control servers this engine runs (E3).
#[derive(Default)]
struct Servers {
    local: Option<ControlServer>,
    /// `[node] listen` as served (None: not listening).
    listen: Option<String>,
    network: Option<NetworkServer>,
    auth: Option<Arc<NodeAuth>>,
    last_auth_refresh: f64,
    last_listen_try: f64,
    /// klif-webui: the address it serves on (None: off), the server, why it does not serve, the last start attempt.
    webui_addr: Option<String>,
    webui: Option<WebServer>,
    webui_error: Option<String>,
    last_webui_try: f64,
    /// The page's address(es) for Tune and the QR code, and when they were worked out.
    webui_urls: (f64, Vec<String>),
}

type Stat = (std::time::SystemTime, u64);

/// What the engine last saw of the files it watches.
#[derive(Default)]
struct FileWatch {
    /// klif.toml (None: missing).
    config: Option<Stat>,
    config_path: Option<PathBuf>,
    /// api-key.txt (source string, stat, when computed).
    key: Option<(String, Option<Stat>, f64)>,
}

/// The machine inventory (GPUs, CPU, RAM, FP32 TFLOPS) and the `[hardware]` it was computed with: read at start and
/// again when `[hardware]` changes, never per tick.
pub(crate) struct HardwareState {
    pub(crate) cfg: HardwareCfg,
    pub(crate) info: HardwareInfo,
    /// The suggested model per slot for `info` from the embedded pool (package B): computed with the inventory,
    /// never per tick.
    pub(crate) suggestions: Vec<klif_common::vm::Suggestion>,
}

impl HardwareState {
    pub(crate) fn read(cfg: &HardwareCfg) -> HardwareState {
        let info = klif_telemetry::hardware::detect(cfg);
        let suggestions = klif_catalog::suggest::suggest(&info, klif_catalog::recommend::embedded_pool());
        HardwareState { cfg: cfg.clone(), info, suggestions }
    }
}

/// Cached bench summaries (`bench::latest` reads files: refreshed every few seconds outside the state lock).
#[derive(Default)]
pub(crate) struct BenchCache {
    /// (preset id, command hash) -> (when read, latest).
    pub(crate) by_hash: BTreeMap<(String, String), (f64, Option<BenchSummary>)>,
    /// preset id -> (when read, latest for its default params).
    pub(crate) by_preset: BTreeMap<String, (f64, Option<BenchSummary>)>,
    /// What the last compose asked for.
    pub(crate) wanted: std::collections::BTreeSet<(String, String)>,
}

/// Downloads of recommendations (updated from the download threads).
#[derive(Default)]
pub(crate) struct Downloads {
    pub(crate) infos: BTreeMap<(String, String), (DownloadInfo, f64)>,
    pub(crate) handles: BTreeMap<String, DownloadHandle>,
    /// A file finished: the catalog (installed flags) is rebuilt on the next tick.
    pub(crate) rebuild: bool,
}

pub(crate) struct Inner {
    me: Weak<Inner>,
    loaded: Mutex<LoadedConfig>,
    catalog: Mutex<Arc<Catalog>>,
    telemetry: Mutex<Option<Telemetry>>,
    nodes: NodeHub,
    sup: Supervisor,
    st: Mutex<State>,
    published: Mutex<Published>,
    subs: Mutex<Vec<Subscriber>>,
    wake: Mutex<bool>,
    wake_cv: Condvar,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
    act_serial: Mutex<()>,
    tick_lock: Mutex<()>,
    /// Serializes klif.toml reloads (tick, own writes, ensure_config): stat, record and apply happen together, so a
    /// slower reload never applies an older file after a newer one was recorded.
    reload_lock: Mutex<()>,
    persist_lock: Mutex<()>,
    files: Mutex<FileWatch>,
    key_info: Mutex<ApiKeyInfo>,
    bench: Mutex<BenchCache>,
    downloads: Arc<Mutex<Downloads>>,
    servers: Mutex<Servers>,
    /// ULPS settings by GPU pci id, read lazily (the first time a dormancy episode is narrated).
    ulps: Mutex<BTreeMap<String, Option<UlpsSetting>>>,
    /// The GPU ids telemetry measures now.
    gpu_ids: Mutex<Vec<String>>,
    /// The machine inventory behind `ViewModel.hardware`.
    hardware: Mutex<HardwareState>,
    /// `[nodes.*]` as the hub was last configured with.
    nodes_sig: Mutex<String>,
    /// Best values per model file and backend (`crate::records`; its locks are leaves).
    records: crate::records::Records,
    /// klif-webui's paired devices and open pairing.
    web_auth: Arc<WebAuth>,
    /// The page's files, from the host (the window app); None: this engine serves no page.
    web_assets: Mutex<Option<WebAssets>>,
    /// `engine.lock`, held for the engine's lifetime.
    engine_lock: Mutex<Option<File>>,
    state_dir: PathBuf,
    data_dir: PathBuf,
}

pub(crate) fn empty_gpu(warn_below_gib: f64) -> GpuMemory {
    GpuMemory {
        id: String::new(),
        name: String::new(),
        device: String::new(),
        total_gib: 0.0,
        used_gib: 0.0,
        layers: Vec::new(),
        spill_mib: 0.0,
        history: Vec::new(),
        layer_history: None,
        baseline_gib: 0.0,
        warn_below_gib,
        dormant: None,
    }
}

pub(crate) fn empty_machine() -> MachineStats {
    MachineStats { ram_used_gib: 0.0, ram_total_gib: 0.0, ram_type: None, cpu_name: String::new(), cpu_pct: 0.0 }
}

fn fallback_vm(host: HostInfo) -> ViewModel {
    ViewModel {
        now: now_s(),
        systems: Vec::new(),
        selected: None,
        session: None,
        last_session: None,
        console: Vec::new(),
        vram: empty_gpu(0.0),
        gpus: Vec::new(),
        machine: empty_machine(),
        host,
        presets: Vec::new(),
        recommendations: Vec::new(),
        hardware: Default::default(),
        suggestions: Vec::new(),
        records: Vec::new(),
        record_events: Vec::new(),
        records_rev: 0,
        downloads: Vec::new(),
        config: ConfigInfo::default(),
        nodes: Vec::new(),
    }
}

// ------------------------------------------------------------------------------------------ start

/// Take `engine.lock` in `state_dir` (std `File::try_lock`). Held by another process -> `Busy` with the pid from
/// its `control.json` (0 when unknown).
fn take_engine_lock(state_dir: &Path) -> Result<File, StartError> {
    let path = state_dir.join(wire::LOCK_FILE);
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| StartError::Other(anyhow!("{} could not be opened: {e}", path.display())))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => Err(StartError::Busy { pid: control_pid(state_dir).unwrap_or(0) }),
        Err(std::fs::TryLockError::Error(e)) => Err(StartError::Other(anyhow!("{} could not be locked: {e}", path.display()))),
    }
}

/// The pid in `control.json` (written by the engine that holds the lock).
fn control_pid(state_dir: &Path) -> Option<u32> {
    let bytes = std::fs::read(state_dir.join(wire::CONTROL_FILE)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str::<wire::ControlFile>(text.trim_start_matches('\u{feff}')).ok().map(|c| c.pid)
}

pub(crate) fn start(loaded: LoadedConfig, host: HostInfo) -> Result<Arc<Inner>, StartError> {
    let t0 = Instant::now();
    let cfg = loaded.cfg.clone();
    let state_dir = cfg.state_dir.clone();
    let data_dir = cfg.data_dir.clone();
    std::fs::create_dir_all(&state_dir)
        .map_err(|e| StartError::Other(anyhow!("The KLIF folder {} could not be created: {e}", state_dir.display())))?;
    let lock_file = take_engine_lock(&state_dir)?;

    let catalog = Catalog::new(&cfg);
    let mut notes: Vec<String> = Vec::new();
    for i in &loaded.issues {
        match i.level {
            IssueLevel::Error => log::warn!("config: {}", i.text),
            IssueLevel::Warn => log::info!("config: {}", i.text),
        }
    }
    let errors = loaded.issues.iter().filter(|i| i.is_error()).count();
    if errors > 0 {
        notes.push(format!("[KLIF] klif.toml: {errors} error(s), see Tune or `klif-cli diag`"));
    }
    if let Some(w) = klif_supervisor::parent_job_warning() {
        log::warn!("{w}");
        notes.push(format!("[KLIF] warning: {w}"));
    }
    let (persisted, origin) = state::load_with_origin(&state_dir);
    match &origin {
        state::Origin::Legacy => notes.push("[KLIF] read 0.2's state.json once (it is left unchanged)".into()),
        state::Origin::Fresh(Some(bad)) => notes.push(format!("[KLIF] state.v3.json was unreadable; moved to {}", bad.display())),
        _ => {}
    }

    // Telemetry on every GPU a System uses (+ [gpu] inference), resolved by PCI id (LUIDs change every boot).
    let session_gpus: Vec<String> = persisted.sessions.values().flat_map(|p| p.all_gpus()).collect();
    let (adapters, gpu_ids, gpu_notes) = watch::resolve_gpus(&cfg, &session_gpus);
    notes.extend(gpu_notes);
    let telemetry = Telemetry::start(adapters, cfg.telemetry.warn_below_gib);
    let nodes = NodeHub::start(&cfg);
    let nodes_sig = watch::nodes_signature(&cfg);
    let hardware = HardwareState::read(&cfg.hardware);

    let st = State {
        selected: persisted.selected.clone(),
        host: host.clone(),
        sessions: BTreeMap::new(),
        pending: BTreeMap::new(),
        externals: BTreeMap::new(),
        last: persisted.last_sessions.clone(),
        arches: persisted.arches.clone(),
        layers: persisted.layers.clone(),
        idle_tails: BTreeMap::new(),
        notes,
        foreign_ports: BTreeMap::new(),
        endpoints: BTreeMap::new(),
        gpu_facts: Vec::new(),
        gpus_mem: Vec::new(),
        machine: empty_machine(),
        engine_issues: Vec::new(),
        dirty: true,
        session_seq: 0,
    };
    let files = FileWatch {
        config: watch::stat(&cfg.file_path()),
        config_path: Some(cfg.file_path()),
        key: None,
    };
    let key_info = keys::info(&cfg);
    let inner = Arc::new_cyclic(|me| Inner {
        me: me.clone(),
        loaded: Mutex::new(loaded),
        catalog: Mutex::new(Arc::new(catalog)),
        telemetry: Mutex::new(Some(telemetry)),
        nodes,
        sup: Supervisor::new(),
        st: Mutex::new(st),
        published: Mutex::new(Published { vm: fallback_vm(host), focus: BTreeMap::new() }),
        subs: Mutex::new(Vec::new()),
        wake: Mutex::new(false),
        wake_cv: Condvar::new(),
        stop: AtomicBool::new(false),
        thread: Mutex::new(None),
        act_serial: Mutex::new(()),
        tick_lock: Mutex::new(()),
        reload_lock: Mutex::new(()),
        persist_lock: Mutex::new(()),
        files: Mutex::new(files),
        key_info: Mutex::new(key_info),
        bench: Mutex::new(BenchCache::default()),
        downloads: Arc::new(Mutex::new(Downloads::default())),
        servers: Mutex::new(Servers::default()),
        ulps: Mutex::new(BTreeMap::new()),
        gpu_ids: Mutex::new(gpu_ids),
        hardware: Mutex::new(hardware),
        nodes_sig: Mutex::new(nodes_sig),
        records: crate::records::Records::open(&data_dir, &crate::records::machine_name(&cfg)),
        engine_lock: Mutex::new(Some(lock_file)),
        web_auth: Arc::new(WebAuth::load(&state_dir)),
        web_assets: Mutex::new(None),
        state_dir,
        data_dir,
    });

    // Adoption: every persisted session (jobs by name). Never start anything.
    {
        let cfg = inner.cfg();
        let mut st = lock(&inner.st);
        for (id, p) in persisted.sessions {
            let id = if p.system.as_str().is_empty() { id } else { p.system.clone() };
            inner.adopt_persisted(&mut st, &cfg, id, p);
        }
        st.dirty = true;
    }
    inner.flush();
    if let Some(sel) = lock(&inner.st).selected.clone() {
        if let Some(node) = sel.node() {
            inner.nodes.set_focus(node, Some(sel.local()));
        }
    }
    inner.tick();
    let worker = inner.clone();
    let h = std::thread::Builder::new()
        .name("klif-engine".into())
        .spawn(move || worker.run())
        .map_err(|e| StartError::Other(anyhow!("could not start the engine thread: {e}")))?;
    *lock(&inner.thread) = Some(h);
    log::info!("engine started in {} ms", t0.elapsed().as_millis());
    Ok(inner)
}

// ------------------------------------------------------------------------------------------ engine

impl Inner {
    pub(crate) fn cfg(&self) -> Config {
        lock(&self.loaded).cfg.clone()
    }

    pub(crate) fn catalog(&self) -> Arc<Catalog> {
        lock(&self.catalog).clone()
    }

    pub(crate) fn tel<R>(&self, f: impl FnOnce(&Telemetry) -> R) -> Option<R> {
        lock(&self.telemetry).as_ref().map(f)
    }

    /// A handle to this engine (for the control servers).
    fn handle(&self) -> Option<EngineHandle> {
        self.me.upgrade().map(|inner| EngineHandle { inner })
    }

    fn poke(&self) {
        *lock(&self.wake) = true;
        self.wake_cv.notify_all();
    }

    /// Write `state.v3.json` when the persisted part changed (the file write happens outside `st`).
    pub(crate) fn flush(&self) {
        let _g = lock(&self.persist_lock);
        let ps = {
            let mut st = lock(&self.st);
            if !st.dirty {
                return;
            }
            st.dirty = false;
            st.persisted()
        };
        if let Err(e) = state::save(&self.state_dir, &ps) {
            log::warn!("could not write {}: {e:#}", state::STATE_FILE);
            lock(&self.st).dirty = true;
        }
    }

    // ---- control servers (E3) ---------------------------------------------------------------

    /// `control::serve_local` always; the network listener when `[node] listen` is set.
    pub(crate) fn start_servers(&self) {
        let Some(handle) = self.handle() else { return };
        match crate::control::serve_local(handle, &self.state_dir) {
            Ok(s) => {
                log::info!("local control server on 127.0.0.1:{}", s.port());
                lock(&self.servers).local = Some(s);
            }
            Err(e) => {
                log::warn!("the local control server did not start: {e:#}");
                lock(&self.st).notes.push(format!("[KLIF] klif-cli cannot reach this KLIF: {e:#}"));
            }
        }
        let cfg = self.cfg();
        self.sync_listener(&cfg);
        self.sync_webui(&cfg);
    }

    /// The host's klif-webui files arrived: serve the page if `[webui]` wants it.
    pub(crate) fn set_web_assets(&self, assets: WebAssets) {
        *lock(&self.web_assets) = Some(assets);
        let cfg = self.cfg();
        self.sync_webui_with(&cfg, true);
        self.poke();
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
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick())).is_err() {
                log::error!("engine tick panicked");
            }
            let el = t.elapsed();
            if el > Duration::from_millis(250) {
                log::debug!("slow tick: {} ms", el.as_millis());
            }
            next = Instant::now() + TICK;
        }
    }

    /// One engine step: reload changed files, advance every session, fire ready launches, refresh port owners,
    /// compose and publish the view model.
    pub(crate) fn tick(&self) {
        let vm = {
            let _t = lock(&self.tick_lock);
            let now = now_s();
            self.reload_if_changed(now);
            self.refresh_side_facts(now);
            let cfg = self.cfg();
            let catalog = self.catalog();
            let remote = self.nodes.remote();
            let mut snap = self.tel(|t| t.snapshot());
            let (ready, seq) = {
                let mut st = lock(&self.st);
                self.advance(&mut st, &cfg, snap.as_ref(), now);
                self.sync_externals(&mut st, &cfg, &catalog, now);
                let ready = self.drive_pending(&mut st, &cfg, now);
                (ready, st.session_seq)
            };
            for id in ready {
                if let Err(e) = self.fire(&id) {
                    log::warn!("launch of {id} failed: {e:#}");
                }
            }
            self.refresh_ports(&cfg, &catalog);
            self.refresh_bench(&cfg, now);
            if lock(&self.st).session_seq != seq {
                snap = self.tel(|t| t.snapshot());
            }
            // The catalog probes files (programs, models, PATH; a dead network share blocks for its timeout): that
            // happens here, never under `st`, so Stop / Launch / previews are not held up by it (SPEC 5).
            let live = self.live_facts(&cfg);
            let parts = compose::CatalogParts {
                systems: catalog.systems(&cfg, &live),
                presets: catalog.presets(&cfg, &live),
                recommendations: catalog.recommendations(&cfg),
            };
            let (vm, focus) = {
                let mut st = lock(&self.st);
                self.compose(&mut st, &cfg, parts, snap.as_ref(), &remote, now)
            };
            *lock(&self.published) = Published { vm: vm.clone(), focus };
            self.flush();
            // records.json / history: written only when a record broke since.
            self.records.flush();
            vm
        };
        self.publish(vm);
    }

    fn publish(&self, vm: ViewModel) {
        let subs: Vec<Subscriber> = lock(&self.subs).clone();
        for f in subs {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&vm))).is_err() {
                log::error!("a view-model subscriber panicked");
            }
        }
    }

    // ---- host-facing ------------------------------------------------------------------------

    pub(crate) fn snapshot(&self) -> ViewModel {
        lock(&self.published).vm.clone()
    }

    pub(crate) fn snapshot_focus(&self, focus: Option<&SystemId>) -> ViewModel {
        let p = lock(&self.published);
        let mut vm = p.vm.clone();
        let Some(id) = focus else { return vm };
        let Some(sys) = vm.systems.iter().find(|s| &s.id == id).cloned() else { return vm };
        vm.session = sys.session.clone();
        vm.last_session = sys.last_session.clone();
        if let Some(f) = p.focus.get(id) {
            vm.console = f.console.clone();
            vm.vram = f.vram.clone();
        }
        vm.selected = Some(sys.id);
        vm
    }

    pub(crate) fn subscribe(&self, f: Box<dyn Fn(&ViewModel) + Send + Sync>) {
        lock(&self.subs).push(Arc::from(f));
    }

    pub(crate) fn set_host(&self, host: HostInfo) {
        lock(&self.st).host = host;
        self.poke();
    }

    pub(crate) fn endpoint_url(&self, system: Option<&SystemId>) -> Option<String> {
        let p = lock(&self.published);
        let id = system.cloned().or_else(|| p.vm.selected.clone())?;
        let s = p.vm.systems.iter().find(|s| s.id == id)?;
        matches!(s.status, SystemStatus::Online | SystemStatus::Busy | SystemStatus::Starting).then(|| s.endpoint.clone()).flatten()
    }

    pub(crate) fn api_key(&self) -> Option<Secret> {
        keys::load(&self.cfg())
    }

    /// A preset in full (masked). Ok(None): there is no such preset (a node answers null for that). A remote node
    /// that cannot be asked, refuses or answers something unreadable is an Err with its sentence, never "no such
    /// preset".
    pub(crate) fn preset(&self, id: &str, node: Option<&str>) -> Result<Option<PresetDetail>> {
        if let Some(node) = node.map(str::trim).filter(|n| !n.is_empty()) {
            let v = self.nodes.call(node, "preset", serde_json::json!({ "id": id }))?;
            if v.is_null() {
                return Ok(None);
            }
            let mut d: PresetDetail = serde_json::from_value(v)
                .map_err(|e| anyhow!("Node \"{node}\": the preset could not be read ({e}). Use the same KLIF version on both machines."))?;
            d.info.node = Some(node.to_string());
            return Ok(Some(d));
        }
        let cfg = self.cfg();
        let live = self.live_facts(&cfg);
        Ok(self.catalog().preset_detail(&cfg, id, &live))
    }

    pub(crate) fn command_preview(&self, spec: &PresetCfg, system: Option<&SystemId>) -> CommandView {
        if let Some(node) = system.and_then(|s| s.node()) {
            // A clear secret never crosses plain TCP (SPEC 7 / 16.10), whichever frontend asks (Tune's live preview,
            // klif-cli, any control client). MASK passes: the node keeps its stored value.
            let clear = actions::clear_secrets(spec);
            if !clear.is_empty() {
                return CommandView {
                    adapter: spec.adapter,
                    issues: vec![Issue::error(None, format!("{} ({}).", actions::REMOTE_SECRET_REFUSAL, clear.join(", ")))],
                    ..CommandView::default()
                };
            }
            let local = system.map(|s| s.local().to_string());
            let params = serde_json::json!({ "spec": spec, "system": local });
            return match self.nodes.call(node, "command_preview", params).and_then(|v| Ok(serde_json::from_value::<CommandView>(v)?)) {
                Ok(c) => c,
                Err(e) => CommandView {
                    adapter: spec.adapter,
                    issues: vec![Issue::error(None, format!("Node \"{node}\" could not preview the command: {e:#}"))],
                    ..CommandView::default()
                },
            };
        }
        let cfg = self.cfg();
        let live = self.live_facts(&cfg);
        self.catalog().command_view(&cfg, spec, system, &live)
    }

    pub(crate) fn plan(&self, system: &SystemId) -> Result<CommandView> {
        if let Some(node) = system.node() {
            let v = self.nodes.call(node, "plan", serde_json::json!({ "system": system.local() }))?;
            return Ok(serde_json::from_value(v)?);
        }
        let cfg = self.cfg();
        let catalog = self.catalog();
        if cfg.system(system.as_str()).is_none() {
            if lock(&self.st).sessions.contains_key(system) {
                anyhow::bail!("{} is not in klif.toml any more; it can only be stopped.", system);
            }
            anyhow::bail!("There is no System \"{system}\".");
        }
        let Some((_, spec)) = cfg.system_preset(system.as_str()) else {
            // No preset, a missing one or one that does not parse: nothing to show; the catalog's sentence says
            // which (resolving stops before any file probe).
            return catalog.plan(&cfg, system, None, timefmt::FIXED_STAMP).map(|p| p.command);
        };
        let live = self.live_facts(&cfg);
        // The System's view: also the cross-System "shares port N" and foreign-port-holder warnings.
        let view = catalog.command_view(&cfg, spec, Some(system), &live);
        if spec.is_external() {
            return Ok(view);
        }
        let key = keys::load(&cfg);
        match catalog.plan(&cfg, system, key.as_ref(), timefmt::FIXED_STAMP) {
            Ok(p) => {
                let mut c = p.command;
                for i in view.issues.into_iter().filter(|i| !i.is_error()) {
                    if !c.issues.iter().any(|x| x.text == i.text) {
                        c.issues.push(i);
                    }
                }
                Ok(c)
            }
            // Not launchable now (error issues, a program that does not resolve...): still the command it would run,
            // with the errors (`klif-cli plan` then says launchable: no). An Invalid System is shown, not refused.
            Err(e) => {
                let mut c = view;
                if !c.issues.iter().any(Issue::is_error) {
                    c.issues.push(Issue::error(None, format!("{e:#}")));
                }
                Ok(c)
            }
        }
    }

    pub(crate) fn config_path(&self) -> Option<PathBuf> {
        self.cfg().source
    }

    pub(crate) fn ensure_config(&self) -> Result<PathBuf> {
        let cfg = self.cfg();
        let p = klif_catalog::store::ensure_file(&cfg)?;
        self.reload_config(true);
        Ok(p)
    }

    pub(crate) fn set_api_key(&self, key: Option<Secret>) -> Result<()> {
        let r = keys::store(&self.cfg(), key.as_ref());
        lock(&self.files).key = None;
        self.poke();
        r
    }

    pub(crate) fn diag(&self) -> serde_json::Value {
        self.diag_json()
    }

    pub(crate) fn ulps(&self) -> Option<UlpsSetting> {
        self.cfg().gpu.inference.as_deref().and_then(|g| self.ulps_for(g))
    }

    /// ULPS of one GPU (cached; read on first use).
    pub(crate) fn ulps_for(&self, gpu: &str) -> Option<UlpsSetting> {
        let pci = gpu.split('#').next().unwrap_or(gpu).trim().to_string();
        if pci.is_empty() || pci.eq_ignore_ascii_case("cpu") {
            return None;
        }
        let mut cache = lock(&self.ulps);
        cache.entry(pci.clone()).or_insert_with(|| klif_telemetry::ulps_setting(&pci)).clone()
    }

    pub(crate) fn state_dir(&self) -> PathBuf {
        self.state_dir.clone()
    }

    pub(crate) fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
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
            let mut s = lock(&self.servers);
            if let Some(n) = s.network.take() {
                n.shutdown();
            }
            if let Some(l) = s.local.take() {
                l.shutdown();
            }
            if let Some(w) = s.webui.take() {
                w.shutdown();
            }
            s.auth = None;
        }
        self.web_auth.flush();
        self.nodes.shutdown();
        for (_, h) in std::mem::take(&mut lock(&self.downloads).handles) {
            h.cancel();
        }
        lock(&self.st).dirty = true;
        self.flush();
        self.records.shutdown();
        // Dropping the telemetry joins its threads. Servers are left running by design.
        let t = lock(&self.telemetry).take();
        drop(t);
        if let Some(f) = lock(&self.engine_lock).take() {
            let _ = f.unlock();
        }
        log::info!("engine stopped (servers keep running)");
    }
}

