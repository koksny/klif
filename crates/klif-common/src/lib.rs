//! Shared contract for the KLIF native core: the view model (mirror of the UI's types.ts), configuration
//! (`klif.toml`), the secret rules, command-line rendering and the supervisor's launch input.
//!
//! - `config`: klif.toml 0.3 (lookup, presets, params, tiers), `load()` never fails.
//! - `vm`: the view model and `Action` (serde camelCase, mirrored in app/ui/src/lib/model/types.ts).
//! - `secret`: `Secret`, `is_secret_env`, `SECRET_ARG_FLAGS`, `mask_args`, `MASK`.
//! - `cmdline`: `render(exe, args)`, the exact command line shown AND run.
//! - `launch`: `LaunchSpec` / `EnvVal`, what the supervisor starts.

pub mod cmdline;
pub mod config;
pub mod launch;
pub mod secret;
pub mod vm;

pub use secret::Secret;

/// The KLIF version (workspace version), e.g. "0.3.1".
pub const KLIF_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Seconds since the Unix epoch as f64 (the view model's time unit).
pub fn now_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
