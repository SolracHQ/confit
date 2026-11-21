//! Manifest builds with portable bundle archives.

#![deny(missing_docs)]

mod pack;
mod unpack;
use std::sync::Arc;

use confit_events::EventHandles;
use confit_store::Stores;
use confit_store::resources::Resources;

/// The archive member name of the bundle manifest.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// A reader and writer for portable bundle archives.
#[derive(Debug, Clone)]
pub struct Bundler {
    /// The shared resource store.
    pub(crate) resources: Arc<Resources>,
    /// The UI communication channel.
    pub(crate) handles: EventHandles,
}

impl Bundler {
    /// Builds one bundler.
    pub fn new(stores: &Stores, handles: EventHandles) -> Self {
        Self {
            resources: stores.resources(),
            handles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;

    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_core::version::VERSION;
    use confit_events::EventHandles;
    use confit_model::document::Data;
    use confit_model::document::Document;
    use confit_model::document::FileMode;
    use confit_model::document::MemberKey;
    use confit_model::document::OpaqueDocument;
    use confit_model::document::TreeDocument;
    use confit_model::document::TreeMember;
    use confit_model::manifest::Manifest;
    use confit_model::routes::Route;
    use confit_model::routes::RouteBase;

    /// Builds stores under one temporary folder.
    fn test_stores(dir: &Path) -> Stores {
        Stores::new(
            confit_store::StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            EventHandles::silent(),
        )
    }

    /// Writes one project file and returns its trusted hash.
    fn project_handle(stores: &Stores, dir: &Path, name: &str, body: &[u8]) -> ResourceRef {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(name), body).unwrap();
        match stores.resources().resource(name) {
            Ok(found) => found,
            Err(error) => panic!("the project fails to open: {error}"),
        }
    }

    fn opaque_doc(name: &str, blob: ResourceRef) -> Document {
        Document {
            path: Route::new(RouteBase::Home, name).unwrap(),
            data: Data::Opaque(OpaqueDocument {
                handle: blob,
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        }
    }

    fn member_key(relative: &str) -> MemberKey {
        match MemberKey::new(relative.to_owned()) {
            Ok(found) => found,
            Err(error) => panic!("the key fails to parse: {error}"),
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
            member_key("member.txt"),
            TreeMember::Full {
                handle: ResourceRef::new(member_sha),
                mode: FileMode::new(0o644),
            },
        );
        let manifest = Manifest {
            version: VERSION,
            documents: vec![
                opaque_doc("blob.bin", ResourceRef::new(opaque_sha)),
                Document::new(
                    Route::new(RouteBase::Home, "fonts").unwrap(),
                    Data::Tree(TreeDocument { members }),
                ),
            ],
            hooks: Vec::new(),
        };
        if let Err(error) = stores.resources().persist(&manifest.handles()) {
            panic!("the manifest fails to persist: {error}");
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
    fn persist_lands_project_bytes_in_the_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let body = b"packaged project bytes";
        let handle = project_handle(&stores, dir.path(), "note.txt", body);
        let documents = vec![Document::new(
            Route::new(RouteBase::Home, "note").unwrap(),
            Data::Opaque(OpaqueDocument {
                handle,
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        )];
        let manifest = Manifest {
            version: VERSION,
            documents,
            hooks: Vec::new(),
        };
        let sha = Sha::hash(body);
        let found = match manifest.documents.first() {
            Some(document) => match &document.data {
                Data::Opaque(opaque) => opaque.handle,
                _ => panic!("expected an opaque body"),
            },
            None => panic!("expected a document"),
        };
        assert_eq!(
            found,
            ResourceRef::new(sha),
            "the manifest keeps the carried sha"
        );
        if let Err(error) = stores.resources().persist(&manifest.handles()) {
            panic!("the manifest fails to persist: {error}");
        }
        match stores.pool().open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, body, "the persisted bytes match the project file");
            }
            Err(error) => panic!("the persisted bytes fail to open: {error}"),
        }
    }

    #[test]
    fn persist_reports_a_vanished_handle() {
        use confit_core::error::FilesystemError;

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
        let manifest = Manifest {
            version: VERSION,
            documents,
            hooks: Vec::new(),
        };
        if let Err(error) = stores.resources().persist(&manifest.handles()) {
            match error.cause() {
                confit_core::error::ErrorCause::FilesystemError(FilesystemError::FileMissing) => {
                    match &error.context.fs_path {
                        Some(_) => {}
                        None => panic!("expected the attempted path: {error}"),
                    }
                }
                other => panic!("expected a file error: {other}"),
            }
        } else {
            panic!("expected a persist rejection");
        }
    }
}
