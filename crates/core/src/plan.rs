//! Plan
//!
//! Desired state builds with two comparisons.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::document::{Document, DocumentKind};
use crate::error::Result;

/// Plan format version written by every plan build.
///
/// # Examples
///
/// ```text
/// use confit_core::plan::PLAN_VERSION;
///
/// assert!(matches!(PLAN_VERSION, 2));
/// ```
pub const PLAN_VERSION: u32 = 2;

/// Versioned desired state written by plan builds.
///
/// Documents hold path order plus filled data hashes.
/// Created at holds an RFC3339 timestamp outside hash input.
///
/// # Examples
///
/// ```text
/// use confit_core::plan::{PLAN_VERSION, Plan};
///
/// let plan = Plan {
///     version: PLAN_VERSION,
///     documents: Vec::new(),
///     created_at: String::new(),
/// };
/// assert!(matches!(plan.documents.len(), 0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Holds the plan format version.
    pub version: u32,
    /// Holds merged documents in path order.
    pub documents: Vec<Document>,
    /// Holds the RFC3339 creation timestamp.
    pub created_at: String,
}

impl Plan {
    /// Builds an empty plan with the current version.
    ///
    /// # Returns
    ///
    /// The plan holding version plus empty documents plus empty timestamp.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::plan::{PLAN_VERSION, Plan};
    ///
    /// let plan = Plan::empty();
    /// assert!(matches!(plan.version, v if v == PLAN_VERSION));
    /// assert!(matches!(plan.documents.len(), 0));
    /// ```
    pub fn empty() -> Self {
        Self {
            version: PLAN_VERSION,
            documents: Vec::new(),
            created_at: String::new(),
        }
    }

    /// Finds one recorded document by its kind plus path key.
    fn find_by_key(&self, key: &str) -> Option<&Document> {
        self.documents.iter().find(|document| document.key() == key)
    }

    /// Finds one recorded document sharing path with opaque kind.
    fn find_same_path_opaque(&self, document: &Document) -> Option<&Document> {
        self.documents.iter().find(|recorded| {
            recorded.path == document.path
                && recorded.key() != document.key()
                && (recorded.is_opaque() || document.is_opaque())
        })
    }
}

/// Lifecycle counts for one plan against a previous plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Counts documents absent from the previous plan.
    pub create: usize,
    /// Counts documents with a differing data hash.
    pub update: usize,
    /// Counts previous documents missing from the plan.
    pub delete: usize,
}

/// Per-document lifecycle status against previous plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from previous plan.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

