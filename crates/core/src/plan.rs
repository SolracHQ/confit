//! Plan
//!
//! Desired state builds with drift warnings.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::document::Document;
use crate::error::{Error, Result};
use crate::ids::{DocPath, ReadOutcome};
use crate::render::render_document;

/// Plan format version written by every plan build.
///
/// # Examples
///
/// ```rust
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
/// ```rust
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

/// Persisted apply record.
///
/// Keys read `kind:path` with the lowercase kind name.
/// Values hold the last recorded data hash per key.
///
/// # Examples
///
/// ```rust
/// use confit_core::plan::State;
///
/// let state = State::empty();
/// assert!(matches!(state.documents.len(), 0));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Holds last recorded entries by `kind:path` key.
    pub documents: BTreeMap<String, StateEntry>,
}

impl State {
    /// Builds an empty apply record.
    ///
    /// # Returns
    ///
    /// The empty state holding no document entries.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::plan::State;
    ///
    /// let state = State::empty();
    /// assert!(matches!(state.documents.len(), 0));
    /// ```
    pub fn empty() -> Self {
        Self {
            documents: BTreeMap::new(),
        }
    }
}

/// Last recorded hashes for one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    /// Holds the hex SHA-256 over last recorded rendered bytes.
    pub data_hash: String,
}

impl StateEntry {
    /// Builds a state entry from a data hash.
    ///
    /// # Arguments
    ///
    /// * `data_hash` - the hex SHA-256 over rendered bytes.
    ///
    /// # Returns
    ///
    /// The entry for state maps.
    pub fn new(data_hash: impl Into<String>) -> Self {
        Self {
            data_hash: data_hash.into(),
        }
    }
}

/// Lifecycle counts for one plan against previous state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSummary {
    /// Counts documents absent from previous state.
    pub create: usize,
    /// Counts documents with a differing data hash.
    pub update: usize,
    /// Counts previous keys matching zero plan documents.
    pub delete: usize,
}

/// Per-document lifecycle status against previous state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from previous state.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

/// Warning shape carried beside a successful plan.
///
/// # Examples
///
/// ```rust
/// use confit_core::ids::DocPath;
/// use confit_core::plan::{Warning, WarningKind};
///
/// let warning = Warning {
///     path: DocPath::new("x"),
///     kind: WarningKind::ExistsButNoRecord,
/// };
/// assert!(matches!(warning.line().as_str(), "x: exists but no record: will be overwritten"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// Untracked path holding bytes. Creation overwrites it.
    ExistsButNoRecord,
    /// Record matches desired yet disk bytes differ. Manual edits overwrite.
    DiffersFromRecorded,
    /// Failing read. Carries the ready stderr reason.
    Unreadable {
        /// Holds the reason line naming path plus failure.
        reason: String,
    },
}

/// One warning attached to a document path.
///
/// # Examples
///
/// ```rust
/// use confit_core::ids::DocPath;
/// use confit_core::plan::{Warning, WarningKind};
///
/// let warning = Warning {
///     path: DocPath::new("x"),
///     kind: WarningKind::ExistsButNoRecord,
/// };
/// assert!(matches!(warning.line().as_str(), "x: exists but no record: will be overwritten"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Holds the document path the warning belongs to.
    pub path: DocPath,
    /// Holds the warning shape.
    pub kind: WarningKind,
}

impl Warning {
    /// Renders the warning as one stderr line.
    ///
    /// # Returns
    ///
    /// The verbatim warning line.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::{Warning, WarningKind};
    ///
    /// let warning = Warning {
    ///     path: DocPath::new("x"),
    ///     kind: WarningKind::DiffersFromRecorded,
    /// };
    /// assert!(matches!(
    ///     warning.line().as_str(),
    ///     "x: differs from recorded: manual modification will be overwritten"
    /// ));
    /// ```
    pub fn line(&self) -> String {
        match &self.kind {
            WarningKind::ExistsButNoRecord => format!(
                "{}: exists but no record: will be overwritten",
                self.path.as_str()
            ),
            WarningKind::DiffersFromRecorded => format!(
                "{}: differs from recorded: manual modification will be overwritten",
                self.path.as_str()
            ),
            WarningKind::Unreadable { reason } => reason.clone(),
        }
    }
}

