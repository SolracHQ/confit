//! Manifest
//! Persisted plans with state history.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use confit_driver::sha::Sha;
use confit_model::document::Document as ModelDocument;
use confit_model::document::{Data, Payload, StructuredFormat, TreeMember};
use confit_model::error::{Error, Result};
use confit_model::hook::Hook;
use confit_model::routes::Route;
use confit_store::Stores;
use confit_store::faults::AccessFault;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;
use confit_store::state::error::StateError as StoreError;

use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

use crate::bundle::codec::{CodecError, decode};
use crate::error::{Result as RuntimeResult, RuntimeError};
use crate::render::render_document;

/// Manifest format version for bundle builds.
pub const MANIFEST_VERSION: u32 = 8;

/// Persisted plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version.
    pub version: u32,
    /// Plan documents in path order.
    pub documents: Vec<Document>,
    /// Content hashes by destination display.
    pub hashes: BTreeMap<String, String>,
    /// Plan hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

/// One persisted document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// The late-bound destination route.
    pub destination: Route,
    /// The persisted payload.
    pub data: Data,
}

/// Per-document lifecycle status against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum DocumentStatus {
    /// Document absent from previous documents.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

/// Lifecycle counts for one bundle against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table)]
pub struct Summary {
    /// Documents absent from previous documents.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub create: usize,
    /// Documents with a differing data hash.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub update: usize,
    /// Previous documents missing from the built documents.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub delete: usize,
}

impl Manifest {
    /// Build an empty manifest at the current version.
    pub fn empty() -> Self {
        Self {
            version: MANIFEST_VERSION,
            documents: Vec::new(),
            hashes: BTreeMap::new(),
            hooks: Vec::new(),
        }
    }

    /// Build the desired manifest from declared documents with a filled hash section.
    ///
    /// - Handles seal through the blob pool before hashing.
    /// - Documents sort by destination display.
    ///
    /// # Errors
    ///
    /// - [`Error::Seal`] for documents failing blob compression.
    /// - [`Error::Unhashable`] for documents failing hashes.
    pub fn build(stores: &Stores, documents: Vec<ModelDocument>, hooks: Vec<Hook>) -> Result<Self> {
        let mut placed: Vec<Document> = documents
            .into_iter()
            .map(|declared| Document::new(declared.path, declared.data))
            .collect();
        for document in &mut placed {
            seal_document(stores, document)?;
        }
        placed.sort_by_key(|document| document.destination.display());
        let hashes = fill_hashes(&placed)?;
        Ok(Self {
            version: MANIFEST_VERSION,
            documents: placed,
            hashes,
            hooks,
        })
    }

    /// List cloned content hashes in document order.
    pub fn refs(&self) -> Vec<Sha> {
        let mut refs = Vec::new();
        for document in &self.documents {
            match &document.data {
                Data::Opaque(document) => {
                    if let Some(blob) = document.handle.resolved() {
                        refs.push(blob.clone());
                    }
                }
                Data::Tree(document) => {
                    for member in document.members.values() {
                        if let Some(blob) = member.blob().resolved() {
                            refs.push(blob.clone());
                        }
                    }
                }
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => {}
            }
        }
        refs
    }

