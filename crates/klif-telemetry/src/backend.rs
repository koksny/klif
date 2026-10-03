//! Per-adapter telemetry: which parser, probes and health rules a session uses, chosen by
//! `WatchSpec.adapter` (not by the System's kind). Owner: package D.
//!
//! - `LlamaCpp`: the llama-server log parser (prefix + timestamps, verbosity 4) fused with `/health`, `/slots`
//!   (live decode rate, prefill progress, context use), `/v1/models` (n_ctx) and, with `--metrics`, `/metrics`.
//! - `SdCpp`: the sd-server log parser + a TCP listener check (sd-server has no /health).
//! - `Vllm`: `/health`, `/metrics` (live counters: running requests, prompt / generation token totals, KV cache
//!   use, speculative acceptance), `/v1/models` (`max_model_len`), `/version`.
//! - `OpenAi`: any OpenAI-compatible server: `/health` -> `/v1/models` -> TCP, `/metrics` if it answers
//!   Prometheus text, `/v1/models` for the model id and context.
//! - `Generic`: any other server (TTS / STT / video...): TCP or the preset's HTTP health, log-activity busy pulses,
//!   `/metrics` if it answers Prometheus text, request counters -> `GenericLive`.
//!
//! The session tracker (`session.rs`) owns the log tailers, the console and the probe results' plumbing; the
//! adapter owns everything server-specific. The llama.cpp / sd.cpp log parsing is unchanged from 0.2 (it is
//! tuned to the operator's builds); only the place it is called from moved.

use crate::llama::{Activity, LlamaParser, Req};
use crate::probe::{ModelsInfo, SlotSample};
use crate::prom::{self, Sample};
use crate::sd::SdParser;
use crate::steps::{self, step_label, Steps, STEP_IDS};
use crate::tail::{Segment, Stream};
use crate::text::{redact, round_to};
use crate::vram::push_cap;
use crate::{Health, SessionSignals};
use klif_common::vm::{
    AdapterId, ContextFill, GenericLive, HealthCheck, LlmActivity, LlmLive, LoadStep, Prefill, RequestRecord, Spec, StepState,
    SystemKind, Totals, VramLayer,
};
use regex::Regex;
use std::collections::VecDeque;
use std::sync::LazyLock;

/// A /slots sample older than this is not used for live values.
const SLOTS_FRESH_S: f64 = 2.5;
/// Display smoothing of the decode speed.
const DECODE_TAU_S: f64 = 1.5;
/// A log line (that is not a probe's own access line) keeps a parser-less server "busy" this long.
const BUSY_PULSE_S: f64 = 2.0;
/// Metrics older than this are not used for live values.
const METRICS_FRESH_S: f64 = 5.0;

/// What the adapter is told about its session at creation.
#[derive(Debug, Clone)]
pub struct AdapterCtx {
    pub kind: SystemKind,
    pub external: bool,
    pub host: String,
    pub port: u16,
    /// Context window from the preset (used until the server reports one).
    pub ctx_tokens: Option<u32>,
    /// Speculative mode label from the preset.
    pub spec_mode: Option<String>,
    /// The preset says the server serves `/metrics` (llama.cpp `--metrics`).
    pub metrics: bool,
    /// Session start (epoch s); with `from_start` it anchors llama.cpp's log clock.
    pub started_at: f64,
    pub from_start: bool,
    /// The preset's HTTP health path (`health = "/path"`), when it is not one of the standard probe paths: its
    /// access-log lines and request counters are KLIF's own traffic too.
    pub health_path: Option<String>,
}

impl Default for AdapterCtx {
    fn default() -> Self {
        AdapterCtx {
            kind: SystemKind::Llm,
            external: false,
            host: "127.0.0.1".into(),
            port: 0,
            ctx_tokens: None,
            spec_mode: None,
            metrics: false,
            started_at: 0.0,
            from_start: false,
            health_path: None,
        }
    }
}

/// How readiness is checked (the preset's `health`, or the adapter's default chain for `Auto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthPlan {
    /// GET path: 200 ready, other status loading, no answer down.
    Http(String),
    /// A TCP listener = ready.
    Tcp,
    /// OpenAI-compatible chain: `/health` -> `/v1/models` -> TCP.
    Chain,
}

impl HealthPlan {
    pub fn resolve(check: &HealthCheck, default: &HealthCheck) -> HealthPlan {
        match check {
            HealthCheck::Http { path } => HealthPlan::Http(path.clone()),
            HealthCheck::Tcp => HealthPlan::Tcp,
            HealthCheck::Auto => match default {
                HealthCheck::Http { path } => HealthPlan::Http(path.clone()),
                HealthCheck::Tcp => HealthPlan::Tcp,
                HealthCheck::Auto => HealthPlan::Chain,
            },
        }
    }
}

/// What one probe round should ask besides health (decided by the adapter from what it knows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProbeWants {
    /// llama.cpp `/slots`.
    pub slots: bool,
    /// `/v1/models` (until it answered once).
    pub models: bool,
    /// `/metrics` (`Some(period_s)`): live counters.
    pub metrics: Option<f64>,
    /// vLLM `/version` (once).
    pub version: bool,
}

/// A probe answer for the adapter (health goes through the tracker).
#[derive(Debug, Clone)]
pub enum ProbeResult {
    Slots(Vec<SlotSample>),
    Models(ModelsInfo),
    Metrics(Vec<Sample>),
    Version(String),
}

/// What one server family contributes to a watched session.
pub trait BackendAdapter: Send {
    fn id(&self) -> AdapterId;
    /// The health check used when the preset says `Auto`.
    fn default_health(&self, kind: SystemKind) -> HealthCheck;
    /// Session shared memory counts as VRAM spill (LLM servers; not sd.cpp's host staging buffers).
    fn reports_spill(&self) -> bool;

    /// Health probe cadence while the server answers (seconds).
    fn probe_period(&self) -> f64 {
        1.0
    }
    /// Extra probes for the next round, given the latest health.
    fn probe_wants(&self, _health: Health) -> ProbeWants {
        ProbeWants::default()
    }
    /// One log segment (a line, or one carriage-return segment of a progress bar).
    fn feed(&mut self, seg: &Segment);
    /// Whether a committed log line belongs in the console. Adapters without a log parser drop the access-log
    /// lines of KLIF's own probes (`GET /health`, `/metrics`...), which would otherwise flood it.
    fn console_keep(&self, _line: &str) -> bool {
        true
    }
    /// After every log poll (2 Hz): smoothing and the 1 Hz histories.
    fn tick(&mut self, _now: f64) {}
    /// A health probe result arrived (the tracker keeps the value; adapters may track readiness).
    fn on_health(&mut self, _at: f64, _h: Health) {}
    /// A probe answer.
    fn apply(&mut self, _at: f64, _r: ProbeResult) {}
    /// Health from the probe (None: probes off or none yet) and the logs.
    fn health(&self, probe: Option<Health>) -> Health;
    /// Load steps for the UI and the load fraction 0..1.
    fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64);
    /// Start (epoch s) of the work in flight, if the server is busy.
    fn busy_since(&self, now: f64) -> Option<f64>;
    /// VRAM composition parsed from the log, if the server logs it.
    fn layers_from_logs(&self) -> Option<Vec<VramLayer>> {
        None
    }
    /// The GPU devices the server reported it runs on (for `device_mismatch`).
    fn reported_devices(&self) -> Vec<String> {
        Vec::new()
    }
    /// The server's build / version as it reports it.
    fn backend_build(&self) -> Option<String> {
        None
    }
    /// Fill the adapter-specific parts of the signals (live shapes, faults, arch...).
    fn fill(&self, now: f64, sig: &mut SessionSignals);
    /// Median decode speed over the session's finished requests (LLM).
    fn median_decode_tps(&self) -> Option<f64> {
        None
    }
    /// The llama.cpp parser, for replays and tools.
    fn llama(&self) -> Option<&LlamaParser> {
        None
    }
    /// The sd.cpp parser, for replays and tools.
    fn sd(&self) -> Option<&SdParser> {
        None
    }
}

