//! KLIF configuration (`klif.toml`, 0.3). Machine-specific values live ONLY in the user's config file, never
//! in tracked code. See `config/klif.example.toml` for the documented template.
//!
//! Lookup ([`locate`]):
//!   1. env `KLIF_CONFIG` (path to a toml file; it may not exist yet)
//!   2. `.local/klif.toml`, searched upward from the executable's directory
//!   3. `.local/klif.toml`, searched upward from the current directory
//!   4. `%APPDATA%\KLIF\klif.toml` (elsewhere `$XDG_CONFIG_HOME/klif/klif.toml`); the folder is created, the
//!      file is not required.
//!
//! `state_dir` = the folder of that klif.toml (state.json, engine.lock, control.json, api-key.txt, node-token.txt).
//! `data_dir` = `state_dir`, except for the default `%APPDATA%\KLIF`, whose data goes to `%LOCALAPPDATA%\KLIF`
//! (logs, bench, webview-data, downloads).
//!
//! [`load`] never fails: an unreadable file gives `Config::empty_at(path)` plus an error [`Issue`] (field
//! [`FILE_FIELD`]). Every section is read on its own (a bad `[net]` falls back to its defaults with an issue), and
//! `[systems.*]`, `[presets.*]` and `[nodes.*]` are read entry by entry, so one bad entry lands in `bad_systems` /
//! `bad_presets` / `bad_nodes` and the rest of the file still works. Sections 0.3 does not know (`[launcher]`,
//! `[krea]`) are ignored, so one file can serve 0.2 and 0.3 during the migration. `toml` runs with
//! `preserve_order`: the System tabs follow the file order of `[systems.*]`.
//!
//! Fallback (SPEC 2.1): a file with NO `[systems]` table but 0.2 `[tiers.low|medium|high|krea]` gets the Systems
//! `s1` "System 1" (llm, fast), `s2` "System 2" (llm, deep), `s3` "System 3" (llm, max), `cgi` "System CGI" (image)
//! for the tiers present, with their `preset` (absent = not set).

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::secret::{is_secret_env, mask_args, secret_arg_values, MASK};
use crate::vm::{AdapterId, Issue, LlmClass, SystemId, SystemKind};

/// The config file name.
pub const FILE_NAME: &str = "klif.toml";
/// `Issue.field` of a whole-file problem (unreadable / does not parse).
pub const FILE_FIELD: &str = "file";
/// Default port of the node listener (`[node] listen`, `[nodes.X] address`).
pub const NODE_PORT: u16 = 7340;

// ------------------------------------------------------------------------------------------ types

/// The result of loading: always a usable config, plus what was wrong with the file.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub cfg: Config,
    pub issues: Vec<Issue>,
}

