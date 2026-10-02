//! The KLIF view model, mirrored 1:1 from `app/ui/src/lib/model/types.ts` (the source of truth).
//! Serialized with camelCase field names; optional fields are omitted when `None`.
//! If you change anything here, change types.ts in the same commit (and vice versa).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotId {
    High,
    Medium,
    Low,
    Krea,
}

impl SlotId {
    pub const ALL: [SlotId; 4] = [SlotId::High, SlotId::Medium, SlotId::Low, SlotId::Krea];
    pub fn as_str(self) -> &'static str {
        match self {
            SlotId::High => "high",
            SlotId::Medium => "medium",
            SlotId::Low => "low",
            SlotId::Krea => "krea",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SlotId::High => "AGENT HIGH",
            SlotId::Medium => "AGENT MEDIUM",
            SlotId::Low => "AGENT LOW",
            SlotId::Krea => "KREA",
        }
    }
    pub fn kind(self) -> SlotKind {
        if self == SlotId::Krea { SlotKind::Image } else { SlotKind::Llm }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotKind {
    Llm,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Ready,
    Unsupported,
    ScriptMissing,
    ModelMissing,
    BuildRequired,
    Busy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backend {
    #[default]
    #[serde(rename = "HIP")]
    Hip,
    #[serde(rename = "Vulkan")]
    Vulkan,
    #[serde(rename = "CPU")]
    Cpu,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub name: String,
    pub quant: String,
    pub engine: String,
    pub backend: Backend,
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
    #[serde(rename = "weightsGiB", skip_serializing_if = "Option::is_none")]
    pub weights_gib: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: SlotId,
    pub label: String,
    pub kind: SlotKind,
    pub model: ModelRef,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_vram: Option<Vec<VramLayer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<Recipe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<RecipeOptions>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub card_id: String,
    pub backend: Backend,
    pub hardware: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv_type: Option<String>,
    #[serde(rename = "promptCacheMiB", skip_serializing_if = "Option::is_none")]
    pub prompt_cache_mib: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Krea speed/quality level (image slot, fast Krea starter only). See types.ts Recipe.precision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<Precision>,
    /// Krea identity-edit mode (image slot, fast Krea starter only). See types.ts Recipe.edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<bool>,
}

/// types.ts `Precision`: the three Krea speed/quality levels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Precision {
    #[default]
    Low,
    Medium,
    High,
}

impl Precision {
    pub const ALL: [Precision; 3] = [Precision::Low, Precision::Medium, Precision::High];
    /// "Low" / "Medium" / "High": the display label and the fast starter's `-Precision` value.
    pub fn label(self) -> &'static str {
        match self {
            Precision::Low => "Low",
            Precision::Medium => "Medium",
            Precision::High => "High",
        }
    }
}