/// The adapter implementation for an id, with a default context (no anchor, kind = the adapter's default).
pub fn for_id(id: AdapterId) -> Box<dyn BackendAdapter> {
    let ctx = AdapterCtx { kind: id.default_kind().unwrap_or(SystemKind::Llm), ..AdapterCtx::default() };
    for_ctx(id, ctx)
}

/// The adapter implementation for a watched session.
pub fn for_ctx(id: AdapterId, ctx: AdapterCtx) -> Box<dyn BackendAdapter> {
    match id {
        AdapterId::LlamaCpp => Box::new(LlamaCpp::new(ctx)),
        AdapterId::SdCpp => Box::new(SdCpp::new(ctx)),
        AdapterId::Vllm => Box::new(Vllm::new(ctx)),
        AdapterId::OpenAi => Box::new(OpenAi::new(ctx)),
        AdapterId::Generic => Box::new(Generic::new(ctx)),
    }
}

// ------------------------------------------------------------------------------------ shared helpers

/// 1 Hz history of a rate (the decode tok/s sparkline), filled at most once per wall second.
#[derive(Debug, Default, Clone)]
struct RateHistory {
    hist: VecDeque<f64>,
    sec: Option<i64>,
}

impl RateHistory {
    fn push(&mut self, now: f64, sample: f64) {
        let sec = now.floor() as i64;
        match self.sec {
            None => self.sec = Some(sec),
            Some(h) if sec > h => {
                let n = ((sec - h) as usize).min(crate::vram::HISTORY_LEN);
                for _ in 0..n {
                    push_cap(&mut self.hist, round_to(sample, 1));
                }
                self.sec = Some(sec);
            }
            _ => {}
        }
    }
    fn values(&self) -> Vec<f64> {
        self.hist.iter().copied().collect()
    }
}

fn norm_device(s: &str) -> String {
    let mut l = s.trim().to_ascii_lowercase();
    for p in ["amd radeon(tm) ", "amd radeon ", "amd ", "nvidia geforce ", "nvidia ", "intel(r) ", "intel "] {
        if let Some(r) = l.strip_prefix(p) {
            l = r.to_string();
            break;
        }
    }
    l.replace("(tm)", "").replace("(r)", "").chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// One sentence when the server reports a device that does not match the expected one (preset `device`, else
/// the GPU's name): names compared without vendor prefixes, punctuation and case, either containing the other.
/// None when nothing was reported, nothing is expected, or any reported device matches.
pub fn device_mismatch(reported: &[String], expected: Option<&str>) -> Option<String> {
    let exp_raw = expected.map(str::trim).filter(|s| !s.is_empty())?;
    let exp = norm_device(exp_raw);
    if exp.len() < 3 {
        return None;
    }
    let rep: Vec<&String> = reported.iter().filter(|r| norm_device(r).len() >= 3).collect();
    if rep.is_empty() {
        return None;
    }
    if rep.iter().any(|r| {
        let n = norm_device(r);
        n.contains(&exp) || exp.contains(&n)
    }) {
        return None;
    }
    Some(format!(
        "The server reports {}, not {exp_raw}: check the preset's device selection (env or args).",
        rep[0].trim()
    ))
}

/// Paths a probe asks for: access-log lines and request counters for these are KLIF's own traffic.
const PROBE_PATHS: [&str; 11] =
    ["/metrics", "/health", "/healthz", "/v1/health", "/v1/models", "/models", "/version", "/slots", "/ready", "/live", "/ping"];

/// A preset health path as the server logs it: leading `/`, no query, no trailing `/`; None when it is empty or
/// already one of `PROBE_PATHS`.
fn extra_probe_path(health_path: Option<&str>) -> Option<String> {
    let p = health_path?.trim();
    let p = p.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    if p.is_empty() {
        return None;
    }
    let p = if p.starts_with('/') { p.to_string() } else { format!("/{p}") };
    (!PROBE_PATHS.contains(&p.as_str())).then_some(p)
}

/// Matches an access-log line of a probe request, whatever the log format: uvicorn / aiohttp
/// (`"GET /health HTTP/1.1" 200`), Gin (`GET      "/v1/models"`), morgan / Go (`GET /metrics 200 1.2 ms`), and HEAD
/// requests; the path may carry a trailing `/` or a query. POST requests, longer paths (`/healthz` for `/health`,
/// `/health/extra`) and real API routes do not match.
fn probe_line_regex(extra: Option<&str>) -> Regex {
    match extra {
        Some(_) => Regex::new(&probe_line_pattern(extra)).unwrap_or_else(|_| DEFAULT_PROBE_LINE.clone()),
        None => DEFAULT_PROBE_LINE.clone(),
    }
}

fn probe_line_pattern(extra: Option<&str>) -> String {
    let alt = PROBE_PATHS.iter().copied().chain(extra).map(regex::escape).collect::<Vec<_>>().join("|");
    format!(r#"\b(?:GET|HEAD)\s+"?(?:{alt})/?(?:[\s"?]|$)"#)
}

static DEFAULT_PROBE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&probe_line_pattern(None)).expect("probe line regex"));

fn is_probe_sample(s: &Sample, extra: Option<&str>) -> bool {
    s.labels.iter().any(|(k, v)| {
        matches!(k.as_str(), "handler" | "path" | "endpoint" | "route" | "url" | "uri")
            && (PROBE_PATHS.iter().any(|p| v == p) || extra.is_some_and(|e| v == e))
    })
}

/// Live counters read from one /metrics body (any server).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counters {
    pub running: Option<f64>,
    pub waiting: Option<f64>,
    pub requests_total: Option<f64>,
    pub prompt_tokens: Option<f64>,
    pub generation_tokens: Option<f64>,
    /// KV cache use 0..1 (vLLM).
    pub kv_usage: Option<f64>,
    pub spec_accepted: Option<f64>,
    pub spec_drafted: Option<f64>,
}

