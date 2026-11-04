//! Reads one portable bundle archive.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    BundleError, ContextualCause, ContextualResult, DocumentError, Require, Terminal,
    TerminalResult, WithContext, contextual, error,
};
use confit_core::sha::Sha;
use confit_model::document::Data;

use crate::{BUNDLE_MANIFEST, Bundler};
use confit_driver as driver;
use confit_model::manifest::Manifest;

impl Bundler {
    /// Reads one bundle archive.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] when the bundle fails to open or the bytes are foreign.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] when a member body fails, with the member slot.
    pub fn read(&self, path: &Path) -> TerminalResult<Manifest> {
        Unpack::new(self, path).run()
    }
}

/// The read state of one bundle.
struct Unpack<'a> {
    /// The bundler that reads the bundle.
    bundler: &'a Bundler,
    /// The bundle path.
    bundle: &'a Path,
    /// The buffered manifest bytes.
    raw: Option<Vec<u8>>,
}

impl<'a> Unpack<'a> {
    /// Builds one walk with no buffered manifest.
    fn new(bundler: &'a Bundler, bundle: &'a Path) -> Self {
        Self {
            bundler,
            bundle,
            raw: None,
        }
    }

    /// Runs one walk and returns its manifest.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] when the bundle fails to open or the bytes are foreign.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] when a member body fails, with the member slot.
    fn run(mut self) -> TerminalResult<Manifest> {
        let bundle: PathBuf = self.bundle.to_path_buf();
        let file = std::fs::File::open(self.bundle)
            .map_err(ContextualCause::from)
            .with_bundle_path(&bundle)
            .terminal(ErrorKind::ReadBundle)?;
        let walk = driver::archive::uncompress(file, |entry| self.visit(entry));
        match walk {
            Ok(()) => self.finish(),
            Err(contextual) => {
                let kind = match contextual.context.member {
                    Some(_) => ErrorKind::UnpackMember,
                    None => ErrorKind::ReadBundle,
                };
                let document = if matches!(kind, ErrorKind::UnpackMember) {
                    match &contextual.context.member {
                        Some(member) => self.required_entry(member),
                        None => None,
                    }
                } else {
                    None
                };
                let mut contextual = contextual.with_bundle_path(bundle);
                if let Some(route) = document {
                    contextual = contextual.with_document(route);
                }
                error(kind, contextual)
            }
        }
    }

    /// Visits one walk entry and adds the member slot on faults.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError`] when a manifest entry doubles.
    /// - [`confit_core::error::DocumentError`] when an entry name is bad, with the member slot.
    /// - [`confit_core::error::ErrorCause`] when the content fails, with the member slot.
    fn visit(&mut self, entry: driver::archive::Entry<'_>) -> ContextualResult<()> {
        let name = entry.name.clone();
        self.entry(entry).with_member(name)
    }

    /// Receives one walk entry and routes it to the manifest buffer or the cache.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError::DuplicateManifest`] when a manifest entry doubles.
    /// - [`confit_core::error::DocumentError::ParseFailed`] when an entry name is bad.
    /// - [`confit_core::error::ErrorCause`] when the content fails.
    fn entry(&mut self, entry: driver::archive::Entry<'_>) -> ContextualResult<()> {
        if entry.name == BUNDLE_MANIFEST {
            return self.receive_manifest(entry.reader);
        }
        self.receive_content(entry.name, entry.reader)
    }

    /// Buffers the manifest entry body for the later decode.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError::DuplicateManifest`] when a manifest entry doubles.
    /// - [`confit_core::error::FilesystemError`] when the manifest body is cut.
    fn receive_manifest(&mut self, mut reader: impl std::io::Read) -> ContextualResult<()> {
        if self.raw.is_some() {
            return Err(contextual(BundleError::DuplicateManifest));
        }
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        self.raw = Some(bytes);
        Ok(())
    }

    /// Stores one content entry body in the cache under its name hash.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when an entry name is bad.
    /// - [`confit_core::error::ErrorCause`] when the content fails.
    fn receive_content(
        &mut self,
        name: String,
        reader: impl std::io::Read,
    ) -> ContextualResult<()> {
        let sha = match Sha::new(name.as_str()) {
            Ok(sha) => sha,
            Err(_) => {
                return Err(contextual(DocumentError::ParseFailed {
                    require: Require::MemberPath,
                }));
            }
        };
        self.bundler.resources.store_checked(&sha, reader)?;
        Ok(())
    }

