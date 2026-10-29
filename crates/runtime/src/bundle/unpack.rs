//! Bundle input.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    BundleError, ContextualCause, ContextualResult, DocumentError, Require, Terminal,
    TerminalResult, WithContext, contextual, error,
};
use confit_core::sha::Sha;
use confit_model::document::Data;

use super::{BUNDLE_MANIFEST, Bundler};
use crate::manifest::Manifest;
use confit_driver as driver;

impl Bundler {
    /// Read one portable bundle into a sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for failed bundle opens and foreign bytes.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for failed member bodies with the member slot.
    pub fn read(&self, path: &Path) -> TerminalResult<Manifest> {
        Unpack::new(self, path).run()
    }
}

/// One bundle walk.
struct Unpack<'a> {
    /// The bundler under reading.
    bundler: &'a Bundler,
    /// The bundle path under reading.
    bundle: &'a Path,
    /// The buffered manifest bytes.
    raw: Option<Vec<u8>>,
}

impl<'a> Unpack<'a> {
    /// Build one bundle walk.
    fn new(bundler: &'a Bundler, bundle: &'a Path) -> Self {
        Self {
            bundler,
            bundle,
            raw: None,
        }
    }

    /// Run one walk into the sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for failed bundle opens and foreign bytes.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for failed member bodies with the member slot.
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

    /// Visit one walk entry with the member slot on faults.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError`] for doubled manifest entries.
    /// - [`confit_core::error::DocumentError`] for bad entry names with the member slot.
    /// - [`confit_core::error::ErrorCause`] for failed contents with the member slot.
    fn visit(&mut self, entry: driver::archive::Entry<'_>) -> ContextualResult<()> {
        let name = entry.name.clone();
        self.entry(entry).with_member(name)
    }

    /// Receive one walk entry into the manifest buffer or the cache.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError::DuplicateManifest`] for doubled manifest entries.
    /// - [`confit_core::error::DocumentError::ParseFailed`] for bad entry names.
    /// - [`confit_core::error::ErrorCause`] for failed contents.
    fn entry(&mut self, entry: driver::archive::Entry<'_>) -> ContextualResult<()> {
        if entry.name == BUNDLE_MANIFEST {
            return self.receive_manifest(entry.reader);
        }
        self.receive_content(entry.name, entry.reader)
    }

    /// Buffer one manifest entry body.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::BundleError::DuplicateManifest`] for doubled manifest entries.
    /// - [`confit_core::error::FilesystemError`] for cut manifest bodies.
    fn receive_manifest(&mut self, mut reader: impl std::io::Read) -> ContextualResult<()> {
        if self.raw.is_some() {
            return Err(contextual(BundleError::DuplicateManifest));
        }
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        self.raw = Some(bytes);
        Ok(())
    }

    /// Land one content entry body in the cache under its name hash.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for bad entry names.
    /// - [`confit_core::error::ErrorCause`] for failed contents.
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

    /// Return the requiring document route for one member from the manifest.
    fn required_entry(&self, member: &str) -> Option<String> {
        let raw = self.raw.as_deref()?;
        let manifest = Manifest::decode(raw).ok()?;
        let sha = Sha::new(member).ok()?;
        required_by(&manifest, &sha)
    }

    /// Decode the buffered manifest with presence checks.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for absent manifests and bad payloads.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for absent contents with bundle and member slots.
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
            shas.insert(handle.sha().clone());
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

/// Return the document route requiring one content hash.
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

    use std::collections::BTreeMap;

