//! Engine
//! Evaluate Lua profiles into documents with hooks.

pub mod assembly;
pub mod engine;
pub mod error;
mod loader;

pub use assembly::assemble;
pub use engine::{Engine, Evaluated};
pub use error::{EngineError, Result};
