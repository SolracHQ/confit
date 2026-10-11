//! Runtime
//! Destination writes, drift, hook runs, and checks.

#![deny(missing_docs)]

pub mod checks;
mod differ;
mod disk;
pub mod error;
mod hooks;
mod prepare;
mod render;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, find_executable};
pub use differ::{Differ, Drift, DriftOrder};
pub use disk::{Live, LiveMember};
pub use hooks::{DecidedHooks, HookDecision, HookRunner, Spawned};
pub use prepare::{Prepared, prepare, refresh};
pub use render::render_document;
pub use write::Writer;
