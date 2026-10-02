//! klif-telemetry: everything KLIF measures, on a background sampler thread.
//! - GPU: DXGI adapters resolved by PCI id (LUIDs change every boot), PDH adapter + per-process memory
//!   (resident = Dedicated Usage, allocations = Total Committed), the device power state, and the
//!   dormant detector (`dormant`: the card slept with a model loaded and its VRAM was paged out).
//! - System: CPU utility, RAM.
//! - Session: tail the session's log files by offset (ANSI strip, CR collapse, API-key line redaction),
//!   parse llama-server / sd-server signals, probe HTTP (/health, /slots with key, /v1/models) at 1-2 Hz.
//!
//! Output is a `TelemetrySnapshot` with view-model-shaped parts; the engine composes the ViewModel.
//!
//! Threads: a sampler thread (logs at 2 Hz, PDH/RAM/CPU at 1 Hz) and a probe thread (HTTP and socket
//! probes at 2 Hz, short timeouts) so a slow server answer never delays sampling or the UI.

pub mod dormant;
pub mod llama;
pub mod probe;
pub mod sd;
pub mod session;
pub mod tail;
pub mod text;
pub mod vram;
mod win;

use klif_common::vm::{Dormant, GpuMemory, ImageLive, LlmLive, LoadStep, SlotKind, SystemStats, VramLayer};
use klif_common::Secret;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use dormant::{DormancyFacts, DormantTracker, PowerState};
pub use session::SessionTracker;

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
}

/// Enumerate hardware adapters (DXGI EnumAdapters1).
pub fn adapters() -> Vec<Adapter> {
    win::dxgi_adapters()
}

/// "VEN:DEV" hex (e.g. "1002:731F") -> (vendor id, device id).
pub fn parse_pci(pci: &str) -> Option<(u32, u32)> {
    let (v, d) = pci.trim().split_once(':')?;
    let hex = |s: &str| {
        let s = s.trim();
        let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
        u32::from_str_radix(s, 16).ok()
    };
    Some((hex(v)?, hex(d)?))
}

/// Find an adapter by "VEN:DEV" hex (e.g. "1002:731F").
pub fn find_adapter(pci: &str) -> Option<Adapter> {
    let (ven, dev) = parse_pci(pci)?;
    adapters().into_iter().find(|a| a.vendor_id == ven && a.device_id == dev)
}

/// The card's current device power state (one SetupDi enumeration + property query; read-only).
pub fn power_state(pci: &str) -> Option<PowerState> {
    let (ven, dev) = parse_pci(pci)?;
    win::DisplayDevice::open(ven, dev)?.power_state_raw().and_then(PowerState::from_raw)
}

/// The card's AMD ULPS setting: `EnableUlps` in its driver key under the display class. Read-only.
/// A change takes effect only after a reboot: the value says what the driver does from the next boot
/// on, which is normally what it does now.
#[derive(Debug, Clone)]
pub struct UlpsSetting {
    /// "PCI\VEN_1002&DEV_7550&...".
    pub instance_id: String,
    /// HKLM path of the driver key that was read.
    pub key: String,
    /// The key's DriverDesc (it matched the device's description).
    pub driver_desc: Option<String>,
    /// The EnableUlps DWORD (None: no such value, e.g. a non-AMD card).
    pub enable_ulps: Option<u32>,
}

impl UlpsSetting {
    pub fn is_on(&self) -> bool {
        self.enable_ulps == Some(1)
    }
}

