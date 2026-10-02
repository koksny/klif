//! Running `scripts/Export-KlifCatalog.ps1` and caching its output.
//!
//! The cache lives in the state directory:
//!
//! - `catalog.json`: the exporter's output (UTF-8, no BOM, written through `-OutFile`),
//! - `catalog.key.json`: the SHA-256 of the launcher, the GUI script and the exporter it was made from.
//!
//! The exporter only runs again when one of those three files changed.

use anyhow::{anyhow, bail, Context, Result};
use klif_common::config::Config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::data::{Data, RawHashes, SCHEMA};
use crate::sha256::sha256_file_hex;

const EXPORTER_FILE: &str = "Export-KlifCatalog.ps1";
const EXPORT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CacheKey {
    schema: String,
    launcher: String,
    gui: String,
    exporter: String,
}

/// How the data was obtained (for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Read from the cache; the exporter did not run.
    Cache,
    /// The exporter ran just now.
    Exported,
    /// The exporter failed; an older cache was used.
    StaleCache,
}

pub(crate) struct Loaded {
    pub data: Data,
    pub origin: Origin,
    pub warnings: Vec<String>,
}

pub(crate) fn load(cfg: &Config) -> Result<Loaded> {
    let launcher = cfg.launcher.script.clone();
    if !launcher.is_file() {
        bail!("Launcher script not found: {}", launcher.display());
    }
    let gui = match &cfg.launcher.gui_script {
        Some(p) => Some(p.clone()),
        // The exporter defaults to the GUI script beside the launcher.
        None => launcher.parent().map(|d| d.join("Launch-LLM.Gui.ps1")).filter(|p| p.is_file()),
    };
    if let Some(g) = &gui {
        if !g.is_file() {
            bail!("Launcher GUI script not found: {}", g.display());
        }
    }
    let exporter = locate_exporter(cfg)?;

    let key = CacheKey {
        schema: SCHEMA.to_string(),
        launcher: sha256_file_hex(&launcher).with_context(|| format!("hashing {}", launcher.display()))?,
        gui: match &gui {
            Some(g) => sha256_file_hex(g).with_context(|| format!("hashing {}", g.display()))?,
            None => String::new(),
        },
        exporter: sha256_file_hex(&exporter).with_context(|| format!("hashing {}", exporter.display()))?,
    };

    let out = cfg.state_path("catalog.json");
    let key_path = cfg.state_path("catalog.key.json");

    if let Some(data) = read_cache(&out, &key_path, &key) {
        return Ok(Loaded { data, origin: Origin::Cache, warnings: Vec::new() });
    }

    match run_exporter(&exporter, &launcher, gui.as_deref(), &out, &key) {
        Ok(data) => {
            let key_json = serde_json::to_vec_pretty(&key).context("serialising cache key")?;
            std::fs::write(&key_path, key_json).with_context(|| format!("writing {}", key_path.display()))?;
            Ok(Loaded { data, origin: Origin::Exported, warnings: Vec::new() })
        }
        Err(e) => {
            // A stale catalog is better than none: the launcher scripts are still there, only the export failed.
            if let Ok(bytes) = std::fs::read(&out) {
                if let Ok((_, data)) = Data::parse(&bytes) {
                    let msg = format!("Catalog may be out of date: the exporter failed ({e:#}).");
                    log::warn!("{msg}");
                    return Ok(Loaded { data, origin: Origin::StaleCache, warnings: vec![msg] });
                }
            }
            Err(e.context("exporting the launcher catalog failed"))
        }
    }
}

/// The cache is valid only if the key file matches AND the JSON itself records the same script hashes.
fn read_cache(out: &Path, key_path: &Path, key: &CacheKey) -> Option<Data> {
    let stored: CacheKey = serde_json::from_slice(&std::fs::read(key_path).ok()?).ok()?;
    if &stored != key {
        return None;
    }
    let bytes = std::fs::read(out).ok()?;
    let (hashes, data) = Data::parse(&bytes).ok()?;
    hashes_match(&hashes, key).then_some(data)
}

