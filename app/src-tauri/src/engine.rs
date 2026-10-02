//! Which engine drives the shell: the real `klif_core` engine, or the sample-data stub (feature
//! `stub-engine`). Both expose the same `Engine::start` / `EngineHandle` API.

#[cfg(feature = "stub-engine")]
pub use crate::stub::{Engine, EngineHandle};

#[cfg(not(feature = "stub-engine"))]
pub use klif_core::{Engine, EngineHandle};

pub const KIND: &str = if cfg!(feature = "stub-engine") { "stub" } else { "klif-core" };