/// Read the card's EnableUlps: from the device's own driver key (SPDRP_DRIVER) when that key's
/// DriverDesc matches the device description, else from the display-class subkey whose DriverDesc
/// matches. None when the device or a matching key is not found.
pub fn ulps_setting(pci: &str) -> Option<UlpsSetting> {
    let (ven, dev) = parse_pci(pci)?;
    let d = win::DisplayDevice::open(ven, dev)?;
    let desc = d.device_desc();
    let read = |key: String| UlpsSetting {
        instance_id: d.instance_id.clone(),
        driver_desc: win::reg_sz(&key, "DriverDesc"),
        enable_ulps: win::reg_dword(&key, "EnableUlps"),
        key: format!(r"HKLM\{key}"),
    };
    let same = |a: &Option<String>| match (a, &desc) {
        (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => false,
    };
    if let Some(drv) = d.driver_key() {
        let s = read(format!(r"{}\{drv}", win::CLASS_KEY));
        if desc.is_none() || same(&s.driver_desc) {
            return Some(s);
        }
    }
    (0..32).map(|i| read(format!(r"{}\{}\{i:04}", win::CLASS_KEY, win::DISPLAY_CLASS))).find(|s| same(&s.driver_desc))
}

/// What to watch for the current session.
#[derive(Debug, Clone)]
pub struct WatchSpec {
    pub kind: SlotKind,
    pub out_log: PathBuf,
    pub err_log: PathBuf,
    pub host: String,
    pub port: u16,
    pub api_key: Option<Secret>,
    /// Session start (seconds since epoch) for elapsed times.
    pub started_at: f64,
    /// Context window from the recipe (used until the server reports n_ctx).
    pub ctx_tokens: Option<u32>,
    /// Speculative mode label, if speculation is configured.
    pub spec_mode: Option<String>,
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

#[derive(Debug, Clone)]
pub struct ServerSignals {
    pub health: Health,
    /// Load steps parsed from logs (process/device/weights/kv/warmup/ready).
    pub load_steps: Vec<LoadStep>,
    pub load_fraction: f64,
    /// VRAM composition parsed from logs (weights/kv/buffers/draft/projector), if available.
    pub layers_from_logs: Option<Vec<VramLayer>>,
    pub llm: Option<LlmLive>,
    pub image: Option<ImageLive>,
    /// Last error-looking lines (for a fault panel), redacted.
    pub error_tail: Vec<String>,
    /// A one-line reason if the logs show a fatal error (e.g. "ROCm error: device kernel image is invalid").
    pub fatal_hint: Option<String>,
    /// Exit code printed by a starter script's own exit line (sd-server `.cmd` starters), if seen.
    pub starter_exit: Option<i64>,
    /// Median decode speed over every finished request of the session (LLM), for the last-session summary.
    pub median_decode_tps: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct TelemetrySnapshot {
    /// Inference GPU memory. `layers` contains session layers (from logs, or one PDH-measured layer)
    /// plus an 'other' baseline layer; `history` is 1 Hz used GiB.
    pub vram: GpuMemory,
    pub system: SystemStats,
    pub server: Option<ServerSignals>,
    /// Last console lines (both logs, interleaved by arrival), redacted, newest last, up to 200.
    pub console: Vec<String>,
    /// Session PIDs' shared (spilled) memory on the inference adapter.
    pub spill_mib: f64,
    /// The dormant detector's facts (also reflected in `vram.dormant` and `vram.layers`).
    pub dormancy: DormancyFacts,
}

struct Shared {
    device: String,
    total_gib: f64,
    warn_below_gib: f64,
    used_gib: f64,
    layers: Vec<VramLayer>,
    baseline_gib: f64,
    spill_mib: f64,
    hist: vram::VramHistory,
    system: SystemStats,
    session: Option<SessionTracker>,
    generation: u64,
    pids: Vec<u32>,
    pids_gen: u64,
    console_kept: Vec<String>,
    /// Adapter usage measured while no session was watched (the pre-session baseline).
    idle_used_gib: Option<f64>,
    /// The watched session's server has answered ready (gates the dormant detector).
    ever_ready: bool,
    dormant: DormantTracker,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Telemetry {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Telemetry {
    /// Start the sampler. `inference` is the cliff GPU; `warn_below_gib` and `verbose` from config.
    pub fn start(inference: Option<Adapter>, warn_below_gib: f64) -> Telemetry {
        let shared = Arc::new(Mutex::new(Shared {
            device: inference.as_ref().map(|a| a.display_name()).unwrap_or_default(),
            total_gib: inference.as_ref().map(|a| text::round_to(a.dedicated_bytes as f64 / text::GIB, 3)).unwrap_or(0.0),
            warn_below_gib,
            used_gib: 0.0,
            layers: Vec::new(),
            baseline_gib: 0.0,
            spill_mib: 0.0,
            hist: vram::VramHistory::default(),
            system: SystemStats { ram_used_gib: 0.0, ram_total_gib: 0.0, ram_type: None, cpu_name: String::new(), cpu_pct: 0.0 },
            session: None,
            generation: 0,
            pids: Vec::new(),
            pids_gen: 0,
            console_kept: Vec::new(),
            idle_used_gib: None,
            ever_ready: false,
            dormant: DormantTracker::default(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        {
            let (sh, st) = (shared.clone(), stop.clone());
            if let Ok(h) = std::thread::Builder::new().name("klif-sampler".into()).spawn(move || sampler(sh, st, inference)) {
                threads.push(h);
            }
        }
        {
            let (sh, st) = (shared.clone(), stop.clone());
            if let Ok(h) = std::thread::Builder::new().name("klif-probe".into()).spawn(move || prober(sh, st)) {
                threads.push(h);
            }
        }
        Telemetry { shared, stop, threads }
    }

    /// Begin watching a session (logs from their current end for a new launch, from the start for an
    /// adopted one: `from_start`).
    pub fn watch(&self, spec: WatchSpec, from_start: bool) {
        let tracker = SessionTracker::new(spec, from_start);
        let mut s = lock(&self.shared);
        s.generation += 1;
        s.console_kept.clear();
        s.session = Some(tracker);
        s.ever_ready = false;
        s.dormant.reset();
    }

    /// Add a KLIF line to the watched session's console, in arrival order with its log lines.
    pub fn note(&self, line: impl Into<String>) {
        let mut s = lock(&self.shared);
        if let Some(t) = s.session.as_mut() {
            t.note(line.into());
        }
    }

    /// The PIDs of the session tree (for per-process GPU memory). Called by the engine each tick.
    pub fn set_session_pids(&self, pids: Vec<u32>) {
        let mut pids = pids;
        pids.sort_unstable();
        pids.dedup();
        let mut s = lock(&self.shared);
        if s.pids != pids {
            s.pids = pids;
            s.pids_gen += 1;
        }
    }

    pub fn unwatch(&self) {
        let mut s = lock(&self.shared);
        if let Some(t) = s.session.take() {
            s.console_kept = t.console();
        }
        s.generation += 1;
        s.pids.clear();
        s.pids_gen += 1;
        s.ever_ready = false;
        s.dormant.reset();
        // Drop the session's layers at once (the 1 Hz sampler would otherwise show them for up to 1 s).
        let used = s.used_gib;
        s.layers = vec![VramLayer { id: klif_common::vm::VramLayerId::Other, label: "other".into(), gib: used }];
        s.baseline_gib = used;
        s.spill_mib = 0.0;
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        let now = klif_common::now_s();
        let s = lock(&self.shared);
        let vram = GpuMemory {
            device: s.device.clone(),
            total_gib: s.total_gib,
            used_gib: text::round_to(s.used_gib, 3),
            layers: s.layers.clone(),
            spill_mib: text::round_to(s.spill_mib, 1),
            history: s.hist.used(),
            layer_history: Some(s.hist.per_layer()),
            baseline_gib: s.baseline_gib,
            warn_below_gib: s.warn_below_gib,
            dormant: s.dormant.current().map(|d| Dormant {
                paged_out_gib: text::round_to(d.paged_out_gib, 3),
                since_s: text::round_to((now - d.since).max(0.0), 1),
                power_state: d.power.map(|p| p.as_str().to_string()),
            }),
        };
        TelemetrySnapshot {
            vram,
            system: s.system.clone(),
            server: s.session.as_ref().map(|t| t.signals(now)),
            console: match &s.session {
                Some(t) => t.console(),
                None => s.console_kept.clone(),
            },
            spill_mib: text::round_to(s.spill_mib, 1),
            dormancy: s.dormant.facts(),
        }
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

// ---------------------------------------------------------------------------------- sampler thread

/// One PDH reading of the inference adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuReading {
    pub used_bytes: f64,
    /// Sum of the session PIDs' dedicated usage on the adapter (None: no PIDs / no instances).
    pub session_bytes: Option<f64>,
    pub session_shared_bytes: f64,
    /// Sum of the session PIDs' Total Committed on the adapter: their allocations, resident or not.
    pub session_committed_bytes: Option<f64>,
}

struct GpuCounters {
    pdh: win::Pdh,
    adapter_ded: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    proc_ded: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    proc_shared: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    proc_committed: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    cpu: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    luid: Option<String>,
}

impl GpuCounters {
    fn open(inference: Option<&Adapter>) -> Option<GpuCounters> {
        let pdh = win::Pdh::open()?;
        let cpu = pdh.add(r"\Processor Information(_Total)\% Processor Utility");
        let (adapter_ded, proc_ded, proc_shared, proc_committed) = if inference.is_some() {
            (
                pdh.add(r"\GPU Adapter Memory(*)\Dedicated Usage"),
                pdh.add(r"\GPU Process Memory(*)\Dedicated Usage"),
                pdh.add(r"\GPU Process Memory(*)\Shared Usage"),
                pdh.add(r"\GPU Process Memory(*)\Total Committed"),
            )
        } else {
            (None, None, None, None)
        };
        pdh.collect();
        Some(GpuCounters { pdh, adapter_ded, proc_ded, proc_shared, proc_committed, cpu, luid: inference.map(|a| a.pdh_luid()) })
    }

    /// Re-add the per-process counters so new PIDs are certainly expanded by the wildcard.
    fn refresh_process_counters(&mut self) {
        if self.luid.is_none() {
            return;
        }
        for c in [self.proc_ded.take(), self.proc_shared.take(), self.proc_committed.take()].into_iter().flatten() {
            self.pdh.remove(c);
        }
        self.proc_ded = self.pdh.add(r"\GPU Process Memory(*)\Dedicated Usage");
        self.proc_shared = self.pdh.add(r"\GPU Process Memory(*)\Shared Usage");
        self.proc_committed = self.pdh.add(r"\GPU Process Memory(*)\Total Committed");
    }

    fn read(&self, pids: &[u32]) -> (Option<GpuReading>, Option<f64>) {
        self.pdh.collect();
        let cpu = self.cpu.and_then(|c| self.pdh.single(c)).map(|v| v.clamp(0.0, 100.0));
        let Some(luid) = &self.luid else { return (None, cpu) };
        let Some(ad) = self.adapter_ded else { return (None, cpu) };
        let used: f64 = self.pdh.array(ad).iter().filter(|(n, _)| n.starts_with(luid.as_str())).map(|(_, v)| *v).sum();
        let sum_for = |c: Option<windows::Win32::System::Performance::PDH_HCOUNTER>| -> Option<f64> {
            let c = c?;
            if pids.is_empty() {
                return None;
            }
            let mut found = false;
            let mut total = 0.0;
            for (n, v) in self.pdh.array(c) {
                if n.contains(luid.as_str()) && win::pid_of(&n).map(|p| pids.binary_search(&p).is_ok()).unwrap_or(false) {
                    found = true;
                    total += v;
                }
            }
            if found { Some(total) } else { None }
        };
        let session = sum_for(self.proc_ded);
        let shared = sum_for(self.proc_shared).unwrap_or(0.0);
        let committed = sum_for(self.proc_committed);
        let reading = GpuReading { used_bytes: used, session_bytes: session, session_shared_bytes: shared, session_committed_bytes: committed };
        (Some(reading), cpu)
    }
}

/// One-off reading of the inference adapter (used by tools and the sample example). Blocks for the
/// PDH provider load (~0.3 s) plus one second between the two collects that rate counters need.
pub fn read_gpu_once(inference: &Adapter, pids: &[u32]) -> Option<GpuReading> {
    let c = GpuCounters::open(Some(inference))?;
    std::thread::sleep(Duration::from_millis(1000));
    let mut pids = pids.to_vec();
    pids.sort_unstable();
    c.read(&pids).0
}

/// One-off system stats (CPU name/utility, RAM). Blocks ~1 s (CPU utility needs two collects).
pub fn read_system_once() -> SystemStats {
    let c = GpuCounters::open(None);
    std::thread::sleep(Duration::from_millis(1000));
    let cpu = c.as_ref().and_then(|c| c.read(&[]).1).unwrap_or(0.0);
    system_stats(cpu, cpu_name(), win::ram_type())
}

fn cpu_name() -> String {
    win::cpu_brand().map(|b| win::short_cpu_name(&b)).unwrap_or_else(|| "CPU".into())
}

fn system_stats(cpu_pct: f64, cpu_name: String, ram_type: Option<String>) -> SystemStats {
    let (total, avail) = win::memory_status().unwrap_or((0, 0));
    SystemStats {
        ram_used_gib: text::round_to((total.saturating_sub(avail)) as f64 / text::GIB, 2),
        ram_total_gib: text::round_to(total as f64 / text::GIB, 1),
        ram_type,
        cpu_name,
        cpu_pct: text::round_to(cpu_pct, 1),
    }
}

/// The card's power state through a device handle kept open (one PnP property query per read, it never
/// touches the device). A failed open or read is retried after 30 s. The sampler reads it at most once
/// per second, only while a session is watched.
pub struct PowerReader {
    ids: Option<(u32, u32)>,
    dev: Option<win::DisplayDevice>,
    retry_at: f64,
}

impl PowerReader {
    pub fn new(inference: Option<&Adapter>) -> PowerReader {
        PowerReader { ids: inference.map(|a| (a.vendor_id, a.device_id)), dev: None, retry_at: 0.0 }
    }

    /// For a "VEN:DEV" PCI id.
    pub fn for_pci(pci: &str) -> PowerReader {
        PowerReader { ids: parse_pci(pci), dev: None, retry_at: 0.0 }
    }

    /// `now`: epoch seconds (for the retry back-off).
    pub fn read(&mut self, now: f64) -> Option<PowerState> {
        let (ven, dev) = self.ids?;
        if self.dev.is_none() {
            if now < self.retry_at {
                return None;
            }
            self.dev = win::DisplayDevice::open(ven, dev);
        }
        match self.dev.as_ref().and_then(|d| d.power_state_raw()) {
            Some(raw) => PowerState::from_raw(raw),
            None => {
                self.dev = None;
                self.retry_at = now + 30.0;
                None
            }
        }
    }
}

fn sampler(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, inference: Option<Adapter>) {
    let name = cpu_name();
    let ram_type = win::ram_type();
    {
        let mut s = lock(&shared);
        s.system = system_stats(0.0, name.clone(), ram_type.clone());
    }
    // The first PdhAddEnglishCounterW loads the provider (~265 ms): do it here, not on the UI thread.
    let mut counters = GpuCounters::open(inference.as_ref());
    let mut power = PowerReader::new(inference.as_ref());
    let mut pids_gen_seen = 0u64;
    let period = Duration::from_millis(500);
    let mut next = Instant::now();
    let mut tick: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let now = klif_common::now_s();
        // Logs at 2 Hz.
        {
            let mut s = lock(&shared);
            if let Some(t) = s.session.as_mut() {
                t.poll(now);
            }
        }
        // GPU / CPU / RAM at 1 Hz.
        if tick.is_multiple_of(2) {
            let (pids, pids_gen, watched) = {
                let s = lock(&shared);
                (s.pids.clone(), s.pids_gen, s.session.is_some())
            };
            if let Some(c) = counters.as_mut() {
                if pids_gen != pids_gen_seen {
                    pids_gen_seen = pids_gen;
                    c.refresh_process_counters();
                }
            }
            let (gpu, cpu) = counters.as_ref().map(|c| c.read(&pids)).unwrap_or((None, None));
            // The power state only matters to the dormant detector: read it only while a session is watched.
            let power_state = if watched { power.read(now) } else { None };
            let sys = system_stats(cpu.unwrap_or(0.0), name.clone(), ram_type.clone());
            let mut s = lock(&shared);
            s.system = sys;
            let used_gib = gpu.map(|g| g.used_bytes / text::GIB).unwrap_or(0.0);
            let in_session = s.session.is_some();
            let log_layers = s.session.as_ref().and_then(|t| t.layers_from_logs());
            let session_gib = gpu.and_then(|g| g.session_bytes).map(|b| b / text::GIB);
            if !in_session && gpu.is_some() {
                s.idle_used_gib = Some(used_gib);
            }
            let comp = vram::compose(used_gib, in_session, session_gib, log_layers.as_deref(), s.idle_used_gib);
            s.used_gib = used_gib;
            // History stays resident memory, dormant or not.
            s.hist.push(used_gib, &comp.layers);
            if in_session {
                let (ready, busy_since) = s.session.as_ref().map(|t| (t.is_ready(), t.busy_since(now))).unwrap_or((false, None));
                s.ever_ready |= ready;
                let sample = dormant::DormantSample {
                    at: now,
                    live: s.ever_ready,
                    committed_gib: gpu.and_then(|g| g.session_committed_bytes).map(|b| b / text::GIB),
                    resident_gib: session_gib,
                    shared_gib: gpu.map(|g| g.session_shared_bytes / text::GIB).unwrap_or(0.0),
                    power: power_state,
                    busy_since,
                };
                s.dormant.sample(&sample);
            }
            // While dormant, the layers are the session's allocations (paged out), not what is resident.
            let comp = match s.dormant.current() {
                Some(d) => vram::compose_dormant(used_gib, d.resident_gib, d.vram_gib, log_layers.as_deref()),
                None => comp,
            };
            s.layers = comp.layers;
            s.baseline_gib = comp.baseline_gib;
            // An image server's shared memory is its host staging buffers (sd-server keeps the weights in RAM by
            // design and copies them per stage), not VRAM overflow: only an LLM session reports spill.
            let llm_session = s.session.as_ref().is_some_and(|t| t.spec().kind == SlotKind::Llm);
            s.spill_mib = if llm_session { gpu.map(|g| g.session_shared_bytes / text::MIB).unwrap_or(0.0) } else { 0.0 };
        }
        tick += 1;
        next += period;
        let now_i = Instant::now();
        if next > now_i {
            std::thread::sleep(next - now_i);
        } else {
            next = now_i;
        }
    }
}

// ------------------------------------------------------------------------------------ probe thread

fn prober(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) {
    let p = probe::Prober::new();
    let mut gen_seen = u64::MAX;
    let mut slots_backoff_until = 0.0f64;
    let mut slots_disabled = false;
    while !stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let job = {
            let s = lock(&shared);
            s.session.as_ref().map(|t| {
                let sp = t.spec();
                (s.generation, sp.kind, sp.host.clone(), sp.port, sp.api_key.clone(), t.has_models())
            })
        };
        let mut period = Duration::from_millis(500);
        if let Some((gen, kind, host, port, key, has_models)) = job {
            if gen != gen_seen {
                gen_seen = gen;
                slots_backoff_until = 0.0;
                slots_disabled = false;
            }
            let apply = |f: &mut dyn FnMut(&mut SessionTracker)| {
                let mut s = lock(&shared);
                if s.generation == gen {
                    if let Some(t) = s.session.as_mut() {
                        f(t);
                    }
                }
            };
            match kind {
                SlotKind::Llm => {
                    let h = p.health(&host, port);
                    let at = klif_common::now_s();
                    apply(&mut |t| t.apply_health(at, h));
                    if h == Health::Ready {
                        if !slots_disabled && at >= slots_backoff_until {
                            match p.slots(&host, port, key.as_ref(), klif_common::now_s) {
                                probe::SlotsResult::Ok(v) => apply(&mut |t| t.apply_slots(&v)),
                                probe::SlotsResult::Unauthorized => slots_backoff_until = at + 30.0,
                                probe::SlotsResult::Unavailable => slots_disabled = true,
                                probe::SlotsResult::Failed => {}
                            }
                        }
                        if !has_models {
                            if let Some(m) = p.models(&host, port, key.as_ref()) {
                                apply(&mut |t| t.apply_models(m.clone()));
                            }
                        }
                    }
                }
                SlotKind::Image => {
                    let l = probe::tcp_listening(&host, port, Duration::from_millis(300));
                    let at = klif_common::now_s();
                    apply(&mut |t| t.apply_listening(at, l));
                    period = Duration::from_millis(1000);
                }
            }
        }
        let el = started.elapsed();
        if el < period {
            // Sleep in short slices so Drop does not wait long.
            let mut left = period - el;
            while left > Duration::ZERO && !stop.load(Ordering::SeqCst) {
                let d = left.min(Duration::from_millis(100));
                std::thread::sleep(d);
                left = left.saturating_sub(d);
            }
        }
    }
}
