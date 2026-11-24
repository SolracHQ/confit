//! Desired state payloads that reach disk.

pub mod rc;
#[cfg(test)]
mod tests;

use std::borrow::Borrow;
use std::collections::BTreeMap;

use crate::routes::Route;
use confit_core::error::{ContextualResult, DocumentError, Require, contextual};
use confit_core::resource_ref::ResourceRef;
use confit_core::sha::Sha;
use lua_binding::Shape;
use lua_binding::error::{DecodeError, DecodeTag};
use lua_binding::value::Hooked;
use mlua::Lua;
use serde::{Deserialize, Serialize};

pub use rc::{RcData, RcEntry};

/// The unix permission bits used when a document carries no mode.
pub const DEFAULT_FILE_MODE: u32 = 0o644;

/// A file permission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(transparent)]
#[shape(proxy = String)]
pub struct FileMode {
    /// The permission bits.
    bits: u32,
}

impl FileMode {
    /// Builds a mode from permission bits.
    pub const fn new(bits: u32) -> Self {
        Self { bits }
    }

    /// Returns the permission bits.
    pub const fn bits(&self) -> u32 {
        self.bits
    }
}

impl Default for FileMode {
    fn default() -> Self {
        Self::new(DEFAULT_FILE_MODE)
    }
}

impl From<u32> for FileMode {
    fn from(bits: u32) -> Self {
        Self::new(bits)
    }
}

impl Hooked<String> for FileMode {
    fn decode(_lua: &Lua, text: String) -> Result<Self, DecodeError> {
        parse_mode_bits(&text)
            .map(Self::new)
            .ok_or_else(|| DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: "octal like '755' or symbolic like 'rwxr-xr-x'".to_owned(),
                    found: format!("'{text}'"),
                },
            })
    }

    fn encode(&self, _lua: &Lua) -> mlua::Result<String> {
        Ok(format!("{:o}", self.bits))
    }
}

/// A serialization format for structured data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Shape)]
#[serde(rename_all = "lowercase")]
#[shape(table, str_alias)]
pub enum StructuredFormat {
    /// The format that writes JSON text.
    Json,
    /// The format that writes TOML text.
    Toml,
    /// The format that writes YAML text.
    Yaml,
}

impl StructuredFormat {
    /// Returns the name that text output uses for the format.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

/// A symbolic link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct LinkDocument {
    /// The link target route.
    pub target: Route,
}

impl LinkDocument {
    /// Returns the target display text as bytes for hashing.
    pub fn bytes(&self) -> Vec<u8> {
        self.target.display().as_bytes().to_vec()
    }
}

/// A plain text file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct TextDocument {
    /// The exact file text.
    #[shape(name = "text")]
    pub content: String,
    /// The file permission mode.
    #[serde(default)]
    #[shape(default)]
    pub mode: FileMode,
}

impl TextDocument {
    /// Returns the file text as bytes for hashing.
    pub fn bytes(&self) -> Vec<u8> {
        self.content.as_bytes().to_vec()
    }
}

/// A binary file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct OpaqueDocument {
    /// The opaque file handle.
    pub handle: ResourceRef,
    /// The file permission mode.
    #[serde(default)]
    #[shape(default)]
    pub mode: FileMode,
}

/// A structured data file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct StructuredDocument {
    /// The serialization format.
    pub format: StructuredFormat,
    /// Structured content for the file.
    pub data: BTreeMap<String, serde_json::Value>,
}

impl StructuredDocument {
    /// Returns the rendered data as bytes for hashing.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] when the serializer fails.
    pub fn bytes(&self) -> ContextualResult<Vec<u8>> {
        match self.format {
            StructuredFormat::Toml => Ok(crate::render::render_toml(&self.data)
                .map_err(contextual)?
                .into_bytes()),
            StructuredFormat::Json => Ok(crate::render::render_json(&self.data)
                .map_err(contextual)?
                .into_bytes()),
            StructuredFormat::Yaml => Ok(crate::render::render_yaml(&self.data)
                .map_err(contextual)?
                .into_bytes()),
        }
    }
}

/// A tree member key with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Shape)]
#[serde(transparent)]
#[shape(proxy = String)]
pub struct MemberKey {
    /// The relative path.
    inner: String,
}

