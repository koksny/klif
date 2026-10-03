//! The supervisor's input: one fully resolved process to start (no placeholders, no presets, no adapters).
//! Built by `klif_catalog::Catalog::plan`, consumed by `klif_supervisor::ProcessHost::launch`.

use std::path::PathBuf;

use crate::secret::{Secret, MASK};

/// Exactly what to start. `Debug` never prints a secret.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Absolute path of the program (already resolved; the supervisor never searches PATH).
    pub exe: PathBuf,
    /// Arguments after the program, final tokens (rendered with `klif_common::cmdline::render`).
    pub args: Vec<String>,
    /// Existing working directory.
    pub cwd: PathBuf,
    /// Inherited variables removed from the child's environment (case-insensitive names), before `env_set`.
    pub env_remove: Vec<String>,
    /// Variables set in the child's environment (user env + KLIF-managed env, API key as a secret).
    pub env_set: Vec<(String, EnvVal)>,
    /// `klif-<stamp>-<preset>-p<port>`: names the job (`Local\KLIF-<session>`) and the logs.
    pub session_name: String,
    pub out_log: PathBuf,
    pub err_log: PathBuf,
    /// The port the server will listen on (port-release wait on stop).
    pub port: u16,
}

/// One environment value for the child.
#[derive(Clone, PartialEq, Eq)]
pub enum EnvVal {
    Plain(String),
    Secret(Secret),
}

impl EnvVal {
    /// The real value (for the child's environment block only).
    pub fn expose(&self) -> &str {
        match self {
            EnvVal::Plain(s) => s,
            EnvVal::Secret(s) => s.expose(),
        }
    }

    pub fn is_secret(&self) -> bool {
        matches!(self, EnvVal::Secret(_))
    }

    /// The value as shown to people: plain values as they are, secrets as [`MASK`].
    pub fn display(&self) -> &str {
        match self {
            EnvVal::Plain(s) => s,
            EnvVal::Secret(_) => MASK,
        }
    }
}

impl std::fmt::Debug for EnvVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvVal::Plain(s) => f.debug_tuple("Plain").field(s).finish(),
            EnvVal::Secret(_) => f.debug_tuple("Secret").field(&MASK).finish(),
        }
    }
}
