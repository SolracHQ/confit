//! Persisted plans with state history.

pub mod status;
pub mod stored;

use std::collections::BTreeMap;

use confit_core::error::ContextualResult;
use confit_core::error::ManifestError;
use confit_core::error::{ContextualCause, DocumentError, WithContext, contextual};
use confit_core::sha::Sha;
use confit_core::version;
use confit_model::document::{Data, Document};
use confit_model::hook::Hook;
use confit_store::Stores;
use confit_store::handles::ResourceHandle;

use serde::{Deserialize, Serialize};

/// One persisted manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version triplet.
    pub version: [u8; 3],
    /// The documents that the plan holds, in path order.
    pub documents: Vec<Document>,
    /// The content hashes keyed by the displayed path.
    pub hashes: BTreeMap<String, Sha>,
    /// The hooks in the order that the plan first sees them.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// Returns the live handles in document order.
    pub fn handles(&self) -> Vec<&ResourceHandle> {
        let mut handles = Vec::new();
        for document in &self.documents {
            match &document.data {
                Data::Opaque(found) => handles.push(&found.handle),
                Data::Tree(found) => {
                    for member in found.members.values() {
                        handles.push(member.handle());
                    }
                }
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => {}
            }
        }
        handles
    }

    /// Fills the pool with the bytes of every live handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] when an entry is unreadable, with the attempted path.
    /// - [`confit_core::error::ErrorCause::ContentHashMismatch`] when a stream fails the carried sha.
    pub fn persist(&self, stores: &Stores) -> ContextualResult<()> {
        for handle in self.handles() {
            match handle {
                ResourceHandle::Cache(sha) => {
                    stores.resources().persist(sha)?;
                }
                ResourceHandle::Project(path, sha) => {
                    let file = std::fs::File::open(path)
                        .map_err(ContextualCause::from)
                        .with_fs_path(path)?;
                    stores.pool().store_checked(sha, file)?;
                }
            }
        }
        Ok(())
    }

    /// Serializes one manifest to pretty JSON text.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] when the serializer fails.
    pub fn json(&self) -> ContextualResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| contextual(DocumentError::RenderFailed).with_detail(error.to_string()))
    }

    /// Decodes one manifest after the version check.
    ///
    /// # Errors
    ///
    /// - [`ManifestError::Invalid`] when the payload is bad.
    /// - [`ManifestError::MissingVersion`] when the version is absent.
    /// - [`ManifestError::VersionUnsupported`] when the version is incompatible.
    pub fn decode(bytes: &[u8]) -> ContextualResult<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        let raw = value
            .get("version")
            .cloned()
            .ok_or_else(|| contextual(ManifestError::MissingVersion))?;
        let found: [u8; 3] = serde_json::from_value(raw.clone()).map_err(|_| {
            contextual(ManifestError::VersionUnsupported {
                got: raw.to_string(),
            })
        })?;
        if !version::compatible(found) {
            return Err(contextual(ManifestError::VersionUnsupported {
                got: format!("[{}, {}, {}]", found[0], found[1], found[2]),
            }));
        }
        let manifest: Self = serde_json::from_value(value)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::ErrorCause;
    use confit_core::sha::Sha;
    use confit_core::version::VERSION;
    use confit_model::document::{
        Data, Document, FileMode, OpaqueDocument, TreeDocument, TreeMember,
    };
    use confit_model::routes::{Route, RouteBase};

    use crate::fixtures::{project_handle, test_stores};
    use crate::prepare::build;

    fn opaque_doc(name: &str, blob: ResourceHandle) -> Document {
        Document {
            path: Route::new(RouteBase::Home, name).unwrap(),
            data: Data::Opaque(OpaqueDocument {
                handle: blob,
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        }
    }

    #[test]
    fn manifest_decode_rejects_an_older_version() {
        let raw = serde_json::json!({"version": [0, 8, 0], "documents": [], "hashes": {}});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match Manifest::decode(&bytes) {
            Ok(_) => panic!("expected a version rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(got, "[0, 8, 0]", "the error keeps the version");
                    assert!(
                        error.context().is_empty(),
                        "the decode error holds no location"
                    );
                }
                other => panic!("expected a version rejection: {other}"),
            },
        }
    }

    #[test]
    fn manifest_decode_rejects_a_numeric_version() {
        let raw = serde_json::json!({"version": 9u32, "documents": [], "hashes": {}});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match Manifest::decode(&bytes) {
            Ok(_) => panic!("expected a version rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(got, "9", "the error keeps the raw value");
                }
                other => panic!("expected a version rejection: {other}"),
            },
        }
    }

    #[test]
    fn persist_copies_opaque_and_tree_members_into_the_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let opaque_sha = Sha::hash(b"opaque bytes");
        match stores
            .resources()
            .store_checked(&opaque_sha, std::io::Cursor::new(b"opaque bytes"))
        {
            Ok(_) => {}
            Err(error) => panic!("the cache fails to store: {error}"),
        }
        let member_sha = Sha::hash(b"member bytes");
        match stores
            .resources()
            .store_checked(&member_sha, std::io::Cursor::new(b"member bytes"))
        {
            Ok(_) => {}
            Err(error) => panic!("the cache fails to store: {error}"),
        }
        let mut members = BTreeMap::new();
        members.insert(
            "member.txt".to_owned(),
            TreeMember::Full {
                handle: ResourceHandle::Cache(member_sha.clone()),
                mode: FileMode::new(0o644),
            },
        );
        let manifest = Manifest {
            version: VERSION,
            documents: vec![
                opaque_doc("blob.bin", ResourceHandle::Cache(opaque_sha.clone())),
                Document::new(
                    Route::new(RouteBase::Home, "fonts").unwrap(),
                    Data::Tree(TreeDocument { members }),
                ),
            ],
            hashes: BTreeMap::new(),
            hooks: Vec::new(),
        };
        match manifest.persist(&stores) {
            Ok(()) => {}
            Err(error) => panic!("the manifest fails to persist: {error}"),
        }
        for (sha, body) in [
            (opaque_sha, b"opaque bytes".as_slice()),
            (member_sha, b"member bytes".as_slice()),
        ] {
            match stores.pool().open(&sha) {
                Ok(mut reader) => {
                    let mut found = Vec::new();
                    reader.read_to_end(&mut found).unwrap();
                    assert_eq!(
                        found.as_slice(),
                        body,
                        "the persisted bytes match the source"
                    );
                }
                Err(error) => panic!("the persisted bytes fail to open: {error}"),
            }
        }
    }

    #[test]
    fn persist_lands_live_bytes_in_the_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let body = b"packaged live bytes";
        let handle = project_handle(&stores, dir.path(), "note.txt", body);
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "note").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("the manifest fails to hash: {error}"),
        };
        let sha = Sha::hash(body);
        let live = match manifest.documents.first() {
            Some(document) => match &document.data {
                Data::Opaque(found) => found.handle.clone(),
                _ => panic!("expected an opaque body"),
            },
            None => panic!("expected a document"),
        };
        match &live {
            ResourceHandle::Project(_, found) => {
                assert_eq!(*found, sha, "the build keeps the carried sha")
            }
            ResourceHandle::Cache(_) => panic!("expected a live handle"),
        }
        match manifest.persist(&stores) {
            Ok(()) => {}
            Err(error) => panic!("the manifest fails to persist: {error}"),
        }
        match stores.pool().open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, body, "the persisted bytes match the live file");
            }
            Err(error) => panic!("the persisted bytes fail to open: {error}"),
        }
    }

    #[test]
    fn persist_reports_a_vanished_handle() {
        use confit_core::error::{ErrorCause, FilesystemError};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("gone.txt");
        std::fs::write(&path, b"gone bytes").unwrap();
        let handle = match stores.resources().resource("gone.txt") {
            Ok(found) => found,
            Err(error) => panic!("the project fails to open: {error}"),
        };
        std::fs::remove_file(&path).unwrap();
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "gone").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("the build fails to keep the document: {error}"),
        };
        match manifest.persist(&stores) {
            Ok(()) => panic!("expected a persist rejection"),
            Err(error) => match error.cause() {
                ErrorCause::FilesystemError(FilesystemError::FileMissing) => {
                    match &error.context.fs_path {
                        Some(_) => {}
                        None => panic!("expected the attempted path: {error}"),
                    }
                }
                other => panic!("expected a file error: {other}"),
            },
        }
    }
}
