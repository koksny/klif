//! Stub engine (cargo feature `stub-engine`): the same API as `klif_core::EngineHandle`, serving view
//! models shaped exactly like the real core's, so the shell runs end to end before the engine exists.
//! Starts from a port of app/ui/src/lib/model/sample.ts (Agent Medium live, decoding), ticks at 2 Hz,
//! and simulates launch / stop / restart / dismiss / setRecipe with a small built-in catalog.
//! It never starts, adopts or stops a process.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{bail, Result};
use klif_common::config::Config;
use klif_common::vm::*;
use klif_common::{now_s, Secret};

const TICK: f64 = 0.5;
const HIST: usize = 300;
const VRAM_TOTAL: f64 = 15.87;
const BASE_VRAM: f64 = 0.12;
const RAM_TOTAL: f64 = 93.6;
const RAM_BASE: f64 = 24.0;
const STUB_KEY: &str = "klif-stub-key-not-a-secret";

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ------------------------------------------------------------------------------------------- catalog

struct Card {
    id: &'static str,
    name: &'static str,
    quant: &'static str,
    kind: SlotKind,
    engine: &'static str,
    weights_gib: f64,
    contexts: &'static [u32],
    default_ctx: u32,
    kv_gib_per_tok_q8: f64,
    buffers_gib: f64,
    vision: bool,
    modes: &'static [&'static str],
    spec: Option<&'static str>,
    image_sizes: &'static [&'static str],
    /// (backend, hardware) pairs with a starter.
    hardware: &'static [&'static str],
    /// Prompt cache cap (MiB) at >= 128k context.
    cache_cap_long: Option<u32>,
}

const CTX: &[u32] = &[16384, 32768, 65536, 98304, 131072, 262144];
const CTX_128K: &[u32] = &[16384, 32768, 65536, 98304, 131072];
const SIZES: &[&str] = &["512x512", "512x768", "640x920", "720x960", "720x1024", "1024x1024"];
const HW_ALL: &[&str] = &["9070", "Dual", "9950X3D"];
const HW: &[(&str, &str, &str)] = &[
    ("9070", "RX 9070 XT", "RX 9070 XT"),
    ("5700", "RX 5700 XT", "RX 5700 XT"),
    ("Dual", "Dual GPU", "RX 9070 XT + 5700 XT"),
    ("9950X3D", "9950X3D (RAM)", "9950X3D"),
];
const BACKENDS: &[Backend] = &[Backend::Hip, Backend::Vulkan];
const CACHE_MIB: &[u32] = &[2048, 8192, 16384, 32768];
const PORTS: &[u16] = &[7030, 7031, 7032, 7033, 7034, 7035];

const fn llm(id: &'static str, name: &'static str, quant: &'static str, weights_gib: f64, default_ctx: u32) -> Card {
    Card {
        id,
        name,
        quant,
        kind: SlotKind::Llm,
        engine: "llama.cpp",
        weights_gib,
        contexts: CTX,
        default_ctx,
        kv_gib_per_tok_q8: 2.9 / 98304.0,
        buffers_gib: 0.9,
        vision: false,
        modes: &[],
        spec: None,
        image_sizes: &[],
        hardware: HW_ALL,
        cache_cap_long: None,
    }
}

static CARDS: &[Card] = &[
    Card { vision: true, spec: Some("MTP+ngram"), ..llm("qwen-gsq", "Qwen 3.8 27B", "GSQ-RCO IQ3_S", 11.6, 98304) },
    Card { vision: true, spec: Some("ngram"), ..llm("qwen-q3k", "Qwen 3.8 27B", "UD-Q3_K_XL", 12.9, 131072) },
    Card {
        contexts: CTX_128K,
        kv_gib_per_tok_q8: 1.6 / 131072.0,
        buffers_gib: 0.99,
        modes: &["Thinking", "Instruct"],
        spec: Some("ngram"),
        hardware: &["9070", "9950X3D"],
        cache_cap_long: Some(2048),
        ..llm("qwen-fn-iq2", "Qwen 3.8 Flash-Next", "IQ2_XXS", 13.2, 131072)
    },
    Card { kv_gib_per_tok_q8: 0.4 / 16384.0, buffers_gib: 0.8, ..llm("gemma-26b", "Gemma 4 26B-A4B", "Q4_0", 13.4, 16384) },
    Card { kv_gib_per_tok_q8: 0.55 / 32768.0, buffers_gib: 0.6, ..llm("gemma-12b", "Gemma 4 12B", "QAT Q4_0", 6.5, 131072) },
    Card {
        kind: SlotKind::Image,
        engine: "sd.cpp",
        contexts: &[],
        buffers_gib: 0.0,
        image_sizes: SIZES,
        hardware: &["9070", "Dual"],
        ..llm("krea-realism", "Krea 2 Realism Turbo", "Q8_0", 7.4, 0)
    },
    Card {
        kind: SlotKind::Image,
        engine: "sd.cpp",
        contexts: &[],
        buffers_gib: 0.0,
        image_sizes: SIZES,
        hardware: &["9070", "Dual"],
        ..llm("krea-muse", "Krea 2 Muse", "Q8_0", 7.4, 0)
    },
];