impl LoadedConfig {
    /// True when the file itself could not be read or parsed (the config is then `Config::empty_at`). The
    /// engine keeps its last good config in that case.
    pub fn unreadable(&self) -> bool {
        self.issues.iter().any(|i| i.is_error() && i.field.as_deref() == Some(FILE_FIELD))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub net: NetCfg,
    pub gpu: GpuCfg,
    pub ui: UiCfg,
    pub telemetry: TelemetryCfg,
    pub paths: PathsCfg,
    pub security: SecurityCfg,
    pub launch: LaunchCfg,
    /// `[node]`: this machine as a node others connect to (None = not reachable from the network).
    pub node: Option<NodeCfg>,
    /// `[nodes.<id>]`: remote nodes shown here, in file order.
    pub nodes: Vec<(String, RemoteNodeCfg)>,
    /// `[systems.<id>]` in file order (or the `[tiers]` fallback).
    pub systems: Vec<(SystemId, SystemCfg)>,
    /// `[presets.<id>]` entries that parsed.
    pub presets: BTreeMap<String, PresetCfg>,
    /// Entries that did not parse (or have an invalid id): id -> error sentence.
    pub bad_presets: BTreeMap<String, String>,
    pub bad_systems: BTreeMap<String, String>,
    pub bad_nodes: BTreeMap<String, String>,
    /// The klif.toml this config belongs to: the located file, or `KLIF_CONFIG` even before it exists.
    /// None = the default location and no file there yet (see [`Config::file_path`]).
    #[serde(skip)]
    pub source: Option<PathBuf>,
    #[serde(skip)]
    pub state_dir: PathBuf,
    #[serde(skip)]
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetCfg {
    /// Default host for presets of every kind but image (binding and probes).
    pub llm_host: String,
    /// Default host for image presets.
    pub image_host: String,
}

impl Default for NetCfg {
    fn default() -> Self {
        Self { llm_host: "127.0.0.1".into(), image_host: "127.0.0.1".into() }
    }
}

impl NetCfg {
    /// The default host for a kind of server: image -> image_host, others -> llm_host.
    pub fn host_for(&self, kind: SystemKind) -> &str {
        match kind {
            SystemKind::Image => &self.image_host,
            _ => &self.llm_host,
        }
    }
}

/// GPUs by PCI "VEN:DEV" (hex), resolved to adapters at runtime (LUIDs change every boot).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuCfg {
    /// The inference card, and the default GPU of presets without `gpu`. Example: "1002:7550".
    pub inference: Option<String>,
    /// The adapter the UI must render on. None = first hardware adapter that is not the inference card.
    pub ui: Option<String>,
    /// Display name override for the inference card, e.g. "RX 9070 XT".
    pub inference_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiCfg {
    /// Frameless window with skin-drawn chrome.
    pub frameless: bool,
    /// Which monitor "Panel mode" fills: "WxH" (e.g. "960x640") or a substring of the monitor's device name.
    /// Unset = automatic (the smallest of two or more monitors, if at most 1280x800).
    pub panel_monitor: Option<String>,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self { frameless: true, panel_monitor: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryCfg {
    /// Free VRAM below which skins warn.
    pub warn_below_gib: f64,
    /// llama.cpp presets with `managed = true` get `LLAMA_ARG_LOG_VERBOSITY=4` (VRAM composition in the log).
    pub verbose_llama_logs: bool,
    /// Display override for the RAM type, e.g. "DDR5" (KLIF does not read firmware tables in the GUI).
    pub ram_type: Option<String>,
}

impl Default for TelemetryCfg {
    fn default() -> Self {
        Self { warn_below_gib: 0.15, verbose_llama_logs: true, ram_type: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsCfg {
    /// Where recommended models are downloaded to. No default: downloads are refused until it is set.
    pub models_dir: Option<PathBuf>,
    /// Session logs. Default `<data_dir>\logs`.
    pub logs_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityCfg {
    /// "file" (default: `<state_dir>\api-key.txt`) | "env:NAME" | "none".
    pub api_key: String,
}

impl Default for SecurityCfg {
    fn default() -> Self {
        Self { api_key: "file".into() }
    }
}

/// `[security] api_key`, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeySource {
    /// `<state_dir>\api-key.txt` (trimmed).
    File,
    /// An environment variable of KLIF's own process.
    Env(String),
    /// No key: servers run without auth (non-loopback hosts are then the user's call).
    None,
}

impl SecurityCfg {
    /// The parsed source; an unknown value counts as "file" (load reports it as an issue).
    pub fn api_key_source(&self) -> ApiKeySource {
        parse_api_key_source(&self.api_key).unwrap_or(ApiKeySource::File)
    }
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_api_key_source(s: &str) -> Option<ApiKeySource> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("file") {
        return Some(ApiKeySource::File);
    }
    if t.eq_ignore_ascii_case("none") {
        return Some(ApiKeySource::None);
    }
    let (head, name) = t.split_once(':')?;
    let name = name.trim();
    (head.trim().eq_ignore_ascii_case("env") && valid_env_name(name)).then(|| ApiKeySource::Env(name.to_string()))
}

impl ApiKeySource {
    /// The `ApiKeyInfo.source` string: "file" | "env:NAME" | "none".
    pub fn as_info(&self) -> String {
        match self {
            ApiKeySource::File => "file".into(),
            ApiKeySource::Env(n) => format!("env:{n}"),
            ApiKeySource::None => "none".into(),
        }
    }
}

/// `[launch] on_conflict`: what Launch does when other Systems must stop first (SPEC section 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    /// Refuse with a sentence naming the conflicts (the UI offers "Stop X & Launch").
    #[default]
    Ask,
    /// Stop the conflicting Systems first, then launch.
    Stop,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchCfg {
    pub on_conflict: OnConflict,
}

/// A right a node grants remote clients besides viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRight {
    /// launch / stop / stopAll / restart / dismiss / usePreset / setParam.
    Launch,
    /// Presets, Systems, downloads: arbitrary command execution on this machine.
    Edit,
}

impl NodeRight {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeRight::Launch => "launch",
            NodeRight::Edit => "edit",
        }
    }
}

/// `[node]`: this machine as a node others can connect to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeCfg {
    /// Display name (default: the computer name).
    pub name: Option<String>,
    /// "0.0.0.0:7340" (explicit opt-in; without it the node does not listen).
    pub listen: Option<String>,
    /// Rights besides view.
    pub allow: Vec<NodeRight>,
}

impl NodeCfg {
    /// The address to listen on with the default port added ("0.0.0.0" -> "0.0.0.0:7340"); None = do not listen.
    pub fn listen_addr(&self) -> Option<String> {
        let l = self.listen.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
        Some(with_default_port(l, NODE_PORT))
    }

    pub fn allows(&self, right: NodeRight) -> bool {
        self.allow.contains(&right)
    }
}

/// `"host"` -> `"host:port"`; `"host:port"` and `"[v6]:port"` unchanged; a bare IPv6 gets brackets.
fn with_default_port(addr: &str, port: u16) -> String {
    if let Some(rest) = addr.strip_prefix('[') {
        return if rest.contains("]:") { addr.to_string() } else { format!("{addr}:{port}") };
    }
    match addr.matches(':').count() {
        0 => format!("{addr}:{port}"),
        1 => addr.to_string(),
        _ => format!("[{addr}]:{port}"),
    }
}

/// `[nodes.<id>]`: a remote KLIF node shown here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteNodeCfg {
    /// "192.0.2.10:7340" (default port 7340).
    pub address: String,
    /// "file:<path relative to state_dir or absolute>" | "env:NAME".
    pub token: String,
    /// Display; default: what the node reports.
    pub name: Option<String>,
}

/// Where a remote node's token comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenRef {
    File(PathBuf),
    Env(String),
}

impl RemoteNodeCfg {
    /// `address` with the default port added.
    pub fn address_with_port(&self) -> String {
        with_default_port(self.address.trim(), NODE_PORT)
    }

    /// The parsed `token` reference; Err = one sentence.
    pub fn token_ref(&self) -> Result<TokenRef, String> {
        let t = self.token.trim();
        if let Some(p) = t.strip_prefix("file:") {
            let p = p.trim();
            if !p.is_empty() {
                return Ok(TokenRef::File(PathBuf::from(p)));
            }
        } else if let Some(n) = t.strip_prefix("env:") {
            let n = n.trim();
            if valid_env_name(n) {
                return Ok(TokenRef::Env(n.to_string()));
            }
        }
        // Never echo the value: it may be the token itself, typed into the wrong field.
        Err("token must be \"file:<path>\" or \"env:NAME\"; put the token in that file or variable, never in klif.toml.".into())
    }
}

impl TokenRef {
    /// A file reference resolved against `state_dir` when relative.
    pub fn resolve(&self, state_dir: &Path) -> TokenRef {
        match self {
            TokenRef::File(p) if p.is_relative() => TokenRef::File(state_dir.join(p)),
            other => other.clone(),
        }
    }
}

/// `[systems.<id>]`: one tab, one server at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemCfg {
    /// Tab label (default: derived from kind / class, see [`default_system_label`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Required.
    pub kind: SystemKind,
    /// LLM only (recommendation grouping, default labels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    /// The active preset id (absent = not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Selected param choices of the active preset: NAME -> choice value (unknown / absent -> the default).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// Needs the whole GPU: launching it lists every other running System on that GPU as a conflict, and vice versa.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclusive: bool,
}

