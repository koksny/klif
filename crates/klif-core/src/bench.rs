//! `klif-cli bench <system>`: launch (only with --yes), stream fixed OpenAI requests, record load / TTFT /
//! prefill / decode / peak VRAM per run, stop only what it started; results in `<data_dir>\bench\<preset-id>.json`
//! (an array of `BenchRecord`). Refuses while another System runs on the same GPU unless `allow_shared`.
//! Owner: package E2 (the engine reads `latest`).
//!
//! LLM: streaming `POST /v1/chat/completions` per run with a unique nonce prefix (no prompt-cache reuse) and, for
//! llama.cpp, `cache_prompt: false` + a wait until `/slots` is idle; tokens are counted from `reasoning_content` +
//! `content`, server `timings` (llama.cpp) are preferred over client clocks, `stream_options.include_usage` gives
//! vLLM/OpenAI token counts. Image: `POST /v1/images/generations` (the sd.cpp server's OpenAI route; the image
//! uses the server's launch defaults), seconds per image = wall time. TTS: fixed texts to `POST /v1/audio/speech`,
//! STT: the `--audio <file.wav>` speech to the server's transcription route; both record audio seconds per wall
//! second (`bench/audio.rs`). Video: not supported yet.
//! Peak VRAM / spill / layers are sampled from the engine's view model (focused on the System) during the runs.
//! Records (`crate::records`): the runs happen inside a bench window (`EngineLink::bench_mark`), so the requests the
//! engine reads from the server's log count as bench records; the result is handed over at the end for servers
//! whose log KLIF does not read.

use anyhow::{anyhow, bail, Context, Result};
use klif_common::config::validate_preset_id;
use klif_common::vm::{
    Action, AdapterId, BenchSummary, ModelRef, Phase, System, SystemId, SystemKind, SystemStatus, ViewModel, VramLayer, VramLayerId,
};
use klif_common::Secret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::link::EngineLink;

mod audio;

/// Bench options (`--runs --prompt --gen --keep-running --allow-shared --yes --audio`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchOpts {
    pub runs: u32,
    pub prompt_tokens: u32,
    pub gen_tokens: u32,
    pub keep_running: bool,
    /// Allowed to launch (and then stop) the System.
    pub yes: bool,
    /// Bench even while another System runs on the same GPU (numbers are then shared-GPU numbers).
    pub allow_shared: bool,
    /// STT: the speech to transcribe (a WAV file; required for stt Systems).
    pub audio: Option<PathBuf>,
}

impl Default for BenchOpts {
    fn default() -> Self {
        BenchOpts { runs: 3, prompt_tokens: 512, gen_tokens: 128, keep_running: false, yes: false, allow_shared: false, audio: None }
    }
}

/// The machine a record was measured on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BenchHardware {
    pub gpu: String,
    #[serde(rename = "vramGiB")]
    pub vram_gib: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
}

/// One measured request (LLM) or image.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BenchRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_tokens: Option<u64>,
    /// TTS / STT: seconds of audio produced / transcribed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_s: Option<f64>,
    /// TTS / STT: wall seconds of the request (the server's own timing when it reports one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_s: Option<f64>,
    /// TTS: audio seconds per wall second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_rtf: Option<f64>,
    /// STT: audio seconds per wall second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stt_rtf: Option<f64>,
}

/// One bench of one preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BenchRecord {
    /// Epoch seconds.
    pub at: f64,
    pub klif_version: String,
    pub preset_id: String,
    /// `Catalog::preset_hash` of what ran.
    pub preset_hash: String,
    pub system: SystemId,
    pub kind: SystemKind,
    pub adapter: AdapterId,
    pub model: ModelRef,
    pub hardware: BenchHardware,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_build: Option<String>,
    pub prompt_tokens: u32,
    pub gen_tokens: u32,
    /// Launch -> Live, when this bench launched the System.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_s: Option<f64>,
    #[serde(rename = "peakVramGiB", default, skip_serializing_if = "Option::is_none")]
    pub peak_vram_gib: Option<f64>,
    #[serde(rename = "spillMiB", default, skip_serializing_if = "Option::is_none")]
    pub spill_mib: Option<f64>,
    #[serde(default)]
    pub layers: Vec<VramLayer>,
    pub runs: Vec<BenchRun>,
}

/// How many records a bench file keeps (oldest dropped first).
pub const MAX_RECORDS: usize = 50;
/// Longest wait for a launched System to become ready.
const LOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Longest wait for a busy llama.cpp server (`/slots`) to become idle before a run.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

// ------------------------------------------------------------------------------------------- run

