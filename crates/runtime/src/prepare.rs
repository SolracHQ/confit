//! Prepared manifests with differences and changed routes.

use std::collections::{BTreeMap, BTreeSet};

use confit_binding::fault::FaceFault;
use confit_core::error::{
    DiffWarning, DocumentError, Error, ErrorContext, EvalDetail, Failure, Mixed, Outcome, Require,
};
use confit_core::sha::Sha;
use confit_model::document::{Data, Document, TreeMember};
use confit_model::hook::Hook;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::differ::{Differ, Difference, DifferenceOrder};
use crate::manifest::status::{DocumentStatus, status};
use crate::manifest::stored::load;
use crate::manifest::{MANIFEST_VERSION, Manifest};
use crate::render::render_document;

/// Prepared manifests.
pub struct Prepared {
    /// Built manifest with counts.
    pub desired: Manifest,
    /// Previous manifest for lifecycle marks.
    pub previous: Manifest,
    /// Difference list behind the rendered preview.
    pub differences: Vec<Difference>,
    /// Non-blocking warnings for failed states.
    pub warnings: Vec<DiffWarning>,
    /// First-run flag for absent state.
    pub first_run: bool,
    /// Changed routes behind hook conditions and writes.
    pub changed: BTreeSet<Route>,
}

/// Build the desired manifest from declared documents with a filled hash section.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for failed renders and member key shapes.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for failed renders and member key shapes.
pub fn build(documents: Vec<Document>, hooks: Vec<Hook>) -> Outcome<Manifest> {
    let mut placed: Vec<Document> = documents;
    for document in &placed {
        if let Data::Tree(found) = &document.data {
            for relative in found.members.keys() {
                if check_member_key(relative).is_err() {
                    return Err(Error::evaluate(
                        EvalDetail::Parse {
                            input: relative.clone(),
                            require: Require::MemberPath,
                        },
                        Some(document.path.display()),
                    ));
                }
            }
        }
    }
    placed.sort_by_key(|document| document.path.display());
    let hashes = fill_hashes(&placed)?;
    Ok(Manifest {
        version: MANIFEST_VERSION,
        documents: placed,
        hashes,
        hooks,
    })
}

/// Fold content hashes over hashed documents keyed by path display.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for failed renders and member shapes.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for failed renders and member shapes.
fn fill_hashes(documents: &[Document]) -> Outcome<BTreeMap<String, Sha>> {
    let mut hashes = BTreeMap::new();
    for document in documents {
        let hash = document_hash(document).map_err(|leaf| complete(document, leaf))?;
        hashes.insert(document.path.display(), hash);
    }
    Ok(hashes)
}

/// Map one document hash failure to its terminal shape.
fn complete(document: &Document, leaf: DocumentError) -> Error {
    match leaf {
        DocumentError::ParseFailed { input, require } => Error::evaluate(
            EvalDetail::Parse { input, require },
            Some(document.path.display()),
        ),
        other => Error::render_manifest(ErrorContext::from(other).route(document.path.display())),
    }
}

/// Hash one document over rendered bytes.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] for serializer failures.
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable rc route bases.
fn document_hash(document: &Document) -> std::result::Result<Sha, DocumentError> {
    match &document.data {
        Data::Opaque(found) => Ok(found.handle.sha().clone()),
        Data::Tree(found) => Ok(Sha::hash(&tree_manifest_bytes(&found.members))),
        Data::Rc(_) => render_document(document).map(|bytes| Sha::hash(&bytes)),
        Data::Structured(found) => found
            .bytes()
            .map(|bytes| Sha::hash(&bytes))
            .map_err(|error| match error.root_cause() {
                confit_core::error::ErrorCause::DocumentError(leaf) => leaf.clone(),
                _ => unreachable!("structured bytes fail rendering alone"),
            }),
        Data::Text(found) => Ok(Sha::hash(&found.bytes())),
        Data::Link(found) => Ok(Sha::hash(&found.bytes())),
    }
}

/// Render the canonical manifest bytes for tree hashing.
fn tree_manifest_bytes(members: &BTreeMap<String, TreeMember>) -> Vec<u8> {
    let mut out = Vec::new();
    for (relative, member) in members {
        let sha = member.handle().sha();
        out.extend_from_slice(format!("{:o} {} {}\n", member.mode(), relative, sha).as_bytes());
    }
    out
}

/// Reject tree keys outside relative member paths.
///
/// # Errors
///
/// - [`FaceFault`] for empty, absolute, and dot-dot keys.
fn check_member_key(relative: &str) -> std::result::Result<(), FaceFault> {
    if relative.is_empty() || relative.starts_with('/') {
        return Err(FaceFault {
            segments: Vec::new(),
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a relative member path".to_owned(),
                found: format!("'{relative}'"),
            },
        });
    }
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(FaceFault {
                segments: Vec::new(),
                reason: confit_binding::fault::FaultReason::ShapeMismatch {
                    require: "a relative member path".to_owned(),
                    found: format!("'{relative}'"),
                },
            });
        }
    }
    Ok(())
}