impl SystemCfg {
    /// A System of a kind with nothing else set.
    pub fn new(kind: SystemKind) -> SystemCfg {
        SystemCfg { label: None, kind, class: None, preset: None, params: BTreeMap::new(), exclusive: false }
    }

    /// The active preset id (trimmed; None when unset or empty).
    pub fn preset_id(&self) -> Option<&str> {
        self.preset.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
}

/// The keys `[systems.<id>]` understands.
pub const SYSTEM_KEYS: &[&str] = &["label", "kind", "class", "preset", "params", "exclusive"];

/// `[presets.<id>.params.<NAME>]`: one independent launch option (no Cartesian product of presets).
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamCfg {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The choice used when the System selects none (else the first choice in the file).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// In file order (the first one is the default when `default` is absent; the UI lists them in this order).
    /// `PartialEq` ignores the order.
    pub choices: IndexMap<String, ParamChoice>,
}

impl ParamCfg {
    /// The effective default choice value: `default` when it names a choice, else the first choice in the file.
    pub fn default_choice(&self) -> Option<&str> {
        match self.default.as_deref() {
            Some(d) if self.choices.contains_key(d) => Some(d),
            _ => self.choices.keys().next().map(String::as_str),
        }
    }
}

/// `choices.<value> = { label?, vars = {k=v}, args = [..], env = {k=v} }`. `{p.NAME}` (a whole arg token)
/// expands to `args`; `{p.NAME.VAR}` to `vars.VAR`; `env` merges into the preset env.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamChoice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// `[presets.<id>]`: one launchable server command, or (with `endpoint`) an external server KLIF only watches.
/// serde snake_case on purpose (mirrors klif.toml); the TS `PresetSpec` uses the same keys. `Default` is written by
/// hand: `managed` and `api_key` default to TRUE and the adapter to llama.cpp, also for every field missing from a
/// file or a UI draft (`#[serde(default)]`).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetCfg {
    /// Display name (default: the id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub adapter: AdapterId,
    /// Default: image for sd.cpp, llm for llama.cpp / vllm / openai; REQUIRED for generic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<SystemKind>,
    /// Absolute path, or a name on PATH (.exe/.com only). Empty for an external preset.
    pub command: String,
    pub args: Vec<String>,
    /// Default: the folder of the resolved command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub env_remove: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Default: `[net]` host by kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// EXTERNAL server URL, e.g. "http://192.0.2.20:11434": KLIF never starts / stops it (command must be empty).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// "/path" = HTTP, "tcp" = TCP listen; absent = the adapter's default chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmproj: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u32>,
    /// The GPU it runs on, for telemetry / fit: "VEN:DEV" | "cpu"; default `[gpu] inference`. Display and
    /// telemetry only: the command itself (env / args) must select the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// KLIF adds the adapter's telemetry env (default true).
    pub managed: bool,
    /// KLIF injects its API key into the adapter's key env var (default true).
    pub api_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    /// Display: HIP | Vulkan | CUDA | Metal | CPU | free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Display; default: the GPU's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The recommendation id it was made from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended: Option<String>,
    /// In file order (the UI lists them in this order). `PartialEq` ignores the order.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, ParamCfg>,
}

impl Default for PresetCfg {
    fn default() -> Self {
        PresetCfg {
            name: None,
            adapter: AdapterId::LlamaCpp,
            kind: None,
            command: String::new(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            port: None,
            host: None,
            endpoint: None,
            health: None,
            model: None,
            mmproj: None,
            ctx: None,
            gpu: None,
            managed: true,
            api_key: true,
            model_name: None,
            quant: None,
            backend: None,
            device: None,
            notes: None,
            recommended: None,
            params: IndexMap::new(),
        }
    }
}

/// The keys `[presets.<id>]` understands (anything else is reported as a warning, or as an error when it is a
/// table: `[presets.gemma-4.26b]` is a dotted key, i.e. a preset "gemma-4" with a sub-table "26b").
pub const PRESET_KEYS: &[&str] = &[
    "name", "adapter", "kind", "command", "args", "cwd", "env", "env_remove", "port", "host", "endpoint", "health", "model",
    "mmproj", "ctx", "gpu", "managed", "api_key", "model_name", "quant", "backend", "device", "notes", "recommended", "params",
];

impl PresetCfg {
    /// `name`, else the id.
    pub fn display_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(id)
    }

    /// `kind`, else the adapter's default kind (None: a generic preset without `kind`, an error).
    pub fn effective_kind(&self) -> Option<SystemKind> {
        self.kind.or(self.adapter.default_kind())
    }

    /// An external server (`endpoint` set): watched, never started or stopped.
    pub fn is_external(&self) -> bool {
        self.endpoint.as_deref().is_some_and(|e| !e.trim().is_empty())
    }

    /// A copy safe to show: every env value whose name is secret (`is_secret_env`) and every secret arg value
    /// (`SECRET_ARG_FLAGS`, also inside params) becomes [`MASK`]. So does every param var that feeds a secret
    /// position (`{p.NAME.VAR}` in the value after a secret flag or in a secret env value) or whose own name
    /// looks secret, and the first arg of every choice of a `{p.NAME}` token that follows a secret flag.
    /// `klif_catalog::store::upsert_preset` resolves each MASK back to the stored value. Used for
    /// `PresetDetail.spec` (and its `spec_hash`), Debug output and diag.
    pub fn masked(&self) -> PresetCfg {
        // Scan the unmasked values first: masking rewrites `--api-key={p.auth.key}` and loses the reference.
        let (secret_vars, secret_params) = self.secret_param_refs();
        let mut p = self.clone();
        mask_env(&mut p.env);
        p.args = mask_args(&p.args);
        for (pname, param) in p.params.iter_mut() {
            for c in param.choices.values_mut() {
                mask_env(&mut c.env);
                c.args = mask_args(&c.args);
                if secret_params.contains(pname.as_str()) {
                    if let Some(first) = c.args.first_mut() {
                        *first = MASK.to_string();
                    }
                }
                for (var, v) in c.vars.iter_mut() {
                    if is_secret_env(var) || secret_vars.contains(&(pname.as_str(), var.as_str())) {
                        *v = MASK.to_string();
                    }
                }
            }
        }
        p
    }