    /// Returns the route of the document that requires one member.
    fn required_entry(&self, member: &str) -> Option<String> {
        let raw = self.raw.as_deref()?;
        let manifest = Manifest::decode(raw).ok()?;
        let sha = Sha::new(member).ok()?;
        required_by(&manifest, &sha)
    }

    /// Decodes the buffered manifest and checks that every member is present.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] when the manifest is absent or the payload is bad.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] when content is absent, with the bundle and
    ///   member slots.
    fn finish(self) -> TerminalResult<Manifest> {
        let bundle: PathBuf = self.bundle.to_path_buf();
        let manifest = match self.raw {
            None => {
                return error(
                    ErrorKind::ReadBundle,
                    contextual(BundleError::ManifestMissing).with_bundle_path(bundle),
                );
            }
            Some(bytes) => Manifest::decode(&bytes)
                .with_bundle_path(&bundle)
                .terminal(ErrorKind::ReadBundle)?,
        };
        let mut shas: BTreeSet<Sha> = BTreeSet::new();
        for handle in manifest.handles() {
            shas.insert(*handle.sha());
        }
        for sha in &shas {
            if !self.bundler.resources.cache_path(sha).exists() {
                let mut contextual = contextual(BundleError::ContentMissing)
                    .with_bundle_path(&bundle)
                    .with_member(sha.hex());
                if let Some(route) = required_by(&manifest, sha) {
                    contextual = contextual.with_document(route);
                }
                return error(ErrorKind::UnpackMember, contextual);
            }
        }
        Ok(manifest)
    }
}

