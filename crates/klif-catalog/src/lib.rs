//! klif-catalog: the launch catalog, taken from the existing PowerShell launcher (single source of
//! truth) through `scripts/Export-KlifCatalog.ps1`, plus the exact launch-plan builder.
//!
//! Contract (owned by the catalog stream; signatures are fixed, bodies are to be implemented):
//! - `Catalog::load` runs the exporter only when the launcher's SHA-256 changed (cache in the state dir),
//!   then re-checks cheap file existence natively.
//! - `Catalog::slots` turns tier recipes into the view model's `Slot`s (label, ModelRef, availability +
//!   reason, expected VRAM layers, recipe, options for the Tune drawer).
//! - `Catalog::plan` builds the launch plan with byte-for-byte parity to the old GUI's `Start-GuiLaunch`
//!   (argv order, injected -PromptCacheMiB/-Port/-Vision/-CacheType, Flash-Next cache cap,
//!   GenerationMode override, working dir, session log names, LLAMA_API_KEY set/removed).
//!
//! Layout: `export` (run + cache the exporter), `data` (the exporter's JSON, typed), `probe` (native file
//! checks), `recipes` (defaults, patches, repairs), `slots` (view model), `plan` (launch plan), `vram`
//! (expected VRAM estimate), `sha256` (cache key).

mod data;
mod export;
mod plan;
mod probe;
mod recipes;
mod sha256;
mod slots;
mod vram;

use anyhow::Result;
use klif_common::config::Config;
use klif_common::vm::{ModelRef, Recipe, RecipePatch, Slot, SlotId, SlotKind, VramLayer};
use klif_common::Secret;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub use export::Origin;

/// Per-tier recipes (persisted by the engine in state.json).
pub type Recipes = BTreeMap<SlotId, Recipe>;

/// Facts the catalog needs from the live system to compute availability.
#[derive(Debug, Clone, Default)]
pub struct LiveFacts {
    /// Ports currently held by a process KLIF does not own.
    pub foreign_ports: Vec<u16>,
}

/// Values carried over from the old launcher's state file (never the key itself).
#[derive(Debug, Clone, Default)]
pub struct OldLauncherDefaults {
    pub cache_type: Option<String>,
    pub vision: Option<bool>,
    pub generation_mode: Option<String>,
    pub prompt_cache_mib: Option<u32>,
    pub server_port: Option<u16>,
}

/// Everything needed to start one session. `Debug` never prints secrets.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub slot: SlotId,
    pub kind: SlotKind,
    pub card_id: String,
    /// Catalog profile key, e.g. "qwen-gsq|HIP|9070|98304".
    pub profile_key: String,
    /// Display info for the session.
    pub model: ModelRef,
    /// powershell.exe (absolute path resolved from %SystemRoot%).
    pub exe: PathBuf,
    /// Arguments after the exe, exactly as the old GUI passed them.
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Environment changes for the child: remove first, then set.
    pub env_remove: Vec<String>,
    pub env_set: Vec<(String, EnvValue)>,
    /// `klif-<yyyyMMdd-HHmmss-fff>-<cardId>-p<port>` (sanitised).
    pub session_name: String,
    pub out_log: PathBuf,
    pub err_log: PathBuf,
    pub port: u16,
    /// Host to probe and to show in the endpoint chip.
    pub host: String,
    pub expected_layers: Vec<VramLayer>,
    /// Speculative mode label for the UI, e.g. "MTP + n-gram", if any.
    pub spec_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EnvValue {
    Plain(String),
    Secret(Secret),
}

pub struct Catalog {
    pub(crate) data: data::Data,
    pub(crate) probe: probe::FileProbe,
    /// Display name of the inference GPU (`gpu.inference_name` from the config, else "GPU").
    pub(crate) device_name: String,
    pub(crate) origin: Origin,
    pub(crate) warnings: Vec<String>,
}

// The engine shares one catalog between its tick thread and the host's command threads.
const _: fn() = || {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<Catalog>();
};

impl Catalog {
    /// Load (or refresh) the catalog for this config.
    pub fn load(cfg: &Config) -> Result<Catalog> {
        let loaded = export::load(cfg)?;
        let mut warnings = loaded.warnings;
        warnings.extend(loaded.data.warnings.iter().cloned());
        let device_name = cfg
            .gpu
            .inference_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("GPU")
            .to_string();
        let catalog = Catalog { data: loaded.data, probe: probe::FileProbe::default(), device_name, origin: loaded.origin, warnings };
        catalog.refresh_files();
        Ok(catalog)
    }

