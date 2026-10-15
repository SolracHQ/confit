//! Payload
//! Blob bytes as live handles or sealed refs.

use confit_binding::face::{Face, FaceKind};
use confit_binding::registry::Registry;
use confit_binding::value::{Decode, Encode, Mismatch};
use confit_driver::sha::Sha;
use confit_store::Stores;
use confit_store::blob::BlobSource;
use confit_store::blob::error::BlobError;
use confit_store::handles::{BlobRef, ResourceHandle};
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

/// Blob payload as a live handle or a sealed ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// Live project handle awaiting compression.
    Handle(ResourceHandle),
    /// Sealed content ref with its pool identity.
    Resolved(BlobRef),
}

impl Payload {
    /// Resolve one payload into its blob ref.
    ///
    /// - Handles compress through the blob content index.
    /// - Resolved payloads pass through untouched.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed handle reads.
    /// - [`BlobError::Write`] for cache write faults.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    /// - [`BlobError::Compress`] for compression failures.
    pub fn resolve(&self, stores: &Stores) -> std::result::Result<BlobRef, BlobError> {
        match self {
            Self::Resolved(blob) => Ok(blob.clone()),
            Self::Handle(handle) => stores
                .blobs()
                .put(BlobSource::Handle(handle))
                .map(|opened| opened.to_ref()),
        }
    }

    /// Return the sealed blob ref.
    pub fn resolved(&self) -> Option<&BlobRef> {
        match self {
            Self::Resolved(blob) => Some(blob),
            Self::Handle(_) => None,
        }
    }

    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        match self {
            Self::Resolved(blob) => &blob.sha256,
            Self::Handle(handle) => handle.sha256(),
        }
    }
}

impl Face for Payload {
    fn face_name() -> String {
        ResourceHandle::face_name()
    }

    fn face_stub() -> String {
        String::new()
    }

    fn kind() -> FaceKind {
        FaceKind::Userdata
    }

    fn collect(collector: &mut Registry) -> confit_binding::error::Result<()> {
        ResourceHandle::collect(collector)
    }
}

impl Decode for Payload {
    fn decode(value: &Value, lua: &Lua) -> std::result::Result<Self, Mismatch> {
        ResourceHandle::decode(value, lua).map(Self::Handle)
    }
}

impl Encode for Payload {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            Self::Handle(handle) => handle.encode(lua),
            Self::Resolved(_) => Err(mlua::Error::RuntimeError(
                "cannot encode resolved payload to Lua".to_owned(),
            )),
        }
    }
}

impl Serialize for Payload {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Resolved(blob) => blob.serialize(serializer),
            Self::Handle(_) => Err(serde::ser::Error::custom("unsealed payload")),
        }
    }
}

impl<'de> Deserialize<'de> for Payload {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        BlobRef::deserialize(deserializer).map(Self::Resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Data;
    use confit_binding::value::Encode;
    use mlua::{Lua, Value};

    fn test_stores(dir: &std::path::Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            confit_store::StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn project_handle(stores: &Stores, dir: &std::path::Path, name: &str) -> ResourceHandle {
        let project = dir.join("project");
        confit_driver::fs::create_dir_all(&project).unwrap();
        confit_driver::fs::write(&project.join(name), b"payload bytes").unwrap();
        match stores.resources().resource(name) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        }
    }

    #[test]
    fn resolve_compresses_handles_through_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let handle = project_handle(&stores, dir.path(), "note.txt");
        let payload = Payload::Handle(handle);
        let first = match payload.resolve(&stores) {
            Ok(found) => found,
            Err(error) => panic!("handle resolves: {error}"),
        };
        assert_eq!(
            first.sha256,
            Sha::hash(b"payload bytes"),
            "resolve keeps the content hash"
        );
        guard.fail_writes(std::io::ErrorKind::StorageFull);
        match payload.resolve(&stores) {
            Ok(again) => assert_eq!(again, first, "repeat resolve hits the index"),
            Err(error) => panic!("indexed resolve skips writes: {error}"),
        }
    }

    #[test]
    fn resolve_passes_sealed_refs_without_io() {
        let dir = tempfile::tempdir().unwrap();
        let guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let blob = BlobRef::new(Sha::hash(b"sealed"), Sha::hash(b"stored"));
        guard.fail_writes(std::io::ErrorKind::StorageFull);
        match Payload::Resolved(blob.clone()).resolve(&stores) {
            Ok(found) => assert_eq!(found, blob, "sealed refs pass through"),
            Err(error) => panic!("sealed resolve skips io: {error}"),
        }
    }

    #[test]
    fn encode_renders_handles_and_refuses_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let handle = project_handle(&stores, dir.path(), "note.txt");
        let source = Data::Opaque {
            handle: Payload::Handle(handle),
            mode: 0o644,
            unmanaged: false,
        };
        match source.encode(&lua) {
            Ok(Value::Table(table)) => {
                let _: mlua::AnyUserData = table.get("handle").unwrap();
            }
            Ok(_) => panic!("handles encode a table"),
            Err(error) => panic!("handles encode: {error}"),
        }
        let blob = BlobRef::new(Sha::hash(b"sealed"), Sha::hash(b"stored"));
        let sealed = Data::Opaque {
            handle: Payload::Resolved(blob),
            mode: 0o644,
            unmanaged: false,
        };
        match sealed.encode(&lua) {
            Ok(_) => panic!("sealed payloads encode"),
            Err(error) => assert!(
                error.to_string().contains("resolved"),
                "the error shows the arm: {error}"
            ),
        }
    }

    #[test]
    fn serde_round_trips_sealed_and_refuses_handles() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let handle = project_handle(&stores, dir.path(), "note.txt");
        match serde_json::to_string(&Payload::Handle(handle)) {
            Ok(_) => panic!("handles serialize"),
            Err(error) => assert!(
                error.to_string().contains("unsealed"),
                "the error shows the state: {error}"
            ),
        }
        let blob = BlobRef::new(Sha::hash(b"sealed"), Sha::hash(b"stored"));
        let text = match serde_json::to_string(&Payload::Resolved(blob.clone())) {
            Ok(found) => found,
            Err(error) => panic!("sealed payloads serialize: {error}"),
        };
        assert!(
            text.contains(&blob.sha256.hex()) && text.contains(&blob.stored.hex()),
            "sealed json carries both hexes: {text}"
        );
        match serde_json::from_str::<Payload>(&text) {
            Ok(Payload::Resolved(found)) => assert_eq!(found, blob, "sealed json decodes"),
            Ok(_) => panic!("sealed json decodes to resolved"),
            Err(error) => panic!("sealed json decodes: {error}"),
        }
    }
}
