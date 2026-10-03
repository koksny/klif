//! One watched session: up to two log tailers, the console ring, the adapter (`backend::BackendAdapter`: parser,
//! probe wants, health rules, live shapes) and the latest health probe, combined into `SessionSignals`. Time is
//! always passed in (`now`, epoch seconds), so the same code runs live (wall clock) and in a replay (virtual
//! clock). External servers have no tailers: probes only.

use crate::backend::{self, AdapterCtx, BackendAdapter, ProbeResult, ProbeWants};
use crate::llama::LlamaParser;
use crate::probe::{ModelsInfo, SlotSample};
use crate::sd::SdParser;
use crate::tail::{ConsoleLine, LogTailer, Segment, Stream};
use crate::text::redact;
use crate::{Health, SessionSignals, WatchSpec};
use klif_common::vm::{HealthCheck, VramLayer};
use std::collections::VecDeque;

pub const CONSOLE_LEN: usize = 200;

pub struct SessionTracker {
    spec: WatchSpec,
    out: Option<LogTailer>,
    err: Option<LogTailer>,
    console: VecDeque<String>,
    noise_last: f64,
    noise_hidden: u64,
    probe_lines_hidden: bool,
    backend: Box<dyn BackendAdapter>,
    probes_enabled: bool,
    health_probe: Option<(f64, Health)>,
}

impl SessionTracker {
    /// `from_start`: read the logs from the beginning (adopted session) instead of their current end.
    pub fn new(spec: WatchSpec, from_start: bool) -> Self {
        let logs = !spec.external;
        let out = spec.out_log.as_ref().filter(|_| logs).map(|p| LogTailer::new(p, Stream::Out, from_start));
        let err = spec.err_log.as_ref().filter(|_| logs).map(|p| LogTailer::new(p, Stream::Err, from_start));
        let ctx = AdapterCtx {
            kind: spec.kind,
            external: spec.external,
            host: spec.host.clone(),
            port: spec.port,
            ctx_tokens: spec.ctx_tokens,
            spec_mode: spec.spec_mode.clone(),
            metrics: spec.metrics,
            started_at: spec.started_at,
            from_start,
            health_path: if let HealthCheck::Http { path } = &spec.health { Some(path.clone()) } else { None },
        };
        let backend = backend::for_ctx(spec.adapter, ctx);
        Self {
            spec,
            out,
            err,
            console: VecDeque::new(),
            noise_last: f64::NEG_INFINITY,
            noise_hidden: 0,
            probe_lines_hidden: false,
            backend,
            probes_enabled: true,
            health_probe: None,
        }
    }

    pub fn spec(&self) -> &WatchSpec {
        &self.spec
    }

    /// The session's adapter.
    pub fn backend(&self) -> &dyn BackendAdapter {
        self.backend.as_ref()
    }

    /// Replays run without HTTP probes: health then comes from the log markers alone.
    pub fn set_probes_enabled(&mut self, on: bool) {
        self.probes_enabled = on;
    }

    pub fn llama(&self) -> Option<&LlamaParser> {
        self.backend.llama()
    }

    pub fn sd(&self) -> Option<&SdParser> {
        self.backend.sd()
    }

    /// The health check the probes run: the preset's, else the adapter's default.
    pub fn health_plan(&self) -> backend::HealthPlan {
        backend::HealthPlan::resolve(&self.spec.health, &self.backend.default_health(self.spec.kind))
    }

    /// Health probe cadence while the server answers (s).
    pub fn probe_period(&self) -> f64 {
        self.backend.probe_period()
    }

    /// Extra probes the adapter wants next (given the current health).
    pub fn probe_wants(&self) -> ProbeWants {
        self.backend.probe_wants(self.health())
    }

    // ------------------------------------------------------------------------------ ingestion

    /// Read new log bytes, parse them, update smoothing and the 1 Hz histories.
    pub fn poll(&mut self, now: f64) {
        let mut segs: Vec<Segment> = Vec::new();
        let mut lines: Vec<ConsoleLine> = Vec::new();
        // stderr first: when a crash shows on both streams in one poll, the server's own error line
        // is the cause and the starter's exit line the consequence.
        if let Some(t) = self.err.as_mut() {
            t.poll(now, &mut segs, &mut lines);
        }
        if let Some(t) = self.out.as_mut() {
            t.poll(now, &mut segs, &mut lines);
        }
        for s in &segs {
            self.backend.feed(s);
        }
        for l in lines {
            self.push_console(l.text, now);
        }
        self.backend.tick(now);
    }