fn suffix_sum(samples: &[Sample], suffixes: &[&str], extra: Option<&str>) -> Option<f64> {
    let mut found = false;
    let mut t = 0.0;
    for s in samples {
        if s.name.ends_with("_bucket") || s.name.ends_with("_created") || is_probe_sample(s, extra) {
            continue;
        }
        if suffixes.iter().any(|x| s.name.ends_with(x)) {
            found = true;
            if s.value.is_finite() {
                t += s.value;
            }
        }
    }
    if found { Some(t) } else { None }
}

impl Counters {
    /// Known names first (vLLM, llama.cpp, SGLang), then generic suffixes (`*_requests_in_progress`,
    /// `*_requests_total`...) with KLIF's own probe requests filtered out by their handler / path label.
    pub fn from_samples(samples: &[Sample]) -> Counters {
        Self::from_samples_excluding(samples, None)
    }

    /// `from_samples`, also leaving out the counters of `extra_probe_path` (the preset's own health path).
    pub fn from_samples_excluding(samples: &[Sample], extra_probe_path: Option<&str>) -> Counters {
        let extra = extra_probe_path;
        let running = prom::first_sum(samples, &["vllm:num_requests_running", "llamacpp:requests_processing", "sglang:num_running_reqs"])
            .or_else(|| {
                suffix_sum(
                    samples,
                    &["requests_in_flight", "requests_in_progress", "requests_inprogress", "requests_running", "requests_processing", "num_requests_running"],
                    extra,
                )
            });
        let waiting = prom::first_sum(samples, &["vllm:num_requests_waiting", "llamacpp:requests_deferred", "sglang:num_queue_reqs"]);
        let requests_total = prom::first_sum(samples, &["vllm:request_success_total"]).or_else(|| {
            let known = suffix_sum(samples, &["request_success_total", "requests_completed_total"], extra);
            known.or_else(|| suffix_sum(samples, &["requests_total", "request_count_total", "_request_count"], extra))
        });
        Counters {
            running,
            waiting,
            requests_total,
            prompt_tokens: prom::first_sum(samples, &["vllm:prompt_tokens_total", "llamacpp:prompt_tokens_total", "sglang:prompt_tokens_total"]),
            generation_tokens: prom::first_sum(
                samples,
                &["vllm:generation_tokens_total", "llamacpp:tokens_predicted_total", "sglang:generation_tokens_total"],
            ),
            kv_usage: prom::first_sum(samples, &["vllm:kv_cache_usage_perc", "vllm:gpu_cache_usage_perc"]).map(|v| v.clamp(0.0, 1.0)),
            spec_accepted: prom::first_sum(samples, &["vllm:spec_decode_num_accepted_tokens_total"]),
            spec_drafted: prom::first_sum(samples, &["vllm:spec_decode_num_draft_tokens_total"]),
        }
    }

    fn any(&self) -> bool {
        self.running.is_some() || self.requests_total.is_some() || self.generation_tokens.is_some() || self.prompt_tokens.is_some()
    }
}

/// Successive /metrics samples turned into rates, busy periods and synthesized request records.
#[derive(Debug, Default, Clone)]
struct MetricsTrack {
    cur: Option<(f64, Counters)>,
    prev: Option<(f64, Counters)>,
    gen_rate: f64,
    prompt_rate: f64,
    /// Start of the current busy period and the counters then.
    busy_start: Option<(f64, Counters)>,
    records: VecDeque<RequestRecord>,
    record_id: u64,
    /// Last time a counter moved (or requests were in flight).
    last_change: Option<f64>,
    last_generated: u64,
    /// The preset's own health path (not in `PROBE_PATHS`): its request counters are KLIF's probes too.
    probe_path: Option<String>,
}

impl MetricsTrack {
    fn apply(&mut self, at: f64, samples: &[Sample]) {
        let c = Counters::from_samples_excluding(samples, self.probe_path.as_deref());
        if !c.any() {
            return;
        }
        self.prev = self.cur.take();
        self.cur = Some((at, c));
        let Some((t0, p)) = self.prev else { return };
        let dt = at - t0;
        if dt <= 0.05 {
            return;
        }
        let d = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) if a >= b => a - b,
            _ => 0.0,
        };
        let dg = d(c.generation_tokens, p.generation_tokens);
        let dp = d(c.prompt_tokens, p.prompt_tokens);
        let dr = d(c.requests_total, p.requests_total);
        let k = 1.0 - (-dt / DECODE_TAU_S).exp();
        let inst_g = dg / dt;
        self.gen_rate = if dg > 0.0 { if self.gen_rate <= 0.0 { inst_g } else { self.gen_rate + (inst_g - self.gen_rate) * k } } else { self.gen_rate };
        self.prompt_rate = dp / dt;
        let running = c.running.unwrap_or(0.0) > 0.0;
        if dg > 0.0 || dp > 0.0 || dr > 0.0 || running {
            self.last_change = Some(at);
        }
        match (self.busy_start, running) {
            (None, true) => self.busy_start = Some((t0, p)),
            (Some((s, b)), false) => {
                self.busy_start = None;
                let gen = d(c.generation_tokens, b.generation_tokens).round() as u64;
                let prompt = d(c.prompt_tokens, b.prompt_tokens).round() as u64;
                self.last_generated = gen;
                if gen > 0 || prompt > 0 {
                    self.record_id += 1;
                    let span = (at - s).max(0.0);
                    self.records.push_back(RequestRecord {
                        id: self.record_id,
                        at: at.round(),
                        prompt_tokens: prompt,
                        cached_tokens: 0,
                        prefill_s: 0.0,
                        generated_tokens: gen,
                        decode_s: round_to(span, 2),
                    });
                    while self.records.len() > 12 {
                        self.records.pop_front();
                    }
                }
            }
            _ => {}
        }
    }

    fn fresh(&self, now: f64) -> Option<&Counters> {
        self.cur.as_ref().filter(|(t, _)| now - t <= METRICS_FRESH_S).map(|(_, c)| c)
    }

    fn running(&self, now: f64) -> bool {
        self.fresh(now).and_then(|c| c.running).map(|r| r > 0.0).unwrap_or(false)
    }

    fn busy_since(&self, now: f64) -> Option<f64> {
        if self.running(now) {
            Some(self.busy_start.map(|b| b.0).unwrap_or(now))
        } else {
            None
        }
    }

    fn activity(&self, now: f64) -> LlmActivity {
        if !self.running(now) {
            return LlmActivity::Idle;
        }
        let moving_gen = self.cur.zip(self.prev).map(|((_, c), (_, p))| c.generation_tokens > p.generation_tokens).unwrap_or(false);
        let moving_prompt = self.cur.zip(self.prev).map(|((_, c), (_, p))| c.prompt_tokens > p.prompt_tokens).unwrap_or(false);
        if !moving_gen && moving_prompt { LlmActivity::Prefill } else { LlmActivity::Decode }
    }

    fn generated_now(&self) -> u64 {
        match (self.busy_start, self.cur) {
            (Some((_, b)), Some((_, c))) => match (c.generation_tokens, b.generation_tokens) {
                (Some(a), Some(b)) if a >= b => (a - b).round() as u64,
                _ => 0,
            },
            _ => self.last_generated,
        }
    }

    fn totals(&self) -> Totals {
        let c = self.cur.map(|x| x.1).unwrap_or_default();
        Totals {
            requests: c.requests_total.map(|v| v.max(0.0).round() as u64).unwrap_or(self.record_id),
            prompt_tokens: c.prompt_tokens.map(|v| v.max(0.0).round() as u64).unwrap_or(0),
            generated_tokens: c.generation_tokens.map(|v| v.max(0.0).round() as u64).unwrap_or(0),
        }
    }

    fn generic(&self, now: f64) -> Option<(Option<u64>, Option<u64>)> {
        let c = self.fresh(now)?;
        Some((c.running.map(|v| v.max(0.0).round() as u64), c.requests_total.map(|v| v.max(0.0).round() as u64)))
    }
}

