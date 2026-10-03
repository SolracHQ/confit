//! Engine
//!
//! Lua profile evaluation into finished documents.

pub mod assembly;
pub mod engine;
pub mod error;
mod loader;

pub use assembly::assemble;
pub use engine::{Engine, Evaluation};
pub use error::{EngineError, Result};
