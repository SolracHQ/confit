//! Non-blocking diff warnings.
//!
//! Warnings name the difference the run states without blocking it.

use super::contextual::ContextualCause;

/// Non-blocking diff warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffWarning {
    route: String,
    contextual: ContextualCause,
}

impl DiffWarning {
    /// Build one diff warning.
    pub fn warn(route: impl Into<String>, contextual: ContextualCause) -> Self {
        Self {
            route: route.into(),
            contextual,
        }
    }

    /// Return the route under stating.
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Return the failed result.
    pub fn cause(&self) -> &super::cause::ErrorCause {
        self.contextual.cause()
    }

    /// Return where the operation ran.
    pub fn context(&self) -> &super::context::ErrorContext {
        self.contextual.context()
    }

    /// Render one honest warning line.
    pub fn summary(&self) -> String {
        format!(
            "Cannot state the difference for '{}': {}",
            self.route,
            self.contextual.cause()
        )
    }
}