/// Returns the route of the document that requires one content hash.
fn required_by(manifest: &Manifest, sha: &Sha) -> Option<String> {
    manifest.documents.iter().find_map(|document| {
        let held = match &document.data {
            Data::Opaque(found) => found.handle.sha() == sha,
            Data::Tree(found) => found
                .members
                .values()
                .any(|member| member.handle().sha() == sha),
            Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => false,
        };
        held.then(|| document.path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::{ErrorCause, ManifestError};

    use confit_core::resource_ref::ResourceRef;
    use confit_core::version::VERSION;
    use confit_model::document::{Data, Document, FileMode, OpaqueDocument};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn project_file(dir: &Path, name: &str, body: &[u8]) -> std::path::PathBuf {
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn opaque_manifest(dir: &Path, bodies: &[&[u8]]) -> (Bundler, Manifest) {
        use confit_store::Stores;

        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots, sender);
        let mut documents = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let name = format!("bin-{index}");
            project_file(dir, &name, body);
            let handle = match stores.resources().resource(&name) {
                Ok(found) => found,
                Err(error) => panic!("the project fails to open: {error}"),
            };
            documents.push(Document {
                path: Route::new(RouteBase::Home, name.as_str()).unwrap(),
                data: Data::Opaque(OpaqueDocument {
                    handle,
                    mode: FileMode::new(0o644),
                    unmanaged: false,
                }),
            });
        }
        let manifest = Manifest {
            version: VERSION,
            documents,
            hooks: Vec::new(),
        };
        (Bundler::new(&stores), manifest)
    }

    fn build_bundle(dest: &Path, entries: &[(&str, Vec<u8>)]) {
        use confit_driver::archive::{Algorithm, Builder, Level};

        let build = |file: &mut std::fs::File| {
            let mut builder = Builder::create(file, Algorithm::TarGz, Level::default());
            for (name, bytes) in entries {
                let mut reader = std::io::Cursor::new(bytes.as_slice());
                builder
                    .member(name, 0o644, bytes.len() as u64, &mut reader)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            }
            builder
                .finish()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(())
        };
        if let Err(error) = driver::StagedFile::write(dest, build) {
            panic!("the fixture bundle fails to build: {error}");
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        driver::archive::names(file).unwrap()
    }

    #[test]
    fn bundle_round_trip_writes_the_manifest_first_with_sorted_contents() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"alpha", b"beta"]);
        let plan = dir.path().join("plan");
        let dest = match store.write(&manifest, &plan) {
            Ok(dest) => dest,
            Err(error) => panic!("the manifest fails to write: {error}"),
        };
        assert_eq!(
            dest,
            dir.path().join("plan.cb"),
            "the bare output gains the suffix"
        );
        let names = entry_names(&dest);
        assert_eq!(names[0], BUNDLE_MANIFEST, "the manifest comes first");
        let contents: Vec<String> = names[1..].to_vec();
        assert_eq!(contents.len(), 2, "the bundle ships every content once");
        let mut sorted = contents.clone();
        sorted.sort();
        assert_eq!(sorted, contents, "the content entries sort by hash");
        for name in &contents {
            assert_eq!(name.len(), 64, "the content name holds a hash: {name}");
            assert!(
                name.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "the content name is hex: {name}"
            );
        }
        match store.read(&dest) {
            Ok(found) => {
                assert_eq!(
                    found.documents, manifest.documents,
                    "the documents round-trip"
                );
                let carried = |manifest: &Manifest| {
                    manifest
                        .handles()
                        .iter()
                        .map(|handle| *handle.sha())
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    carried(&found),
                    carried(&manifest),
                    "the contents round-trip"
                );
            }
            Err(error) => panic!("the manifest fails to load: {error}"),
        }
    }

    #[test]
    fn read_rejects_stale_version() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);
        let stale = serde_json::json!({"version": [0, 8, 0], "documents": []});
        let raw = serde_json::to_vec(&stale).unwrap();
        let stale_path = dir.path().join("stale.cb");
        build_bundle(&stale_path, &[(BUNDLE_MANIFEST, raw)]);
        match store.read(&stale_path) {
            Ok(_) => panic!("expected a version rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(got, "[0, 8, 0]", "the rejection keeps the version");
                    assert!(
                        error
                            .to_string()
                            .contains(&stale_path.display().to_string()),
                        "the rejection keeps the bundle path"
                    );
                }
                other => panic!("expected a version rejection: {other:?}"),
            },
        }
    }

    #[test]
    fn read_rejects_a_bundle_with_absent_content() {
        use confit_store::Stores;

        let dir = tempfile::tempdir().unwrap();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots.clone(), sender);
        let wanted = Sha::hash(b"wanted bytes");
        project_file(dir.path(), "wanted.bin", b"wanted bytes");
        let handle = match stores.resources().resource("wanted.bin") {
            Ok(found) => found,
            Err(error) => panic!("the project fails to open: {error}"),
        };
        assert_eq!(handle.sha(), &wanted);
        let manifest = Manifest {
            version: VERSION,
            documents: vec![Document {
                path: Route::new(RouteBase::Home, "bin").unwrap(),
                data: Data::Opaque(OpaqueDocument {
                    handle,
                    mode: FileMode::new(0o644),
                    unmanaged: false,
                }),
            }],
            hooks: Vec::new(),
        };
        let raw = serde_json::to_vec(&manifest).unwrap();
        let manifest_only = dir.path().join("manifest_only.cb");
        build_bundle(&manifest_only, &[(BUNDLE_MANIFEST, raw)]);
        let store = Bundler::new(&stores);
        match store.read(&manifest_only) {
            Ok(_) => panic!("expected a content rejection"),
            Err(error) => match error.cause() {
                ErrorCause::BundleError(BundleError::ContentMissing) => {
                    assert!(
                        error.to_string().contains("home('bin')"),
                        "the rejection keeps the requiring document"
                    );
                }
                other => panic!("expected a content rejection: {other:?}"),
            },
        }
    }

    #[test]
    fn read_rejects_duplicate_manifests_and_invalid_entry_names() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);

        let manifest = serde_json::to_vec(&Manifest {
            version: VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        })
        .unwrap();
        let dup_path = dir.path().join("dup-manifest.cb");
        build_bundle(
            &dup_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                (BUNDLE_MANIFEST, manifest.clone()),
            ],
        );
        match store.read(&dup_path) {
            Ok(_) => panic!("expected a manifest rejection"),
            Err(error) => match error.cause() {
                ErrorCause::BundleError(BundleError::DuplicateManifest) => {
                    assert!(
                        error.to_string().contains(BUNDLE_MANIFEST),
                        "the rejection shows the manifest"
                    );
                }
                other => panic!("expected a manifest rejection: {other:?}"),
            },
        }

        let bad_path = dir.path().join("bad-id.cb");
        build_bundle(
            &bad_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("short", b"x".to_vec()),
            ],
        );
        match store.read(&bad_path) {
            Ok(_) => panic!("expected a member rejection"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => {
                            assert_eq!(member, "short", "the rejection keeps the member name")
                        }
                        None => panic!("expected a member name: {error}"),
                    }
                    assert!(
                        error.to_string().contains("short"),
                        "the rejection shows the entry"
                    );
                }
                other => panic!("expected a member rejection: {other:?}"),
            },
        }

        let odd_path = dir.path().join("odd.cb");
        build_bundle(
            &odd_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("other.txt", b"x".to_vec()),
            ],
        );
        match store.read(&odd_path) {
            Ok(_) => panic!("expected a member rejection"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => {
                            assert_eq!(member, "other.txt", "the rejection keeps the member name")
                        }
                        None => panic!("expected a member name: {error}"),
                    }
                    assert!(
                        error.to_string().contains("other.txt"),
                        "the rejection shows the entry"
                    );
                }
                other => panic!("expected a member rejection: {other:?}"),
            },
        }
    }

    #[test]
    fn read_stops_at_the_first_invalid_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);

        let empty = serde_json::to_vec(&Manifest {
            version: VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        })
        .unwrap();
        let mixed_path = dir.path().join("mixed.cb");
        build_bundle(
            &mixed_path,
            &[
                ("nope", b"x".to_vec()),
                (BUNDLE_MANIFEST, empty.clone()),
                (BUNDLE_MANIFEST, empty),
            ],
        );
        match store.read(&mixed_path) {
            Ok(_) => panic!("expected a member rejection"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => {
                            assert_eq!(member, "nope", "the rejection keeps the member name")
                        }
                        None => panic!("expected a member name: {error}"),
                    }
                    assert!(
                        error.to_string().contains("nope"),
                        "the rejection shows the entry"
                    );
                }
                other => panic!("expected a member rejection: {other:?}"),
            },
        }

        let sha = Sha::hash(b"alpha");
        let drift = serde_json::to_vec(&Manifest {
            version: VERSION,
            documents: vec![Document::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: ResourceRef::new(sha),
                    mode: FileMode::new(0o644),
                    unmanaged: false,
                }),
            )],
            hooks: Vec::new(),
        })
        .unwrap();
        let drift_path = dir.path().join("drift.cb");
        build_bundle(
            &drift_path,
            &[
                (BUNDLE_MANIFEST, drift),
                (sha.hex().as_str(), b"beta".to_vec()),
            ],
        );
        match store.read(&drift_path) {
            Ok(_) => panic!("expected a content rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { .. } => {
                    assert!(
                        error.to_string().contains("home('bin')"),
                        "the rejection shows the requiring document"
                    );
                }
                other => panic!("expected a content rejection: {other:?}"),
            },
        }
    }

    #[test]
    fn read_rejects_broken_envelope_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"alpha"]);
        let plan = dir.path().join("plan");
        let dest = match store.write(&manifest, &plan) {
            Ok(dest) => dest,
            Err(error) => panic!("the manifest fails to write: {error}"),
        };

        let foreign_path = dir.path().join("foreign.cb");
        std::fs::write(&foreign_path, b"not an archive").unwrap();
        match store.read(&foreign_path) {
            Ok(_) => panic!("expected an archive rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ArchiveError(_) => {
                    assert!(
                        error
                            .to_string()
                            .contains(&foreign_path.display().to_string()),
                        "the rejection keeps the bundle path"
                    );
                }
                other => panic!("expected an archive rejection: {other:?}"),
            },
        }

        let raw = std::fs::read(&dest).unwrap();
        let cut_path = dir.path().join("cut.cb");
        std::fs::write(&cut_path, &raw[..raw.len() / 2]).unwrap();
        match store.read(&cut_path) {
            Ok(_) => panic!("expected a member rejection"),
            Err(error) => match error.cause() {
                ErrorCause::FilesystemError(_) => {
                    assert!(
                        error.to_string().contains(BUNDLE_MANIFEST),
                        "the rejection shows the manifest entry"
                    );
                }
                other => panic!("expected a member rejection: {other:?}"),
            },
        }
    }

    #[test]
    fn write_builds_gzipped_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"legacy bytes"]);
        let plan = dir.path().join("plan");
        let dest = match store.write(&manifest, &plan) {
            Ok(dest) => dest,
            Err(error) => panic!("the manifest fails to write: {error}"),
        };
        let raw = std::fs::read(&dest).unwrap();
        assert!(!raw.is_empty(), "the bundle holds bytes");
        assert_eq!(
            &raw[0..2],
            &[0x1f, 0x8b],
            "the envelope carries the gzip magic"
        );
    }
}
