//! The embedded model pool, `data/recommendations.toml` (schema 2), and the recommendations it expands to. A pool
//! model has a ladder of quant rungs; every (model, rung) becomes one concrete [`Recommendation`] with the id
//! `<model>.<rung>` (download, adopt, Tune's cards), and [`crate::suggest`] picks rungs per machine from the same data.
//! Files come only from the official Hugging Face hub and only when the user asks (`klif_core::download`). Models and
//! rungs with network/model-fetch flags (`-hf --hf-repo -hff --hf-file -hft --hf-token -mu --model-url -dr
//! --docker-repo`, plus their draft / vocoder / mmproj variants), secret flags or env templates are rejected at load,
//! as are those whose repo, revision or file names are not plain. Owner: package B.
//!
//! A file may come from another repo than the entry's `hf_repo` (a model whose encoder or VAE lives elsewhere): its
//! own `repo` + `revision` override the entry's for that file (download, install folder). A rung may name its own
//! `hf_repo` + `revision` (Gemma's QAT repo). Recommendation args may name a listed file as `{file:<name>}` (the pool
//! may also say `{file:<key>}`, resolved at load); a preset made from the recommendation gets that file's install path
//! there (`{model}` / `{mmproj}` come from the files with role model / mmproj). `save_as` installs a file under another
//! path below its repo's folder (a replacement first shard that must sit where the original would).

use klif_common::config::Config;
use klif_common::secret::is_secret_flag;
use klif_common::vm::{AdapterId, LlmClass, Measured, RecFileRole, SuggestSlot, SystemKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The embedded data file.
pub const RECOMMENDATIONS_TOML: &str = include_str!("../data/recommendations.toml");

/// The schema version this KLIF reads.
pub const SCHEMA: i64 = 2;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Flags that make a server fetch models or talk to the network on its own (refused in recommendation args).
pub const NETWORK_FLAGS: &[&str] = &[
    "-hf",
    "-hfr",
    "--hf-repo",
    "-hff",
    "--hf-file",
    "-hft",
    "--hf-token",
    "-mu",
    "--model-url",
    "-dr",
    "--docker-repo",
    "-hfd",
    "-hfrd",
    "--hf-repo-draft",
    "-hffd",
    "--hf-file-draft",
    "-hfv",
    "-hfrv",
    "--hf-repo-v",
    "-hffv",
    "--hf-file-v",
    "-mmu",
    "--mmproj-url",
];

/// One file of a recommendation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecFileSpec {
    /// Path of the file in the repo.
    pub name: String,
    pub role: RecFileRole,
    /// The repo ("owner/name") this file comes from, when not the entry's `hf_repo`. Needs `revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The commit sha (40 hex) of the file's repo; default: the entry's `revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// LFS sha256 (hex), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Pool only: a short name the model's args template uses as `{file:<key>}` ("te", "vae").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Install under this path (below the repo's folder) instead of `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
}

impl RecFileSpec {
    /// The path below the repo's install folder: `save_as`, else `name`.
    pub fn install_name(&self) -> &str {
        self.save_as.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&self.name)
    }
}

/// One recommendation: a pool model at one quant rung (snake_case keys).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    /// `<model id>.<rung id>`.
    pub id: String,
    pub kind: SystemKind,
    /// LLM only: the first tier (fast | deep | max) that lists the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    pub name: String,
    pub adapter: AdapterId,
    pub hf_repo: String,
    /// Commit sha the files were verified at.
    pub revision: String,
    pub files: Vec<RecFileSpec>,
    pub quant: String,
    pub license: String,
    pub hardware_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_vram_gib: Option<f64>,
    /// Arg template for the preset (placeholders allowed; no env templates).
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    /// The pool model's id ("voxcpm2") and family ("voxcpm2", "qwen3_tts"): what an audio.cpp server config names.
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<RecMeasured>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Recommendation {
    /// The file with role `model` (else the first file).
    pub fn model_file(&self) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.role == RecFileRole::Model).or_else(|| self.files.first())
    }

    /// The file with role `mmproj`, if any.
    pub fn mmproj_file(&self) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.role == RecFileRole::Mmproj)
    }

    /// Total size of all files, when every size is known.
    pub fn total_bytes(&self) -> Option<u64> {
        self.files.iter().map(|f| f.size).sum()
    }

    /// A listed file by its path in the repo.
    pub fn file(&self, name: &str) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.name == name)
    }

    /// The repo a file comes from: its own `repo`, else the entry's `hf_repo`.
    pub fn repo_of<'a>(&'a self, f: &'a RecFileSpec) -> &'a str {
        f.repo.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or(&self.hf_repo)
    }

    /// The commit a file is fetched at: its own `revision`, else the entry's `revision`.
    pub fn revision_of<'a>(&'a self, f: &'a RecFileSpec) -> &'a str {
        f.revision.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or(&self.revision)
    }
}

