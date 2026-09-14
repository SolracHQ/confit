//! Model
//!
//! Private declaration plus patch types behind evaluation.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::level::Level;
use confit_core::document::StructuredFormat;

/// Declared structured document from profile plus configs.
#[derive(Debug, Clone)]
pub(crate) struct StructuredDecl {
    /// Destination path.
    pub(crate) path: String,
    /// Serialization format.
    pub(crate) format: StructuredFormat,
    /// Data table in canonical form.
    pub(crate) data: BTreeMap<String, Json>,
}

/// Declared plain text document.
#[derive(Debug, Clone)]
pub(crate) struct TextDecl {
    /// Destination path.
    pub(crate) path: String,
    /// Exact file text.
    pub(crate) content: String,
}

/// Declared symlink document.
#[derive(Debug, Clone)]
pub(crate) struct LinkDecl {
    /// Link path.
    pub(crate) path: String,
    /// Link target.
    pub(crate) target: String,
}

/// Declared rc entry with its section plus canonical JSON form.
#[derive(Debug, Clone)]
pub(crate) struct RcEntryDecl {
    /// Section holding the entry: profile, config, or final.
    pub(crate) section: String,
    /// Canonical entry JSON.
    pub(crate) json: Json,
}

/// Stored patch handle with owner for live execution.
#[derive(Debug, Clone)]
pub(crate) struct StoredPatch {
    /// Target document key: `rc` or one document path.
    pub(crate) target: String,
    /// Structured format for patch-created documents.
    pub(crate) format: Option<StructuredFormat>,
    /// Callback receiving the live wrapper.
    pub(crate) callback: mlua::Function,
    /// Merge priority for ordering.
    pub(crate) priority: Level,
    /// Contributing config name.
    pub(crate) owner: String,
}

/// Accumulated per-config contribution.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigData {
    /// Config name stamping ownership.
    pub(crate) name: String,
    /// Declared structured documents.
    pub(crate) structured: Vec<StructuredDecl>,
    /// Declared text documents.
    pub(crate) texts: Vec<TextDecl>,
    /// Declared link documents.
    pub(crate) links: Vec<LinkDecl>,
    /// Declared rc entries.
    pub(crate) rc: Vec<RcEntryDecl>,
    /// Patch handles in declaration order.
    pub(crate) patches: Vec<StoredPatch>,
}
