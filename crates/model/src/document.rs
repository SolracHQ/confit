//! Document
//! Desired state payloads reaching disk.

pub mod rc;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::routes::Route;
use confit_binding::fault::FaceFault;
use confit_binding::value::Decode;
use confit_binding_macros::Face;
use confit_core::error::{DocumentError, ErrorContext, Result};
use confit_store::handles::ResourceHandle;
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

pub use rc::{RcData, RcEntry};

/// Default unix permission bits for files without explicit modes.
pub const DEFAULT_FILE_MODE: u32 = 0o644;

/// Serialization format for structured documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Face)]
#[serde(rename_all = "lowercase")]
#[face(table, string)]
pub enum StructuredFormat {
    /// JSON format.
    Json,
    /// TOML format.
    Toml,
    /// YAML format.
    Yaml,
}

impl StructuredFormat {
    /// Return the lowercase format name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    /// Parse a format name in any letter case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

impl std::fmt::Display for StructuredFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Symbolic link content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct LinkDocument {
    /// The link target route.
    pub target: Route,
}

impl LinkDocument {
    /// Render target display bytes for hashing.
    pub fn bytes(&self) -> Vec<u8> {
        self.target.display().as_bytes().to_vec()
    }
}

/// Plain text file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct TextDocument {
    /// The exact file text.
    #[face(rename = "text")]
    pub content: String,
    /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
    #[serde(default = "default_file_mode")]
    #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode, default = default_file_mode)]
    pub mode: u32,
    /// Existence-only document flag.
    #[serde(default)]
    #[face(default)]
    pub unmanaged: bool,
}

impl TextDocument {
    /// Render text bytes for hashing.
    pub fn bytes(&self) -> Vec<u8> {
        self.content.as_bytes().to_vec()
    }
}

/// Binary file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct OpaqueDocument {
    /// The opaque file handle.
    pub handle: ResourceHandle,
    /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
    #[serde(default = "default_file_mode")]
    #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode, default = default_file_mode)]
    pub mode: u32,
    /// Existence-only document flag.
    #[serde(default)]
    #[face(default)]
    pub unmanaged: bool,
}

/// Structured file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct StructuredDocument {
    /// The serialization format.
    pub format: StructuredFormat,
    /// Structured content for the file.
    pub data: BTreeMap<String, serde_json::Value>,
}

impl StructuredDocument {
    /// Render canonical bytes for hashing.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] for serializer failures.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        match self.format {
            StructuredFormat::Toml => Ok(render_toml(&self.data)?.into_bytes()),
            StructuredFormat::Json => Ok(render_json(&self.data)?.into_bytes()),
            StructuredFormat::Yaml => Ok(render_yaml(&self.data)?.into_bytes()),
        }
    }
}

/// One tree member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(untagged)]
#[face(table, untagged, rename = "TreeMember")]
pub enum TreeMember {
    /// Bare handle with default mode.
    Handle(ResourceHandle),
    /// Handle with explicit mode.
    Full {
        /// The file payload.
        handle: ResourceHandle,
        /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
        #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode)]
        mode: u32,
    },
}

impl TreeMember {
    /// Return the member payload.
    pub fn handle(&self) -> &ResourceHandle {
        match self {
            Self::Handle(handle) | Self::Full { handle, .. } => handle,
        }
    }

    /// Return the unix permission bits for the member file.
    pub fn mode(&self) -> u32 {
        match self {
            Self::Handle(_) => DEFAULT_FILE_MODE,
            Self::Full { mode, .. } => *mode,
        }
    }
}

/// Managed file set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct TreeDocument {
    /// Tree members keyed by relative path.
    pub members: BTreeMap<String, TreeMember>,
}

/// One document payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(untagged)]
#[face(table, untagged)]
pub enum Data {
    /// Structured data file.
    Structured(StructuredDocument),
    /// Plain text file.
    Text(TextDocument),
    /// Symbolic link.
    Link(LinkDocument),
    /// Per-shell rc data object.
    Rc(RcData),
    /// Raw binary file.
    Opaque(OpaqueDocument),
    /// Managed file set.
    Tree(TreeDocument),
}