/// The `{file:<name>}` references in a recommendation arg, in order.
pub fn file_refs(arg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = arg;
    while let Some(i) = rest.find("{file:") {
        let after = &rest[i + "{file:".len()..];
        let Some(j) = after.find('}') else { break };
        out.push(&after[..j]);
        rest = &after[j + 1..];
    }
    out
}

/// `arg` with every `{file:<name>}` replaced by `path(name)`.
pub(crate) fn expand_file_refs(arg: &str, path: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(i) = rest.find("{file:") {
        let after = &rest[i + "{file:".len()..];
        let Some(j) = after.find('}') else { break };
        out.push_str(&rest[..i]);
        out.push_str(&path(&after[..j]));
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

/// `measured = { ... }` in the TOML (snake_case); shown as `vm::Measured`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecMeasured {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
    #[serde(default)]
    pub hardware: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub date: String,
}

impl From<&RecMeasured> for Measured {
    fn from(m: &RecMeasured) -> Measured {
        Measured {
            decode_tps: m.decode_tps,
            prefill_tps: m.prefill_tps,
            seconds_per_image: m.seconds_per_image,
            hardware: m.hardware.clone(),
            backend: m.backend.clone(),
            date: m.date.clone(),
        }
    }
}

// ------------------------------------------------------------------------------------------------ pool

/// KV cache cost of a model (MiB at f16). KV(ctx, factor) = (f16_mib_per_1k x ctx / 1024 + fixed_mib) x factor +
/// state_mib; factor 1.0 for f16, 0.53 for q8_0.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KvCost {
    /// Cache that grows with the context: MiB per 1024 tokens at f16.
    pub f16_mib_per_1k: f64,
    /// Cache cells that do not grow with the context (sliding windows), MiB at f16.
    #[serde(default)]
    pub fixed_mib: f64,
    /// Recurrent state (F32, never quantized), MiB per sequence.
    #[serde(default)]
    pub state_mib: f64,
}

impl KvCost {
    /// MiB at `ctx` tokens with this K/V type factor (1.0 = f16).
    pub fn mib(&self, ctx: u32, factor: f64) -> f64 {
        (self.f16_mib_per_1k * ctx as f64 / 1024.0 + self.fixed_mib) * factor + self.state_mib
    }
}

/// One quant rung of a pool model (`[[model.quant]]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolQuant {
    /// "ud-q4-k-xl": the recommendation id is `<model id>.<this>`.
    pub id: String,
    /// "UD-Q4_K_XL".
    pub label: String,
    /// About the effective bits (Q8_0 8, UD-Q5_K_XL 5.5, ...): what the suggester ranks by.
    pub quality: f64,
    /// The file's real bits per weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<f64>,
    /// The rung's own repo and commit (both or neither), else the model's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hf_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub files: Vec<RecFileSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<RecMeasured>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Image rungs: GiB of VRAM with everything on the GPU (estimate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_vram_gib: Option<f64>,
    /// Image rungs: GiB of VRAM and of RAM with --offload-to-cpu (estimates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload_vram_gib: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload_ram_gib: Option<f64>,
}

impl PoolQuant {
    /// Sum of the listed file sizes (unknown sizes count 0; LLM rungs must list every size).
    pub fn bytes(&self) -> u64 {
        self.files.iter().filter_map(|f| f.size).sum()
    }

