//! Bundle input.

use std::collections::BTreeSet;
use std::path::Path;

use confit_driver as driver;
use confit_driver::sha::Sha;
use confit_store::faults::AccessFault;
use confit_store::resources::error::ResourceError;

use super::{BUNDLE_MANIFEST, Bundler};
use crate::error::{Result, RuntimeError};
use crate::manifest::{CodecError, Manifest};

impl Bundler {
    /// Read one portable bundle into a sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable bundles.
    /// - [`RuntimeError::BundleVersion`] for unsupported versions.
    /// - [`RuntimeError::BundleCorrupt`] for corrupt bundles.
    pub fn read(&self, path: &Path) -> Result<Manifest> {
        Unpack::new(self, path).run()
    }
}

/// One bundle walk with its fail state.
struct Unpack<'a> {
    /// The bundler under reading.
    bundler: &'a Bundler,
    /// The bundle path under reading.
    bundle: &'a Path,
    /// The buffered manifest bytes.
    raw: Option<Vec<u8>>,
    /// Whether any fault logged.
    failed: bool,
    /// Shas that failed mid-walk, skipped at presence check.
    bad: BTreeSet<Sha>,
}

impl<'a> Unpack<'a> {
    /// Build one bundle walk.
    fn new(bundler: &'a Bundler, bundle: &'a Path) -> Self {
        Self {
            bundler,
            bundle,
            raw: None,
            failed: false,
            bad: BTreeSet::new(),
        }
    }

    /// Run one walk into the sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable bundles.
    /// - [`RuntimeError::BundleVersion`] for unsupported versions.
    /// - [`RuntimeError::BundleCorrupt`] for corrupt bundles.
    fn run(mut self) -> Result<Manifest> {
        let file = std::fs::File::open(self.bundle)
            .map_err(|error| RuntimeError::from_bundle_io(self.bundle, error))?;
        let bundle = self.bundle;
        driver::archive::uncompress(file, |entry| self.entry(entry))
            .map_err(|error| from_driver(bundle, error))?;
        self.finish()
    }

    /// Receive one walk entry into the manifest buffer or the cache.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleCorrupt`] for cut envelopes.
    /// - [`RuntimeError::BundleRead`] for failed contents.
    fn entry(&mut self, entry: driver::archive::Entry<'_>) -> Result<()> {
        if entry.name == BUNDLE_MANIFEST {
            return self.receive_manifest(entry.reader);
        }
        self.receive_content(entry.name, entry.reader)
    }

    /// Buffer one manifest entry body.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleCorrupt`] for cut envelopes.
    fn receive_manifest(&mut self, mut reader: impl std::io::Read) -> Result<()> {
        if self.raw.is_some() {
            log::error!(
                "bundle '{}' corrupt: duplicate manifest entry",
                self.bundle.display()
            );
            self.failed = true;
            return self.drain(reader);
        }
        let mut bytes = Vec::new();
        if reader.read_to_end(&mut bytes).is_err() {
            return Err(abort_envelope(self.bundle));
        }
        self.raw = Some(bytes);
        Ok(())
    }

    /// Land one content entry body in the cache under its name hash.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleCorrupt`] for cut envelopes.
    /// - [`RuntimeError::BundleRead`] for failed contents.
    fn receive_content(&mut self, name: String, reader: impl std::io::Read) -> Result<()> {
        let sha = match Sha::new(name.as_str()) {
            Ok(sha) => sha,
            Err(_) => {
                log::error!(
                    "bundle '{}' corrupt: entry '{name}' is not a content hash",
                    self.bundle.display()
                );
                self.failed = true;
                return self.drain(reader);
            }
        };
        match self.bundler.resources.store_checked(&sha, reader) {
            Ok(_) => Ok(()),
            Err(ResourceError::Mismatch { .. }) => {
                log::error!(
                    "bundle '{}' corrupt: content '{sha}' fails its name hash",
                    self.bundle.display()
                );
                self.bad.insert(sha);
                self.failed = true;
                Ok(())
            }
            Err(ResourceError::Read {
                fault: AccessFault::Unknown { .. },
                ..
            }) => {
                // entry body cut short mid-stream: stage faults carry mapped
                // kinds, so an unknown read here means a broken envelope
                Err(abort_envelope(self.bundle))
            }
            Err(error) => Err(from_resource(self.bundle, error)),
        }
    }

    /// Drain one unusable body to advance the walk.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleCorrupt`] for cut envelopes.
    fn drain(&mut self, mut reader: impl std::io::Read) -> Result<()> {
        if std::io::copy(&mut reader, &mut std::io::sink()).is_err() {
            return Err(abort_envelope(self.bundle));
        }
        Ok(())
    }

    /// Decode the buffered manifest with presence checks.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleVersion`] for unsupported versions.
    /// - [`RuntimeError::BundleCorrupt`] for corrupt bundles.
    fn finish(mut self) -> Result<Manifest> {
        let manifest = match self.raw {
            None => {
                log::error!(
                    "bundle '{}' corrupt: manifest entry is missing",
                    self.bundle.display()
                );
                None
            }
            Some(bytes) => match Manifest::decode(&bytes) {
                Ok(manifest) => Some(manifest),
                Err(error) => return Err(from_codec(self.bundle, error)),
            },
        };
        if let Some(found) = manifest.as_ref() {
            let mut shas: BTreeSet<Sha> = BTreeSet::new();
            for handle in found.handles() {
                shas.insert(handle.sha().clone());
            }
            for sha in &shas {
                if self.bad.contains(sha) {
                    continue;
                }
                if !self.bundler.resources.cache_path(sha).exists() {
                    log::error!(
                        "bundle '{}' corrupt: content '{sha}' is missing from the cache",
                        self.bundle.display()
                    );
                    self.failed = true;
                }
            }
        }
        match manifest {
            Some(manifest) if !self.failed => Ok(manifest),
            _ => Err(RuntimeError::BundleCorrupt {
                path: self.bundle.to_path_buf(),
            }),
        }
    }
}

