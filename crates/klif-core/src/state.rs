//! Persistence in the state directory: `state.v3.json` (0.3 never writes 0.2's `state.json`) — the selected tab,
//! every running session's record (to adopt them after a restart), the last session per System, learned model
//! shapes and measured VRAM layers. Never the API key. Written atomically (temp file, then rename).
//!
//! `state.v3.json`: `{ version: 3, selected, sessions: { <system>: PersistedSession }, lastSessions: { <system>:
//! PersistedLast }, arches, layers }`. Sessions keep `origin` / `cardId` (= preset id) for 0.2 readability.
//! If `state.v3.json` is missing, `state.json` (v1/v2) is read once, read-only (session / selected / lastSession
//! mapped low -> s1, medium -> s2, high -> s3, krea -> cgi) and the result is written to `state.v3.json`. Only
//! `state.v3.json` is ever renamed `.bad-<stamp>` (when it is not JSON at all). Lenient everywhere else: every
//! struct defaults, `command` / `model` fall back to defaults when they do not parse, and a map entry (a session,
//! a last session, an arch, a layer list) that does not parse is dropped on its own instead of the whole file.
//!
//! The remote nodes' last-known Systems live in `<data_dir>\node-cache.json`, owned by `crate::nodes` (E3).
//! Owner: package E1.

use klif_common::vm::{AdapterId, CommandView, HealthCheck, LastSession, ModelArch, ModelRef, SystemId, SystemKind, VramLayer};
use klif_supervisor::SessionRecord;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STATE_VERSION: u32 = 3;
/// The 0.3 state file in the state dir.
pub const STATE_FILE: &str = "state.v3.json";
/// 0.2's state file: read once (read-only) when `state.v3.json` does not exist yet.
pub const LEGACY_STATE_FILE: &str = "state.json";
/// At most this many measured layer lists are kept (by preset hash).
pub const MAX_LAYERS: usize = 32;