    pub fn gib(&self) -> f64 {
        self.bytes() as f64 / GIB
    }
}

/// One model of the pool (`[[model]]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolModel {
    /// "qwen3.8-27b" (a-z, 0-9, '.', '-', '_').
    pub id: String,
    pub kind: SystemKind,
    pub name: String,
    pub adapter: AdapterId,
    pub license: String,
    #[serde(default)]
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_total_b: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_active_b: Option<f64>,
    #[serde(default)]
    pub moe: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx_max: Option<u32>,
    pub hf_repo: String,
    pub revision: String,
    /// Arg template: {ctx} and the -ctk / -ctv / -fitt values are fitted when a suggestion is adopted.
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The preset's health check ("/health", "tcp"), when the adapter's default does not fit the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// LLM: the KV cache cost (required for suggestions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv: Option<KvCost>,
    /// Compute buffers and the like, MiB.
    #[serde(default)]
    pub overhead_mib: f64,
    #[serde(default, rename = "quant")]
    pub quants: Vec<PoolQuant>,
}

impl PoolModel {
    /// The share of a MoE's weights that stays on the GPU when its experts go to RAM: active / total parameters
    /// (an upper bound: the active count includes the routed experts in use). 1.0 when unknown or dense.
    pub fn dense_share(&self) -> f64 {
        match (self.moe, self.params_active_b, self.params_total_b) {
            (true, Some(a), Some(t)) if a > 0.0 && t > 0.0 && a < t => a / t,
            _ => 1.0,
        }
    }
}

/// `[tiers]`: model ids per slot, in preference order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Tiers {
    pub fast: Vec<String>,
    pub deep: Vec<String>,
    pub max: Vec<String>,
    pub image: Vec<String>,
    pub tts: Vec<String>,
    pub stt: Vec<String>,
    pub video: Vec<String>,
}

impl Tiers {
    pub fn of(&self, slot: SuggestSlot) -> &[String] {
        match slot {
            SuggestSlot::Fast => &self.fast,
            SuggestSlot::Deep => &self.deep,
            SuggestSlot::Max => &self.max,
            SuggestSlot::Image => &self.image,
            SuggestSlot::Tts => &self.tts,
            SuggestSlot::Stt => &self.stt,
            SuggestSlot::Video => &self.video,
        }
    }

    fn of_mut(&mut self, slot: SuggestSlot) -> &mut Vec<String> {
        match slot {
            SuggestSlot::Fast => &mut self.fast,
            SuggestSlot::Deep => &mut self.deep,
            SuggestSlot::Max => &mut self.max,
            SuggestSlot::Image => &mut self.image,
            SuggestSlot::Tts => &mut self.tts,
            SuggestSlot::Stt => &mut self.stt,
            SuggestSlot::Video => &mut self.video,
        }
    }

    /// The first LLM tier that lists a model (its recommendations' class).
    pub fn class_of(&self, model: &str) -> Option<LlmClass> {
        [(LlmClass::Fast, &self.fast), (LlmClass::Deep, &self.deep), (LlmClass::Max, &self.max)]
            .into_iter()
            .find(|(_, l)| l.iter().any(|m| m == model))
            .map(|(c, _)| c)
    }
}

/// Every slot in suggestion order.
pub const SLOTS: [SuggestSlot; 7] =
    [SuggestSlot::Fast, SuggestSlot::Deep, SuggestSlot::Max, SuggestSlot::Image, SuggestSlot::Tts, SuggestSlot::Stt, SuggestSlot::Video];

/// `[budget]`: how much of the machine each tier may use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Budget {
    /// GiB left free on every counted GPU.
    pub gpu_margin_gib: f64,
    pub fast: FastBudget,
    pub max: MaxBudget,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { gpu_margin_gib: 1.0, fast: FastBudget::default(), max: MaxBudget::default() }
    }
}

/// System 1 leaves more than these shares of the VRAM pool and of the RAM free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FastBudget {
    pub vram_free_frac: f64,
    pub ram_free_frac: f64,
}

impl Default for FastBudget {
    fn default() -> FastBudget {
        FastBudget { vram_free_frac: 1.0 / 3.0, ram_free_frac: 0.25 }
    }
}

