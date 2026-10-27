//! Differ
//! Recorded manifests against disk readers.

mod compare;
mod disk;
mod ordered;

use std::collections::BTreeMap;

use compare::Comparator;

use crate::manifest::Manifest;
use confit_core::error::{DiffWarning, Result};
use confit_model::routes::Route;
use confit_store::Stores;

/// Manual edit at one recorded destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// Leaf difference between recorded and disk.
    Key {
        /// The recorded destination route.
        path: Route,
        /// Dotted leaf key with `target` for links.
        key: String,
        /// The recorded leaf value.
        old: Option<serde_json::Value>,
        /// The disk leaf value.
        new: Option<serde_json::Value>,
    },
    /// Structured difference between recorded and disk.
    Structured {
        /// The recorded destination route.
        path: Route,
        /// The recorded map.
        old: BTreeMap<String, serde_json::Value>,
        /// The disk map.
        new: BTreeMap<String, serde_json::Value>,
    },
    /// Text difference between recorded and disk.
    Diff {
        /// The recorded destination route.
        path: Route,
        /// The unified diff from recorded to disk.
        diff: String,
    },
    /// Absent recorded destination on disk.
    Missing {
        /// The recorded destination route.
        path: Route,
    },
    /// Unreadable recorded destination on disk.
    Unreadable {
        /// The recorded destination route.
        path: Route,
        /// The fault cause for the failed read.
        reason: String,
    },
    /// Directory blocks the recorded destination on disk.
    Blocked {
        /// The recorded destination route.
        path: Route,
    },
}

/// One difference side order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifferenceOrder {
    /// Recorded bytes as old with disk as new.
    RecordedFirst,
    /// Disk bytes as old with recorded as new.
    DiskFirst,
}

/// Recorded against disk difference reader.
pub struct Differ {
    /// Read capabilities for one run.
    pub stores: Stores,
}

impl Differ {
    /// Report manual edits between recorded documents and disk.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    pub fn differences(
        &self,
        manifest: &Manifest,
        order: DifferenceOrder,
    ) -> Result<(Vec<Difference>, Vec<DiffWarning>)> {
        Comparator::new(&self.stores, order).run(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use confit_core::sha::Sha;
    use confit_model::document::{
        Data, Document, RcData, RcEntry, TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;
    use confit_store::handles::ResourceHandle;

    use crate::manifest::MANIFEST_VERSION;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn test_differ(dir: &std::path::Path) -> Differ {
        let (sender, _) = crossbeam_channel::unbounded();
        Differ {
            stores: Stores::new(StoreRoots::under(dir, dir), sender),
        }
    }

    fn manifest_of(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: MANIFEST_VERSION,
            documents,
            hashes: BTreeMap::new(),
            hooks: Vec::new(),
        }
    }

    fn text_doc(path: &std::path::Path, content: &str) -> Document {
        Document {
            path: literal(path),
            data: Data::Text(TextDocument {
                content: content.to_string(),
                mode: 0o644,
                unmanaged: false,
            }),
        }
    }

    fn tree_doc(dir: &std::path::Path, relative: &str) -> Document {
        let mut members = BTreeMap::new();
        members.insert(
            relative.to_string(),
            TreeMember::Handle(ResourceHandle::Cache(Sha::hash(b"member bytes"))),
        );
        Document::new(literal(dir), Data::Tree(TreeDocument { members }))
    }

    fn differences_of(dir: &std::path::Path, documents: Vec<Document>) -> Vec<Difference> {
        test_differ(dir)
            .differences(&manifest_of(documents), DifferenceOrder::RecordedFirst)
            .unwrap()
            .0
    }

    #[test]
    fn absent_doc_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("absent.txt");
        let found = differences_of(dir.path(), vec![text_doc(&dest, "recorded")]);
        assert_eq!(
            found,
            vec![Difference::Missing {
                path: literal(&dest)
            }]
        );
    }

    #[test]
    fn blocked_doc_reports_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("blocked.txt");
        std::fs::create_dir_all(&dest).unwrap();
        let found = differences_of(dir.path(), vec![text_doc(&dest, "recorded")]);
        assert_eq!(
            found,
            vec![Difference::Blocked {
                path: literal(&dest)
            }]
        );
    }

    #[test]
    fn changed_doc_reports_diff() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("note.txt");
        std::fs::write(&dest, b"disk bytes").unwrap();
        let found = differences_of(dir.path(), vec![text_doc(&dest, "recorded")]);
        assert!(
            matches!(found.as_slice(), [Difference::Diff { .. }]),
            "changed bytes render one diff"
        );
    }

    #[test]
    fn tree_member_missing_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tree");
        let member = literal(&dest).join("file.txt");
        let found = differences_of(dir.path(), vec![tree_doc(&dest, "file.txt")]);
        assert_eq!(found, vec![Difference::Missing { path: member }]);
    }

    #[test]
    fn tree_member_blocked_reports_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tree");
        std::fs::create_dir_all(dest.join("file.txt")).unwrap();
        let member = literal(&dest).join("file.txt");
        let found = differences_of(dir.path(), vec![tree_doc(&dest, "file.txt")]);
        assert_eq!(found, vec![Difference::Blocked { path: member }]);
    }

    #[test]
    fn difference_renders_routes_expanded_like_for_like() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("shellrc");
        let sourced = dir.path().join("sourced.sh");
        let document = Document::new(
            literal(&dest),
            Data::Rc(RcData::new(
                vec![RcEntry::Source {
                    path: literal(&sourced),
                    when: None,
                }],
                Vec::new(),
                Vec::new(),
            )),
        );
        let recorded = crate::render::render_document(&document).unwrap();
        let text = String::from_utf8_lossy(&recorded).into_owned();
        assert!(
            text.contains(sourced.to_str().unwrap()),
            "rendered bytes carry the expanded path"
        );
        assert!(
            !text.contains("literal:"),
            "rendered bytes carry no display alias"
        );
        std::fs::write(&dest, &recorded).unwrap();
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![document.clone()],
            hashes: BTreeMap::new(),
            hooks: Vec::new(),
        };
        let (sender, _) = crossbeam_channel::unbounded();
        let differ = Differ {
            stores: Stores::new(StoreRoots::under(dir.path(), dir.path()), sender),
        };
        let found = differ
            .differences(&manifest, DifferenceOrder::RecordedFirst)
            .unwrap()
            .0;
        assert!(
            found.is_empty(),
            "matching expanded bytes are zero difference"
        );
        assert_eq!(
            document.path.display(),
            dest.to_str().unwrap(),
            "literal display renders the bare host path"
        );
    }
}
