//! Decode errors with a path to the failing value.
//!
//! An error carries the path from the root value and the reason the decode
//! failed. The rendered message puts the path before the reason.

use std::fmt::{Display, Formatter};

/// One step in a decode path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// A table key.
    Field(String),
    /// A 1-based sequence position.
    Index(u32),
}

/// The reason a decode failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeTag {
    /// A string outside the variant set.
    UnknownVariant {
        /// The value that was found.
        found: String,
        /// The valid variant strings.
        options: Vec<String>,
    },
    /// A value that fails the required shape.
    ShapeMismatch {
        /// The required shape.
        require: String,
        /// The value that was found.
        found: String,
    },
}

/// A decode failure with the path to the value that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// The path from the root value.
    pub segments: Vec<Segment>,
    /// The failure reason.
    pub reason: DecodeTag,
}

impl DecodeError {
    /// Renders the path with dots for fields and brackets for indices.
    pub fn path(&self) -> String {
        render_path(&self.segments)
    }
}

impl Display for DecodeTag {
    /// Renders the reason.
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

impl Display for DecodeError {
    /// Renders the path and the reason.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let path = self.path();
        if path.is_empty() {
            write!(formatter, "{}", self.reason)
        } else {
            write!(formatter, "{path}: {}", self.reason)
        }
    }
}

/// Renders segments with dots for fields and brackets for indices.
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