    /// Default recipe per tier from config tiers, else from the launcher presets, filling KV/vision/mode
    /// from the old launcher state like the old GUI did.
    pub fn default_recipes(&self, cfg: &Config, old: &OldLauncherDefaults) -> Recipes {
        recipes::default_recipes(self, cfg, old)
    }

    /// Apply a Tune patch, repairing dependent fields (e.g. a context the new card does not offer).
    pub fn apply_patch(&self, slot: SlotId, current: &Recipe, patch: &RecipePatch) -> Recipe {
        recipes::apply_patch(self, slot, current, patch)
    }

    /// View-model slots for the given recipes (availability uses `live` for port busy).
    pub fn slots(&self, recipes: &Recipes, live: &LiveFacts) -> Vec<Slot> {
        slots::slots(self, recipes, live)
    }

    /// The exact launch plan for a slot's recipe. `now_local` formats the session name.
    ///
    /// Fails with a plain sentence when the recipe has no launcher profile or the profile's starter script,
    /// model file or server binary is missing (the old GUI only launched a READY combination). A busy port
    /// is NOT checked here: that is the engine's call.
    pub fn plan(
        &self,
        cfg: &Config,
        slot: SlotId,
        recipe: &Recipe,
        api_key: Option<&Secret>,
        now_local: chrono_like::LocalStamp,
    ) -> Result<LaunchPlan> {
        plan::plan(self, cfg, slot, recipe, api_key, now_local, true)
    }

    /// Like [`Catalog::plan`] but without the file-existence gate. Used by the parity check, which builds the
    /// plan of every profile, including those whose model or binary is not on this machine.
    pub fn plan_unchecked(
        &self,
        cfg: &Config,
        slot: SlotId,
        recipe: &Recipe,
        api_key: Option<&Secret>,
        now_local: chrono_like::LocalStamp,
    ) -> Result<LaunchPlan> {
        plan::plan(self, cfg, slot, recipe, api_key, now_local, false)
    }

    /// The runtime-logs directory (config override or catalog root).
    pub fn logs_dir(&self, cfg: &Config) -> PathBuf {
        match &cfg.launcher.logs_dir {
            Some(p) if !p.as_os_str().is_empty() => p.clone(),
            _ => self.data.runtime_logs.clone(),
        }
    }

    // ---- extras (not part of the fixed API) ---------------------------------------------------

    /// Forget cached file-existence results and look again (cheap; call after the user installed a model).
    pub fn refresh_files(&self) {
        self.probe.clear();
        for p in &self.data.profiles {
            self.probe.is_file(&p.script);
            self.probe.is_file(&p.binary);
            self.probe.model_bytes(&p.model);
        }
    }

    /// How the catalog data was obtained (cache hit, fresh export, stale cache).
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Exporter and launcher warnings (e.g. orphan profiles, a stale export).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Keys (`card|backend|hardware|context`) of the profiles reachable from the launcher's UI, i.e. those
    /// whose card exists. Orphan profiles (their card was removed from the launcher) are left out.
    pub fn reachable_profile_keys(&self) -> Vec<String> {
        self.data
            .profiles
            .iter()
            .filter(|p| !p.orphan && self.data.card(&p.card_id).is_some())
            .map(|p| p.key.clone())
            .collect()
    }

    /// The recipe that selects the given profile, with the catalog's own defaults for the remaining fields.
    pub fn recipe_for_profile(&self, key: &str) -> Option<Recipe> {
        recipes::recipe_for_profile(self, key)
    }

    /// The slot a card normally sits in (Flash-Next -> high, dense Qwen and Bonsai -> medium, Gemma and other
    /// language models -> low, image cards -> krea).
    pub fn home_slot(&self, card_id: &str) -> Option<SlotId> {
        recipes::home_slot(self, card_id)
    }
}

/// Minimal local timestamp for session names (yyyyMMdd-HHmmss-fff), so callers can pass a fixed
/// time in parity checks.
pub mod chrono_like {
    #[derive(Debug, Clone, Copy)]
    pub struct LocalStamp {
        pub year: u16,
        pub month: u8,
        pub day: u8,
        pub hour: u8,
        pub minute: u8,
        pub second: u8,
        pub millis: u16,
    }

    impl LocalStamp {
        pub fn format(&self) -> String {
            format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
                self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
            )
        }
    }
}
