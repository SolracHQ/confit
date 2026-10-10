//! Engine
//! Evaluate Lua profiles into finished manifests.

pub mod assembly;
pub mod engine;
pub mod error;
mod loader;

pub use assembly::assemble;
pub use engine::Engine;
pub use error::{EngineError, Result};
