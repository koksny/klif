//! The KLIF view model, mirrored 1:1 in `app/ui/src/lib/model/types.ts`.
//! Serialized with camelCase field names; optional fields are omitted when `None`.
//! If you change anything here, change types.ts in the same change (and vice versa).
//! Every type that klif-cli prints (or that one of them contains) also derives `schemars::JsonSchema` under the
//! `schema` feature, which is what `klif-cli schema` is built from: give a new type
//! `#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]` next to its serde derive.
//!
//! 0.3: KLIF manages N user-defined Systems (`[systems.<id>]`, tabs in file order) running concurrently, each
//! with its own session, plus Systems on remote nodes (`"<node>/<id>"`) and external servers. A System runs a
//! PRESET (`[presets.<id>]`, see `crate::config::PresetCfg`); `CommandView` shows its command exactly.
//! Renames vs 0.2: Slot -> System, SlotId -> SystemId (string), SlotKind -> SystemKind, SystemStats -> MachineStats,
//! vm.slots -> vm.systems, vm.system -> vm.machine. The 0.2 Recipe / Card / Precision types and Backend enum are gone.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::config::{NodeRight, OnConflict, PresetCfg};

// ----------------------------------------------------------------------------------------- systems

/// A System's id: a local id from `[systems.<id>]` (`s1`, `cgi`, `tts`...) or, for a System on a remote node,
/// `"<node>/<id>"`. Serialized as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SystemId(pub String);

impl SystemId {
    pub fn new(id: impl Into<String>) -> SystemId {
        SystemId(id.into())
    }

    /// `"<node>/<id>"`.
    pub fn remote(node: &str, local: &str) -> SystemId {
        SystemId(format!("{node}/{local}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The node part of a remote id (None for a local System).
    pub fn node(&self) -> Option<&str> {
        self.0.split_once('/').map(|(n, _)| n)
    }

    /// The id on its own node (`"s1"` for both `"s1"` and `"render-box/s1"`).
    pub fn local(&self) -> &str {
        self.0.split_once('/').map(|(_, l)| l).unwrap_or(&self.0)
    }

    pub fn is_remote(&self) -> bool {
        self.0.contains('/')
    }

    /// A local System / node id: `[a-z0-9][a-z0-9_-]{0,31}` (no "/"). Err = one sentence.
    pub fn validate_local(id: &str) -> Result<(), String> {
        let b = id.as_bytes();
        let ok_first = b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let ok_rest = b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-');
        if ok_first && ok_rest && b.len() <= 32 {
            Ok(())
        } else {
            Err(format!("Id \"{id}\" is not allowed: use 1-32 characters a-z, 0-9, '-' or '_', starting with a letter or digit."))
        }
    }
}

impl fmt::Display for SystemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SystemId {
    fn from(s: &str) -> SystemId {
        SystemId(s.to_string())
    }
}

impl From<String> for SystemId {
    fn from(s: String) -> SystemId {
        SystemId(s)
    }
}

impl std::borrow::Borrow<str> for SystemId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SystemId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for SystemId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for SystemId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// What a System serves. TS: 'llm'|'image'|'tts'|'stt'|'video'|'music'.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SystemKind {
    #[default]
    Llm,
    Image,
    /// Text to speech.
    Tts,
    /// Speech to text (audio in).
    Stt,
    Video,
    /// Music and song generation.
    Music,
}

impl SystemKind {
    pub const ALL: [SystemKind; 6] =
        [SystemKind::Llm, SystemKind::Image, SystemKind::Tts, SystemKind::Stt, SystemKind::Video, SystemKind::Music];

    /// The serde / klif.toml value.
    pub fn as_str(self) -> &'static str {
        match self {
            SystemKind::Llm => "llm",
            SystemKind::Image => "image",
            SystemKind::Tts => "tts",
            SystemKind::Stt => "stt",
            SystemKind::Video => "video",
            SystemKind::Music => "music",
        }
    }

    /// Display name: "LLM", "Image", "Speech (TTS)", "Transcription (STT)", "Video", "Music".
    pub fn label(self) -> &'static str {
        match self {
            SystemKind::Llm => "LLM",
            SystemKind::Image => "Image",
            SystemKind::Tts => "Speech (TTS)",
            SystemKind::Stt => "Transcription (STT)",
            SystemKind::Video => "Video",
            SystemKind::Music => "Music",
        }
    }

    /// Parse the klif.toml / CLI value (case-insensitive).
    pub fn parse(s: &str) -> Option<SystemKind> {
        SystemKind::ALL.into_iter().find(|k| k.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

impl fmt::Display for SystemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// LLM System class (recommendation grouping, default labels). TS: 'fast'|'deep'|'max'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LlmClass {
    /// System 1: fast, cheap, always on.
    Fast,
    /// System 2: slower, deeper.
    Deep,
    /// System 3: every resource.
    Max,
}

impl LlmClass {
    pub const ALL: [LlmClass; 3] = [LlmClass::Fast, LlmClass::Deep, LlmClass::Max];

    pub fn as_str(self) -> &'static str {
        match self {
            LlmClass::Fast => "fast",
            LlmClass::Deep => "deep",
            LlmClass::Max => "max",
        }
    }

    /// The default System label of the class: "System 1" / "System 2" / "System 3".
    pub fn default_label(self) -> &'static str {
        match self {
            LlmClass::Fast => "System 1",
            LlmClass::Deep => "System 2",
            LlmClass::Max => "System 3",
        }
    }

    pub fn parse(s: &str) -> Option<LlmClass> {
        LlmClass::ALL.into_iter().find(|c| c.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

/// A System's tab status. TS: 'not-set'|'invalid'|'offline'|'starting'|'online'|'busy'|'stopping'|'fault'|'unreachable'.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SystemStatus {
    /// No preset selected.
    NotSet,
    /// Preset / kind errors, exe or model missing, port held by a foreign process (`reason` says which).
    Invalid,
    /// Ready, not running (external: not answering).
    Offline,
    /// Boot / loading.
    Starting,
    /// Live and idle.
    Online,
    /// Live and working (prefill / decode / steps / requests).
    Busy,
    Stopping,
    Fault,
    #[default]
    /// Its remote node is down, unauthorized or incompatible.
    Unreachable,
}

impl SystemStatus {
    /// The kebab-case wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            SystemStatus::NotSet => "not-set",
            SystemStatus::Invalid => "invalid",
            SystemStatus::Offline => "offline",
            SystemStatus::Starting => "starting",
            SystemStatus::Online => "online",
            SystemStatus::Busy => "busy",
            SystemStatus::Stopping => "stopping",
            SystemStatus::Fault => "fault",
            SystemStatus::Unreachable => "unreachable",
        }
    }
}

// ---------------------------------------------------------------------------------------- adapters

