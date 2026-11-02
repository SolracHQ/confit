//! Prepared manifests with differences and changed routes.

use std::collections::{BTreeMap, BTreeSet};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    ContextualResult, DiffWarning, DocumentError, ErrorCause, Require, Terminal, TerminalResult,
    contextual, error,
};
use confit_core::sha::Sha;
use confit_core::version::VERSION;
use confit_model::document::{Data, Document, TreeMember};
use confit_model::hook::Hook;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::differ::{Differ, Difference, DifferenceOrder};
use crate::manifest::Manifest;
use crate::manifest::status::{self, DocumentStatus};
use crate::manifest::stored;
use crate::render;

/// One resolved manifest.
pub struct Prepared {
    /// The built manifest.
    pub desired: Manifest,
    /// The manifest from the previous run.
    pub previous: Manifest,
    /// The differences that the preview shows.
    pub differences: Vec<Difference>,
    /// The warnings that failed states produce without stopping the run.
    pub warnings: Vec<DiffWarning>,
    /// Whether the run starts with no stored state.
    pub first_run: bool,
    /// The routes that changed, for hook conditions and writes.
    pub changed: BTreeSet<Route>,
}

/// Builds the desired manifest from the declared documents with its hashes.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a render fails or a member key is bad.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a render fails or a member key is bad.
pub fn build(documents: Vec<Document>, hooks: Vec<Hook>) -> TerminalResult<Manifest> {
    let mut placed: Vec<Document> = documents;
    for document in &placed {
        if let Data::Tree(found) = &document.data {
            for relative in found.members.keys() {
                if !is_member_key(relative) {
                    return error(
                        ErrorKind::Evaluate,
                        contextual(DocumentError::ParseFailed {
                            require: Require::MemberPath,
                        })
                        .with_document(document.path.display()),
                    );
                }
            }
        }
    }
    placed.sort_by_key(|document| document.path.display());
    let hashes = fill_hashes(&placed)?;
    Ok(Manifest {
        version: VERSION,
        documents: placed,
        hashes,
        hooks,
    })
}

/// Folds the content hashes over the documents, keyed by displayed path.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a render fails or a member shape is bad.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a render fails or a member shape is bad.
fn fill_hashes(documents: &[Document]) -> TerminalResult<BTreeMap<String, Sha>> {
    let mut hashes = BTreeMap::new();
    for document in documents {
        let hash = match document_hash(document) {
            Ok(hash) => hash,
            Err(contextual) => {
                let contextual = contextual.with_document(document.path.display());
                let kind = match contextual.cause() {
                    ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                        ErrorKind::Evaluate
                    }
                    _ => ErrorKind::RenderManifest,
                };
                return error(kind, contextual);
            }
        };
        hashes.insert(document.path.display(), hash);
    }
    Ok(hashes)
}

/// Hashes the rendered bytes of one document.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
/// - [`confit_core::error::DocumentError::ParseFailed`] when an rc route base is absent.
fn document_hash(document: &Document) -> ContextualResult<Sha> {
    match &document.data {
        Data::Opaque(found) => Ok(*found.handle.sha()),
        Data::Tree(found) => Ok(Sha::hash(&tree_manifest_bytes(&found.members))),
        Data::Rc(_) => Ok(Sha::hash(&render::render_document(document)?)),
        Data::Structured(found) => found.bytes().map(|bytes| Sha::hash(&bytes)),
        Data::Text(found) => Ok(Sha::hash(&found.bytes())),
        Data::Link(found) => Ok(Sha::hash(&found.bytes())),
    }
}

/// Builds the manifest bytes that hash a tree.
fn tree_manifest_bytes(members: &BTreeMap<String, TreeMember>) -> Vec<u8> {
    let mut out = Vec::new();
    for (relative, member) in members {
        let sha = member.handle().sha();
        out.extend_from_slice(
            format!("{:o} {} {}\n", member.mode().bits(), relative, sha).as_bytes(),
        );
    }
    out
}

/// Returns whether a tree key is a relative member path.
fn is_member_key(relative: &str) -> bool {
    if relative.is_empty() || relative.starts_with('/') {
        return false;
    }
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return false;
        }
    }
    true
}

