//! klif-telemetry: everything KLIF measures, on background threads. Passive: it never starts or stops anything.
//! - GPUs: every adapter passed to `Telemetry::start` / `set_gpus` (resolved by PCI id: LUIDs change every boot),
//!   PDH adapter memory and per-process memory (resident = Dedicated Usage, allocations = Total Committed,
//!   spill = Shared Usage) attributed per watched session, a 1 Hz history per GPU, the device power state and
//!   the dormant detector (`dormant`: an AMD card slept with a model loaded and its VRAM was paged out).
//! - Machine: CPU name (CPUID), CPU utility, RAM.
//! - Sessions (multi-watch, keyed by the System id): each key has its own tracker (`session::SessionTracker`),
//!   generation counter, PIDs, log tails and console ring. The tracker's adapter (`backend::BackendAdapter`,
//!   chosen by `WatchSpec.adapter`) parses logs and fuses probe answers into `SessionSignals`.
//!
//! Threads: the sampler (logs at 2 Hz; memory / CPU / RAM / power at 1 Hz) and the probe pool (a scheduler plus
//! 4 workers; one probe round per key at a time, a per-key deadline, failing keys back off 0.5 s -> 5 s), so a
//! slow or dead server never delays sampling, another System's probes or the UI.
//!
//! GPU ids: "VEN:DEV" (upper-case hex), and for the second, third... of several identical adapters "VEN:DEV#n"
//! (n = index among identical adapters in DXGI order; the first one keeps the plain "VEN:DEV", which is also
//! what "VEN:DEV#0" means). `gpu_id_eq` compares ids with that rule.
//!
//! Platform: Windows readers live in `win` behind `platform` (`GpuPlatform` / `HostPlatform`); other targets get
//! a stub (no GPU / RAM numbers) and still compile. The SMBIOS RAM-type reader exists only with the `smbios`
//! feature (klif-cli diag); the GUI shows `[telemetry] ram_type`.

pub mod backend;
pub mod dormant;
pub mod llama;
pub mod platform;
pub mod probe;
mod prober;
pub mod prom;
mod sampler;
pub mod sd;
pub mod session;
pub mod steps;
pub mod tail;
pub mod text;
pub mod vram;
#[cfg(windows)]
mod win;

use klif_common::vm::{
    AdapterId, Dormant, GenericLive, GpuMemory, HealthCheck, ImageLive, LlmLive, LoadStep, MachineStats, ModelArch, SystemKind,
    VramLayer, VramLayerId,
};
use klif_common::Secret;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub use dormant::{DormancyFacts, DormantTracker, PowerState};
pub use session::SessionTracker;

// ------------------------------------------------------------------------------------------- adapters

/// A resolved DXGI adapter.
#[derive(Debug, Clone)]
pub struct Adapter {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub luid_high: i32,
    pub luid_low: u32,
    pub dedicated_bytes: u64,
}

impl Adapter {
    /// Chromium `--use-adapter-luid` value: decimal "high,low".
    pub fn chromium_luid_arg(&self) -> String {
        format!("--use-adapter-luid={},{}", self.luid_high, self.luid_low)
    }
    /// PDH instance fragment, e.g. "luid_0x00000000_0x00017798".
    pub fn pdh_luid(&self) -> String {
        format!("luid_0x{:08X}_0x{:08X}", self.luid_high as u32, self.luid_low)
    }
    /// "VEN:DEV" in upper-case hex, e.g. "1002:731F".
    pub fn pci_id(&self) -> String {
        format!("{:04X}:{:04X}", self.vendor_id, self.device_id)
    }
    /// Display name without the vendor prefix: "AMD Radeon RX 9070 XT" -> "RX 9070 XT".
    /// (The engine may override `name` with the configured `inference_name`.)
    pub fn display_name(&self) -> String {
        let n = self.name.trim();
        for p in ["AMD Radeon ", "AMD ", "NVIDIA GeForce ", "NVIDIA ", "Intel(R) "] {
            if let Some(rest) = n.strip_prefix(p) {
                if !rest.trim().is_empty() {
                    return rest.trim().to_string();
                }
            }
        }
        n.to_string()
    }
    /// AMD (vendor 0x1002): the only vendor the dormant detector (ULPS / D3) runs for.
    pub fn is_amd(&self) -> bool {
        self.vendor_id == 0x1002
    }
    fn same_device(&self, o: &Adapter) -> bool {
        self.vendor_id == o.vendor_id && self.device_id == o.device_id && self.luid_high == o.luid_high && self.luid_low == o.luid_low
    }
}

