//! Pre-run capture of unmanaged destinations into the rescue state.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;

use confit_core::error::{ContextualCause, Terminal, TerminalResult, kind::ErrorKind};
use confit_core::resource_ref::ResourceRef;
use confit_model::document::{
    Data, Document, FileMode, MemberKey, OpaqueDocument, TreeDocument, TreeMember,
};
use confit_model::routes::Route;
use confit_store::Stores;

use super::prepare::Prepared;
use confit_core::version::VERSION;
use confit_model::manifest::Manifest;

/// The named state that holds the pre-run capture.
pub const RESCUE_NAME: &str = "!";

/// The largest file the capture reads.
const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;

/// The captured documents with their counts.
pub struct Capture {
    /// The manifest that the rescue state stores.
    pub manifest: Manifest,
    /// The files the capture read from disk.
    pub files: usize,
    /// The destinations the capture skipped with their reasons.
    pub skipped: Vec<String>,
}

/// Builds the capture manifest from the destinations on disk.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when a path or content read fails.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a document fails to render.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a document fails to render.
pub fn capture(stores: &Stores, prepared: &Prepared) -> TerminalResult<Capture> {
    let previous = previous_routes(&prepared.previous);
    let previous_members = previous_tree_members(&prepared.previous);
    let mut documents = Vec::new();
    let mut files = 0;
    let mut skipped = Vec::new();
    for document in &prepared.desired.documents {
        let known = previous.contains(&document.path.display());
        match &document.data {
            Data::Tree(found) => {
                let members = previous_members.get(&document.path.display());
                if let Some(tree) = capture_tree(stores, document, &found.members, members)? {
                    files += tree_member_count(&tree);
                    documents.push(tree);
                }
            }
            Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) | Data::Opaque(_) => {
                if known {
                    continue;
                }
                match capture_file(stores, &document.path)? {
                    Captured::Saved(saved) => {
                        files += 1;
                        documents.push(saved);
                    }
                    Captured::Absent => {}
                    Captured::Skipped(reason) => skipped.push(reason),
                }
            }
        }
    }
    let manifest = Manifest {
        version: VERSION,
        documents,
        hooks: Vec::new(),
    };
    stores
        .resources()
        .persist(&manifest.handles())
        .terminal(ErrorKind::StoreState)?;
    Ok(Capture {
        manifest,
        files,
        skipped,
    })
}

/// The outcome of one destination read.
enum Captured {
    /// The document that holds the disk bytes.
    Saved(Document),
    /// The destination is absent from disk.
    Absent,
    /// The destination stays out of the capture with its reason.
    Skipped(String),
}

/// Captures one tree destination as a tree of opaque members.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when a content read fails.
fn capture_tree(
    stores: &Stores,
    document: &Document,
    desired: &BTreeMap<MemberKey, TreeMember>,
    known: Option<&BTreeSet<MemberKey>>,
) -> TerminalResult<Option<Document>> {
    let mut members = BTreeMap::new();
    for relative in desired.keys() {
        if known.is_some_and(|known| known.contains(relative)) {
            continue;
        }
        let member = document.path.join(relative.as_str());
        let expanded = match member.expand() {
            Ok(expanded) => expanded,
            Err(_) => continue,
        };
        match read_file(stores, &expanded)? {
            Read::Saved((handle, mode)) => {
                members.insert(relative.clone(), TreeMember::Full { handle, mode });
            }
            Read::Absent | Read::Skipped => {}
        }
    }
    if members.is_empty() {
        return Ok(None);
    }
    Ok(Some(Document::new(
        document.path.clone(),
        Data::Tree(TreeDocument { members }),
    )))
}

/// Captures one file destination as an opaque document.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when a path or content read fails.
fn capture_file(stores: &Stores, route: &Route) -> TerminalResult<Captured> {
    let expanded = route.expand().terminal(ErrorKind::StoreState)?;
    match read_file(stores, &expanded)? {
        Read::Saved((handle, mode)) => Ok(Captured::Saved(Document::new(
            route.clone(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode,
                unmanaged: false,
            }),
        ))),
        Read::Absent => Ok(Captured::Absent),
        Read::Skipped => Ok(Captured::Skipped(format!(
            "{} exceeds the capture limit",
            route.display()
        ))),
    }
}

/// The outcome of one file read.
enum Read {
    /// The hash and mode that hold the disk bytes.
    Saved((ResourceRef, FileMode)),
    /// The path is absent from disk or hides a folder.
    Absent,
    /// The file stays out of the capture.
    Skipped,
}

/// Reads one file from disk into the cache.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when a content read fails.
fn read_file(stores: &Stores, expanded: &std::path::Path) -> TerminalResult<Read> {
    let metadata = match std::fs::metadata(expanded) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(Read::Absent),
    };
    if !metadata.is_file() {
        return Ok(Read::Absent);
    }
    if metadata.len() > MAX_CAPTURE_BYTES {
        return Ok(Read::Skipped);
    }
    let file = std::fs::File::open(expanded)
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::StoreState)?;
    let handle = stores
        .resources()
        .store(file)
        .terminal(ErrorKind::StoreState)?;
    let mode = FileMode::from(metadata.permissions().mode() & 0o7777);
    Ok(Read::Saved((handle, mode)))
}

/// Collects the routes that the previous manifest records.
fn previous_routes(previous: &Manifest) -> BTreeSet<String> {
    previous
        .documents
        .iter()
        .map(|document| document.path.display())
        .collect()
}

/// Collects the tree members that the previous manifest records by route.
fn previous_tree_members(previous: &Manifest) -> BTreeMap<String, BTreeSet<MemberKey>> {
    let mut out: BTreeMap<String, BTreeSet<MemberKey>> = BTreeMap::new();
    for document in &previous.documents {
        let Data::Tree(found) = &document.data else {
            continue;
        };
        out.insert(
            document.path.display(),
            found.members.keys().cloned().collect(),
        );
    }
    out
}

/// Counts the members of one captured tree document.
fn tree_member_count(document: &Document) -> usize {
    match &document.data {
        Data::Tree(found) => found.members.len(),
        Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) | Data::Opaque(_) => 1,
    }
}