impl Document {
    /// Reports the lifecycle status against a previous plan.
    ///
    /// Same-path kind changes to or from opaque read as update.
    /// All other kind changes read as create plus delete through
    /// the build counts. Mode changes read as update while
    /// hashes agree, since hashes cover bytes only.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous plan with filled hashes.
    ///
    /// # Returns
    ///
    /// Create for absent keys, update for differing hashes or
    /// modes plus opaque kind changes, else unchanged.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::{DocumentStatus, Plan};
    ///
    /// let mut document = Document::new(
    ///     DocPath::new("x"),
    ///     DocumentData::Text { content: "hi".into() },
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.status(&Plan::empty()), DocumentStatus::Create));
    /// ```
    pub fn status(&self, previous: &Plan) -> DocumentStatus {
        match previous.find_by_key(&self.key()) {
            None => match previous.find_same_path_opaque(self) {
                Some(_) => DocumentStatus::Update,
                None => DocumentStatus::Create,
            },
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
    /// separately through status and drift.
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
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let mut document = Document::new(
    ///     DocPath::new("x"),
    ///     DocumentData::Text { content: "hi".into() },
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.data_hash.is_empty(), false));
    /// ```
    pub fn fill_hash(&mut self) -> Result<()> {
        let bytes = self.bytes()?;
        self.data_hash = sha256_hex(&bytes);
        Ok(())
    }

    /// Reports whether the document carries opaque bytes.
    ///
    /// # Returns
    ///
    /// True for the opaque kind only.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("bin"),
    ///     DocumentData::Opaque { content: vec![0xFF] },
    /// );
    /// assert!(matches!(document.is_opaque(), true));
    /// ```
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind(), DocumentKind::Opaque)
    }

    /// Reports whether a recorded document yields to desired documents.
    ///
    /// A recorded key yields while some desired document shares
    /// its path under another key with either side opaque.
    ///
    /// # Arguments
    ///
    /// * `desired` - the desired documents under comparing.
    ///
    /// # Returns
    ///
    /// True while an opaque same-path sibling exists in desired.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let recorded = Document::new(
    ///     DocPath::new("bin"),
    ///     DocumentData::Text { content: "hi".into() },
    /// );
    /// let desired = Document::new(
    ///     DocPath::new("bin"),
    ///     DocumentData::Opaque { content: vec![0xFF] },
    /// );
    /// assert!(matches!(recorded.superseded_by(&[desired]), true));
    /// ```
    pub fn superseded_by(&self, desired: &[Document]) -> bool {
        desired.iter().any(|document| {
            document.path == self.path
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Reads the hash plus size label for opaque bytes.
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
/// ```text
/// use confit_core::plan::opaque_label;
///
/// let label = opaque_label(&[0xFF, 0x00]);
/// assert!(matches!(label.starts_with("sha256:"), true));
/// assert!(matches!(label.contains("(2 bytes)"), true));
/// ```
pub fn opaque_label(bytes: &[u8]) -> String {
    format!("sha256:{} ({} bytes)", sha256_hex(bytes), bytes.len())
}

impl Plan {
    /// Builds the desired state plan from documents.
    ///
    /// Renders every document, hashes rendered bytes, and sorts
    /// documents by path. The caller holds one document per path.
    /// The engine enforces this before calling. Counts generate
    /// through `summary` against a previous plan.
    ///
    /// # Arguments
    ///
    /// * `documents` - desired documents in engine pipeline order, unique per path.
    ///
    /// # Returns
    ///
    /// The built plan.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Plan;
    ///
    /// let document = Document::new(
    ///     DocPath::new("note"),
    ///     DocumentData::Text { content: "hi".into() },
    /// );
    /// let outcome = Plan::build(vec![document]);
    /// let previous = Plan::empty();
    /// assert!(matches!(outcome, Ok(plan) if plan.summary(&previous).create == 1));
    /// ```
    pub fn build(mut documents: Vec<Document>) -> Result<Self> {
        for document in &mut documents {
            document.fill_hash()?;
        }
        documents.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self {
            version: PLAN_VERSION,
            documents,
            created_at: now_timestamp(),
        })
    }

    /// Counts lifecycle states against a previous plan.
    ///
    /// Opaque kind changes count as updates, other kind changes
    /// count as create plus delete.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous plan with filled hashes.
    ///
    /// # Returns
    ///
    /// Create, update, plus delete counts.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Plan;
    ///
    /// let mut previous = Plan::empty();
    /// previous.documents = vec![Document::new(
    ///     DocPath::new("note"),
    ///     DocumentData::Text { content: "hi".into() },
    /// )];
    /// let plan = Plan::build(vec![Document::new(
    ///     DocPath::new("note"),
    ///     DocumentData::Text { content: "changed".into() },
    /// )]);
    /// assert!(matches!(plan, Ok(plan) if plan.summary(&previous).update == 1));
    /// ```
    pub fn summary(&self, previous: &Plan) -> Summary {
        let mut summary = Summary {
            create: 0,
            update: 0,
            delete: 0,
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for document in &self.documents {
            seen.insert(document.key());
            match document.status(previous) {
                DocumentStatus::Create => summary.create += 1,
                DocumentStatus::Update => summary.update += 1,
                DocumentStatus::Unchanged => {}
            }
        }
        for recorded in &previous.documents {
            if !seen.contains(&recorded.key()) && !recorded.superseded_by(&self.documents) {
                summary.delete += 1;
            }
        }
        summary
    }
}

/// Computes lowercase hex SHA-256 over bytes.
///
/// # Arguments
///
/// * `bytes` - the input bytes.
///
/// # Returns
///
/// Lowercase hex digest.
///
/// # Examples
///
/// ```text
/// use confit_core::plan::sha256_hex;
///
/// let digest = sha256_hex(b"abc");
/// assert!(matches!(digest.starts_with("ba7816"), true));
/// ```
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reads the current UTC time as an RFC3339 timestamp.
fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::ids::DocPath;

    fn text_doc(path: &str, content: &str) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Text {
                content: content.to_string(),
                mode: None,
            },
        )
    }

    fn with_hashes(documents: Vec<Document>) -> Plan {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = docs;
        previous
    }

    #[test]
    fn plan_counts_create_update_delete() {
        let previous = with_hashes(vec![text_doc("a", "same-a"), text_doc("gone", "gone")]);
        let stale = previous.documents[0].clone();
        let desired = vec![text_doc("a", "same-a"), text_doc("b", "fresh-b")];
        let _ = stale;
        let outcome = Plan::build(desired);
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.create, 1);
        assert_eq!(summary.update, 0);
        assert_eq!(summary.delete, 1);
        assert!(matches!(
            built.documents[0].status(&previous),
            DocumentStatus::Unchanged
        ));
        assert!(matches!(
            built.documents[1].status(&previous),
            DocumentStatus::Create
        ));
    }

    #[test]
    fn plan_marks_update_on_hash_change() {
        let previous = with_hashes(vec![text_doc("b", "old")]);
        let outcome = Plan::build(vec![text_doc("b", "new")]);
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        assert_eq!(built.summary(&previous).update, 1);
        assert!(matches!(
            built.documents[0].status(&previous),
            DocumentStatus::Update
        ));
    }

    fn opaque_doc(path: &str, bytes: &[u8]) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Opaque {
                content: bytes.to_vec(),
                mode: None,
            },
        )
    }

    #[test]
    fn opaque_kind_change_reads_as_update_both_ways() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let desired = opaque_doc("bin", &[0xFF, 0x00]);
        let mut hashed = vec![desired.clone()];
        for document in &mut hashed {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        assert!(matches!(
            hashed[0].status(&previous),
            DocumentStatus::Update
        ));
        let previous_opaque = with_hashes(vec![opaque_doc("bin", &[0xFF, 0x00])]);
        let mut back = vec![text_doc("bin", "hi")];
        for document in &mut back {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        assert!(matches!(
            back[0].status(&previous_opaque),
            DocumentStatus::Update
        ));
    }

    #[test]
    fn opaque_kind_change_skips_superseded_delete() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let mut desired = vec![opaque_doc("bin", &[0xFF, 0x00])];
        for document in &mut desired {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let built = match Plan::build(vec![opaque_doc("bin", &[0xFF, 0x00])]) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.update, 1);
        assert_eq!(summary.delete, 0);
        assert_eq!(summary.create, 0);
    }

    #[test]
    fn plain_kind_change_keeps_create_plus_delete() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let built = match Plan::build(vec![Document::new(
            DocPath::new("bin"),
            DocumentData::Link {
                target: "dest".to_string(),
            },
        )]) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.create, 1);
        assert_eq!(summary.delete, 1);
        assert_eq!(summary.update, 0);
    }
}