fn hashes_match(h: &RawHashes, key: &CacheKey) -> bool {
    h.launcher.eq_ignore_ascii_case(&key.launcher) && (key.gui.is_empty() || h.gui.eq_ignore_ascii_case(&key.gui))
}

fn run_exporter(exporter: &Path, launcher: &Path, gui: Option<&Path>, out: &Path, key: &CacheKey) -> Result<Data> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = out.with_extension("json.new");
    let err_log = out.with_extension("export.err");
    let _ = std::fs::remove_file(&tmp);

    let mut cmd = Command::new(powershell_exe());
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(exporter)
        .arg("-LauncherPath")
        .arg(launcher);
    if let Some(g) = gui {
        cmd.arg("-GuiShellPath").arg(g);
    }
    cmd.arg("-OutFile").arg(&tmp);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::from(std::fs::File::create(&err_log).with_context(|| format!("creating {}", err_log.display()))?));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().context("starting powershell.exe for the catalog exporter")?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait().context("waiting for the catalog exporter")? {
            Some(s) => break s,
            None if started.elapsed() > EXPORT_TIMEOUT => {
                // We started this process; it is ours to stop.
                let _ = child.kill();
                let _ = child.wait();
                bail!("the catalog exporter did not finish within {} s", EXPORT_TIMEOUT.as_secs());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    if !status.success() {
        let detail = std::fs::read_to_string(&err_log).unwrap_or_default();
        let first = detail.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no error output");
        bail!("the catalog exporter exited with {status}: {first}");
    }
    let _ = std::fs::remove_file(&err_log);

    let bytes = std::fs::read(&tmp).with_context(|| format!("reading {}", tmp.display()))?;
    let (hashes, data) = Data::parse(&bytes)?;
    if !hashes_match(&hashes, key) {
        let _ = std::fs::remove_file(&tmp);
        bail!("a launcher script changed while the catalog was being exported; try again");
    }
    // std::fs::rename replaces an existing destination on Windows.
    std::fs::rename(&tmp, out).with_context(|| format!("writing {}", out.display()))?;
    Ok(data)
}

/// `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe` (the old launcher runs Windows PowerShell 5.1).
pub(crate) fn powershell_exe() -> PathBuf {
    match std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")) {
        Some(root) => PathBuf::from(root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"),
        None => PathBuf::from("powershell.exe"),
    }
}

/// `cfg.launcher.exporter`, else `scripts/Export-KlifCatalog.ps1` found upward from the state directory
/// (`.local` -> repo root), the working directory and the executable (or next to / below the executable
/// for a packaged build).
fn locate_exporter(cfg: &Config) -> Result<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    let push_ancestors = |start: &Path, bases: &mut Vec<PathBuf>| {
        for a in start.ancestors() {
            if !a.as_os_str().is_empty() {
                bases.push(a.to_path_buf());
            }
        }
    };
    push_ancestors(&cfg.state_dir, &mut bases);
    if let Ok(cwd) = std::env::current_dir() {
        push_ancestors(&cwd, &mut bases);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    if let Some(d) = &exe_dir {
        push_ancestors(d, &mut bases);
    }

    let check = |p: PathBuf| -> Option<PathBuf> { p.is_file().then_some(p) };

    if let Some(configured) = &cfg.launcher.exporter {
        if configured.is_absolute() {
            if let Some(p) = check(configured.clone()) {
                return Ok(p);
            }
        } else {
            for b in &bases {
                if let Some(p) = check(b.join(configured)) {
                    return Ok(p);
                }
            }
        }
        return Err(anyhow!("The configured catalog exporter was not found: {}", configured.display()));
    }

    for b in &bases {
        if let Some(p) = check(b.join("scripts").join(EXPORTER_FILE)) {
            return Ok(p);
        }
    }
    if let Some(d) = &exe_dir {
        for p in [d.join(EXPORTER_FILE), d.join("resources").join("scripts").join(EXPORTER_FILE)] {
            if let Some(p) = check(p) {
                return Ok(p);
            }
        }
    }
    Err(anyhow!("scripts/{EXPORTER_FILE} not found (set launcher.exporter in klif.toml)"))
}
