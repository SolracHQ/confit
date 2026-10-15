//! Manifest
//! Persisted plans with state history.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use confit_driver::sha::Sha;
use confit_model::document::{Data, DocumentKind, Payload, StructuredFormat, TreeMember};
use confit_model::error::{Error, Result};
use confit_model::hook::Hook;
use confit_model::routes::Route;
use confit_store::Stores;
use confit_store::faults::AccessFault;
use confit_store::handles::BlobRef;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;
use confit_store::state::error::StateError as StoreError;

use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

use crate::bundle::codec::{CodecError, decode};
use crate::error::{Result as RuntimeResult, RuntimeError};

/// Manifest format version for bundle builds.
pub const MANIFEST_VERSION: u32 = 7;

/// Persisted plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version.
    pub version: u32,
    /// Plan documents in path order.
    pub documents: Vec<Document>,
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
    /// The hex SHA-256 over rendered bytes.
    pub data_hash: String,
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
            hooks: Vec::new(),
        }
    }

    /// Build the desired manifest from route documents with filled hashes.
    ///
    /// - Handles seal through the blob pool before hashing.
    /// - Documents sort by destination display.
    ///
    /// # Errors
    ///
    /// - [`Error::Seal`] for documents failing blob compression.
    /// - [`Error::Unhashable`] for documents failing hashes.
    pub fn build(
        stores: &Stores,
        documents: BTreeMap<Route, Data>,
        hooks: Vec<Hook>,
    ) -> Result<Self> {
        let mut placed: Vec<Document> = documents
            .into_iter()
            .map(|(destination, data)| Document::new(destination, data))
            .collect();
        for document in &mut placed {
            seal_document(stores, document)?;
            document.fill_hash().map_err(|error| Error::Unhashable {
                document: document.destination.display(),
                reason: error.to_string(),
            })?;
        }
        placed.sort_by_key(|document| document.destination.display());
        Ok(Self {
            version: MANIFEST_VERSION,
            documents: placed,
            hooks,
        })
    }

    /// List cloned blob refs in document order.
    pub fn refs(&self) -> Vec<BlobRef> {
        let mut refs = Vec::new();
        for document in &self.documents {
            refs.extend(document.data.blob_refs().into_iter().cloned());
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
    /// - [`RuntimeError::StateRead`] for failed state reads.
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

    /// Resolve one picker to its stored manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BadPick`] for absent states and out-of-range
    ///   picks.
    /// - [`RuntimeError::StateRead`] for failed state reads.
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

    /// Gather stored refs for a sweep.
    ///
    /// Every readable stored manifest feeds the keep set.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::StateRead`] for failed folder reads.
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
    /// Build a document with an empty data hash.
    pub fn new(destination: Route, data: Data) -> Self {
        Self {
            destination,
            data,
            data_hash: String::new(),
        }
    }

    /// Return the kind label for this document.
    pub fn kind(&self) -> DocumentKind {
        self.data.kind()
    }

    /// Return the unix permission bits for this document.
    pub fn mode(&self) -> Option<u32> {
        self.data.mode()
    }

    /// Build one kind with display key.
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.destination.display())
    }

    /// Report whether the document carries opaque bytes.
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind(), DocumentKind::Opaque)
    }
}

