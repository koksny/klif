//! One watched session: two log tailers, the console ring, the kind-specific parser and the latest
//! probe results, combined into `ServerSignals`. Time is always passed in (`now`, epoch seconds),
//! so the same code runs live (wall clock) and in a replay (virtual clock).

use crate::llama::{step_label, Activity, LlamaParser, Req, Steps, STEP_IDS};
use crate::probe::{ModelsInfo, SlotSample};
use crate::sd::SdParser;
use crate::tail::{ConsoleLine, LogTailer, Segment, Stream};
use crate::text::{redact, round_to};
use crate::vram::push_cap;
use crate::{Health, ServerSignals, WatchSpec};
use klif_common::vm::{
    ContextFill, LlmActivity, LlmLive, LoadStep, Prefill, SlotKind, Spec, StepState, VramLayer,
};
use std::collections::VecDeque;

pub const CONSOLE_LEN: usize = 200;
/// A /slots sample older than this is not used for live values.
const SLOTS_FRESH_S: f64 = 2.5;
/// Display smoothing of the decode speed.
const DECODE_TAU_S: f64 = 1.5;

enum Parser {
    Llama(Box<LlamaParser>),
    Sd(Box<SdParser>),
}

pub struct SessionTracker {
    spec: WatchSpec,
    out: LogTailer,
    err: LogTailer,
    console: VecDeque<String>,
    noise_last: f64,
    noise_hidden: u64,
    parser: Parser,
    probes_enabled: bool,
    health_probe: Option<(f64, Health)>,
    listening_probe: Option<(f64, bool)>,
    slots: Option<SlotSample>,
    prev_slots: Option<SlotSample>,
    slots_ema: Option<f64>,
    models: Option<ModelsInfo>,
    decode_tps: f64,
    decode_hist: VecDeque<f64>,
    hist_sec: Option<i64>,
    last_poll: Option<f64>,
    finished_seen: Option<u64>,
}

impl SessionTracker {
    /// `from_start`: read the logs from the beginning (adopted session) instead of their current end.
    pub fn new(spec: WatchSpec, from_start: bool) -> Self {
        let out = LogTailer::new(&spec.out_log, Stream::Out, from_start);
        let err = LogTailer::new(&spec.err_log, Stream::Err, from_start);
        let parser = match spec.kind {
            SlotKind::Llm => {
                let anchor = if from_start && spec.started_at > 0.0 { Some(spec.started_at) } else { None };
                Parser::Llama(Box::new(LlamaParser::new(anchor)))
            }
            SlotKind::Image => Parser::Sd(Box::default()),
        };
        Self {
            spec,
            out,
            err,
            console: VecDeque::new(),
            noise_last: f64::NEG_INFINITY,
            noise_hidden: 0,
            parser,
            probes_enabled: true,
            health_probe: None,
            listening_probe: None,
            slots: None,
            prev_slots: None,
            slots_ema: None,
            models: None,
            decode_tps: 0.0,
            decode_hist: VecDeque::new(),
            hist_sec: None,
            last_poll: None,
            finished_seen: None,
        }
    }

    pub fn spec(&self) -> &WatchSpec {
        &self.spec
    }

    /// Replays run without HTTP probes: health then comes from the log markers alone.
    pub fn set_probes_enabled(&mut self, on: bool) {
        self.probes_enabled = on;
    }

    pub fn llama(&self) -> Option<&LlamaParser> {
        match &self.parser {
            Parser::Llama(p) => Some(p),
            _ => None,
        }
    }

    pub fn sd(&self) -> Option<&SdParser> {
        match &self.parser {
            Parser::Sd(p) => Some(p),
            _ => None,
        }
    }

    // ------------------------------------------------------------------------------ ingestion

