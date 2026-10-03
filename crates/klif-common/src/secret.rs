//! Secret values and the one set of rules for what counts as secret.
//!
//! - [`Secret`]: an API key or token. Never serialized, never printed (Debug and Display show [`MASK`]);
//!   [`Secret::expose`] is the only way to read it (child environment block, Authorization header).
//! - [`is_secret_env`]: environment variable names whose values are masked everywhere KLIF shows or logs
//!   them (CommandView, PresetDetail, Debug output, diag).
//! - [`SECRET_ARG_FLAGS`] / [`mask_args`]: command-line flags whose VALUE is a secret (`--api-key x`,
//!   `--api-key=x`). The value is masked in every display; `--api-key-file` takes a path and is not secret.
//!
//! Every package uses these rules; none invents its own.

use std::fmt;

/// What a masked value looks like everywhere (UI, CLI, logs). A preset sent back with this value means
/// "keep the stored value" (resolved by `klif_catalog::store::upsert_preset`).
pub const MASK: &str = "••••";

/// Flags whose following token (or `=value` part) is a secret.
pub const SECRET_ARG_FLAGS: &[&str] = &["--api-key", "--hf-token", "-hft"];

/// A secret string (API key, token). No `Serialize`; Debug and Display print [`MASK`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Trims the value; `None` when it is empty.
    pub fn new(s: impl Into<String>) -> Option<Secret> {
        let s = s.into();
        let t = s.trim();
        if t.is_empty() { None } else { Some(Secret(t.to_string())) }
    }
    /// The only way to read the value. Use it for a child's environment block or an Authorization
    /// header, nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Best effort: overwrite the bytes before the allocation is freed.
        // SAFETY: zero bytes are valid UTF-8; the string is not used afterwards.
        unsafe {
            for b in self.0.as_bytes_mut() {
                std::ptr::write_volatile(b, 0);
            }
        }
    }
}

/// True for environment variable names whose value is secret: case-insensitive `KEY`, `TOKEN`, `SECRET`,
/// `PASS` or `AUTH` anywhere in the name (this covers `HF_TOKEN`, `LLAMA_API_KEY`, `VLLM_API_KEY`).
/// Deliberately broad: masking a harmless value costs nothing, leaking a key does.
pub fn is_secret_env(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    const PARTS: [&str; 5] = ["KEY", "TOKEN", "SECRET", "PASS", "AUTH"];
    const EXACT: [&str; 3] = ["HF_TOKEN", "LLAMA_API_KEY", "VLLM_API_KEY"];
    EXACT.contains(&n.as_str()) || PARTS.iter().any(|p| n.contains(p))
}

/// True when `token` is one of [`SECRET_ARG_FLAGS`] (exact, case-sensitive like the servers' parsers).
pub fn is_secret_flag(token: &str) -> bool {
    SECRET_ARG_FLAGS.contains(&token)
}

/// The secret flag a `--flag=value` token starts with, if any.
fn secret_flag_prefix(token: &str) -> Option<&'static str> {
    SECRET_ARG_FLAGS.iter().copied().find(|f| token.len() > f.len() && token.starts_with(f) && token.as_bytes()[f.len()] == b'=')
}

/// `args` with every secret value replaced by [`MASK`]: the token after a secret flag, and the value part
/// of `--flag=value`. Tokens are never split or merged.
pub fn mask_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut mask_next = false;
    for a in args {
        if mask_next {
            out.push(MASK.to_string());
            mask_next = false;
            continue;
        }
        if is_secret_flag(a) {
            mask_next = true;
            out.push(a.clone());
        } else if let Some(flag) = secret_flag_prefix(a) {
            out.push(format!("{flag}={MASK}"));
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// The secret values of `args` as written (the positions [`mask_args`] masks): the token after a secret flag
/// and the value part of `--flag=value`.
pub fn secret_arg_values(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut take_next = false;
    for a in args {
        if take_next {
            out.push(a.as_str());
            take_next = false;
        } else if is_secret_flag(a) {
            take_next = true;
        } else if let Some(flag) = secret_flag_prefix(a) {
            out.push(&a[flag.len() + 1..]);
        }
    }
    out
}

/// True when `args` carry a secret value literally (a warn issue: a key in args is visible to every local
/// process through the command line; prefer the API-key env / `api_key = true`).
pub fn args_have_secret(args: &[String]) -> bool {
    args.iter().enumerate().any(|(i, a)| (is_secret_flag(a) && i + 1 < args.len()) || secret_flag_prefix(a).is_some())
}