/// Resolve one desired manifest against previous state.
///
/// # Errors
///
/// - [`confit_core::error::Failure::Mid`] for unreadable states.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation terminals from differences.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation terminals from differences.
pub fn prepare(stores: &Stores, desired: Manifest) -> Mixed<Prepared> {
    let previous = load(&stores.states())?;
    log_processed(&desired, &previous);
    let first_run = stores.states().is_first_run();
    let (differences, warnings) = difference_with(stores, first_run, &desired, &previous)?;
    let changed = changed_paths(&desired, &previous, &differences, first_run);
    Ok(Prepared {
        desired,
        previous,
        differences,
        warnings,
        first_run,
        changed,
    })
}

/// Refresh the difference list with first-run reference rules.
///
/// # Errors
///
/// - [`confit_core::error::Failure::Mid`] for unmapped difference faults.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation terminals from differences.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation terminals from differences.
pub fn refresh(stores: &Stores, prepared: &Prepared) -> Mixed<(Vec<Difference>, Vec<DiffWarning>)> {
    let found = difference_with(
        stores,
        stores.states().is_first_run(),
        &prepared.desired,
        &prepared.previous,
    )?;
    Ok(found)
}

/// Diff one built manifest with first-run reference rules.
///
/// # Errors
///
/// - [`confit_core::error::Failure::Mid`] for unmapped difference faults.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation terminals from differences.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation terminals from differences.
fn difference_with(
    stores: &Stores,
    first_run: bool,
    built: &Manifest,
    previous: &Manifest,
) -> Mixed<(Vec<Difference>, Vec<DiffWarning>)> {
    let reference = if first_run { built } else { previous };
    let order = if first_run {
        DifferenceOrder::DiskFirst
    } else {
        DifferenceOrder::RecordedFirst
    };
    let differ = Differ {
        stores: stores.clone(),
    };
    match differ.differences(reference, order) {
        Ok(found) => Ok(found),
        Err(chain) => match Error::evaluate_chain(chain) {
            Ok(terminal) => Err(Failure::Terminal(terminal)),
            Err(mid) => Err(Failure::Mid(mid)),
        },
    }
}

