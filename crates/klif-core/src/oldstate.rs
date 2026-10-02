//! The old PowerShell launcher's state file (`launcher.state_file`), read-only.
//!
//! KLIF reads three things from it: the defaults the old GUI carried (KV type, vision, mode, prompt cache,
//! port), the run it may have left behind (`RuntimeProcesses` + `RuntimeOutLog`/`RuntimeErrLog`), and the
//! API key. The key is only ever held as a `Secret`; the parsed JSON tree is never logged or printed.

use klif_catalog::OldLauncherDefaults;
use klif_common::Secret;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What the old launcher's state says (everything but the key).
#[derive(Debug, Clone, Default)]
pub struct OldState {
    pub defaults: OldLauncherDefaults,
    /// `RuntimeProcesses`: (PID, StartTimeUtcTicks).
    pub runtime: Vec<(u32, i64)>,
    pub out_log: Option<PathBuf>,
    pub err_log: Option<PathBuf>,
    pub backend: Option<String>,
    pub hardware: Option<String>,
    /// Context of the old GUI's selection (tokens, or the W*10000+H image-size encoding).
    pub context: Option<u32>,
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).ok()
}

fn str_field(v: &Value, name: &str) -> Option<String> {
    v.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn u64_field(v: &Value, name: &str) -> Option<u64> {
    match v.get(name)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn i64_field(v: &Value, name: &str) -> Option<i64> {
    match v.get(name)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Parse the state file. None when there is no file or it is not JSON.
pub fn read(path: Option<&Path>) -> Option<OldState> {
    let v = read_json(path?)?;
    let vision = str_field(&v, "Vision").and_then(|s| match s.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Some(true),
        "off" | "false" | "0" => Some(false),
        _ => None,
    });
    let defaults = OldLauncherDefaults {
        cache_type: str_field(&v, "CacheType"),
        vision,
        generation_mode: str_field(&v, "GenerationMode"),
        prompt_cache_mib: u64_field(&v, "PromptCacheMiB").and_then(|x| u32::try_from(x).ok()),
        server_port: u64_field(&v, "ServerPort").and_then(|x| u16::try_from(x).ok()),
    };
    // RuntimeProcesses is an array of {Id, StartTimeUtcTicks}, or a single object when PowerShell
    // serialised a one-element array as a scalar.
    let mut runtime = Vec::new();
    let entries: Vec<&Value> = match v.get("RuntimeProcesses") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(o @ Value::Object(_)) => vec![o],
        _ => Vec::new(),
    };
    for e in entries {
        let pid = u64_field(e, "Id").and_then(|x| u32::try_from(x).ok());
        let ticks = i64_field(e, "StartTimeUtcTicks");
        if let (Some(pid), Some(ticks)) = (pid, ticks) {
            if pid > 4 && ticks > 0 {
                runtime.push((pid, ticks));
            }
        }
    }
    Some(OldState {
        defaults,
        runtime,
        out_log: str_field(&v, "RuntimeOutLog").map(PathBuf::from),
        err_log: str_field(&v, "RuntimeErrLog").map(PathBuf::from),
        backend: str_field(&v, "Backend"),
        hardware: str_field(&v, "Hardware"),
        context: u64_field(&v, "Context").and_then(|x| u32::try_from(x).ok()),
    })
}

/// The API key (`ApiKey`), read on demand. Never logged.
pub fn read_api_key(path: Option<&Path>) -> Option<Secret> {
    let v = read_json(path?)?;
    let key = v.get("ApiKey")?.as_str()?;
    Secret::new(key)
}

pub fn mtime(path: Option<&Path>) -> Option<SystemTime> {
    std::fs::metadata(path?).ok()?.modified().ok()
}

/// Card id and port from a session log name: `klif-<yyyyMMdd-HHmmss-fff>-<cardId>-p<port>.out.log`.
pub fn parse_session_log_name(path: &Path) -> Option<(String, u16)> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".out.log").or_else(|| name.strip_suffix(".err.log"))?;
    let rest = stem.strip_prefix("klif-")?;
    // The stamp is 19 characters (yyyyMMdd-HHmmss-fff) followed by '-'.
    let stamp = rest.get(..19)?;
    if !stamp.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return None;
    }
    let tail = rest.get(20..)?;
    let (card, port) = tail.rsplit_once("-p")?;
    let port: u16 = port.parse().ok()?;
    if card.is_empty() {
        return None;
    }
    Some((card.to_string(), port))
}

/// The old GUI's image-size encoding (W*10000+H) as a "WxH" label.
pub fn image_size_label(context: u32) -> Option<String> {
    let (w, h) = (context / 10000, context % 10000);
    (w > 0 && h > 0).then(|| format!("{w}x{h}"))
}