    fn push_console(&mut self, text: String, now: f64) {
        if !self.backend.console_keep(&text) {
            if !self.probe_lines_hidden {
                self.probe_lines_hidden = true;
                let what = match &self.spec.health {
                    HealthCheck::Http { path } if !matches!(path.as_str(), "/health" | "/metrics" | "/v1/models") => {
                        format!("/health, /metrics, /v1/models, {path}")
                    }
                    _ => "/health, /metrics, /v1/models".to_string(),
                };
                self.console_push(format!("[KLIF] access-log lines of KLIF's own probes ({what}) are hidden"));
            }
            return;
        }
        if text.contains("find_slot: non-consecutive token position") {
            if now - self.noise_last < 10.0 {
                self.noise_hidden += 1;
                return;
            }
            self.noise_last = now;
            if self.noise_hidden > 0 {
                let n = std::mem::take(&mut self.noise_hidden);
                self.console_push(format!("[KLIF] {n} more 'non-consecutive token position' lines hidden"));
            }
        }
        self.console_push(redact(text));
    }

    /// A KLIF line in the console, in arrival order with the log lines (e.g. the dormant-GPU notes).
    pub fn note(&mut self, line: String) {
        self.console_push(line);
    }

    /// The server answers ready (probe, or the log's markers when probes are off).
    pub fn is_ready(&self) -> bool {
        self.health() == Health::Ready
    }

    /// Start (epoch s) of the request or job in flight, if the server is busy.
    pub fn busy_since(&self, now: f64) -> Option<f64> {
        self.backend.busy_since(now)
    }

    fn console_push(&mut self, s: String) {
        self.console.push_back(s);
        while self.console.len() > CONSOLE_LEN {
            self.console.pop_front();
        }
    }

    pub fn apply_health(&mut self, at: f64, h: Health) {
        self.health_probe = Some((at, h));
        self.backend.on_health(at, h);
    }

    /// A TCP listener check (true = listening): the same as a health result Ready / Down.
    pub fn apply_listening(&mut self, at: f64, listening: bool) {
        self.apply_health(at, if listening { Health::Ready } else { Health::Down });
    }

    pub fn apply_models(&mut self, m: ModelsInfo) {
        self.backend.apply(klif_common::now_s(), ProbeResult::Models(m));
    }

    /// Feed a `/slots` answer (llama.cpp).
    pub fn apply_slots(&mut self, slots: &[SlotSample]) {
        self.backend.apply(klif_common::now_s(), ProbeResult::Slots(slots.to_vec()));
    }

    /// Any probe answer for the adapter.
    pub fn apply_probe(&mut self, at: f64, r: ProbeResult) {
        self.backend.apply(at, r);
    }

    /// Whether `/v1/models` answered (the adapter stops asking).
    pub fn has_models(&self) -> bool {
        !self.backend.probe_wants(Health::Ready).models
    }

    // --------------------------------------------------------------------------------- signals

    fn health(&self) -> Health {
        let probe = if self.probes_enabled { self.health_probe.map(|(_, h)| h) } else { None };
        self.backend.health(probe)
    }

    /// The latest raw probe health (None before the first probe).
    pub fn probed_health(&self) -> Option<Health> {
        self.health_probe.map(|(_, h)| h)
    }

    /// VRAM composition from the log (llama-server at `-lv 4`).
    pub fn layers_from_logs(&self) -> Option<Vec<VramLayer>> {
        self.backend.layers_from_logs()
    }

    pub fn signals(&self, now: f64) -> SessionSignals {
        let health = self.health();
        let (load_steps, load_fraction) = self.backend.steps_view(health);
        let mut sig = SessionSignals {
            health,
            load_steps,
            load_fraction,
            layers_from_logs: self.layers_from_logs(),
            llm: None,
            image: None,
            generic: None,
            busy: self.busy_since(now).is_some(),
            error_tail: Vec::new(),
            fatal_hint: None,
            starter_exit: None,
            median_decode_tps: None,
            arch: None,
            backend_build: self.backend.backend_build(),
            device_mismatch: backend::device_mismatch(&self.backend.reported_devices(), self.spec.expect_device.as_deref()),
            console: self.console(),
            // Filled from the per-process measurements (Telemetry::snapshot).
            spill_mib: 0.0,
            layers: Vec::new(),
            resident_gib: 0.0,
            committed_gib: 0.0,
            per_gpu: Vec::new(),
        };
        self.backend.fill(now, &mut sig);
        sig
    }

    /// Console ring plus the lines being drawn right now (a live progress bar), newest last.
    pub fn console(&self) -> Vec<String> {
        let mut v: Vec<String> = self.console.iter().cloned().collect();
        let live = [self.err.as_ref().and_then(|t| t.live_line()), self.out.as_ref().and_then(|t| t.live_line())];
        for l in live.into_iter().flatten() {
            v.push(redact(l));
        }
        let n = v.len();
        if n > CONSOLE_LEN {
            v.drain(..n - CONSOLE_LEN);
        }
        v
    }

    /// Median decode speed over finished requests (for a last-session summary).
    pub fn median_decode_tps(&self) -> Option<f64> {
        self.backend.median_decode_tps()
    }

    /// The health check the preset configured (for diagnostics).
    pub fn configured_health(&self) -> &HealthCheck {
        &self.spec.health
    }
}