/// The server family a preset launches; selects telemetry, defaults and managed env (SPEC section 6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AdapterId {
    #[default]
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    #[serde(rename = "sd.cpp")]
    SdCpp,
    #[serde(rename = "vllm")]
    Vllm,
    #[serde(rename = "openai")]
    OpenAi,
    /// audio.cpp `audiocpp_server` (text to speech and music; one `--config` JSON names its models).
    #[serde(rename = "audiocpp")]
    AudioCpp,
    /// Any other server (TTS/STT/video servers, ComfyUI...): kind required, port or health required.
    #[serde(rename = "generic")]
    Generic,
}

impl AdapterId {
    pub const ALL: [AdapterId; 6] =
        [AdapterId::LlamaCpp, AdapterId::SdCpp, AdapterId::Vllm, AdapterId::OpenAi, AdapterId::AudioCpp, AdapterId::Generic];

    /// The serde / klif.toml value, also the display label: "llama.cpp", "sd.cpp", "vllm", "openai", "audiocpp",
    /// "generic".
    pub fn as_str(self) -> &'static str {
        match self {
            AdapterId::LlamaCpp => "llama.cpp",
            AdapterId::SdCpp => "sd.cpp",
            AdapterId::Vllm => "vllm",
            AdapterId::OpenAi => "openai",
            AdapterId::AudioCpp => "audiocpp",
            AdapterId::Generic => "generic",
        }
    }

    /// Parse the klif.toml / CLI value (case-insensitive; also "llamacpp", "sdcpp", "openai-compatible", "audio.cpp").
    pub fn parse(s: &str) -> Option<AdapterId> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "llama.cpp" | "llamacpp" | "llama-cpp" => AdapterId::LlamaCpp,
            "sd.cpp" | "sdcpp" | "sd-cpp" => AdapterId::SdCpp,
            "vllm" => AdapterId::Vllm,
            "openai" | "openai-compatible" => AdapterId::OpenAi,
            "audiocpp" | "audio.cpp" | "audio-cpp" => AdapterId::AudioCpp,
            "generic" => AdapterId::Generic,
            _ => return None,
        })
    }

    /// Port used when neither the preset nor its args name one (generic: none; that is an error unless the
    /// preset gives `health`). audio.cpp: its own default, 8080.
    pub fn default_port(self) -> Option<u16> {
        match self {
            AdapterId::LlamaCpp => Some(7030),
            AdapterId::SdCpp => Some(1234),
            AdapterId::Vllm => Some(8000),
            AdapterId::OpenAi | AdapterId::AudioCpp => Some(8080),
            AdapterId::Generic => None,
        }
    }

    /// Kind when the preset does not say: image for sd.cpp (`kind = "video"` for sd.cpp video), llm for
    /// llama.cpp/vllm/openai, tts for audiocpp (a music preset says `kind = "music"`), none for generic (required there).
    pub fn default_kind(self) -> Option<SystemKind> {
        match self {
            AdapterId::SdCpp => Some(SystemKind::Image),
            AdapterId::LlamaCpp | AdapterId::Vllm | AdapterId::OpenAi => Some(SystemKind::Llm),
            AdapterId::AudioCpp => Some(SystemKind::Tts),
            AdapterId::Generic => None,
        }
    }

    /// The environment variable the server reads its API key from (KLIF injects the key there).
    pub fn api_key_env(self) -> Option<&'static str> {
        match self {
            AdapterId::LlamaCpp => Some("LLAMA_API_KEY"),
            AdapterId::Vllm => Some("VLLM_API_KEY"),
            AdapterId::SdCpp | AdapterId::OpenAi | AdapterId::AudioCpp | AdapterId::Generic => None,
        }
    }
}

impl fmt::Display for AdapterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How KLIF decides the server is ready. TS: `{type:'auto'} | {type:'http', path} | {type:'tcp'}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HealthCheck {
    /// The adapter's default chain (llama.cpp/vllm `/health`, sd.cpp TCP, openai `/health` → `/v1/models` → TCP).
    #[default]
    Auto,
    /// GET `path`: 200 = ready, any other answer = loading.
    Http { path: String },
    /// A TCP listener on the port = ready.
    Tcp,
}

impl HealthCheck {
    /// From a preset's `health` value: absent/empty = Auto, "tcp" = Tcp, "/path" = Http. Anything else is an error
    /// sentence (the catalog turns it into an error Issue on the `health` field).
    pub fn from_preset(value: Option<&str>) -> Result<HealthCheck, String> {
        match value.map(str::trim) {
            None | Some("") => Ok(HealthCheck::Auto),
            Some(v) if v.eq_ignore_ascii_case("tcp") => Ok(HealthCheck::Tcp),
            Some(v) if v.eq_ignore_ascii_case("auto") => Ok(HealthCheck::Auto),
            Some(v) if v.starts_with('/') => Ok(HealthCheck::Http { path: v.to_string() }),
            Some(v) => Err(format!("health must be \"/path\" (HTTP) or \"tcp\", not \"{v}\"")),
        }
    }
}

/// Availability of a preset's (or a System's) launch. TS: 'ready'|'unsupported'|'invalid'|'exe-missing'|'model-missing'|'busy'.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Ready,
    #[default]
    /// No preset selected for the System (or it names a preset that does not exist).
    Unsupported,
    /// The preset has error issues (bad placeholder, port conflict, kind mismatch, unreadable entry...).
    Invalid,
    /// The program could not be found.
    ExeMissing,
    ModelMissing,
    /// The tier's port is held by a process KLIF does not own.
    Busy,
}

// ------------------------------------------------------------------------------------------- model

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ModelRef {
    /// "Qwen 3.8 27B"
    pub name: String,
    /// "IQ3_S" ("" unknown)
    pub quant: String,
    /// Adapter label: "llama.cpp", "sd.cpp", "vllm", "openai".
    pub engine: String,
    /// Display text: "HIP", "Vulkan", "CUDA", "Metal", "CPU" or free text; "" when unknown.
    #[serde(default)]
    pub backend: String,
    /// "RX 9070 XT"
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<ModelArch>,
}

/// The model's shape from the server log (llama.cpp `print_info`), mirrored in app/ui types.ts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ModelArch {
    pub layers: u32,
    pub experts: u32,
    pub experts_used: u32,
    pub shared_experts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv_heads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embd: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocab: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
}

// ----------------------------------------------------------------------------------- command view

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum IssueLevel {
    Error,
    #[default]
    Warn,
}

/// One validation finding. Errors block Launch (Availability::Invalid); warnings never do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Issue {
    pub level: IssueLevel,
    /// The preset / config field it is about ("command", "args", "env.HIP_PATH", "port", "presets.my-id", "file").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// One plain sentence.
    pub text: String,
}

impl Issue {
    pub fn error(field: Option<&str>, text: impl Into<String>) -> Issue {
        Issue { level: IssueLevel::Error, field: field.map(str::to_string), text: text.into() }
    }
    pub fn warn(field: Option<&str>, text: impl Into<String>) -> Issue {
        Issue { level: IssueLevel::Warn, field: field.map(str::to_string), text: text.into() }
    }
    pub fn is_error(&self) -> bool {
        self.level == IssueLevel::Error
    }
}

/// One environment row of a command, as shown (never the value of a secret).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct EnvView {
    pub name: String,
    /// None for a secret (shown as ••••).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub secret: bool,
    /// Set by KLIF (telemetry / API key), not by the preset.
    pub managed: bool,
    /// An inherited variable the preset removes (`env_remove`, or always-removed key vars).
    pub removed: bool,
    /// A managed variable the preset's own env overrides.
    pub overridden: bool,
}

/// Exactly what a preset launches, resolved for a System, with secrets masked. `display` is the exact command
/// line (`klif_common::cmdline::render` of `program` + `args`), `{env:X}` shown as `%X%`. For an external preset
/// (`endpoint`) there is no command: `external` holds the endpoint URL and `program`/`args` are empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CommandView {
    /// Resolved program path (or the command as written when it cannot be resolved).
    pub program: String,
    /// Final argument tokens; values after secret flags are masked.
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<EnvView>,
    pub port: u16,
    pub host: String,
    pub health: HealthCheck,
    pub adapter: AdapterId,
    pub display: String,
    pub issues: Vec<Issue>,
    /// `klif_catalog::Catalog::preset_hash` of what runs (program/args/env/cwd; not display metadata).
    pub hash: String,
    /// The endpoint URL of an external preset (KLIF never starts or stops it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<String>,
}

/// One param choice as offered by the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ParamOption {
    pub value: String,
    pub label: String,
}

/// A preset param (`[presets.X.params.NAME]`) with the System's current selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ParamView {
    pub name: String,
    pub label: String,
    /// The selected choice's value (the System's `params.NAME`, else the param's default).
    pub value: String,
    pub choices: Vec<ParamOption>,
}

// ------------------------------------------------------------------------------- bench / presets

/// The latest benchmark of a preset (from `<data_dir>\bench\<preset-id>.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct BenchSummary {
    /// Epoch seconds of the run.
    pub at: f64,
    pub runs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
    /// TTS: audio seconds per wall second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts_rtf: Option<f64>,
    /// STT: audio seconds per wall second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stt_rtf: Option<f64>,
    /// Music: seconds of music per wall second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music_rtf: Option<f64>,
    #[serde(rename = "peakVramGiB", skip_serializing_if = "Option::is_none")]
    pub peak_vram_gib: Option<f64>,
    #[serde(rename = "spillMiB", skip_serializing_if = "Option::is_none")]
    pub spill_mib: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_build: Option<String>,
    pub klif_version: String,
    /// Recorded for a different preset hash (the command changed since).
    pub stale: bool,
}