    /// Serialize one manifest as pretty JSON.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
    pub fn json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|source| Error::Render {
            format: StructuredFormat::Json,
            reason: source.to_string(),
        })
    }

    /// Load the applied manifest with missing states as empty.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::StateRead`] for unreadable states.
    /// - [`RuntimeError::StateCorrupt`] for bad payloads.
    /// - [`RuntimeError::StateVersion`] for unsupported versions.
    pub fn load(states: &StateStore) -> RuntimeResult<Self> {
        let handle = match states.applied() {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => return Ok(Self::empty()),
            Err(error) => return Err(from_store(error)),
        };
        match states.read(&handle) {
            Ok(bytes) => decode(&bytes).map_err(|error| from_codec(handle.canonical(), error)),
            Err(StoreError::Read {
                fault: AccessFault::Missing,
                ..
            }) => Ok(Self::empty()),
            Err(error) => Err(from_store(error)),
        }
    }

    /// Write the applied state with history rotation.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Write`] for state write faults.
    /// - [`RuntimeError::StateWriteUnknown`] for clock and other write
    ///   failures.
    /// - [`RuntimeError::StateManifest`] for manifest render failures.
    pub fn store(states: &StateStore, manifest: &Self) -> RuntimeResult<()> {
        let text = manifest
            .json()
            .map_err(|error| RuntimeError::StateManifest { source: error })?;
        let bytes = text.into_bytes();
        let picker = Picker::Applied;
        match &picker {
            Picker::Applied => {}
            Picker::Named(name) => {
                return Err(RuntimeError::BadPick {
                    input: format!("@{name}"),
                });
            }
            Picker::History(pick) => {
                return Err(RuntimeError::BadPick {
                    input: format!("%{pick}"),
                });
            }
        }
        let handle = match states.resolve(&picker) {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => states.create(&picker).map_err(from_store)?,
            Err(error) => return Err(from_store(error)),
        };
        states
            .write(&handle, bytes.as_slice())
            .map_err(from_store)?;
        states.rotate(bytes.as_slice()).map_err(from_store)?;
        Ok(())
    }

    /// Resolve one picker to its sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BadPick`] for absent states and out-of-range
    ///   picks.
    /// - [`RuntimeError::StateRead`] for unreadable states.
    /// - [`RuntimeError::StateCorrupt`] for bad payloads.
    /// - [`RuntimeError::StateVersion`] for unsupported versions.
    pub fn resolve(states: &StateStore, picker: &Picker) -> RuntimeResult<Self> {
        let handle = states.resolve(picker).map_err(from_store)?;
        let bytes = states.read(&handle).map_err(from_store)?;
        let manifest = decode(&bytes).map_err(|error| from_codec(handle.canonical(), error))?;
        Ok(manifest)
    }

    /// Write one named state with its bundle manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BadPick`] for wrong picker kinds and bad names.
    /// - [`RuntimeError::Write`] for state write faults.
    /// - [`RuntimeError::StateWriteUnknown`] for other write failures.
    /// - [`RuntimeError::StateManifest`] for manifest render failures.
    pub fn store_named(states: &StateStore, picker: &Picker, manifest: &Self) -> RuntimeResult<()> {
        let Picker::Named(_) = picker else {
            return Err(RuntimeError::BadPick {
                input: picker_input(picker),
            });
        };
        let text = manifest
            .json()
            .map_err(|error| RuntimeError::StateManifest { source: error })?;
        let handle = match states.resolve(picker) {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => states.create(picker).map_err(from_store)?,
            Err(error) => return Err(from_store(error)),
        };
        states.write(&handle, text.as_bytes()).map_err(from_store)?;
        Ok(())
    }

    /// Gather content refs for a sweep.
    ///
    /// Every readable state manifest feeds the keep set.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::StateRead`] for unreadable folders.
    pub fn keep_set(states: &StateStore) -> RuntimeResult<BTreeSet<Sha>> {
        let mut keep = BTreeSet::new();
        if let Ok(handle) = states.applied() {
            collect_state_refs(&handle, states, &mut keep);
        }
        for handle in states.list().map_err(from_store)? {
            collect_state_refs(&handle, states, &mut keep);
        }
        for handle in states.list_named().map_err(from_store)? {
            collect_state_refs(&handle, states, &mut keep);
        }
        Ok(keep)
    }
}

impl Document {
    /// Build one document with its destination and data.
    pub fn new(destination: Route, data: Data) -> Self {
        Self { destination, data }
    }

    /// Build one kind with display key.
    pub fn key(&self) -> String {
        let kind = match &self.data {
            Data::Structured(_) => "structured",
            Data::Text(_) => "text",
            Data::Link(_) => "link",
            Data::Rc(_) => "rc",
            Data::Opaque(_) => "opaque",
            Data::Tree(_) => "tree",
        };
        format!("{}:{}", kind, self.destination.display())
    }