    /// Params whose values land in a secret position of the preset's own args / env: (`{p.NAME.VAR}` refs as
    /// (NAME, VAR), `{p.NAME}` tokens right after a secret flag as NAME). Choices cannot refer to params, so
    /// the preset-level args and env are all there is to scan.
    fn secret_param_refs(&self) -> (BTreeSet<(&str, &str)>, BTreeSet<&str>) {
        let mut vars = BTreeSet::new();
        let mut whole = BTreeSet::new();
        let secret_env = self.env.iter().filter(|(k, _)| is_secret_env(k)).map(|(_, v)| v.as_str());
        for value in secret_arg_values(&self.args).into_iter().chain(secret_env) {
            let t = value.trim();
            if let Some(name) = t.strip_prefix("{p.").and_then(|r| r.strip_suffix('}')).filter(|n| !n.is_empty() && !n.contains(['.', '{'])) {
                whole.insert(name);
            }
            vars.extend(param_var_refs(value));
        }
        (vars, whole)
    }
}

/// Every `{p.NAME.VAR}` in `s` as (NAME, VAR).
fn param_var_refs(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("{p.") {
        let after = &rest[i + 3..];
        let Some(j) = after.find('}') else { break };
        if let Some((name, var)) = after[..j].split_once('.') {
            if !name.is_empty() && !var.is_empty() && !var.contains('.') {
                out.push((name, var));
            }
        }
        rest = &after[j + 1..];
    }
    out
}

fn mask_env(env: &mut BTreeMap<String, String>) {
    for (k, v) in env.iter_mut() {
        if is_secret_env(k) {
            *v = MASK.to_string();
        }
    }
}

/// Debug output for logs: env VALUES are never printed (names only), nor the value after a secret flag.
struct EnvNames<'a>(&'a BTreeMap<String, String>);

impl fmt::Debug for EnvNames<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.keys().map(|k| (k, MASK))).finish()
    }
}

struct MaskedArgs<'a>(&'a [String]);

impl fmt::Debug for MaskedArgs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let masked = mask_args(self.0);
        f.debug_list().entries(masked.iter()).finish()
    }
}

impl fmt::Debug for PresetCfg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PresetCfg")
            .field("name", &self.name)
            .field("adapter", &self.adapter)
            .field("kind", &self.kind)
            .field("command", &self.command)
            .field("args", &MaskedArgs(&self.args))
            .field("cwd", &self.cwd)
            .field("env", &EnvNames(&self.env))
            .field("env_remove", &self.env_remove)
            .field("port", &self.port)
            .field("host", &self.host)
            .field("endpoint", &self.endpoint)
            .field("health", &self.health)
            .field("model", &self.model)
            .field("mmproj", &self.mmproj)
            .field("ctx", &self.ctx)
            .field("gpu", &self.gpu)
            .field("managed", &self.managed)
            .field("api_key", &self.api_key)
            .field("model_name", &self.model_name)
            .field("quant", &self.quant)
            .field("backend", &self.backend)
            .field("device", &self.device)
            .field("notes", &self.notes)
            .field("recommended", &self.recommended)
            // Masked like PresetDetail: a var that feeds a secret flag is hidden too.
            .field("params", &self.masked().params)
            .finish()
    }
}

impl fmt::Debug for ParamCfg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParamCfg").field("label", &self.label).field("default", &self.default).field("choices", &self.choices).finish()
    }
}

impl fmt::Debug for ParamChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A var with a secret-looking name is hidden here; PresetCfg's Debug also hides vars that feed a secret flag.
        let vars: BTreeMap<&str, &str> = self.vars.iter().map(|(k, v)| (k.as_str(), if is_secret_env(k) { MASK } else { v.as_str() })).collect();
        f.debug_struct("ParamChoice")
            .field("label", &self.label)
            .field("vars", &vars)
            .field("args", &MaskedArgs(&self.args))
            .field("env", &EnvNames(&self.env))
            .finish()
    }
}

// --------------------------------------------------------------------------------------------- ids

/// Windows device names a preset id (it becomes a file name: `bench\<id>.json`) must not be.
const RESERVED_IDS: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// `[a-z0-9][a-z0-9_-]{0,63}` and not a Windows reserved device name. Err = one sentence.
pub fn validate_preset_id(id: &str) -> Result<(), String> {
    let b = id.as_bytes();
    let ok_first = b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let ok_rest = b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-');
    if !ok_first || !ok_rest || b.len() > 64 {
        return Err(format!(
            "Preset id \"{id}\" is not allowed: use 1-64 characters a-z, 0-9, '-' or '_', starting with a letter or digit."
        ));
    }
    if RESERVED_IDS.contains(&id) {
        return Err(format!("Preset id \"{id}\" is a reserved Windows device name; choose another id."));
    }
    Ok(())
}

/// A local System id or node id: `[a-z0-9][a-z0-9_-]{0,31}` (no "/"). Err = one sentence.
pub fn validate_system_id(id: &str) -> Result<(), String> {
    SystemId::validate_local(id)
}

/// The default label of a new System (SPEC 2.2): llm fast/deep/max -> "System 1/2/3", llm without class -> the
/// next free "System N"; image -> "System CGI"; tts -> "System TTS"; stt -> "System STT"; video -> "System Video".
/// A label already in `taken` gets " (2)", " (3)"... appended ("System 1 (2)", "System TTS (2)"; compared ignoring
/// case).
pub fn default_system_label(kind: SystemKind, class: Option<LlmClass>, taken: &[String]) -> String {
    let is_taken = |l: &str| taken.iter().any(|t| t.trim().eq_ignore_ascii_case(l));
    let base = match (kind, class) {
        (SystemKind::Llm, Some(c)) => c.default_label().to_string(),
        (SystemKind::Llm, None) => {
            return (1..).map(|n| format!("System {n}")).find(|l| !is_taken(l)).unwrap_or_else(|| "System".into());
        }
        (SystemKind::Image, _) => "System CGI".into(),
        (SystemKind::Tts, _) => "System TTS".into(),
        (SystemKind::Stt, _) => "System STT".into(),
        (SystemKind::Video, _) => "System Video".into(),
    };
    if !is_taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base} ({n})")).find(|l| !is_taken(l)).unwrap_or(base)
}

