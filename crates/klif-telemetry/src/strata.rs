//! Strata's `/metrics` JSON (asked without `Accept: text/plain`, which would get the Prometheus text instead): the
//! engine's facts (`engine`), what it is doing now (`live`), the last finished requests with their timings
//! (`requests`) and the session totals (`totals`). Only the fields KLIF shows are read; a missing one stays None.

use serde_json::Value;

/// What the engine is doing (`live.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrataState {
    #[default]
    Idle,
    /// Reading the prompt (prefill).
    Reading,
    Generating,
    /// The model is not in memory (an idle unload).
    Unloaded,
}

/// One finished request (`requests[]`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrataRequest {
    /// When it finished (epoch s).
    pub time: f64,
    pub prompt_tokens: u64,
    /// Prompt tokens served from the conversation cache.
    pub reused: u64,
    pub output_tokens: u64,
    pub prompt_ms: f64,
    pub decode_ms: f64,
    pub drafts_offered: u64,
    pub drafts_accepted: u64,
}

/// One `/metrics` answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrataMetrics {
    /// `engine.version` ("0.1.42").
    pub version: Option<String>,
    /// `engine.context`: the context window it was started with.
    pub context: Option<u64>,
    pub state: StrataState,
    /// `live.phase` ("reading the prompt", "thinking", "answering"...).
    pub phase: Option<String>,
    pub prompt_tokens: Option<u64>,
    /// Prompt progress of a long, chunked prompt.
    pub prompt_read: Option<u64>,
    pub prompt_total: Option<u64>,
    pub generated: Option<u64>,
    /// Seconds since the request in flight started.
    pub elapsed_s: Option<f64>,
    /// The engine's decode rate over its last window.
    pub tok_s: Option<f64>,
    pub requests: Vec<StrataRequest>,
    pub total_requests: Option<u64>,
    pub total_prompt_tokens: Option<u64>,
    pub total_output_tokens: Option<u64>,
    pub total_drafts_offered: Option<u64>,
    pub total_drafts_accepted: Option<u64>,
}

impl StrataMetrics {
    pub fn busy(&self) -> bool {
        matches!(self.state, StrataState::Reading | StrataState::Generating)
    }
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn u(v: &Value, k: &str) -> Option<u64> {
    f(v, k).filter(|x| *x >= 0.0).map(|x| x.round() as u64)
}

/// Parse a `/metrics` JSON body. None when it is not Strata's (no `engine` or `live` object).
pub fn parse(body: &str) -> Option<StrataMetrics> {
    let v: Value = serde_json::from_str(body).ok()?;
    let engine = v.get("engine").filter(|e| e.is_object())?;
    let live = v.get("live").filter(|l| l.is_object())?;
    let totals = v.get("totals").cloned().unwrap_or(Value::Null);
    let state = match live.get("state").and_then(Value::as_str).unwrap_or("") {
        "reading" => StrataState::Reading,
        "generating" => StrataState::Generating,
        "unloaded" => StrataState::Unloaded,
        _ => StrataState::Idle,
    };
    let requests = v
        .get("requests")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some(StrataRequest {
                        time: f(r, "time")?,
                        prompt_tokens: u(r, "prompt_tokens").unwrap_or(0),
                        reused: u(r, "reused").unwrap_or(0),
                        output_tokens: u(r, "output_tokens").unwrap_or(0),
                        prompt_ms: f(r, "prompt_ms").unwrap_or(0.0).max(0.0),
                        decode_ms: f(r, "decode_ms").unwrap_or(0.0).max(0.0),
                        drafts_offered: u(r, "drafts_offered").unwrap_or(0),
                        drafts_accepted: u(r, "drafts_accepted").unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let version = engine
        .get("version")
        .or_else(|| engine.get("engine"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() < 64);
    Some(StrataMetrics {
        version,
        context: u(engine, "context").or_else(|| u(engine, "max_context")).filter(|c| *c > 0),
        state,
        phase: live.get("phase").and_then(Value::as_str).map(str::to_string),
        prompt_tokens: u(live, "prompt_tokens"),
        prompt_read: u(live, "prompt_read"),
        prompt_total: u(live, "prompt_total"),
        generated: u(live, "generated"),
        elapsed_s: f(live, "elapsed_s").map(|x| x.max(0.0)),
        tok_s: f(live, "tok_s").map(|x| x.max(0.0)),
        requests,
        total_requests: u(&totals, "requests"),
        total_prompt_tokens: u(&totals, "prompt_tokens"),
        total_output_tokens: u(&totals, "output_tokens"),
        total_drafts_offered: u(&totals, "drafts_offered"),
        total_drafts_accepted: u(&totals, "drafts_accepted"),
    })
}