/// Enumerate hardware adapters (DXGI EnumAdapters1 order; empty off Windows until a platform reader exists).
pub fn adapters() -> Vec<Adapter> {
    platform::gpu().adapters()
}

/// "VEN:DEV" hex (e.g. "1002:731F"; a "#n" suffix is ignored) -> (vendor id, device id).
pub fn parse_pci(pci: &str) -> Option<(u32, u32)> {
    parse_gpu_id(pci).map(|(v, d, _)| (v, d))
}

/// "VEN:DEV" or "VEN:DEV#n" -> (vendor id, device id, n). n = 0 without a suffix.
pub fn parse_gpu_id(id: &str) -> Option<(u32, u32, usize)> {
    let id = id.trim();
    let (pci, n) = match id.split_once('#') {
        Some((p, n)) => (p, n.trim().parse::<usize>().ok()?),
        None => (id, 0),
    };
    let (v, d) = pci.trim().split_once(':')?;
    let hex = |s: &str| {
        let s = s.trim();
        let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
        u32::from_str_radix(s, 16).ok()
    };
    Some((hex(v)?, hex(d)?, n))
}

/// The index of `a` among the adapters of `all` with the same VEN:DEV (by LUID; 0 when not found).
pub fn adapter_index(a: &Adapter, all: &[Adapter]) -> usize {
    all.iter()
        .filter(|x| x.vendor_id == a.vendor_id && x.device_id == a.device_id)
        .position(|x| x.same_device(a))
        .unwrap_or(0)
}

/// The GPU id KLIF shows for `a` given every adapter of the machine (`all`, DXGI order): "VEN:DEV" for a unique
/// adapter and for the first of several identical ones, "VEN:DEV#n" for the n-th (n >= 1).
pub fn gpu_id(a: &Adapter, all: &[Adapter]) -> String {
    match adapter_index(a, all) {
        0 => a.pci_id(),
        n => format!("{}#{n}", a.pci_id()),
    }
}

/// Two GPU ids name the same device ("VEN:DEV" == "VEN:DEV#0", hex case and 0x prefixes ignored). "cpu" equals
/// only "cpu".
pub fn gpu_id_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.eq_ignore_ascii_case("cpu") || b.eq_ignore_ascii_case("cpu") {
        return a.eq_ignore_ascii_case(b);
    }
    match (parse_gpu_id(a), parse_gpu_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Find an adapter by "VEN:DEV" or "VEN:DEV#n" (the n-th identical adapter in DXGI order).
pub fn find_adapter(id: &str) -> Option<Adapter> {
    let (ven, dev, n) = parse_gpu_id(id)?;
    adapters().into_iter().filter(|a| a.vendor_id == ven && a.device_id == dev).nth(n)
}

/// The card's current device power state (one SetupDi enumeration + property query; read-only).
pub fn power_state(pci: &str) -> Option<PowerState> {
    platform::gpu().power_state(pci)
}

/// The card's AMD ULPS setting: `EnableUlps` in its own driver key. Read-only, read only when asked (never by
/// the sampler). A change takes effect only after a reboot: the value says what the driver does from the next
/// boot on, which is normally what it does now.
#[derive(Debug, Clone)]
pub struct UlpsSetting {
    /// "PCI\VEN_1002&DEV_7550&...".
    pub instance_id: String,
    /// HKLM path of the driver key that was read.
    pub key: String,
    /// The key's DriverDesc (else the device description).
    pub driver_desc: Option<String>,
    /// The EnableUlps DWORD (None: no such value, e.g. a non-AMD card).
    pub enable_ulps: Option<u32>,
}

impl UlpsSetting {
    pub fn is_on(&self) -> bool {
        self.enable_ulps == Some(1)
    }
}

/// Read the card's EnableUlps from the device's own driver key (`SetupDiOpenDevRegKey`). `pci`: "VEN:DEV" or
/// "VEN:DEV#n". None when the device is not found.
pub fn ulps_setting(pci: &str) -> Option<UlpsSetting> {
    let (ven, dev, n) = parse_gpu_id(pci)?;
    let a = find_adapter(pci).unwrap_or(Adapter {
        name: String::new(),
        vendor_id: ven,
        device_id: dev,
        luid_high: 0,
        luid_low: 0,
        dedicated_bytes: 0,
    });
    platform::gpu().ulps(&a, n)
}

// ---------------------------------------------------------------------------------------- watch spec

/// What to watch for one System's session.
#[derive(Debug, Clone)]
pub struct WatchSpec {
    /// The VM shape (LlmLive / ImageLive / GenericLive), from the preset's kind.
    pub kind: SystemKind,
    /// The server family: parser, probes and health rules.
    pub adapter: AdapterId,
    /// An external server: no process, no logs, probes only.
    pub external: bool,
    /// The session's log files (None: no logs, e.g. an external server).
    pub out_log: Option<PathBuf>,
    pub err_log: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    /// Sent as `Authorization: Bearer` to /slots, /v1/models and /metrics of `host` only. The engine sets it only
    /// for KLIF-launched llama.cpp / vLLM Systems whose preset has `api_key = true` (SPEC 16.12).
    pub api_key: Option<Secret>,
    /// Session start (seconds since epoch) for elapsed times.
    pub started_at: f64,
    /// Context window from the preset (used until the server reports n_ctx).
    pub ctx_tokens: Option<u32>,
    /// Speculative mode label, if speculation is configured.
    pub spec_mode: Option<String>,
    /// The preset's health check (`Auto` = the adapter's default chain).
    pub health: HealthCheck,
    /// The server serves Prometheus /metrics.
    pub metrics: bool,
    /// The device name the preset expects (its `device`, else the GPU's name), for `device_mismatch`.
    pub expect_device: Option<String>,
    /// The GPU(s) it runs on: "VEN:DEV", "VEN:DEV#n", a comma list of those, or "cpu" (its VRAM is attributed to
    /// these GPUs; None = every measured GPU).
    pub gpu: Option<String>,
}

/// Server health as seen from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// No answer on the port yet.
    Down,
    /// Socket answers 503 "Loading model" (llama) / no listener yet (sd).
    Loading,
    /// Ready to serve.
    Ready,
}