/// Compute the changed destination routes for one apply run.
fn changed_paths(
    built: &Manifest,
    previous: &Manifest,
    differences: &[Difference],
    first_run: bool,
) -> BTreeSet<Route> {
    if first_run {
        return built
            .documents
            .iter()
            .map(|document| document.path.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.documents {
        if !matches!(
            status(
                document,
                &previous.documents,
                &built.hashes,
                &previous.hashes
            ),
            DocumentStatus::Unchanged
        ) {
            out.insert(document.path.clone());
        }
    }
    for document in &built.documents {
        for difference in differences {
            let path = match difference {
                Difference::Key { path, .. }
                | Difference::Structured { path, .. }
                | Difference::Diff { path, .. }
                | Difference::Missing { path }
                | Difference::Blocked { path }
                | Difference::Unreadable { path, .. } => path,
            };
            if path == &document.path || tree_member_of(&document.path, path) {
                out.insert(document.path.clone());
                break;
            }
        }
    }
    out
}

/// Report whether one tree member route nests under its destination.
fn tree_member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative() != destination.relative()
        && path.relative().starts_with(destination.relative())
}

/// Log finished documents with lifecycle status.
fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match status(
            document,
            &previous.documents,
            &built.hashes,
            &previous.hashes,
        ) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.path.display(),
            match &document.data {
                Data::Structured(_) => "structured",
                Data::Text(_) => "text",
                Data::Link(_) => "link",
                Data::Rc(_) => "rc",
                Data::Opaque(_) => "opaque",
                Data::Tree(_) => "tree",
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::Document;
    use confit_model::document::{Data, OpaqueDocument, TextDocument, TreeDocument};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::handles::ResourceHandle;

    use crate::fixtures::{project_handle, test_stores};

    fn literal(relative: &str) -> Route {
        Route::new(RouteBase::Literal, relative).unwrap()
    }

    fn hash_of(data: Data) -> Sha {
        let document = Document {
            path: literal("pinned/subject"),
            data,
        };
        document_hash(&document).unwrap()
    }

    #[test]
    fn hash_pins_text_bytes() {
        let hash = hash_of(Data::Text(TextDocument {
            content: "pinned text\n".into(),
            mode: 0o644,
            unmanaged: false,
        }));
        assert_eq!(
            hash,
            Sha::new("f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a").unwrap(),
            "text hash differs only when rendered bytes change"
        );
    }

    // Hash contracts pin text and tree alone. Other variants reuse model bytes.
    #[test]
    fn hash_covers_tree_manifest_bytes() {
        let first = Sha::new("aa".repeat(32)).unwrap();
        let second = Sha::new("cc".repeat(32)).unwrap();
        let mut members = BTreeMap::new();
        members.insert(
            "b".to_owned(),
            TreeMember::Full {
                handle: ResourceHandle::Cache(second),
                mode: 0o755,
            },
        );
        members.insert(
            "a".to_owned(),
            TreeMember::Full {
                handle: ResourceHandle::Cache(first),
                mode: 0o644,
            },
        );
        let expected = Sha::hash(&tree_manifest_bytes(&members));
        assert_eq!(
            hash_of(Data::Tree(TreeDocument { members })),
            expected,
            "tree hash covers manifest bytes"
        );
    }

    #[test]
    fn build_hashes_live_handles_with_verified_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        let other = project_handle(&stores, dir.path(), "other.txt", b"other bytes");
        let mut members = BTreeMap::new();
        members.insert(
            "top.txt".to_owned(),
            TreeMember::Full {
                handle: note,
                mode: 0o644,
            },
        );
        let documents = vec![
            Document::new(
                Route::new(RouteBase::Home, "fonts").unwrap(),
                Data::Tree(TreeDocument { members }),
            ),
            Document::new(
                Route::new(RouteBase::Home, "blob.bin").unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: other,
                    mode: 0o644,
                    unmanaged: false,
                }),
            ),
        ];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest hashes: {error}"),
        };
        assert_eq!(manifest.documents.len(), 2, "build keeps documents");
        assert!(
            confit_driver::fs::read_dir(&dir.path().join("config").join("blobs"))
                .unwrap_or_default()
                .is_empty(),
            "build writes zero pool files"
        );
        for document in &manifest.documents {
            let payloads: Vec<&ResourceHandle> = match &document.data {
                Data::Opaque(found) => vec![&found.handle],
                Data::Tree(found) => found.members.values().map(TreeMember::handle).collect(),
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => Vec::new(),
            };
            for payload in payloads {
                assert!(
                    matches!(payload, ResourceHandle::Project(..)),
                    "build keeps live payloads"
                );
            }
        }
        match manifest.hashes.get("home('blob.bin')") {
            Some(hash) => assert_eq!(
                *hash,
                Sha::hash(b"other bytes"),
                "opaque hash copies the content sha"
            ),
            None => panic!("hashed opaque builds"),
        }
        match manifest.hashes.get("home('fonts')") {
            Some(hash) => {
                let members = match manifest
                    .documents
                    .iter()
                    .find(|document| document.path.display() == "home('fonts')")
                {
                    Some(document) => match &document.data {
                        Data::Tree(found) => found.members.clone(),
                        _ => panic!("hashed tree keeps its shape"),
                    },
                    None => panic!("hashed tree builds"),
                };
                assert_eq!(
                    *hash,
                    Sha::hash(&tree_manifest_bytes(&members)),
                    "tree hash covers manifest bytes"
                );
            }
            None => panic!("hashed tree builds"),
        }
    }

    #[test]
    fn hash_bare_and_full_members_with_modes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let bare = project_handle(&stores, dir.path(), "note.txt", b"bare bytes");
        let full = project_handle(&stores, dir.path(), "other.txt", b"full bytes");
        let mut members = BTreeMap::new();
        members.insert("top.txt".to_owned(), TreeMember::Handle(bare));
        members.insert(
            "sub/nested.txt".to_owned(),
            TreeMember::Full {
                handle: full,
                mode: 0o755,
            },
        );
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "tree").unwrap(),
            Data::Tree(TreeDocument { members }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest hashes: {error}"),
        };
        let document = match manifest
            .documents
            .iter()
            .find(|document| document.path.display() == "home('tree')")
        {
            Some(found) => found,
            None => panic!("hashed tree builds"),
        };
        match &document.data {
            Data::Tree(found) => {
                let members = &found.members;
                assert_eq!(members.len(), 2, "build keeps both members");
                let bare = match members.get("top.txt") {
                    Some(found) => found,
                    None => panic!("bare member keeps its key"),
                };
                assert!(
                    matches!(bare.handle(), ResourceHandle::Project(..)),
                    "bare member keeps its live bytes"
                );
                assert_eq!(bare.mode(), 0o644, "bare member keeps the default mode");
                let full = match members.get("sub/nested.txt") {
                    Some(found) => found,
                    None => panic!("full member keeps its mode"),
                };
                assert!(
                    matches!(full.handle(), ResourceHandle::Project(..)),
                    "full member keeps its live bytes"
                );
                assert_eq!(full.mode(), 0o755, "full member keeps its mode");
            }
            _ => panic!("hashed tree keeps its shape"),
        }
    }

    #[test]
    fn reject_bad_member_keys_at_hash() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        for key in ["", "/top.txt", "sub//nested.txt", "sub/../nested.txt"] {
            let mut members = BTreeMap::new();
            members.insert(key.to_owned(), TreeMember::Handle(note.clone()));
            let documents = vec![Document::new(
                Route::new(RouteBase::Home, "tree").unwrap(),
                Data::Tree(TreeDocument { members }),
            )];
            match build(documents, Vec::new()) {
                Ok(_) => panic!("bad key hashes: {key}"),
                Err(error) => {
                    let line = error.to_string();
                    assert!(line.contains(key), "bad key keeps its input: {line}");
                    assert!(
                        line.contains("home('tree')"),
                        "bad key keeps its route: {line}"
                    );
                }
            }
        }
    }
}