// ---------------------------------------------------------------------------------------- llama.cpp

/// llama.cpp `llama-server`: logs (prefix + timestamps, verbosity 4) + /health + /slots + /v1/models (+ /metrics).
pub struct LlamaCpp {
    ctx: AdapterCtx,
    p: LlamaParser,
    slots: Option<SlotSample>,
    prev_slots: Option<SlotSample>,
    slots_ema: Option<f64>,
    models: Option<ModelsInfo>,
    decode_tps: f64,
    decode_hist: RateHistory,
    last_poll: Option<f64>,
    finished_seen: Option<u64>,
    metrics: MetricsTrack,
    last_line: Option<f64>,
}

impl Default for LlamaCpp {
    fn default() -> Self {
        Self::new(AdapterCtx::default())
    }
}

impl LlamaCpp {
    pub fn new(ctx: AdapterCtx) -> Self {
        let anchor = if ctx.from_start && ctx.started_at > 0.0 { Some(ctx.started_at) } else { None };
        LlamaCpp {
            p: LlamaParser::new(anchor),
            ctx,
            slots: None,
            prev_slots: None,
            slots_ema: None,
            models: None,
            decode_tps: 0.0,
            decode_hist: RateHistory::default(),
            last_poll: None,
            finished_seen: None,
            metrics: MetricsTrack::default(),
            last_line: None,
        }
    }

    pub fn parser(&self) -> &LlamaParser {
        &self.p
    }

    /// Feed a `/slots` answer (slot 0, or the processing one with a single-slot server).
    pub fn apply_slots(&mut self, slots: &[SlotSample]) {
        let Some(s) = slots.iter().find(|s| s.is_processing).or_else(|| slots.first()).cloned() else { return };
        // Live decode rate from n_decoded deltas of the same task.
        if let (Some(prev), Some(n1)) = (&self.slots, s.n_decoded) {
            let same = prev.id_task == s.id_task && prev.id == s.id;
            let dt = s.at - prev.at;
            match (same, prev.n_decoded) {
                (true, Some(n0)) if s.is_processing && n1 > 0 && n1 >= n0 && dt > 0.2 => {
                    let inst = (n1 - n0) as f64 / dt;
                    let a = 1.0 - (-dt / DECODE_TAU_S).exp();
                    self.slots_ema = Some(match self.slots_ema {
                        Some(e) => e + (inst - e) * a,
                        None => inst,
                    });
                }
                (true, _) if s.is_processing && n1 > 0 => {}
                _ => self.slots_ema = None,
            }
        }
        let p = &mut self.p;
        let task = s.id_task;
        if s.is_processing {
            let fresh_task = p.cur.as_ref().map(|r| Some(r.task) != task).unwrap_or(true);
            if fresh_task && task.is_some() && p.last.as_ref().map(|r| Some(r.task) != task).unwrap_or(true) {
                // The log has not shown this request yet (lv3 short prompt, or poll order).
                if let Some(prev) = p.cur.take() {
                    p.last = Some(prev);
                }
                p.cur = Some(Req { task: task.unwrap_or(0), start_t: s.at, ..Default::default() });
            }
            if let Some(r) = p.cur.as_mut().filter(|r| Some(r.task) == task) {
                if let Some(c) = s.n_prompt_tokens_cache {
                    r.cached = Some(c);
                    r.cached_exact = true;
                    r.exact = r.exact || r.prompt_total.is_some();
                }
                if let Some(n) = s.n_prompt_tokens_processed {
                    r.slots_pp = Some((s.at, n));
                }
                match s.n_decoded {
                    Some(d) if d > 0 => {
                        r.decode_start_t.get_or_insert(s.at);
                        r.slots_gen = Some((s.at, d));
                    }
                    _ => {}
                }
            }
        }
        self.prev_slots = self.slots.take();
        self.slots = Some(s);
    }

    fn fresh_slots(&self, now: f64) -> Option<&SlotSample> {
        self.slots.as_ref().filter(|s| now - s.at <= SLOTS_FRESH_S)
    }

    fn activity(&self, now: f64) -> Activity {
        let p = &self.p;
        let log = p.activity();
        match self.fresh_slots(now) {
            Some(s) if s.at >= p.last_event_t => {
                if !s.is_processing {
                    Activity::Idle
                } else if s.n_decoded.unwrap_or(0) > 0 {
                    Activity::Decode
                } else {
                    Activity::Prefill
                }
            }
            _ => log,
        }
    }

    /// Instantaneous decode rate right now, if measurable.
    fn inst_decode(&self, now: f64) -> Option<f64> {
        if self.activity(now) != Activity::Decode {
            return None;
        }
        if self.fresh_slots(now).is_some() {
            if let Some(e) = self.slots_ema {
                return Some(e);
            }
        }
        self.p.cur.as_ref().and_then(|r| r.tg_ticks.last()).map(|t| t.3)
    }

    fn update_decode(&mut self, now: f64) {
        let dt = self.last_poll.map(|t| (now - t).max(0.0)).unwrap_or(0.5);
        self.last_poll = Some(now);
        // A request finished: show its final average.
        if let Some(r) = &self.p.last {
            if self.finished_seen != Some(r.task) && r.end_t.is_some() {
                self.finished_seen = Some(r.task);
                if let Some((_, n, tps)) = r.tg_final {
                    if n > 0 && tps > 0.0 {
                        self.decode_tps = tps;
                    }
                }
            }
        }
        let act = self.activity(now);
        let inst = self.inst_decode(now);
        if let Some(v) = inst {
            let a = 1.0 - (-dt / DECODE_TAU_S).exp();
            self.decode_tps = if self.decode_tps <= 0.0 { v } else { self.decode_tps + (v - self.decode_tps) * a };
        }
        let sample = if act == Activity::Decode { inst.unwrap_or(self.decode_tps) } else { 0.0 };
        self.decode_hist.push(now, sample);
    }