/// Reports the lifecycle status of one document.
///
/// # Arguments
///
/// * `document` - the desired document with a filled data hash.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// Create for absent keys, update for differing hashes, else unchanged.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::ids::DocPath;
/// use confit_core::plan::{DocumentStatus, State, document_status};
///
/// let mut document = Document::new(
///     DocPath::new("x"),
///     DocumentData::Text { content: "hi".into() },
/// );
/// document.data_hash = "abc".to_string();
/// assert!(matches!(document_status(&document, &State::empty()), DocumentStatus::Create));
/// ```
pub fn document_status(document: &Document, previous: &State) -> DocumentStatus {
    match previous.documents.get(&document.key()) {
        None => DocumentStatus::Create,
        Some(entry) if entry.data_hash == document.data_hash => DocumentStatus::Unchanged,
        Some(_) => DocumentStatus::Update,
    }
}

/// Built plan with lifecycle counts plus warnings in plan order.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::ids::{DocPath, ReadOutcome};
/// use confit_core::plan::{State, build};
///
/// let document = Document::new(
///     DocPath::new("note"),
///     DocumentData::Text { content: "hi".into() },
/// );
/// let outcome = build(
///     vec![document],
///     &State::empty(),
///     &|_| ReadOutcome::Absent,
/// );
/// assert!(matches!(
///     outcome,
///     Ok(built) if built.summary.create == 1 && built.warnings.is_empty()
/// ));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPlan {
    /// Holds the versioned desired state.
    pub plan: Plan,
    /// Holds lifecycle counts against previous state.
    pub summary: PlanSummary,
    /// Holds warnings in plan order.
    pub warnings: Vec<Warning>,
}

/// Builds the desired state plan from documents.
///
/// Renders every document, hashes rendered bytes, sorts documents by
/// path, diffs hashes against previous state, and snapshots disk
/// paths through the caller provider. Disk warnings ride along with
/// success. One path holds one document; repeats fail as plan errors.
///
/// # Arguments
///
/// * `documents` - desired documents in engine pipeline order.
/// * `previous` - the last apply record.
/// * `snapshot` - the disk reader mapping paths to outcomes.
///
/// # Returns
///
/// The built plan with counts plus warnings.
///
/// # Errors
///
/// Repeated declarations on one path fail as plan errors.
/// Kind mismatches on one path fail as plan errors.
/// Serializer failures fail as plan errors.
pub fn build(
    documents: Vec<Document>,
    previous: &State,
    snapshot: &dyn Fn(&DocPath) -> ReadOutcome,
) -> Result<BuiltPlan> {
    let mut warnings = Vec::new();
    let mut rendered = render_unique(documents)?;
    rendered.sort_by(|left, right| left.0.path.cmp(&right.0.path));
    let mut summary = PlanSummary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (document, bytes) in &rendered {
        seen.insert(document.key());
        match document_status(document, previous) {
            DocumentStatus::Create => summary.create += 1,
            DocumentStatus::Update => summary.update += 1,
            DocumentStatus::Unchanged => {}
        }
        if let Some(warning) = disk_warning(document, bytes, previous, snapshot) {
            warnings.push(warning);
        }
    }
    for key in previous.documents.keys() {
        if !seen.contains(key) {
            summary.delete += 1;
        }
    }
    let planned = rendered.into_iter().map(|(document, _)| document).collect();
    Ok(BuiltPlan {
        plan: Plan {
            version: PLAN_VERSION,
            documents: planned,
            created_at: now_timestamp(),
        },
        summary,
        warnings,
    })
}

