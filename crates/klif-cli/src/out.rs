//! Output contract: `--json` prints exactly ONE JSON document on stdout, `{schemaVersion: 1, ...}`; a failure is
//! `{schemaVersion: 1, error: {code, message}}` with exit code 1 (usage errors: exit code 2). Without `--json` the
//! result is human text on stdout, errors on stderr. Progress notes always go to stderr.

use serde::Serialize;
use serde_json::{Map, Value};

/// The CLI's JSON schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Set by the commands that print JSON lines (`watch`, `logs --follow`): their failure document is one line too.
static STREAMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_streaming() {
    STREAMING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// A failure: a stable code, one sentence, the process exit code.
#[derive(Debug)]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
    pub exit: u8,
}

pub type CliResult<T = ()> = Result<T, CliError>;

impl CliError {
    /// Exit 1.
    pub fn new(code: &'static str, message: impl Into<String>) -> CliError {
        CliError { code, message: message.into(), exit: 1 }
    }
    /// Bad command line: exit 2.
    pub fn usage(message: impl Into<String>) -> CliError {
        CliError { code: "usage", message: message.into(), exit: 2 }
    }
    /// A command that changes something was given without `--yes`: exit 2.
    pub fn needs_yes(message: impl Into<String>) -> CliError {
        CliError { code: "needs_yes", message: message.into(), exit: 2 }
    }
}

impl From<anyhow::Error> for CliError {
    fn from(e: anyhow::Error) -> CliError {
        CliError::new("error", format!("{e:#}"))
    }
}

/// An engine refusal (an action's sentence).
pub fn refused(e: anyhow::Error) -> CliError {
    CliError::new("refused", format!("{e:#}"))
}

/// Where results go.
#[derive(Debug, Clone, Copy)]
pub struct Out {
    pub json: bool,
}

impl Out {
    /// The command's result: `value` (a JSON object) with `schemaVersion` in JSON mode, else `human()`.
    pub fn doc(&self, value: Value, human: impl FnOnce() -> String) {
        if self.json {
            println!("{}", json_doc(value));
        } else {
            let text = human();
            if text.ends_with('\n') {
                print!("{text}");
            } else {
                println!("{text}");
            }
        }
    }

    /// One event of a stream (`watch`, `logs --follow`): with `--json` a compact JSON line carrying `schemaVersion`,
    /// else `human()`. A reader that went away (a closed pipe) ends the command quietly.
    pub fn event(&self, value: Value, human: impl FnOnce() -> String) {
        use std::io::Write;
        let text = if self.json { json_line(value) } else { human() };
        let mut stdout = std::io::stdout().lock();
        if writeln!(stdout, "{text}").and_then(|_| stdout.flush()).is_err() {
            std::process::exit(0);
        }
    }

    /// The error document (JSON) or line (stderr).
    pub fn error(&self, e: &CliError) {
        if self.json {
            let mut m = Map::new();
            m.insert("code".into(), Value::String(e.code.into()));
            m.insert("message".into(), Value::String(e.message.clone()));
            let mut doc = Map::new();
            doc.insert("error".into(), Value::Object(m));
            if STREAMING.load(std::sync::atomic::Ordering::Relaxed) {
                println!("{}", json_line(Value::Object(doc)));
            } else {
                println!("{}", json_doc(Value::Object(doc)));
            }
        } else {
            eprintln!("klif-cli: {}", e.message);
        }
    }
}

/// A progress / information line on stderr (both modes; stdout stays one document).
pub fn note(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

/// One compact JSON line `{schemaVersion: 1, ...}` (stream events).
pub fn json_line(value: Value) -> String {
    let mut doc = Map::new();
    doc.insert("schemaVersion".into(), Value::from(SCHEMA_VERSION));
    if let Value::Object(m) = value {
        doc.extend(m);
    }
    serde_json::to_string(&Value::Object(doc)).unwrap_or_else(|_| "{\"schemaVersion\":1}".into())
}

fn json_doc(value: Value) -> String {
    let mut doc = Map::new();
    doc.insert("schemaVersion".into(), Value::from(SCHEMA_VERSION));
    match value {
        Value::Object(m) => doc.extend(m),
        Value::Null => {}
        other => {
            doc.insert("result".into(), other);
        }
    }
    serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_else(|_| "{\"schemaVersion\":1}".into())
}

/// `serde_json::to_value` that never fails the command (non-serializable values become null).
pub fn val<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// A small text table: columns padded to their widest cell (the last column is not padded).
pub fn table(rows: &[Vec<String>]) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut width = vec![0usize; cols];
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            width[i] = width[i].max(c.chars().count());
        }
    }
    let mut out = String::new();
    for r in rows {
        let mut line = String::new();
        for (i, c) in r.iter().enumerate() {
            if i + 1 < r.len() {
                line.push_str(c);
                line.push_str(&" ".repeat(width[i] - c.chars().count() + 2));
            } else {
                line.push_str(c);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// "12.3" / "-" for an optional number.
pub fn num(v: Option<f64>, digits: usize) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x:.digits$}"),
        _ => "-".into(),
    }
}

/// Bytes as GiB text.
pub fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Epoch seconds as "YYYY-MM-DD HH:MM UTC".
pub fn date(epoch: f64) -> String {
    if !epoch.is_finite() || epoch <= 0.0 {
        return "-".into();
    }
    let secs = epoch as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}
