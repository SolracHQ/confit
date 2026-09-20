//! Model
//!
//! Private declaration plus patch types behind evaluation.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::level::Level;
use confit_core::document::StructuredFormat;
use confit_core::hook::Hook;

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
    /// Unix permission bits, holding `None` for default handling.
    pub(crate) mode: Option<u32>,
    /// Presence alone satisfies the document while true.
    pub(crate) unmanaged: bool,
}

/// Declared symlink document.
#[derive(Debug, Clone)]
pub(crate) struct LinkDecl {
    /// Link path.
    pub(crate) path: String,
    /// Link target.
    pub(crate) target: String,
}

/// Declared opaque document holding raw bytes.
#[derive(Debug, Clone)]
pub(crate) struct OpaqueDecl {
    /// Destination path.
    pub(crate) path: String,
    /// Raw file bytes.
    pub(crate) content: Vec<u8>,
    /// Unix permission bits, holding `None` for default handling.
    pub(crate) mode: Option<u32>,
    /// True while presence alone satisfies the document.
    pub(crate) unmanaged: bool,
}

/// Declared tree member holding destination slot plus bytes.
#[derive(Debug, Clone)]
pub(crate) struct TreeMemberDecl {
    /// Destination-relative member path.
    pub(crate) rel: String,
    /// Raw member bytes.
    pub(crate) content: Vec<u8>,
    /// Unix permission bits from the archive member.
    pub(crate) mode: u32,
}

/// Declared tree document holding one managed file set.
#[derive(Debug, Clone)]
pub(crate) struct TreeDecl {
    /// Destination folder.
    pub(crate) path: String,
    /// Members in relative path order.
    pub(crate) members: Vec<TreeMemberDecl>,
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
    /// Declaration index across the profile in config order.
    pub(crate) order: usize,
    /// Contributing config name.
    pub(crate) owner: String,
}

/// Declared require edge with its target plus optional hint.
#[derive(Debug, Clone)]
pub(crate) struct RequireDecl {
    /// Required sibling config name.
    pub(crate) target: String,
    /// Hint text rendered on its own line while present.
    pub(crate) hint: Option<String>,
}

/// Accumulated per-config contribution.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigData {
    /// Config name stamping ownership.
    pub(crate) name: String,
    /// Required sibling config names in declaration order.
    pub(crate) requires: Vec<RequireDecl>,
    /// Declared structured documents.
    pub(crate) structured: Vec<StructuredDecl>,
    /// Declared text documents.
    pub(crate) texts: Vec<TextDecl>,
    /// Declared link documents.
    pub(crate) links: Vec<LinkDecl>,
    /// Declared opaque documents.
    pub(crate) opaques: Vec<OpaqueDecl>,
    /// Declared tree documents.
    pub(crate) trees: Vec<TreeDecl>,
    /// Optional rc base holding section buckets.
    pub(crate) rc_base: Option<Vec<RcEntryDecl>>,
    /// Patch handles in declaration order.
    pub(crate) patches: Vec<StoredPatch>,
    /// Declared hooks in declaration order.
    pub(crate) hooks: Vec<Hook>,
}