/// Run a bench of `system` through `link`.
pub fn run(link: &dyn EngineLink, system: &SystemId, opts: &BenchOpts) -> Result<BenchRecord> {
    if system.is_remote() {
        bail!("Bench is not supported for remote Systems; run klif-cli on that machine.");
    }
    if opts.runs == 0 || opts.runs > 100 {
        bail!("--runs must be between 1 and 100.");
    }
    if opts.prompt_tokens == 0 || opts.prompt_tokens > 1_000_000 || opts.gen_tokens == 0 || opts.gen_tokens > 100_000 {
        bail!("--prompt must be 1-1000000 tokens and --gen 1-100000 tokens.");
    }
    let vm = link.snapshot(Some(system))?;
    let sys = find(&vm, system)?.clone();
    let label = sys.label.clone();
    match sys.kind {
        SystemKind::Llm | SystemKind::Image | SystemKind::Tts => {}
        // Fail before anything is launched: the speech file must be there and readable.
        SystemKind::Stt => match opts.audio.as_deref() {
            Some(p) => {
                audio::read_wav(p)?;
            }
            None => bail!(
                "Bench of a Transcription (STT) System needs real speech: pass --audio <file.wav> (a WAV recording of a few seconds to a few minutes)."
            ),
        },
        k => bail!("Bench of {} Systems is not supported yet ({label}).", k.label()),
    }
    let preset_id = sys.preset.clone().ok_or_else(|| anyhow!("{label} has no preset; choose one first (klif-cli presets use {} <id>).", sys.id))?;
    match sys.status {
        SystemStatus::NotSet => bail!("{label} has no preset; choose one first (klif-cli presets use {} <id>).", sys.id),
        SystemStatus::Invalid => bail!("{label} cannot run: {}", sys.reason.as_deref().unwrap_or("its preset is invalid.")),
        SystemStatus::Fault => bail!("{label} is in fault; dismiss it or restart it first."),
        SystemStatus::Unreachable => bail!("{label} is unreachable."),
        SystemStatus::Stopping => bail!("{label} is stopping; try again when it is offline."),
        _ => {}
    }
    let adapter = sys
        .session
        .as_ref()
        .and_then(|s| s.command.as_ref())
        .or(sys.command.as_ref())
        .map(|c| c.adapter)
        .or_else(|| AdapterId::parse(&sys.model.engine))
        .unwrap_or_default();
    if sys.kind == SystemKind::Image && !matches!(adapter, AdapterId::SdCpp | AdapterId::OpenAi) {
        bail!("Image bench needs an sd.cpp server (POST /v1/images/generations); {label} uses the {adapter} adapter, which is not supported yet.");
    }
    if !opts.allow_shared {
        if let Some(other) = shared_gpu_holder(&vm, &sys) {
            bail!(
                "Bench refused: {} runs on the same GPU as {label}, so the numbers would be shared-GPU numbers. Stop it first, or pass --allow-shared.",
                other.label
            );
        }
    }

    // Launch when needed (only with --yes), else use the running session (never stopped by the bench).
    let running = sys.session.is_some() || matches!(sys.status, SystemStatus::Starting | SystemStatus::Online | SystemStatus::Busy);
    let mut launched = false;
    let mut load_s = None;
    if sys.external {
        if !matches!(sys.status, SystemStatus::Online | SystemStatus::Busy) {
            bail!("{label} is an external server and does not answer; start it where it runs.");
        }
    } else if !running {
        if !opts.yes {
            bail!(
                "Bench needs --yes to launch {label} (preset {preset_id}); it is stopped again afterwards unless --keep-running."
            );
        }
        log::info!("launching {label} (preset {preset_id})");
        let t0 = Instant::now();
        link.act(Action::Launch { system: Some(system.clone()), stop_others: false })?;
        launched = true;
        match wait_ready(link, system, &label) {
            Ok(()) => load_s = Some(t0.elapsed().as_secs_f64()),
            Err(e) => {
                if !opts.keep_running {
                    stop_launched(link, system, &label);
                }
                return Err(e);
            }
        }
        log::info!("{label} is ready after {:.1} s", load_s.unwrap_or_default());
    } else if !matches!(sys.status, SystemStatus::Online | SystemStatus::Busy) {
        log::info!("waiting for {label} to finish loading");
        wait_ready(link, system, &label)?;
    }

    // Records: what the server logs during the runs counts as bench records, and the result goes to the engine
    // before a launched System is stopped (an engine that does not know `bench` answers an error: ignored).
    if let Err(e) = link.bench_mark(system, true, None) {
        log::debug!("bench window of {label} not opened: {e:#}");
    }
    let result = measure(link, system, &sys, adapter, &preset_id, opts, load_s);
    if let Err(e) = link.bench_mark(system, false, result.as_ref().ok()) {
        log::debug!("bench result of {label} not handed to the records: {e:#}");
    }
    if launched && !opts.keep_running {
        stop_launched(link, system, &label);
    }
    result
}

fn find<'a>(vm: &'a ViewModel, id: &SystemId) -> Result<&'a System> {
    vm.systems.iter().find(|s| &s.id == id).ok_or_else(|| anyhow!("There is no System \"{id}\"."))
}

/// Whether a System holds a session (or, external, answers): it uses its GPU now.
fn holds(s: &System) -> bool {
    if s.external {
        return matches!(s.status, SystemStatus::Online | SystemStatus::Busy) && s.endpoint.as_deref().is_some_and(is_loopback_url);
    }
    s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Online | SystemStatus::Busy | SystemStatus::Stopping)
}

