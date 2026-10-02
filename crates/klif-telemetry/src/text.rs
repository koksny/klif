//! Small text helpers shared by the tailer and the parsers: redaction, sd-server tag handling,
//! number formatting.

/// Replacement for a console/log line that may carry a secret.
pub const REDACTED: &str = "[line hidden by KLIF: it mentions an API key]";

/// True when a line must never reach the UI. llama-server at `-lv 4` echoes the last 4 characters
/// of the key (`srv init_listene: api_keys: ****XXXX`); env dumps would carry the full value.
pub fn must_redact(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("api_key") || l.contains("api-key") || l.contains("authorization:") || l.contains("bearer ")
}

/// The line itself, or the redaction marker.
pub fn redact(line: String) -> String {
    if must_redact(&line) { REDACTED.to_string() } else { line }
}

/// sd-server prefixes lines with `[INFO ] ` / `[INFO   ] ` (and the Krea build prints the tag of the
/// previous message, so a tag on its own is an artifact). Returns the message without the tag.
pub fn strip_sd_tag(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let tag = rest[..end].trim();
            if matches!(tag, "INFO" | "WARN" | "ERROR" | "DEBUG") {
                return rest[end + 1..].trim_start();
            }
        }
    }
    line
}

/// A segment that is only an sd-server level tag (`[INFO ] `), left behind by the tag-shift bug.
pub fn is_tag_only(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.starts_with('[') && strip_sd_tag(t).trim().is_empty()
}

pub fn round_to(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    let r = (v * p).round() / p;
    // No "-0.0" in the view model (an empty f64 sum is -0.0).
    if r == 0.0 || !r.is_finite() { 0.0 } else { r }
}

pub const MIB: f64 = 1024.0 * 1024.0;
pub const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// File name only (never a machine path in a UI sentence).
pub fn file_name(p: &str) -> &str {
    p.rsplit(['\\', '/']).next().unwrap_or(p)
}
