//! Payload
//! Blob bytes as live handles or sealed refs.

use confit_binding::face::{Face, FaceKind};
use confit_binding::registry::Registry;
use confit_binding::value::{Decode, Encode, Mismatch};
use confit_driver::sha::Sha;
use confit_store::Stores;
use confit_store::blob::error::BlobError;
use confit_store::handles::ResourceHandle;
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

/// Blob payload as a live handle or sealed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// Live project handle awaiting compression.
    Handle(ResourceHandle),
    /// Sealed content hash with its pool identity.
    Resolved(Sha),
}

impl Payload {
    /// Resolve one payload into its content hash.
    ///
    /// - Handles compress into the blob pool.
    /// - Resolved payloads pass through untouched.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for unreadable handle content.
    /// - [`BlobError::Write`] for cache write faults.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    /// - [`BlobError::Compress`] for compression failures.
    pub fn resolve(&self, stores: &Stores) -> std::result::Result<Sha, BlobError> {
        match self {
            Self::Resolved(blob) => Ok(blob.clone()),
            Self::Handle(handle) => stores
                .blobs()
                .put(handle)
                .map(|opened| opened.sha256().clone()),
        }
    }

    /// Return the sealed content hash.
    pub fn resolved(&self) -> Option<&Sha> {
        match self {
            Self::Resolved(blob) => Some(blob),
            Self::Handle(_) => None,
        }
    }

    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        match self {
            Self::Resolved(blob) => blob,
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
        Sha::deserialize(deserializer).map(Self::Resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Data;
    use confit_binding::value::Encode;
    use confit_driver as driver;
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
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(name), b"payload bytes").unwrap();
        match stores.resources().resource(name) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        }
    }

    #[test]
    fn resolve_compresses_handles_into_the_pool() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let handle = project_handle(&stores, dir.path(), "note.txt");
        let payload = Payload::Handle(handle);
        let first = match payload.resolve(&stores) {
            Ok(found) => found,
            Err(error) => panic!("handle resolves: {error}"),
        };
        assert_eq!(
            first,
            Sha::hash(b"payload bytes"),
            "resolve keeps the content hash"
        );
        let cached = dir.path().join("temp").join("blobs");
        driver::fs::set_mode(&cached, Some(0o555)).unwrap();
        match payload.resolve(&stores) {
            Ok(again) => assert_eq!(again, first, "repeat resolve reuses the file"),
            Err(error) => panic!("present resolve skips writes: {error}"),
        }
        driver::fs::set_mode(&cached, Some(0o755)).unwrap();
    }

    #[test]
    fn resolve_passes_sealed_refs_without_io() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let blob = Sha::hash(b"sealed");
        match Payload::Resolved(blob.clone()).resolve(&stores) {
            Ok(found) => assert_eq!(found, blob, "sealed refs pass through"),
            Err(error) => panic!("sealed resolve skips io: {error}"),
        }
    }

    #[test]
    fn encode_renders_handles_and_refuses_sealed() {
        let dir = tempfile::tempdir().unwrap();
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
        let blob = Sha::hash(b"sealed");
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
        let stores = test_stores(dir.path());
        let handle = project_handle(&stores, dir.path(), "note.txt");
        match serde_json::to_string(&Payload::Handle(handle)) {
            Ok(_) => panic!("handles serialize"),
            Err(error) => assert!(
                error.to_string().contains("unsealed"),
                "the error shows the state: {error}"
            ),
        }
        let blob = Sha::hash(b"sealed");
        let text = match serde_json::to_string(&Payload::Resolved(blob.clone())) {
            Ok(found) => found,
            Err(error) => panic!("sealed payloads serialize: {error}"),
        };
        assert!(
            text.contains(&blob.hex()),
            "sealed json carries the hex: {text}"
        );
        match serde_json::from_str::<Payload>(&text) {
            Ok(Payload::Resolved(found)) => assert_eq!(found, blob, "sealed json decodes"),
            Ok(_) => panic!("sealed json decodes to resolved"),
            Err(error) => panic!("sealed json decodes: {error}"),
        }
    }
}