fn gpus_of(s: &System) -> Vec<String> {
    if !s.gpus.is_empty() {
        s.gpus.clone()
    } else {
        s.gpu.iter().cloned().collect()
    }
}

/// Another local System that runs on (one of) the same GPU(s). Unknown GPUs count as the same GPU; "cpu" only
/// shares with "cpu".
fn shared_gpu_holder<'a>(vm: &'a ViewModel, sys: &System) -> Option<&'a System> {
    let mine = gpus_of(sys);
    vm.systems.iter().filter(|o| o.node.is_none() && !o.id.is_remote() && o.id != sys.id && holds(o)).find(|o| {
        let theirs = gpus_of(o);
        mine.is_empty() || theirs.is_empty() || theirs.iter().any(|g| mine.contains(g))
    })
}

fn is_loopback_url(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(h) = authority.strip_prefix('[') {
        h.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
    };
    let host = host.to_ascii_lowercase();
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// Poll until the System is live (online / busy). Fault -> the fault title and log tail.
fn wait_ready(link: &dyn EngineLink, id: &SystemId, label: &str) -> Result<()> {
    let t0 = Instant::now();
    let mut last_note = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let vm = link.snapshot(Some(id))?;
        let s = find(&vm, id)?;
        let phase = s.session.as_ref().map(|x| x.phase);
        match s.status {
            SystemStatus::Online | SystemStatus::Busy if s.external || phase == Some(Phase::Live) || phase.is_none() => return Ok(()),
            SystemStatus::Fault => {
                let f = s.session.as_ref().and_then(|x| x.fault.as_ref());
                let title = f.map(|f| f.title.clone()).unwrap_or_else(|| "it faulted".into());
                let tail: Vec<&str> = f.map(|f| f.log_tail.iter().rev().take(8).rev().map(String::as_str).collect()).unwrap_or_default();
                if tail.is_empty() {
                    bail!("{label} failed to start: {title}");
                }
                bail!("{label} failed to start: {title}\n  | {}", tail.join("\n  | "));
            }
            SystemStatus::Offline | SystemStatus::Invalid | SystemStatus::NotSet if t0.elapsed() > Duration::from_secs(5) => {
                bail!("{label} stopped before it was ready{}", s.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_else(|| ".".into()));
            }
            _ => {}
        }
        if t0.elapsed() > LOAD_TIMEOUT {
            bail!("{label} was not ready after {} minutes; it is left running.", LOAD_TIMEOUT.as_secs() / 60);
        }
        if last_note.elapsed() > Duration::from_secs(5) {
            last_note = Instant::now();
            let frac = s.session.as_ref().and_then(|x| x.loading.as_ref()).map(|l| format!(" ({:.0}%)", l.fraction * 100.0)).unwrap_or_default();
            log::info!("{label} is loading{frac}, {:.0} s", t0.elapsed().as_secs_f64());
        }
    }
}

/// Stop a System this bench launched and wait for it (never called for a session the bench did not start).
fn stop_launched(link: &dyn EngineLink, id: &SystemId, label: &str) {
    log::info!("stopping {label} (the bench launched it)");
    if let Err(e) = link.act(Action::Stop { system: Some(id.clone()) }) {
        log::warn!("{label} could not be stopped after the bench: {e:#}");
        return;
    }
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(400));
        match link.snapshot(Some(id)) {
            Ok(vm) => match vm.systems.iter().find(|s| &s.id == id) {
                Some(s) if s.session.is_some() || matches!(s.status, SystemStatus::Stopping | SystemStatus::Online | SystemStatus::Busy | SystemStatus::Starting) => {}
                _ => return,
            },
            Err(_) => return,
        }
    }
    log::warn!("{label} is still stopping after 60 s.");
}

// --------------------------------------------------------------------------------------- measure

/// Peak VRAM bookkeeping over the samples taken during the runs.
#[derive(Default)]
struct Peak {
    vram_gib: Option<f64>,
    spill_mib: Option<f64>,
    layers: Vec<VramLayer>,
}

impl Peak {
    fn sample(&mut self, link: &dyn EngineLink, id: &SystemId) {
        let Ok(vm) = link.snapshot(Some(id)) else {
            return;
        };
        let layers: Vec<VramLayer> = vm.vram.layers.iter().filter(|l| l.id != VramLayerId::Other).cloned().collect();
        let sess = vm.systems.iter().find(|s| &s.id == id).and_then(|s| s.session.as_ref());
        let gib = sess.and_then(|s| s.vram_gib).filter(|g| g.is_finite() && *g > 0.0).or_else(|| {
            let sum: f64 = layers.iter().map(|l| l.gib).sum();
            (sum > 0.0).then_some(sum)
        });
        if let Some(g) = gib {
            if self.vram_gib.is_none_or(|p| g > p) {
                self.vram_gib = Some(g);
                self.layers = layers;
            }
        }
        let spill = vm.vram.spill_mib;
        if spill.is_finite() && self.spill_mib.is_none_or(|p| spill > p) {
            self.spill_mib = Some(spill);
        }
    }
}