/// A session's memory on one GPU (per-process PDH counters summed over the session's PIDs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionGpu {
    /// The GPU id ("VEN:DEV" / "VEN:DEV#n").
    pub id: String,
    pub resident_gib: f64,
    pub committed_gib: f64,
    pub shared_gib: f64,
}

/// Everything telemetry knows about one watched session.
#[derive(Debug, Clone)]
pub struct SessionSignals {
    pub health: Health,
    /// Load steps (llama.cpp / sd.cpp: the six log steps; others: start / load model / ready).
    pub load_steps: Vec<LoadStep>,
    pub load_fraction: f64,
    /// VRAM composition parsed from logs (weights/kv/buffers/draft/projector), if available.
    pub layers_from_logs: Option<Vec<VramLayer>>,
    pub llm: Option<LlmLive>,
    pub image: Option<ImageLive>,
    /// Request counters / last activity / model id (vLLM, OpenAI-compatible, generic; llama.cpp and sd.cpp when
    /// the kind is not theirs or llama.cpp serves /metrics).
    pub generic: Option<GenericLive>,
    /// Working right now (prefill / decode / image steps / requests in flight / log activity).
    pub busy: bool,
    /// Last error-looking lines (for a fault panel), redacted.
    pub error_tail: Vec<String>,
    /// A one-line reason if the logs show a fatal error (e.g. "ROCm error: device kernel image is invalid").
    pub fatal_hint: Option<String>,
    /// Exit code printed by a starter script's own exit line (sd-server `.cmd` starters), if seen.
    pub starter_exit: Option<i64>,
    /// Median decode speed over every finished request of the session (LLM), for the last-session summary.
    pub median_decode_tps: Option<f64>,
    /// The LLM's shape from its `print_info` block (layers, experts, heads), once logged.
    pub arch: Option<ModelArch>,
    /// The server's build as it reports it, e.g. "b6500" (llama.cpp) or "0.6.3" (vLLM), for bench records.
    pub backend_build: Option<String>,
    /// One sentence when the server loaded on a different device than `WatchSpec.expect_device`.
    pub device_mismatch: Option<String>,
    /// The session's console: both logs interleaved by arrival plus KLIF notes, redacted, newest last, up to 200.
    pub console: Vec<String>,
    /// The session processes' shared (spilled) memory on their GPUs (adapters that report spill: LLM servers).
    pub spill_mib: f64,
    /// This session's VRAM layers: the log composition (else one "model" layer), scaled to its VRAM allocations
    /// (`committed_gib` minus what sits in shared memory; equal to `resident_gib` unless paged out), with an
    /// "other" layer for allocations the log does not explain. Bottom first.
    pub layers: Vec<VramLayer>,
    /// PDH "GPU Process Memory(pid_<pid>_*)\Dedicated Usage" summed over the session's pids on its GPUs.
    pub resident_gib: f64,
    /// PDH "GPU Process Memory(pid_<pid>_*)\Total Committed" summed over the session's pids on its GPUs.
    pub committed_gib: f64,
    /// The same measurements per GPU, for every measured GPU the session's processes hold memory on (multi-GPU
    /// presets reserve on each).
    pub per_gpu: Vec<SessionGpu>,
}