/// `Partial<Recipe>` from the UI: every field optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipePatch {
    pub card_id: Option<String>,
    pub backend: Option<Backend>,
    pub hardware: Option<String>,
    pub ctx_tokens: Option<u32>,
    pub image_size: Option<String>,
    pub kv_type: Option<String>,
    #[serde(rename = "promptCacheMiB")]
    pub prompt_cache_mib: Option<u32>,
    pub port: Option<u16>,
    pub vision: Option<bool>,
    pub mode: Option<String>,
    pub precision: Option<Precision>,
    pub edit: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeChoice<T> {
    pub value: T,
    pub label: String,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardChoice {
    pub value: String,
    pub label: String,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub name: String,
    pub quant: String,
}

/// types.ts `RecipeChoice<Precision> & { hint: string }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecisionChoice {
    pub value: Precision,
    pub label: String,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// One short line, e.g. "fastest · ~19 s edit 1024×768".
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeOptions {
    pub cards: Vec<CardChoice>,
    pub backends: Vec<RecipeChoice<Backend>>,
    pub hardware: Vec<RecipeChoice<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<Vec<RecipeChoice<u32>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sizes: Option<Vec<RecipeChoice<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv_types: Option<Vec<String>>,
    #[serde(rename = "promptCacheMiB", skip_serializing_if = "Option::is_none")]
    pub prompt_cache_mib: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<String>>,
    /// Krea precision levels (fast Krea starter only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precisions: Option<Vec<PrecisionChoice>>,
    /// True when the current card takes the Edit toggle (fast Krea starter only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_toggle: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Starting,
    Loading,
    Live,
    Stopping,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadStepId {
    Process,
    Device,
    Weights,
    Kv,
    Warmup,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    Done,
    Active,
    Pending,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadStep {
    pub id: LoadStepId,
    pub label: String,
    pub state: StepState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadProgress {
    pub steps: Vec<LoadStep>,
    pub fraction: f64,
    pub elapsed_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fault {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code_hex: Option<String>,
    pub log_tail: Vec<String>,
    pub since_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<LoadStep>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    Stopped,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSession {
    pub slot: SlotId,
    pub model: ModelRef,
    pub uptime_s: f64,
    pub ended_ago_s: f64,
    pub ended: Ended,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub id: u64,
    pub at: f64,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub prefill_s: f64,
    pub generated_tokens: u64,
    pub decode_s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmActivity {
    Idle,
    Prefill,
    Decode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefill {
    pub tokens: u64,
    pub done_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    pub tps: f64,
    pub elapsed_s: f64,
    pub eta_s: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextFill {
    pub used_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub acceptance_pct: f64,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub generated_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmLive {
    pub activity: LlmActivity,
    pub decode_tps: f64,
    pub decode_history: Vec<f64>,
    pub prefill: Option<Prefill>,
    pub generated_tokens: u64,
    pub context: ContextFill,
    pub spec: Option<Spec>,
    pub requests: Vec<RequestRecord>,
    pub totals: Totals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageJob {
    pub at: f64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub edit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageActivity {
    Idle,
    Generating,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageLive {
    pub activity: ImageActivity,
    pub step: u32,
    pub steps: u32,
    pub s_per_it: f64,
    pub elapsed_s: f64,
    pub width: u32,
    pub height: u32,
    pub edit: bool,
    pub recent: Vec<ImageJob>,
    pub images_this_session: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub slot: SlotId,
    pub model: ModelRef,
    pub phase: Phase,
    pub uptime_s: f64,
    pub endpoint: Endpoint,
    pub api_key_set: bool,
    pub loading: Option<LoadProgress>,
    pub fault: Option<Fault>,
    pub llm: Option<LlmLive>,
    pub image: Option<ImageLive>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VramLayerId {
    Weights,
    Kv,
    Buffers,
    Draft,
    Projector,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VramLayer {
    pub id: VramLayerId,
    pub label: String,
    pub gib: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMemory {
    pub device: String,
    #[serde(rename = "totalGiB")]
    pub total_gib: f64,
    #[serde(rename = "usedGiB")]
    pub used_gib: f64,
    pub layers: Vec<VramLayer>,
    #[serde(rename = "spillMiB")]
    pub spill_mib: f64,
    pub history: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_history: Option<BTreeMap<VramLayerId, Vec<f64>>>,
    #[serde(rename = "baselineGiB")]
    pub baseline_gib: f64,
    #[serde(rename = "warnBelowGiB")]
    pub warn_below_gib: f64,
    /// The inference GPU powered down (or paged the session out) while a model is loaded. While set,
    /// `layers` are the session's ALLOCATIONS (they may sum to more than `used_gib`, which stays the
    /// resident amount). See types.ts GpuMemory.dormant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant: Option<Dormant>,
}

/// types.ts GpuMemory.dormant. Explicit renames for the "GiB" fields (camelCase would give "Gib").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dormant {
    #[serde(rename = "pagedOutGiB")]
    pub paged_out_gib: f64,
    pub since_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    #[serde(rename = "ramUsedGiB")]
    pub ram_used_gib: f64,
    #[serde(rename = "ramTotalGiB")]
    pub ram_total_gib: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_type: Option<String>,
    pub cpu_name: String,
    pub cpu_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Browser,
    Tauri,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    pub kind: HostKind,
    pub frameless: bool,
    pub maximized: bool,
    pub app_version: String,
    #[serde(default)]
    pub panel: PanelInfo,
}

/// Panel mode: the read-only mini layout on the small status screen (see types.ts HostInfo.panel).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelInfo {
    pub available: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewModel {
    pub now: f64,
    pub slots: Vec<Slot>,
    pub selected: SlotId,
    pub session: Option<Session>,
    pub vram: GpuMemory,
    pub system: SystemStats,
    pub last_session: Option<LastSession>,
    pub host: HostInfo,
    pub console: Vec<String>,
}

/// Actions the engine executes. Shell-only actions (open/copy endpoint, copy key, console, tune,
/// window chrome) are handled by the host and never reach the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    Select { slot: SlotId },
    Launch { slot: Option<SlotId> },
    Stop,
    Restart,
    Dismiss,
    SetRecipe { slot: SlotId, patch: RecipePatch },
}