/// System 3 (and RAM offload elsewhere) keeps max(ram_reserve_gib, ram_reserve_frac x RAM) for the OS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MaxBudget {
    pub ram_reserve_gib: f64,
    pub ram_reserve_frac: f64,
}

impl Default for MaxBudget {
    fn default() -> MaxBudget {
        MaxBudget { ram_reserve_gib: 8.0, ram_reserve_frac: 0.10 }
    }
}

/// `[floors]`: nothing below these is suggested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Floors {
    pub ctx: CtxFloors,
    /// "q8_0" (f16 or q8_0 offered) or "f16" (f16 only).
    pub kv_min: String,
    pub dense_min_quality: f64,
    pub moe_min_quality: f64,
    /// The MoE floor applies from this many total parameters (billions); a smaller MoE (Gemma 26B-A4B) degrades like
    /// a dense model at 2 bits and keeps the dense floor.
    pub moe_floor_from_b: f64,
}

impl Default for Floors {
    fn default() -> Floors {
        Floors { ctx: CtxFloors::default(), kv_min: "q8_0".into(), dense_min_quality: 3.0, moe_min_quality: 2.0, moe_floor_from_b: 100.0 }
    }
}

/// The smallest context per LLM tier (capped by the model's ctx_max).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CtxFloors {
    pub fast: u32,
    pub deep: u32,
    pub max: u32,
}

impl Default for CtxFloors {
    fn default() -> CtxFloors {
        CtxFloors { fast: 32768, deep: 65536, max: 131072 }
    }
}

impl CtxFloors {
    pub fn of(&self, class: Option<LlmClass>) -> u32 {
        match class {
            Some(LlmClass::Deep) => self.deep,
            Some(LlmClass::Max) => self.max,
            _ => self.fast,
        }
    }
}

/// `[choice]`: how the suggester trades quality for context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Choice {
    /// Quality is preferred up to this (the Q5 class), then context up to ctx_max, then quality again.
    pub quality_first: f64,
}

impl Default for Choice {
    fn default() -> Choice {
        Choice { quality_first: 5.5 }
    }
}

/// The parsed pool: models with their usable rungs, tiers, budgets, floors, and every rung as a recommendation.
#[derive(Debug, Clone, Default)]
pub struct Pool {
    pub models: Vec<PoolModel>,
    pub tiers: Tiers,
    pub budget: Budget,
    pub floors: Floors,
    pub choice: Choice,
    /// Every (model, rung) as a recommendation, in file order.
    pub recs: Vec<Recommendation>,
}

impl Pool {
    pub fn model(&self, id: &str) -> Option<&PoolModel> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The model and rung of a recommendation id (`<model>.<rung>`).
    pub fn find(&self, rec: &str) -> Option<(&PoolModel, &PoolQuant)> {
        let (m, q) = rec.rsplit_once('.')?;
        let model = self.model(m)?;
        Some((model, model.quants.iter().find(|x| x.id == q)?))
    }
}

/// The recommendation id of a rung.
pub fn rec_id(model: &str, quant: &str) -> String {
    format!("{model}.{quant}")
}

/// The embedded pool (parsed once per process). Rejected models and rungs are logged as warnings.
pub fn embedded_pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let (pool, warnings) = parse_pool(RECOMMENDATIONS_TOML);
        for w in warnings {
            log::warn!("recommendations.toml: {w}");
        }
        pool
    })
}

/// The embedded recommendations that passed the load rules.
pub fn embedded() -> &'static [Recommendation] {
    &embedded_pool().recs
}

/// Parse a pool file into its recommendations, and one sentence per rejected model or rung.
pub fn parse(text: &str) -> (Vec<Recommendation>, Vec<String>) {
    let (pool, warnings) = parse_pool(text);
    (pool.recs, warnings)
}

/// A `[table]` of the pool with defaults for what it leaves out.
fn section<T: Default + serde::de::DeserializeOwned>(doc: &toml::Table, key: &str, warnings: &mut Vec<String>) -> T {
    match doc.get(key) {
        None => T::default(),
        Some(v) => v.clone().try_into().unwrap_or_else(|e: toml::de::Error| {
            warnings.push(format!("[{key}] is ignored (defaults apply): {}", one_line(&e.to_string())));
            T::default()
        }),
    }
}

