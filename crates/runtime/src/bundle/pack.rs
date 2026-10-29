//! Bundle output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    ContextualCause, ContextualResult, Terminal, TerminalResult, WithContext,
};
use confit_core::sha::Sha;
use confit_driver as driver;
use confit_store::handles::ResourceHandle;

use super::BUNDLE_MANIFEST;
use super::Bundler;
use super::ensure_bundle_extension;
use crate::manifest::Manifest;

/// Entry mode for bundle members.
const BUNDLE_ENTRY_MODE: u32 = 0o644;

impl Bundler {
    /// Write one portable bundle with the manifest and the contents.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for render faults, unreadable contents,
    ///   and write faults with the bundle path slot.
    pub fn write(&self, manifest: &Manifest, dest: &Path) -> TerminalResult<PathBuf> {
        let dest = ensure_bundle_extension(dest);
        Pack::new(self, manifest, &dest)
            .and_then(|pack| pack.run())
            .with_bundle_path(dest)
            .terminal(ErrorKind::WriteBundle)
    }
}

/// One bundle build.
struct Pack<'a> {
    /// The bundler under writing.
    bundler: &'a Bundler,
    /// The ensured bundle destination.
    dest: PathBuf,
    /// The rendered manifest text.
    text: String,
    /// Unique handles in content-hash order.
    contents: Vec<ResourceHandle>,
    /// Opened content files.
    staged: Vec<Staged>,
}

/// One staged content file.
struct Staged {
    /// The handle naming the entry with its carried sha.
    handle: ResourceHandle,
    /// The opened content bytes.
    file: std::fs::File,
}

impl<'a> Pack<'a> {
    /// Build one bundle write with deduped contents.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] for manifest render failures.
    fn new(bundler: &'a Bundler, manifest: &Manifest, dest: &Path) -> ContextualResult<Self> {
        let dest = dest.to_path_buf();
        let text = manifest.json()?;
        let mut seen: BTreeMap<Sha, ResourceHandle> = BTreeMap::new();
        for handle in manifest.handles() {
            seen.entry(handle.sha().clone())
                .or_insert_with(|| handle.clone());
        }
        Ok(Self {
            bundler,
            dest,
            text,
            contents: seen.into_values().collect(),
            staged: Vec::new(),
        })
    }

    /// Run one bundle write into the ensured destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable contents and write faults.
    /// - [`confit_core::error::ErrorCause::ContentHashMismatch`] for contents failing the carried sha.
    fn run(mut self) -> ContextualResult<PathBuf> {
        self.collect()?;
        self.build()
    }

    /// Open every unique content outside the atomic write.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable contents with the attempted path.
    /// - [`confit_core::error::ErrorCause::ContentHashMismatch`] for contents failing the carried sha.
    fn collect(&mut self) -> ContextualResult<()> {
        for handle in &self.contents {
            let file = match handle {
                ResourceHandle::Cache(_) => self.bundler.resources.open(handle)?,
                ResourceHandle::Project(path, _) => std::fs::File::open(path)
                    .map_err(ContextualCause::from)
                    .with_fs_path(path)?,
            };
            self.staged.push(Staged {
                handle: handle.clone(),
                file,
            });
        }
        Ok(())
    }

    /// Write the staged bundle into its destination file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for write faults.
    /// - [`confit_core::error::ArchiveError`] for failed members.
    fn build(self) -> ContextualResult<PathBuf> {
        use confit_driver::archive::{Algorithm, Builder, Level};

        let Pack {
            bundler: _,
            dest,
            text,
            contents: _,
            mut staged,
        } = self;
        let scratch = driver::StagedFile::stage(&dest)?;
        {
            let sink = std::fs::File::create(scratch.path())?;
            let mut builder = Builder::create(sink, Algorithm::TarGz, Level::default());
            let mut manifest_reader = std::io::Cursor::new(text.as_bytes());
            builder.member(
                BUNDLE_MANIFEST,
                BUNDLE_ENTRY_MODE,
                text.len() as u64,
                &mut manifest_reader,
            )?;
            for entry in &mut staged {
                let len = entry.file.metadata()?.len();
                builder.member(
                    &entry.handle.sha().hex(),
                    BUNDLE_ENTRY_MODE,
                    len,
                    &mut entry.file,
                )?;
            }
            let sink = builder.finish()?;
            drop(sink);
        }
        scratch.persist(&dest)?;
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    use confit_core::error::ErrorCause;
    use confit_driver as driver;
    use confit_model::document::{Data, Document, OpaqueDocument};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;
    use confit_store::handles::ResourceHandle;

    use crate::prepare::build;

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
                    handle: ResourceHandle::Project(path, Sha::hash(body)),
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

    fn entry_bodies(path: &Path) -> Vec<Vec<u8>> {
        use std::io::Read as _;

        let file = std::fs::File::open(path).unwrap();
        let mut bodies = Vec::new();
        driver::archive::uncompress(file, |entry| -> ContextualResult<()> {
            let name = entry.name;
            let mut body = entry.reader;
            if name == BUNDLE_MANIFEST {
                std::io::copy(&mut body, &mut std::io::sink()).unwrap();
                return Ok(());
            }
            let mut found = Vec::new();
            body.read_to_end(&mut found).unwrap();
            bodies.push(found);
            Ok(())
        })
        .unwrap();
        bodies
    }

    #[test]
    fn write_registers_finished_file_with_streamed_contents() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"leading bytes", b"streamed body"]);
        let plan = dir.path().join("plan");
        let dest = match store.write(&manifest, &plan) {
            Ok(dest) => dest,
            Err(error) => panic!("manifest writes: {error}"),
        };
        assert_eq!(
            dest,
            dir.path().join("plan.cb").as_path(),
            "write points at the destination"
        );
        let raw = std::fs::read(&dest).unwrap();
        assert!(!raw.is_empty(), "write leaves bytes");
        let mut bodies = entry_bodies(&dest);
        bodies.sort();
        let mut expected = Vec::new();
        for document in &manifest.documents {
            let Data::Opaque(found) = &document.data else {
                continue;
            };
            let mut reader = match store.resources.open(&found.handle) {
                Ok(reader) => reader,
                Err(error) => panic!("content opens: {error}"),
            };
            let mut body = Vec::new();
            reader.read_to_end(&mut body).unwrap();
            expected.push(body);
        }
        expected.sort();
        assert_eq!(bodies, expected, "streamed contents stay intact");
    }

    #[test]
    fn write_reports_write_denied() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"written bytes"]);
        let locked = dir.path().join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        driver::fs::set_mode(&locked, Some(0o555)).unwrap();
        let dest = locked.join("plan");
        match store.write(&manifest, &dest) {
            Ok(_) => panic!("denied disk passes"),
            Err(error) => match error.cause() {
                ErrorCause::FilesystemError(_) | ErrorCause::ArchiveError(_) => {
                    assert!(
                        error
                            .to_string()
                            .contains(&locked.join("plan.cb").display().to_string()),
                        "write keeps the destination"
                    );
                }
                other => panic!("wrong write cause: {other:?}"),
            },
        }
        driver::fs::set_mode(&locked, Some(0o755)).unwrap();
    }
}