fn card(id: &str) -> Option<&'static Card> {
    CARDS.iter().find(|c| c.id == id)
}

fn hw(value: &str) -> (&'static str, &'static str) {
    HW.iter().find(|h| h.0 == value).map(|h| (h.1, h.2)).unwrap_or(("unknown", "unknown"))
}

fn backend_name(b: Backend) -> &'static str {
    match b {
        Backend::Hip => "HIP",
        Backend::Vulkan => "Vulkan",
        Backend::Cpu => "CPU",
    }
}

fn combo(c: &Card, backend: Backend, hardware: &str) -> (Availability, Option<String>) {
    if backend == Backend::Cpu || !c.hardware.contains(&hardware) {
        return (Availability::Unsupported, Some(format!("No {} starter for {}.", backend_name(backend), hw(hardware).0)));
    }
    (Availability::Ready, None)
}

fn default_recipe(slot: SlotId) -> Recipe {
    let base = |card_id: &str, ctx: Option<u32>, cache: Option<u32>, port: u16| Recipe {
        card_id: card_id.into(),
        backend: Backend::Hip,
        hardware: "9070".into(),
        ctx_tokens: ctx,
        kv_type: ctx.map(|_| "q8_0".into()),
        prompt_cache_mib: cache,
        port: Some(port),
        ..Default::default()
    };
    match slot {
        SlotId::High => Recipe { mode: Some("Thinking".into()), ..base("qwen-fn-iq2", Some(131072), Some(2048), 7030) },
        SlotId::Medium => Recipe { vision: Some(true), ..base("qwen-gsq", Some(98304), Some(16384), 7030) },
        SlotId::Low => base("gemma-26b", Some(16384), Some(8192), 7030),
        SlotId::Krea => Recipe { image_size: Some("512x768".into()), ..base("krea-realism", None, None, 1234) },
    }
}

fn apply_patch(kind: SlotKind, cur: &Recipe, p: &RecipePatch) -> Recipe {
    let mut n = cur.clone();
    if let Some(id) = &p.card_id {
        if card(id).map(|c| c.kind == kind).unwrap_or(false) {
            n.card_id = id.clone();
        }
    }
    let Some(c) = card(&n.card_id) else { return cur.clone() };
    if let Some(b) = p.backend {
        n.backend = b;
    }
    if let Some(h) = &p.hardware {
        if HW.iter().any(|x| x.0 == h) {
            n.hardware = h.clone();
        }
    }
    if c.kind == SlotKind::Llm {
        if let Some(ctx) = p.ctx_tokens.filter(|x| c.contexts.contains(x)) {
            n.ctx_tokens = Some(ctx);
        }
        if !n.ctx_tokens.map(|x| c.contexts.contains(&x)).unwrap_or(false) {
            n.ctx_tokens = Some(c.default_ctx);
        }
        if let Some(kv) = p.kv_type.as_deref().filter(|k| *k == "q4_0" || *k == "q8_0") {
            n.kv_type = Some(kv.into());
        }
        n.kv_type.get_or_insert_with(|| "q8_0".into());
        if let Some(m) = p.prompt_cache_mib.filter(|m| CACHE_MIB.contains(m)) {
            n.prompt_cache_mib = Some(m);
        }
        let mut cache = n.prompt_cache_mib.unwrap_or(8192);
        if let (Some(cap), Some(ctx)) = (c.cache_cap_long, n.ctx_tokens) {
            if ctx >= 131072 {
                cache = cache.min(cap);
            }
        }
        n.prompt_cache_mib = Some(cache);
        if let Some(port) = p.port.filter(|x| PORTS.contains(x)) {
            n.port = Some(port);
        }
        n.port.get_or_insert(7030);
        n.vision = if c.vision { Some(p.vision.or(n.vision).unwrap_or(true)) } else { None };
        n.mode = if c.modes.is_empty() {
            None
        } else {
            let want = p.mode.clone().or(n.mode.clone()).filter(|m| c.modes.contains(&m.as_str()));
            Some(want.unwrap_or_else(|| c.modes[0].into()))
        };
        n.image_size = None;
    } else {
        if let Some(s) = p.image_size.clone().filter(|s| c.image_sizes.contains(&s.as_str())) {
            n.image_size = Some(s);
        }
        if !n.image_size.as_deref().map(|s| c.image_sizes.contains(&s)).unwrap_or(false) {
            n.image_size = Some("512x768".into());
        }
        n.ctx_tokens = None;
        n.kv_type = None;
        n.prompt_cache_mib = None;
        n.vision = None;
        n.mode = None;
    }
    n
}

