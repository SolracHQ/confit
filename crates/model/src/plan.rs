//! Plan
//!
//! Desired state builds with two comparisons.

use crate::document::{ManifestData, ManifestDocument};
use crate::error::Result;
use crate::handles::Sha;
use crate::manifest::Manifest;

/// Lifecycle counts for one bundle against a previous manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Counts documents absent from the previous manifest.
    pub create: usize,
    /// Counts documents with a differing data hash.
    pub update: usize,
    /// Counts previous documents missing from the manifest.
    pub delete: usize,
}

/// Per-document lifecycle status against previous manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from previous manifest.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

impl ManifestDocument {
    /// Reports the lifecycle status against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous manifest with filled hashes.
    ///
    /// # Returns
    ///
    /// Create for absent keys, update for differing hashes,
    /// differing modes, or opaque kind changes, else unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::handles::{Route, RouteBase};
    /// use confit_model::plan::DocumentStatus;
    /// use confit_model::manifest::Manifest;
    ///
    /// let mut document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// let previous = Manifest { version: 7, documents: Vec::new(), hooks: Vec::new() };
    /// assert!(matches!(document.status(&previous), DocumentStatus::Create));
    /// ```
    pub fn status(&self, previous: &Manifest) -> DocumentStatus {
        let recorded = previous
            .documents
            .iter()
            .find(|document| document.key() == self.key());
        match recorded {
            None => {
                let opaque = previous.documents.iter().find(|recorded| {
                    recorded.destination == self.destination
                        && recorded.key() != self.key()
                        && (recorded.is_opaque() || self.is_opaque())
                });
                match opaque {
                    Some(_) => DocumentStatus::Update,
                    None => DocumentStatus::Create,
                }
            }
            Some(recorded)
                if recorded.data_hash == self.data_hash && recorded.mode() == self.mode() =>
            {
                DocumentStatus::Unchanged
            }
            Some(_) => DocumentStatus::Update,
        }
    }

    /// Fills the data hash by rendering the document.
    ///
    /// The hash covers rendered bytes only. Modes compare
    /// separately through status and drift. Opaque hashes
    /// copy the blob handle, since the handle is the
    /// SHA-256 over raw bytes. Tree hashes cover canonical
    /// manifest bytes over blob handles.
    ///
    /// # Returns
    ///
    /// Unit once the hash fills.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::handles::{Route, RouteBase};
    ///
    /// let mut document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.data_hash.is_empty(), false));
    /// ```
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            ManifestData::Opaque { blob, .. } => {
                self.data_hash = blob.sha().hex();
                Ok(())
            }
            ManifestData::Tree { members } => {
                self.data_hash = Sha::hash(&crate::document::tree_manifest_bytes(members)).hex();
                Ok(())
            }
            inline => {
                let bytes = crate::render::inline_bytes(inline)?;
                self.data_hash = Sha::hash(&bytes).hex();
                Ok(())
            }
        }
    }

    /// Reports whether a recorded document yields to desired documents.
    ///
    /// A recorded key yields while some desired document shares
    /// its destination under another key with either side opaque.
    ///
    /// # Arguments
    ///
    /// * `desired` - the desired documents under comparing.
    ///
    /// # Returns
    ///
    /// True while an opaque same-destination sibling exists in desired.
    ///
    pub fn superseded_by(&self, desired: &[ManifestDocument]) -> bool {
        desired.iter().any(|document| {
            document.destination == self.destination
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Reads the hash and size label for opaque bytes.
///
/// # Arguments
///
/// * `bytes` - the raw bytes under labeling.
///
/// # Returns
///
/// The `sha256:{hex} ({n} bytes)` label.
///
/// # Examples
///
/// ```rust
/// use confit_model::plan::opaque_label;
///
/// let label = opaque_label(&[0xFF, 0x00]);
/// assert!(matches!(label.starts_with("sha256:"), true));
/// assert!(matches!(label.contains("(2 bytes)"), true));
/// ```
pub fn opaque_label(bytes: &[u8]) -> String {
    opaque_id(&Sha::hash(bytes), bytes.len() as u64)
}

/// Labels one opaque payload from its content hash and byte count.
///
/// Refs carry both, so callers label recorded payloads
/// without reading blob bytes.
///
/// # Returns
///
/// The `sha256:{hex} ({n} bytes)` label.
pub fn opaque_id(sha: &Sha, len: u64) -> String {
    format!("sha256:{sha} ({len} bytes)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{RcData, RcEntry, RcOp, StructuredFormat, Table};
    use crate::handles::{Route, RouteBase};

    fn literal(relative: &str) -> Route {
        Route::new(RouteBase::Literal, relative).unwrap()
    }

    fn fill(data: ManifestData) -> String {
        let mut document = ManifestDocument::new(literal("pinned/subject"), data);
        document.fill_hash().unwrap();
        document.data_hash
    }

    #[test]
    fn fill_hash_pins_text_bytes() {
        let hash = fill(ManifestData::Text {
            content: "pinned text\n".into(),
            mode: None,
            unmanaged: false,
        });
        assert_eq!(
            hash, "f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a",
            "text hash drifts only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_structured_bytes() {
        let mut table: Table = Table::new();
        table.insert("key".to_string(), serde_json::Value::String("value".into()));
        let hash = fill(ManifestData::Structured {
            format: StructuredFormat::Json,
            data: table,
        });
        assert_eq!(
            hash, "796a0bdfc73f373f33ec3098a246b3d27a10d75e9f4f3dd4e4630efc0f2d3184",
            "structured hash drifts only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_rc_display_alias_bytes() {
        let data = ManifestData::Rc(RcData::new(
            vec![RcEntry {
                op: RcOp::Env {
                    name: "EDITOR".into(),
                    value: "hx".into(),
                },
                when: None,
            }],
            vec![RcEntry {
                op: RcOp::Source {
                    path: Route::new(RouteBase::Literal, "/pinned/sourced.sh").unwrap(),
                },
                when: None,
            }],
            vec![RcEntry {
                op: RcOp::Alias {
                    name: "ll".into(),
                    expansion: "ls -l".into(),
                },
                when: None,
            }],
        ));
        let bytes = crate::render::inline_bytes(&data).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            text.contains("literal:/pinned/sourced.sh"),
            "rc inline bytes carry the display alias: {text}"
        );
        assert_eq!(
            fill(data),
            "cebb8dd1bf3d0ada60a47b7c2dc670760f11053893ec7f0b324e453f42e5ddaf",
            "rc hash drifts only when rendered bytes change"
        );
    }
}