/// Resolves one desired manifest against the stored state.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored state is unreadable.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
pub fn prepare(stores: &Stores, desired: Manifest) -> TerminalResult<Prepared> {
    let previous = stored::load(&stores.states())?;
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

/// Refreshes the differences with the first-run reference rules.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
pub fn refresh(
    stores: &Stores,
    prepared: &Prepared,
) -> TerminalResult<(Vec<Difference>, Vec<DiffWarning>)> {
    let found = difference_with(
        stores,
        stores.states().is_first_run(),
        &prepared.desired,
        &prepared.previous,
    )?;
    Ok(found)
}

/// Diffs one built manifest against the reference under the first-run rules.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
fn difference_with(
    stores: &Stores,
    first_run: bool,
    built: &Manifest,
    previous: &Manifest,
) -> TerminalResult<(Vec<Difference>, Vec<DiffWarning>)> {
    let reference = if first_run { built } else { previous };
    let order = if first_run {
        DifferenceOrder::DiskFirst
    } else {
        DifferenceOrder::RecordedFirst
    };
    let differ = Differ {
        stores: stores.clone(),
    };
    differ
        .differences(reference, order)
        .terminal(ErrorKind::Evaluate)
}

/// Computes the changed routes for one apply run.
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
            status::status(
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

/// Returns whether one tree member route nests under its destination.
fn tree_member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative_path() != destination.relative_path()
        && path
            .relative_path()
            .starts_with(destination.relative_path())
}

/// Logs the documents that finished with their lifecycle status.
fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match status::status(
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
    use confit_core::resource_ref::ResourceRef;
    use confit_model::document::Document;
    use confit_model::document::{Data, FileMode, OpaqueDocument, TextDocument, TreeDocument};
    use confit_model::routes::{Route, RouteBase};

    use crate::fixtures::{project_handle, test_stores};
    use confit_driver::fs;

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
    fn text_hash_pins_the_rendered_bytes() {
        let hash = hash_of(Data::Text(TextDocument {
            content: "pinned text\n".into(),
            mode: FileMode::new(0o644),
            unmanaged: false,
        }));
        assert_eq!(
            hash,
            Sha::new("f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a").unwrap(),
            "the text hash follows the rendered bytes"
        );
    }

    // Hash contracts pin text and tree alone. Other variants reuse model bytes.
    #[test]
    fn tree_hash_covers_the_manifest_bytes() {
        let first = Sha::new("aa".repeat(32)).unwrap();
        let second = Sha::new("cc".repeat(32)).unwrap();
        let mut members = BTreeMap::new();
        members.insert(
            "b".to_owned(),
            TreeMember::Full {
                handle: ResourceRef::new(second),
                mode: FileMode::new(0o755),
            },
        );
        members.insert(
            "a".to_owned(),
            TreeMember::Full {
                handle: ResourceRef::new(first),
                mode: FileMode::new(0o644),
            },
        );
        let expected = Sha::hash(&tree_manifest_bytes(&members));
        assert_eq!(
            hash_of(Data::Tree(TreeDocument { members })),
            expected,
            "the tree hash covers the manifest bytes"
        );
    }

    #[test]
    fn build_hashes_project_handles_with_their_content_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        let other = project_handle(&stores, dir.path(), "other.txt", b"other bytes");
        let mut members = BTreeMap::new();
        members.insert(
            "top.txt".to_owned(),
            TreeMember::Full {
                handle: note,
                mode: FileMode::new(0o644),
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
                    mode: FileMode::new(0o644),
                    unmanaged: false,
                }),
            ),
        ];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("the manifest fails to hash: {error}"),
        };
        assert_eq!(
            manifest.documents.len(),
            2,
            "the build keeps both documents"
        );
        assert!(
            fs::read_dir(&dir.path().join("config").join("blobs"))
                .unwrap_or_default()
                .is_empty(),
            "the build leaves the pool empty"
        );
        for document in &manifest.documents {
            let payloads: Vec<&ResourceRef> = match &document.data {
                Data::Opaque(found) => vec![&found.handle],
                Data::Tree(found) => found.members.values().map(TreeMember::handle).collect(),
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => Vec::new(),
            };
            for payload in payloads {
                assert!(
                    *payload == note || *payload == other,
                    "the build keeps the declared hashes"
                );
            }
        }
        match manifest.hashes.get("home('blob.bin')") {
            Some(hash) => assert_eq!(
                *hash,
                Sha::hash(b"other bytes"),
                "the opaque hash copies the content sha"
            ),
            None => panic!("expected an opaque hash"),
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
                        _ => panic!("expected a tree body"),
                    },
                    None => panic!("expected a tree hash"),
                };
                assert_eq!(
                    *hash,
                    Sha::hash(&tree_manifest_bytes(&members)),
                    "the tree hash covers the manifest bytes"
                );
            }
            None => panic!("expected a tree hash"),
        }
    }

    #[test]
    fn tree_hash_covers_bare_and_full_members_with_their_modes() {
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
                mode: FileMode::new(0o755),
            },
        );
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "tree").unwrap(),
            Data::Tree(TreeDocument { members }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("the manifest fails to hash: {error}"),
        };
        let document = match manifest
            .documents
            .iter()
            .find(|document| document.path.display() == "home('tree')")
        {
            Some(found) => found,
            None => panic!("expected a tree document"),
        };
        match &document.data {
            Data::Tree(found) => {
                let members = &found.members;
                assert_eq!(members.len(), 2, "the build keeps both members");
                let bare_member = match members.get("top.txt") {
                    Some(found) => found,
                    None => panic!("expected the top.txt member"),
                };
                assert_eq!(
                    bare_member.handle(),
                    &bare,
                    "the bare member keeps its bytes"
                );
                assert_eq!(
                    bare_member.mode().bits(),
                    0o644,
                    "the bare member takes the default mode"
                );
                let full_member = match members.get("sub/nested.txt") {
                    Some(found) => found,
                    None => panic!("expected the sub/nested.txt member"),
                };
                assert_eq!(
                    full_member.handle(),
                    &full,
                    "the full member keeps its bytes"
                );
                assert_eq!(
                    full_member.mode().bits(),
                    0o755,
                    "the full member keeps its mode"
                );
            }
            _ => panic!("expected a tree body"),
        }
    }

    #[test]
    fn build_rejects_an_invalid_member_key() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        for key in ["", "/top.txt", "sub//nested.txt", "sub/../nested.txt"] {
            let mut members = BTreeMap::new();
            members.insert(key.to_owned(), TreeMember::Handle(note));
            let documents = vec![Document::new(
                Route::new(RouteBase::Home, "tree").unwrap(),
                Data::Tree(TreeDocument { members }),
            )];
            match build(documents, Vec::new()) {
                Ok(_) => panic!("expected a key rejection: {key}"),
                Err(error) => {
                    let line = error.to_string();
                    assert!(
                        line.contains("home('tree')"),
                        "the rejection keeps the route: {line}"
                    );
                    assert!(
                        matches!(
                            error.cause(),
                            ErrorCause::DocumentError(DocumentError::ParseFailed { .. })
                        ),
                        "the rejection keeps the member path require: {line}"
                    );
                }
            }
        }
    }
}