fn model_from_recipe(r: &Recipe, device_name: &str) -> ModelRef {
    let device = match r.hardware.as_str() {
        "9070" => device_name.to_string(),
        other => hw(other).1.to_string(),
    };
    let Some(c) = card(&r.card_id) else {
        return ModelRef { name: r.card_id.clone(), backend: r.backend, device, ..Default::default() };
    };
    let mut m = ModelRef {
        name: c.name.into(),
        quant: c.quant.into(),
        engine: c.engine.into(),
        backend: r.backend,
        device,
        weights_gib: Some(c.weights_gib),
        ..Default::default()
    };
    if c.kind == SlotKind::Llm {
        m.ctx_tokens = r.ctx_tokens;
        m.kv_type = r.kv_type.clone();
        m.spec_mode = c.spec.map(Into::into);
        m.vision = c.vision.then(|| r.vision.unwrap_or(false));
        m.mode = r.mode.clone();
    } else {
        m.image_size = r.image_size.clone();
    }
    m
}

fn choice<T>(value: T, label: String, (availability, reason): (Availability, Option<String>)) -> RecipeChoice<T> {
    RecipeChoice { value, label, availability, reason }
}

fn options_for(kind: SlotKind, r: &Recipe) -> RecipeOptions {
    let c = card(&r.card_id);
    let mut o = RecipeOptions {
        cards: CARDS
            .iter()
            .filter(|x| x.kind == kind)
            .map(|x| {
                let (a, reason) = combo(x, r.backend, &r.hardware);
                CardChoice {
                    value: x.id.into(),
                    label: format!("{} · {}", x.name, x.quant),
                    availability: a,
                    reason,
                    name: x.name.into(),
                    quant: x.quant.into(),
                }
            })
            .collect(),
        backends: BACKENDS
            .iter()
            .map(|b| {
                let a = c.map(|c| combo(c, *b, &r.hardware)).unwrap_or((Availability::Unsupported, None));
                choice(*b, backend_name(*b).to_string(), a)
            })
            .collect(),
        hardware: HW
            .iter()
            .map(|h| {
                let a = c.map(|c| combo(c, r.backend, h.0)).unwrap_or((Availability::Unsupported, None));
                choice(h.0.to_string(), h.1.to_string(), a)
            })
            .collect(),
        ..Default::default()
    };
    let Some(c) = c else { return o };
    if c.kind == SlotKind::Llm {
        o.contexts = Some(
            c.contexts
                .iter()
                .map(|x| RecipeChoice { value: *x, label: format!("{}k tokens", x / 1024), availability: Availability::Ready, reason: None })
                .collect(),
        );
        o.kv_types = Some(vec!["q4_0".into(), "q8_0".into()]);
        let cap = match (c.cache_cap_long, r.ctx_tokens) {
            (Some(cap), Some(ctx)) if ctx >= 131072 => cap,
            _ => u32::MAX,
        };
        o.prompt_cache_mib = Some(CACHE_MIB.iter().copied().filter(|m| *m <= cap).collect());
        o.ports = Some(PORTS.to_vec());
        o.vision = Some(c.vision);
        if !c.modes.is_empty() {
            o.modes = Some(c.modes.iter().map(|m| m.to_string()).collect());
        }
    } else {
        o.image_sizes = Some(
            c.image_sizes
                .iter()
                .map(|s| RecipeChoice { value: s.to_string(), label: s.to_string(), availability: Availability::Ready, reason: None })
                .collect(),
        );
    }
    o
}

fn expected_layers(r: &Recipe) -> Vec<VramLayer> {
    let Some(c) = card(&r.card_id) else { return Vec::new() };
    let layer = |id, label: &str, gib: f64| VramLayer { id, label: label.into(), gib: r2(gib) };
    if c.kind == SlotKind::Llm {
        let ctx = r.ctx_tokens.unwrap_or(c.default_ctx) as f64;
        let factor = if r.kv_type.as_deref() == Some("q4_0") { 0.53 } else { 1.0 };
        let mut v = vec![
            layer(VramLayerId::Weights, "weights", c.weights_gib),
            layer(VramLayerId::Kv, "KV cache", c.kv_gib_per_tok_q8 * ctx * factor),
            layer(VramLayerId::Buffers, "buffers", c.buffers_gib),
        ];
        if c.spec.map(|s| s.contains("MTP")).unwrap_or(false) {
            v.push(layer(VramLayerId::Draft, "draft", 0.12));
        }
        v
    } else {
        let (w, h) = r
            .image_size
            .as_deref()
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<f64>().ok()?, h.parse::<f64>().ok()?)))
            .unwrap_or((512.0, 768.0));
        vec![
            layer(VramLayerId::Weights, "DiT layers", c.weights_gib * 6.1 / 7.4),
            layer(VramLayerId::Buffers, "activations", 3.2 * w * h / (512.0 * 768.0)),
            layer(VramLayerId::Other, "VAE", 0.6),
        ]
    }
}

fn slot_with_recipe(id: SlotId, r: &Recipe, device_name: &str) -> Slot {
    let (availability, reason) = card(&r.card_id)
        .map(|c| combo(c, r.backend, &r.hardware))
        .unwrap_or((Availability::ModelMissing, Some("Unknown card.".into())));
    Slot {
        id,
        label: id.label().into(),
        kind: id.kind(),
        model: model_from_recipe(r, device_name),
        availability,
        reason,
        expected_vram: Some(expected_layers(r)),
        recipe: Some(r.clone()),
        options: Some(options_for(id.kind(), r)),
    }
}