/// The default id of a new System: llm fast/deep/max -> "s1"/"s2"/"s3", llm without class -> the next free
/// "sN"; image -> "cgi"; tts -> "tts"; stt -> "stt"; video -> "video". Taken -> "-2", "-3"... appended
/// (an llm class id that is taken falls back to the next free "sN").
pub fn default_system_id(kind: SystemKind, class: Option<LlmClass>, taken: &[&str]) -> SystemId {
    let is_taken = |id: &str| taken.contains(&id);
    let next_s = || (1..).map(|n| format!("s{n}")).find(|id| !is_taken(id)).unwrap_or_else(|| "s".into());
    let base = match (kind, class) {
        (SystemKind::Llm, Some(c)) => {
            let id = match c {
                LlmClass::Fast => "s1",
                LlmClass::Deep => "s2",
                LlmClass::Max => "s3",
            };
            return SystemId::new(if is_taken(id) { next_s() } else { id.to_string() });
        }
        (SystemKind::Llm, None) => return SystemId::new(next_s()),
        (SystemKind::Image, _) => "cgi",
        (SystemKind::Tts, _) => "tts",
        (SystemKind::Stt, _) => "stt",
        (SystemKind::Video, _) => "video",
    };
    if !is_taken(base) {
        return SystemId::new(base);
    }
    SystemId::new((2..).map(|n| format!("{base}-{n}")).find(|id| !is_taken(id)).unwrap_or_else(|| base.to_string()))
}

/// A GPU id as KLIF writes it: "cpu", "VEN:DEV" (upper-case hex) or "VEN:DEV#n" (the n-th identical adapter in
/// DXGI order, n >= 1; "#0" is the first one and is written as plain "VEN:DEV", the id telemetry reports for it).
/// None for an empty or malformed value.
pub fn normalize_gpu_id(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("cpu") {
        return Some("cpu".into());
    }
    let (pci, index) = match s.split_once('#') {
        Some((p, n)) => (p.trim(), Some(n.trim().parse::<u32>().ok()?)),
        None => (s, None),
    };
    let (v, d) = pci.split_once(':')?;
    let hex = |x: &str| {
        let x = x.trim();
        let x = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")).unwrap_or(x);
        u32::from_str_radix(x, 16).ok().filter(|n| *n <= 0xFFFF)
    };
    let (v, d) = (hex(v)?, hex(d)?);
    Some(match index {
        Some(n) if n > 0 => format!("{v:04X}:{d:04X}#{n}"),
        _ => format!("{v:04X}:{d:04X}"),
    })
}

// ------------------------------------------------------------------------------------- locations

/// How [`locate`] found the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocationOrigin {
    /// `KLIF_CONFIG`.
    Env,
    /// `.local/klif.toml` above the executable.
    ExeDir,
    /// `.local/klif.toml` above the current directory.
    Cwd,
    /// `%APPDATA%\KLIF\klif.toml` (or the XDG equivalent).
    Default,
}

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub path: PathBuf,
    pub origin: LocationOrigin,
    pub exists: bool,
}

/// `%APPDATA%\KLIF` on Windows; `$XDG_CONFIG_HOME/klif` (or `~/.config/klif`) elsewhere.
pub fn default_state_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map(|a| PathBuf::from(a).join("KLIF"))
    }
    #[cfg(not(windows))]
    {
        xdg_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join("klif"))
    }
}

/// `%LOCALAPPDATA%\KLIF` on Windows; `$XDG_DATA_HOME/klif` (or `~/.local/share/klif`) elsewhere.
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(|a| PathBuf::from(a).join("KLIF"))
    }
    #[cfg(not(windows))]
    {
        xdg_dir("XDG_DATA_HOME", ".local/share").map(|d| d.join("klif"))
    }
}

#[cfg(not(windows))]
fn xdg_dir(var: &str, home_rel: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(home_rel)))
}

fn find_up(start: &Path) -> Option<PathBuf> {
    start.ancestors().map(|dir| dir.join(".local").join(FILE_NAME)).find(|cand| cand.is_file())
}

/// Which klif.toml KLIF uses, and why (diag prints it).
pub fn locate() -> Location {
    if let Some(p) = std::env::var_os("KLIF_CONFIG").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(p);
        let path = if path.is_relative() { std::env::current_dir().map(|c| c.join(&path)).unwrap_or(path) } else { path };
        let exists = path.is_file();
        return Location { path, origin: LocationOrigin::Env, exists };
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        if let Some(path) = find_up(&dir) {
            return Location { path, origin: LocationOrigin::ExeDir, exists: true };
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(path) = find_up(&cwd) {
            return Location { path, origin: LocationOrigin::Cwd, exists: true };
        }
    }
    let dir = default_state_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = dir.join(FILE_NAME);
    let exists = path.is_file();
    Location { path, origin: LocationOrigin::Default, exists }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', "\\");
        s.trim_end_matches('\\').to_ascii_lowercase()
    };
    if cfg!(windows) { norm(a) == norm(b) } else { a == b }
}

/// `state_dir`, except the default state dir maps to the default data dir.
fn data_dir_for(state_dir: &Path) -> PathBuf {
    match (default_state_dir(), default_data_dir()) {
        (Some(def_state), Some(def_data)) if same_dir(state_dir, &def_state) => def_data,
        _ => state_dir.to_path_buf(),
    }
}

// ------------------------------------------------------------------------------------------ load