impl Document {
    /// Report the lifecycle status against previous documents.
    pub fn status(&self, previous: &[Document]) -> DocumentStatus {
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
                if recorded.data_hash == self.data_hash && recorded.mode() == self.mode() =>
            {
                DocumentStatus::Unchanged
            }
            Some(_) => DocumentStatus::Update,
        }
    }

    /// Fill the data hash by rendering the document.
    ///
    /// - The hash covers rendered bytes only.
    /// - Opaque hashes copy the blob ref.
    /// - Tree hashes cover canonical manifest bytes over blob refs.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            Data::Opaque { handle: blob, .. } => {
                self.data_hash = blob.sha256().hex();
                Ok(())
            }
            Data::Tree { members } => {
                self.data_hash = Sha::hash(&tree_manifest_bytes(members)).hex();
                Ok(())
            }
            inline => {
                let bytes = confit_model::render::inline_bytes(inline)?;
                self.data_hash = Sha::hash(&bytes).hex();
                Ok(())
            }
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
pub fn summary(built: &[Document], previous: &[Document]) -> Summary {
    let mut counts = Summary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for document in built {
        seen.insert(document.key());
        match document.status(previous) {
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
        Data::Opaque { handle: blob, .. } => {
            let resolved = blob.resolve(stores).map_err(|error| Error::Seal {
                document: display.clone(),
                reason: error.to_string(),
            })?;
            *blob = Payload::Resolved(resolved);
        }
        Data::Tree { members } => {
            for relative in members.keys() {
                if let Err(error) = check_member_key(relative) {
                    return Err(Error::Seal {
                        document: display.clone(),
                        reason: error.to_string(),
                    });
                }
            }
            for member in members.values_mut() {
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
        Data::Structured { .. } | Data::Text { .. } | Data::Link { .. } | Data::Rc(_) => {}
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

/// Collect blob refs from one readable stored manifest.
fn collect_state_refs(handle: &StateHandle, states: &StateStore, keep: &mut BTreeSet<Sha>) {
    let bytes = match states.read(handle) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let stored: Manifest = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => return,
    };
    if stored.version != MANIFEST_VERSION {
        return;
    }
    keep.extend(stored.refs().iter().map(|blob| blob.stored.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::sha::Sha;
    use confit_model::document::{Data, RcData, RcEntry, StructuredFormat};
    use confit_model::error::Error;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

    fn test_ref(content: &str, stored: &str) -> BlobRef {
        let sha = Sha::new(content).unwrap();
        let pool = Sha::new(stored).unwrap();
        BlobRef::new(sha, pool)
    }

    fn opaque_doc(name: &str, blob: Payload) -> Document {
        Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Opaque {
                handle: blob,
                mode: 0o644,
                unmanaged: false,
            },
        )
    }

    #[test]
    fn refs_preserves_document_order() {
        let first = test_ref(&"11".repeat(32), &"33".repeat(32));
        let second = test_ref(&"22".repeat(32), &"44".repeat(32));
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![
                opaque_doc("a", Payload::Resolved(first.clone())),
                opaque_doc("b", Payload::Resolved(second.clone())),
            ],
            hooks: Vec::new(),
        };
        let refs = manifest.refs();
        assert_eq!(refs, vec![first, second], "refs keep document order");
    }

    fn literal(relative: &str) -> Route {
        Route::new(RouteBase::Literal, relative).unwrap()
    }

    fn fill(data: Data) -> String {
        let mut document = Document::new(literal("pinned/subject"), data);
        document.fill_hash().unwrap();
        document.data_hash
    }

    fn text_doc(name: &str, content: &str) -> Document {
        let mut document = Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Text {
                content: content.into(),
                mode: 0o644,
                unmanaged: false,
            },
        );
        document.fill_hash().unwrap();
        document
    }

    #[test]
    fn summary_counts_create_and_delete() {
        let built = vec![text_doc("fresh", "hi")];
        let previous = vec![text_doc("stale", "hi")];
        let counts = summary(&built, &previous);
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "fresh is create and stale is delete"
        );
    }

    #[test]
    fn fill_hash_pins_text_bytes() {
        let hash = fill(Data::Text {
            content: "pinned text\n".into(),
            mode: 0o644,
            unmanaged: false,
        });
        assert_eq!(
            hash, "f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a",
            "text hash differs only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_structured_bytes() {
        let data = serde_json::json!({"key": "value"});
        let hash = fill(Data::Structured {
            format: StructuredFormat::Json,
            data,
        });
        assert_eq!(
            hash, "796a0bdfc73f373f33ec3098a246b3d27a10d75e9f4f3dd4e4630efc0f2d3184",
            "structured hash differs only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_rc_display_alias_bytes() {
        let data = Data::Rc(RcData::new(
            vec![RcEntry::Env {
                name: "EDITOR".into(),
                value: "hx".into(),
                when: None,
            }],
            vec![RcEntry::Source {
                path: Route::new(RouteBase::Literal, "/pinned/sourced.sh").unwrap(),
                when: None,
            }],
            vec![RcEntry::Alias {
                name: "ll".into(),
                expansion: "ls -l".into(),
                when: None,
            }],
        ));
        let bytes = confit_model::render::inline_bytes(&data).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            text.contains("/pinned/sourced.sh"),
            "rc inline bytes carry the display alias: {text}"
        );
        assert_eq!(
            fill(data),
            "4e0bb0e9341fc39990d9a6f3476e68a3d2c49e75102ccb2daa7ba7bfcf275f97",
            "rc hash differs only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_copies_opaque_blob_sha() {
        let blob = BlobRef::new(
            confit_driver::sha::Sha::new("aa".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let expected = blob.sha256.hex();
        let hash = fill(Data::Opaque {
            handle: Payload::Resolved(blob),
            mode: 0o644,
            unmanaged: false,
        });
        assert_eq!(
            hash, expected,
            "opaque hash copies content sha without pool bytes"
        );
    }

    #[test]
    fn fill_hash_covers_tree_manifest_bytes() {
        let first = BlobRef::new(
            confit_driver::sha::Sha::new("aa".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let second = BlobRef::new(
            confit_driver::sha::Sha::new("cc".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("dd".repeat(32)).unwrap(),
        );
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
        let expected = confit_driver::sha::Sha::hash(&tree_manifest_bytes(&members)).hex();
        assert_eq!(
            fill(Data::Tree { members }),
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
        confit_driver::fs::create_dir_all(&project).unwrap();
        confit_driver::fs::write(&project.join(name), body).unwrap();
        match stores.resources().resource(name) {
            Ok(found) => Payload::Handle(found),
            Err(error) => panic!("project opens: {error}"),
        }
    }

    #[test]
    fn build_seals_handles_with_verified_hashes() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
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
        let documents = [
            (
                Route::new(RouteBase::Home, "fonts").unwrap(),
                Data::Tree { members },
            ),
            (
                Route::new(RouteBase::Home, "blob.bin").unwrap(),
                Data::Opaque {
                    handle: other,
                    mode: 0o644,
                    unmanaged: false,
                },
            ),
        ]
        .into_iter()
        .collect();
        let manifest = match Manifest::build(&stores, documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest seals: {error}"),
        };
        assert_eq!(manifest.documents.len(), 2, "build keeps documents");
        for document in &manifest.documents {
            for payload in document.data.payloads() {
                let resolved = match payload.resolved() {
                    Some(found) => found,
                    None => panic!("build seals every payload"),
                };
                let cache = dir
                    .path()
                    .join("temp")
                    .join("blobs")
                    .join(resolved.stored.hex());
                assert!(
                    confit_driver::fs::exists(&cache),
                    "the blob file lands in cache"
                );
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
                    resolved.sha256,
                    Sha::hash(&seen),
                    "content sha covers member bytes"
                );
            }
        }
        match manifest
            .documents
            .iter()
            .find(|document| document.destination.display() == "home('blob.bin')")
        {
            Some(document) => assert_eq!(
                document.data_hash,
                Sha::hash(b"other bytes").hex(),
                "opaque hash copies the content sha"
            ),
            None => panic!("sealed opaque builds"),
        }
    }

    #[test]
    fn build_passes_sealed_refs_without_io() {
        let dir = tempfile::tempdir().unwrap();
        let guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let blob = BlobRef::new(Sha::hash(b"sealed"), Sha::hash(b"stored"));
        let documents = [(
            Route::new(RouteBase::Home, "bin").unwrap(),
            Data::Opaque {
                handle: Payload::Resolved(blob),
                mode: 0o644,
                unmanaged: false,
            },
        )]
        .into_iter()
        .collect();
        guard.fail_writes(std::io::ErrorKind::StorageFull);
        match Manifest::build(&stores, documents, Vec::new()) {
            Ok(found) => assert_eq!(found.documents.len(), 1, "sealed builds pass through"),
            Err(error) => panic!("sealed builds skip io: {error}"),
        }
    }

    #[test]
    fn seal_bare_and_full_members_with_modes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
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
        let documents = [(
            Route::new(RouteBase::Home, "tree").unwrap(),
            Data::Tree { members },
        )]
        .into_iter()
        .collect();
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
            Data::Tree { members } => {
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
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let note = project_handle(&stores, dir.path(), "note.txt", b"member bytes");
        for key in ["", "/top.txt", "sub//nested.txt", "sub/../nested.txt"] {
            let mut members = BTreeMap::new();
            members.insert(key.to_owned(), TreeMember::Handle(note.clone()));
            let documents = [(
                Route::new(RouteBase::Home, "tree").unwrap(),
                Data::Tree { members },
            )]
            .into_iter()
            .collect();
            match Manifest::build(&stores, documents, Vec::new()) {
                Ok(_) => panic!("bad key seals: {key}"),
                Err(Error::Seal { document, reason }) => {
                    assert!(
                        document.contains("tree"),
                        "seal shows the document: {document}"
                    );
                    assert!(
                        reason.contains("a relative member path"),
                        "the reason shows the fault: {reason}"
                    );
                }
                Err(error) => panic!("wrong seal variant: {error}"),
            }
        }
    }

    #[test]
    fn round_trip_sealed_manifest_with_both_variants() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
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
        let documents = [(
            Route::new(RouteBase::Home, "tree").unwrap(),
            Data::Tree { members },
        )]
        .into_iter()
        .collect();
        let manifest = match Manifest::build(&stores, documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest seals: {error}"),
        };
        let text = match serde_json::to_string(&manifest) {
            Ok(found) => found,
            Err(error) => panic!("sealed manifest serializes: {error}"),
        };
        match serde_json::from_str::<Manifest>(&text) {
            Ok(back) => assert_eq!(back, manifest, "sealed manifest survives serde"),
            Err(error) => panic!("sealed manifest decodes: {error}"),
        }
    }

    #[test]
    fn build_reports_vanished_handles() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = dir.path().join("project");
        confit_driver::fs::create_dir_all(&project).unwrap();
        let path = project.join("gone.txt");
        confit_driver::fs::write(&path, b"gone bytes").unwrap();
        let handle = match stores.resources().resource("gone.txt") {
            Ok(found) => found,
            Err(error) => panic!("project opens: {error}"),
        };
        confit_driver::fs::remove_file(&path).unwrap();
        let documents = [(
            Route::new(RouteBase::Home, "gone").unwrap(),
            Data::Opaque {
                handle: Payload::Handle(handle),
                mode: 0o644,
                unmanaged: false,
            },
        )]
        .into_iter()
        .collect();
        match Manifest::build(&stores, documents, Vec::new()) {
            Ok(_) => panic!("vanished handles seal"),
            Err(Error::Seal { document, reason }) => {
                assert!(
                    document.contains("gone"),
                    "seal shows the document: {document}"
                );
                assert!(!reason.is_empty(), "seal shows the reason");
            }
            Err(error) => panic!("wrong seal variant: {error}"),
        }
    }
}