    /// Report whether the document carries opaque bytes.
    pub fn is_opaque(&self) -> bool {
        matches!(self.data, Data::Opaque(_))
    }
}

impl Document {
    /// Report the lifecycle status against previous documents.
    ///
    /// - Hashes arrive keyed by destination display from both manifests.
    pub fn status(
        &self,
        previous: &[Document],
        hashes: &BTreeMap<String, String>,
        previous_hashes: &BTreeMap<String, String>,
    ) -> DocumentStatus {
        let recorded = previous
            .iter()
            .find(|document| document.key() == self.key());
        match recorded {
            None => {
                let opaque = previous.iter().find(|recorded| {
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
                if hash_for(hashes, self) == hash_for(previous_hashes, recorded)
                    && document_mode(recorded) == document_mode(self) =>
            {
                DocumentStatus::Unchanged
            }
            Some(_) => DocumentStatus::Update,
        }
    }

    /// Report whether a recorded document yields to desired documents.
    ///
    /// The flag is true while an opaque same-destination sibling exists in desired.
    pub fn superseded_by(&self, desired: &[Document]) -> bool {
        desired.iter().any(|document| {
            document.destination == self.destination
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Count lifecycle states against previous documents.
///
/// - Opaque kind changes count as updates, other kind changes count as create
///   and delete.
/// - Hashes arrive keyed by destination display from both manifests.
pub fn summary(
    built: &[Document],
    built_hashes: &BTreeMap<String, String>,
    previous: &[Document],
    previous_hashes: &BTreeMap<String, String>,
) -> Summary {
    let mut counts = Summary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for document in built {
        seen.insert(document.key());
        match document.status(previous, built_hashes, previous_hashes) {
            DocumentStatus::Create => counts.create += 1,
            DocumentStatus::Update => counts.update += 1,
            DocumentStatus::Unchanged => {}
        }
    }
    for recorded in previous {
        if !seen.contains(&recorded.key()) && !recorded.superseded_by(built) {
            counts.delete += 1;
        }
    }
    counts
}

/// Read the unix permission bits for one document.
fn document_mode(document: &Document) -> Option<u32> {
    match &document.data {
        Data::Text(found) => Some(found.mode),
        Data::Opaque(found) => Some(found.mode),
        Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
    }
}

/// Read one content hash by destination display with empty fallback.
fn hash_for<'a>(hashes: &'a BTreeMap<String, String>, document: &Document) -> &'a str {
    hashes
        .get(&document.destination.display())
        .map(String::as_str)
        .unwrap_or_default()
}

/// Fold content hashes over sealed documents keyed by destination display.
///
/// # Errors
///
/// - [`Error::Unhashable`] for documents failing hashes.
fn fill_hashes(documents: &[Document]) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for document in documents {
        let hash = document_hash(document).map_err(|error| Error::Unhashable {
            document: document.destination.display(),
            reason: error.to_string(),
        })?;
        hashes.insert(document.destination.display(), hash);
    }
    Ok(hashes)
}

/// Hash one sealed document over rendered bytes.
///
/// - Opaque hashes copy the blob ref.
/// - Tree hashes cover canonical manifest bytes over blob refs.
/// - Rc hashes cover expanded shell bytes.
///
/// # Errors
///
/// - [`Error::Render`] for serializer failures.
/// - [`Error::Base`] for unresolvable rc route bases.
/// - [`Error::Unhashable`] for rc shell bytes failing renders.
fn document_hash(document: &Document) -> Result<String> {
    match &document.data {
        Data::Opaque(found) => Ok(found.handle.sha256().hex()),
        Data::Tree(found) => Ok(Sha::hash(&tree_manifest_bytes(&found.members)).hex()),
        Data::Rc(_) => {
            let bytes = render_document(document).map_err(|error| match error {
                RuntimeError::Expand { source, .. } => source,
                other => Error::Unhashable {
                    document: document.destination.display(),
                    reason: other.to_string(),
                },
            })?;
            Ok(Sha::hash(&bytes).hex())
        }
        Data::Structured(found) => {
            let bytes = found.bytes()?;
            Ok(Sha::hash(&bytes).hex())
        }
        Data::Text(found) => Ok(Sha::hash(&found.bytes()).hex()),
        Data::Link(found) => Ok(Sha::hash(&found.bytes()).hex()),
    }
}

/// Render the canonical manifest bytes for tree hashing.
fn tree_manifest_bytes(members: &BTreeMap<String, TreeMember>) -> Vec<u8> {
    let mut out = Vec::new();
    for (relative, member) in members {
        out.extend_from_slice(
            format!(
                "{:o} {} {}\n",
                member.mode(),
                relative,
                member.blob().sha256()
            )
            .as_bytes(),
        );
    }
    out
}

/// Seal every payload in one document through the blob pool.
///
/// # Errors
///
/// - [`Error::Seal`] for payloads failing blob compression.
fn seal_document(stores: &Stores, document: &mut Document) -> Result<()> {
    let display = document.destination.display();
    match &mut document.data {
        Data::Opaque(found) => {
            let resolved = found.handle.resolve(stores).map_err(|error| Error::Seal {
                document: display.clone(),
                reason: error.to_string(),
            })?;
            found.handle = Payload::Resolved(resolved);
        }
        Data::Tree(found) => {
            for relative in found.members.keys() {
                if let Err(error) = check_member_key(relative) {
                    return Err(Error::Seal {
                        document: display.clone(),
                        reason: error.to_string(),
                    });
                }
            }
            for member in found.members.values_mut() {
                let payload = match member {
                    TreeMember::Handle(blob) | TreeMember::Full { handle: blob, .. } => blob,
                };
                let resolved = payload.resolve(stores).map_err(|error| Error::Seal {
                    document: display.clone(),
                    reason: error.to_string(),
                })?;
                *payload = Payload::Resolved(resolved);
            }
        }
        Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => {}
    }
    Ok(())
}

/// Reject tree keys outside relative member paths.
///
/// # Errors
///
/// - [`Mismatch`] for empty, absolute, and dot-dot keys.
fn check_member_key(relative: &str) -> std::result::Result<(), Mismatch> {
    if relative.is_empty() || relative.starts_with('/') {
        return Err(Mismatch {
            path: "tree files key".to_owned(),
            want: "a relative member path".to_owned(),
            found: ["'", relative, "'"].concat(),
        });
    }
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(Mismatch {
                path: "tree files key".to_owned(),
                want: "a relative member path".to_owned(),
                found: ["'", relative, "'"].concat(),
            });
        }
    }
    Ok(())
}

/// Decode one count field into its size value.
///
/// # Errors
///
/// - [`Mismatch`] for values outside the count shape.
fn decode_count(lua: &Lua, value: &Value) -> std::result::Result<usize, Mismatch> {
    let found = u64::decode(value, lua)?;
    usize::try_from(found).map_err(|_| Mismatch {
        path: "count".to_owned(),
        want: "a small non-negative integer".to_owned(),
        found: found.to_string(),
    })
}

/// Map one store failure into runtime language.
fn from_store(error: StoreError) -> RuntimeError {
    match error {
        StoreError::Read { path, fault } => RuntimeError::StateRead { path, fault },
        StoreError::Write { path, fault } => RuntimeError::Write { path, fault },
        StoreError::WriteUnknown { path, message } => {
            RuntimeError::StateWriteUnknown { path, message }
        }
        StoreError::BadPick { input } => RuntimeError::BadPick { input },
    }
}

/// Render one picker as its raw input.
fn picker_input(picker: &Picker) -> String {
    match picker {
        Picker::Applied => String::new(),
        Picker::Named(name) => format!("@{name}"),
        Picker::History(pick) => format!("%{pick}"),
    }
}

/// Map one codec failure at the state path into runtime language.
fn from_codec(path: &Path, error: CodecError) -> RuntimeError {
    match error {
        CodecError::Version { got } => RuntimeError::StateVersion {
            path: path.to_path_buf(),
            got,
        },
        CodecError::Corrupt { .. } => RuntimeError::StateCorrupt {
            path: path.to_path_buf(),
        },
    }
}

/// Collect blob refs from one readable state manifest.
fn collect_state_refs(handle: &StateHandle, states: &StateStore, keep: &mut BTreeSet<Sha>) {
    let bytes = match states.read(handle) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let found: Manifest = match serde_json::from_slice(&bytes) {
        Ok(found) => found,
        Err(_) => return,
    };
    if found.version != MANIFEST_VERSION {
        return;
    }
    keep.extend(found.refs());
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::sha::Sha;
    use confit_model::document::Document as DeclaredDocument;
    use confit_model::document::{Data, OpaqueDocument, TextDocument, TreeDocument};
    use confit_model::error::Error;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

    fn test_ref(content: &str) -> Sha {
        Sha::new(content).unwrap()
    }

    fn opaque_doc(name: &str, blob: Payload) -> Document {
        Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Opaque(OpaqueDocument {
                handle: blob,
                mode: 0o644,
                unmanaged: false,
            }),
        )
    }

    #[test]
    fn refs_preserves_document_order() {
        let first = test_ref(&"11".repeat(32));
        let second = test_ref(&"22".repeat(32));
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![
                opaque_doc("a", Payload::Resolved(first.clone())),
                opaque_doc("b", Payload::Resolved(second.clone())),
            ],
            hashes: BTreeMap::new(),
            hooks: Vec::new(),
        };
        let refs = manifest.refs();
        assert_eq!(refs, vec![first, second], "refs keep document order");
    }

