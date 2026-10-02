//! klif-core: the engine. Owns the catalog, supervisor and telemetry; runs the session state machine;
//! publishes a `ViewModel` at 2 Hz; executes `Action`s.
//!
//! State machine (one session at a time, one inference GPU):
//!   idle --launch--> starting (process spawned) --log/health--> loading --health ready--> live
//!   live/loading --stop--> stopping --tree gone--> idle (lastSession recorded)
//!   any --process exited unexpectedly / fatal log--> fault (exit code, log tail, failed load step)
//!   fault --dismiss--> idle;  fault/live --restart--> stop + launch same recipe
//! Startup: adopt a persisted KLIF session, else an old-launcher run; never auto-start anything.
//! Persistence (state dir, state.json): recipes per tier, selected slot, current session record,
//! last session summary. Never the API key (read from the old launcher's state file on demand).
//!
//! Layout: `engine` (state machine, tick thread, view model), `state` (state.json), `oldstate` (the old
//! launcher's state file, read-only), `timefmt` (local time for session names), `narrate` (KLIF's own
//! console lines about the inference GPU, e.g. when it goes dormant and wakes up).

mod engine;
pub mod narrate;
mod oldstate;
mod state;
mod timefmt;

use anyhow::Result;
use klif_common::config::Config;
use klif_common::vm::{Action, HostInfo, SlotId, ViewModel};
use std::sync::Arc;

pub use klif_catalog;
pub use klif_common;
pub use klif_supervisor;
pub use klif_telemetry;

pub struct Engine {
    _private: (),
}

/// Thread-safe handle used by the host (Tauri) and tools.
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<engine::Inner>,
}

impl Engine {
    /// Load config + catalog, adopt sessions, start telemetry and the 2 Hz tick thread.
    pub fn start(cfg: Config, host: HostInfo) -> Result<EngineHandle> {
        Ok(EngineHandle { inner: engine::start(cfg, host)? })
    }
}

impl EngineHandle {
    /// The latest view model.
    pub fn snapshot(&self) -> ViewModel {
        self.inner.snapshot()
    }

    /// Register a callback invoked (on the engine thread) with every new view model (2 Hz).
    pub fn subscribe(&self, f: Box<dyn Fn(&ViewModel) + Send + Sync>) {
        self.inner.subscribe(f)
    }

    /// Execute an action. Errors are user-facing sentences (shown as toasts).
    pub fn act(&self, action: Action) -> Result<()> {
        self.inner.act(action)
    }

    /// Update host facts (e.g. maximized) that the view model reports.
    pub fn set_host(&self, host: HostInfo) {
        self.inner.set_host(host)
    }

    /// The current session endpoint URL for "open endpoint" / "copy endpoint" (None when idle).
    pub fn endpoint_url(&self) -> Option<String> {
        self.inner.endpoint_url()
    }

    /// The API key for "copy API key" (host copies it to the clipboard natively; never sent to the UI).
    pub fn api_key(&self) -> Option<klif_common::Secret> {
        self.inner.api_key()
    }

    /// Stop background threads (does NOT stop servers: they survive KLIF by design).
    pub fn shutdown(&self) {
        self.inner.shutdown()
    }

    // ---- extras (not part of the fixed API) ---------------------------------------------------

    /// Run one tick now (recompute and publish the view model). Tools use it to avoid waiting.
    pub fn refresh(&self) {
        self.inner.tick()
    }

    /// The engine's current recipe for a tier.
    pub fn recipe(&self, slot: SlotId) -> Option<klif_common::vm::Recipe> {
        self.inner.recipe(slot)
    }

    /// The inference card's AMD ULPS setting (EnableUlps in its driver key; read-only registry check).
    pub fn ulps_setting(&self) -> Option<klif_telemetry::UlpsSetting> {
        self.inner.ulps()
    }

    /// The state directory (state.json, catalog cache, logs of the shell).
    pub fn state_dir(&self) -> std::path::PathBuf {
        self.inner.state_dir()
    }
}

/// A launch plan as KLIF would build it right now, for inspection. Nothing is started.
pub struct PlanPreview {
    pub plan: klif_catalog::LaunchPlan,
    /// Why `launch` would refuse this plan (the catalog's file gate), if it would.
    pub refused: Option<String>,
    /// Whether an API key exists (its value is never exposed here).
    pub api_key_present: bool,
}

/// Build the launch plan for a tier from the persisted (or default) recipe, without starting the engine.
/// The plan carries the API key as an `EnvValue::Secret` when one exists: print names only.
pub fn preview_plan(cfg: &Config, slot: SlotId) -> Result<PlanPreview> {
    let catalog = klif_catalog::Catalog::load(cfg)?;
    let old = oldstate::read(cfg.launcher.state_file.as_deref());
    let persisted = state::load(&cfg.state_path("state.json"));
    let recipes = engine::initial_recipes(cfg, &catalog, &persisted, old.as_ref());
    let recipe = recipes.get(&slot).cloned().ok_or_else(|| anyhow::anyhow!("no recipe for {}", slot.as_str()))?;
    let key = if slot.kind() == klif_common::vm::SlotKind::Llm {
        oldstate::read_api_key(cfg.launcher.state_file.as_deref())
    } else {
        None
    };
    let stamp = timefmt::local_stamp();
    let (plan, refused) = match catalog.plan(cfg, slot, &recipe, key.as_ref(), stamp) {
        Ok(p) => (p, None),
        Err(e) => (catalog.plan_unchecked(cfg, slot, &recipe, key.as_ref(), stamp)?, Some(e.to_string())),
    };
    Ok(PlanPreview { plan, refused, api_key_present: key.is_some() })
}
