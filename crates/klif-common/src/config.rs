//! KLIF configuration (`klif.toml`). Machine-specific values live ONLY in the local config file,
//! never in tracked code. See `config/klif.example.toml` for the documented template.
//!
//! Lookup order:
//!   1. env `KLIF_CONFIG` (path to a toml file)
//!   2. `.local/klif.toml`, searched upward from the current directory and from the executable's directory
//!   3. `%APPDATA%\KLIF\klif.toml`
//! The directory of the config file is the state directory (state.json, catalog cache).

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::vm::SlotId;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub launcher: LauncherCfg,
    #[serde(default)]
    pub net: NetCfg,
    #[serde(default)]
    pub gpu: GpuCfg,
    #[serde(default)]
    pub tiers: TiersCfg,
    #[serde(default)]
    pub ui: UiCfg,
    #[serde(default)]
    pub telemetry: TelemetryCfg,
    /// Filled by `Config::load`: the file it came from and its directory.
    #[serde(skip)]
    pub source: PathBuf,
    #[serde(skip)]
    pub state_dir: PathBuf,
}

/// The existing PowerShell launcher, which stays the single source of truth for the catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct LauncherCfg {
    /// Path to Launch-LLM.ps1.
    pub script: PathBuf,
    /// Path to Launch-LLM.Gui.ps1 (the exporter reads its launch envelope).
    pub gui_script: Option<PathBuf>,
    /// The old launcher's state file: read-only source of the API key and of runs to adopt.
    pub state_file: Option<PathBuf>,
    /// The catalog exporter script. Default: `scripts/Export-KlifCatalog.ps1` in the repo.
    pub exporter: Option<PathBuf>,
    /// Where session logs go. Default: the catalog's runtime-logs root.
    pub logs_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetCfg {
    /// Host the starters bind LLM servers to (used for probes and the endpoint chip).
    pub llm_host: String,
    /// Host for image servers.
    pub image_host: String,
}

impl Default for NetCfg {
    fn default() -> Self {
        Self { llm_host: "127.0.0.1".into(), image_host: "127.0.0.1".into() }
    }
}

/// GPUs by PCI "VEN:DEV" (hex), resolved to adapters at runtime (LUIDs change every boot).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GpuCfg {
    /// The inference card (the VRAM cliff). Example: "1002:7550".
    pub inference: Option<String>,
    /// The adapter the UI must render on. Example: "1002:731F". None = first hardware adapter that
    /// is not the inference card.
    pub ui: Option<String>,
    /// Display name override for the inference card, e.g. "RX 9070 XT".
    pub inference_name: Option<String>,
}

/// Default recipe per tier: a catalog card plus overrides. Missing tiers fall back to the
/// launcher's presets (qwen-moe -> high, qwen-dense -> medium, gemma-tavern -> low, krea -> krea).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TiersCfg {
    pub high: Option<TierCfg>,
    pub medium: Option<TierCfg>,
    pub low: Option<TierCfg>,
    pub krea: Option<TierCfg>,
}

impl TiersCfg {
    pub fn get(&self, slot: SlotId) -> Option<&TierCfg> {
        match slot {
            SlotId::High => self.high.as_ref(),
            SlotId::Medium => self.medium.as_ref(),
            SlotId::Low => self.low.as_ref(),
            SlotId::Krea => self.krea.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TierCfg {
    /// Launcher preset id to start from ("qwen-dense", "qwen-moe", "gemma-tavern", "krea").
    pub preset: Option<String>,
    /// Or an explicit card id.
    pub card: Option<String>,
    pub backend: Option<String>,
    pub hardware: Option<String>,
    pub context: Option<u32>,
    pub image_size: Option<String>,
    pub kv: Option<String>,
    pub prompt_cache_mib: Option<u32>,
    pub port: Option<u16>,
    pub vision: Option<bool>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiCfg {
    /// Frameless window with skin-drawn chrome.
    #[serde(default = "default_frameless")]
    pub frameless: bool,
    /// Which monitor "Panel mode" (the read-only mini layout) fills: a "WxH" resolution such as "960x640"
    /// or a substring of the monitor's device name such as "DISPLAY3". Unset = automatic: with two or more
    /// monitors, the one with the fewest pixels, if it is at most 1280x800.
    #[serde(default)]
    pub panel_monitor: Option<String>,
}

fn default_frameless() -> bool {
    true
}

impl Default for UiCfg {
    fn default() -> Self {
        Self { frameless: true, panel_monitor: None }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryCfg {
    /// Free VRAM below which skins warn.
    pub warn_below_gib: f64,
    /// Ask llama-server for verbose load logs (VRAM composition) via the environment.
    pub verbose_llama_logs: bool,
}

impl Default for TelemetryCfg {
    fn default() -> Self {
        Self { warn_below_gib: 0.15, verbose_llama_logs: true }
    }
}

impl Config {
    pub fn load() -> Result<Config> {
        let path = Self::locate().ok_or_else(|| {
            anyhow!("no klif.toml found (set KLIF_CONFIG, or create .local/klif.toml or %APPDATA%\\KLIF\\klif.toml)")
        })?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let text = text.trim_start_matches('\u{feff}');
        let mut cfg: Config = toml::from_str(text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.source = path.to_path_buf();
        cfg.state_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(cfg)
    }

    pub fn locate() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("KLIF_CONFIG") {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
        }
        let mut starts = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            starts.push(cwd);
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            starts.push(dir);
        }
        for start in starts {
            for dir in start.ancestors() {
                let cand = dir.join(".local").join("klif.toml");
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            let cand = PathBuf::from(appdata).join("KLIF").join("klif.toml");
            if cand.is_file() {
                return Some(cand);
            }
        }
        None
    }

    /// state.json, catalog cache etc. live next to the config file.
    pub fn state_path(&self, name: &str) -> PathBuf {
        self.state_dir.join(name)
    }
}