fn one_line(s: &str) -> String {
    s.trim().replace(['\r', '\n'], " ")
}

/// Parse a pool file (schema 2): the models and rungs that pass the load rules, and one sentence per rejected one.
pub fn parse_pool(text: &str) -> (Pool, Vec<String>) {
    let mut warnings = Vec::new();
    let mut pool = Pool::default();
    let doc: toml::Table = match toml::from_str(text) {
        Ok(d) => d,
        Err(e) => return (pool, vec![format!("does not parse: {}", one_line(&e.to_string()))]),
    };
    match doc.get("schema").and_then(|v| v.as_integer()) {
        Some(SCHEMA) => {}
        other => return (pool, vec![format!("schema {other:?} is not {SCHEMA}; every entry is ignored.")]),
    }
    pool.tiers = section(&doc, "tiers", &mut warnings);
    pool.budget = section(&doc, "budget", &mut warnings);
    pool.floors = section(&doc, "floors", &mut warnings);
    pool.choice = section(&doc, "choice", &mut warnings);
    let entries = match doc.get("model") {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => a.clone(),
        Some(_) => {
            warnings.push("\"model\" must be an array of tables ([[model]]).".into());
            Vec::new()
        }
    };
    for (i, mut entry) in entries.into_iter().enumerate() {
        let label = entry.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("#{}", i + 1));
        if entry.get("env").is_some() {
            warnings.push(format!("model {label} is skipped: env templates are not allowed."));
            continue;
        }
        // A rung with an env template is dropped before the model is read.
        if let Some(toml::Value::Array(qs)) = entry.get_mut("quant") {
            qs.retain(|q| {
                let bad = q.get("env").is_some();
                if bad {
                    let qid = q.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    warnings.push(format!("rung {label}.{qid} is skipped: env templates are not allowed."));
                }
                !bad
            });
        }
        let mut model: PoolModel = match entry.try_into() {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("model {label} is skipped: {}", one_line(&e.to_string())));
                continue;
            }
        };
        if let Err(e) = check_model(&model) {
            warnings.push(format!("model {label} is skipped: {e}"));
            continue;
        }
        if pool.models.iter().any(|m| m.id == model.id) {
            warnings.push(format!("model {label} is skipped: the id is used twice."));
            continue;
        }
        let class = if model.kind == SystemKind::Llm { pool.tiers.class_of(&model.id) } else { None };
        let mut kept: Vec<PoolQuant> = Vec::new();
        for q in std::mem::take(&mut model.quants) {
            let id = rec_id(&model.id, &q.id);
            if kept.iter().any(|k| k.id == q.id) {
                warnings.push(format!("rung {id} is skipped: the id is used twice."));
                continue;
            }
            let rec = check_quant(&model, &q).map(|()| expand(&model, &q, class, &pool.floors)).and_then(|r| check(&r).map(|()| r));
            match rec {
                Ok(r) => {
                    pool.recs.push(r);
                    kept.push(q);
                }
                Err(e) => warnings.push(format!("rung {id} is skipped: {e}")),
            }
        }
        if kept.is_empty() {
            warnings.push(format!("model {label} is skipped: it has no usable quant rung."));
            continue;
        }
        model.quants = kept;
        pool.models.push(model);
    }
    // Tier entries must name a model of the right kind that loaded.
    for slot in SLOTS {
        let want = slot_kind(slot);
        let models = &pool.models;
        let mut dropped = Vec::new();
        pool.tiers.of_mut(slot).retain(|id| {
            let ok = models.iter().any(|m| &m.id == id && m.kind == want);
            if !ok {
                dropped.push(id.clone());
            }
            ok
        });
        for id in dropped {
            warnings.push(format!("[tiers] {}: \"{id}\" is not a loaded {} model; it is left out.", slot_name(slot), want.as_str()));
        }
    }
    (pool, warnings)
}

