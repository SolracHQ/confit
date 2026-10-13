//! Engine
//! Evaluate Lua profiles into documents with hooks.

pub mod api;
pub mod assembly;
pub mod engine;
pub mod error;
mod loader;
pub mod project;
pub mod stubs;

pub use assembly::assemble;
pub use engine::{Engine, Evaluated};
pub use error::{EngineError, Result};