// --------------------------------------------------------------------------------------- sample port

fn sample_decode_history() -> Vec<f64> {
    let mut out: Vec<f64> = (0..HIST)
        .map(|i| {
            let t = i as f64 / 299.0;
            r1(41.0 + 5.0 * t + 1.6 * (i as f64 * 0.21).sin() + 0.9 * (i as f64 * 0.057 + 1.3).sin())
        })
        .collect();
    out[HIST - 1] = 47.3;
    out
}

fn sample_vram_history() -> Vec<f64> {
    (0..HIST).map(|i| r2(15.52 - if i < 20 { (20 - i) as f64 * 0.01 } else { 0.0 })).collect()
}

fn sample_llm(now: f64) -> LlmLive {
    let req = |id, ago: f64, p, c, ps, g, ds| RequestRecord { id, at: now - ago, prompt_tokens: p, cached_tokens: c, prefill_s: ps, generated_tokens: g, decode_s: ds };
    LlmLive {
        activity: LlmActivity::Decode,
        decode_tps: 47.3,
        decode_history: sample_decode_history(),
        prefill: Some(Prefill { tokens: 18432, done_tokens: 18432, cached_tokens: None, tps: 812.0, elapsed_s: 22.7, eta_s: 0.0 }),
        generated_tokens: 1284,
        context: ContextFill { used_tokens: 19716, total_tokens: 98304 },
        spec: Some(Spec { acceptance_pct: 71.0, mode: "MTP + n-gram".into(), active: None }),
        requests: vec![
            req(5, 900.0, 6120, 4096, 3.1, 512, 11.2),
            req(6, 780.0, 8400, 6100, 3.4, 760, 16.9),
            req(7, 640.0, 9900, 8300, 2.4, 1020, 22.1),
            req(8, 500.0, 11800, 9800, 3.0, 640, 13.8),
            req(9, 380.0, 13200, 11700, 2.6, 1410, 30.5),
            req(10, 240.0, 15100, 13100, 3.2, 880, 18.4),
            req(11, 120.0, 16600, 15000, 2.7, 1190, 25.0),
            req(12, 30.0, 18432, 0, 22.7, 1284, 27.1),
        ],
        totals: Totals { requests: 12, prompt_tokens: 152_300, generated_tokens: 11_420 },
    }
}

fn spec_label(mode: &str) -> String {
    mode.replace('+', " + ").replace("ngram", "n-gram")
}

// ------------------------------------------------------------------------------------------- engine

type Sub = Box<dyn Fn(&ViewModel) + Send + Sync>;

pub struct Engine;

#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    subs: Mutex<Vec<Sub>>,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct State {
    vm: ViewModel,
    recipes: BTreeMap<SlotId, Recipe>,
    device: String,
    llm_host: String,
    image_host: String,
    tick: u64,
    /// Seconds in the current session phase (or activity cycle while live).
    phase_t: f64,
    started_at: f64,
    session_layers: Vec<VramLayer>,
    pending_launch: Option<SlotId>,
    cpu: f64,
}

