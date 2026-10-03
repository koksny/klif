//! llama-server log parser: load steps, VRAM composition, per-request prefill/decode/cache/spec
//! signals and fault lines. Regexes are the tested catalogue (`L*`, `R*`, `B*`, `P*` ids) with the
//! `{TS}` prefix parsed by hand. Every timestamped line is `M.SS.mmm.uuu L ` since process start.
//!
//! Time model: a line's wall time is `anchor + ts`, where `anchor` (process start, epoch seconds) is
//! the smallest `arrival - ts` seen (or fixed by the caller for an adopted session read from start).

use crate::text::{file_name, redact, round_to, MIB};
use klif_common::vm::{ModelArch, RequestRecord, StepState, Totals, VramLayer, VramLayerId};
use regex::Regex;
use std::collections::VecDeque;
use std::sync::LazyLock;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect($pat));
    };
}

// ---- load (message part, after the timestamp prefix) ----
re!(L02, r"^cmn\s+common_param: common_params_print_info: build (\d+) \(([0-9a-f]+)\)(?: with (.+?))? for (.+)$");
re!(L03, r"^cmn\s+common_param: common_params_print_info: verbosity = (\d+)");
re!(L04, r"^srv\s+llama_server: initializing \.\.\.$");
re!(L10, r"^cmn\s+common_param:\s+- (\S+)\s*: (.+?)\s*\((\d+) MiB, (\d+) MiB free\)$");
re!(L11, r"^llama_prepare_model_devices: using device (\S+) \((.+?)\) \((.+?)\) - (\d+) MiB free$");
re!(L12, r"^srv\s+load_model: loading model '(.+)'$");
re!(L13, r"^print_info: file size\s+= ([\d.]+) (GiB|MiB) \(([\d.]+) BPW\)");
re!(L14, r"^load_tensors: loading model tensors, this can take a while\.\.\. \(load_mode = (\S+)\)");
re!(L15, r"^load_tensors: offloaded (\d+)/(\d+) layers to GPU");
re!(L16, r"^load_tensors:\s+(\S+) model buffer size =\s+([\d.]+) MiB");
re!(L17, r"^llama_context: constructing llama_context");
re!(L18, r"^llama_context: n_ctx\s+= (\d+)");
re!(L21, r"^llama_kv_cache:\s+(\S+) KV buffer size =\s+([\d.]+) MiB");
re!(L22, r"^llama_kv_cache: size =\s+([\d.]+) MiB \(\s*(\d+) cells,\s+(\d+) layers,\s+(\d+)/(\d+) seqs\), K \((\w+)\):\s+([\d.]+) MiB, V \((\w+)\):\s+([\d.]+) MiB");
re!(L23, r"^llama_memory_recurrent:\s+(\S+) RS buffer size =\s+([\d.]+) MiB");
re!(L24, r"^sched_reserve:\s+(\S+) compute buffer size =\s+([\d.]+) MiB");
re!(L26, r"^sched_reserve: reserve took ([\d.]+) ms");
re!(L27, r"^cmn\s+init: llama threadpool init, n_threads = (\d+)");
re!(L28, r"^cmn\s+common_init_: warming up the model with an empty run");
re!(L29, r"^common_speculative_init_result: (loading draft model|creating MTP draft context against the target model) '(.+)'$");
re!(L31, r"^srv\s+load_model: loaded multimodal model, '(.+)'$");
re!(L32, r"^srv\s+load_model: initializing, n_slots = (\d+), n_ctx_slot = (\d+), kv_unified = '(\w+)'");
re!(L33, r"^spec common_specu: adding speculative implementation '(\S+)'");
re!(L38, r"^srv\s+llama_server: model loaded$");
re!(L39, r"^srv\s+llama_server: listening on (https?://\S+)$");
re!(L50, r"^srv\s+llama_server: exiting due to model loading error");
re!(L51, r"^srv\s+load_model: failed to load draft model, '(.+)'");
re!(L53, r"^(.+):(\d+): GGML_ASSERT\((.+)\) failed$");
re!(L54, r"^ggml_vulkan: Failed to allocate pinned memory \((.+)\)");
re!(L61, r"^srv\s+init: init: chat template, thinking = (\d)");
// model shape (`print_info` block and the GGUF kv dump; -lv 4)
re!(L62, r"^print_info: (n_layer|n_expert|n_expert_used|n_head|n_head_kv|n_embd|n_vocab)\s+= (\d+)\s*$");
re!(L63, r"^print_info: model params\s+= (.+?)\s*$");
re!(L64, r"^llama_model_loader: - kv\s+\d+:\s+\S+\.expert_shared_(feed_forward_length|count) u32\s+= (\d+)");
// ---- per request ----
re!(R01, r"^slot get_availabl: id\s+(\d+) \| task -1 \| selected slot by LRU, t_last = (-?\d+)");
re!(R02, r"^slot get_availabl: id\s+(\d+) \| task -1 \| selected slot by LCP similarity, f_sim_best = ([\d.]+) \(> ([\d.]+) thold\), f_keep = ([\d.]+)");
re!(R03, r"^slot launch_slot_:\s+id\s+(\d+) \| task (\d+) \| processing task, is_child = (\d)");
re!(R04, r"^slot\s+operator\s*\(\):\s+id\s+(\d+) \| task (\d+) \| new prompt, n_ctx_slot = (\d+), n_keep = (\d+), task.n_tokens = (\d+)");
re!(R05, r"^slot\s+operator\s*\(\):\s+id\s+(\d+) \| task (\d+) \| cached n_tokens = (\d+), memory_seq_rm \[(\d+), end\)");
re!(R06, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \| prompt processing, n_tokens =\s+(\d+), progress = ([\d.]+), t =\s+([\d.]+) s / ([\d.]+) tokens per second");
re!(R07, r"^slot init_sampler:\s+id\s+(\d+) \| task (\d+) \| init sampler, took ([\d.]+) ms, tokens: text = (\d+), total = (\d+)");
re!(R08, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \| n_gen =\s+(\d+), tg =\s+([\d.]+) t/s, tg_3s =\s+([\d.]+) t/s");
re!(R09, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \| prompt eval time =\s+([\d.]+) ms /\s+(\d+) tokens \(\s*([\d.]+) ms per token,\s+([\d.]+) tokens per second\)");
re!(R10, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \|\s+eval time =\s+([\d.]+) ms /\s+(\d+) tokens \(\s*([\d.]+) ms per token,\s+([\d.]+) tokens per second\)");
re!(R11, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \|\s+total time =\s+([\d.]+) ms /\s+(\d+) tokens");
re!(R13, r"^slot print_timing:\s+id\s+(\d+) \| task (\d+) \| draft acceptance = ([\d.]+) \(\s*(\d+) accepted /\s*(\d+) generated\), mean len =\s*([\d.]+)");
re!(R15, r"^slot\s+release:\s+id\s+(\d+) \| task (\d+) \| stop processing: n_tokens = (\d+), truncated = (\d)");
re!(R16, r"^srv\s+stop: cancel task, id_task = (\d+)");
re!(R18, r"^spec\s+begin: ngram_mod occupancy");
re!(R20, r"^slot\s+operator\s*\(\):\s+id\s+(\d+) \| task (\d+) \| restored context checkpoint \(pos_min = (\d+), pos_max = (\d+), n_tokens = (\d+), n_past = (\d+), size = ([\d.]+) MiB\)");
re!(R24, r"^find_slot: non-consecutive token position");
// ---- starter banner (stdout) and PowerShell starter failure ----
re!(B01, r"^Starting (.+) on (http://\S+)$");
re!(B05, r"^Batch: (\d+), micro-batch: (\d+), (?:spec|MTP): (\w+), draft tokens: (\d+)(.*)$");
re!(BSPEC, r"^Speculation: (\S+)");
re!(P01, r"^(\S+\.exe) : (.+)$");
re!(P02, r"^\s+\+ FullyQualifiedErrorId : NativeCommandError\s*$");
re!(P03, r"^At (.+\.ps1):(\d+) char:(\d+)$");

/// Parse the `M.SS.mmm.uuu L ` prefix. Returns (seconds since process start, level, message).
pub fn split_ts(line: &str) -> Option<(f64, char, &str)> {
    let b = line.as_bytes();
    let mut i = 0;
    let mut mins: u64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        mins = mins * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    if i == 0 || i > 6 {
        return None;
    }
    let mut parts = [0u64; 3];
    for (k, width) in [2usize, 3, 3].iter().enumerate() {
        if b.get(i) != Some(&b'.') {
            return None;
        }
        i += 1;
        let mut v = 0u64;
        for _ in 0..*width {
            let c = *b.get(i)?;
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + (c - b'0') as u64;
            i += 1;
        }
        parts[k] = v;
    }
    if b.get(i) != Some(&b' ') {
        return None;
    }
    let lvl = *b.get(i + 1)? as char;
    if !matches!(lvl, 'I' | 'W' | 'E' | 'D') || b.get(i + 2) != Some(&b' ') {
        return None;
    }
    let ts = mins as f64 * 60.0 + parts[0] as f64 + parts[1] as f64 / 1e3 + parts[2] as f64 / 1e6;
    Some((ts, lvl, &line[i + 3..]))
}

fn cap_u64(c: &regex::Captures, i: usize) -> u64 {
    c.get(i).and_then(|m| m.as_str().parse().ok()).unwrap_or(0)
}
fn cap_f64(c: &regex::Captures, i: usize) -> f64 {
    c.get(i).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0)
}

/// One `prompt processing` tick (R06): printed once per batch after 3 s of prefill.
#[derive(Debug, Clone, Copy, Default)]
pub struct PpTick {
    /// Wall time of the line (epoch s).
    pub at: f64,
    /// Prompt tokens processed so far (excluding the cached prefix).
    pub n: u64,
    /// (C + n) / T, two decimals.
    pub progress: f64,
    /// Server time since prefill start (s).
    pub t: f64,
    /// Average rate since prefill start (n / t).
    pub avg_tps: f64,
}

/// A request as seen in the log (and refined by /slots).
#[derive(Debug, Clone, Default)]
pub struct Req {
    pub task: u64,
    /// Prefill start (epoch s).
    pub start_t: f64,
    /// Whole prompt T (R04, lv4 only).
    pub prompt_total: Option<u64>,
    /// Cached prefix C (R05 first line / R20 restore / /slots / end-of-request identity).
    pub cached: Option<u64>,
    /// T and C are exact (lv4 or /slots), not estimated from progress ticks.
    pub exact: bool,
    /// f_sim_best of the slot selection (R02), 0 for an LRU pick.
    pub f_sim: Option<f64>,
    /// The cached prefix is known to be exact (empty slot, R05, /slots), even when T is not.
    pub cached_exact: bool,
    /// R06 ticks.
    pub pp_ticks: Vec<PpTick>,
    /// /slots processed count (epoch s, n).
    pub slots_pp: Option<(f64, u64)>,
    /// Transition to decode (R07 / R18 / first R08 / /slots n_decoded > 0).
    pub decode_start_t: Option<f64>,
    /// R08 ticks: (epoch s, n_gen, tg, tg_3s).
    pub tg_ticks: Vec<(f64, u64, f64, f64)>,
    /// /slots n_decoded (epoch s, n).
    pub slots_gen: Option<(f64, u64)>,
    /// R09: (ms, P tokens, tps).
    pub pp_final: Option<(f64, u64, f64)>,
    /// R10: (ms, n_gen, tps).
    pub tg_final: Option<(f64, u64, f64)>,
    /// R11 total tokens (P + n_gen).
    pub total_tokens: Option<u64>,
    /// R13 acceptance ratio.
    pub accept: Option<f64>,
    /// R15 n_tokens (context use at the end).
    pub release_n: Option<u64>,
    pub end_t: Option<f64>,
    pub cancelled: bool,
}

impl Req {
    /// Prompt tokens this request must process (T - C) and the cached prefix, as best known.
    pub fn prefill_shape(&self) -> (Option<u64>, Option<u64>) {
        if let Some((_, p, _)) = self.pp_final {
            return (Some(p), self.cached.or_else(|| self.cached_from_identity()));
        }
        if let (Some(t), Some(c)) = (self.prompt_total, self.cached) {
            return (Some(t.saturating_sub(c)), Some(c));
        }
        // lv3: estimate T and C from progress = (C + n) / T over the ticks.
        let ticks = &self.pp_ticks;
        let Some(last) = ticks.last() else { return (None, self.cached) };
        let n_last = last.n as f64;
        let known_c = if self.cached_exact { self.cached.map(|c| c as f64) } else { None };
        let (t_est, c_est) = if let Some(c) = known_c {
            // C known: least squares of progress = (C + n) / T for 1/T.
            let (sxy, sxx) = ticks.iter().fold((0.0, 0.0), |(a, b), t| {
                let x = c + t.n as f64;
                (a + x * t.progress, b + x * x)
            });
            let inv = if sxx > 0.0 { sxy / sxx } else { 0.0 };
            (if inv > 0.0 { 1.0 / inv } else { n_last }, c)
        } else {
            let lo = ticks.iter().map(|t| t.progress).fold(f64::INFINITY, f64::min);
            let span = last.progress - lo;
            if (ticks.len() >= 3 && span >= 0.045) || (ticks.len() == 2 && span >= 0.095) {
                // Least squares: progress = a + b * n  ->  T = 1/b, C = a/b.
                let k = ticks.len() as f64;
                let (sx, sy) = ticks.iter().fold((0.0, 0.0), |(x, y), t| (x + t.n as f64, y + t.progress));
                let (mx, my) = (sx / k, sy / k);
                let (mut sxx, mut sxy) = (0.0, 0.0);
                for t in ticks {
                    let dx = t.n as f64 - mx;
                    sxx += dx * dx;
                    sxy += dx * (t.progress - my);
                }
                let b = if sxx > 0.0 { sxy / sxx } else { 0.0 };
                if b > 0.0 {
                    let a = my - b * mx;
                    (1.0 / b, (a / b).max(0.0))
                } else {
                    (n_last / last.progress.max(0.01), 0.0)
                }
            } else {
                let f = self.f_sim.unwrap_or(0.0);
                if last.progress - f > 0.02 {
                    let t = n_last / (last.progress - f);
                    (t, f * t)
                } else {
                    (n_last / last.progress.max(0.01), 0.0)
                }
            }
        };
        // An estimated prefix below 1% of the prompt is rounding noise of the 2-decimal progress.
        let c_est = if c_est < 0.01 * t_est { 0.0 } else { c_est };
        let c = self.cached.map(|c| c as f64).unwrap_or(c_est);
        let todo = (t_est - c).max(n_last).round() as u64;
        (Some(todo), Some(c.round() as u64))
    }

    /// C = context use at release - (P + n_gen): the lv3 identity (exact up to one token).
    fn cached_from_identity(&self) -> Option<u64> {
        let rel = self.release_n?;
        let tot = self.total_tokens?;
        Some(rel.saturating_sub(tot))
    }

    /// Processed prompt tokens known at `now` (ticks / slots), with the time of that knowledge.
    pub fn pp_known(&self) -> Option<(f64, u64)> {
        let a = self.pp_ticks.last().map(|t| (t.at, t.n));
        match (a, self.slots_pp) {
            (Some(a), Some(b)) => Some(if b.1 >= a.1 { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    /// Current prefill rate (tokens/s) over the last ~4 s of server time (at least one batch), else
    /// the server's own average. A window keeps a tiny batch (an image chunk) from swinging the ETA.
    pub fn pp_rate(&self) -> Option<f64> {
        let t = &self.pp_ticks;
        let last = t.last()?;
        if t.len() >= 2 {
            let reference = t[..t.len() - 1].iter().rev().find(|x| last.t - x.t >= 4.0).unwrap_or(&t[0]);
            let dt = last.t - reference.t;
            if dt > 0.05 && last.n > reference.n {
                return Some((last.n - reference.n) as f64 / dt);
            }
        }
        Some(last.avg_tps).filter(|v| *v > 0.0)
    }

    /// Largest token step between consecutive ticks (about one batch).
    pub fn pp_batch(&self) -> Option<u64> {
        let t = &self.pp_ticks;
        let first = t.first().map(|x| x.n);
        t.windows(2).map(|w| w[1].n.saturating_sub(w[0].n)).chain(first).max()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Prefill,
    Decode,
}

pub use crate::steps::{step_label, Steps, STEP_IDS};

/// Raw shape facts as the log names them (first model only).
#[derive(Debug, Default)]
struct Shape {
    layers: Option<u32>,
    experts: Option<u32>,
    experts_used: Option<u32>,
    heads: Option<u32>,
    kv_heads: Option<u32>,
    embd: Option<u32>,
    vocab: Option<u32>,
    params: Option<String>,
    shared_count: Option<u32>,
    shared_ff: Option<u32>,
}

#[derive(Debug)]
pub struct LlamaParser {
    anchor: Option<f64>,
    anchor_fixed: bool,
    pub verbosity: Option<u32>,
    pub build: Option<String>,
    pub steps: Steps,
    pub any_line: bool,
    // devices / composition
    devices: Vec<(String, String, u64, u64)>,
    pub primary_dev: Option<String>,
    /// The primary device's description as the server names it (L11), e.g. "AMD Radeon RX 9070 XT".
    pub primary_desc: Option<String>,
    pub primary_free_mib: Option<u64>,
    pub file_size_gib: Option<f64>,
    shape: Shape,
    ctx_index: i32,
    draft_group_open: bool,
    weights_mib: f64,
    draft_weights_mib: f64,
    kv_mib: Vec<f64>,
    compute_mib: Vec<f64>,
    pub saw_buffers: bool,
    pub n_ctx_alloc: Option<u32>,
    pub kv_type: Option<String>,
    pub n_ctx_slot: Option<u32>,
    pub spec_impls: Vec<String>,
    pub mtp: bool,
    pub draft_sidecar: bool,
    pub banner_spec: Option<String>,
    pub mmproj: bool,
    pub thinking: Option<bool>,
    pub loaded: bool,
    pub listening: Option<String>,
    first_reserve_done: bool,
    // requests
    pending_fsim: Option<f64>,
    pending_empty: bool,
    pub cur: Option<Req>,
    pub last: Option<Req>,
    pub records: VecDeque<RequestRecord>,
    pub totals: Totals,
    pub finished_rates: Vec<f64>,
    pub last_pp_rate: Option<f64>,
    /// Draft acceptance of the most recent request that reported one.
    pub last_accept: Option<f64>,
    // faults
    pub fatal_hint: Option<String>,
    pub error_tail: VecDeque<String>,
    pub load_failed: bool,
    last_err_msg: Option<String>,
    nce_exe: Option<String>,
    nce_site: Option<String>,
    /// Time of the most recent request-state evidence from the log (epoch s).
    pub last_event_t: f64,
}

impl LlamaParser {
    /// `anchor`: process start (epoch s) when known up front (adopted session read from the start).
    pub fn new(anchor: Option<f64>) -> Self {
        Self {
            anchor,
            anchor_fixed: anchor.is_some(),
            verbosity: None,
            build: None,
            steps: Steps::default(),
            any_line: false,
            devices: Vec::new(),
            primary_dev: None,
            primary_desc: None,
            primary_free_mib: None,
            file_size_gib: None,
            shape: Shape::default(),
            ctx_index: -1,
            draft_group_open: false,
            weights_mib: 0.0,
            draft_weights_mib: 0.0,
            kv_mib: Vec::new(),
            compute_mib: Vec::new(),
            saw_buffers: false,
            n_ctx_alloc: None,
            kv_type: None,
            n_ctx_slot: None,
            spec_impls: Vec::new(),
            mtp: false,
            draft_sidecar: false,
            banner_spec: None,
            mmproj: false,
            thinking: None,
            loaded: false,
            listening: None,
            first_reserve_done: false,
            pending_fsim: None,
            pending_empty: false,
            cur: None,
            last: None,
            records: VecDeque::new(),
            totals: Totals::default(),
            finished_rates: Vec::new(),
            last_pp_rate: None,
            last_accept: None,
            fatal_hint: None,
            error_tail: VecDeque::new(),
            load_failed: false,
            last_err_msg: None,
            nce_exe: None,
            nce_site: None,
            last_event_t: 0.0,
        }
    }

    fn wall(&mut self, ts: f64, arrival: f64) -> f64 {
        if !self.anchor_fixed {
            let a = arrival - ts;
            self.anchor = Some(match self.anchor {
                Some(x) => x.min(a),
                None => a,
            });
        }
        self.anchor.unwrap_or(arrival - ts) + ts
    }

    fn push_error(&mut self, line: &str) {
        self.error_tail.push_back(redact(line.to_string()));
        while self.error_tail.len() > 30 {
            self.error_tail.pop_front();
        }
    }

    fn set_fatal(&mut self, hint: String) {
        if self.fatal_hint.is_none() {
            self.fatal_hint = Some(hint);
        }
        if !self.loaded && !self.load_failed {
            self.load_failed = true;
            self.steps.fail_active();
        }
    }

    /// Feed one stdout line (the starter banner).
    pub fn feed_out(&mut self, line: &str, _arrival: f64) {
        let l = line.trim_end();
        if !l.trim().is_empty() && self.steps.state[0] == StepState::Active {
            // The starter is running (its banner precedes llama-server's own log).
            self.steps.finish(0);
            self.steps.reach(1);
        }
        if let Some(c) = B05.captures(l) {
            let spec_on = &c[3] == "on";
            let ngram_on = c.get(5).map(|m| m.as_str().contains("n-gram: on")).unwrap_or(false);
            self.banner_spec = match (spec_on, ngram_on) {
                (true, true) => Some("MTP+ngram".into()),
                (true, false) => Some("MTP".into()),
                (false, true) => Some("ngram".into()),
                _ => None,
            };
        } else if let Some(c) = BSPEC.captures(l) {
            let v = c[1].to_ascii_lowercase();
            if v != "off" && v != "none" {
                self.banner_spec = Some(spec_label_from_impls(&[c[1].to_string()]));
            }
        } else if B01.is_match(l) {
            // Starter banner; the endpoint comes from the launch plan.
        }
        self.feed_raw(l);
    }

    /// Lines without a timestamp: GGML_ASSERT and PowerShell starter failures.
    fn feed_raw(&mut self, l: &str) {
        if let Some(c) = L53.captures(l) {
            self.push_error(l);
            let at = format!("{}:{}", file_name(&c[1]), &c[2]);
            self.set_fatal(format!("llama-server aborted: GGML_ASSERT({}) failed at {}", &c[3], at));
        } else if let Some(c) = P01.captures(l) {
            self.nce_exe = Some(file_name(&c[1]).to_string());
            self.push_error(l);
        } else if let Some(c) = P03.captures(l) {
            self.nce_site = Some(format!("{}:{}", file_name(&c[1]), &c[2]));
            self.push_error(l);
        } else if P02.is_match(l) {
            self.push_error(l);
            let site = self.nce_site.clone().unwrap_or_else(|| "the starter script".into());
            let exe = self.nce_exe.clone().unwrap_or_else(|| "a native command".into());
            self.set_fatal(format!("The starter script stopped on a NativeCommandError from {exe} at {site}"));
        }
    }

    /// Feed one stderr line (llama-server's log).
    pub fn feed_err(&mut self, line: &str, arrival: f64) {
        let Some((ts, lvl, msg)) = split_ts(line) else {
            self.feed_raw(line.trim_end());
            return;
        };
        let t = self.wall(ts, arrival);
        if !self.any_line {
            self.any_line = true;
            self.steps.finish(0);
            self.steps.reach(1);
        }
        if lvl == 'E' {
            self.push_error(line);
            self.last_err_msg = Some(msg.trim().to_string());
        }
        let msg = msg.trim_end();
        // Cheap dispatch on the component prefix before running regexes.
        if msg.starts_with("slot ") || msg.starts_with("srv ") || msg.starts_with("spec ") || msg.starts_with("find_slot") {
            self.request_line(msg, t);
        }
        if !self.loaded || msg.starts_with("srv") {
            self.load_line(msg, t, lvl);
        }
    }

    fn load_line(&mut self, msg: &str, _t: f64, _lvl: char) {
        if let Some(c) = L03.captures(msg) {
            self.verbosity = c[1].parse().ok();
        } else if let Some(c) = L02.captures(msg) {
            self.build = Some(c[1].to_string());
        } else if L04.is_match(msg) {
            self.steps.finish(0);
        } else if let Some(c) = L10.captures(msg) {
            let dev = c[1].to_string();
            if dev != "CPU" {
                self.devices.push((dev, c[2].to_string(), cap_u64(&c, 3), cap_u64(&c, 4)));
                self.steps.reach(1);
            }
        } else if let Some(c) = L11.captures(msg) {
            self.primary_dev = Some(c[1].to_string());
            self.primary_desc = Some(c[2].trim().to_string());
            self.primary_free_mib = Some(cap_u64(&c, 4));
            self.steps.detail[1] = Some(format!("{} · {:.1} GiB free", &c[1], cap_u64(&c, 4) as f64 / 1024.0));
            self.steps.finish(1);
            self.steps.reach(2);
        } else if L12.is_match(msg) {
            if self.steps.state[1] != StepState::Done && self.verbosity.unwrap_or(3) < 4 {
                // lv3: device lines are hidden; the load start implies the device was found.
                self.steps.finish(1);
            }
            self.steps.reach(2);
        } else if let Some(c) = L13.captures(msg) {
            let v = cap_f64(&c, 1);
            let gib = if &c[2] == "MiB" { v / 1024.0 } else { v };
            if self.file_size_gib.is_none() {
                self.file_size_gib = Some(gib);
                self.steps.detail[2] = Some(format!("{gib:.1} GiB file"));
            }
        } else if let Some(c) = L62.captures(msg) {
            // Only the first model's block: a draft model prints its own after the main context exists.
            if self.ctx_index < 0 {
                let v = cap_u64(&c, 2) as u32;
                let f = match &c[1] {
                    "n_layer" => &mut self.shape.layers,
                    "n_expert" => &mut self.shape.experts,
                    "n_expert_used" => &mut self.shape.experts_used,
                    "n_head" => &mut self.shape.heads,
                    "n_head_kv" => &mut self.shape.kv_heads,
                    "n_embd" => &mut self.shape.embd,
                    _ => &mut self.shape.vocab,
                };
                f.get_or_insert(v);
            }
        } else if let Some(c) = L63.captures(msg) {
            if self.ctx_index < 0 && self.shape.params.is_none() {
                self.shape.params = Some(c[1].to_string());
            }
        } else if let Some(c) = L64.captures(msg) {
            if self.ctx_index < 0 {
                let v = cap_u64(&c, 2) as u32;
                if &c[1] == "count" {
                    self.shape.shared_count.get_or_insert(v);
                } else {
                    self.shape.shared_ff.get_or_insert(v);
                }
            }
        } else if L14.is_match(msg) {
            self.steps.reach(2);
        } else if L15.is_match(msg) {
            // End of the tensor upload; buffer lines follow within the same millisecond.
        } else if let Some(c) = L16.captures(msg) {
            let dev = c[1].to_string();
            let mib = cap_f64(&c, 2);
            if self.is_primary(&dev) {
                self.saw_buffers = true;
                if self.draft_group_open {
                    self.draft_weights_mib += mib;
                } else {
                    self.weights_mib += mib;
                }
                let w = (self.weights_mib / 1024.0, self.file_size_gib);
                self.steps.detail[2] = Some(match w.1 {
                    Some(f) if !self.draft_group_open => format!("{:.1} GiB on GPU · {:.1} GiB file", w.0, f),
                    _ => format!("{:.1} GiB on GPU", w.0),
                });
            }
        } else if L17.is_match(msg) {
            self.ctx_index += 1;
            self.draft_group_open = false;
            let i = self.ctx_index.max(0) as usize;
            if self.kv_mib.len() <= i {
                self.kv_mib.resize(i + 1, 0.0);
                self.compute_mib.resize(i + 1, 0.0);
            }
            if i == 0 {
                self.steps.finish(2);
                self.steps.reach(3);
            }
        } else if let Some(c) = L18.captures(msg) {
            if self.ctx_index == 0 && self.n_ctx_alloc.is_none() {
                self.n_ctx_alloc = c[1].parse().ok();
                self.kv_detail();
            }
        } else if let Some(c) = L21.captures(msg).or_else(|| L23.captures(msg)) {
            if self.is_primary(&c[1]) {
                self.saw_buffers = true;
                let i = self.ctx_index.max(0) as usize;
                if self.kv_mib.len() <= i {
                    self.kv_mib.resize(i + 1, 0.0);
                    self.compute_mib.resize(i + 1, 0.0);
                }
                self.kv_mib[i] += cap_f64(&c, 2);
            }
        } else if let Some(c) = L22.captures(msg) {
            if self.ctx_index <= 0 && self.kv_type.is_none() {
                self.kv_type = Some(c[6].to_string());
                self.kv_detail();
            }
        } else if let Some(c) = L24.captures(msg) {
            if self.is_primary(&c[1]) {
                self.saw_buffers = true;
                let i = self.ctx_index.max(0) as usize;
                if self.compute_mib.len() <= i {
                    self.kv_mib.resize(i + 1, 0.0);
                    self.compute_mib.resize(i + 1, 0.0);
                }
                // A context can print its compute buffer twice: the last value replaces.
                self.compute_mib[i] = cap_f64(&c, 2);
            }
        } else if L26.is_match(msg) {
            if self.ctx_index == 0 && !self.first_reserve_done {
                self.first_reserve_done = true;
                self.steps.finish(3);
            }
        } else if L27.is_match(msg) {
            // lv3: the context is built (weights + KV done); warm-up / draft / mmproj follow.
            self.steps.finish(3);
            self.steps.reach(4);
        } else if L28.is_match(msg) {
            self.steps.reach(4);
        } else if let Some(c) = L29.captures(msg) {
            if c[1].starts_with("loading") {
                self.draft_sidecar = true;
                self.draft_group_open = true;
            } else {
                self.mtp = true;
            }
        } else if L31.is_match(msg) {
            self.mmproj = true;
        } else if let Some(c) = L32.captures(msg) {
            self.n_ctx_slot = c[2].parse().ok();
            self.kv_detail();
        } else if let Some(c) = L33.captures(msg) {
            let s = c[1].to_string();
            if !self.spec_impls.contains(&s) {
                self.spec_impls.push(s);
            }
        } else if let Some(c) = L61.captures(msg) {
            self.thinking = Some(&c[1] == "1");
        } else if L38.is_match(msg) {
            self.loaded = true;
            self.steps.finish(4);
            self.steps.reach(5);
        } else if let Some(c) = L39.captures(msg) {
            self.loaded = true;
            self.listening = Some(c[1].to_string());
            self.steps.finish(4);
            self.steps.reach(5);
        } else if let Some(c) = L51.captures(msg) {
            self.set_fatal(format!("The draft model failed to load ({})", file_name(&c[1])));
        } else if L50.is_match(msg) {
            let why = self.last_err_msg.clone().map(|m| format!(": {m}")).unwrap_or_default();
            self.set_fatal(format!("llama-server could not load the model{why}"));
        } else if let Some(c) = L54.captures(msg) {
            self.push_error(&format!("ggml_vulkan: Failed to allocate pinned memory ({})", &c[1]));
        }
    }

    fn kv_detail(&mut self) {
        let ctx = self.n_ctx_slot.or(self.n_ctx_alloc);
        let mut parts = Vec::new();
        if let Some(c) = ctx {
            parts.push(format!("{}k", (c as f64 / 1024.0).round() as u64));
        }
        if let Some(k) = &self.kv_type {
            parts.push(k.clone());
        }
        if !parts.is_empty() {
            self.steps.detail[3] = Some(parts.join(" · "));
        }
    }

    fn is_primary(&self, dev: &str) -> bool {
        if dev.starts_with("CPU") || dev.ends_with("_Host") || dev.contains("Host") {
            return false;
        }
        match &self.primary_dev {
            Some(p) => p == dev,
            // lv3/older builds without L11: the first GPU device seen in a buffer line.
            None => {
                dev.starts_with("ROCm")
                    || dev.starts_with("Vulkan")
                    || dev.starts_with("CUDA")
                    || dev.starts_with("Metal")
                    || dev.starts_with("MTL")
                    || dev.starts_with("SYCL")
            }
        }
    }

    fn request_line(&mut self, msg: &str, t: f64) {
        if R24.is_match(msg) {
            return;
        }
        if R01.is_match(msg) {
            // LRU pick: no prefix above the similarity threshold.
            self.pending_fsim = Some(0.0);
            // t_last = -1: the slot is empty, nothing can be reused.
            self.pending_empty = msg.ends_with("t_last = -1");
            return;
        }
        if let Some(c) = R02.captures(msg) {
            self.pending_fsim = Some(cap_f64(&c, 2));
            self.pending_empty = false;
            return;
        }
        if let Some(c) = R03.captures(msg) {
            let task = cap_u64(&c, 2);
            if let Some(prev) = self.cur.take() {
                // A request we never saw finish (cancelled without a release line).
                self.last = Some(prev);
            }
            let empty = std::mem::take(&mut self.pending_empty);
            self.cur = Some(Req {
                task,
                start_t: t,
                f_sim: self.pending_fsim.take(),
                cached: if empty { Some(0) } else { None },
                cached_exact: empty,
                ..Default::default()
            });
            self.last_event_t = t;
            return;
        }
        if let Some(c) = R04.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.prompt_total = Some(cap_u64(&c, 5));
            }
            if self.n_ctx_slot.is_none() {
                self.n_ctx_slot = Some(cap_u64(&c, 3) as u32);
            }
            return;
        }
        if let Some(c) = R05.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                // The first one after `new prompt` is the reused prefix; later ones are positions.
                if r.pp_ticks.is_empty() && !r.exact {
                    r.cached = Some(cap_u64(&c, 3));
                    r.cached_exact = true;
                    r.exact = r.prompt_total.is_some();
                }
            }
            return;
        }
        if let Some(c) = R20.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                if r.pp_ticks.is_empty() {
                    r.cached = Some(cap_u64(&c, 6));
                    r.cached_exact = true;
                }
            }
            return;
        }
        if let Some(c) = R06.captures(msg) {
            let task = cap_u64(&c, 2);
            if let Some(r) = self.req_mut(task) {
                r.pp_ticks.push(PpTick {
                    at: t,
                    n: cap_u64(&c, 3),
                    progress: cap_f64(&c, 4),
                    t: cap_f64(&c, 5),
                    avg_tps: cap_f64(&c, 6),
                });
                // Short prompts are dominated by fixed costs: only real batches make a good prior.
                if r.pp_ticks.last().map(|t| t.n >= 1024).unwrap_or(false) {
                    if let Some(rate) = r.pp_rate() {
                        self.last_pp_rate = Some(rate);
                    }
                }
            }
            self.last_event_t = t;
            return;
        }
        if let Some(c) = R07.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.decode_start_t.get_or_insert(t);
                if r.prompt_total.is_none() {
                    r.prompt_total = Some(cap_u64(&c, 5));
                }
            }
            self.last_event_t = t;
            return;
        }
        if R18.is_match(msg) {
            if let Some(r) = self.cur.as_mut() {
                r.decode_start_t.get_or_insert(t);
            }
            self.last_event_t = t;
            return;
        }
        if let Some(c) = R08.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.decode_start_t.get_or_insert(t);
                r.tg_ticks.push((t, cap_u64(&c, 3), cap_f64(&c, 4), cap_f64(&c, 5)));
            }
            self.last_event_t = t;
            return;
        }
        if let Some(c) = R09.captures(msg) {
            let p = cap_u64(&c, 4);
            let tps = cap_f64(&c, 6);
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.pp_final = Some((cap_f64(&c, 3), p, tps));
                r.decode_start_t.get_or_insert(r.start_t + cap_f64(&c, 3) / 1000.0);
            }
            if p >= 1024 && tps > 0.0 {
                self.last_pp_rate = Some(tps);
            }
            return;
        }
        if let Some(c) = R10.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.tg_final = Some((cap_f64(&c, 3), cap_u64(&c, 4), cap_f64(&c, 6)));
            }
            return;
        }
        if let Some(c) = R11.captures(msg) {
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.total_tokens = Some(cap_u64(&c, 4));
            }
            return;
        }
        if let Some(c) = R13.captures(msg) {
            let a = cap_f64(&c, 3);
            if let Some(r) = self.req_mut(cap_u64(&c, 2)) {
                r.accept = Some(a);
            }
            self.last_accept = Some(a);
            return;
        }
        if let Some(c) = R15.captures(msg) {
            let task = cap_u64(&c, 2);
            let n = cap_u64(&c, 3);
            if let Some(r) = self.req_mut(task) {
                r.release_n = Some(n);
                r.end_t = Some(t);
            }
            self.finish(task, t);
            self.last_event_t = t;
            return;
        }
        if let Some(c) = R16.captures(msg) {
            let task = cap_u64(&c, 1);
            if let Some(r) = self.req_mut(task) {
                r.cancelled = true;
            }
        }
        // `got exception` (R28) is an E line and already in the error tail; the GGML_ASSERT that
        // may follow is what makes it fatal.
    }

    fn req_mut(&mut self, task: u64) -> Option<&mut Req> {
        match self.cur.as_mut() {
            Some(r) if r.task == task => Some(r),
            _ => match self.last.as_mut() {
                Some(r) if r.task == task => Some(r),
                _ => None,
            },
        }
    }

    fn finish(&mut self, task: u64, t: f64) {
        let Some(r) = self.cur.take_if(|r| r.task == task) else { return };
        let (todo, cached) = r.prefill_shape();
        let p = r.pp_final.map(|x| x.1).or(todo).unwrap_or(0);
        let c = if r.exact || r.cached_exact { r.cached.unwrap_or(0) } else { r.cached_from_identity().or(cached).unwrap_or(0) };
        let gen = r.tg_final.map(|x| x.1).or(r.tg_ticks.last().map(|x| x.1)).unwrap_or(0);
        let prefill_s = r.pp_final.map(|x| x.0 / 1000.0).unwrap_or(0.0);
        let decode_s = r.tg_final.map(|x| x.0 / 1000.0).unwrap_or(0.0);
        if r.pp_final.is_some() || r.tg_final.is_some() {
            let rec = RequestRecord {
                id: r.task,
                at: t.round(),
                prompt_tokens: c + p,
                cached_tokens: c,
                prefill_s: round_to(prefill_s, 2),
                generated_tokens: gen,
                decode_s: round_to(decode_s, 2),
            };
            self.records.push_back(rec);
            while self.records.len() > 12 {
                self.records.pop_front();
            }
            self.totals.requests += 1;
            self.totals.prompt_tokens += c + p;
            self.totals.generated_tokens += gen;
            if let Some((_, n, tps)) = r.tg_final {
                if n > 0 && tps > 0.0 {
                    self.finished_rates.push(tps);
                }
            }
        }
        let mut r = r;
        if !r.exact && r.cached.is_none() {
            r.cached = Some(c);
        }
        self.last = Some(r);
    }

    /// The GPU devices the server reported: the primary device (L11) if known, else every non-CPU device of
    /// the `common_params_print_info` list (L10). Descriptions as printed, e.g. "AMD Radeon RX 9070 XT".
    pub fn gpu_devices(&self) -> Vec<String> {
        if let Some(d) = &self.primary_desc {
            return vec![d.clone()];
        }
        self.devices.iter().map(|(_, desc, _, _)| desc.trim().to_string()).filter(|d| !d.is_empty()).collect()
    }

    /// The server's build as it reports it ("b6500"), from the `common_params_print_info` banner.
    pub fn build_label(&self) -> Option<String> {
        self.build.as_ref().map(|b| format!("b{b}"))
    }

    /// Activity from the log alone.
    pub fn activity(&self) -> Activity {
        match &self.cur {
            None => Activity::Idle,
            Some(r) if r.decode_start_t.is_some() => Activity::Decode,
            Some(_) => Activity::Prefill,
        }
    }

    /// VRAM composition on the primary device from the buffer lines (lv4 only).
    /// The main model's shape, once its `print_info` block named the layer count.
    pub fn arch(&self) -> Option<ModelArch> {
        let a = &self.shape;
        let layers = a.layers.filter(|&n| n > 0)?;
        let experts = a.experts.unwrap_or(0);
        let (experts_used, shared_experts) = if experts == 0 {
            (0, 0)
        } else {
            // Qwen-style MoE names a shared expert by its FFN width; DeepSeek-style by a count.
            let shared = a.shared_count.unwrap_or(u32::from(a.shared_ff.unwrap_or(0) > 0));
            (a.experts_used.unwrap_or(0), shared)
        };
        Some(ModelArch {
            layers,
            experts,
            experts_used,
            shared_experts,
            heads: a.heads,
            kv_heads: a.kv_heads,
            embd: a.embd,
            vocab: a.vocab,
            params: a.params.clone(),
        })
    }

    pub fn layers(&self) -> Option<Vec<VramLayer>> {
        if !self.saw_buffers {
            return None;
        }
        let kv0 = self.kv_mib.first().copied().unwrap_or(0.0);
        let cmp0 = self.compute_mib.first().copied().unwrap_or(0.0);
        let draft = self.draft_weights_mib
            + self.kv_mib.iter().skip(1).sum::<f64>()
            + self.compute_mib.iter().skip(1).sum::<f64>();
        let mut out = Vec::new();
        let mut push = |id: VramLayerId, label: &str, mib: f64| {
            if mib > 0.5 {
                out.push(VramLayer { id, label: label.to_string(), gib: round_to(mib * MIB / crate::text::GIB, 3) });
            }
        };
        push(VramLayerId::Weights, "weights", self.weights_mib);
        push(VramLayerId::Kv, "KV cache", kv0);
        push(VramLayerId::Buffers, "buffers", cmp0);
        push(VramLayerId::Draft, "draft", draft);
        if out.is_empty() { None } else { Some(out) }
    }

    /// Speculation label from the log (L33 impls, L29 MTP/sidecar, banner).
    pub fn spec_label(&self) -> Option<String> {
        if !self.spec_impls.is_empty() {
            let mut impls = self.spec_impls.clone();
            if self.mtp && !impls.iter().any(|s| s.contains("mtp")) {
                impls.push("draft-mtp".into());
            }
            return Some(spec_label_from_impls(&impls));
        }
        if let Some(b) = &self.banner_spec {
            return Some(b.clone());
        }
        if self.mtp {
            return Some("MTP".into());
        }
        if self.draft_sidecar {
            return Some("draft".into());
        }
        None
    }
}

/// "MTP+ngram" style label from llama.cpp speculative implementation names.
pub fn spec_label_from_impls(impls: &[String]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut add = |p: &'static str| {
        if !parts.contains(&p) {
            parts.push(p);
        }
    };
    for i in impls {
        let l = i.to_ascii_lowercase();
        if l.contains("mtp") {
            add("MTP");
        } else if l.contains("dflash") {
            add("DFlash");
        } else if l.contains("ngram") || l.contains("n-gram") {
            add("ngram");
        } else if l.contains("draft") || l.contains("eagle") {
            add("draft");
        }
    }
    // MTP first, n-gram last (the launcher's naming).
    parts.sort_by_key(|p| match *p {
        "MTP" => 0,
        "DFlash" => 1,
        "draft" => 2,
        _ => 3,
    });
    if parts.is_empty() { impls.join("+") } else { parts.join("+") }
}