/// A preset as listed (Tune picker, `klif-cli presets list`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    pub adapter: AdapterId,
    pub kind: SystemKind,
    pub model: ModelRef,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The recommendation it was made from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<BenchSummary>,
    /// The GPU it runs on: "VEN:DEV" or "cpu" (preset `gpu`, else `[gpu] inference`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// An external server (`endpoint`): watched, never started or stopped.
    #[serde(default)]
    pub external: bool,
    /// The node the preset lives on (None = this machine). `vm.presets` are local; remote ones are in
    /// `vm.nodes[n].presets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// `Catalog::spec_hash` of the stored spec (= `PresetDetail.spec_hash`): a clean editor draft reloads when it
    /// changes. Empty for an entry that does not parse, and from a node that does not report it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub spec_hash: String,
}

/// One preset in full, for the Tune editor (`klif_preset_get`). `spec` env values that are secret are MASK.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PresetDetail {
    pub id: String,
    pub spec: PresetCfg,
    pub command: CommandView,
    pub info: PresetInfo,
    /// Hash of the stored spec as served here (every field, masked; `klif_catalog::Catalog::spec_hash`). Send it
    /// back as `SavePreset.base_hash`: the write is refused when the preset changed on disk meanwhile (any field,
    /// not only what runs). Empty from a node that does not report it.
    #[serde(default)]
    pub spec_hash: String,
}

// --------------------------------------------------------------------- recommendations / downloads

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum RecFileRole {
    Model,
    Mmproj,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecFile {
    /// Path of the file in the Hugging Face repo.
    pub name: String,
    pub role: RecFileRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Measured on one machine (a starting point, not a promise).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Measured {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
    /// "RX 9070 XT 16 GB, Ryzen 9 9950X3D"
    pub hardware: String,
    /// "llama.cpp b6500 HIP"
    pub backend: String,
    /// "2026-10-01"
    pub date: String,
}

/// A model recommendation for a kind (and LLM class) of System (data, with a disclaimer; downloads only on
/// explicit request).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecommendationInfo {
    pub id: String,
    pub kind: SystemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    pub name: String,
    pub adapter: AdapterId,
    pub hf_repo: String,
    /// Commit sha.
    pub revision: String,
    pub files: Vec<RecFile>,
    pub quant: String,
    pub license: String,
    pub hardware_class: String,
    #[serde(rename = "minVramGiB", skip_serializing_if = "Option::is_none")]
    pub min_vram_gib: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<Measured>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Files present in the models dir (or already referenced by a preset).
    pub installed: bool,
    /// Preset id already using these files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing: Option<String>,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Running,
    Verifying,
    Done,
    #[default]
    Failed,
    Cancelled,
}

/// One file of a recommendation being downloaded (one entry per (id, file)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DownloadInfo {
    /// Recommendation id.
    pub id: String,
    /// File path in the repo.
    pub file: String,
    pub done_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    pub state: DownloadState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ------------------------------------------------------------------------------------------ hardware

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    #[default]
    Gpu,
    Cpu,
}

/// Where a device's FP32 TFLOPS number comes from.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TflopsSource {
    /// The embedded GPU table (vendor-published peak).
    Table,
    /// `[hardware] tflops` in klif.toml.
    Config,
    /// Cores x FLOP per cycle x clock (CPUs).
    Computed,
    #[default]
    Unknown,
}

/// One GPU or CPU of a machine, with its theoretical peak FP32 throughput.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ComputeDevice {
    /// "VEN:DEV", "VEN:DEV#n" (n-th identical adapter) or "cpu".
    pub id: String,
    /// Display name: "RX 9070 XT", "Ryzen 9 9950X3D".
    pub name: String,
    pub kind: DeviceKind,
    /// An integrated GPU (shares system memory).
    pub integrated: bool,
    /// Part of the memory pool and of the TFLOPS total (integrated GPUs only on machines without a discrete GPU;
    /// `[hardware] exclude / include` override).
    pub counted: bool,
    #[serde(rename = "vramGiB", skip_serializing_if = "Option::is_none")]
    pub vram_gib: Option<f64>,
    /// The part of `vram_gib` that is system memory (a counted integrated GPU), already inside `ram_total_gib`.
    #[serde(rename = "sharedGiB", skip_serializing_if = "Option::is_none")]
    pub shared_gib: Option<f64>,
    /// "GDDR6", "HBM3", "LPDDR5X (unified)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<String>,
    /// Theoretical peak FP32 TFLOPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tflops_fp32: Option<f64>,
    pub tflops_source: TflopsSource,
    /// "64 CU, 2970 MHz" / "16 cores, AVX-512, 4.3 GHz".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A machine's static compute and memory facts (no live load).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct HardwareInfo {
    /// Every GPU, counted or not, in enumeration order.
    pub gpus: Vec<ComputeDevice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<ComputeDevice>,
    #[serde(rename = "ramTotalGiB")]
    pub ram_total_gib: f64,
    /// Sum of the counted GPUs' memory: the fast-memory pool.
    #[serde(rename = "vramPoolGiB")]
    pub vram_pool_gib: f64,
    /// The largest single counted GPU (image / audio / video runtimes do not split across cards).
    #[serde(rename = "largestGpuGiB")]
    pub largest_gpu_gib: f64,
    /// The pool is unified memory (a machine whose only GPU is integrated).
    pub unified: bool,
    /// Sum over the counted devices whose TFLOPS are known.
    pub tflops_fp32: f64,
    /// Counted devices without a TFLOPS number (shown as "+?").
    pub tflops_unknown: u32,
}