/// Renders unique documents with hashes.
///
/// One path holds one document. Repeats fail naming the path.
///
/// # Arguments
///
/// * `documents` - desired documents in engine pipeline order.
///
/// # Returns
///
/// Unique documents with rendered bytes in path group order.
///
/// # Errors
///
/// Repeated declarations on one path fail as plan errors.
/// Kind mismatches on one path fail as plan errors.
/// Serializer failures fail as plan errors.
fn render_unique(documents: Vec<Document>) -> Result<Vec<(Document, Vec<u8>)>> {
    let mut groups: BTreeMap<String, Vec<Document>> = BTreeMap::new();
    for document in documents {
        groups
            .entry(document.path.as_str().to_string())
            .or_default()
            .push(document);
    }
    let mut unique = Vec::new();
    for (path, entries) in groups {
        let Some((first, rest)) = entries.split_first() else {
            continue;
        };
        if !rest.is_empty() {
            return Err(Error::Plan(format!(
                "document '{path}' is declared more than once: declare once, patch to modify"
            )));
        }
        let mut document = first.clone();
        let bytes = render_document(&document)?;
        document.data_hash = sha256_hex(&bytes);
        unique.push((document, bytes));
    }
    Ok(unique)
}

/// Reports one disk warning for a document, if any.
fn disk_warning(
    document: &Document,
    bytes: &[u8],
    previous: &State,
    snapshot: &dyn Fn(&DocPath) -> ReadOutcome,
) -> Option<Warning> {
    let outcome = snapshot(&document.path);
    match previous.documents.get(&document.key()) {
        None => match outcome {
            ReadOutcome::Absent => None,
            ReadOutcome::Present(_) => Some(Warning {
                path: document.path.clone(),
                kind: WarningKind::ExistsButNoRecord,
            }),
            ReadOutcome::Unreadable { reason } => Some(unreadable_warning(&document.path, &reason)),
        },
        Some(entry) if entry.data_hash != document.data_hash => None,
        Some(_) => match outcome {
            ReadOutcome::Absent => None,
            ReadOutcome::Present(disk) if disk.as_slice() == bytes => None,
            ReadOutcome::Present(_) => Some(Warning {
                path: document.path.clone(),
                kind: WarningKind::DiffersFromRecorded,
            }),
            ReadOutcome::Unreadable { reason } => Some(unreadable_warning(&document.path, &reason)),
        },
    }
}

/// Builds an unreadable warning with the verbatim reason line.
fn unreadable_warning(path: &DocPath, detail: &str) -> Warning {
    Warning {
        path: path.clone(),
        kind: WarningKind::Unreadable {
            reason: format!("cannot read '{}': {detail}", path.as_str()),
        },
    }
}

