//! Non-blocking diff warnings.
//!
//! A warning holds a difference, and the run continues.

use super::contextual::ContextualCause;

/// A warning for one route whose diff failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffWarning {
    route: String,
    contextual: ContextualCause,
}

impl DiffWarning {
    /// Wraps one cause into a warning for the route.
    pub fn warn(route: impl Into<String>, contextual: ContextualCause) -> Self {
        Self {
            route: route.into(),
            contextual,
        }
    }

    /// Returns the route that the warning concerns.
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Returns the cause that failed.
    pub fn cause(&self) -> &super::cause::ErrorCause {
        self.contextual.cause()
    }

    /// Returns the location where the operation ran.
    pub fn context(&self) -> &super::context::ErrorContext {
        self.contextual.context()
    }

    /// Formats one warning line.
    pub fn summary(&self) -> String {
        format!(
            "Cannot state the difference for '{}': {}",
            self.route,
            self.contextual.cause()
        )
    }
}