    /// Read new log bytes, parse them, update smoothing and the 1 Hz decode history.
    pub fn poll(&mut self, now: f64) {
        let mut segs: Vec<Segment> = Vec::new();
        let mut lines: Vec<ConsoleLine> = Vec::new();
        // stderr first: when a crash shows on both streams in one poll, the server's own error line
        // is the cause and the starter's exit line the consequence.
        self.err.poll(now, &mut segs, &mut lines);
        self.out.poll(now, &mut segs, &mut lines);
        for s in &segs {
            match &mut self.parser {
                Parser::Llama(p) => match s.stream {
                    Stream::Err => p.feed_err(&s.text, s.at),
                    Stream::Out => p.feed_out(&s.text, s.at),
                },
                Parser::Sd(p) => p.feed(&s.text, s.at, s.stream == Stream::Err),
            }
        }
        for l in lines {
            self.push_console(l.text, now);
        }
        self.update_decode(now);
    }

    fn push_console(&mut self, text: String, now: f64) {
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

    /// Start (epoch s) of the request or image job in flight, if the server is busy.
    pub fn busy_since(&self, now: f64) -> Option<f64> {
        match &self.parser {
            Parser::Llama(p) => {
                if self.activity(now) == Activity::Idle {
                    return None;
                }
                Some(p.cur.as_ref().map(|r| r.start_t).unwrap_or(now))
            }
            Parser::Sd(p) => p.job_start(),
        }
    }

    fn console_push(&mut self, s: String) {
        self.console.push_back(s);
        while self.console.len() > CONSOLE_LEN {
            self.console.pop_front();
        }
    }

    pub fn apply_health(&mut self, at: f64, h: Health) {
        self.health_probe = Some((at, h));
    }

    pub fn apply_listening(&mut self, at: f64, listening: bool) {
        self.listening_probe = Some((at, listening));
    }

    pub fn apply_models(&mut self, m: ModelsInfo) {
        self.models = Some(m);
    }

    pub fn has_models(&self) -> bool {
        self.models.is_some()
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
        if let Parser::Llama(p) = &mut self.parser {
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
        }
        self.prev_slots = self.slots.take();
        self.slots = Some(s);
    }

    fn fresh_slots(&self, now: f64) -> Option<&SlotSample> {
        self.slots.as_ref().filter(|s| now - s.at <= SLOTS_FRESH_S)
    }

    // ------------------------------------------------------------------------------- LLM state

    fn activity(&self, now: f64) -> Activity {
        let Parser::Llama(p) = &self.parser else { return Activity::Idle };
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
        let Parser::Llama(p) = &self.parser else { return None };
        if self.activity(now) != Activity::Decode {
            return None;
        }
        if self.fresh_slots(now).is_some() {
            if let Some(e) = self.slots_ema {
                return Some(e);
            }
        }
        p.cur.as_ref().and_then(|r| r.tg_ticks.last()).map(|t| t.3)
    }

    fn update_decode(&mut self, now: f64) {
        let dt = self.last_poll.map(|t| (now - t).max(0.0)).unwrap_or(0.5);
        self.last_poll = Some(now);
        let Parser::Llama(p) = &self.parser else { return };
        // A request finished: show its final average.
        if let Some(r) = &p.last {
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
        let sec = now.floor() as i64;
        match self.hist_sec {
            None => self.hist_sec = Some(sec),
            Some(h) if sec > h => {
                let n = ((sec - h) as usize).min(crate::vram::HISTORY_LEN);
                for _ in 0..n {
                    push_cap(&mut self.decode_hist, round_to(sample, 1));
                }
                self.hist_sec = Some(sec);
            }
            _ => {}
        }
    }

    fn llm_live(&self, now: f64) -> Option<LlmLive> {
        let Parser::Llama(p) = &self.parser else { return None };
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
                Prefill {
                    tokens,
                    done_tokens: tokens,
                    cached_tokens: Some(cached),
                    tps: round_to(tps, 1),
                    elapsed_s: round_to(secs, 1),
                    eta_s: 0.0,
                }
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
            .or(self.spec.ctx_tokens.map(|v| v as u64))
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

        let mode = self.spec.spec_mode.clone().or_else(|| p.spec_label());
        let spec = mode.map(|m| {
            let acc = r.and_then(|r| r.accept).or(p.last_accept).unwrap_or(0.0);
            Spec { acceptance_pct: round_to(acc * 100.0, 1), mode: m, active: Some(act == Activity::Decode) }
        });

        Some(LlmLive {
            activity: match act {
                Activity::Idle => LlmActivity::Idle,
                Activity::Prefill => LlmActivity::Prefill,
                Activity::Decode => LlmActivity::Decode,
            },
            decode_tps: round_to(self.decode_tps, 1),
            decode_history: self.decode_hist.iter().copied().collect(),
            prefill,
            generated_tokens: generated,
            context: ContextFill { used_tokens: used.min(total.max(used)), total_tokens: total },
            spec,
            requests: p.records.iter().cloned().collect(),
            totals: p.totals.clone(),
        })
    }

    // --------------------------------------------------------------------------------- signals

    fn health(&self) -> Health {
        match &self.parser {
            Parser::Llama(p) => {
                if self.probes_enabled {
                    if let Some((_, h)) = self.health_probe {
                        return h;
                    }
                }
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
            Parser::Sd(p) => {
                if self.probes_enabled {
                    if let Some((_, true)) = self.listening_probe {
                        return Health::Ready;
                    }
                    if self.listening_probe.is_some() {
                        // No listener: still loading, unless it already listened once or crashed.
                        let gone = p.listening.is_some() || p.fatal_hint.is_some() || !p.any_line;
                        return if gone { Health::Down } else { Health::Loading };
                    }
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
        }
    }

    fn steps_view(&self, health: Health) -> (Vec<LoadStep>, f64) {
        let (mut steps, image, process_detail): (Steps, bool, Option<String>) = match &self.parser {
            Parser::Llama(p) => (p.steps.clone(), false, Some(match &p.build {
                Some(b) => format!("llama.cpp b{b}"),
                None => "llama.cpp".to_string(),
            })),
            Parser::Sd(p) => (p.steps.clone(), true, Some("sd.cpp".to_string())),
        };
        if steps.detail[0].is_none() {
            steps.detail[0] = process_detail;
        }
        let failed = steps.state.contains(&StepState::Failed);
        if health == Health::Ready && !failed {
            steps.finish(5);
            steps.detail[5] = Some(format!("{}:{}", self.spec.host, self.spec.port));
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

    /// VRAM composition from the log (llama-server at `-lv 4`).
    pub fn layers_from_logs(&self) -> Option<Vec<VramLayer>> {
        match &self.parser {
            Parser::Llama(p) => p.layers(),
            Parser::Sd(_) => None,
        }
    }

    pub fn signals(&self, now: f64) -> ServerSignals {
        let health = self.health();
        let (load_steps, load_fraction) = self.steps_view(health);
        let mut sig = ServerSignals {
            health,
            load_steps,
            load_fraction,
            layers_from_logs: self.layers_from_logs(),
            llm: None,
            image: None,
            error_tail: Vec::new(),
            fatal_hint: None,
            starter_exit: None,
            median_decode_tps: None,
            arch: None,
        };
        match &self.parser {
            Parser::Llama(p) => {
                sig.llm = self.llm_live(now);
                sig.arch = p.arch();
                sig.median_decode_tps = self.median_decode_tps();
                sig.error_tail = p.error_tail.iter().cloned().collect();
                sig.fatal_hint = p.fatal_hint.clone();
            }
            Parser::Sd(p) => {
                sig.image = Some(p.live(now));
                sig.error_tail = p.error_tail.iter().cloned().collect();
                sig.fatal_hint = p.fatal_hint.clone();
                sig.starter_exit = p.starter_exit;
            }
        }
        sig
    }

    /// Console ring plus the lines being drawn right now (a live progress bar), newest last.
    pub fn console(&self) -> Vec<String> {
        let mut v: Vec<String> = self.console.iter().cloned().collect();
        for live in [self.err.live_line(), self.out.live_line()].into_iter().flatten() {
            v.push(redact(live));
        }
        let n = v.len();
        if n > CONSOLE_LEN {
            v.drain(..n - CONSOLE_LEN);
        }
        v
    }

    /// Median decode speed over finished requests (for a last-session summary).
    pub fn median_decode_tps(&self) -> Option<f64> {
        let p = self.llama()?;
        if p.finished_rates.is_empty() {
            return None;
        }
        Some(crate::sd::median(&p.finished_rates))
    }
}