/// The System kind a slot suggests for.
pub fn slot_kind(slot: SuggestSlot) -> SystemKind {
    match slot {
        SuggestSlot::Fast | SuggestSlot::Deep | SuggestSlot::Max => SystemKind::Llm,
        SuggestSlot::Image => SystemKind::Image,
        SuggestSlot::Tts => SystemKind::Tts,
        SuggestSlot::Stt => SystemKind::Stt,
        SuggestSlot::Video => SystemKind::Video,
    }
}

/// The LLM class of a slot.
pub fn slot_class(slot: SuggestSlot) -> Option<LlmClass> {
    match slot {
        SuggestSlot::Fast => Some(LlmClass::Fast),
        SuggestSlot::Deep => Some(LlmClass::Deep),
        SuggestSlot::Max => Some(LlmClass::Max),
        _ => None,
    }
}

/// "fast", "image", ...
pub fn slot_name(slot: SuggestSlot) -> &'static str {
    match slot {
        SuggestSlot::Fast => "fast",
        SuggestSlot::Deep => "deep",
        SuggestSlot::Max => "max",
        SuggestSlot::Image => "image",
        SuggestSlot::Tts => "tts",
        SuggestSlot::Stt => "stt",
        SuggestSlot::Video => "video",
    }
}

/// A pool model id: 1-64 of a-z, 0-9, '.', '-', '_', starting with a letter or digit, no "..", not ending in '.'.
fn valid_model_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'_'))
        && !id.contains("..")
        && !id.ends_with('.')
}

/// A rung id: 1-32 of a-z, 0-9, '-', '_', starting with a letter or digit (no '.': it ends the recommendation id).
fn valid_quant_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

/// A recommendation id: `<model>.<rung>`.
pub fn valid_rec_id(id: &str) -> bool {
    id.rsplit_once('.').is_some_and(|(m, q)| valid_model_id(m) && valid_quant_id(q))
}

/// No network / model-fetch or secret flags.
fn check_args(args: &[String]) -> Result<(), String> {
    for a in args {
        let flag = a.split_once('=').map(|(f, _)| f).unwrap_or(a);
        if NETWORK_FLAGS.contains(&flag) {
            return Err(format!("args contain the network flag {flag} (models come only from an explicit download)."));
        }
        if is_secret_flag(flag) {
            return Err(format!("args contain the secret flag {flag}."));
        }
    }
    Ok(())
}

/// The load rules for a pool model (its rungs are checked one by one). Err = one sentence.
fn check_model(m: &PoolModel) -> Result<(), String> {
    if !valid_model_id(&m.id) {
        return Err(format!("the id \"{}\" is not 1-64 characters a-z, 0-9, '.', '-' or '_'.", m.id));
    }
    if !valid_repo(&m.hf_repo) {
        return Err(format!("hf_repo \"{}\" is not \"owner/name\".", m.hf_repo));
    }
    if !valid_revision(&m.revision) {
        return Err("revision must be a 40-character commit sha.".into());
    }
    check_args(&m.args)?;
    if m.kind == SystemKind::Llm {
        let Some(kv) = &m.kv else { return Err("an llm model needs `kv` (the KV cache estimate).".into()) };
        let nums = [kv.f16_mib_per_1k, kv.fixed_mib, kv.state_mib, m.overhead_mib];
        if nums.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err("kv and overhead_mib must be numbers of 0 or more.".into());
        }
        if m.ctx_max.is_none_or(|c| c < 256) {
            return Err("an llm model needs ctx_max (256 or more).".into());
        }
    }
    Ok(())
}

