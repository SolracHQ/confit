//! Structured decode faults with bracket paths.

use std::fmt::{Display, Formatter};

/// One path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// One table key.
    Field(String),
    /// One 1-based sequence position.
    Index(u32),
}

/// One face fault reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultReason {
    /// Unknown variant.
    UnknownVariant {
        /// The found value text.
        found: String,
        /// The valid option texts.
        options: Vec<String>,
    },
    /// Shape mismatch.
    ShapeMismatch {
        /// The required shape text.
        require: String,
        /// The found shape text.
        found: String,
    },
}

/// One structured face fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceFault {
    /// Path segments from the profile root.
    pub segments: Vec<Segment>,
    /// The fault reason.
    pub reason: FaultReason,
}

impl FaceFault {
    /// Return the bracket path from the first segment.
    pub fn path(&self) -> String {
        render_path(&self.segments)
    }
}

impl Display for FaultReason {
    /// Render one fault reason.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariant { found, options } => write!(
                formatter,
                "{found} is not a valid variant, choose one of {}",
                options.join(", ")
            ),
            Self::ShapeMismatch { require, found } => {
                write!(formatter, "require {require}, found {found}")
            }
        }
    }
}

impl Display for FaceFault {
    /// Render one face fault with its bracket path.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let path = self.path();
        if path.is_empty() {
            write!(formatter, "{}", self.reason)
        } else {
            write!(formatter, "{path}: {}", self.reason)
        }
    }
}

/// Render one bracket path from root segments.
fn render_path(segments: &[Segment]) -> String {
    let mut path = String::new();
    for segment in segments {
        match segment {
            Segment::Field(name) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
            }
            Segment::Index(position) => {
                path.push_str(&format!("[{position}]"));
            }
        }
    }
    path
}
