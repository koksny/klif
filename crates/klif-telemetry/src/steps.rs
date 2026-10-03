//! Load steps shared by every adapter: the six step ids, their labels and weights, the `Steps` bookkeeping the
//! log parsers advance, and the initial steps the engine shows before the first log line.
//! Owner: package D (moved out of llama.rs so adapters other than llama.cpp use it too).

use klif_common::vm::{AdapterId, LoadStep, LoadStepId, StepState, SystemKind};

/// Load step bookkeeping shared with the sd parser.
#[derive(Debug, Clone)]
pub struct Steps {
    pub state: [StepState; 6],
    pub detail: [Option<String>; 6],
}

pub const STEP_IDS: [LoadStepId; 6] =
    [LoadStepId::Process, LoadStepId::Device, LoadStepId::Weights, LoadStepId::Kv, LoadStepId::Warmup, LoadStepId::Ready];
/// Weight of each step in the overall load fraction.
const STEP_WEIGHT: [f64; 6] = [0.05, 0.05, 0.55, 0.15, 0.15, 0.05];

impl Default for Steps {
    fn default() -> Self {
        let mut s = Steps { state: [StepState::Pending; 6], detail: Default::default() };
        s.state[0] = StepState::Active;
        s
    }
}

impl Steps {
    /// Mark `idx` active and everything before it done (steps only move forward).
    pub fn reach(&mut self, idx: usize) {
        for i in 0..idx {
            if self.state[i] != StepState::Failed {
                self.state[i] = StepState::Done;
            }
        }
        if self.state[idx] == StepState::Pending {
            self.state[idx] = StepState::Active;
        }
    }
    pub fn finish(&mut self, idx: usize) {
        self.reach(idx);
        self.state[idx] = StepState::Done;
    }
    pub fn fail_active(&mut self) {
        let idx = self.state.iter().rposition(|s| *s == StepState::Active).or_else(|| {
            self.state.iter().position(|s| *s == StepState::Pending)
        });
        if let Some(i) = idx {
            self.state[i] = StepState::Failed;
        }
    }
    pub fn fraction(&self) -> f64 {
        let f: f64 = self
            .state
            .iter()
            .zip(STEP_WEIGHT)
            .map(|(s, w)| match s {
                StepState::Done => w,
                StepState::Active => w * 0.5,
                _ => 0.0,
            })
            .sum();
        f.clamp(0.0, 1.0)
    }
}

pub fn step_label(id: LoadStepId, image: bool) -> &'static str {
    match id {
        LoadStepId::Process => "Start process",
        LoadStepId::Device => "Find device",
        LoadStepId::Weights => {
            if image { "Load diffusion weights" } else { "Load weights" }
        }
        LoadStepId::Kv => {
            if image { "Load text encoder and VAE" } else { "Allocate KV cache" }
        }
        LoadStepId::Warmup => "Warm up",
        LoadStepId::Ready => "Ready",
    }
}

/// Load steps of adapters without a load-log parser (vLLM, OpenAI-compatible, generic): "Start process" ->
/// "Load model" -> "Ready" (ids Process, Weights, Ready).
pub const SHORT_STEP_IDS: [LoadStepId; 3] = [LoadStepId::Process, LoadStepId::Weights, LoadStepId::Ready];
/// Weight of each short step in the load fraction.
pub const SHORT_STEP_WEIGHT: [f64; 3] = [0.1, 0.8, 0.1];

/// The adapter parses its server's load log into the six steps (llama.cpp, sd.cpp).
pub fn has_log_steps(adapter: AdapterId) -> bool {
    matches!(adapter, AdapterId::LlamaCpp | AdapterId::SdCpp)
}

/// Label of a short step ("Load model" instead of "Load weights").
pub fn short_step_label(id: LoadStepId) -> &'static str {
    match id {
        LoadStepId::Weights => "Load model",
        other => step_label(other, false),
    }
}

/// The steps of a session that has just been spawned: "Start process" active, the rest pending. llama.cpp and
/// sd.cpp get their six log-parsed steps (sd.cpp with the diffusion wording, whatever its kind); vLLM,
/// OpenAI-compatible and generic servers get "Start process -> Load model -> Ready" for every kind.
pub fn initial_steps(adapter: AdapterId, kind: SystemKind) -> Vec<LoadStep> {
    let pending = |i: usize| if i == 0 { StepState::Active } else { StepState::Pending };
    if !has_log_steps(adapter) {
        return SHORT_STEP_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| LoadStep { id: *id, label: short_step_label(*id).to_string(), state: pending(i), detail: None })
            .collect();
    }
    let _ = kind;
    let image = adapter == AdapterId::SdCpp;
    STEP_IDS
        .iter()
        .enumerate()
        .map(|(i, id)| LoadStep { id: *id, label: step_label(*id, image).to_string(), state: pending(i), detail: None })
        .collect()
}

/// The three short steps from a six-step `Steps` (indices 0, 2, 5) with their own weights.
pub fn short_view(steps: &Steps) -> (Vec<LoadStep>, f64) {
    let idx = [0usize, 2, 5];
    let out = SHORT_STEP_IDS
        .iter()
        .zip(idx)
        .map(|(id, i)| LoadStep {
            id: *id,
            label: short_step_label(*id).to_string(),
            state: steps.state[i],
            detail: if steps.state[i] == StepState::Pending { None } else { steps.detail[i].clone() },
        })
        .collect();
    let f: f64 = idx
        .iter()
        .zip(SHORT_STEP_WEIGHT)
        .map(|(i, w)| match steps.state[*i] {
            StepState::Done => w,
            StepState::Active => w * 0.5,
            _ => 0.0,
        })
        .sum();
    (out, f.clamp(0.0, 1.0))
}