/// The load rules for one rung of a model (before it is expanded).
fn check_quant(m: &PoolModel, q: &PoolQuant) -> Result<(), String> {
    if !valid_quant_id(&q.id) {
        return Err(format!("the rung id \"{}\" is not 1-32 characters a-z, 0-9, '-' or '_'.", q.id));
    }
    if !q.quality.is_finite() || q.quality <= 0.0 {
        return Err("quality must be a number above 0.".into());
    }
    match (&q.hf_repo, &q.revision) {
        (None, None) => {}
        (Some(r), Some(v)) => {
            if !valid_repo(r) {
                return Err(format!("hf_repo \"{r}\" is not \"owner/name\"."));
            }
            if !valid_revision(v) {
                return Err("revision must be a 40-character commit sha.".into());
            }
        }
        _ => return Err("a rung names its own hf_repo and revision together, or neither.".into()),
    }
    if m.kind == SystemKind::Llm {
        if let Some(f) = q.files.iter().find(|f| f.size.is_none()) {
            return Err(format!("file \"{}\" has no size (the memory estimate needs every size).", f.name));
        }
    } else if q.min_vram_gib.is_none_or(|v| !v.is_finite() || v <= 0.0) {
        return Err("a rung of this kind needs min_vram_gib (its memory estimate).".into());
    }
    for f in &q.files {
        if let Some(k) = &f.key {
            if k.is_empty() || k.contains(['{', '}', '/', '\\']) || q.files.iter().filter(|g| g.key.as_ref() == Some(k)).count() > 1 {
                return Err(format!("the key \"{k}\" of file \"{}\" is empty, not plain or used twice.", f.name));
            }
        }
    }
    Ok(())
}

/// A rung as a concrete recommendation.
fn expand(m: &PoolModel, q: &PoolQuant, class: Option<LlmClass>, floors: &Floors) -> Recommendation {
    // `{file:<key>}` in the model's template becomes `{file:<that file's name>}` for this rung.
    let args: Vec<String> = m
        .args
        .iter()
        .map(|a| {
            if !a.contains("{file:") {
                return a.clone();
            }
            expand_file_refs(a, |r| {
                let name = q.files.iter().find(|f| f.key.as_deref() == Some(r)).map(|f| f.name.as_str()).unwrap_or(r);
                format!("{{file:{name}}}")
            })
        })
        .collect();
    let gib = q.gib();
    let hardware_class = match m.kind {
        SystemKind::Llm if m.moe => format!("{gib:.1} GiB of files + KV (MoE: experts can stay in RAM)"),
        SystemKind::Llm => format!("{gib:.1} GiB of files + KV"),
        _ => {
            let mut s = format!("{:.1} GiB VRAM", q.min_vram_gib.unwrap_or(0.0));
            if let (Some(v), Some(r)) = (q.offload_vram_gib, q.offload_ram_gib) {
                s.push_str(&format!(", or {v:.1} GiB VRAM + {r:.1} GiB RAM with --offload-to-cpu"));
            }
            s.push_str(" (estimates)");
            s
        }
    };
    let ctx = (m.kind == SystemKind::Llm).then(|| floors.ctx.of(class).min(m.ctx_max.unwrap_or(u32::MAX)));
    let notes = match (m.notes.as_deref(), q.notes.as_deref()) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b).map(str::to_string),
    };
    Recommendation {
        id: rec_id(&m.id, &q.id),
        kind: m.kind,
        class,
        name: m.name.clone(),
        adapter: m.adapter,
        hf_repo: q.hf_repo.clone().unwrap_or_else(|| m.hf_repo.clone()),
        revision: q.revision.clone().unwrap_or_else(|| m.revision.clone()),
        files: q.files.clone(),
        quant: q.label.clone(),
        license: m.license.clone(),
        hardware_class,
        min_vram_gib: q.min_vram_gib,
        args,
        ctx,
        port: m.port,
        health: m.health.clone(),
        model_id: m.id.clone(),
        family: m.family.clone(),
        measured: q.measured.clone(),
        notes,
    }
}