// --------------------------------------------------------------------------------------- suggestions

/// What a suggestion is for: an LLM class or a non-LLM kind.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SuggestSlot {
    #[default]
    Fast,
    Deep,
    Max,
    Image,
    Tts,
    Stt,
    Video,
    Music,
}

/// The suggested model for one slot on this machine (an estimate from the embedded pool; `rec` None = nothing fits).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Suggestion {
    pub slot: SuggestSlot,
    pub kind: SystemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    /// Recommendation id (`<model>.<quant>`) to download / adopt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rec: Option<String>,
    /// "Qwen 3.8 27B" ("" when nothing fits).
    pub model: String,
    /// "UD-Q4_K_XL".
    pub quant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u32>,
    /// KV cache type: "f16", "q8_0".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv: Option<String>,
    #[serde(rename = "estVramGiB")]
    pub est_vram_gib: f64,
    #[serde(rename = "estRamGiB")]
    pub est_ram_gib: f64,
    #[serde(rename = "budgetVramGiB")]
    pub budget_vram_gib: f64,
    #[serde(rename = "budgetRamGiB")]
    pub budget_ram_gib: f64,
    /// "experts in RAM", "layers in RAM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    /// Why nothing fits, or what to watch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ------------------------------------------------------------------------------------------- records

/// A record metric. `*Tps` and `*Rtf` are best when highest, `*S` when lowest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum RecordMetric {
    /// Generated tokens per second (>= 128 tokens).
    DecodeTps,
    /// Prompt tokens per second, cache excluded (>= 1024 tokens).
    PrefillTps,
    /// Time to first token, any request.
    TtftS,
    /// Seconds per finished image.
    ImageS,
    /// Audio seconds per wall second (TTS).
    TtsRtf,
    /// Audio seconds per wall second (STT).
    SttRtf,
    /// Seconds per finished video job.
    VideoS,
    /// Seconds of music per wall second (higher is better).
    MusicRtf,
}

impl RecordMetric {
    /// Higher is better (`*Tps`, `*Rtf`); otherwise lower is better.
    pub fn higher_is_better(self) -> bool {
        matches!(
            self,
            RecordMetric::DecodeTps | RecordMetric::PrefillTps | RecordMetric::TtsRtf | RecordMetric::SttRtf | RecordMetric::MusicRtf
        )
    }
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum RecordSource {
    #[default]
    Live,
    Bench,
}

/// The model a record belongs to: one exact file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecordModel {
    /// SHA-256 of the model file (the first part of a split GGUF). None until hashed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// File name, e.g. "Qwen3.8-27B-UD-Q4_K_XL.gguf".
    pub file: String,
    /// "Qwen 3.8 27B" (the preset's model name, else the file stem).
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    /// "unsloth/Qwen3.8-27B-GGUF@<sha>" when the file matches a known recommendation file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// One best value with the conditions it was reached under.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecordValue {
    pub value: f64,
    /// Epoch seconds.
    pub at: f64,
    pub source: RecordSource,
    /// The context the server was launched with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_tokens: Option<u64>,
    /// KV cache type from the launch args ("q8_0").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    /// Video frames of the job (videoS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// GPU names it ran on.
    pub gpus: Vec<String>,
    /// "b6500".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_build: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub klif_version: String,
    /// The machine's FP32 TFLOPS total at the time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tflops_fp32: Option<f64>,
}

/// The best values of one model file on one backend on one machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecordEntry {
    /// "<machine>|<sha256 or file:size>|<backend>".
    pub key: String,
    /// Remote node id (None: this machine).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// `[node] name` of the machine, else "This machine" (never the host name).
    pub machine: String,
    pub kind: SystemKind,
    pub model: RecordModel,
    /// "HIP", "Vulkan", "CUDA", "CPU", "Metal", "" (unknown).
    pub backend: String,
    pub best: BTreeMap<RecordMetric, RecordValue>,
}

/// A broken record (the last few, for the "new record" moment).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecordEvent {
    pub key: String,
    pub metric: Option<RecordMetric>,
    /// The previous best (None: the first value for this key and metric).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<f64>,
    pub new: f64,
    pub at: f64,
}

// ---------------------------------------------------------------------------------------- config

/// Where the API key comes from and whether one is set (never the key).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ApiKeyInfo {
    /// "file" | "env:NAME" | "none"
    pub source: String,
    pub set: bool,
}

/// Facts about the configuration the engine runs with.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ConfigInfo {
    /// The klif.toml in use (None: defaults, no file yet).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub state_dir: String,
    pub data_dir: String,
    pub issues: Vec<Issue>,
    pub api_key: ApiKeyInfo,
    /// `[paths] models_dir` (None: downloads are refused until it is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_dir: Option<String>,
    /// `[launch] on_conflict`: what Launch does when other Systems must stop first.
    #[serde(default)]
    pub on_conflict: OnConflict,
    /// `[ui] record_moment`: show the "new record" moment (default on).
    #[serde(default = "yes")]
    pub record_moment: bool,
    /// `[ui] skin`: the skin the window reported last (klif-webui follows it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,
    /// klif-webui, the LAN control page (`[webui]`, pairing, devices). Local only: a node's peers never see it.
    #[serde(default)]
    pub webui: WebUiInfo,
}

/// klif-webui as Tune shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct WebUiInfo {
    /// `[webui] enabled`.
    pub enabled: bool,
    /// `[webui] host`: "0.0.0.0" (every network) or one IP address.
    pub host: String,
    pub port: u16,
    /// Serving now.
    pub listening: bool,
    /// What a phone opens ("http://192.0.2.10:7341/"), best first; empty while it does not listen.
    pub urls: Vec<String>,
    /// Why it does not serve although enabled: one sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub devices: Vec<WebUiDevice>,
    /// The open pairing, for the QR code (None: none open).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<WebUiPairing>,
}