/// Load the config from where [`locate`] says. Never fails.
pub fn load() -> LoadedConfig {
    let loc = locate();
    match (loc.origin, loc.exists) {
        (LocationOrigin::Default, false) => {
            // First run: no file required. The state folder is created so state.json / engine.lock can live there.
            let dir = loc.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
            let mut issues = Vec::new();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                issues.push(Issue::warn(Some(FILE_FIELD), format!("The KLIF folder {} could not be created: {e}.", dir.display())));
            }
            let mut cfg = Config::empty_at(&loc.path);
            cfg.source = None;
            LoadedConfig { cfg, issues }
        }
        _ => load_from(&loc.path),
    }
}

/// Load one specific file (KLIF_CONFIG, hot reload). Never fails; a missing file gives an empty config at
/// that path with a warning.
pub fn load_from(path: &Path) -> LoadedConfig {
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Config::parse(&text, path),
            Err(_) => LoadedConfig {
                cfg: Config::empty_at(path),
                issues: vec![Issue::error(
                    Some(FILE_FIELD),
                    format!("{} is not UTF-8 text; KLIF uses an empty configuration until it is fixed.", path.display()),
                )],
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LoadedConfig {
            cfg: Config::empty_at(path),
            issues: vec![Issue::warn(Some(FILE_FIELD), format!("{} does not exist yet; KLIF starts with an empty configuration.", path.display()))],
        },
        Err(e) => LoadedConfig {
            cfg: Config::empty_at(path),
            issues: vec![Issue::error(Some(FILE_FIELD), format!("{} could not be read: {e}.", path.display()))],
        },
    }
}

fn one_line(e: impl fmt::Display) -> String {
    e.to_string().trim().replace(['\r', '\n'], " ")
}

/// What replaces a value in an error sentence.
const HIDDEN: &str = "<hidden>";

