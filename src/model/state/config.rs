//! Config
//!
//! Named contribution bags built by `confit.config` for Lua.

use serde::{Deserialize, Serialize};

use super::document::{Document, StructuredFormat};
use super::level::Level;

/// Holds one patch record: target plus owner plus priority.
///
/// Callbacks execute live, nothing records ops. Binding sorts records
/// by priority desc plus owner asc, then runs each callback.
///
/// # Examples
///
/// ```rust
/// use confit::model::state::config::Patch;
/// use confit::model::state::level::Level;
///
/// let patch = Patch { document: "rc".into(), format: None, owner: "bat".into(), priority: Level::Normal };
/// assert_eq!(patch.owner, "bat");
/// assert_eq!(patch.document, "rc");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    /// Holds the target document key.
    pub document: String,
    /// Holds the structured format for patch-created documents.
    pub format: Option<StructuredFormat>,
    /// Holds the contributing config name.
    pub owner: String,
    /// Holds the merge priority for the patch.
    pub priority: Level,
}

/// Accumulated per-config contribution built by `config:add_document`.
///
/// Holds declared documents plus patch records in declaration order.
/// Callbacks execute live, so patches carry target plus owner plus
/// priority only.
///
/// # Examples
///
/// ```rust
/// use confit::model::state::config::ConfigContribution;
///
/// let config = ConfigContribution { name: "bat".into(), ..ConfigContribution::default() };
/// assert_eq!(config.name, "bat");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigContribution {
    /// Config name from `confit.config(name)`, unique per evaluation.
    pub name: String,
    /// Declared documents in declaration order.
    pub documents: Vec<Document>,
    /// Patch records in declaration order.
    pub patches: Vec<Patch>,
}
