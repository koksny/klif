//! The KLIF API key: `[security] api_key = "file"` (`<state_dir>\api-key.txt`, trimmed), `"env:NAME"` or
//! `"none"`. Injected into the adapter's key env var (independent of `managed`; preset `api_key = false` opts
//! out). Never logged, never sent to JS (only `set: bool`). Owner: package E2.

use anyhow::{bail, Context, Result};
use klif_common::config::{ApiKeySource, Config};
use klif_common::vm::ApiKeyInfo;
use klif_common::Secret;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// The `file` source's file name in the state dir.
pub const KEY_FILE: &str = "api-key.txt";

/// The `file` source's path: `<state_dir>\api-key.txt`.
pub fn key_file(cfg: &Config) -> PathBuf {
    cfg.state_path(KEY_FILE)
}

/// The configured key, if one is set.
pub fn load(cfg: &Config) -> Option<Secret> {
    match cfg.security.api_key_source() {
        ApiKeySource::File => read_key_file(&key_file(cfg)),
        ApiKeySource::Env(name) => std::env::var(&name).ok().and_then(Secret::new),
        ApiKeySource::None => None,
    }
}

fn read_key_file(path: &Path) -> Option<Secret> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Secret::new(text.trim_start_matches('\u{feff}'))
}

/// Set (Some) or clear (None) the key for the `file` source; refused for `env:NAME` / `none` (a sentence).
/// The file is replaced atomically (temp file + rename); clearing removes it.
pub fn store(cfg: &Config, key: Option<&Secret>) -> Result<()> {
    match cfg.security.api_key_source() {
        ApiKeySource::File => {}
        ApiKeySource::Env(name) => bail!(
            "The API key comes from the environment variable {name} ([security] api_key = \"env:{name}\"); set it there, or switch to api_key = \"file\" in klif.toml."
        ),
        ApiKeySource::None => bail!(
            "API keys are switched off ([security] api_key = \"none\"); set api_key = \"file\" in klif.toml to store one."
        ),
    }
    let path = key_file(cfg);
    match key {
        Some(k) => {
            if k.expose().chars().any(|c| c.is_control()) {
                bail!("The API key must be one line without control characters.");
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("The folder {} could not be created", dir.display()))?;
            }
            // Unix: readable by its owner only (Windows: as before, the profile's ACL protects it).
            #[cfg(unix)]
            crate::wire::write_secret_atomic(&path, k.expose().as_bytes()).with_context(|| format!("{} could not be written", path.display()))?;
            #[cfg(not(unix))]
            write_atomic(&path, k.expose().as_bytes()).with_context(|| format!("{} could not be written", path.display()))?;
        }
        None => match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => bail!("{} could not be removed: {e}", path.display()),
        },
    }
    *CACHE.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(())
}

/// Write `bytes` to `path` through a temp file in the same folder and a rename (retried briefly when another
/// program holds the file). Shared by `bench` (bench files).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!("{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    let mut last = None;
    for attempt in 0..10 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(30 * (attempt + 1)));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

/// The key file as last seen: (path, mtime, len, set).
type Seen = (PathBuf, Option<SystemTime>, u64, bool);
/// `info` runs on every engine tick; the file is only re-read when it changed.
static CACHE: Mutex<Option<Seen>> = Mutex::new(None);

/// Source + whether a key is set (cheap: called every engine tick).
pub fn info(cfg: &Config) -> ApiKeyInfo {
    let source = cfg.security.api_key_source();
    let set = match &source {
        ApiKeySource::File => file_key_set(&key_file(cfg)),
        ApiKeySource::Env(name) => std::env::var(name).map(|v| !v.trim().is_empty()).unwrap_or(false),
        ApiKeySource::None => false,
    };
    ApiKeyInfo { source: source.as_info(), set }
}

fn file_key_set(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let (mtime, len) = (meta.modified().ok(), meta.len());
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((p, m, l, set)) = cache.as_ref() {
        if p == path && *m == mtime && *l == len && mtime.is_some() {
            return *set;
        }
    }
    let set = read_key_file(path).is_some();
    *cache = Some((path.to_path_buf(), mtime, len, set));
    set
}
