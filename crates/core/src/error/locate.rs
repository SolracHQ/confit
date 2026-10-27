//! Layer wraps for results carrying convertible errors.

use std::path::PathBuf;

use super::{ErrorCause, ErrorContext, Result};

/// Layer wraps for results carrying convertible errors.
pub trait Locate<T> {
    /// Wrap one route layer over one convertible failure.
    ///
    /// # Errors
    ///
    /// Returns the mapped error with its route layer.
    fn route(self, route: impl Into<String>) -> Result<T>;
    /// Wrap one path layer over one convertible failure.
    ///
    /// # Errors
    ///
    /// Returns the mapped error with its path layer.
    fn path(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T, E> Locate<T> for std::result::Result<T, E>
where
    E: Into<ErrorCause>,
{
    /// Wrap one route layer over one convertible failure.
    ///
    /// # Errors
    ///
    /// Returns the mapped error with its route layer.
    fn route(self, route: impl Into<String>) -> Result<T> {
        self.map_err(|error| {
            let cause: ErrorCause = error.into();
            ErrorContext::from(cause).route(route)
        })
    }

    /// Wrap one path layer over one convertible failure.
    ///
    /// # Errors
    ///
    /// Returns the mapped error with its path layer.
    fn path(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|error| {
            let cause: ErrorCause = error.into();
            ErrorContext::from(cause).path(path)
        })
    }
}

impl<T> Locate<T> for std::result::Result<T, ErrorContext> {
    /// Wrap one route layer over one built error.
    ///
    /// # Errors
    ///
    /// Returns the error with its route layer.
    fn route(self, route: impl Into<String>) -> Result<T> {
        self.map_err(|error| error.route(route))
    }

    /// Wrap one path layer over one built error.
    ///
    /// # Errors
    ///
    /// Returns the error with its path layer.
    fn path(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|error| error.path(path))
    }
}
