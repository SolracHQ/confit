//! Context error layers for the log chain.

use std::path::PathBuf;

use super::cause::{ErrorCause, EvalError};

/// Structured error chain piece for logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Document route under use.
    Route {
        /// The route text under use.
        route: String,
    },
    /// Filesystem path under use.
    Path {
        /// The filesystem path under use.
        path: String,
    },
    /// Innermost failure detail.
    Leaf {
        /// The leaf failure text.
        detail: String,
    },
    /// Ordered require chain under use.
    Cycle {
        /// The require names in load order.
        chain: Vec<String>,
    },
}

/// Shared error with context layers for the log chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorContext {
    /// Leaf cause.
    Leaf(ErrorCause),
    /// Document route under use with its inner error.
    Route {
        /// The route text under use.
        route: String,
        /// The inner error.
        inner: Box<ErrorContext>,
    },
    /// Filesystem path under use with its inner error.
    AtPath {
        /// The filesystem path under use.
        path: PathBuf,
        /// The inner error.
        inner: Box<ErrorContext>,
    },
}

impl std::fmt::Display for ErrorContext {
    /// Render one log chain segment with its inner segments.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Leaf(cause) => write!(formatter, "{cause}"),
            Self::AtPath { path, inner } => write!(formatter, "{inner} at '{}'", path.display()),
            Self::Route { route, inner } => write!(formatter, "document '{route}' | {inner}"),
        }
    }
}

impl std::error::Error for ErrorContext {
    /// Report one inner error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner().map(|inner| inner as _)
    }
}

impl<T> From<T> for ErrorContext
where
    T: Into<ErrorCause>,
{
    /// Wrap one cause-like value as a leaf error.
    fn from(cause: T) -> Self {
        Self::new(cause.into())
    }
}

/// Shared error result alias.
pub type Result<T> = std::result::Result<T, ErrorContext>;

impl ErrorContext {
    /// Build one error from one cause.
    pub fn new(cause: ErrorCause) -> Self {
        Self::Leaf(cause)
    }

    /// Report one inner error.
    pub fn inner(&self) -> Option<&ErrorContext> {
        match self {
            Self::Leaf(_) => None,
            Self::Route { inner, .. } | Self::AtPath { inner, .. } => Some(inner),
        }
    }

    /// Report the innermost cause of one chain.
    pub fn root_cause(&self) -> &ErrorCause {
        let mut current = self;
        loop {
            match current {
                Self::Leaf(cause) => return cause,
                Self::Route { inner, .. } | Self::AtPath { inner, .. } => current = inner,
            }
        }
    }

    /// Report outermost-first pieces of one chain.
    pub fn segments(&self) -> Vec<Segment> {
        let mut pieces = Vec::new();
        let mut current = self;
        loop {
            match current {
                Self::Route { route, inner } => {
                    pieces.push(Segment::Route {
                        route: route.clone(),
                    });
                    current = inner;
                }
                Self::AtPath { path, inner } => {
                    pieces.push(Segment::Path {
                        path: path.display().to_string(),
                    });
                    current = inner;
                }
                Self::Leaf(cause) => {
                    match cause {
                        ErrorCause::EvalError(EvalError::CycleDetected { cycle }) => {
                            pieces.push(Segment::Cycle {
                                chain: cycle.clone(),
                            });
                        }
                        _ => {
                            pieces.push(Segment::Leaf {
                                detail: cause.to_string(),
                            });
                        }
                    }
                    break;
                }
            }
        }
        pieces
    }

    /// Wrap one route layer and return the error.
    pub fn route(self, route: impl Into<String>) -> Self {
        Self::Route {
            route: route.into(),
            inner: Box::new(self),
        }
    }

    /// Wrap one path layer and return the error.
    pub fn path(self, path: impl Into<PathBuf>) -> Self {
        Self::AtPath {
            path: path.into(),
            inner: Box::new(self),
        }
    }
}

/// Build one failure result from one cause-like value.
pub fn fail<C, T>(cause: C) -> Result<T>
where
    C: Into<ErrorCause>,
{
    Err(ErrorContext::new(cause.into()))
}

impl From<mlua::Error> for ErrorContext {
    /// Map one Lua failure into shared language keeping carried errors.
    fn from(error: mlua::Error) -> Self {
        if let Some(known) = error.downcast_ref::<ErrorContext>().cloned() {
            return known;
        }
        Self::from(EvalError::LuaFailed {
            message: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorContext;
    use super::Segment;
    use crate::error::cause::ErrorCause;
    use crate::error::cause::FilesystemError;

    #[test]
    fn segments_reports_route_over_path_over_leaf() {
        let error = ErrorContext::new(ErrorCause::FilesystemError(FilesystemError::FileMissing))
            .path("/tmp/x")
            .route("doc.route");
        assert_eq!(
            error.segments(),
            vec![
                Segment::Route {
                    route: "doc.route".to_owned(),
                },
                Segment::Path {
                    path: "/tmp/x".to_owned(),
                },
                Segment::Leaf {
                    detail: "missing file".to_owned(),
                },
            ]
        );
    }
}