impl Engine {
    pub fn start(cfg: Config, host: HostInfo) -> Result<EngineHandle> {
        let device = cfg.gpu.inference_name.clone().unwrap_or_else(|| "RX 9070 XT".into());
        let recipes: BTreeMap<SlotId, Recipe> = SlotId::ALL.iter().map(|s| (*s, default_recipe(*s))).collect();
        let slots: Vec<Slot> = SlotId::ALL.iter().map(|s| slot_with_recipe(*s, &recipes[s], &device)).collect();
        let now = now_s();
        let medium = slots.iter().find(|s| s.id == SlotId::Medium).unwrap().clone();
        let layers = vec![
            VramLayer { id: VramLayerId::Weights, label: "weights".into(), gib: 11.6 },
            VramLayer { id: VramLayerId::Kv, label: "KV cache".into(), gib: 2.9 },
            VramLayer { id: VramLayerId::Buffers, label: "buffers".into(), gib: 0.9 },
            VramLayer { id: VramLayerId::Draft, label: "draft".into(), gib: 0.12 },
        ];
        let vm = ViewModel {
            now,
            slots,
            selected: SlotId::Medium,
            session: Some(Session {
                slot: SlotId::Medium,
                model: medium.model.clone(),
                phase: Phase::Live,
                uptime_s: (2 * 3600 + 14 * 60 + 7) as f64,
                endpoint: Endpoint { host: cfg.net.llm_host.clone(), port: 7030 },
                api_key_set: true,
                loading: None,
                fault: None,
                llm: Some(sample_llm(now)),
                image: None,
            }),
            vram: GpuMemory {
                device: device.clone(),
                total_gib: VRAM_TOTAL,
                used_gib: 15.52,
                layers: layers.clone(),
                spill_mib: 0.0,
                history: sample_vram_history(),
                layer_history: None,
                baseline_gib: 0.0,
                warn_below_gib: cfg.telemetry.warn_below_gib,
                dormant: None,
            },
            system: SystemStats { ram_used_gib: 31.2, ram_total_gib: RAM_TOTAL, ram_type: Some("DDR5".into()), cpu_name: "9950X3D".into(), cpu_pct: 6.0 },
            last_session: None,
            host,
            console: vec![
                "srv  load_model: loading model GSQ-RCO IQ3_S".into(),
                "main: server is listening on :7030".into(),
                "slot 0 · task 12 · prompt 18432 tok · cache 0".into(),
                "slot 0 · prompt done · 812 t/s".into(),
                "slot 0 · n_past 19716 · 47.3 t/s".into(),
            ],
        };
        let state = State {
            vm,
            recipes,
            device,
            llm_host: cfg.net.llm_host.clone(),
            image_host: cfg.net.image_host.clone(),
            tick: 0,
            // Inside the decode part of the activity cycle, like the sample.
            phase_t: 20.0,
            started_at: now - 8047.0,
            session_layers: layers,
            pending_launch: None,
            cpu: 6.0,
        };
        let inner = Arc::new(Inner { state: Mutex::new(state), subs: Mutex::new(Vec::new()), stop: AtomicBool::new(false), thread: Mutex::new(None) });
        let worker = inner.clone();
        let t = std::thread::Builder::new().name("klif-stub-engine".into()).spawn(move || {
            while !worker.stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis((TICK * 1000.0) as u64));
                if worker.stop.load(Ordering::Relaxed) {
                    break;
                }
                let vm = {
                    let mut s = worker.state.lock().unwrap();
                    s.advance();
                    s.vm.clone()
                };
                worker.publish(&vm);
            }
        })?;
        *inner.thread.lock().unwrap() = Some(t);
        log::info!("stub engine started (sample data; no process is ever started)");
        Ok(EngineHandle { inner })
    }
}

impl Inner {
    fn publish(&self, vm: &ViewModel) {
        for f in self.subs.lock().unwrap().iter() {
            f(vm);
        }
    }
}

impl EngineHandle {
    pub fn snapshot(&self) -> ViewModel {
        let mut s = self.inner.state.lock().unwrap();
        s.vm.now = now_s();
        s.vm.clone()
    }

    pub fn subscribe(&self, f: Box<dyn Fn(&ViewModel) + Send + Sync>) {
        self.inner.subs.lock().unwrap().push(f);
    }

    pub fn act(&self, action: Action) -> Result<()> {
        let vm = {
            let mut s = self.inner.state.lock().unwrap();
            s.act(action)?;
            s.vm.now = now_s();
            s.vm.clone()
        };
        self.inner.publish(&vm);
        Ok(())
    }

    pub fn set_host(&self, host: HostInfo) {
        self.inner.state.lock().unwrap().vm.host = host;
    }

    pub fn endpoint_url(&self) -> Option<String> {
        let s = self.inner.state.lock().unwrap();
        let sess = s.vm.session.as_ref()?;
        Some(format!("http://{}:{}/", sess.endpoint.host, sess.endpoint.port))
    }

    pub fn api_key(&self) -> Option<Secret> {
        let s = self.inner.state.lock().unwrap();
        let sess = s.vm.session.as_ref()?;
        if sess.api_key_set { Secret::new(STUB_KEY) } else { None }
    }

    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.inner.thread.lock().unwrap().take() {
            let _ = t.join();
        }
        log::info!("stub engine stopped");
    }
}

impl State {
    fn slot(&self, id: SlotId) -> &Slot {
        self.vm.slots.iter().find(|s| s.id == id).expect("all four slots exist")
    }

    fn act(&mut self, action: Action) -> Result<()> {
        let running = self.vm.session.as_ref().map(|s| s.phase != Phase::Fault).unwrap_or(false);
        match action {
            Action::Select { slot } => {
                if running {
                    bail!("Stop the running session to change the slot.");
                }
                self.vm.selected = slot;
            }
            Action::Launch { slot } => {
                if running {
                    bail!("A session is already running. Stop it first.");
                }
                self.launch(slot.unwrap_or(self.vm.selected))?;
            }
            Action::Stop => match self.vm.session.as_ref().map(|s| s.phase) {
                None => bail!("Nothing is running."),
                Some(Phase::Fault) => self.end_session(Ended::Fault),
                Some(Phase::Stopping) => {}
                Some(_) => self.begin_stop(),
            },
            Action::Restart => match self.vm.session.as_ref().map(|s| (s.phase, s.slot)) {
                None => bail!("Nothing is running."),
                Some((Phase::Fault, slot)) => {
                    self.end_session(Ended::Fault);
                    self.launch(slot)?;
                }
                Some((_, slot)) => {
                    self.pending_launch = Some(slot);
                    self.begin_stop();
                }
            },
            Action::Dismiss => match self.vm.session.as_ref().map(|s| s.phase) {
                Some(Phase::Fault) => self.end_session(Ended::Fault),
                Some(_) => bail!("Stop the running session first."),
                None => {}
            },
            Action::SetRecipe { slot, patch } => {
                let next = apply_patch(slot.kind(), &self.recipes[&slot], &patch);
                let s = slot_with_recipe(slot, &next, &self.device);
                self.recipes.insert(slot, next);
                if let Some(x) = self.vm.slots.iter_mut().find(|x| x.id == slot) {
                    *x = s;
                }
            }
        }
        Ok(())
    }