/// A paired klif-webui device.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct WebUiDevice {
    pub id: String,
    /// What the device called itself when it paired ("Phone", "Firefox on Windows"...).
    pub name: String,
    /// Unix seconds.
    pub paired_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<f64>,
}

/// An open klif-webui pairing: the QR code carries `url` (the secret in its `#` part), `code` is for typing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct WebUiPairing {
    pub url: String,
    /// Six digits.
    pub code: String,
    /// Unix seconds.
    pub expires_at: f64,
}

fn yes() -> bool {
    true
}

// ----------------------------------------------------------------------------------------- systems view

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum VramSource {
    /// The composition of the last live session of this exact preset hash.
    Measured,
    #[default]
    /// Weights only, from the model (+ mmproj) file sizes.
    FileSize,
}

/// One System (a tab): its configuration, what it would run, and its live state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct System {
    pub id: SystemId,
    pub label: String,
    pub kind: SystemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    /// The remote node it lives on (None = this machine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub status: SystemStatus,
    /// One plain sentence for not-set / invalid / fault / unreachable (and conflicts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The launch availability of the active preset.
    pub availability: Availability,
    pub model: ModelRef,
    /// The active preset id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// The active preset's params with the System's selection (empty when it has none).
    #[serde(default)]
    pub params: Vec<ParamView>,
    /// What Launch would run now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench: Option<BenchSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_vram: Option<Vec<VramLayer>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_vram_source: Option<VramSource>,
    /// The first GPU it runs on ("VEN:DEV", "VEN:DEV#n" or "cpu"): the fit display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// Every GPU it runs on (preset `gpu` may list several); conflicts / reservations apply to all of them.
    #[serde(default)]
    pub gpus: Vec<String>,
    /// Its preset is an external server (watched only).
    #[serde(default)]
    pub external: bool,
    /// Needs the whole GPU (`exclusive = true`).
    #[serde(default)]
    pub exclusive: bool,
    /// Its preset keeps weights in system RAM (`--cpu-moe`, `-ncmoe N`, `-ot ...=CPU`, `-ngl 0`, sd.cpp
    /// `--offload-to-cpu`, vLLM `--cpu-offload-gb`): the server makes do with the VRAM it finds, so KLIF never says
    /// it does not fit (it still frees the GPU from other Systems for speed).
    #[serde(default, skip_serializing_if = "is_false")]
    pub ram_offload: bool,
    /// False for remote Systems whose node does not grant "edit" (and for ghost sessions).
    #[serde(default)]
    pub editable: bool,
    /// May be launched / stopped from here: local Systems true; remote ones when the node grants "launch".
    #[serde(default)]
    pub controllable: bool,
    /// Running Systems that must stop before this one can launch (port, exclusive GPU, VRAM).
    #[serde(default)]
    pub conflicts: Vec<SystemId>,
    /// The base URL clients use (LLM: ".../v1").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_session: Option<LastSession>,
    /// 0..1: how hard it works right now (decode / prefill / steps / requests), for the tab pulse.
    #[serde(default)]
    pub activity: f64,
}