impl MemberKey {
    /// Builds a member key from a relative path.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] when the key is bad.
    pub fn new(text: String) -> ContextualResult<Self> {
        if text.is_empty() || text.starts_with('/') {
            return Err(contextual(DocumentError::ParseFailed {
                require: Require::MemberPath,
            }));
        }
        for segment in text.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
                return Err(contextual(DocumentError::ParseFailed {
                    require: Require::MemberPath,
                }));
            }
        }
        Ok(Self { inner: text })
    }

    /// Returns the key text.
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl Borrow<str> for MemberKey {
    /// Borrows the key text.
    fn borrow(&self) -> &str {
        &self.inner
    }
}

impl Hooked<String> for MemberKey {
    fn decode(_lua: &Lua, text: String) -> Result<Self, DecodeError> {
        Self::new(text.clone()).map_err(|_| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "a relative member path".to_owned(),
                found: format!("'{text}'"),
            },
        })
    }

    fn encode(&self, _lua: &Lua) -> mlua::Result<String> {
        Ok(self.inner.clone())
    }
}

/// One tree member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(untagged)]
#[shape(table, untagged, name = "TreeMember")]
pub enum TreeMember {
    /// A member that carries a handle and the default mode.
    Handle(ResourceRef),
    /// A member that carries a handle and an explicit mode.
    Full {
        /// The file payload.
        handle: ResourceRef,
        /// The file permission mode.
        mode: FileMode,
    },
}

impl TreeMember {
    /// Returns the handle that the member carries.
    pub fn handle(&self) -> &ResourceRef {
        match self {
            Self::Handle(handle) | Self::Full { handle, .. } => handle,
        }
    }

    /// Returns the mode that the member applies.
    pub fn mode(&self) -> FileMode {
        match self {
            Self::Handle(_) => FileMode::default(),
            Self::Full { mode, .. } => *mode,
        }
    }
}

/// A file tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct TreeDocument {
    /// Tree members keyed by relative path.
    pub members: BTreeMap<MemberKey, TreeMember>,
}

/// One document payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(untagged)]
#[shape(table, untagged)]
pub enum Data {
    /// A document with structured data in one format.
    Structured(StructuredDocument),
    /// A document with exact text.
    Text(TextDocument),
    /// A document that points at another route.
    Link(LinkDocument),
    /// A document with shell rc entries in three stages.
    Rc(RcData),
    /// A document with bytes from a handle.
    Opaque(OpaqueDocument),
    /// A document with a set of file members under one route.
    Tree(TreeDocument),
}

/// One configuration artifact, compared as one unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(table)]
pub struct Document {
    /// The destination route.
    pub path: Route,
    /// Whether the document checks only the route existence.
    #[serde(default)]
    #[shape(default)]
    pub unmanaged: bool,
    /// The document payload.
    #[shape(flatten)]
    #[serde(flatten)]
    pub data: Data,
}

impl Document {
    /// Builds one document.
    pub fn new(path: Route, unmanaged: bool, data: Data) -> Self {
        Self {
            path,
            unmanaged,
            data,
        }
    }

    /// Computes the content hash of one document.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    pub fn hash(&self) -> std::result::Result<Sha, DocumentError> {
        match &self.data {
            Data::Opaque(found) => Ok(*found.handle.sha()),
            Data::Tree(found) => {
                let mut out = Vec::new();
                for (relative, member) in &found.members {
                    let line = format!(
                        "{:o} {} {}\n",
                        member.mode().bits(),
                        relative.as_str(),
                        member.handle().sha()
                    );
                    out.extend_from_slice(line.as_bytes());
                }
                Ok(Sha::hash(&out))
            }
            Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => {
                Ok(Sha::hash(&self.render()?))
            }
        }
    }

    /// Encodes the payload as the exact bytes for disk.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::OpaqueNotRenderable`] when the payload is opaque.
    /// - [`confit_core::error::DocumentError::TreeNotRenderable`] when the payload is a tree.
    /// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    pub fn render(&self) -> std::result::Result<Vec<u8>, DocumentError> {
        match &self.data {
            Data::Structured(found) => found.bytes().map_err(|_| DocumentError::RenderFailed),
            Data::Text(found) => Ok(found.bytes()),
            Data::Link(found) => Ok(found.bytes()),
            Data::Rc(data) => crate::render::render_rc(data).map(|text| text.into_bytes()),
            Data::Opaque(_) => Err(DocumentError::OpaqueNotRenderable),
            Data::Tree(_) => Err(DocumentError::TreeNotRenderable),
        }
    }
}

/// Counts the tree members that differ between two member maps.
pub fn tree_changed(
    old: &BTreeMap<MemberKey, TreeMember>,
    new: &BTreeMap<MemberKey, TreeMember>,
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

/// Returns permission bits from octal or symbolic text.
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