    fn llm_live(&self, now: f64) -> LlmLive {
        let p = &self.p;
        let act = self.activity(now);
        let slots = self.fresh_slots(now);
        let cur_active = act != Activity::Idle && p.cur.is_some();
        let r: Option<&Req> = if cur_active { p.cur.as_ref() } else { p.last.as_ref().or(p.cur.as_ref()) };

        let mut cached_now = 0u64;
        let mut todo_now = 0u64;
        let prefill = r.map(|r| {
            let (todo, cached) = r.prefill_shape();
            let todo = todo.or_else(|| if r.decode_start_t.is_some() { r.slots_pp.map(|x| x.1) } else { None });
            let cached = cached.unwrap_or(0);
            cached_now = cached;
            if cur_active && act == Activity::Prefill {
                let (t_k, n_k) = r.pp_known().unwrap_or((r.start_t, 0));
                let rate = r.pp_rate().or(p.last_pp_rate).unwrap_or(0.0);
                let mut done = n_k as f64;
                if let Some(tot) = todo {
                    // Ticks are a batch apart (up to ~20 s): interpolate, but never past the next tick
                    // (one batch ahead) nor past the end.
                    let cap = r.pp_batch().map(|b| (n_k + b) as f64).unwrap_or(tot as f64).min(tot as f64);
                    done = (done + rate * (now - t_k).max(0.0)).min(cap).max(n_k as f64);
                }
                let tokens = todo.unwrap_or(n_k);
                todo_now = done.round() as u64;
                let remaining = tokens as f64 - done;
                Prefill {
                    tokens,
                    done_tokens: done.round() as u64,
                    cached_tokens: Some(cached),
                    tps: round_to(rate, 1),
                    elapsed_s: round_to((now - r.start_t).max(0.0), 1),
                    eta_s: if rate > 0.0 && remaining > 0.0 { round_to(remaining / rate, 1) } else { 0.0 },
                }
            } else {
                let tokens = r.pp_final.map(|x| x.1).or(todo).unwrap_or(0);
                todo_now = tokens;
                let secs = r
                    .pp_final
                    .map(|x| x.0 / 1000.0)
                    .or_else(|| r.decode_start_t.map(|d| d - r.start_t))
                    .unwrap_or(0.0)
                    .max(0.0);
                let tps = r
                    .pp_final
                    .map(|x| x.2)
                    .or_else(|| r.pp_ticks.last().map(|t| t.avg_tps))
                    .unwrap_or(if secs > 0.0 { tokens as f64 / secs } else { 0.0 });
                Prefill { tokens, done_tokens: tokens, cached_tokens: Some(cached), tps: round_to(tps, 1), elapsed_s: round_to(secs, 1), eta_s: 0.0 }
            }
        });

        let generated = match (act, r) {
            (Activity::Prefill, _) | (_, None) => 0,
            (Activity::Decode, Some(r)) => {
                if let Some(d) = slots.and_then(|s| s.n_decoded).filter(|_| cur_active) {
                    d
                } else if let Some(t) = r.tg_ticks.last() {
                    // Log ticks come every ~3 s: extrapolate with the window rate, at most one tick ahead.
                    let ahead = (now - t.0).clamp(0.0, 3.5);
                    t.1 + (t.3 * ahead).round() as u64
                } else {
                    r.slots_gen.map(|x| x.1).unwrap_or(0)
                }
            }
            (Activity::Idle, Some(r)) => r.tg_final.map(|x| x.1).or(r.tg_ticks.last().map(|t| t.1)).unwrap_or(0),
        };

        let total = self
            .slots
            .as_ref()
            .and_then(|s| s.n_ctx)
            .or(p.n_ctx_slot.map(|v| v as u64))
            .or(self.models.as_ref().and_then(|m| m.n_ctx))
            .or(self.ctx.ctx_tokens.map(|v| v as u64))
            .unwrap_or(0);
        let used = match slots.and_then(|s| s.n_prompt_tokens) {
            Some(n) => n,
            None => match (act, r) {
                (_, None) => 0,
                (Activity::Prefill, Some(_)) => cached_now + todo_now,
                (Activity::Decode, Some(_)) => cached_now + todo_now + generated,
                (Activity::Idle, Some(r)) => r.release_n.unwrap_or(cached_now + todo_now + generated),
            },
        };

        let mode = self.ctx.spec_mode.clone().or_else(|| p.spec_label());
        let spec = mode.map(|m| {
            let acc = r.and_then(|r| r.accept).or(p.last_accept).unwrap_or(0.0);
            Spec { acceptance_pct: round_to(acc * 100.0, 1), mode: m, active: Some(act == Activity::Decode) }
        });

        LlmLive {
            activity: match act {
                Activity::Idle => LlmActivity::Idle,
                Activity::Prefill => LlmActivity::Prefill,
                Activity::Decode => LlmActivity::Decode,
            },
            decode_tps: round_to(self.decode_tps, 1),
            decode_history: self.decode_hist.values(),
            prefill,
            generated_tokens: generated,
            context: ContextFill { used_tokens: used.min(total.max(used)), total_tokens: total },
            spec,
            requests: p.records.iter().cloned().collect(),
            totals: p.totals.clone(),
        }
    }
}

