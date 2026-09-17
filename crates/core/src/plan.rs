//! Plan
//!
//! Desired state builds with two comparisons.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::document::{Document, DocumentKind};
use crate::error::Result;
use crate::fs::Filesystem;
use crate::hook::{Hook, preview_hook};
use crate::runtime::Runtime;

/// Plan format version written by every plan build.
///
/// # Examples
///
/// ```text
/// use confit_core::plan::PLAN_VERSION;
///
/// assert!(matches!(PLAN_VERSION, 4));
/// ```
pub const PLAN_VERSION: u32 = 4;

/// Versioned desired state written by plan builds.
///
/// Documents hold path order plus filled data hashes.
/// Hooks hold merged post-config steps in first-seen order.
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
///     hooks: Vec::new(),
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
    /// Holds merged hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
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
            hooks: Vec::new(),
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
    /// * `hooks` - desired hooks in declaration order, merged downstream.
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
    /// let outcome = Plan::build(vec![document], Vec::new());
    /// let previous = Plan::empty();
    /// assert!(matches!(outcome, Ok(plan) if plan.summary(&previous).create == 1));
    /// ```
    pub fn build(mut documents: Vec<Document>, hooks: Vec<Hook>) -> Result<Self> {
        for document in &mut documents {
            document.fill_hash()?;
        }
        documents.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self {
            version: PLAN_VERSION,
            documents,
            created_at: now_timestamp(),
            hooks,
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
    /// Create, update, plus delete counts.    ///
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
    /// let plan = Plan::build(
    ///     vec![Document::new(
    ///         DocPath::new("note"),
    ///         DocumentData::Text { content: "changed".into() },
    ///     )],
    ///     Vec::new(),
    /// );
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

    /// Renders one preview line per hook in plan order.
    ///
    /// Resolution runs first: `argv[0]` searches hook path
    /// dirs plus runtime dirs, first hit wins. Runnable hooks
    /// read `! run:` with the resolved binary. Satisfied
    /// checks read `skipped:`. Closed gates read `warn:` with
    /// the gate named. Recorded hooks drift through the same
    /// lines: previous hooks re-evaluate checks each plan, so
    /// a failed check reads as not applied beside drift lines.
    ///
    /// # Arguments
    ///
    /// * `rt` - the runtime facts under reading.
    /// * `fs` - the backend under stating.
    ///
    /// # Returns
    ///
    /// The preview lines in plan order.
    ///
    /// # Errors
    ///
    /// Unresolvable binaries fail as plan errors naming the hook.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::plan::Plan;
    /// use confit_core::fs::MemoryFs;
    /// use confit_core::runtime::Runtime;
    ///
    /// let rt = Runtime { vars: Default::default(), path_dirs: Vec::new() };
    /// let lines = Plan::empty().hook_preview(&rt, &MemoryFs::new());
    /// assert!(matches!(lines, Ok(lines) if lines.is_empty()));
    /// ```
    pub fn hook_preview(&self, rt: &Runtime, fs: &dyn Filesystem) -> Result<Vec<String>> {
        let mut lines = Vec::with_capacity(self.hooks.len());
        for hook in &self.hooks {
            lines.push(preview_hook(hook, rt, fs)?);
        }
        Ok(lines)
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
        let outcome = Plan::build(desired, Vec::new());
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
        let outcome = Plan::build(vec![text_doc("b", "new")], Vec::new());
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
        let built = match Plan::build(vec![opaque_doc("bin", &[0xFF, 0x00])], Vec::new()) {
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
        let built = match Plan::build(
            vec![Document::new(
                DocPath::new("bin"),
                DocumentData::Link {
                    target: "dest".to_string(),
                },
            )],
            Vec::new(),
        ) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.create, 1);
        assert_eq!(summary.delete, 1);
        assert_eq!(summary.update, 0);
    }
    #[test]
    fn build_carries_hooks_through() {
        use crate::hook::Hook;

        let hooks = vec![Hook {
            argv: vec!["mise".to_string()],
            path: Vec::new(),
            when: None,
            checks: Vec::new(),
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }];
        let built = match Plan::build(Vec::new(), hooks) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        assert_eq!(built.version, PLAN_VERSION);
        assert_eq!(built.hooks.len(), 1);
        assert_eq!(built.hooks[0].argv, vec!["mise".to_string()]);
    }

    fn preview_runtime() -> (crate::runtime::Runtime, crate::fs::MemoryFs) {
        use crate::fs::Filesystem;

        let fs = crate::fs::MemoryFs::new();
        let _ = fs.write(std::path::Path::new("/opt/tool"), b"run");
        let _ = fs.set_mode(std::path::Path::new("/opt/tool"), 0o755);
        let _ = fs.write(std::path::Path::new("/opt/probe"), b"run");
        let rt = crate::runtime::Runtime {
            vars: std::collections::BTreeMap::new(),
            path_dirs: vec![std::path::PathBuf::from("/opt")],
        };
        (rt, fs)
    }

    fn preview_hook(argv: &[&str]) -> crate::hook::Hook {
        crate::hook::Hook {
            argv: argv.iter().map(|item| item.to_string()).collect(),
            path: Vec::new(),
            when: None,
            checks: Vec::new(),
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }
    }

    #[test]
    fn hook_preview_renders_run_skip_warn_lines() {
        use crate::document::Condition;

        let (rt, fs) = preview_runtime();
        let mut plan = Plan::empty();
        plan.hooks = vec![
            preview_hook(&["tool", "--flag"]),
            crate::hook::Hook {
                checks: vec![Condition::Exists {
                    path: "/opt/probe".into(),
                }],
                ..preview_hook(&["tool"])
            },
            crate::hook::Hook {
                when: Some(Condition::InPath {
                    name: "absent".into(),
                }),
                ..preview_hook(&["tool"])
            },
        ];
        let lines = match plan.hook_preview(&rt, &fs) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(
            lines,
            vec![
                "! run: /opt/tool --flag".to_string(),
                "skipped: tool (checks pass)".to_string(),
                "warn: tool cannot run (in_path(absent))".to_string(),
            ]
        );
    }

    #[test]
    fn hook_preview_runs_on_failing_checks() {
        use crate::document::Condition;

        let (rt, fs) = preview_runtime();
        let mut plan = Plan::empty();
        plan.hooks = vec![crate::hook::Hook {
            checks: vec![Condition::Exists {
                path: "/opt/absent".into(),
            }],
            ..preview_hook(&["tool"])
        }];
        let lines = match plan.hook_preview(&rt, &fs) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(lines, vec!["! run: /opt/tool".to_string()]);
    }

    #[test]
    fn hook_preview_miss_fails_naming_hook() {
        let (rt, fs) = preview_runtime();
        let mut plan = Plan::empty();
        plan.hooks = vec![preview_hook(&["absent", "install"])];
        match plan.hook_preview(&rt, &fs) {
            Ok(_) => panic!("missing binary passes"),
            Err(error) => assert_eq!(
                error.to_string(),
                "hook 'absent install' cannot resolve 'absent'"
            ),
        }
    }
}