/// Deserialize a value, falling back to its default when it does not parse.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// Deserialize a map entry by entry: entries that do not parse are dropped (and logged), the rest is kept.
fn lenient_map<'de, D: Deserializer<'de>, K: Ord + From<String>, T: DeserializeOwned>(d: D) -> Result<BTreeMap<K, T>, D::Error> {
    let v = Value::deserialize(d)?;
    let mut out = BTreeMap::new();
    if let Value::Object(map) = v {
        for (k, v) in map {
            match serde_json::from_value::<T>(v) {
                Ok(t) => {
                    out.insert(K::from(k), t);
                }
                Err(e) => log::warn!("state: entry \"{k}\" is not readable ({e}); it is dropped"),
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedState {
    pub version: u32,
    #[serde(deserialize_with = "lenient")]
    pub selected: Option<SystemId>,
    #[serde(deserialize_with = "lenient_map")]
    pub sessions: BTreeMap<SystemId, PersistedSession>,
    #[serde(deserialize_with = "lenient_map")]
    pub last_sessions: BTreeMap<SystemId, PersistedLast>,
    /// Model shapes seen in server logs, by model key (see `engine::model_key`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "lenient_map")]
    pub arches: BTreeMap<String, ModelArch>,
    /// Measured VRAM composition of the last live session per preset hash (at most [`MAX_LAYERS`]).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "lenient_map")]
    pub layers: BTreeMap<String, Vec<VramLayer>>,
}

impl Default for PersistedState {
    fn default() -> Self {
        PersistedState {
            version: STATE_VERSION,
            selected: None,
            sessions: BTreeMap::new(),
            last_sessions: BTreeMap::new(),
            arches: BTreeMap::new(),
            layers: BTreeMap::new(),
        }
    }
}

fn origin_klif() -> String {
    "klif".into()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Everything needed to show and re-adopt one running session. Everything but record/kind defaults.
/// While a session runs, the engine uses THESE values (kind, adapter, port, host, GPUs), never the current config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSession {
    pub record: SessionRecord,
    pub kind: SystemKind,
    #[serde(default)]
    pub system: SystemId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// None (a v1 record): sd.cpp for an image session, else llama.cpp. See [`PersistedSession::adapter`].
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub adapter: Option<AdapterId>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub health: Option<HealthCheck>,
    #[serde(default)]
    pub metrics: bool,
    /// What ran (masked).
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandView>,
    #[serde(default, deserialize_with = "lenient")]
    pub model: ModelRef,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub spec_mode: Option<String>,
    #[serde(default)]
    pub ctx_tokens: Option<u32>,
    /// KLIF injected its API key at launch (llama.cpp / vllm, preset `api_key`): probes then send it.
    #[serde(default)]
    pub api_key_set: bool,
    /// The first GPU: "VEN:DEV", "VEN:DEV#n" or "cpu".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// Every GPU it runs on (conflicts / reservations apply to all of them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpus: Vec<String>,
    /// The System was `exclusive` at launch (used while it is not in klif.toml any more).
    #[serde(default, skip_serializing_if = "is_false")]
    pub exclusive: bool,
    /// The expected VRAM total at launch (GiB), for the reservation while it loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_gib: Option<f64>,
    /// Always "klif" (0.2 compatibility).
    #[serde(default = "origin_klif")]
    pub origin: String,
    /// The preset id (0.2 compatibility: 0.2 reads it as the card id).
    #[serde(default)]
    pub card_id: String,
}

impl PersistedSession {
    /// The adapter, with the v1 default.
    pub fn adapter(&self) -> AdapterId {
        self.adapter.unwrap_or(if self.kind == SystemKind::Image { AdapterId::SdCpp } else { AdapterId::LlamaCpp })
    }

    /// Every GPU of the session (`gpus`, else `gpu`).
    pub fn all_gpus(&self) -> Vec<String> {
        if !self.gpus.is_empty() {
            self.gpus.clone()
        } else {
            self.gpu.iter().cloned().collect()
        }
    }
}

/// The last session of a System (`ended_at` gives `endedAgoS`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLast {
    pub summary: LastSession,
    /// Epoch seconds when it ended.
    #[serde(default)]
    pub ended_at: f64,
}

/// Where `load` got the state from (for a console note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `state.v3.json`.
    V3,
    /// Migrated once from 0.2's `state.json` (read-only).
    Legacy,
    /// Nothing usable (first run, or an unreadable file moved aside to the given path).
    Fresh(Option<PathBuf>),
}

#[allow(dead_code)]
/// Read `state.v3.json` from `state_dir`; when it is missing, migrate 0.2's `state.json` once (read-only) and
/// write the result to `state.v3.json`. Never fails.
pub fn load(state_dir: &Path) -> PersistedState {
    load_with_origin(state_dir).0
}

/// [`load`], also telling where the state came from.
pub fn load_with_origin(state_dir: &Path) -> (PersistedState, Origin) {
    let path = state_dir.join(STATE_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            match serde_json::from_str::<PersistedState>(text.trim_start_matches('\u{feff}')) {
                Ok(mut s) => {
                    s.version = STATE_VERSION;
                    (s, Origin::V3)
                }
                Err(e) => {
                    let bad = state_dir.join(format!("{STATE_FILE}.bad-{}", crate::timefmt::local_stamp()));
                    log::warn!("{} is not readable ({e}); moved to {} and starting from defaults", path.display(), bad.display());
                    let moved = std::fs::rename(&path, &bad).is_ok();
                    (PersistedState::default(), Origin::Fresh(moved.then_some(bad)))
                }
            }
        }
        Err(_) => match read_legacy(&state_dir.join(LEGACY_STATE_FILE)) {
            Some(s) => {
                if let Err(e) = save(state_dir, &s) {
                    log::warn!("could not write {}: {e:#}", path.display());
                }
                log::info!("state: migrated 0.2's {} (read-only) into {}", LEGACY_STATE_FILE, STATE_FILE);
                (s, Origin::Legacy)
            }
            None => (PersistedState::default(), Origin::Fresh(None)),
        },
    }
}

/// Write `state.v3.json` into `state_dir` (atomically).
pub fn save(state_dir: &Path, state: &PersistedState) -> anyhow::Result<()> {
    let path = state_dir.join(STATE_FILE);
    let json = serde_json::to_vec_pretty(state)?;
    std::fs::create_dir_all(state_dir)?;
    let tmp = state_dir.join(format!("{STATE_FILE}.tmp"));
    std::fs::write(&tmp, &json)?;
    let mut last = None;
    for _ in 0..5 {
        match std::fs::rename(&tmp, &path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
    Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("rename failed")))
}

// ------------------------------------------------------------------------------------------ 0.2 migration

/// The 0.2 slot -> 0.3 System id mapping (SPEC 2.1).
pub fn legacy_system_id(slot: &str) -> Option<&'static str> {
    match slot.trim().to_ascii_lowercase().as_str() {
        "low" => Some("s1"),
        "medium" => Some("s2"),
        "high" => Some("s3"),
        "krea" => Some("cgi"),
        _ => None,
    }
}