// ----------------------------------------------------------------------------------------- session

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Starting,
    Loading,
    Live,
    Stopping,
    Fault,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LoadStepId {
    #[default]
    Process,
    Device,
    Weights,
    Kv,
    Warmup,
    Ready,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    Done,
    Active,
    #[default]
    Pending,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct LoadStep {
    pub id: LoadStepId,
    pub label: String,
    pub state: StepState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct LoadProgress {
    pub steps: Vec<LoadStep>,
    pub fraction: f64,
    pub elapsed_s: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
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

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    #[default]
    Stopped,
    Fault,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct LastSession {
    pub system: SystemId,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RequestRecord {
    pub id: u64,
    pub at: f64,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub prefill_s: f64,
    pub generated_tokens: u64,
    pub decode_s: f64,
    /// The best decode speed of one full window the server measured during the request (llama.cpp `tg_3s`, a
    /// window of at least 64 tokens): what the live readout showed at its peak. None when the server reports none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_decode_tps: Option<f64>,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LlmActivity {
    #[default]
    Idle,
    Prefill,
    Decode,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Prefill {
    pub tokens: u64,
    pub done_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    pub tps: f64,
    pub elapsed_s: f64,
    pub eta_s: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ContextFill {
    pub used_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Spec {
    pub acceptance_pct: f64,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Totals {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub generated_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ImageJob {
    pub at: f64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub edit: bool,
    /// Sampling steps, when the log showed them (sd.cpp progress bar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    /// A video job (sd.cpp `generate_video WxHxT`): its frame count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ImageActivity {
    #[default]
    Idle,
    Generating,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
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
    /// Frames of the video job in flight (or the last one), for sd.cpp video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Session {
    pub system: SystemId,
    pub model: ModelRef,
    pub phase: Phase,
    pub uptime_s: f64,
    pub endpoint: Endpoint,
    pub api_key_set: bool,
    pub loading: Option<LoadProgress>,
    pub fault: Option<Fault>,
    /// The live part is ONE of `llm` / `image` / `generic` (the others are null): llm for kind llm (llama.cpp,
    /// vllm), image for kind image (sd.cpp), generic for tts / stt / video and adapters without a parser. One
    /// exception: a video System on sd.cpp has `generic` AND `image` (steps, the job in flight, recent jobs with
    /// their frames).
    pub llm: Option<LlmLive>,
    pub image: Option<ImageLive>,
    #[serde(default)]
    pub generic: Option<GenericLive>,
    /// The preset the session was launched from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// What actually ran (compare `hash` with the System's `command.hash`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandView>,
    /// The GPU it runs on: "VEN:DEV" or "cpu".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// VRAM committed by the session's processes (PDH Total Committed over its pids).
    #[serde(rename = "vramGiB", default, skip_serializing_if = "Option::is_none")]
    pub vram_gib: Option<f64>,
}

/// What KLIF can tell about a server without a dedicated parser (tts / stt / video, generic, openai).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GenericLive {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_in_flight: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_total: Option<u64>,
    /// Seconds since the last log line / request activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_s: Option<f64>,
    /// The model id the server reports (/v1/models).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Whether the model's weights are resident (audio.cpp `/v1/models` `loaded`; lazy loading and
    /// `--idle-unload-ms` unload them). None: the server does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_loaded: Option<bool>,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum VramLayerId {
    Weights,
    Kv,
    Buffers,
    Draft,
    Projector,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct VramLayer {
    pub id: VramLayerId,
    pub label: String,
    pub gib: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GpuMemory {
    /// "VEN:DEV" (e.g. "1002:7550"), "VEN:DEV#n" (n-th identical adapter in DXGI order) or "cpu".
    #[serde(default)]
    pub id: String,
    /// Display name, e.g. "RX 9070 XT" (`[gpu] inference_name` for the inference card).
    #[serde(default)]
    pub name: String,
    /// Same as `name` (kept so 0.2 skins keep working).
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
    /// The GPU powered down (or paged the sessions out) while a model is loaded. While set,
    /// `layers` are the sessions' ALLOCATIONS (they may sum to more than `used_gib`, which stays the
    /// resident amount). See types.ts GpuMemory.dormant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant: Option<Dormant>,
}

/// types.ts GpuMemory.dormant. Explicit renames for the "GiB" fields (camelCase would give "Gib").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Dormant {
    #[serde(rename = "pagedOutGiB")]
    pub paged_out_gib: f64,
    pub since_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MachineStats {
    #[serde(rename = "ramUsedGiB")]
    pub ram_used_gib: f64,
    #[serde(rename = "ramTotalGiB")]
    pub ram_total_gib: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_type: Option<String>,
    pub cpu_name: String,
    pub cpu_pct: f64,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    #[default]
    Browser,
    Tauri,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct PanelInfo {
    pub available: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}


// ------------------------------------------------------------------------------------------- nodes

/// A remote node's connection state. TS: 'connecting'|'online'|'offline'|'unauthorized'|'incompatible'.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    /// First contact not finished yet.
    Connecting,
    Online,
    #[default]
    Offline,
    Unauthorized,
    Incompatible,
}

/// One `[nodes.<id>]` entry as seen from here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct NodeView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub state: NodeState,
    /// The node's KLIF version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    /// Rights the node grants us besides view: "launch", "edit".
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub gpus: Vec<GpuMemory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<HardwareInfo>,
    /// The node's presets (masked), `node` set to this node's id.
    #[serde(default)]
    pub presets: Vec<PresetInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// -------------------------------------------------------------------------------------- view model

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ViewModel {
    pub now: f64,
    /// Local Systems in file order, then ghost sessions, then remote Systems by node.
    pub systems: Vec<System>,
    /// The selected tab (None: no Systems).
    pub selected: Option<SystemId>,
    // Conveniences so skins keep working, all derived from the SELECTED System:
    /// The selected System's session.
    pub session: Option<Session>,
    /// The selected System's last session.
    pub last_session: Option<LastSession>,
    /// The selected System's console: last lines, ANSI-stripped, newest last, up to 200.
    pub console: Vec<String>,
    /// The selected System's GPU, else `[gpu] inference`: `[other, ...the selected session's layers]`.
    pub vram: GpuMemory,
    /// Every local GPU KLIF measures (each System's GPUs + `[gpu] inference`), `layers = [other]`.
    #[serde(default)]
    pub gpus: Vec<GpuMemory>,
    pub machine: MachineStats,
    pub host: HostInfo,
    /// Every LOCAL preset (including unreadable ones, as Invalid). Remote presets: `nodes[n].presets`.
    #[serde(default)]
    pub presets: Vec<PresetInfo>,
    #[serde(default)]
    pub recommendations: Vec<RecommendationInfo>,
    /// This machine's compute and memory.
    #[serde(default)]
    pub hardware: HardwareInfo,
    /// The suggested model per slot for this machine (estimates).
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
    /// Best values per model file and backend, this machine and the nodes.
    #[serde(default)]
    pub records: Vec<RecordEntry>,
    /// The last broken records, newest last.
    #[serde(default)]
    pub record_events: Vec<RecordEvent>,
    /// Changes whenever `records` / `record_events` change (0: unknown). A node's `snapshot {recordsRev}` leaves
    /// both lists empty when the caller already holds that revision.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub records_rev: u64,
    #[serde(default)]
    pub downloads: Vec<DownloadInfo>,
    #[serde(default)]
    pub config: ConfigInfo,
    #[serde(default)]
    pub nodes: Vec<NodeView>,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

// ----------------------------------------------------------------------------------------- actions

fn is_false(b: &bool) -> bool {
    !*b
}

/// The right an action (or protocol method) needs (SPEC 16.11, default-deny). TS: 'view'|'launch'|'edit'|'local-only'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Right {
    /// hello, snapshot, status, preset, plan, records_history: every authenticated peer.
    View,
    /// Launch, Stop, StopAll, Restart, Dismiss, UsePreset, SetParam.
    Launch,
    /// SavePreset, DeletePreset, AddSystem, RemoveSystem, UpdateSystem, downloads, AdoptRecommendation,
    /// ForgetRecord, command_preview: arbitrary command execution on that machine. Implies Launch.
    Edit,
    /// Never over the network: Select, diag.
    LocalOnly,
}

impl Right {
    pub fn as_str(self) -> &'static str {
        match self {
            Right::View => "view",
            Right::Launch => "launch",
            Right::Edit => "edit",
            Right::LocalOnly => "local-only",
        }
    }

    /// Whether a network peer holding `allow` (`[node] allow`) may use this right (edit implies launch).
    pub fn granted(self, allow: &[NodeRight]) -> bool {
        match self {
            Right::View => true,
            Right::Launch => allow.contains(&NodeRight::Launch) || allow.contains(&NodeRight::Edit),
            Right::Edit => allow.contains(&NodeRight::Edit),
            Right::LocalOnly => false,
        }
    }

    /// The right a protocol method needs (`act` is decided by its Action: [`Action::required_right`]); unknown
    /// methods are LocalOnly (default-deny).
    pub fn for_method(method: &str) -> Right {
        match method {
            "hello" | "snapshot" | "status" | "preset" | "plan" | "records_history" => Right::View,
            "command_preview" => Right::Edit,
            _ => Right::LocalOnly,
        }
    }
}

/// Actions the engine executes. Shell-only actions (open/copy endpoint, copy key, console, tune, window chrome,
/// API key, open klif.toml / logs) are handled by the host and never reach the engine. `system` absent = the
/// selected System.
///
/// Routing (SPEC 16.1): an action goes to `node` when present, else to the node of its `"<node>/<id>"` System ids,
/// else stays local; both present and different -> refused ([`Action::route`]). The forwarder strips `"<node>/"`
/// from every System id ([`Action::map_system_ids`]) and clears `node`.
///
/// Wire shape (TS `EngineAction`): `{type:'select',system}` `{type:'launch',system?,stopOthers?}`
/// `{type:'stop',system?}` `{type:'stopAll'}` `{type:'restart',system?}` `{type:'dismiss',system?}`
/// `{type:'usePreset',system,preset}` `{type:'setParam',system,name,value}`
/// `{type:'savePreset',id,preset,selectFor?,secretsFrom?,baseHash?,node?}` `{type:'deletePreset',id,node?}`
/// `{type:'addSystem',id?,label?,kind,class?,preset?,node?}` `{type:'removeSystem',system}`
/// `{type:'updateSystem',system,label?,moveTo?,exclusive?}` `{type:'downloadRecommendation',id,node?}`
/// `{type:'cancelDownload',id,node?}` `{type:'adoptRecommendation',id,system?}`.
///
/// `Debug` masks the SavePreset env (through `PresetCfg`'s Debug); hosts log only [`Action::summary`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    /// Select a tab (never stops anything). Local only.
    Select {
        system: SystemId,
    },
    /// Launch a System. With conflicts: `stop_others` (or `[launch] on_conflict = "stop"`, never for a busy
    /// holder) stops them first, else the launch is refused with a sentence naming them.
    Launch {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<SystemId>,
        #[serde(default, skip_serializing_if = "is_false")]
        stop_others: bool,
    },
    Stop {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<SystemId>,
    },
    /// Stop every local running System.
    StopAll,
    Restart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<SystemId>,
    },
    /// Leave a fault and return the System to offline.
    Dismiss {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<SystemId>,
    },
    /// Make `preset` the System's active preset (writes `[systems.X] preset`). Refused on a kind mismatch.
    UsePreset {
        system: SystemId,
        preset: String,
    },
    /// Select a param choice for the System (writes `[systems.X] params.NAME`). Applies on the next launch.
    SetParam {
        system: SystemId,
        name: String,
        value: String,
    },
    /// Create or replace `[presets.<id>]`. MASK values are resolved against the stored preset `secrets_from`
    /// (default: `id`); `base_hash` (= `PresetDetail.spec_hash` of the preset as loaded) refuses the write if the
    /// stored spec changed meanwhile.
    SavePreset {
        id: String,
        preset: PresetCfg,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        select_for: Option<SystemId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secrets_from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    /// Refused while the preset is active on a System or running.
    DeletePreset {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    /// Add `[systems.<id>]` (id absent = the next free default id, label absent = the default label).
    AddSystem {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        kind: SystemKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<LlmClass>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    /// Refused while it runs.
    RemoveSystem {
        system: SystemId,
    },
    /// Rename, move to a tab index (0-based among the local Systems), set exclusive.
    UpdateSystem {
        system: SystemId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        move_to: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive: Option<bool>,
    },
    DownloadRecommendation {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    CancelDownload {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    /// Turn a recommendation into a preset (and make it the System's active preset when `system` is given).
    AdoptRecommendation {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system: Option<SystemId>,
        /// Context from a suggestion (else the recommendation's).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ctx: Option<u32>,
        /// KV cache type from a suggestion ("q8_0").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kv: Option<String>,
    },
    /// Remove a junk record entry (`RecordEntry.key` as that machine knows it; `node` = a remote node's entry).
    ForgetRecord {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
    },
    /// This machine's settings in `[ui]` and `[webui]` (absent fields stay as they are). Never over the network.
    UpdateSettings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record_moment: Option<bool>,
        /// The skin the window shows (its id); klif-webui follows it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skin: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        webui_enabled: Option<bool>,
        /// An IP address ("0.0.0.0": every network).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        webui_host: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        webui_port: Option<u16>,
    },
    /// Open a klif-webui pairing: a one-time secret for a QR code plus a 6-digit code, valid a few minutes
    /// (`config.webui.pairing`). Replaces an open one. Never over the network.
    PairWebDevice,
    /// Close the open klif-webui pairing. Never over the network.
    CancelWebPairing,
    /// Remove a paired klif-webui device (None: every device); it has to pair again. Never over the network.
    ForgetWebDevice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
}

impl Action {
    /// The wire `type`: "select", "launch", ..., "adoptRecommendation".
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Select { .. } => "select",
            Action::Launch { .. } => "launch",
            Action::Stop { .. } => "stop",
            Action::StopAll => "stopAll",
            Action::Restart { .. } => "restart",
            Action::Dismiss { .. } => "dismiss",
            Action::UsePreset { .. } => "usePreset",
            Action::SetParam { .. } => "setParam",
            Action::SavePreset { .. } => "savePreset",
            Action::DeletePreset { .. } => "deletePreset",
            Action::AddSystem { .. } => "addSystem",
            Action::RemoveSystem { .. } => "removeSystem",
            Action::UpdateSystem { .. } => "updateSystem",
            Action::DownloadRecommendation { .. } => "downloadRecommendation",
            Action::CancelDownload { .. } => "cancelDownload",
            Action::AdoptRecommendation { .. } => "adoptRecommendation",
            Action::ForgetRecord { .. } => "forgetRecord",
            Action::UpdateSettings { .. } => "updateSettings",
            Action::PairWebDevice => "pairWebDevice",
            Action::CancelWebPairing => "cancelWebPairing",
            Action::ForgetWebDevice { .. } => "forgetWebDevice",
        }
    }

    /// The System the action targets, as written (None = the selected System, or no System involved).
    pub fn system(&self) -> Option<&SystemId> {
        match self {
            Action::Select { system }
            | Action::UsePreset { system, .. }
            | Action::SetParam { system, .. }
            | Action::RemoveSystem { system }
            | Action::UpdateSystem { system, .. } => Some(system),
            Action::Launch { system, .. }
            | Action::Stop { system }
            | Action::Restart { system }
            | Action::Dismiss { system }
            | Action::AdoptRecommendation { system, .. } => system.as_ref(),
            Action::SavePreset { select_for, .. } => select_for.as_ref(),
            Action::StopAll
            | Action::DeletePreset { .. }
            | Action::AddSystem { .. }
            | Action::DownloadRecommendation { .. }
            | Action::CancelDownload { .. }
            | Action::ForgetRecord { .. }
            | Action::UpdateSettings { .. }
            | Action::PairWebDevice
            | Action::CancelWebPairing
            | Action::ForgetWebDevice { .. } => None,
        }
    }

    /// Every System id field the action carries (0 or 1 today).
    pub fn system_ids(&self) -> Vec<&SystemId> {
        self.system().into_iter().collect()
    }

    /// The action with `f` applied to every System id field (the forwarder strips `"<node>/"` with it).
    pub fn map_system_ids(self, mut f: impl FnMut(SystemId) -> SystemId) -> Action {
        let mut a = self;
        match &mut a {
            Action::Select { system }
            | Action::UsePreset { system, .. }
            | Action::SetParam { system, .. }
            | Action::RemoveSystem { system }
            | Action::UpdateSystem { system, .. } => *system = f(std::mem::take(system)),
            Action::Launch { system, .. }
            | Action::Stop { system }
            | Action::Restart { system }
            | Action::Dismiss { system }
            | Action::AdoptRecommendation { system, .. }
            | Action::SavePreset { select_for: system, .. } => *system = system.take().map(&mut f),
            Action::StopAll
            | Action::DeletePreset { .. }
            | Action::AddSystem { .. }
            | Action::DownloadRecommendation { .. }
            | Action::CancelDownload { .. }
            | Action::ForgetRecord { .. }
            | Action::UpdateSettings { .. }
            | Action::PairWebDevice
            | Action::CancelWebPairing
            | Action::ForgetWebDevice { .. } => {}
        }
        a
    }

    /// The explicit `node` field (SavePreset, DeletePreset, AddSystem, DownloadRecommendation, CancelDownload,
    /// ForgetRecord), else the node of the first `"<node>/<id>"` System id; None = local.
    pub fn node(&self) -> Option<&str> {
        self.explicit_node().or_else(|| self.system_ids().into_iter().find_map(|id| id.node()))
    }

    fn explicit_node(&self) -> Option<&str> {
        match self {
            Action::SavePreset { node, .. }
            | Action::DeletePreset { node, .. }
            | Action::AddSystem { node, .. }
            | Action::DownloadRecommendation { node, .. }
            | Action::CancelDownload { node, .. }
            | Action::ForgetRecord { node, .. } => node.as_deref().map(str::trim).filter(|n| !n.is_empty()),
            _ => None,
        }
    }

    /// Where the action goes: Ok(None) = local, Ok(Some(node)). Err (a sentence) when the explicit `node` and a
    /// System id's node differ, or the System ids name different nodes / mix local and remote.
    pub fn route(&self) -> Result<Option<&str>, String> {
        let ids = self.system_ids();
        let mut nodes = ids.iter().map(|id| id.node());
        let first = nodes.next().flatten();
        if ids.iter().any(|id| id.node() != first) {
            return Err("The action names Systems on different machines.".into());
        }
        match (self.explicit_node(), first) {
            (Some(a), Some(b)) if a != b => Err(format!("The action names node \"{a}\" but its System is on \"{b}\".")),
            (Some(n), _) | (None, Some(n)) => Ok(Some(n)),
            (None, None) => Ok(None),
        }
    }

    /// The same action for the node it is forwarded to: System ids without `"<node>/"`, `node` cleared.
    pub fn for_forwarding(self) -> Action {
        let mut a = self.map_system_ids(|id| SystemId::new(id.local()));
        match &mut a {
            Action::SavePreset { node, .. }
            | Action::DeletePreset { node, .. }
            | Action::AddSystem { node, .. }
            | Action::DownloadRecommendation { node, .. }
            | Action::CancelDownload { node, .. }
            | Action::ForgetRecord { node, .. } => *node = None,
            _ => {}
        }
        a
    }

    /// The right a network peer needs for this action (SPEC 16.11).
    pub fn required_right(&self) -> Right {
        match self {
            Action::Select { .. }
            | Action::UpdateSettings { .. }
            | Action::PairWebDevice
            | Action::CancelWebPairing
            | Action::ForgetWebDevice { .. } => Right::LocalOnly,
            Action::Launch { .. }
            | Action::Stop { .. }
            | Action::StopAll
            | Action::Restart { .. }
            | Action::Dismiss { .. }
            | Action::UsePreset { .. }
            | Action::SetParam { .. } => Right::Launch,
            Action::SavePreset { .. }
            | Action::DeletePreset { .. }
            | Action::AddSystem { .. }
            | Action::RemoveSystem { .. }
            | Action::UpdateSystem { .. }
            | Action::DownloadRecommendation { .. }
            | Action::CancelDownload { .. }
            | Action::AdoptRecommendation { .. }
            | Action::ForgetRecord { .. } => Right::Edit,
        }
    }

    /// What a host may log: the type plus System / id / node, never a preset body. E.g. "savePreset id=gemma-26b".
    pub fn summary(&self) -> String {
        let mut out = self.kind().to_string();
        match self {
            Action::SavePreset { id, .. }
            | Action::DeletePreset { id, .. }
            | Action::DownloadRecommendation { id, .. }
            | Action::CancelDownload { id, .. }
            | Action::AdoptRecommendation { id, .. } => out.push_str(&format!(" id={id}")),
            Action::ForgetRecord { key, .. } => out.push_str(&format!(" key={key}")),
            Action::AddSystem { id, kind, .. } => {
                out.push_str(&format!(" kind={kind}"));
                if let Some(id) = id {
                    out.push_str(&format!(" id={id}"));
                }
            }
            _ => {}
        }
        if let Some(s) = self.system() {
            out.push_str(&format!(" system={s}"));
        }
        if let Some(n) = self.explicit_node() {
            out.push_str(&format!(" node={n}"));
        }
        if let Action::Launch { stop_others: true, .. } = self {
            out.push_str(" stopOthers");
        }
        out
    }
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Select { system } => f.debug_struct("Select").field("system", system).finish(),
            Action::Launch { system, stop_others } => {
                f.debug_struct("Launch").field("system", system).field("stop_others", stop_others).finish()
            }
            Action::Stop { system } => f.debug_struct("Stop").field("system", system).finish(),
            Action::StopAll => f.write_str("StopAll"),
            Action::Restart { system } => f.debug_struct("Restart").field("system", system).finish(),
            Action::Dismiss { system } => f.debug_struct("Dismiss").field("system", system).finish(),
            Action::UsePreset { system, preset } => {
                f.debug_struct("UsePreset").field("system", system).field("preset", preset).finish()
            }
            Action::SetParam { system, name, value } => {
                f.debug_struct("SetParam").field("system", system).field("name", name).field("value", value).finish()
            }
            // PresetCfg's Debug masks every env value and every secret arg value.
            Action::SavePreset { id, preset, select_for, secrets_from, base_hash, node } => f
                .debug_struct("SavePreset")
                .field("id", id)
                .field("preset", preset)
                .field("select_for", select_for)
                .field("secrets_from", secrets_from)
                .field("base_hash", base_hash)
                .field("node", node)
                .finish(),
            Action::DeletePreset { id, node } => f.debug_struct("DeletePreset").field("id", id).field("node", node).finish(),
            Action::AddSystem { id, label, kind, class, preset, node } => f
                .debug_struct("AddSystem")
                .field("id", id)
                .field("label", label)
                .field("kind", kind)
                .field("class", class)
                .field("preset", preset)
                .field("node", node)
                .finish(),
            Action::RemoveSystem { system } => f.debug_struct("RemoveSystem").field("system", system).finish(),
            Action::UpdateSystem { system, label, move_to, exclusive } => f
                .debug_struct("UpdateSystem")
                .field("system", system)
                .field("label", label)
                .field("move_to", move_to)
                .field("exclusive", exclusive)
                .finish(),
            Action::DownloadRecommendation { id, node } => {
                f.debug_struct("DownloadRecommendation").field("id", id).field("node", node).finish()
            }
            Action::CancelDownload { id, node } => f.debug_struct("CancelDownload").field("id", id).field("node", node).finish(),
            Action::AdoptRecommendation { id, system, ctx, kv } => f
                .debug_struct("AdoptRecommendation")
                .field("id", id)
                .field("system", system)
                .field("ctx", ctx)
                .field("kv", kv)
                .finish(),
            Action::ForgetRecord { key, node } => f.debug_struct("ForgetRecord").field("key", key).field("node", node).finish(),
            Action::UpdateSettings { record_moment, skin, webui_enabled, webui_host, webui_port } => f
                .debug_struct("UpdateSettings")
                .field("record_moment", record_moment)
                .field("skin", skin)
                .field("webui_enabled", webui_enabled)
                .field("webui_host", webui_host)
                .field("webui_port", webui_port)
                .finish(),
            Action::PairWebDevice => f.write_str("PairWebDevice"),
            Action::CancelWebPairing => f.write_str("CancelWebPairing"),
            Action::ForgetWebDevice { device } => f.debug_struct("ForgetWebDevice").field("device", device).finish(),
        }
    }
}