    fn begin_stop(&mut self) {
        if let Some(s) = self.vm.session.as_mut() {
            if s.phase != Phase::Stopping {
                s.phase = Phase::Stopping;
                s.loading = None;
                self.phase_t = 0.0;
            }
        }
        self.push_console("srv  stop: shutting the server down");
    }

    fn launch(&mut self, id: SlotId) -> Result<()> {
        let slot = self.slot(id).clone();
        if slot.availability != Availability::Ready {
            bail!("{} cannot start: {}", slot.label, slot.reason.unwrap_or_else(|| format!("{:?}.", slot.availability)));
        }
        let recipe = &self.recipes[&id];
        let host = if id.kind() == SlotKind::Image { self.image_host.clone() } else { self.llm_host.clone() };
        let port = recipe.port.unwrap_or(if id.kind() == SlotKind::Image { 1234 } else { 7030 });
        self.vm.selected = id;
        self.session_layers = slot.expected_vram.clone().unwrap_or_default();
        self.started_at = now_s();
        self.phase_t = 0.0;
        self.vm.console.clear();
        self.push_console(&format!("stub: launching {} ({})", slot.label, slot.model.name));
        self.vm.session = Some(Session {
            slot: id,
            model: slot.model.clone(),
            phase: Phase::Starting,
            uptime_s: 0.0,
            endpoint: Endpoint { host, port },
            api_key_set: id.kind() == SlotKind::Llm,
            loading: None,
            fault: None,
            llm: None,
            image: None,
        });
        let lp = self.load_progress(0.0);
        if let Some(s) = self.vm.session.as_mut() {
            s.loading = Some(lp);
        }
        Ok(())
    }

    fn end_session(&mut self, ended: Ended) {
        let Some(sess) = self.vm.session.take() else { return };
        let llm = sess.llm.as_ref();
        let img = sess.image.as_ref();
        self.vm.last_session = Some(LastSession {
            slot: sess.slot,
            model: sess.model.clone(),
            uptime_s: sess.uptime_s.round(),
            ended_ago_s: 0.0,
            ended,
            requests: llm.map(|l| l.totals.requests),
            generated_tokens: llm.map(|l| l.totals.generated_tokens),
            decode_tps: llm.map(|l| r1(l.decode_tps)).filter(|v| *v > 0.0),
            images: img.map(|i| i.images_this_session),
            seconds_per_image: img.and_then(|i| i.recent.last().map(|j| r1(j.seconds))),
        });
        self.session_layers.clear();
        self.push_console("srv  stopped");
        if let Some(next) = self.pending_launch.take() {
            let _ = self.launch(next);
        }
    }

    fn push_console(&mut self, line: &str) {
        self.vm.console.push(line.to_string());
        let n = self.vm.console.len();
        if n > 200 {
            self.vm.console.drain(..n - 200);
        }
    }

    /// Loading: 1 s start + 4 s of load steps.
    fn load_progress(&self, t: f64) -> LoadProgress {
        let model = self.vm.session.as_ref().map(|s| s.model.clone()).unwrap_or_default();
        let img = model.engine == "sd.cpp";
        let order: [(LoadStepId, &str, f64); 5] = [
            (LoadStepId::Process, "Start process", 0.5),
            (LoadStepId::Device, "Find device", 0.5),
            (LoadStepId::Weights, if img { "Load diffusion weights" } else { "Load weights" }, 2.5),
            (LoadStepId::Kv, if img { "Load text encoder and VAE" } else { "Allocate KV cache" }, 1.0),
            (LoadStepId::Warmup, "Warm up", 0.5),
        ];
        let total: f64 = order.iter().map(|o| o.2).sum();
        let mut t0 = 0.0;
        let mut steps = Vec::new();
        for (id, label, dur) in order {
            let u = ((t - t0) / dur).clamp(0.0, 1.0);
            let state = if t >= t0 + dur { StepState::Done } else if t >= t0 { StepState::Active } else { StepState::Pending };
            let detail = match (id, state) {
                (_, StepState::Pending) => None,
                (LoadStepId::Process, _) => Some(format!("{} · {}", model.engine, backend_name(model.backend))),
                (LoadStepId::Device, _) => Some("ROCm · gfx1201".into()),
                (LoadStepId::Weights, _) => {
                    let w = model.weights_gib.unwrap_or(0.0);
                    Some(format!("{:.1} / {:.1} GiB", w * u, w))
                }
                (LoadStepId::Kv, _) if img => Some("text encoder + VAE".into()),
                (LoadStepId::Kv, _) => Some(format!("{}k · {}", model.ctx_tokens.unwrap_or(0) / 1024, model.kv_type.clone().unwrap_or_default())),
                _ => None,
            };
            steps.push(LoadStep { id, label: label.into(), state, detail });
            t0 += dur;
        }
        steps.push(LoadStep { id: LoadStepId::Ready, label: "Ready".into(), state: if t >= total { StepState::Done } else { StepState::Pending }, detail: None });
        LoadProgress { steps, fraction: r2((t / total).clamp(0.0, 1.0)), elapsed_s: r1(t) }
    }

