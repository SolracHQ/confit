//! Recorded manifests against disk readers.

mod compare;
mod disk;
mod ordered;

use std::collections::BTreeMap;

use compare::Comparator;

use crate::manifest::Manifest;
use confit_core::error::{ContextualResult, DiffWarning};
use confit_model::routes::Route;
use confit_store::Stores;

/// A difference between a recorded document and the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// A difference in one leaf value of a structured document.
    Key {
        /// The recorded destination route.
        path: Route,
        /// The leaf key with dots, and `target` for a link.
        key: String,
        /// The recorded leaf value.
        old: Option<serde_json::Value>,
        /// The disk leaf value.
        new: Option<serde_json::Value>,
    },
    /// A difference in a structured map.
    Structured {
        /// The recorded destination route.
        path: Route,
        /// The recorded map.
        old: BTreeMap<String, serde_json::Value>,
        /// The disk map.
        new: BTreeMap<String, serde_json::Value>,
    },
    /// A difference in text lines.
    Diff {
        /// The recorded destination route.
        path: Route,
        /// The unified diff from recorded to disk.
        diff: String,
    },
    /// A recorded destination that is absent on disk.
    Missing {
        /// The recorded destination route.
        path: Route,
    },
    /// A recorded destination that fails to read on disk.
    Unreadable {
        /// The recorded destination route.
        path: Route,
        /// The fault cause for the failed read.
        reason: String,
    },
    /// A directory that blocks the recorded destination on disk.
    Blocked {
        /// The recorded destination route.
        path: Route,
    },
}

/// The order of the old and new sides in a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifferenceOrder {
    /// An order that puts the recorded bytes first.
    RecordedFirst,
    /// An order that puts the disk bytes first.
    DiskFirst,
}

/// A reader of recorded documents against disk.
pub struct Differ {
    /// The stores that serve the reads for one run.
    pub stores: Stores,
}

impl Differ {
    /// Reports the differences between the recorded documents and the disk.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    pub fn differences(
        &self,
        manifest: &Manifest,
        order: DifferenceOrder,
    ) -> ContextualResult<(Vec<Difference>, Vec<DiffWarning>)> {
        Comparator::new(&self.stores, order).run(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use confit_core::sha::Sha;
    use confit_model::document::{
        Data, Document, FileMode, RcData, RcEntry, TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;
    use confit_store::handles::ResourceHandle;

    use crate::manifest::Manifest;
    use crate::render;
    use confit_core::version::VERSION;

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
            version: VERSION,
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
                mode: FileMode::new(0o644),
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
    fn absent_document_reports_an_absent_destination() {
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
    fn directory_destination_reports_a_blocked_difference() {
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
    fn changed_bytes_report_one_text_difference() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("note.txt");
        std::fs::write(&dest, b"disk bytes").unwrap();
        let found = differences_of(dir.path(), vec![text_doc(&dest, "recorded")]);
        assert!(
            matches!(found.as_slice(), [Difference::Diff { .. }]),
            "the changed bytes produce one text difference"
        );
    }

    #[test]
    fn absent_tree_member_reports_an_absent_destination() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tree");
        let member = literal(&dest).join("file.txt");
        let found = differences_of(dir.path(), vec![tree_doc(&dest, "file.txt")]);
        assert_eq!(found, vec![Difference::Missing { path: member }]);
    }

    #[test]
    fn blocked_tree_member_reports_a_blocked_difference() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tree");
        std::fs::create_dir_all(dest.join("file.txt")).unwrap();
        let member = literal(&dest).join("file.txt");
        let found = differences_of(dir.path(), vec![tree_doc(&dest, "file.txt")]);
        assert_eq!(found, vec![Difference::Blocked { path: member }]);
    }

    #[test]
    fn difference_renders_expanded_routes_in_both_bytes() {
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
        let recorded = render::render_document(&document).unwrap();
        let text = String::from_utf8_lossy(&recorded).into_owned();
        assert!(
            text.contains(sourced.to_str().unwrap()),
            "the rendered bytes hold the expanded path"
        );
        assert!(
            !text.contains("literal:"),
            "the rendered bytes hold the plain host path"
        );
        std::fs::write(&dest, &recorded).unwrap();
        let manifest = Manifest {
            version: VERSION,
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
        assert!(found.is_empty(), "the matching bytes produce no difference");
        assert_eq!(
            document.path.display(),
            dest.to_str().unwrap(),
            "the literal route displays the bare host path"
        );
    }
}