/// Computes lowercase hex SHA-256 over bytes.
fn sha256_hex(bytes: &[u8]) -> String {
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

    fn text_doc(path: &str, content: &str) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Text {
                content: content.to_string(),
            },
        )
    }

    fn hashed(documents: Vec<Document>) -> BTreeMap<String, String> {
        let Ok(built) = build(documents, &State::empty(), &|_| ReadOutcome::Absent) else {
            panic!("hashing build succeeds");
        };
        let plan = built.plan;
        plan.documents
            .iter()
            .map(|document| (document.key(), document.data_hash.clone()))
            .collect()
    }

    #[test]
    fn three_way_diff_counts_create_update_delete() {
        let hashes = hashed(vec![text_doc("a", "same-a"), text_doc("b", "fresh-b")]);
        let hash_of = |key: &str| match hashes.get(key) {
            Some(hash) => hash.clone(),
            None => panic!("hash covers {key}"),
        };
        let mut previous = State::empty();
        previous
            .documents
            .insert("text:a".to_string(), StateEntry::new(hash_of("text:a")));
        previous
            .documents
            .insert("text:b".to_string(), StateEntry::new("stale-b"));
        previous
            .documents
            .insert("text:gone".to_string(), StateEntry::new("gone-hash"));
        let outcome = build(
            vec![text_doc("a", "same-a"), text_doc("b", "fresh-b")],
            &previous,
            &|_| ReadOutcome::Absent,
        );
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary;
        let plan = built.plan;
        assert_eq!(summary.create, 0);
        assert_eq!(summary.update, 1);
        assert_eq!(summary.delete, 1);
        assert!(matches!(
            document_status(&plan.documents[0], &previous),
            DocumentStatus::Unchanged
        ));
        assert!(matches!(
            document_status(&plan.documents[1], &previous),
            DocumentStatus::Update
        ));
    }

    #[test]
    fn snapshot_warnings_ride_with_success() {
        let hashes = hashed(vec![
            text_doc("a", "same"),
            text_doc("b", "new"),
            text_doc("c", "x"),
            text_doc("d", "y"),
            text_doc("e", "z"),
        ]);
        let hash_of = |key: &str| match hashes.get(key) {
            Some(hash) => hash.clone(),
            None => panic!("hash covers {key}"),
        };
        let mut previous = State::empty();
        previous
            .documents
            .insert("text:a".to_string(), StateEntry::new(hash_of("text:a")));
        previous
            .documents
            .insert("text:c".to_string(), StateEntry::new(hash_of("text:c")));
        previous
            .documents
            .insert("text:d".to_string(), StateEntry::new(hash_of("text:d")));
        let documents = vec![
            text_doc("a", "same"),
            text_doc("b", "new"),
            text_doc("c", "x"),
            text_doc("d", "y"),
            text_doc("e", "z"),
        ];
        let outcome = build(
            documents,
            &previous,
            &|path: &DocPath| match path.as_str() {
                "a" => ReadOutcome::Present(b"edited".to_vec()),
                "b" => ReadOutcome::Present(b"new".to_vec()),
                "c" => ReadOutcome::Unreadable {
                    reason: "denied".to_string(),
                },
                "d" => ReadOutcome::Present(b"y".to_vec()),
                _ => ReadOutcome::Absent,
            },
        );
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("plan builds with warnings: {error}"),
        };
        let summary = built.summary;
        let warnings = built.warnings;
        assert_eq!(summary.create, 2);
        let lines: Vec<String> = warnings.iter().map(Warning::line).collect();
        assert_eq!(
            lines,
            vec![
                "a: differs from recorded: manual modification will be overwritten".to_string(),
                "b: exists but no record: will be overwritten".to_string(),
                "cannot read 'c': denied".to_string(),
            ]
        );
    }

    #[test]
    fn repeated_declaration_fails() {
        let outcome = build(
            vec![text_doc("x", "first"), text_doc("x", "second")],
            &State::empty(),
            &|_| ReadOutcome::Absent,
        );
        let error = match outcome {
            Ok(_) => panic!("repeated declaration passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
        assert_eq!(
            error.to_string(),
            "plan error: document 'x' is declared more than once: declare once, patch to modify"
        );
    }

    #[test]
    fn kind_mismatch_is_a_repeated_declaration() {
        let link = Document::new(
            DocPath::new("x"),
            DocumentData::Link {
                target: "dest".into(),
            },
        );
        let outcome = build(vec![text_doc("x", "first"), link], &State::empty(), &|_| {
            ReadOutcome::Absent
        });
        let error = match outcome {
            Ok(_) => panic!("kind mismatch passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
        assert_eq!(
            error.to_string(),
            "plan error: document 'x' is declared more than once: declare once, patch to modify"
        );
    }

    #[test]
    fn plan_sorts_documents_by_path() {
        let outcome = build(
            vec![text_doc("b", "bee"), text_doc("a", "aye")],
            &State::empty(),
            &|_| ReadOutcome::Absent,
        );
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary;
        let plan = built.plan;
        assert_eq!(plan.version, PLAN_VERSION);
        assert!(!plan.created_at.is_empty());
        assert_eq!(summary.create, 2);
        assert_eq!(plan.documents.len(), 2);
        assert_eq!(plan.documents[0].path, DocPath::new("a"));
        assert_eq!(plan.documents[1].path, DocPath::new("b"));
        assert!(!plan.documents[0].data_hash.is_empty());
    }
}
