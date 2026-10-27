//! Runtime
//! Manifests, bundles, destination writes, differences, hook runs, and checks.

#![deny(missing_docs)]

pub mod bundle;
pub mod checks;
mod differ;
#[cfg(test)]
mod fixtures;
mod hooks;
pub mod manifest;
pub mod prepare;
mod render;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, Unmet, find_executable, unmet_names};
pub use differ::{Differ, Difference, DifferenceOrder};
pub use hooks::{DecidedHooks, HookDecision, HookRunner, Spawned};
pub use prepare::{Prepared, prepare, refresh};
pub use render::render_document;
pub use write::Writer;