/// One GPU as measured right now.
#[derive(Debug, Clone)]
pub struct GpuSnapshot {
    /// The raw PDH reading (zeros before the first sample); session fields = every watched session's PIDs.
    pub reading: GpuReading,
    /// For the view model: id ("VEN:DEV" / "VEN:DEV#n"), name, total, used, history, layer_history, dormant,
    /// `baseline_gib` (measured only while no session runs on that GPU, else the last value; before the first such
    /// measurement, e.g. with adopted sessions, used minus the sessions' resident memory) and
    /// `layers = [other]` only, other = max(0, used - every KLIF session's resident memory on this GPU) (the
    /// engine adds the selected session's layers).
    pub memory: GpuMemory,
    /// The dormant detector's facts for this GPU (also reflected in `memory.dormant`; AMD only).
    pub dormancy: DormancyFacts,
}

#[derive(Debug, Clone)]
pub struct TelemetrySnapshot {
    /// Every GPU passed to `start` / `set_gpus`, in that order.
    pub gpus: Vec<GpuSnapshot>,
    pub machine: MachineStats,
    /// Watched sessions by key (the System id).
    pub sessions: BTreeMap<String, SessionSignals>,
}

// ------------------------------------------------------------------------------------------- state

/// A session's memory as the sampler measured it (1 Hz).
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionVram {
    pub resident_gib: f64,
    pub committed_gib: f64,
    pub spill_mib: f64,
    pub layers: Vec<VramLayer>,
    pub per_gpu: Vec<SessionGpu>,
}

/// One watched session.
pub(crate) struct Watched {
    pub tracker: SessionTracker,
    /// Guards probe results against a re-watch mid-probe.
    pub generation: u64,
    pub pids: Vec<u32>,
    pub vram: SessionVram,
}

pub(crate) type WatchedRef = Arc<Mutex<Watched>>;

/// One measured GPU.
pub(crate) struct GpuState {
    pub adapter: Adapter,
    pub id: String,
    /// Index among identical adapters (DXGI order).
    pub nth: usize,
    pub total_gib: f64,
    pub used_gib: f64,
    pub other_gib: f64,
    pub baseline_gib: f64,
    /// `baseline_gib` was measured with no session on this GPU (else it is an estimate).
    pub baseline_measured: bool,
    pub spill_mib: f64,
    pub hist: vram::VramHistory,
    pub reading: GpuReading,
    pub dormant: DormantTracker,
    /// A session on this GPU has answered ready since it was watched (gates the dormant detector).
    pub ever_ready: bool,
}

impl GpuState {
    fn new(adapter: Adapter, all: &[Adapter]) -> GpuState {
        let nth = adapter_index(&adapter, all);
        let id = gpu_id(&adapter, all);
        GpuState {
            total_gib: text::round_to(adapter.dedicated_bytes as f64 / text::GIB, 3),
            adapter,
            id,
            nth,
            used_gib: 0.0,
            other_gib: 0.0,
            baseline_gib: 0.0,
            baseline_measured: false,
            spill_mib: 0.0,
            hist: vram::VramHistory::default(),
            reading: GpuReading::default(),
            dormant: DormantTracker::default(),
            ever_ready: false,
        }
    }
}