    fn go_live(&mut self) {
        let Some(sess) = self.vm.session.as_mut() else { return };
        sess.phase = Phase::Live;
        sess.loading = None;
        if sess.slot.kind() == SlotKind::Llm {
            let total = sess.model.ctx_tokens.unwrap_or(16384) as u64;
            sess.llm = Some(LlmLive {
                activity: LlmActivity::Idle,
                decode_tps: 0.0,
                decode_history: vec![0.0; HIST],
                prefill: None,
                generated_tokens: 0,
                context: ContextFill { used_tokens: 0, total_tokens: total },
                spec: sess.model.spec_mode.as_deref().map(|m| Spec { acceptance_pct: 0.0, mode: spec_label(m), active: None }),
                requests: Vec::new(),
                totals: Totals::default(),
            });
        } else {
            let (w, h) = sess
                .model
                .image_size
                .as_deref()
                .and_then(|s| s.split_once('x'))
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .unwrap_or((512, 768));
            sess.image = Some(ImageLive {
                activity: ImageActivity::Idle,
                step: 0,
                steps: 8,
                s_per_it: 0.0,
                elapsed_s: 0.0,
                width: w,
                height: h,
                edit: false,
                recent: Vec::new(),
                images_this_session: 0,
            });
        }
        let port = sess.endpoint.port;
        self.phase_t = 0.0;
        self.push_console(&format!("main: server is listening on :{port}"));
    }

    /// One 0.5 s step of the simulation.
    fn advance(&mut self) {
        self.tick += 1;
        self.phase_t += TICK;
        let now = now_s();
        self.vm.now = now;
        let second = self.tick % 2 == 0;
        if let Some(ls) = self.vm.last_session.as_mut() {
            ls.ended_ago_s = r1(ls.ended_ago_s + TICK);
        }

        let phase = self.vm.session.as_ref().map(|s| s.phase);
        match phase {
            Some(Phase::Starting) | Some(Phase::Loading) => {
                let t = self.phase_t;
                let lp = self.load_progress(t);
                let done = t >= 5.0;
                if let Some(s) = self.vm.session.as_mut() {
                    s.uptime_s = r1(t);
                    s.phase = if t < 1.0 { Phase::Starting } else { Phase::Loading };
                    s.loading = Some(lp);
                }
                if done {
                    self.go_live();
                }
            }
            Some(Phase::Live) => self.step_live(second),
            Some(Phase::Stopping) => {
                if self.phase_t >= 1.5 {
                    self.end_session(Ended::Stopped);
                }
            }
            Some(Phase::Fault) => {
                if let Some(f) = self.vm.session.as_mut().and_then(|s| s.fault.as_mut()) {
                    f.since_s = r1(f.since_s + TICK);
                }
            }
            None => {}
        }
        if let Some(s) = self.vm.session.as_mut() {
            if s.phase == Phase::Live {
                s.uptime_s = r1(s.uptime_s + TICK);
            }
        }
        self.step_resources(second);
    }

