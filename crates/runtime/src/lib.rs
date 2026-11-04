//! Destination writes, differences, hook runs, and checks.

#![deny(missing_docs)]

pub mod checks;
mod differ;
mod hooks;
pub mod prepare;
pub mod snapshot;
pub mod status;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, Unmet, find_executable, unmet_names};
pub use differ::{Differ, Difference, DifferenceOrder};
pub use hooks::{DecidedHooks, HookDecision, HookRunner, Spawned};
pub use prepare::{Prepared, prepare, refresh};
pub use write::Writer;