pub(crate) struct Shared {
    pub gpus: Vec<GpuState>,
    pub gpus_gen: u64,
    pub warn_below_gib: f64,
    pub machine: MachineStats,
    pub sessions: BTreeMap<String, WatchedRef>,
    pub next_generation: u64,
    /// Bumped whenever any session's PID set changes (per-process PDH counters are re-added).
    pub pids_gen: u64,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn empty_machine() -> MachineStats {
    MachineStats { ram_used_gib: 0.0, ram_total_gib: 0.0, ram_type: None, cpu_name: String::new(), cpu_pct: 0.0 }
}

fn gpu_states(gpus: Vec<Adapter>) -> Vec<GpuState> {
    let all = if gpus.is_empty() { Vec::new() } else { adapters() };
    let mut out: Vec<GpuState> = Vec::new();
    for a in gpus {
        // The same adapter twice (two Systems on one GPU) is measured once.
        if out.iter().any(|g| g.adapter.same_device(&a)) {
            continue;
        }
        out.push(GpuState::new(a, &all));
    }
    out
}

// ---------------------------------------------------------------------------------------- Telemetry

pub struct Telemetry {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Telemetry {
    /// Start the sampler and the probe pool. `gpus`: every GPU used by a System plus `[gpu] inference` (the first
    /// one is the default / inference card). Duplicates are measured once.
    pub fn start(gpus: Vec<Adapter>, warn_below_gib: f64) -> Telemetry {
        let sh = Shared {
            gpus: gpu_states(gpus),
            gpus_gen: 0,
            warn_below_gib,
            machine: empty_machine(),
            sessions: BTreeMap::new(),
            next_generation: 0,
            pids_gen: 0,
        };
        let shared = Arc::new(Mutex::new(sh));
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        {
            let (sh, st) = (shared.clone(), stop.clone());
            if let Ok(h) = std::thread::Builder::new().name("klif-sampler".into()).spawn(move || sampler::run(sh, st)) {
                threads.push(h);
            }
        }
        threads.extend(prober::start(shared.clone(), stop.clone()));
        Telemetry { shared, stop, threads }
    }

    /// Replace the measured GPUs (a System moved to another GPU, or the config changed). Cheap when unchanged.
    pub fn set_gpus(&self, gpus: Vec<Adapter>) {
        {
            let s = lock(&self.shared);
            let mut want: Vec<&Adapter> = Vec::new();
            for a in &gpus {
                if !want.iter().any(|w| w.same_device(a)) {
                    want.push(a);
                }
            }
            let same = s.gpus.len() == want.len()
                && s.gpus.iter().zip(&want).all(|(g, a)| g.adapter.same_device(a) && g.adapter.name == a.name);
            if same {
                return;
            }
        }
        // DXGI enumeration (for "#n" ids) outside the lock.
        let mut fresh = gpu_states(gpus);
        let mut s = lock(&self.shared);
        // Keep the history / dormancy of GPUs that stay.
        let old = std::mem::take(&mut s.gpus);
        for g in fresh.iter_mut() {
            if let Some(o) = old.iter().find(|o| o.adapter.same_device(&g.adapter)) {
                g.used_gib = o.used_gib;
                g.other_gib = o.other_gib;
                g.baseline_gib = o.baseline_gib;
                g.baseline_measured = o.baseline_measured;
                g.hist = o.hist.clone();
                g.reading = o.reading;
                g.dormant = o.dormant.clone();
                g.ever_ready = o.ever_ready;
            }
        }
        s.gpus = fresh;
        s.gpus_gen += 1;
    }

    /// Begin watching a System's session (logs from their current end for a new launch, from the start for an
    /// adopted one: `from_start`). Replaces an existing watch of the same key (new generation).
    pub fn watch(&self, key: &str, spec: WatchSpec, from_start: bool) {
        let tracker = SessionTracker::new(spec, from_start);
        let mut s = lock(&self.shared);
        s.next_generation += 1;
        let generation = s.next_generation;
        let w = Watched { tracker, generation, pids: Vec::new(), vram: SessionVram::default() };
        s.sessions.insert(key.to_string(), Arc::new(Mutex::new(w)));
        s.pids_gen += 1;
    }

    /// Stop watching a System's session (its console is gone from the next snapshot on; keep it yourself).
    pub fn unwatch(&self, key: &str) {
        let mut s = lock(&self.shared);
        let Some(w) = s.sessions.remove(key) else { return };
        s.pids_gen += 1;
        // Give its resident memory back to "other" at once (the 1 Hz sampler would show it for up to 1 s).
        let per_gpu = lock(&w).vram.per_gpu.clone();
        for pg in per_gpu {
            if let Some(g) = s.gpus.iter_mut().find(|g| g.id == pg.id) {
                g.other_gib = text::round_to((g.other_gib + pg.resident_gib).min(g.used_gib.max(g.other_gib)), 3);
            }
        }
    }

