//! Lua evaluation leaf: `confit` global plus profile loader.
//!
//! Lua constructs model data for the plan service to consume. Plan works
//! on data; `apply` renders bytes (minijinja, shell codegen).

pub mod bindings;
pub mod eval;

// Re-exported graph types for the plan service.
pub use bindings::{InitEntryShape, MiseSpec, ToolBuilder, ToolContribution};
pub use eval::{ProfileGraph, evaluate};
