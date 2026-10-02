//! The exporter's JSON (`klif-catalog/1`) and the typed, indexed form the rest of the crate works with.
//!
//! Only what KLIF needs is read; unknown fields are ignored, so a newer exporter stays compatible.

use anyhow::{anyhow, bail, Context, Result};
use klif_common::vm::Backend;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

pub(crate) const SCHEMA: &str = "klif-catalog/1";

// ---------------------------------------------------------------------------------------------
// Raw JSON
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawCatalog {
    pub schema: String,
    #[serde(default)]
    pub meta: RawMeta,
    #[serde(default)]
    pub roots: RawRoots,
    #[serde(default)]
    pub network: RawNetwork,
    #[serde(default)]
    pub hardware_values: Vec<String>,
    #[serde(default)]
    pub backend_values: Vec<String>,
    #[serde(default)]
    pub families: Vec<RawFamily>,
    #[serde(default)]
    pub cards: Vec<RawCard>,
    #[serde(default)]
    pub presets: Vec<RawPreset>,
    #[serde(default)]
    pub profiles: Vec<RawProfile>,
    #[serde(default)]
    pub defaults: RawDefaults,
    #[serde(default)]
    pub rules: RawRules,
    #[serde(default)]
    pub envelope: RawEnvelope,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMeta {
    #[serde(default)]
    pub launcher: RawHashed,
    #[serde(default)]
    pub gui_shell: RawHashed,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawHashed {
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawRoots {
    #[serde(default)]
    pub runtime_logs: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawNetwork {
    #[serde(default)]
    pub port_values: Vec<u16>,
    #[serde(default, rename = "cacheValuesMiB")]
    pub cache_values_mib: Vec<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawFamily {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawCtx {
    pub value: u32,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawCard {
    pub id: String,
    #[serde(default)]
    pub family_id: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub size_label: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub quant: String,
    #[serde(default)]
    pub file_size: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub is_sd_server: bool,
    #[serde(default)]
    pub is_dense_qwen27: bool,
    #[serde(default)]
    pub is_bonsai: bool,
    #[serde(default)]
    pub contexts: Vec<RawCtx>,
    #[serde(default)]
    pub default_context: Option<u32>,
    #[serde(default)]
    pub prompt_cache_caps: BTreeMap<String, u32>,
    #[serde(default)]
    pub bonsai_kv_by_context: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawProfile {
    pub key: String,
    pub card_id: String,
    pub backend: String,
    pub hardware: String,
    pub context: u32,
    #[serde(default)]
    pub context_label: String,
    pub script: String,
    #[serde(default)]
    pub args: Vec<(String, Value)>,
    #[serde(default)]
    pub binary: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(default)]
    pub orphan: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawPreset {
    pub id: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub hardware: String,
    #[serde(default)]
    pub context: u32,
    #[serde(default)]
    pub resolved: RawPresetResolved,
    #[serde(default)]
    pub applies: RawPresetApplies,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawPresetResolved {
    #[serde(default)]
    pub card_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawPresetApplies {
    #[serde(default, rename = "promptCacheMiB")]
    pub prompt_cache_mib: Option<u32>,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(default)]
    pub cache_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawDefaults {
    #[serde(default = "d_cache", rename = "promptCacheMiB")]
    pub prompt_cache_mib: u32,
    #[serde(default = "d_port")]
    pub server_port: u16,
    #[serde(default = "d_mode")]
    pub generation_mode: String,
    #[serde(default = "d_vision")]
    pub vision: String,
    #[serde(default = "d_kv")]
    pub cache_type: String,
}

fn d_cache() -> u32 {
    32768
}
fn d_port() -> u16 {
    7030
}
fn d_mode() -> String {
    "Thinking".into()
}
fn d_vision() -> String {
    "off".into()
}
fn d_kv() -> String {
    "q8_0".into()
}

impl Default for RawDefaults {
    fn default() -> Self {
        RawDefaults {
            prompt_cache_mib: d_cache(),
            server_port: d_port(),
            generation_mode: d_mode(),
            vision: d_vision(),
            cache_type: d_kv(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawRules {
    #[serde(default, rename = "generationModeOverrideCardIds_extracted")]
    pub generation_mode_override_card_ids: Vec<String>,
    #[serde(default, rename = "skipProfileArgsWhenNotSd_extracted")]
    pub skip_profile_args_when_not_sd: Vec<String>,
    #[serde(default, rename = "generationModeArgument")]
    pub generation_mode_argument: Option<String>,
    #[serde(default, rename = "generationModeValues")]
    pub generation_mode_values: Vec<String>,
    #[serde(default, rename = "kvValues")]
    pub kv_values: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawEnvelope {
    #[serde(default, rename = "fixedArgsBeforeScript_extracted")]
    pub fixed_args_before_script: Vec<String>,
    #[serde(default, rename = "sessionName")]
    pub session_name: RawSessionName,
    #[serde(default)]
    pub logs: RawLogs,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawSessionName {
    #[serde(default, rename = "format_extracted")]
    pub format: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawLogs {
    #[serde(default)]
    pub directory: String,
}

// ---------------------------------------------------------------------------------------------
// Typed form
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Ctx {
    pub value: u32,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Card {
    pub id: String,
    pub family_id: String,
    pub family: String,
    pub size_label: String,
    pub name: String,
    pub quant: String,
    pub file_size_label: String,
    pub is_image: bool,
    pub is_sd: bool,
    pub is_dense27: bool,
    pub is_bonsai: bool,
    pub contexts: Vec<Ctx>,
    pub default_context: u32,
    pub cache_caps: BTreeMap<u32, u32>,
    pub bonsai_kv: BTreeMap<u32, String>,
}

impl Card {
    pub fn ctx_label(&self, value: u32) -> Option<&str> {
        self.contexts.iter().find(|c| c.value == value).map(|c| c.label.as_str())
    }
    pub fn ctx_by_label(&self, label: &str) -> Option<u32> {
        self.contexts.iter().find(|c| c.label.eq_ignore_ascii_case(label)).map(|c| c.value)
    }
    pub fn offers(&self, ctx: u32) -> bool {
        self.contexts.iter().any(|c| c.value == ctx)
    }
    /// Flash-Next: the Qwen 3.8 MoE family (size label like "125B-A6B").
    pub fn is_flash_next(&self) -> bool {
        self.family_id == "qwen" && !self.is_dense27 && !self.is_bonsai && !self.is_image
    }
    pub fn is_gemma(&self) -> bool {
        self.family_id == "gemma"
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Profile {
    pub key: String,
    pub card_id: String,
    pub backend: String,
    pub hardware: String,
    pub context: u32,
    pub context_label: String,
    pub script: PathBuf,
    /// Arguments exactly as the launcher builds them (`[name, value]` pairs, in order). Values are the
    /// PowerShell `[string]` form (integers in decimal).
    pub args: Vec<(String, String)>,
    pub binary: PathBuf,
    pub model: PathBuf,
    pub label: String,
    pub server_port: u16,
    pub orphan: bool,
}

impl Profile {
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn arg_u32(&self, name: &str) -> Option<u32> {
        self.arg(name).and_then(|v| v.trim().parse().ok())
    }
    pub fn script_name(&self) -> String {
        self.script.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Preset {
    pub id: String,
    pub backend: String,
    pub hardware: String,
    pub context: u32,
    pub card_id: Option<String>,
    pub applies_cache: Option<u32>,
    pub applies_port: Option<u16>,
    pub applies_kv: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Defaults {
    pub prompt_cache_mib: u32,
    pub server_port: u16,
    pub generation_mode: String,
    pub vision: bool,
    pub cache_type: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Rules {
    pub generation_mode_override: Vec<String>,
    pub skip_when_not_sd: Vec<String>,
    pub generation_mode_arg: String,
    pub generation_modes: Vec<String>,
    pub kv_values: Vec<String>,
    pub fixed_args: Vec<String>,
    pub session_format: String,
}

/// Everything parsed from the exporter's JSON, indexed for the lookups the crate needs.
#[derive(Debug)]
pub(crate) struct Data {
    pub cards: Vec<Card>,
    pub card_index: HashMap<String, usize>,
    pub profiles: Vec<Profile>,
    pub profile_index: HashMap<String, usize>,
    /// Per card: the (backend, hardware) combinations that have at least one profile, in catalog order.
    pub combos: HashMap<String, Vec<(String, String)>>,
    pub presets: Vec<Preset>,
    pub families: Vec<(String, String)>,
    pub hardware_values: Vec<String>,
    pub backend_values: Vec<String>,
    /// The launcher lists its primary (inference) GPU first among the hardware values.
    pub primary_hardware: String,
    /// Hardware values that only run on the CPU (every profile is labelled "RAM only").
    pub cpu_hardware: Vec<String>,
    pub port_values: Vec<u16>,
    pub cache_values: Vec<u32>,
    pub defaults: Defaults,
    pub rules: Rules,
    pub runtime_logs: PathBuf,
    pub warnings: Vec<String>,
}

/// `[string]` of a JSON value, the way PowerShell renders the launcher's argument values.
fn ps_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

impl Data {
    pub(crate) fn from_raw(raw: RawCatalog) -> Result<Data> {
        if raw.schema != SCHEMA {
            bail!("catalog schema {:?} is not supported (expected {SCHEMA:?})", raw.schema);
        }
        let mut cards = Vec::with_capacity(raw.cards.len());
        for c in raw.cards {
            let contexts: Vec<Ctx> = c.contexts.into_iter().map(|x| Ctx { value: x.value, label: x.label }).collect();
            let default_context = c.default_context.or_else(|| contexts.first().map(|x| x.value)).unwrap_or(0);
            let cache_caps = c
                .prompt_cache_caps
                .into_iter()
                .filter_map(|(k, v)| k.parse::<u32>().ok().map(|k| (k, v)))
                .collect();
            let bonsai_kv = c
                .bonsai_kv_by_context
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(k, v)| k.parse::<u32>().ok().map(|k| (k, v)))
                .collect();
            let is_image = c.kind.eq_ignore_ascii_case("image") || c.kind.eq_ignore_ascii_case("video");
            cards.push(Card {
                id: c.id,
                family_id: c.family_id,
                family: c.family,
                size_label: c.size_label,
                name: c.name,
                quant: c.quant,
                file_size_label: c.file_size,
                is_image,
                is_sd: c.is_sd_server,
                is_dense27: c.is_dense_qwen27,
                is_bonsai: c.is_bonsai,
                contexts,
                default_context,
                cache_caps,
                bonsai_kv,
            });
        }
        let card_index: HashMap<String, usize> = cards.iter().enumerate().map(|(i, c)| (c.id.clone(), i)).collect();

        let mut profiles = Vec::with_capacity(raw.profiles.len());
        for p in raw.profiles {
            let args = p.args.into_iter().map(|(k, v)| (k, ps_string(&v))).collect();
            profiles.push(Profile {
                key: p.key,
                card_id: p.card_id,
                backend: p.backend,
                hardware: p.hardware,
                context: p.context,
                context_label: p.context_label,
                script: PathBuf::from(p.script),
                args,
                binary: PathBuf::from(p.binary),
                model: PathBuf::from(p.model),
                label: p.label,
                server_port: p.server_port.unwrap_or(7030),
                orphan: p.orphan,
            });
        }
        let profile_index: HashMap<String, usize> = profiles.iter().enumerate().map(|(i, p)| (p.key.clone(), i)).collect();
        if profile_index.len() != profiles.len() {
            bail!("catalog has duplicate profile keys");
        }
        if profiles.is_empty() || cards.is_empty() {
            bail!("catalog is empty (no cards or no profiles)");
        }
        let mut combos: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for p in &profiles {
            let list = combos.entry(p.card_id.clone()).or_default();
            let pair = (p.backend.clone(), p.hardware.clone());
            if !list.contains(&pair) {
                list.push(pair);
            }
        }

        let presets = raw
            .presets
            .into_iter()
            .map(|p| Preset {
                id: p.id,
                backend: p.backend,
                hardware: p.hardware,
                context: p.context,
                card_id: p.resolved.card_id,
                applies_cache: p.applies.prompt_cache_mib,
                applies_port: p.applies.server_port,
                applies_kv: p.applies.cache_type,
            })
            .collect();

        let d = raw.defaults;
        let defaults = Defaults {
            prompt_cache_mib: d.prompt_cache_mib,
            server_port: d.server_port,
            generation_mode: d.generation_mode,
            vision: d.vision.eq_ignore_ascii_case("on"),
            cache_type: d.cache_type,
        };

        let r = raw.rules;
        let e = raw.envelope;
        let rules = Rules {
            generation_mode_override: r.generation_mode_override_card_ids,
            skip_when_not_sd: if r.skip_profile_args_when_not_sd.is_empty() {
                vec!["Port".into(), "PromptCacheMiB".into()]
            } else {
                r.skip_profile_args_when_not_sd
            },
            generation_mode_arg: r.generation_mode_argument.unwrap_or_else(|| "GenerationMode".into()),
            generation_modes: if r.generation_mode_values.is_empty() {
                vec!["Thinking".into(), "Instruct".into()]
            } else {
                r.generation_mode_values
            },
            kv_values: if r.kv_values.is_empty() { vec!["q4_0".into(), "q8_0".into()] } else { r.kv_values },
            fixed_args: if e.fixed_args_before_script.is_empty() {
                ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].iter().map(|s| s.to_string()).collect()
            } else {
                e.fixed_args_before_script
            },
            session_format: e.session_name.format.unwrap_or_else(|| "klif-{0}-{1}-p{2}".into()),
        };

        let runtime_logs = if !e.logs.directory.is_empty() {
            PathBuf::from(&e.logs.directory)
        } else {
            PathBuf::from(&raw.roots.runtime_logs)
        };

        let mut port_values = raw.network.port_values;
        if port_values.is_empty() {
            port_values = (7030..=7035).collect();
        }
        let mut cache_values = raw.network.cache_values_mib;
        if cache_values.is_empty() {
            cache_values = vec![2048, 8192, 16384, 32768];
        }

        let primary_hardware = raw
            .hardware_values
            .first()
            .cloned()
            .or_else(|| profiles.first().map(|p| p.hardware.clone()))
            .unwrap_or_default();
        let cpu_hardware: Vec<String> = raw
            .hardware_values
            .iter()
            .filter(|h| {
                let mut any = false;
                let all = profiles.iter().filter(|p| &p.hardware == *h).all(|p| {
                    any = true;
                    p.label.to_ascii_lowercase().contains("ram only")
                });
                any && all
            })
            .cloned()
            .collect();

        Ok(Data {
            cards,
            card_index,
            profiles,
            profile_index,
            combos,
            presets,
            families: raw.families.into_iter().map(|f| (f.id, f.label)).collect(),
            hardware_values: raw.hardware_values,
            backend_values: raw.backend_values,
            primary_hardware,
            cpu_hardware,
            port_values,
            cache_values,
            defaults,
            rules,
            runtime_logs,
            warnings: raw.meta.warnings,
        })
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<(RawHashes, Data)> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let raw: RawCatalog = serde_json::from_slice(bytes).context("parsing catalog json")?;
        let hashes = RawHashes { launcher: raw.meta.launcher.sha256.clone(), gui: raw.meta.gui_shell.sha256.clone() };
        let data = Data::from_raw(raw).map_err(|e| anyhow!("{e:#}"))?;
        Ok((hashes, data))
    }

    pub(crate) fn card(&self, id: &str) -> Option<&Card> {
        self.card_index.get(id).map(|&i| &self.cards[i])
    }

    pub(crate) fn profile_by_key(&self, key: &str) -> Option<&Profile> {
        self.profile_index.get(key).map(|&i| &self.profiles[i])
    }

    pub(crate) fn family_label(&self, id: &str) -> Option<&str> {
        self.families.iter().find(|(i, _)| i == id).map(|(_, l)| l.as_str())
    }

    pub(crate) fn is_gen_override(&self, card_id: &str) -> bool {
        self.rules.generation_mode_override.iter().any(|c| c == card_id)
    }

    pub(crate) fn backend_str(b: Backend) -> &'static str {
        match b {
            Backend::Hip => "HIP",
            Backend::Vulkan => "Vulkan",
            Backend::Cpu => "CPU",
        }
    }

    pub(crate) fn backend_of(s: &str) -> Option<Backend> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hip" => Some(Backend::Hip),
            "vulkan" => Some(Backend::Vulkan),
            "cpu" => Some(Backend::Cpu),
            _ => None,
        }
    }
}

/// Hashes recorded by the exporter in the JSON (compared with the scripts' own SHA-256).
#[derive(Debug, Clone)]
pub(crate) struct RawHashes {
    pub launcher: String,
    pub gui: String,
}