    /// Add a KLIF line to a watched session's console, in arrival order with its log lines.
    pub fn note(&self, key: &str, line: impl Into<String>) {
        let w = lock(&self.shared).sessions.get(key).cloned();
        if let Some(w) = w {
            lock(&w).tracker.note(line.into());
        }
    }

    /// The PIDs of a session's processes (for per-process GPU memory). Called by the engine each tick.
    pub fn set_session_pids(&self, key: &str, pids: Vec<u32>) {
        let mut pids = pids;
        pids.sort_unstable();
        pids.dedup();
        let mut s = lock(&self.shared);
        let Some(w) = s.sessions.get(key).cloned() else { return };
        let changed = {
            let mut w = lock(&w);
            if w.pids != pids {
                w.pids = pids;
                true
            } else {
                false
            }
        };
        if changed {
            s.pids_gen += 1;
        }
    }

    /// Keys being watched right now.
    pub fn watched(&self) -> Vec<String> {
        lock(&self.shared).sessions.keys().cloned().collect()
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        let now = klif_common::now_s();
        let s = lock(&self.shared);
        let gpus = s
            .gpus
            .iter()
            .map(|g| {
                let name = g.adapter.display_name();
                let memory = GpuMemory {
                    id: g.id.clone(),
                    name: name.clone(),
                    device: name,
                    total_gib: g.total_gib,
                    used_gib: text::round_to(g.used_gib, 3),
                    layers: vec![VramLayer { id: VramLayerId::Other, label: "other".into(), gib: text::round_to(g.other_gib, 3) }],
                    spill_mib: text::round_to(g.spill_mib, 1),
                    history: g.hist.used(),
                    layer_history: Some(g.hist.per_layer()),
                    baseline_gib: text::round_to(g.baseline_gib, 3),
                    warn_below_gib: s.warn_below_gib,
                    dormant: g.dormant.current().map(|d| Dormant {
                        paged_out_gib: text::round_to(d.paged_out_gib, 3),
                        since_s: text::round_to((now - d.since).max(0.0), 1),
                        power_state: d.power.map(|p| p.as_str().to_string()),
                    }),
                };
                GpuSnapshot { reading: g.reading, memory, dormancy: g.dormant.facts() }
            })
            .collect();
        let sessions = s
            .sessions
            .iter()
            .map(|(k, w)| {
                let w = lock(w);
                let mut sig = w.tracker.signals(now);
                sig.resident_gib = text::round_to(w.vram.resident_gib, 3);
                sig.committed_gib = text::round_to(w.vram.committed_gib, 3);
                sig.spill_mib = text::round_to(w.vram.spill_mib, 1);
                sig.layers = w.vram.layers.clone();
                sig.per_gpu = w.vram.per_gpu.clone();
                (k.clone(), sig)
            })
            .collect();
        TelemetrySnapshot { gpus, machine: s.machine.clone(), sessions }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }
}

// ------------------------------------------------------------------------------------ one-off reads

/// One PDH reading of an adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuReading {
    pub used_bytes: f64,
    /// Sum of the PIDs' dedicated usage on the adapter (None: no PIDs / no instances).
    pub session_bytes: Option<f64>,
    pub session_shared_bytes: f64,
    /// Sum of the PIDs' Total Committed on the adapter: their allocations, resident or not.
    pub session_committed_bytes: Option<f64>,
}

impl GpuReading {
    /// The reading of one adapter (`key` = `Adapter::pdh_luid()`) from a sampler frame, for `pids` (sorted).
    pub(crate) fn from_frame(frame: &platform::Frame, key: &str, pids: &[u32]) -> GpuReading {
        let used: f64 = frame.adapters.iter().filter(|a| a.key == key).map(|a| a.dedicated).sum();
        let mut found = false;
        let (mut ded, mut sh, mut com) = (0.0, 0.0, 0.0);
        for p in frame.procs.iter().filter(|p| p.key == key && pids.binary_search(&p.pid).is_ok()) {
            found = true;
            ded += p.dedicated;
            sh += p.shared;
            com += p.committed;
        }
        GpuReading {
            used_bytes: used,
            session_bytes: found.then_some(ded),
            session_shared_bytes: sh,
            session_committed_bytes: found.then_some(com),
        }
    }
}

/// One-off reading of an adapter (tools, klif-cli diag). Blocks for the PDH provider load (~0.3 s) plus one
/// second. None when the platform has no reader.
pub fn read_gpu_once(adapter: &Adapter, pids: &[u32]) -> Option<GpuReading> {
    let mut s = platform::gpu().sampler()?;
    std::thread::sleep(Duration::from_millis(1000));
    let mut pids = pids.to_vec();
    pids.sort_unstable();
    pids.dedup();
    s.refresh_processes();
    let f = s.read(&pids);
    Some(GpuReading::from_frame(&f, &adapter.pdh_luid(), &pids))
}

/// One-off machine stats (CPU name/utility, RAM). Blocks ~1 s (CPU utility needs two collects). `ram_type` is
/// None (the engine applies `[telemetry] ram_type`; klif-cli may add `smbios_ram_type`).
pub fn read_system_once() -> MachineStats {
    let s = platform::gpu().sampler();
    std::thread::sleep(Duration::from_millis(1000));
    let cpu = s.map(|mut s| s.read(&[])).and_then(|f| f.cpu_pct).unwrap_or(0.0);
    machine_stats(cpu, cpu_name(), None)
}

/// The RAM type from the SMBIOS memory-device records (GetSystemFirmwareTable 'RSMB'). Only with the `smbios`
/// feature (klif-cli diag); the GUI never reads firmware tables. None off Windows.
#[cfg(feature = "smbios")]
pub fn smbios_ram_type() -> Option<String> {
    #[cfg(windows)]
    {
        win::ram_type()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Short CPU name from CPUID ("9950X3D"), "CPU" when unknown.
pub fn cpu_name() -> String {
    platform::host().cpu_name().unwrap_or_else(|| "CPU".into())
}

pub(crate) fn machine_stats(cpu_pct: f64, cpu_name: String, ram_type: Option<String>) -> MachineStats {
    let (total, avail) = platform::host().memory().unwrap_or((0, 0));
    MachineStats {
        ram_used_gib: text::round_to((total.saturating_sub(avail)) as f64 / text::GIB, 2),
        ram_total_gib: text::round_to(total as f64 / text::GIB, 1),
        ram_type,
        cpu_name,
        cpu_pct: text::round_to(cpu_pct, 1),
    }
}

/// The card's power state through a device handle kept open (one PnP property query per read, it never
/// touches the device). A failed open or read is retried after 30 s. The sampler reads it at most once
/// per second per AMD GPU, only while a session runs on that GPU.
pub struct PowerReader {
    target: Option<PowerTarget>,
    dev: Option<Box<dyn platform::PowerHandle>>,
    retry_at: f64,
}

#[derive(Debug, Clone)]
enum PowerTarget {
    Adapter(Adapter, usize),
    Pci(String),
}

impl PowerReader {
    pub fn new(inference: Option<&Adapter>) -> PowerReader {
        PowerReader { target: inference.map(|a| PowerTarget::Adapter(a.clone(), adapter_index(a, &adapters()))), dev: None, retry_at: 0.0 }
    }

    /// For one adapter (`nth`: its index among identical adapters).
    pub fn for_adapter(a: &Adapter, nth: usize) -> PowerReader {
        PowerReader { target: Some(PowerTarget::Adapter(a.clone(), nth)), dev: None, retry_at: 0.0 }
    }

    /// For a "VEN:DEV" / "VEN:DEV#n" id.
    pub fn for_pci(pci: &str) -> PowerReader {
        PowerReader { target: parse_gpu_id(pci).map(|_| PowerTarget::Pci(pci.to_string())), dev: None, retry_at: 0.0 }
    }

    /// `now`: epoch seconds (for the retry back-off).
    pub fn read(&mut self, now: f64) -> Option<PowerState> {
        let target = self.target.as_ref()?;
        if self.dev.is_none() {
            if now < self.retry_at {
                return None;
            }
            self.dev = match target {
                PowerTarget::Adapter(a, n) => platform::gpu().power_handle(a, *n),
                PowerTarget::Pci(id) => {
                    let (_, _, n) = parse_gpu_id(id)?;
                    find_adapter(id).and_then(|a| platform::gpu().power_handle(&a, n))
                }
            };
        }
        match self.dev.as_mut().and_then(|d| d.read()) {
            Some(p) => Some(p),
            None => {
                self.dev = None;
                self.retry_at = now + 30.0;
                None
            }
        }
    }
}