    fn step_live(&mut self, second: bool) {
        let t = self.phase_t;
        let tick = self.tick as f64;
        let now = self.vm.now;
        let Some(sess) = self.vm.session.as_mut() else { return };
        let mut line: Option<String> = None;
        if let Some(l) = sess.llm.as_mut() {
            // A 34 s cycle: 6 s idle, 8 s prefill, 20 s decode.
            let c = t % 34.0;
            let req_no = l.totals.requests + 1;
            if c < 6.0 {
                l.activity = LlmActivity::Idle;
                l.decode_tps = 0.0;
                if let Some(s) = l.spec.as_mut() {
                    s.active = None;
                }
            } else if c < 14.0 {
                let tokens = 12000;
                let done = (((c - 6.0) / 8.0) * tokens as f64) as u64;
                if l.activity != LlmActivity::Prefill {
                    line = Some(format!("slot 0 · task {req_no} · prompt {tokens} tok"));
                    l.generated_tokens = 0;
                }
                l.activity = LlmActivity::Prefill;
                l.prefill = Some(Prefill { tokens, done_tokens: done, cached_tokens: Some(4096), tps: 1500.0, elapsed_s: r1(c - 6.0), eta_s: r1(14.0 - c) });
            } else {
                if l.activity == LlmActivity::Prefill {
                    if let Some(p) = l.prefill.as_mut() {
                        p.done_tokens = p.tokens;
                        p.eta_s = 0.0;
                    }
                    l.context.used_tokens = (l.context.used_tokens + 12000).min(l.context.total_tokens);
                    l.totals.requests += 1;
                    l.totals.prompt_tokens += 16096;
                }
                l.activity = LlmActivity::Decode;
                l.decode_tps = r1(47.3 + 1.6 * (tick * 0.37).sin());
                let gen = (l.decode_tps * TICK) as u64;
                l.generated_tokens += gen;
                l.totals.generated_tokens += gen;
                l.context.used_tokens = (l.context.used_tokens + gen).min(l.context.total_tokens);
                if let Some(s) = l.spec.as_mut() {
                    s.acceptance_pct = r1(70.0 + 6.0 * (tick * 0.11).sin());
                    s.active = Some(true);
                }
                if c + TICK >= 34.0 {
                    let id = l.requests.last().map(|r| r.id + 1).unwrap_or(1);
                    l.requests.push(RequestRecord { id, at: now, prompt_tokens: 16096, cached_tokens: 4096, prefill_s: 8.0, generated_tokens: l.generated_tokens, decode_s: 20.0 });
                    let n = l.requests.len();
                    if n > 12 {
                        l.requests.drain(..n - 12);
                    }
                    line = Some(format!("slot 0 · n_past {} · {} t/s", l.context.used_tokens, l.decode_tps));
                }
            }
            if second {
                l.decode_history.push(l.decode_tps);
                let n = l.decode_history.len();
                if n > HIST {
                    l.decode_history.drain(..n - HIST);
                }
            }
        }
        if let Some(im) = sess.image.as_mut() {
            // 8 steps at 1.6 s/it, then 10 s idle.
            let c = t % 22.8;
            if c < 12.8 {
                im.activity = ImageActivity::Generating;
                im.step = ((c / 1.6) as u32 + 1).min(8);
                im.s_per_it = 1.6;
                im.elapsed_s = r1(c);
            } else {
                if im.activity == ImageActivity::Generating {
                    im.recent.push(ImageJob { at: now, seconds: 12.8, width: im.width, height: im.height, edit: false });
                    let n = im.recent.len();
                    if n > 24 {
                        im.recent.drain(..n - 24);
                    }
                    im.images_this_session += 1;
                    line = Some(format!("[INFO ] stub - image {} done in 12.8 s", im.images_this_session));
                }
                im.activity = ImageActivity::Idle;
                im.step = 0;
            }
        }
        if let Some(l) = line {
            self.push_console(&l);
        }
    }

    fn step_resources(&mut self, second: bool) {
        // VRAM: the session's layers while loading/live/stopping, scaled by progress; 'other' while idle.
        let phase = self.vm.session.as_ref().map(|s| s.phase);
        let scale = match phase {
            Some(Phase::Loading) | Some(Phase::Starting) => {
                self.vm.session.as_ref().and_then(|s| s.loading.as_ref()).map(|l| l.fraction).unwrap_or(0.0)
            }
            Some(Phase::Live) => 1.0,
            Some(Phase::Stopping) => (1.0 - self.phase_t / 1.5).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let mut layers: Vec<VramLayer> = self
            .session_layers
            .iter()
            .map(|l| VramLayer { gib: r2(l.gib * scale), ..l.clone() })
            .filter(|l| l.gib > 0.004)
            .collect();
        let sample_live = self.vm.vram.baseline_gib == 0.0 && phase == Some(Phase::Live);
        if !sample_live {
            layers.insert(0, VramLayer { id: VramLayerId::Other, label: "other".into(), gib: BASE_VRAM });
            self.vm.vram.baseline_gib = BASE_VRAM;
        }
        let used = r2(layers.iter().map(|l| l.gib).sum::<f64>());
        self.vm.vram.spill_mib = ((used - VRAM_TOTAL).max(0.0) * 1024.0).round();
        self.vm.vram.used_gib = used.min(VRAM_TOTAL);
        self.vm.vram.layers = layers;
        if second {
            self.vm.vram.history.push(self.vm.vram.used_gib);
            let n = self.vm.vram.history.len();
            if n > HIST {
                self.vm.vram.history.drain(..n - HIST);
            }
        }
        // CPU / RAM: phase-dependent targets with a gentle wobble.
        let tick = self.tick as f64;
        let cpu_t = match phase {
            Some(Phase::Loading) => 24.0,
            Some(Phase::Starting) | Some(Phase::Stopping) => 10.0,
            Some(Phase::Live) => 6.0,
            _ => 2.5,
        };
        self.cpu += (cpu_t - self.cpu) * 0.3;
        self.vm.system.cpu_pct = (self.cpu + 0.8 * (tick * 0.9).sin()).max(0.5).round();
        let ram_t = if phase.is_some() { 31.2 } else { RAM_BASE };
        self.vm.system.ram_used_gib = r1(self.vm.system.ram_used_gib + (ram_t - self.vm.system.ram_used_gib) * 0.2);
        let _ = self.started_at;
    }
}