impl BackendAdapter for LlamaCpp {
    fn id(&self) -> AdapterId {
        AdapterId::LlamaCpp
    }
    fn default_health(&self, _kind: SystemKind) -> HealthCheck {
        HealthCheck::Http { path: "/health".into() }
    }
    fn reports_spill(&self) -> bool {
        true
    }
    fn probe_period(&self) -> f64 {
        0.5
    }
    fn probe_wants(&self, health: Health) -> ProbeWants {
        let ready = health == Health::Ready;
        ProbeWants {
            slots: ready,
            models: ready && self.models.is_none(),
            metrics: if ready && self.ctx.metrics { Some(2.0) } else { None },
            version: false,
        }
    }
    fn feed(&mut self, seg: &Segment) {
        self.last_line = Some(seg.at);
        match seg.stream {
            Stream::Err => self.p.feed_err(&seg.text, seg.at),
            Stream::Out => self.p.feed_out(&seg.text, seg.at),
        }
    }
    fn tick(&mut self, now: f64) {
        self.update_decode(now);
    }
    fn apply(&mut self, at: f64, r: ProbeResult) {
        match r {
            ProbeResult::Slots(v) => self.apply_slots(&v),
            ProbeResult::Models(m) => self.models = Some(m),
            ProbeResult::Metrics(s) => self.metrics.apply(at, &s),
            ProbeResult::Version(_) => {}
        }
    }
    fn health(&self, probe: Option<Health>) -> Health {
        if let Some(h) = probe {
            return h;
        }
        let p = &self.p;
        if p.fatal_hint.is_some() {
            // Without a probe, a fatal line is the best evidence that nothing answers.
            Health::Down
        } else if p.listening.is_some() || p.loaded {
            Health::Ready
        } else if p.any_line {
            Health::Loading
        } else {
            Health::Down
        }
    }
    fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64) {
        let mut steps = self.p.steps.clone();
        if steps.detail[0].is_none() {
            steps.detail[0] = Some(match &self.p.build {
                Some(b) => format!("llama.cpp b{b}"),
                None => "llama.cpp".to_string(),
            });
        }
        six_steps(steps, false, health, &self.ctx)
    }
    fn busy_since(&self, now: f64) -> Option<f64> {
        if self.activity(now) == Activity::Idle {
            return None;
        }
        Some(self.p.cur.as_ref().map(|r| r.start_t).unwrap_or(now))
    }
    fn layers_from_logs(&self) -> Option<Vec<VramLayer>> {
        self.p.layers()
    }
    fn reported_devices(&self) -> Vec<String> {
        self.p.gpu_devices()
    }
    fn backend_build(&self) -> Option<String> {
        self.p.build_label()
    }
    fn fill(&self, now: f64, sig: &mut SessionSignals) {
        let p = &self.p;
        let live = self.llm_live(now);
        if self.ctx.kind != SystemKind::Llm || self.metrics.cur.is_some() {
            let (inflight, total) = self.metrics.generic(now).unwrap_or((None, None));
            sig.generic = Some(GenericLive {
                requests_in_flight: inflight.or(Some(u64::from(live.activity != LlmActivity::Idle))),
                requests_total: total.or(Some(live.totals.requests)),
                last_activity_s: self.last_line.map(|t| round_to((now - t).max(0.0), 1)),
                model_id: self.models.as_ref().and_then(|m| m.id.clone()),
            });
        }
        sig.llm = Some(live);
        sig.arch = p.arch();
        sig.median_decode_tps = self.median_decode_tps();
        sig.error_tail = p.error_tail.iter().cloned().collect();
        sig.fatal_hint = p.fatal_hint.clone();
    }
    fn median_decode_tps(&self) -> Option<f64> {
        if self.p.finished_rates.is_empty() {
            return None;
        }
        Some(crate::sd::median(&self.p.finished_rates))
    }
    fn llama(&self) -> Option<&LlamaParser> {
        Some(&self.p)
    }
}

/// The six log steps as the UI shows them (the "Ready" step finishes on a ready health).
fn six_steps(mut steps: Steps, image: bool, health: Health, ctx: &AdapterCtx) -> (Vec<LoadStep>, f64) {
    let failed = steps.state.contains(&StepState::Failed);
    if health == Health::Ready && !failed {
        steps.finish(5);
        steps.detail[5] = Some(format!("{}:{}", ctx.host, ctx.port));
    }
    let out = STEP_IDS
        .iter()
        .enumerate()
        .map(|(i, id)| LoadStep {
            id: *id,
            label: step_label(*id, image).to_string(),
            state: steps.state[i],
            detail: if steps.state[i] == StepState::Pending { None } else { steps.detail[i].clone() },
        })
        .collect();
    (out, round_to(steps.fraction(), 3))
}

// ------------------------------------------------------------------------------------------ sd.cpp

/// sd.cpp `sd-server`: logs + TCP.
pub struct SdCpp {
    ctx: AdapterCtx,
    p: SdParser,
    last_line: Option<f64>,
}

impl Default for SdCpp {
    fn default() -> Self {
        Self::new(AdapterCtx { kind: SystemKind::Image, ..AdapterCtx::default() })
    }
}

impl SdCpp {
    pub fn new(ctx: AdapterCtx) -> Self {
        SdCpp { ctx, p: SdParser::default(), last_line: None }
    }
}

impl BackendAdapter for SdCpp {
    fn id(&self) -> AdapterId {
        AdapterId::SdCpp
    }
    fn default_health(&self, _kind: SystemKind) -> HealthCheck {
        HealthCheck::Tcp
    }
    fn reports_spill(&self) -> bool {
        false
    }
    fn probe_period(&self) -> f64 {
        1.0
    }
    fn feed(&mut self, seg: &Segment) {
        self.last_line = Some(seg.at);
        self.p.feed(&seg.text, seg.at, seg.stream == Stream::Err);
    }
    fn health(&self, probe: Option<Health>) -> Health {
        let p = &self.p;
        match probe {
            Some(Health::Ready) => return Health::Ready,
            Some(Health::Loading) => return Health::Loading,
            Some(Health::Down) => {
                // No listener: still loading, unless it already listened once or crashed.
                let gone = p.listening.is_some() || p.fatal_hint.is_some() || !p.any_line;
                return if gone { Health::Down } else { Health::Loading };
            }
            None => {}
        }
        if p.fatal_hint.is_some() {
            Health::Down
        } else if p.listening.is_some() {
            Health::Ready
        } else if p.any_line {
            Health::Loading
        } else {
            Health::Down
        }
    }
    fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64) {
        let mut steps = self.p.steps.clone();
        if steps.detail[0].is_none() {
            steps.detail[0] = Some("sd.cpp".to_string());
        }
        six_steps(steps, true, health, &self.ctx)
    }
    fn busy_since(&self, _now: f64) -> Option<f64> {
        self.p.job_start()
    }
    fn reported_devices(&self) -> Vec<String> {
        self.p.device_name.clone().into_iter().collect()
    }
    fn fill(&self, now: f64, sig: &mut SessionSignals) {
        let p = &self.p;
        let image = p.live(now);
        if self.ctx.kind != SystemKind::Image {
            sig.generic = Some(GenericLive {
                requests_in_flight: Some(u64::from(p.generating())),
                requests_total: Some(p.images),
                last_activity_s: self.last_line.map(|t| round_to((now - t).max(0.0), 1)),
                model_id: None,
            });
        }
        sig.image = Some(image);
        sig.error_tail = p.error_tail.iter().cloned().collect();
        sig.fatal_hint = p.fatal_hint.clone();
        sig.starter_exit = p.starter_exit;
    }
    fn sd(&self) -> Option<&SdParser> {
        Some(&self.p)
    }
}

// ------------------------------------------------------------------- servers without a log parser

