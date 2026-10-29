//! Manifest
//! Persisted plans with state history.

pub mod status;
pub mod stored;

use std::collections::BTreeMap;

use confit_core::error::ContextualResult;
use confit_core::error::ManifestError;
use confit_core::error::{ContextualCause, DocumentError, WithContext, contextual};
use confit_core::sha::Sha;
use confit_model::document::{Data, Document};
use confit_model::hook::Hook;
use confit_store::Stores;
use confit_store::handles::ResourceHandle;

use serde::{Deserialize, Serialize};

/// Manifest format version for bundle builds.
pub const MANIFEST_VERSION: u32 = 9;

/// Persisted plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version.
    pub version: u32,
    /// Plan documents in path order.
    pub documents: Vec<Document>,
    /// Content hashes by path display.
    pub hashes: BTreeMap<String, Sha>,
    /// Plan hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// List live handles in document order.
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

    /// Fill the pool from live and cached bytes.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable entries with the attempted path.
    /// - [`confit_core::error::ErrorCause::ContentHashMismatch`] for streams failing the carried sha.
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

    /// Serialize one manifest as pretty JSON.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] for serializer failures.
    pub fn json(&self) -> ContextualResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| contextual(DocumentError::RenderFailed).with_detail(error.to_string()))
    }

    /// Decode one manifest with the version check first.
    ///
    /// # Errors
    ///
    /// - [`ManifestError::Invalid`] for bad payloads.
    /// - [`ManifestError::MissingVersion`] for absent versions.
    /// - [`ManifestError::VersionUnsupported`] for stale versions.
    pub fn decode(bytes: &[u8]) -> ContextualResult<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        if value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .is_none()
        {
            return Err(contextual(ManifestError::MissingVersion));
        }
        let manifest: Self = serde_json::from_value(value)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(contextual(ManifestError::VersionUnsupported {
                got: u64::from(manifest.version),
            }));
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::ErrorCause;
    use confit_core::sha::Sha;
    use confit_model::document::{Data, Document, OpaqueDocument, TreeDocument, TreeMember};
    use confit_model::routes::{Route, RouteBase};

    use crate::fixtures::{project_handle, test_stores};
    use crate::prepare::build;

    fn opaque_doc(name: &str, blob: ResourceHandle) -> Document {
        Document {
            path: Route::new(RouteBase::Home, name).unwrap(),
            data: Data::Opaque(OpaqueDocument {
                handle: blob,
                mode: 0o644,
                unmanaged: false,
            }),
        }
    }

    #[test]
    fn decode_rejected_stale_version_with_got() {
        let raw = serde_json::json!({"version": 0u32, "documents": [], "hashes": {}});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match Manifest::decode(&bytes) {
            Ok(_) => panic!("stale manifest passes"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(*got, 0, "the error keeps the version");
                    assert!(error.context().is_empty(), "decode stays location-free");
                }
                other => panic!("wrong stale variant: {other}"),
            },
        }
    }

    #[test]
    fn persist_opaque_and_tree_members_into_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let opaque_sha = Sha::hash(b"opaque bytes");
        match stores
            .resources()
            .store_checked(&opaque_sha, std::io::Cursor::new(b"opaque bytes"))
        {
            Ok(_) => {}
            Err(error) => panic!("cache stores: {error}"),
        }
        let member_sha = Sha::hash(b"member bytes");
        match stores
            .resources()
            .store_checked(&member_sha, std::io::Cursor::new(b"member bytes"))
        {
            Ok(_) => {}
            Err(error) => panic!("cache stores: {error}"),
        }
        let mut members = BTreeMap::new();
        members.insert(
            "member.txt".to_owned(),
            TreeMember::Full {
                handle: ResourceHandle::Cache(member_sha.clone()),
                mode: 0o644,
            },
        );
        let manifest = Manifest {
            version: MANIFEST_VERSION,
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
            Err(error) => panic!("manifest persists: {error}"),
        }
        for (sha, body) in [
            (opaque_sha, b"opaque bytes".as_slice()),
            (member_sha, b"member bytes".as_slice()),
        ] {
            match stores.pool().open(&sha) {
                Ok(mut reader) => {
                    let mut found = Vec::new();
                    reader.read_to_end(&mut found).unwrap();
                    assert_eq!(found.as_slice(), body, "persisted bytes match the source");
                }
                Err(error) => panic!("persisted bytes open: {error}"),
            }
        }
    }

    #[test]
    fn persist_lands_live_bytes_in_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let body = b"sealed live bytes";
        let handle = project_handle(&stores, dir.path(), "note.txt", body);
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "note").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode: 0o644,
                unmanaged: false,
            }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("manifest hashes: {error}"),
        };
        let sha = Sha::hash(body);
        let live = match manifest.documents.first() {
            Some(document) => match &document.data {
                Data::Opaque(found) => found.handle.clone(),
                _ => panic!("live opaque keeps its shape"),
            },
            None => panic!("live build keeps its document"),
        };
        match &live {
            ResourceHandle::Project(_, found) => {
                assert_eq!(*found, sha, "build keeps the carried sha")
            }
            ResourceHandle::Cache(_) => panic!("build keeps live payloads"),
        }
        match manifest.persist(&stores) {
            Ok(()) => {}
            Err(error) => panic!("manifest persists: {error}"),
        }
        match stores.pool().open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, body, "persisted bytes match the live file");
            }
            Err(error) => panic!("persisted bytes open: {error}"),
        }
    }

    #[test]
    fn persist_reports_vanished_handles() {
        use confit_core::error::{ErrorCause, FilesystemError};

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
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "gone").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode: 0o644,
                unmanaged: false,
            }),
        )];
        let manifest = match build(documents, Vec::new()) {
            Ok(found) => found,
            Err(error) => panic!("live build keeps its document: {error}"),
        };
        match manifest.persist(&stores) {
            Ok(()) => panic!("vanished handles persist"),
            Err(error) => match error.cause() {
                ErrorCause::FilesystemError(FilesystemError::FileMissing) => {
                    match &error.context.fs_path {
                        Some(_) => {}
                        None => panic!("vanished keeps the attempted path: {error}"),
                    }
                }
                other => panic!("wrong persist cause: {other}"),
            },
        }
    }
}
