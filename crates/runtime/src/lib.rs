//! Destination writes, hook runs, and checks.

#![deny(missing_docs)]

pub mod checks;
mod hooks;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, Unmet, find_executable, unmet_names};
pub use hooks::{DecidedHooks, HookDecision, HookRunner, Spawned};
pub use write::Writer;