/// Abort one cut-short walk as corrupt.
fn abort_envelope(bundle: &Path) -> RuntimeError {
    log::error!("bundle '{}' corrupt: envelope is invalid", bundle.display());
    RuntimeError::BundleCorrupt {
        path: bundle.to_path_buf(),
    }
}

/// Map one driver archive failure at the bundle path into domain language.
fn from_driver(bundle: &Path, error: driver::archive::Error<RuntimeError>) -> RuntimeError {
    use driver::archive::Error as Driver;

    match error {
        Driver::Visit(error) => error,
        Driver::Foreign
        | Driver::Bare
        | Driver::Corrupt(_)
        | Driver::Truncated(_)
        | Driver::Invalid(_)
        | Driver::Locked => abort_envelope(bundle),
        Driver::Missing(inner) | Driver::Denied(inner) | Driver::Io(inner) => {
            RuntimeError::from_bundle_io(bundle, inner)
        }
        Driver::Unknown { detail } | Driver::UnknownAlgorithm { detail } => {
            RuntimeError::from_bundle_io(
                bundle,
                std::io::Error::new(std::io::ErrorKind::Unsupported, detail),
            )
        }
    }
}

/// Map one manifest codec failure at the bundle path into domain language.
fn from_codec(bundle: &Path, error: CodecError) -> RuntimeError {
    match error {
        CodecError::Version { got } => RuntimeError::BundleVersion {
            path: bundle.to_path_buf(),
            got,
        },
        CodecError::Corrupt { .. } => {
            log::error!(
                "bundle '{}' corrupt: manifest entry is invalid",
                bundle.display()
            );
            RuntimeError::BundleCorrupt {
                path: bundle.to_path_buf(),
            }
        }
    }
}

/// Map one resource failure at the bundle path into domain language.
fn from_resource(bundle: &Path, error: ResourceError) -> RuntimeError {
    match error {
        ResourceError::Read { fault, .. } => RuntimeError::BundleRead {
            path: bundle.to_path_buf(),
            fault,
        },
        ResourceError::Mismatch {
            expected, found, ..
        } => RuntimeError::BundleRead {
            path: bundle.to_path_buf(),
            fault: AccessFault::Corrupt { expected, found },
        },
        other => RuntimeError::BundleRead {
            path: bundle.to_path_buf(),
            fault: AccessFault::Unknown {
                message: other.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    handle: ResourceHandle::Project(path, confit_driver::sha::Sha::hash(body)),
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
                builder.member(name, 0o644, bytes.len() as u64, &mut reader)?;
            }
            builder.finish()?;
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
            Err(RuntimeError::BundleVersion { got, .. }) => {
                assert_eq!(got, 0, "stale keeps version")
            }
            Err(error) => panic!("wrong stale variant: {error}"),
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
                        confit_driver::sha::Sha::hash(b"wanted bytes"),
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
            Err(RuntimeError::BundleCorrupt { path }) => {
                assert_eq!(path, thin, "corrupt keeps the bundle");
            }
            Err(error) => panic!("wrong missing variant: {error}"),
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
            Err(RuntimeError::BundleCorrupt { path }) => {
                assert_eq!(path, dup_path, "corrupt keeps the bundle");
            }
            Err(error) => panic!("wrong duplicate variant: {error}"),
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
            Err(RuntimeError::BundleCorrupt { .. }) => {}
            Err(error) => panic!("wrong bad id variant: {error}"),
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
            Err(RuntimeError::BundleCorrupt { .. }) => {}
            Err(error) => panic!("wrong unexpected variant: {error}"),
        }
    }

    #[test]
    fn read_faults_report_once_as_corrupt() {
        use confit_driver::sha::Sha;

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
            Err(RuntimeError::BundleCorrupt { .. }) => {}
            Err(error) => panic!("wrong mixed variant: {error}"),
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
            Err(RuntimeError::BundleCorrupt { .. }) => {}
            Err(error) => panic!("wrong drift variant: {error}"),
        }
    }

    #[test]
    fn read_rejects_broken_envelope_as_corrupt() {
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
            Err(RuntimeError::BundleCorrupt { path }) => {
                assert_eq!(path, foreign_path, "corrupt keeps the bundle");
            }
            Err(error) => panic!("wrong foreign variant: {error}"),
        }

        let raw = std::fs::read(&dest).unwrap();
        let cut_path = dir.path().join("cut.cb");
        std::fs::write(&cut_path, &raw[..raw.len() / 2]).unwrap();
        match store.read(&cut_path) {
            Ok(_) => panic!("cut envelope passes"),
            Err(RuntimeError::BundleCorrupt { path }) => {
                assert_eq!(path, cut_path, "corrupt keeps the bundle");
            }
            Err(error) => panic!("wrong cut variant: {error}"),
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