    fn literal(relative: &str) -> Route {
        Route::new(RouteBase::Literal, relative).unwrap()
    }

    fn hash_of(data: Data) -> String {
        let document = Document::new(literal("pinned/subject"), data);
        document_hash(&document).unwrap()
    }

    fn text_doc(name: &str, content: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Text(TextDocument {
                content: content.into(),
                mode: 0o644,
                unmanaged: false,
            }),
        )
    }

    #[test]
    fn summary_counts_create_and_delete() {
        let built = vec![text_doc("fresh", "hi")];
        let previous = vec![text_doc("stale", "hi")];
        let built_hashes = fill_hashes(&built).unwrap();
        let previous_hashes = fill_hashes(&previous).unwrap();
        let counts = summary(&built, &built_hashes, &previous, &previous_hashes);
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "fresh is create and stale is delete"
        );
    }

    #[test]
    fn hash_pins_text_bytes() {
        let hash = hash_of(Data::Text(TextDocument {
            content: "pinned text\n".into(),
            mode: 0o644,
            unmanaged: false,
        }));
        assert_eq!(
            hash, "f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a",
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
                handle: Payload::Resolved(second),
                mode: 0o755,
            },
        );
        members.insert(
            "a".to_owned(),
            TreeMember::Full {
                handle: Payload::Resolved(first),
                mode: 0o644,
            },
        );
        let expected = Sha::hash(&tree_manifest_bytes(&members)).hex();
        assert_eq!(
            hash_of(Data::Tree(TreeDocument { members })),
            expected,
            "tree hash covers manifest bytes"
        );
    }

    fn test_stores(dir: &std::path::Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn project_handle(stores: &Stores, dir: &std::path::Path, name: &str, body: &[u8]) -> Payload {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(name), body).unwrap();
        match stores.resources().resource(name) {
            Ok(found) => Payload::Handle(found),
            Err(error) => panic!("project opens: {error}"),
        }
    }

    #[test]
    fn build_seals_handles_with_verified_hashes() {
        use std::io::Read as _;

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
            DeclaredDocument::new(
                Route::new(RouteBase::Home, "fonts").unwrap(),
                Data::Tree(TreeDocument { members }),
            ),
            DeclaredDocument::new(
                Route::new(RouteBase::Home, "blob.bin").unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: other,
                    mode: 0o644,
                    unmanaged: false,
                }),
            ),
        ];
        let manifest = match Manifest::build(&stores, documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest seals: {error}"),
        };
        assert_eq!(manifest.documents.len(), 2, "build keeps documents");
        for document in &manifest.documents {
            let payloads: Vec<&Payload> = match &document.data {
                Data::Opaque(found) => vec![&found.handle],
                Data::Tree(found) => found.members.values().map(TreeMember::blob).collect(),
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => Vec::new(),
            };
            for payload in payloads {
                let resolved = match payload.resolved() {
                    Some(found) => found,
                    None => panic!("build seals every payload"),
                };
                let cache = dir.path().join("temp").join("blobs").join(resolved.hex());
                assert!(cache.exists(), "the blob file lands in cache");
                let handle = match stores.blobs().resolve(resolved) {
                    Ok(found) => found,
                    Err(error) => panic!("sealed refs resolve: {error}"),
                };
                let mut seen = Vec::new();
                match stores.blobs().open(&handle) {
                    Ok(mut reader) => {
                        reader.read_to_end(&mut seen).unwrap();
                    }
                    Err(error) => panic!("sealed bytes open: {error}"),
                }
                assert_eq!(
                    *resolved,
                    Sha::hash(&seen),
                    "content sha covers member bytes"
                );
            }
        }
        match manifest.hashes.get("home('blob.bin')") {
            Some(hash) => assert_eq!(
                *hash,
                Sha::hash(b"other bytes").hex(),
                "opaque hash copies the content sha"
            ),
            None => panic!("sealed opaque builds"),
        }
    }

    #[test]
    fn seal_bare_and_full_members_with_modes() {
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
        let documents = vec![DeclaredDocument::new(
            Route::new(RouteBase::Home, "tree").unwrap(),
            Data::Tree(TreeDocument { members }),
        )];
        let manifest = match Manifest::build(&stores, documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest seals: {error}"),
        };
        let document = match manifest
            .documents
            .iter()
            .find(|document| document.destination.display() == "home('tree')")
        {
            Some(found) => found,
            None => panic!("sealed tree builds"),
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
                    matches!(bare.blob(), Payload::Resolved(_)),
                    "bare member seals its bytes"
                );
                assert_eq!(bare.mode(), 0o644, "bare member keeps the default mode");
                let full = match members.get("sub/nested.txt") {
                    Some(found) => found,
                    None => panic!("full member keeps its key"),
                };
                assert!(
                    matches!(full.blob(), Payload::Resolved(_)),
                    "full member seals its bytes"
                );
                assert_eq!(full.mode(), 0o755, "full member keeps its mode");
            }
            _ => panic!("sealed tree keeps its shape"),
        }
    }

    #[test]
    fn reject_bad_member_keys_at_seal() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        for key in ["", "/top.txt", "sub//nested.txt", "sub/../nested.txt"] {
            let mut members = BTreeMap::new();
            members.insert(key.to_owned(), TreeMember::Handle(note.clone()));
            let documents = vec![DeclaredDocument::new(
                Route::new(RouteBase::Home, "tree").unwrap(),
                Data::Tree(TreeDocument { members }),
            )];
            match Manifest::build(&stores, documents, Vec::new()) {
                Ok(_) => panic!("bad key seals: {key}"),
                Err(Error::Seal { .. }) => {}
                Err(error) => panic!("wrong seal variant: {error}"),
            }
        }
    }

    #[test]
    fn build_reports_vanished_handles() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("gone.txt");
        std::fs::write(&path, b"gone bytes").unwrap();
        let handle = match stores.resources().resource("gone.txt") {
            Ok(found) => found,
            Err(error) => panic!("project opens: {error}"),
        };
        std::fs::remove_file(&path).unwrap();
        let documents = vec![DeclaredDocument::new(
            Route::new(RouteBase::Home, "gone").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle: Payload::Handle(handle),
                mode: 0o644,
                unmanaged: false,
            }),
        )];
        match Manifest::build(&stores, documents, Vec::new()) {
            Ok(_) => panic!("vanished handles seal"),
            Err(Error::Seal { .. }) => {}
            Err(error) => panic!("wrong seal variant: {error}"),
        }
    }
}