/// Where and how to talk to the System's server.
struct Target {
    base: String,
    key: Option<Secret>,
    adapter: AdapterId,
}

fn target_of(link: &dyn EngineLink, sys: &System, adapter: AdapterId) -> Result<Target> {
    let base = if sys.external {
        let url = sys
            .command
            .as_ref()
            .and_then(|c| c.external.clone())
            .or_else(|| sys.endpoint.clone())
            .ok_or_else(|| anyhow!("{} has no endpoint URL.", sys.label))?;
        let url = url.trim().trim_end_matches('/');
        url.strip_suffix("/v1").unwrap_or(url).to_string()
    } else {
        let ep = sys.session.as_ref().map(|s| &s.endpoint).ok_or_else(|| anyhow!("{} has no running session.", sys.label))?;
        let host = match ep.host.trim() {
            "" | "0.0.0.0" => "127.0.0.1".to_string(),
            "::" | "[::]" => "[::1]".to_string(),
            h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
            h => h.to_string(),
        };
        format!("http://{host}:{}", ep.port)
    };
    // KLIF's key only goes to servers KLIF launched with it (llama.cpp / vllm), never to an external server.
    let key = if !sys.external && adapter.api_key_env().is_some() { link.api_key() } else { None };
    Ok(Target { base, key, adapter })
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_response(Some(Duration::from_secs(15 * 60)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(format!("KLIF/{} bench", klif_common::KLIF_VERSION))
        .build()
        .into()
}

fn auth(req: ureq::RequestBuilder<ureq::typestate::WithoutBody>, key: &Option<Secret>) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    match key {
        Some(k) => req.header("Authorization", format!("Bearer {}", k.expose())),
        None => req,
    }
}

fn auth_body(req: ureq::RequestBuilder<ureq::typestate::WithBody>, key: &Option<Secret>) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
    match key {
        Some(k) => req.header("Authorization", format!("Bearer {}", k.expose())),
        None => req,
    }
}

/// GET a small JSON document (None on any failure).
fn get_json(agent: &ureq::Agent, t: &Target, path: &str) -> Option<Value> {
    let resp = auth(agent.get(format!("{}{path}", t.base)), &t.key).call().ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    resp.into_body().with_config().limit(4 * 1024 * 1024).read_json::<Value>().ok()
}

fn post_json(agent: &ureq::Agent, t: &Target, path: &str, body: &Value) -> Option<Value> {
    let resp = auth_body(agent.post(format!("{}{path}", t.base)), &t.key).send_json(body).ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    resp.into_body().with_config().limit(16 * 1024 * 1024).read_json::<Value>().ok()
}

fn measure(
    link: &dyn EngineLink,
    id: &SystemId,
    sys0: &System,
    adapter: AdapterId,
    preset_id: &str,
    opts: &BenchOpts,
    load_s: Option<f64>,
) -> Result<BenchRecord> {
    // The System as it runs now (session command / model / endpoint).
    let vm = link.snapshot(Some(id))?;
    let sys = find(&vm, id).cloned().unwrap_or_else(|_| sys0.clone());
    let target = target_of(link, &sys, adapter)?;
    let agent = agent();
    let mut peak = Peak::default();
    peak.sample(link, id);

    let (runs, backend_build) = match sys.kind {
        SystemKind::Image => (bench_image(link, id, &sys, &agent, &target, opts, &mut peak)?, None),
        SystemKind::Tts => (audio::bench_tts(link, id, &sys, &agent, &target, opts, &mut peak)?, None),
        SystemKind::Stt => (audio::bench_stt(link, id, &sys, &agent, &target, opts, &mut peak)?, None),
        _ => bench_llm(link, id, &sys, &agent, &target, opts, &mut peak)?,
    };

    let session = sys.session.as_ref();
    let hash = session
        .and_then(|s| s.command.as_ref())
        .map(|c| c.hash.clone())
        .filter(|h| !h.is_empty())
        .or_else(|| sys.command.as_ref().map(|c| c.hash.clone()))
        .unwrap_or_default();
    let model = session.map(|s| s.model.clone()).unwrap_or_else(|| sys.model.clone());
    Ok(BenchRecord {
        at: klif_common::now_s(),
        klif_version: klif_common::KLIF_VERSION.to_string(),
        preset_id: preset_id.to_string(),
        preset_hash: hash,
        system: sys.id.clone(),
        kind: sys.kind,
        adapter,
        model,
        hardware: hardware(&vm, &sys),
        backend_build,
        prompt_tokens: if sys.kind == SystemKind::Llm { opts.prompt_tokens } else { 0 },
        gen_tokens: if sys.kind == SystemKind::Llm { opts.gen_tokens } else { 0 },
        load_s,
        peak_vram_gib: peak.vram_gib.map(round3),
        spill_mib: peak.spill_mib.map(round3),
        layers: peak.layers,
        runs,
    })
}

