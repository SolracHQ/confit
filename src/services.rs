//! Services
//!
//! Transformation logic plus the effect calls behind injected seams.
//! Specified inputs enter and structured outputs leave. Effects run
//! through repository and binding traits, never through direct disk use.

pub mod diff;
pub mod fold;
pub mod logging;
pub mod merge;
pub mod path;
pub mod plan;
pub mod render;
