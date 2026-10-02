//! `state.json` in the state directory: recipes per tier, the selected slot, the current session record
//! (to adopt it after a restart) and the last-session summary. Never the API key.
//! Written atomically (temp file, then rename).

use klif_catalog::Recipes;
use klif_common::vm::{LastSession, ModelRef, Recipe, SlotId, SlotKind};
use klif_supervisor::SessionRecord;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub version: u32,
    #[serde(default)]
    pub recipes: Recipes,
    #[serde(default)]
    pub selected: Option<SlotId>,
    #[serde(default)]
    pub session: Option<PersistedSession>,
    #[serde(default)]
    pub last_session: Option<PersistedLast>,
}

impl Default for PersistedState {
    fn default() -> Self {
        PersistedState { version: STATE_VERSION, recipes: Recipes::new(), selected: None, session: None, last_session: None }
    }
}

/// How the current session came to be owned by KLIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionOrigin {
    /// Launched by KLIF.
    Klif,
    /// A run of the old PowerShell GUI (RuntimeProcesses), adopted.
    Legacy,
}

/// Everything needed to show and re-adopt the running session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSession {
    pub record: SessionRecord,
    pub origin: SessionOrigin,
    pub slot: SlotId,
    pub kind: SlotKind,
    pub card_id: String,
    pub model: ModelRef,
    #[serde(default)]
    pub recipe: Option<Recipe>,
    pub host: String,
    #[serde(default)]
    pub spec_mode: Option<String>,
    #[serde(default)]
    pub ctx_tokens: Option<u32>,
    pub api_key_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLast {
    pub summary: LastSession,
    /// Epoch seconds when it ended (`endedAgoS` is derived from it).
    pub ended_at: f64,
}

pub fn load(path: &Path) -> PersistedState {
    let Ok(bytes) = std::fs::read(path) else { return PersistedState::default() };
    let text = String::from_utf8_lossy(&bytes);
    match serde_json::from_str::<PersistedState>(text.trim_start_matches('\u{feff}')) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("{} is not readable ({e}); starting from defaults", path.display());
            PersistedState::default()
        }
    }
}

pub fn save(path: &Path, state: &PersistedState) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(state)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