/// A 0.2 slot (or an id that is already a 0.3 id) as a System id. Unmapped slots keep their name (the session is
/// then adopted as a ghost).
fn map_slot(slot: &str) -> SystemId {
    SystemId::new(legacy_system_id(slot).map(str::to_string).unwrap_or_else(|| slot.trim().to_string()))
}

/// Read 0.2's `state.json` (v1 single `session`, or a `sessions` map) without ever writing it.
fn read_legacy(path: &Path) -> Option<PersistedState> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let v: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("{} (0.2) is not readable ({e}); it is left alone", path.display());
            return None;
        }
    };
    let obj = v.as_object()?;
    let mut out = PersistedState { selected: obj.get("selected").and_then(Value::as_str).map(map_slot), ..PersistedState::default() };
    let mut legacy_sessions: Vec<Value> = Vec::new();
    if let Some(s) = obj.get("session").filter(|s| s.is_object()) {
        legacy_sessions.push(s.clone());
    }
    if let Some(Value::Object(m)) = obj.get("sessions") {
        for (k, s) in m {
            let mut s = s.clone();
            if let Some(o) = s.as_object_mut() {
                if !o.contains_key("slot") && !o.contains_key("system") {
                    o.insert("slot".into(), Value::String(k.clone()));
                }
            }
            legacy_sessions.push(s);
        }
    }
    for s in legacy_sessions {
        if let Some(p) = legacy_session(&s) {
            out.sessions.insert(p.system.clone(), p);
        }
    }
    if let Some(l) = obj.get("lastSession").and_then(legacy_last) {
        out.last_sessions.insert(l.summary.system.clone(), l);
    }
    if let Some(Value::Object(m)) = obj.get("lastSessions") {
        for l in m.values().filter_map(legacy_last) {
            out.last_sessions.insert(l.summary.system.clone(), l);
        }
    }
    if let Some(Value::Object(m)) = obj.get("arches") {
        for (k, a) in m {
            if let Ok(a) = serde_json::from_value::<ModelArch>(a.clone()) {
                out.arches.insert(k.clone(), a);
            }
        }
    }
    Some(out)
}

fn legacy_session(s: &Value) -> Option<PersistedSession> {
    let o = s.as_object()?;
    let record: SessionRecord = serde_json::from_value(o.get("record")?.clone()).ok()?;
    let kind = o.get("kind").and_then(Value::as_str).and_then(SystemKind::parse).unwrap_or(SystemKind::Llm);
    let system = o
        .get("system")
        .and_then(Value::as_str)
        .map(SystemId::from)
        .or_else(|| o.get("slot").and_then(Value::as_str).map(map_slot))?;
    let str_of = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    Some(PersistedSession {
        record,
        kind,
        system,
        preset: str_of("preset"),
        adapter: o.get("adapter").cloned().and_then(|a| serde_json::from_value(a).ok()),
        health: None,
        metrics: false,
        command: None,
        model: o.get("model").cloned().and_then(|m| serde_json::from_value(m).ok()).unwrap_or_default(),
        host: str_of("host").unwrap_or_default(),
        spec_mode: str_of("specMode"),
        ctx_tokens: o.get("ctxTokens").and_then(Value::as_u64).map(|c| c as u32),
        api_key_set: o.get("apiKeySet").and_then(Value::as_bool).unwrap_or(false),
        gpu: None,
        gpus: Vec::new(),
        exclusive: false,
        expected_gib: None,
        origin: origin_klif(),
        card_id: str_of("cardId").unwrap_or_default(),
    })
}

fn legacy_last(l: &Value) -> Option<PersistedLast> {
    let o = l.as_object()?;
    let mut summary = o.get("summary")?.clone();
    let so = summary.as_object_mut()?;
    if !so.contains_key("system") {
        let slot = so.get("slot").and_then(Value::as_str).map(map_slot)?;
        so.insert("system".into(), Value::String(slot.0));
    }
    let summary: LastSession = serde_json::from_value(summary).ok()?;
    Some(PersistedLast { summary, ended_at: o.get("endedAt").and_then(Value::as_f64).unwrap_or(0.0) })
}