    use crate::manifest::MANIFEST_VERSION;
    use crate::prepare::build;
    use confit_model::document::{Data, Document, OpaqueDocument};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;
    use confit_store::handles::ResourceHandle;

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
            let path = project_file(dir, &name, body);
            documents.push(Document {
                path: Route::new(RouteBase::Home, name.as_str()).unwrap(),
                data: Data::Opaque(OpaqueDocument {
                    handle: ResourceHandle::Project(path, confit_core::sha::Sha::hash(body)),
                    mode: 0o644,
                    unmanaged: false,
                }),
            });
        }
        match build(documents, Vec::new()) {
            Ok(manifest) => (Bundler::new(&stores), manifest),
            Err(error) => panic!("manifest builds: {error}"),
        }
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
            panic!("fixture bundle builds: {error}");
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        driver::archive::names(file).unwrap()
    }

    #[test]
    fn round_trip_writes_manifest_first_with_sorted_contents() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"alpha", b"beta"]);
        let plan = dir.path().join("plan");
        let dest = match store.write(&manifest, &plan) {
            Ok(dest) => dest,
            Err(error) => panic!("manifest writes: {error}"),
        };
        assert_eq!(dest, dir.path().join("plan.cb"), "bare output gains suffix");
        let names = entry_names(&dest);
        assert_eq!(names[0], BUNDLE_MANIFEST, "the manifest comes first");
        let contents: Vec<String> = names[1..].to_vec();
        assert_eq!(contents.len(), 2, "every content ships once");
        let mut sorted = contents.clone();
        sorted.sort();
        assert_eq!(sorted, contents, "content entries sort by hash");
        for name in &contents {
            assert_eq!(name.len(), 64, "content names carry hashes: {name}");
            assert!(
                name.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "content names stay hex: {name}"
            );
        }
        match store.read(&dest) {
            Ok(found) => {
                assert_eq!(found.hashes, manifest.hashes, "hashes round-trip");
                let carried = |manifest: &Manifest| {
                    manifest
                        .handles()
                        .iter()
                        .map(|handle| handle.sha().clone())
                        .collect::<Vec<_>>()
                };
                assert_eq!(carried(&found), carried(&manifest), "contents round-trip");
            }
            Err(error) => panic!("manifest load: {error}"),
        }
    }

    #[test]
    fn read_rejects_stale_version() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);
        let stale = serde_json::json!({"version": 0u32, "documents": [], "hashes": {}});
        let raw = serde_json::to_vec(&stale).unwrap();
        let stale_path = dir.path().join("stale.cb");
        build_bundle(&stale_path, &[(BUNDLE_MANIFEST, raw)]);
        match store.read(&stale_path) {
            Ok(_) => panic!("stale manifest passes"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(*got, 0, "stale keeps version");
                    assert!(
                        error
                            .to_string()
                            .contains(&stale_path.display().to_string()),
                        "stale keeps bundle"
                    );
                }
                other => panic!("wrong stale cause: {other:?}"),
            },
        }
    }

    #[test]
    fn read_rejects_missing_content() {
        use confit_store::Stores;

        let dir = tempfile::tempdir().unwrap();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots.clone(), sender);
        let path = project_file(dir.path(), "wanted.bin", b"wanted bytes");
        let manifest = match build(
            vec![Document {
                path: Route::new(RouteBase::Home, "bin").unwrap(),
                data: Data::Opaque(OpaqueDocument {
                    handle: ResourceHandle::Project(
                        path,
                        confit_core::sha::Sha::hash(b"wanted bytes"),
                    ),
                    mode: 0o644,
                    unmanaged: false,
                }),
            }],
            Vec::new(),
        ) {
            Ok(manifest) => manifest,
            Err(error) => panic!("manifest builds: {error}"),
        };
        let raw = serde_json::to_vec(&manifest).unwrap();
        let thin = dir.path().join("thin.cb");
        build_bundle(&thin, &[(BUNDLE_MANIFEST, raw)]);
        let store = Bundler::new(&stores);
        match store.read(&thin) {
            Ok(_) => panic!("thin manifest passes"),
            Err(error) => match error.cause() {
                ErrorCause::BundleError(BundleError::ContentMissing) => {
                    assert!(
                        error.to_string().contains("home('bin')"),
                        "missing keeps the requiring document"
                    );
                }
                other => panic!("wrong missing cause: {other:?}"),
            },
        }
    }

    #[test]
    fn read_rejects_duplicate_and_unexpected_and_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);

        let manifest = serde_json::to_vec(&Manifest {
            version: MANIFEST_VERSION,
            documents: Vec::new(),
            hashes: BTreeMap::new(),
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
            Ok(_) => panic!("duplicate manifest passes"),
            Err(error) => match error.cause() {
                ErrorCause::BundleError(BundleError::DuplicateManifest) => {
                    assert!(
                        error.to_string().contains(BUNDLE_MANIFEST),
                        "duplicate names the manifest"
                    );
                }
                other => panic!("wrong duplicate cause: {other:?}"),
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
            Ok(_) => panic!("bad id passes"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => assert_eq!(member, "short", "bad id keeps its input"),
                        None => panic!("bad id keeps its input: {error}"),
                    }
                    assert!(
                        error.to_string().contains("short"),
                        "bad id names the entry"
                    );
                }
                other => panic!("wrong bad id cause: {other:?}"),
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
            Ok(_) => panic!("unexpected entry passes"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => {
                            assert_eq!(member, "other.txt", "unexpected keeps its input")
                        }
                        None => panic!("unexpected keeps its input: {error}"),
                    }
                    assert!(
                        error.to_string().contains("other.txt"),
                        "unexpected names the entry"
                    );
                }
                other => panic!("wrong unexpected cause: {other:?}"),
            },
        }
    }

    #[test]
    fn read_faults_fail_at_first_bad_entry() {
        use confit_core::sha::Sha;

        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);

        let empty = serde_json::to_vec(&Manifest {
            version: MANIFEST_VERSION,
            documents: Vec::new(),
            hashes: BTreeMap::new(),
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
            Ok(_) => panic!("mixed bundle passes"),
            Err(error) => match error.cause() {
                ErrorCause::DocumentError(DocumentError::ParseFailed { .. }) => {
                    match &error.context().member {
                        Some(member) => {
                            assert_eq!(member, "nope", "first bad entry keeps its input")
                        }
                        None => panic!("first bad entry keeps its input: {error}"),
                    }
                    assert!(
                        error.to_string().contains("nope"),
                        "first bad entry names itself"
                    );
                }
                other => panic!("wrong mixed cause: {other:?}"),
            },
        }

        let sha = Sha::hash(b"alpha");
        let drift = serde_json::to_vec(&Manifest {
            version: MANIFEST_VERSION,
            documents: vec![Document::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: ResourceHandle::Cache(sha.clone()),
                    mode: 0o644,
                    unmanaged: false,
                }),
            )],
            hashes: BTreeMap::new(),
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
            Ok(_) => panic!("drifted bytes pass"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { .. } => {
                    assert!(
                        error.to_string().contains("home('bin')"),
                        "drifted bytes name the requiring document"
                    );
                }
                other => panic!("wrong drift cause: {other:?}"),
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
            Err(error) => panic!("manifest writes: {error}"),
        };

        let foreign_path = dir.path().join("foreign.cb");
        std::fs::write(&foreign_path, b"not an archive").unwrap();
        match store.read(&foreign_path) {
            Ok(_) => panic!("foreign bytes pass"),
            Err(error) => match error.cause() {
                ErrorCause::ArchiveError(_) => {
                    assert!(
                        error
                            .to_string()
                            .contains(&foreign_path.display().to_string()),
                        "failed member keeps the bundle"
                    );
                }
                other => panic!("wrong foreign cause: {other:?}"),
            },
        }

        let raw = std::fs::read(&dest).unwrap();
        let cut_path = dir.path().join("cut.cb");
        std::fs::write(&cut_path, &raw[..raw.len() / 2]).unwrap();
        match store.read(&cut_path) {
            Ok(_) => panic!("cut envelope passes"),
            Err(error) => match error.cause() {
                ErrorCause::FilesystemError(_) => {
                    assert!(
                        error.to_string().contains(BUNDLE_MANIFEST),
                        "cut manifest names the manifest entry"
                    );
                }
                other => panic!("wrong cut cause: {other:?}"),
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
            Err(error) => panic!("manifest writes: {error}"),
        };
        let raw = std::fs::read(&dest).unwrap();
        assert!(!raw.is_empty(), "gzipped holds bytes");
        assert_eq!(
            &raw[0..2],
            &[0x1f, 0x8b],
            "gzipped envelope carries gzip magic"
        );
    }
}