fn hardware(vm: &ViewModel, sys: &System) -> BenchHardware {
    let gpu = sys.session.as_ref().and_then(|s| s.gpu.clone()).or_else(|| sys.gpu.clone());
    if gpu.as_deref() == Some("cpu") {
        let cpu = vm.machine.cpu_name.trim();
        return BenchHardware { gpu: if cpu.is_empty() { "CPU".into() } else { format!("CPU ({cpu})") }, vram_gib: 0.0, driver: None };
    }
    let mem = gpu.as_deref().and_then(|g| vm.gpus.iter().find(|m| m.id == g)).unwrap_or(&vm.vram);
    let name = [&mem.name, &mem.device].into_iter().map(|n| n.trim()).find(|n| !n.is_empty()).map(str::to_string);
    let name = name.or(gpu).unwrap_or_else(|| "unknown GPU".into());
    BenchHardware { gpu: name, vram_gib: round3(mem.total_gib), driver: None }
}

fn round3(x: f64) -> f64 {
    if x.is_finite() { (x * 1000.0).round() / 1000.0 } else { 0.0 }
}

/// Run `work` on a helper thread while sampling VRAM on this one (the link stays on this thread).
fn with_sampling<T: Send + 'static>(
    link: &dyn EngineLink,
    id: &SystemId,
    peak: &mut Peak,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let handle = std::thread::Builder::new().name("klif-bench-request".into()).spawn(work).context("The bench thread could not start")?;
    let mut last = Instant::now();
    while !handle.is_finished() {
        std::thread::sleep(Duration::from_millis(50));
        if last.elapsed() >= Duration::from_millis(500) {
            last = Instant::now();
            peak.sample(link, id);
        }
    }
    peak.sample(link, id);
    handle.join().map_err(|_| anyhow!("The bench request thread panicked."))?
}

// ------------------------------------------------------------------------------------------- llm

/// Words that are single tokens in common tokenizers; the filler text is built from them.
const WORDS: &[&str] = &[
    "the", "river", "light", "stone", "house", "small", "green", "water", "under", "over", "after", "before", "people", "city",
    "north", "south", "winter", "summer", "music", "paper", "table", "window", "garden", "market", "story", "morning", "evening",
    "road", "bridge", "forest", "mountain", "old", "new", "quiet", "bright", "dark", "warm", "cold", "long", "short", "slow",
    "fast", "red", "blue", "white", "black", "clear", "deep", "high", "low", "and", "with", "from", "into", "near", "far",
    "every", "some", "many", "few", "first", "last", "open", "closed", "work", "play", "walk", "talk", "read", "write", "build",
    "carry", "follow", "watch", "listen", "train", "ship", "letter", "voice", "field", "farm", "school", "street", "corner",
    "season", "harbor", "island", "valley", "station", "kitchen", "lamp", "clock", "door", "wall", "roof", "floor", "chair",
];

/// Filler text of `n` words (deterministic from `seed`, sentences of 12 words).
fn filler(n: usize, seed: u64) -> String {
    let mut x = seed | 1;
    let mut out = String::with_capacity(n * 7);
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let w = WORDS[(x % WORDS.len() as u64) as usize];
        if i > 0 {
            out.push_str(if i % 12 == 0 { ". " } else { " " });
        }
        out.push_str(w);
    }
    out.push('.');
    out
}