/// [`one_line`] with every value serde quotes from the file hidden: `string "..."` (serde's `Unexpected::Str`,
/// Debug-escaped) and the payload of `unknown variant `...``. A secret typed into the wrong field (an API key
/// where a bool goes, an `HF_TOKEN=...` string where a table goes) must not reach the log, the UI, the CLI or a
/// view-only network peer through `bad_presets` / issues. The type, the expected type and the key stay:
/// `invalid type: string <hidden>, expected a boolean in `api_key``.
pub fn scrub(e: impl fmt::Display) -> String {
    let s = one_line(e);
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    loop {
        let quoted = rest.find("string \"").map(|i| (i + "string ".len(), true));
        let variant = rest.find("unknown variant `").map(|i| (i + "unknown variant ".len(), false));
        let next = match (quoted, variant) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (a, b) => a.or(b),
        };
        let Some((start, is_str)) = next else { break };
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = if is_str {
            // The closing quote: the first one not escaped by a backslash.
            let mut escaped = false;
            tail.char_indices().find(|&(_, c)| {
                let close = c == '"' && !escaped;
                escaped = c == '\\' && !escaped;
                close
            })
        } else {
            // serde: "unknown variant `X`, expected ..." (X is not escaped, so prefer the "`, expected" boundary).
            tail.find("`, expected").or_else(|| tail.find("`, there are no variants")).or_else(|| tail.find('`')).map(|i| (i, '`'))
        };
        out.push_str(HIDDEN);
        match end {
            Some((i, c)) => rest = &tail[i + c.len_utf8()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A klif.toml syntax error as one sentence WITHOUT the source line (`toml`'s Display quotes the whole line, which
/// may hold a secret): "<path> does not parse (line L, column C): <what>".
fn parse_error_sentence(path: &Path, text: &str, e: &toml::de::Error) -> String {
    let at = e
        .span()
        .map(|s| {
            let (line, col) = line_col(text, s.start);
            format!(" (line {line}, column {col})")
        })
        .unwrap_or_default();
    format!(
        "{} does not parse{at}: {}, so KLIF uses an empty configuration until it is fixed.",
        path.display(),
        scrub(e.message()).trim_end_matches('.')
    )
}

/// 1-based line and column (in characters) of a byte offset of `text`.
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut at = offset.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

/// Read one top-level section; a bad section becomes its default plus an error issue.
fn section<T: DeserializeOwned + Default>(raw: &mut toml::Table, name: &str, issues: &mut Vec<Issue>) -> T {
    match raw.remove(name) {
        None => T::default(),
        Some(v) => v.try_into::<T>().unwrap_or_else(|e| {
            issues.push(Issue::error(Some(name), format!("[{name}] could not be read, so KLIF uses its defaults: {}", scrub(e))));
            T::default()
        }),
    }
}

/// The table of a `[<section>]` that holds entries (`[systems.*]`, `[presets.*]`, `[nodes.*]`).
fn entries(raw: &mut toml::Table, name: &str, issues: &mut Vec<Issue>) -> Option<toml::Table> {
    match raw.remove(name)? {
        toml::Value::Table(t) => Some(t),
        _ => {
            issues.push(Issue::error(Some(name), format!("\"{name}\" must be a table of entries ([{name}.<id>]); it is ignored.")));
            None
        }
    }
}

/// Unknown keys of an entry: warnings; a table among them is an error (a dotted id such as `[presets.a.b]`).
fn check_keys(what: &str, id: &str, table: &toml::Table, known: &[&str], issues: &mut Vec<Issue>) -> Result<(), String> {
    let field = format!("{what}.{id}");
    for (k, v) in table {
        if known.contains(&k.as_str()) {
            continue;
        }
        if v.is_table() {
            return Err(format!(
                "[{what}.{id}] contains a table \"{k}\": an id cannot contain '.', so [{what}.{id}.{k}] is read as a sub-table. Rename it to [{what}.{id}-{k}] or similar."
            ));
        }
        issues.push(Issue::warn(Some(&field), format!("[{what}.{id}]: unknown key \"{k}\" is ignored.")));
    }
    Ok(())
}

impl Config {
    /// An empty config belonging to `path` (state_dir = its folder). Used for a missing or unreadable file.
    pub fn empty_at(path: &Path) -> Config {
        let state_dir = path.parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."));
        let data_dir = data_dir_for(&state_dir);
        Config {
            net: NetCfg::default(),
            gpu: GpuCfg::default(),
            ui: UiCfg::default(),
            telemetry: TelemetryCfg::default(),
            paths: PathsCfg::default(),
            security: SecurityCfg::default(),
            launch: LaunchCfg::default(),
            node: None,
            nodes: Vec::new(),
            systems: Vec::new(),
            presets: BTreeMap::new(),
            bad_presets: BTreeMap::new(),
            bad_systems: BTreeMap::new(),
            bad_nodes: BTreeMap::new(),
            source: Some(path.to_path_buf()),
            state_dir,
            data_dir,
        }
    }

    /// Parse klif.toml text that belongs to `path`. Never fails: a file that does not parse gives
    /// `empty_at(path)` plus an error issue (field [`FILE_FIELD`]); a bad section falls back to its defaults; a bad
    /// entry lands in `bad_systems` / `bad_presets` / `bad_nodes`.
    pub fn parse(text: &str, path: &Path) -> LoadedConfig {
        let text = text.trim_start_matches('\u{feff}');
        let mut cfg = Config::empty_at(path);
        let mut raw: toml::Table = match toml::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                return LoadedConfig { cfg, issues: vec![Issue::error(Some(FILE_FIELD), parse_error_sentence(path, text, &e))] };
            }
        };
        let mut issues = Vec::new();
        cfg.net = section(&mut raw, "net", &mut issues);
        cfg.gpu = section(&mut raw, "gpu", &mut issues);
        cfg.ui = section(&mut raw, "ui", &mut issues);
        cfg.telemetry = section(&mut raw, "telemetry", &mut issues);
        cfg.paths = section(&mut raw, "paths", &mut issues);
        cfg.security = section(&mut raw, "security", &mut issues);
        cfg.launch = section(&mut raw, "launch", &mut issues);
        if parse_api_key_source(&cfg.security.api_key).is_none() {
            // Never echo the value: it may be the key itself.
            issues.push(Issue::error(
                Some("security.api_key"),
                "[security] api_key must be \"file\", \"env:NAME\" or \"none\" (never the key itself); KLIF uses \"file\".",
            ));
        }
        if let Some(v) = raw.remove("node") {
            match v.try_into::<NodeCfg>() {
                Ok(n) => cfg.node = Some(n),
                Err(e) => issues.push(Issue::error(
                    Some("node"),
                    format!("[node] could not be read, so this machine does not listen for other nodes: {}", scrub(e)),
                )),
            }
        }
        if let Some(nodes) = entries(&mut raw, "nodes", &mut issues) {
            for (id, value) in nodes {
                match parse_remote_node(&id, value, &mut issues) {
                    Ok(n) => cfg.nodes.push((id, n)),
                    Err(e) => {
                        issues.push(Issue::error(Some(&format!("nodes.{id}")), e.clone()));
                        cfg.bad_nodes.insert(id, e);
                    }
                }
            }
        }
        match entries(&mut raw, "systems", &mut issues) {
            Some(systems) => {
                for (id, value) in systems {
                    match parse_system(&id, value, &mut issues) {
                        Ok(s) => cfg.systems.push((SystemId::new(id), s)),
                        Err(e) => {
                            issues.push(Issue::error(Some(&format!("systems.{id}")), e.clone()));
                            cfg.bad_systems.insert(id, e);
                        }
                    }
                }
            }
            None => {
                if let Some(tiers) = raw.remove("tiers").and_then(|v| v.as_table().cloned()) {
                    cfg.systems = systems_from_tiers(&tiers);
                }
            }
        }
        if let Some(presets) = entries(&mut raw, "presets", &mut issues) {
            for (id, value) in presets {
                match parse_preset(&id, value, &mut issues) {
                    Ok(p) => {
                        cfg.presets.insert(id, p);
                    }
                    Err(e) => {
                        issues.push(Issue::error(Some(&format!("presets.{id}")), e.clone()));
                        cfg.bad_presets.insert(id, e);
                    }
                }
            }
        }
        LoadedConfig { cfg, issues }
    }

    /// The klif.toml KLIF reads and writes: `source`, else `<state_dir>\klif.toml`.
    pub fn file_path(&self) -> PathBuf {
        self.source.clone().unwrap_or_else(|| self.state_dir.join(FILE_NAME))
    }

    /// A file in the state directory (state.json, engine.lock, control.json, api-key.txt, node-token.txt).
    pub fn state_path(&self, name: &str) -> PathBuf {
        self.state_dir.join(name)
    }

    /// A file or folder in the data directory (logs, bench, webview-data).
    pub fn data_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    /// Session logs: `[paths] logs_dir` (relative = under state_dir), else `<data_dir>\logs`.
    pub fn logs_dir(&self) -> PathBuf {
        match self.paths.logs_dir.as_deref().filter(|p| !p.as_os_str().is_empty()) {
            Some(p) if p.is_relative() => self.state_dir.join(p),
            Some(p) => p.to_path_buf(),
            None => self.data_dir.join("logs"),
        }
    }

    /// `[paths] models_dir` (relative = under state_dir); None when unset.
    pub fn models_dir(&self) -> Option<PathBuf> {
        let p = self.paths.models_dir.as_deref().filter(|p| !p.as_os_str().is_empty())?;
        Some(if p.is_relative() { self.state_dir.join(p) } else { p.to_path_buf() })
    }

    /// A local System by id.
    pub fn system(&self, id: &str) -> Option<&SystemCfg> {
        self.systems.iter().find(|(k, _)| k.as_str() == id).map(|(_, s)| s)
    }

    /// The System's active preset (id and spec), when it names a preset that parsed.
    pub fn system_preset(&self, id: &str) -> Option<(&str, &PresetCfg)> {
        let pid = self.system(id)?.preset_id()?;
        self.presets.get_key_value(pid).map(|(k, v)| (k.as_str(), v))
    }

    /// Local Systems whose active preset is `preset_id`.
    pub fn systems_using(&self, preset_id: &str) -> Vec<SystemId> {
        self.systems.iter().filter(|(_, s)| s.preset_id() == Some(preset_id)).map(|(id, _)| id.clone()).collect()
    }

    /// The tab label of every local System in file order: `label`, else [`default_system_label`] (explicit labels
    /// are reserved first, then defaults are given in file order).
    pub fn system_labels(&self) -> Vec<(SystemId, String)> {
        let mut taken: Vec<String> =
            self.systems.iter().filter_map(|(_, s)| s.label.as_deref().map(str::trim).filter(|l| !l.is_empty())).map(str::to_string).collect();
        self.systems
            .iter()
            .map(|(id, s)| {
                let label = match s.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                    Some(l) => l.to_string(),
                    None => {
                        let l = default_system_label(s.kind, s.class, &taken);
                        taken.push(l.clone());
                        l
                    }
                };
                (id.clone(), label)
            })
            .collect()
    }

    /// One System's tab label (see [`Config::system_labels`]).
    pub fn system_label(&self, id: &str) -> Option<String> {
        self.system_labels().into_iter().find(|(k, _)| k.as_str() == id).map(|(_, l)| l)
    }

    /// Every GPU a preset runs on: its `gpu` (a comma list), else `[gpu] inference`; normalized with
    /// [`normalize_gpu_id`]. Empty when neither is set.
    pub fn preset_gpus(&self, spec: &PresetCfg) -> Vec<String> {
        let list = spec.gpu.as_deref().map(str::trim).filter(|s| !s.is_empty()).or(self.gpu.inference.as_deref());
        list.map(|l| l.split(',').filter_map(normalize_gpu_id).collect()).unwrap_or_default()
    }

    /// The first GPU a preset runs on (the fit display), see [`Config::preset_gpus`].
    pub fn preset_gpu(&self, spec: &PresetCfg) -> Option<String> {
        self.preset_gpus(spec).into_iter().next()
    }

    /// A remote node by id.
    pub fn remote_node(&self, id: &str) -> Option<&RemoteNodeCfg> {
        self.nodes.iter().find(|(k, _)| k == id).map(|(_, n)| n)
    }

    /// A copy safe to print (diag): every preset [`PresetCfg::masked`].
    pub fn redacted(&self) -> Config {
        let mut c = self.clone();
        for p in c.presets.values_mut() {
            *p = p.masked();
        }
        c
    }
}

/// The 0.2 `[tiers]` fallback (SPEC 2.1): low -> s1, medium -> s2, high -> s3, krea -> cgi, for the tiers present.
fn systems_from_tiers(tiers: &toml::Table) -> Vec<(SystemId, SystemCfg)> {
    const MAP: [(&str, &str, &str, SystemKind, Option<LlmClass>); 4] = [
        ("low", "s1", "System 1", SystemKind::Llm, Some(LlmClass::Fast)),
        ("medium", "s2", "System 2", SystemKind::Llm, Some(LlmClass::Deep)),
        ("high", "s3", "System 3", SystemKind::Llm, Some(LlmClass::Max)),
        ("krea", "cgi", "System CGI", SystemKind::Image, None),
    ];
    MAP.iter()
        .filter_map(|&(tier, id, label, kind, class)| {
            let t = tiers.get(tier)?.as_table()?;
            let preset = t.get("preset").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
            let params = t
                .get("params")
                .and_then(|v| v.as_table())
                .map(|p| p.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
                .unwrap_or_default();
            Some((SystemId::new(id), SystemCfg { label: Some(label.to_string()), kind, class, preset, params, exclusive: false }))
        })
        .collect()
}

/// One `[systems.<id>]` entry. Err = the sentence stored in `bad_systems`; warnings go to `issues`.
fn parse_system(id: &str, value: toml::Value, issues: &mut Vec<Issue>) -> Result<SystemCfg, String> {
    validate_system_id(id).map_err(|e| format!("System {e}"))?;
    let toml::Value::Table(table) = value else {
        return Err(format!("System \"{id}\" must be a table ([systems.{id}]), not a single value."));
    };
    check_keys("systems", id, &table, SYSTEM_KEYS, issues)?;
    if !table.contains_key("kind") {
        return Err(format!("System \"{id}\" needs a kind: llm, image, tts, stt or video."));
    }
    let sys = toml::Value::Table(table)
        .try_into::<SystemCfg>()
        .map_err(|e| format!("System \"{id}\" could not be read: {}", scrub(e)))?;
    if sys.class.is_some() && sys.kind != SystemKind::Llm {
        issues.push(Issue::warn(Some(&format!("systems.{id}")), format!("System \"{id}\": class only applies to llm Systems; it is ignored.")));
    }
    Ok(sys)
}

/// One `[nodes.<id>]` entry. Err = the sentence stored in `bad_nodes`.
fn parse_remote_node(id: &str, value: toml::Value, issues: &mut Vec<Issue>) -> Result<RemoteNodeCfg, String> {
    validate_system_id(id).map_err(|e| format!("Node {e}"))?;
    let toml::Value::Table(table) = value else {
        return Err(format!("Node \"{id}\" must be a table ([nodes.{id}]), not a single value."));
    };
    check_keys("nodes", id, &table, &["address", "token", "name"], issues)?;
    let node = toml::Value::Table(table).try_into::<RemoteNodeCfg>().map_err(|e| format!("Node \"{id}\" could not be read: {}", scrub(e)))?;
    if node.address.trim().is_empty() {
        return Err(format!("Node \"{id}\" needs an address, e.g. \"192.0.2.10:{NODE_PORT}\"."));
    }
    node.token_ref().map_err(|e| format!("Node \"{id}\": {e}"))?;
    Ok(node)
}

/// One `[presets.<id>]` entry. Err = the sentence stored in `bad_presets`; warnings go to `issues`.
fn parse_preset(id: &str, value: toml::Value, issues: &mut Vec<Issue>) -> Result<PresetCfg, String> {
    validate_preset_id(id)?;
    let toml::Value::Table(table) = value else {
        return Err(format!("Preset \"{id}\" must be a table ([presets.{id}]), not a single value."));
    };
    check_keys("presets", id, &table, PRESET_KEYS, issues)?;
    toml::Value::Table(table).try_into::<PresetCfg>().map_err(|e| format!("Preset \"{id}\" could not be read: {}", scrub(e)))
}
