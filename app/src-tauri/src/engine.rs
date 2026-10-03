//! The engine that drives the shell: the `klif_core` engine. The UI has its own mock engine for browser development.

pub use klif_core::{Engine, EngineHandle, StartError};

pub const KIND: &str = "klif-core";