static VLLM_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"vLLM API server version (\S+)").expect("vllm version"));
static PY_EXC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z_][\w.]*\.)?([A-Za-z_]\w*(?:Error|Exception)): (.{1,200})").expect("py exception")
});
static WIN_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[A-Za-z]:\\(?:[^\\'"\r\n]+\\)*([^\\'"\r\n]*)"#).expect("win path"));
static UNIX_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:/[\w.-]+){2,}/([\w.-]+)").expect("unix path"));

fn short_paths(msg: &str) -> String {
    let s = WIN_PATH.replace_all(msg, |c: &regex::Captures| c[1].to_string());
    UNIX_PATH.replace_all(&s, |c: &regex::Captures| c[1].to_string()).into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Vllm,
    OpenAi,
    Generic,
}

/// The shared body of vLLM / OpenAI-compatible / generic sessions: short load steps, health from probes (with
/// "loading" while a launched process logs but does not answer yet), log-activity busy pulses, /metrics
/// counters, /v1/models, faults from Python tracebacks before the server was ever ready.
struct ApiServer {
    ctx: AdapterCtx,
    flavor: Flavor,
    steps: Steps,
    any_line: bool,
    ever_ready: bool,
    /// Arrival of the last log line that is not a probe's own access line.
    last_line: Option<f64>,
    models: Option<ModelsInfo>,
    version: Option<String>,
    metrics: MetricsTrack,
    decode_hist: RateHistory,
    traceback: bool,
    fatal_hint: Option<String>,
    error_tail: VecDeque<String>,
    /// Access-log lines of KLIF's own probes: the standard probe paths plus the preset's health path, in any of
    /// the common log formats (`probe_line_regex`).
    probe_re: Regex,
}

impl ApiServer {
    fn new(ctx: AdapterCtx, flavor: Flavor) -> Self {
        let extra = extra_probe_path(ctx.health_path.as_deref());
        ApiServer {
            probe_re: probe_line_regex(extra.as_deref()),
            metrics: MetricsTrack { probe_path: extra, ..MetricsTrack::default() },
            ctx,
            flavor,
            steps: Steps::default(),
            any_line: false,
            ever_ready: false,
            last_line: None,
            models: None,
            version: None,
            decode_hist: RateHistory::default(),
            traceback: false,
            fatal_hint: None,
            error_tail: VecDeque::new(),
        }
    }

    /// The line is the access-log line of one of KLIF's own probes.
    fn is_probe(&self, l: &str) -> bool {
        self.probe_re.is_match(l)
    }

    /// A KLIF-launched server without a port (a generic preset KLIF found no port for): nothing can be probed, so
    /// it counts as ready while its process runs (the engine watches the process) and no fatal hint was seen.
    fn portless(&self) -> bool {
        !self.ctx.external && self.ctx.port == 0
    }

    fn push_error(&mut self, line: &str) {
        self.error_tail.push_back(redact(short_paths(line.trim_end())));
        while self.error_tail.len() > 30 {
            self.error_tail.pop_front();
        }
    }

    fn feed(&mut self, seg: &Segment) {
        let l = seg.text.trim_end();
        if l.trim().is_empty() {
            return;
        }
        if !self.any_line {
            self.any_line = true;
            self.steps.finish(0);
            self.steps.reach(2);
        }
        if !self.is_probe(l) {
            self.last_line = Some(seg.at);
        }
        if self.flavor == Flavor::Vllm && self.version.is_none() {
            if let Some(c) = VLLM_VERSION.captures(l) {
                self.version = Some(c[1].trim_end_matches(['.', ',']).to_string());
            }
        }
        let low = l.to_ascii_lowercase();
        if l.starts_with("Traceback (most recent call last)") {
            self.traceback = true;
            self.push_error(l);
        } else if let Some(c) = PY_EXC.captures(l.trim_start()) {
            self.push_error(l);
            if self.traceback && !self.ever_ready && self.fatal_hint.is_none() {
                self.fatal_hint = Some(format!("{}: {}", &c[1], short_paths(c[2].trim())));
                self.steps.fail_active();
            }
            self.traceback = false;
        } else if low.contains("error") && (seg.stream == Stream::Err || low.contains("[error") || low.starts_with("error")) && !self.is_probe(l) {
            self.push_error(l);
        }
    }

    fn on_health(&mut self, h: Health) {
        if h != Health::Down && self.steps.state[0] != StepState::Done {
            self.steps.finish(0);
            self.steps.reach(2);
        }
        if h == Health::Ready {
            self.ever_ready = true;
            self.steps.finish(2);
            self.steps.finish(5);
        }
    }

    fn health(&self, probe: Option<Health>) -> Health {
        match probe {
            Some(Health::Down) if !self.ctx.external && self.any_line && self.fatal_hint.is_none() && !self.ever_ready => Health::Loading,
            Some(h) => h,
            None if self.ctx.external => Health::Down,
            None if self.fatal_hint.is_some() => Health::Down,
            None if self.portless() => Health::Ready,
            None if self.any_line => Health::Loading,
            None => Health::Down,
        }
    }

    fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64) {
        let mut steps = self.steps.clone();
        if health == Health::Ready && !steps.state.contains(&StepState::Failed) {
            steps.finish(0);
            steps.finish(2);
            steps.finish(5);
            steps.detail[5] = Some(if self.portless() {
                "process running (no port)".to_string()
            } else {
                format!("{}:{}", self.ctx.host, self.ctx.port)
            });
        } else if health == Health::Loading && steps.state[0] != StepState::Done {
            steps.finish(0);
            steps.reach(2);
        }
        if steps.detail[0].is_none() {
            steps.detail[0] = Some(match (self.flavor, &self.version) {
                (Flavor::Vllm, Some(v)) => format!("vLLM {v}"),
                (Flavor::Vllm, None) => "vLLM".into(),
                (Flavor::OpenAi, _) => "OpenAI-compatible".into(),
                (Flavor::Generic, _) => "generic".into(),
            });
        }
        if steps.detail[2].is_none() {
            steps.detail[2] = self.models.as_ref().and_then(|m| m.id.clone()).map(|id| crate::text::file_name(&id).to_string());
        }
        steps::short_view(&steps)
    }

    fn busy_since(&self, now: f64) -> Option<f64> {
        if let Some(b) = self.metrics.busy_since(now) {
            return Some(b);
        }
        let t = self.last_line.filter(|t| self.ever_ready && now - t <= BUSY_PULSE_S)?;
        Some(t)
    }

    fn last_activity(&self) -> Option<f64> {
        match (self.last_line, self.metrics.last_change) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    fn tick(&mut self, now: f64) {
        if self.portless() && !self.ever_ready && self.fatal_hint.is_none() {
            // No probe will ever answer: the first poll that saw no startup traceback makes it ready (log-activity
            // busy pulses then work, and a later traceback is a request's error, not a failed start).
            self.on_health(Health::Ready);
        }
        let sample = if self.metrics.activity(now) == LlmActivity::Decode { self.metrics.gen_rate } else { 0.0 };
        self.decode_hist.push(now, sample);
    }

    fn llm_live(&self, now: f64) -> LlmLive {
        let m = &self.metrics;
        let activity = m.activity(now);
        let total = self.models.as_ref().and_then(|x| x.n_ctx).or(self.ctx.ctx_tokens.map(u64::from)).unwrap_or(0);
        let fresh = m.fresh(now).copied();
        let used = match (fresh.and_then(|c| c.kv_usage), activity) {
            (Some(k), _) if total > 0 => (k * total as f64).round() as u64,
            _ => 0,
        };
        let prefill = if activity == LlmActivity::Prefill {
            let (start, base) = m.busy_start.unwrap_or((now, Counters::default()));
            let done = match (fresh.and_then(|c| c.prompt_tokens), base.prompt_tokens) {
                (Some(a), Some(b)) if a >= b => (a - b).round() as u64,
                _ => 0,
            };
            Some(Prefill {
                tokens: done,
                done_tokens: done,
                cached_tokens: None,
                tps: round_to(m.prompt_rate, 1),
                elapsed_s: round_to((now - start).max(0.0), 1),
                eta_s: 0.0,
            })
        } else {
            None
        };
        let spec = match (fresh.and_then(|c| c.spec_accepted), fresh.and_then(|c| c.spec_drafted)) {
            (Some(a), Some(d)) if d > 0.0 => Some(Spec {
                acceptance_pct: round_to(a / d * 100.0, 1),
                mode: self.ctx.spec_mode.clone().unwrap_or_else(|| "spec".into()),
                active: Some(activity == LlmActivity::Decode),
            }),
            _ => self.ctx.spec_mode.clone().map(|mode| Spec { acceptance_pct: 0.0, mode, active: Some(activity == LlmActivity::Decode) }),
        };
        LlmLive {
            activity,
            decode_tps: round_to(m.gen_rate, 1),
            decode_history: self.decode_hist.values(),
            prefill,
            generated_tokens: if activity == LlmActivity::Prefill { 0 } else { m.generated_now() },
            context: ContextFill { used_tokens: used.min(total), total_tokens: total },
            spec,
            requests: m.records.iter().cloned().collect(),
            totals: m.totals(),
        }
    }

    fn fill(&self, now: f64, sig: &mut SessionSignals) {
        let (inflight, total) = self.metrics.generic(now).unwrap_or((None, None));
        sig.generic = Some(GenericLive {
            // Without /metrics (or without an in-flight counter in it) the log-activity busy pulse stands in, like
            // llama.cpp and sd.cpp do, so busy drives the heroes of tts / stt / video Systems (SPEC 13.1).
            requests_in_flight: inflight.or(Some(u64::from(self.busy_since(now).is_some()))),
            requests_total: total,
            last_activity_s: self.last_activity().map(|t| round_to((now - t).max(0.0), 1)),
            model_id: self.models.as_ref().and_then(|m| m.id.clone()),
        });
        if self.ctx.kind == SystemKind::Llm {
            // Without metrics this is the idle shape with what is known (context from /v1/models or the preset).
            sig.llm = Some(self.llm_live(now));
        }
        if self.ctx.kind == SystemKind::Image {
            sig.image = Some(klif_common::vm::ImageLive {
                activity: if self.busy_since(now).is_some() {
                    klif_common::vm::ImageActivity::Generating
                } else {
                    klif_common::vm::ImageActivity::Idle
                },
                step: 0,
                steps: 0,
                s_per_it: 0.0,
                elapsed_s: self.busy_since(now).map(|b| round_to((now - b).max(0.0), 1)).unwrap_or(0.0),
                width: 0,
                height: 0,
                edit: false,
                recent: Vec::new(),
                images_this_session: total.unwrap_or(0),
            });
        }
        sig.error_tail = self.error_tail.iter().cloned().collect();
        sig.fatal_hint = self.fatal_hint.clone();
    }

    fn probe_wants(&self, health: Health, metrics_default: Option<f64>) -> ProbeWants {
        let ready = health == Health::Ready;
        ProbeWants {
            slots: false,
            models: ready && self.models.is_none() && self.flavor != Flavor::Generic,
            metrics: if ready { metrics_default } else { None },
            version: ready && self.flavor == Flavor::Vllm && self.version.is_none(),
        }
    }

    fn apply(&mut self, at: f64, r: ProbeResult) {
        match r {
            ProbeResult::Models(m) => {
                if self.steps.detail[2].is_none() {
                    self.steps.detail[2] = m.id.as_ref().map(|id| crate::text::file_name(id).to_string());
                }
                self.models = Some(m);
            }
            ProbeResult::Metrics(s) => self.metrics.apply(at, &s),
            ProbeResult::Version(v) => self.version = Some(v),
            ProbeResult::Slots(_) => {}
        }
    }
}

macro_rules! api_adapter {
    ($ty:ident, $flavor:expr, $doc:literal) => {
        #[doc = $doc]
        pub struct $ty {
            s: ApiServer,
        }

        impl Default for $ty {
            fn default() -> Self {
                Self::new(AdapterCtx::default())
            }
        }

        impl $ty {
            pub fn new(ctx: AdapterCtx) -> Self {
                $ty { s: ApiServer::new(ctx, $flavor) }
            }
        }
    };
}

api_adapter!(Vllm, Flavor::Vllm, "vLLM: /health + /metrics + /v1/models (max_model_len) + /version.");
api_adapter!(OpenAi, Flavor::OpenAi, "Any OpenAI-compatible server: /health -> /v1/models -> TCP, /metrics if it answers.");
api_adapter!(
    Generic,
    Flavor::Generic,
    "Any other server (TTS / STT / video...): TCP or the preset's HTTP health, log-activity busy pulses, /metrics if it \
     answers Prometheus text, request counters when available."
);

macro_rules! api_impl {
    ($ty:ident, $id:expr, $health:expr, $spill:expr, $period:expr, $metrics:expr) => {
        impl BackendAdapter for $ty {
            fn id(&self) -> AdapterId {
                $id
            }
            fn default_health(&self, _kind: SystemKind) -> HealthCheck {
                $health
            }
            fn reports_spill(&self) -> bool {
                $spill
            }
            fn probe_period(&self) -> f64 {
                $period
            }
            fn probe_wants(&self, health: Health) -> ProbeWants {
                self.s.probe_wants(health, $metrics)
            }
            fn feed(&mut self, seg: &Segment) {
                self.s.feed(seg)
            }
            fn console_keep(&self, line: &str) -> bool {
                !self.s.is_probe(line)
            }
            fn tick(&mut self, now: f64) {
                self.s.tick(now)
            }
            fn on_health(&mut self, _at: f64, h: Health) {
                self.s.on_health(h)
            }
            fn apply(&mut self, at: f64, r: ProbeResult) {
                self.s.apply(at, r)
            }
            fn health(&self, probe: Option<Health>) -> Health {
                self.s.health(probe)
            }
            fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64) {
                self.s.steps_view(health)
            }
            fn busy_since(&self, now: f64) -> Option<f64> {
                self.s.busy_since(now)
            }
            fn backend_build(&self) -> Option<String> {
                self.s.version.clone()
            }
            fn fill(&self, now: f64, sig: &mut SessionSignals) {
                self.s.fill(now, sig)
            }
        }
    };
}

api_impl!(Vllm, AdapterId::Vllm, HealthCheck::Http { path: "/health".into() }, true, 0.5, Some(1.0));
api_impl!(OpenAi, AdapterId::OpenAi, HealthCheck::Auto, true, 0.5, Some(2.0));
api_impl!(Generic, AdapterId::Generic, HealthCheck::Tcp, false, 1.0, Some(2.0));