fn nonce() -> String {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = (t as u64).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Tokens per filler word, from the server's tokenizer when it has one (llama.cpp / vLLM `/tokenize`).
fn tokens_per_word(agent: &ureq::Agent, t: &Target, model: &str) -> f64 {
    let sample = filler(200, 0x9e37_79b9_7f4a_7c15);
    let count = match t.adapter {
        AdapterId::LlamaCpp => post_json(agent, t, "/tokenize", &json!({ "content": sample }))
            .and_then(|v| v.get("tokens").and_then(Value::as_array).map(|a| a.len() as f64)),
        AdapterId::Vllm => post_json(agent, t, "/tokenize", &json!({ "model": model, "prompt": sample }))
            .and_then(|v| v.get("count").and_then(Value::as_f64)),
        _ => None,
    };
    match count {
        Some(c) if c > 0.0 => (c / 200.0).clamp(0.3, 4.0),
        _ => 1.15,
    }
}

/// llama.cpp: wait until no slot is processing (other clients may be mid-request). Servers without /slots skip.
fn wait_slots_idle(agent: &ureq::Agent, t: &Target, label: &str) -> Result<()> {
    if t.adapter != AdapterId::LlamaCpp {
        return Ok(());
    }
    let t0 = Instant::now();
    let mut noted = false;
    loop {
        let Some(v) = get_json(agent, t, "/slots") else {
            return Ok(());
        };
        let Some(slots) = v.as_array() else {
            return Ok(());
        };
        let busy = slots.iter().any(|s| {
            s.get("is_processing").and_then(Value::as_bool).unwrap_or(false) || s.get("state").and_then(Value::as_i64).is_some_and(|x| x != 0)
        });
        if !busy {
            return Ok(());
        }
        if t0.elapsed() > IDLE_TIMEOUT {
            bail!("{label} stayed busy for {} minutes (another client is using it); bench stopped.", IDLE_TIMEOUT.as_secs() / 60);
        }
        if !noted {
            noted = true;
            log::info!("{label} is busy with another request; waiting until it is idle");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// What one streamed chat completion reported.
#[derive(Debug, Default)]
struct StreamResult {
    send_to_first: Option<f64>,
    first_to_last: Option<f64>,
    chunks: u64,
    timings: Option<Value>,
    usage: Option<Value>,
}

fn stream_chat(agent: ureq::Agent, url: String, key: Option<Secret>, body: Value) -> Result<StreamResult> {
    let t_send = Instant::now();
    let resp = auth_body(agent.post(&url), &key).header("Accept", "text/event-stream").send_json(&body).map_err(|e| anyhow!("{url}: {e}"))?;
    let status = resp.status().as_u16();
    if status != 200 {
        let text = resp.into_body().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
        if status == 401 || status == 403 {
            bail!("The server refused the bench request ({status}): check the API key.");
        }
        bail!("The server answered {status}: {}", one_line(&text, 300));
    }
    let reader = std::io::BufReader::new(resp.into_body().into_reader());
    let mut r = StreamResult::default();
    let (mut t_first, mut t_last) = (None::<Instant>, None::<Instant>);
    for line in reader.lines() {
        let line = line.context("The stream broke off")?;
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
            bail!("The server reported an error: {}", one_line(&msg, 300));
        }
        let delta = v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta"));
        let has_text = delta.is_some_and(|d| {
            ["content", "reasoning_content", "reasoning"].iter().any(|k| d.get(*k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
        });
        if has_text {
            let now = Instant::now();
            t_first.get_or_insert(now);
            t_last = Some(now);
            r.chunks += 1;
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            r.timings = Some(t.clone());
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            r.usage = Some(u.clone());
        }
    }
    let first = t_first.ok_or_else(|| anyhow!("The server streamed no tokens."))?;
    r.send_to_first = Some(first.duration_since(t_send).as_secs_f64());
    r.first_to_last = t_last.map(|l| l.duration_since(first).as_secs_f64());
    Ok(r)
}

fn one_line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max { format!("{}...", s.chars().take(max).collect::<String>()) } else { s }
}

fn num(v: Option<&Value>, key: &str) -> Option<f64> {
    v.and_then(|v| v.get(key)).and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0)
}

fn bench_llm(
    link: &dyn EngineLink,
    id: &SystemId,
    sys: &System,
    agent: &ureq::Agent,
    t: &Target,
    opts: &BenchOpts,
    peak: &mut Peak,
) -> Result<(Vec<BenchRun>, Option<String>)> {
    let label = sys.label.as_str();
    let model = get_json(agent, t, "/v1/models")
        .and_then(|v| v.get("data").and_then(|d| d.get(0)).and_then(|m| m.get("id")).and_then(Value::as_str).map(str::to_string))
        .or_else(|| sys.session.as_ref().and_then(|s| s.generic.as_ref()).and_then(|g| g.model_id.clone()))
        .unwrap_or_else(|| sys.preset.clone().unwrap_or_else(|| "default".into()));
    let backend_build = match t.adapter {
        AdapterId::LlamaCpp => get_json(agent, t, "/props").and_then(|v| v.get("build_info").and_then(Value::as_str).map(str::to_string)),
        AdapterId::Vllm => get_json(agent, t, "/version").and_then(|v| v.get("version").and_then(Value::as_str).map(|s| format!("vLLM {s}"))),
        _ => None,
    };
    let ratio = tokens_per_word(agent, t, &model);
    // ~16 tokens of chat template + nonce line around the filler.
    let words = (((opts.prompt_tokens as f64) - 16.0).max(1.0) / ratio).round().max(1.0) as usize;
    let url = format!("{}/v1/chat/completions", t.base);
    let mut runs = Vec::new();
    for i in 0..opts.runs {
        wait_slots_idle(agent, t, label)?;
        let n = nonce();
        let seed = u64::from_str_radix(&n, 16).unwrap_or(i as u64 + 1);
        let prompt = format!("[{n}] Bench run {}. Continue this text in the same style:\n{}", i + 1, filler(words, seed));
        let mut body = json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": opts.gen_tokens,
            "stream": true,
            "stream_options": { "include_usage": true },
            "temperature": 0.8,
            "seed": (seed & 0x7fff_ffff),
        });
        match t.adapter {
            AdapterId::LlamaCpp => {
                body["cache_prompt"] = json!(false);
                body["ignore_eos"] = json!(true);
            }
            AdapterId::Vllm => body["ignore_eos"] = json!(true),
            _ => {}
        }
        let (a, u, k) = (agent.clone(), url.clone(), t.key.clone());
        let r = with_sampling(link, id, peak, move || stream_chat(a, u, k, body)).with_context(|| format!("Bench run {} of {label} failed", i + 1))?;

        let timings = r.timings.as_ref();
        let usage = r.usage.as_ref();
        let prompt_n = num(timings, "prompt_n").or_else(|| num(usage, "prompt_tokens")).map(|x| x as u64);
        let gen_n = num(timings, "predicted_n").or_else(|| num(usage, "completion_tokens")).map(|x| x as u64).or(Some(r.chunks));
        let ttft = r.send_to_first;
        let prefill_tps = num(timings, "prompt_per_second").or_else(|| match (prompt_n, ttft) {
            (Some(p), Some(s)) if s > 0.0 => Some(p as f64 / s),
            _ => None,
        });
        let decode_tps = num(timings, "predicted_per_second").or_else(|| match (gen_n, r.first_to_last) {
            (Some(g), Some(s)) if g > 1 && s > 0.0 => Some((g - 1) as f64 / s),
            _ => None,
        });
        let run = BenchRun {
            ttft_s: ttft.map(round3),
            prefill_tps: prefill_tps.map(round3),
            decode_tps: decode_tps.map(round3),
            seconds_per_image: None,
            prompt_tokens: prompt_n,
            gen_tokens: gen_n,
            ..BenchRun::default()
        };
        log::info!(
            "run {}/{}: ttft {} s, prefill {} tok/s, decode {} tok/s ({} prompt / {} generated tokens)",
            i + 1,
            opts.runs,
            fmt_opt(run.ttft_s, 2),
            fmt_opt(run.prefill_tps, 1),
            fmt_opt(run.decode_tps, 1),
            run.prompt_tokens.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            run.gen_tokens.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
        );
        runs.push(run);
    }
    Ok((runs, backend_build))
}

fn fmt_opt(v: Option<f64>, digits: usize) -> String {
    v.map(|x| format!("{x:.digits$}")).unwrap_or_else(|| "?".into())
}

// ----------------------------------------------------------------------------------------- image

fn generate_image(agent: ureq::Agent, url: String, key: Option<Secret>, body: Value) -> Result<f64> {
    let t0 = Instant::now();
    let resp = auth_body(agent.post(&url), &key).send_json(&body).map_err(|e| anyhow!("{url}: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.into_body().with_config().limit(256 * 1024 * 1024).read_to_string().map_err(|e| anyhow!("{url}: {e}"))?;
    let secs = t0.elapsed().as_secs_f64();
    match status {
        200 => {}
        404 | 405 => bail!("This image server has no POST /v1/images/generations, so its image bench is not supported."),
        401 | 403 => bail!("The server refused the bench request ({status}): check the API key."),
        _ => bail!("The server answered {status}: {}", one_line(&text, 300)),
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| anyhow!("The image server answered something other than JSON."))?;
    let images = v.get("data").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    if images == 0 {
        bail!("The image server returned no image.");
    }
    Ok(secs / images as f64)
}

fn bench_image(
    link: &dyn EngineLink,
    id: &SystemId,
    sys: &System,
    agent: &ureq::Agent,
    t: &Target,
    opts: &BenchOpts,
    peak: &mut Peak,
) -> Result<Vec<BenchRun>> {
    let url = format!("{}/v1/images/generations", t.base);
    let mut runs = Vec::new();
    for i in 0..opts.runs {
        let n = nonce();
        // The server's launch defaults decide size / steps (what the preset runs); the nonce varies the prompt.
        let body = json!({
            "prompt": format!("a lighthouse on a cliff above a stormy sea at dusk, detailed, {n}"),
            "n": 1,
            "output_format": "png",
        });
        let (a, u, k) = (agent.clone(), url.clone(), t.key.clone());
        let secs =
            with_sampling(link, id, peak, move || generate_image(a, u, k, body)).with_context(|| format!("Bench run {} of {} failed", i + 1, sys.label))?;
        log::info!("run {}/{}: {:.2} s per image", i + 1, opts.runs, secs);
        runs.push(BenchRun { seconds_per_image: Some(round3(secs)), ..BenchRun::default() });
    }
    Ok(runs)
}

// ----------------------------------------------------------------------------------------- files

/// `<data_dir>\bench\<preset_id>.json`.
pub fn bench_file(data_dir: &Path, preset_id: &str) -> PathBuf {
    data_dir.join("bench").join(format!("{preset_id}.json"))
}

/// Append a record to `<data_dir>\bench\<preset_id>.json`.
pub fn store(data_dir: &Path, preset_id: &str, rec: &BenchRecord) -> Result<()> {
    validate_preset_id(preset_id).map_err(|e| anyhow!(e))?;
    let path = bench_file(data_dir, preset_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("The folder {} could not be created", dir.display()))?;
    }
    let mut records = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Vec<BenchRecord>>(&bytes) {
            Ok(r) => r,
            Err(_) => {
                // Keep an unreadable file aside instead of overwriting it.
                let aside = path.with_file_name(format!("{preset_id}.json.bad-{}", klif_common::now_s() as u64));
                let _ = std::fs::rename(&path, &aside);
                log::warn!("{} could not be read; it was kept as {}", path.display(), aside.display());
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => bail!("{} could not be read: {e}", path.display()),
    };
    records.push(rec.clone());
    if records.len() > MAX_RECORDS {
        let drop = records.len() - MAX_RECORDS;
        records.drain(..drop);
    }
    let text = serde_json::to_vec_pretty(&records)?;
    crate::keys::write_atomic(&path, &text).with_context(|| format!("{} could not be written", path.display()))?;
    Ok(())
}

/// Every record of a preset, oldest first (empty when there is no file).
pub fn records(data_dir: &Path, preset_id: &str) -> Result<Vec<BenchRecord>> {
    validate_preset_id(preset_id).map_err(|e| anyhow!(e))?;
    let path = bench_file(data_dir, preset_id);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("{} could not be read", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => bail!("{} could not be read: {e}", path.display()),
    }
}

/// Every bench file in `<data_dir>\bench` as (preset id, records), sorted by preset id. Unreadable files are skipped.
pub fn all_records(data_dir: &Path) -> Vec<(String, Vec<BenchRecord>)> {
    let Ok(dir) = std::fs::read_dir(data_dir.join("bench")) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<BenchRecord>)> = dir
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?.to_string();
            validate_preset_id(&id).ok()?;
            let recs = records(data_dir, &id).ok()?;
            Some((id, recs))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// A record as the summary the UI shows (medians over the runs).
pub fn summarize(rec: &BenchRecord, stale: bool) -> BenchSummary {
    let med = |f: fn(&BenchRun) -> Option<f64>| median(rec.runs.iter().filter_map(f).collect()).map(round3);
    BenchSummary {
        at: rec.at,
        runs: rec.runs.len() as u32,
        load_s: rec.load_s,
        ttft_s: med(|r| r.ttft_s),
        prefill_tps: med(|r| r.prefill_tps),
        decode_tps: med(|r| r.decode_tps),
        seconds_per_image: med(|r| r.seconds_per_image),
        tts_rtf: med(|r| r.tts_rtf),
        stt_rtf: med(|r| r.stt_rtf),
        peak_vram_gib: rec.peak_vram_gib,
        spill_mib: rec.spill_mib,
        backend_build: rec.backend_build.clone(),
        klif_version: rec.klif_version.clone(),
        stale,
    }
}

/// path -> (mtime, len, records): `latest` runs for every preset on engine ticks; files are re-read only when
/// they change.
type Cache = HashMap<PathBuf, (Option<SystemTime>, u64, Arc<Vec<BenchRecord>>)>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn cached_records(path: &Path) -> Option<Arc<Vec<BenchRecord>>> {
    let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    let Ok(meta) = std::fs::metadata(path) else {
        cache.remove(path);
        return None;
    };
    let (mtime, len) = (meta.modified().ok(), meta.len());
    if let Some((m, l, recs)) = cache.get(path) {
        if *m == mtime && *l == len && mtime.is_some() {
            return Some(recs.clone());
        }
    }
    let recs: Vec<BenchRecord> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let recs = Arc::new(recs);
    cache.insert(path.to_path_buf(), (mtime, len, recs.clone()));
    Some(recs)
}

/// The latest record of a preset as a summary; `stale` when it was recorded for another hash. The latest record
/// with the same hash wins over a newer one of another hash; an empty `hash` (unknown) never marks stale.
pub fn latest(data_dir: &Path, preset_id: &str, hash: &str) -> Option<BenchSummary> {
    validate_preset_id(preset_id).ok()?;
    let recs = cached_records(&bench_file(data_dir, preset_id))?;
    if hash.is_empty() {
        return recs.last().map(|r| summarize(r, false));
    }
    if let Some(r) = recs.iter().rev().find(|r| r.preset_hash == hash) {
        return Some(summarize(r, false));
    }
    recs.last().map(|r| summarize(r, !r.preset_hash.is_empty()))
}

/// Like [`latest`], but a record matching any of `hashes` is current: a preset is run with different params by
/// different Systems, and a bench of any of those runs is not stale. Empty hashes are ignored.
pub fn latest_any(data_dir: &Path, preset_id: &str, hashes: &[String]) -> Option<BenchSummary> {
    validate_preset_id(preset_id).ok()?;
    let recs = cached_records(&bench_file(data_dir, preset_id))?;
    let known: Vec<&str> = hashes.iter().map(String::as_str).filter(|h| !h.is_empty()).collect();
    if known.is_empty() {
        return recs.last().map(|r| summarize(r, false));
    }
    if let Some(r) = recs.iter().rev().find(|r| known.contains(&r.preset_hash.as_str())) {
        return Some(summarize(r, false));
    }
    recs.last().map(|r| summarize(r, !r.preset_hash.is_empty()))
}