/// The load rules for one recommendation. Err = one sentence.
pub fn check(rec: &Recommendation) -> Result<(), String> {
    if !valid_rec_id(&rec.id) {
        return Err(format!("the id \"{}\" is not <model>.<rung>.", rec.id));
    }
    if !valid_repo(&rec.hf_repo) {
        return Err(format!("hf_repo \"{}\" is not \"owner/name\".", rec.hf_repo));
    }
    if !valid_revision(&rec.revision) {
        return Err("revision must be a 40-character commit sha.".into());
    }
    if rec.files.is_empty() {
        return Err("it lists no files.".into());
    }
    for (i, f) in rec.files.iter().enumerate() {
        if file_segments(&f.name).is_none() {
            return Err(format!("file \"{}\" is not a plain relative path in the repo.", f.name));
        }
        if rec.files[..i].iter().any(|g| g.name == f.name) {
            return Err(format!("file \"{}\" is listed twice.", f.name));
        }
        if let Some(s) = &f.save_as {
            if file_segments(s).is_none() {
                return Err(format!("save_as \"{s}\" of file \"{}\" is not a plain relative path.", f.name));
            }
        }
        if let Some(r) = &f.repo {
            if !valid_repo(r) {
                return Err(format!("the repo \"{r}\" of file \"{}\" is not \"owner/name\".", f.name));
            }
            if f.revision.is_none() {
                return Err(format!("file \"{}\" names its own repo, so it needs its own revision (a 40-character commit sha).", f.name));
            }
        }
        if f.revision.as_deref().is_some_and(|r| !valid_revision(r)) {
            return Err(format!("the revision of file \"{}\" must be a 40-character commit sha.", f.name));
        }
        if let Some(s) = &f.sha256 {
            if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("file \"{}\" has a sha256 that is not 64 hex characters.", f.name));
            }
        }
        // Two files may not be installed at the same place.
        let target = (rec.repo_of(f), f.install_name().to_ascii_lowercase());
        if rec.files[..i].iter().any(|g| (rec.repo_of(g), g.install_name().to_ascii_lowercase()) == target) {
            return Err(format!("file \"{}\" would be installed where another file goes.", f.name));
        }
    }
    if rec.class.is_some() && rec.kind != SystemKind::Llm {
        return Err("class only applies to llm recommendations.".into());
    }
    check_args(&rec.args)?;
    for a in &rec.args {
        if let Some(name) = file_refs(a).into_iter().find(|n| rec.file(n).is_none()) {
            return Err(format!("args refer to {{file:{name}}}, which is not one of its files."));
        }
    }
    Ok(())
}

fn valid_revision(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `owner/name` with plain characters.
fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok =
        |s: &str| !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if ok(a) && ok(b))
}

/// The `/`-separated segments of a repo file path, when it is a plain relative path (no `..`, no drive, no
/// backslash, no empty segment).
fn file_segments(file: &str) -> Option<Vec<&str>> {
    if file.is_empty() || file.starts_with('/') || file.contains('\\') || file.contains(':') || file.contains('\0') {
        return None;
    }
    let segs: Vec<&str> = file.split('/').collect();
    let bad = |s: &str| s.is_empty() || s == "." || s == ".." || s.ends_with(' ') || s.ends_with('.');
    if segs.iter().any(|s| bad(s)) {
        return None;
    }
    Some(segs)
}

/// `[paths] models_dir` (None: downloads are refused until it is set). Relative values are under state_dir.
pub fn models_dir(cfg: &Config) -> Option<PathBuf> {
    cfg.models_dir()
}

/// Where a recommendation's file is installed: `<models_dir>\<repo with '/' -> "--">\<file>`, where repo is the
/// file's own `repo` (a listed file with one), else the entry's `hf_repo`, and `<file>` its `save_as` when it has
/// one. Shared by the catalog (`installed`, presets made from it) and the downloader, so all agree. None when
/// models_dir is unset or the repo / file name is not a plain relative path.
pub fn install_path(cfg: &Config, rec: &Recommendation, file: &str) -> Option<PathBuf> {
    let dir = models_dir(cfg)?;
    Some(dir.join(install_rel(rec, file)?))
}

/// The install path below models_dir (see [`install_path`]).
pub(crate) fn install_rel(rec: &Recommendation, file: &str) -> Option<PathBuf> {
    let spec = rec.file(file);
    let repo = spec.map(|f| rec.repo_of(f)).unwrap_or(&rec.hf_repo);
    if !valid_repo(repo) {
        return None;
    }
    let segs = file_segments(spec.map(RecFileSpec::install_name).unwrap_or(file))?;
    let mut p = PathBuf::from(repo.replace('/', "--"));
    for s in segs {
        p.push(s);
    }
    Some(p)
}
