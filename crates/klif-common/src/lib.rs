//! Shared contract for the KLIF native core: the view model (mirror of the UI's types.ts),
//! configuration, and small shared types.

pub mod config;
pub mod vm;

use std::fmt;

/// A secret string (API key). Never printed: Debug and Display are redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Option<Secret> {
        let s = s.into();
        let t = s.trim();
        if t.is_empty() { None } else { Some(Secret(t.to_string())) }
    }
    /// The only way to read the value. Use it to build a child's environment block, nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Seconds since the Unix epoch as f64 (the view model's time unit).
pub fn now_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