/// One declared document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[face(table)]
pub struct Document {
    /// The destination route.
    pub path: Route,
    /// The document payload.
    #[face(flatten)]
    #[serde(flatten)]
    pub data: Data,
}

impl Document {
    /// Build one document from its path with its payload.
    pub fn new(path: Route, data: Data) -> Self {
        Self { path, data }
    }
}

/// Count changed members between two tree manifests.
pub fn tree_changed(
    old: &BTreeMap<String, TreeMember>,
    new: &BTreeMap<String, TreeMember>,
) -> usize {
    let mut changed = 0;
    for (relative, member) in new {
        match old.get(relative) {
            Some(previous) if previous == member => {
                continue;
            }
            _ => changed += 1,
        }
    }
    for relative in old.keys() {
        if !new.contains_key(relative) {
            changed += 1;
        }
    }
    changed
}

/// Render mode bits as octal digits.
pub fn render_mode(_lua: &Lua, mode: &u32) -> mlua::Result<String> {
    Ok(format!("{:o}", mode))
}

/// Return the default unix permission bits for decode.
pub(crate) fn default_file_mode() -> u32 {
    DEFAULT_FILE_MODE
}

/// Decode mode text into permission bits.
///
/// # Errors
///
/// - [`FaceFault`] for values outside octal or symbolic text.
pub(crate) fn decode_mode(lua: &Lua, value: &Value) -> std::result::Result<u32, FaceFault> {
    let text = String::decode(value, lua).map_err(|error| {
        let found = match error.reason {
            confit_binding::fault::FaultReason::ShapeMismatch { found, .. } => found,
            confit_binding::fault::FaultReason::UnknownVariant { found, .. } => found,
        };
        FaceFault {
            segments: Vec::new(),
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "expected a string".to_owned(),
                found,
            },
        }
    })?;
    parse_mode_bits(&text).map_or_else(
        || {
            Err(FaceFault {
                segments: Vec::new(),
                reason: confit_binding::fault::FaultReason::ShapeMismatch {
                    require: "octal like '755' or symbolic like 'rwxr-xr-x'".to_owned(),
                    found: format!("'{text}'"),
                },
            })
        },
        Ok,
    )
}

/// Parse octal or symbolic permission text.
fn parse_mode_bits(text: &str) -> Option<u32> {
    if text.starts_with('d') {
        return None;
    }
    match text.len() {
        9 => {
            let mut bits: u32 = 0;
            for (index, byte) in text.as_bytes().iter().enumerate() {
                let bit: u32 = match (index % 3, byte) {
                    (0, b'r') => 4,
                    (1, b'w') => 2,
                    (2, b'x') => 1,
                    (_, b'-') => 0,
                    _ => return None,
                };
                bits |= bit << ((2 - index / 3) * 3);
            }
            Some(bits)
        }
        3 | 4 => {
            let body = match text.len() {
                3 => text,
                _ => text.strip_prefix('0')?,
            };
            if text.len() == 4 && !text.starts_with('0') {
                return None;
            }
            if !body.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
                return None;
            }
            u32::from_str_radix(body, 8).ok()
        }
        _ => None,
    }
}

/// Render one map to TOML text.
///
/// # Errors
///
/// - [`DocumentError::RenderFailed`] for serializer failures.
fn render_toml(value: &BTreeMap<String, serde_json::Value>) -> Result<String> {
    toml::to_string(value).map_err(|source| {
        ErrorContext::from(DocumentError::RenderFailed {
            reason: source.to_string(),
        })
    })
}

/// Render one map to pretty JSON text.
///
/// # Errors
///
/// - [`DocumentError::RenderFailed`] for serializer failures.
fn render_json(value: &BTreeMap<String, serde_json::Value>) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|source| {
        ErrorContext::from(DocumentError::RenderFailed {
            reason: source.to_string(),
        })
    })
}

/// Render one map to YAML text.
///
/// # Errors
///
/// - [`DocumentError::RenderFailed`] for serializer failures.
fn render_yaml(value: &BTreeMap<String, serde_json::Value>) -> Result<String> {
    noyalib::to_string(value).map_err(|source| {
        ErrorContext::from(DocumentError::RenderFailed {
            reason: source.to_string(),
        })
    })
}
