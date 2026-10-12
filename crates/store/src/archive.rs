//! Archive
//! Member listing and extraction for compressed archives.

pub mod error;
mod extract;
mod tar;
mod zip;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;

use crate::StoreRoots;
use crate::handles::{ArchiveHandle, ResourceHandle};
use crate::resources::Resources;
use crate::resources::error::ResourceError;
use confit_driver::{self as driver};
use error::{ArchiveError, Result};
use extract::{ExtractedMember, check_backend_names, check_member_path};
use tar::TarBackend;
use zip::ZipBackend;

/// Unpack folder name under the temp base.
const EXTRACT_DIR: &str = "extract";

/// Head window for every probe, wide enough for tar magic.
const HEAD_LEN: u64 = 512;

/// Archive backend.
trait ArchiveBackend {
    /// List member names.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for verified compression.
    fn names(&self, source: &Path) -> Result<Vec<String>>;

    /// Extract decoded members under pending with hashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for verified compression.
    fn unpack(&self, source: &Path, pending: &Path) -> Result<Vec<ExtractedMember>>;
}

/// File-backed archive member listing and extraction.
///
/// Members are archive-relative with forward slashes.
#[derive(Debug, Clone)]
pub struct ArchiveStore {
    temp_base: PathBuf,
    resources: Arc<Resources>,
    progress: ProgressSender,
}

impl ArchiveStore {
    /// Build a file-backed archive store under the temp base.
    pub fn new(roots: &StoreRoots, resources: Arc<Resources>, progress: ProgressSender) -> Self {
        Self {
            temp_base: roots.temp_base.clone(),
            resources,
            progress,
        }
    }

    /// Register one trusted source as a verified archive.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::NotArchive`] for non-archives.
    pub fn archive(&self, source: &ResourceHandle) -> Result<ArchiveHandle> {
        let path = source.canonical();
        check_compressed_source(path)?;
        ArchiveHandle::new(path.to_path_buf(), source.sha256().clone())
    }

    /// Register one file path as a verified archive.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::NotArchive`] for non-archive sources.
    pub fn register_archive(&self, source: &Path) -> Result<ArchiveHandle> {
        let sha = file_sha(source)?;
        check_compressed_source(source)?;
        ArchiveHandle::new(source.to_path_buf(), sha)
    }

    /// List member names.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for verified compression.
    pub fn members(&self, archive: &ArchiveHandle) -> Result<Vec<String>> {
        let source = archive.canonical();
        match sniff(source)? {
            Shape::Zip => ZipBackend.names(source),
            Shape::Plain => TarBackend.names(source),
            Shape::Tar => match TarBackend.names(source) {
                Err(_) if wants_tar(source) => Err(not_archive(source)),
                outcome => outcome,
            },
        }
    }

    /// Unpack one archive once into the extract folder.
    ///
    /// Members return as extracted resource handles hashed at extract time.
    ///
    /// - Present folders skip.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for verified compression.
    /// - [`ArchiveError::Write`] for extract write faults.
    /// - [`ArchiveError::WriteUnknown`] for other extract write failures.
    /// - [`ArchiveError::NotArchive`] for non-archives.
    pub fn extract(&self, archive: &ArchiveHandle) -> Result<Vec<ResourceHandle>> {
        let source = archive.canonical();
        let dest = self
            .temp_base
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        if driver::fs::is_dir(&dest) {
            return extracted_handles(&dest, source, &self.resources);
        }
        match sniff(source)? {
            Shape::Zip => {
                check_backend_names(source, &ZipBackend)?;
                self.run_backend(source, &dest, &ZipBackend)
            }
            Shape::Plain => {
                check_backend_names(source, &TarBackend)?;
                self.run_backend(source, &dest, &TarBackend)
            }
            Shape::Tar => {
                match TarBackend.names(source) {
                    Ok(names) => {
                        for name in &names {
                            check_member_path(name, source)?;
                        }
                    }
                    Err(_) if wants_tar(source) => {
                        return Err(not_archive(source));
                    }
                    Err(_) => {}
                }
                match self.run_backend(source, &dest, &TarBackend) {
                    Err(_) if wants_tar(source) => Err(not_archive(source)),
                    outcome => outcome,
                }
            }
        }
    }

    /// Unpack one backend through pending into place.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::Write`] for extract write faults.
    /// - [`ArchiveError::WriteUnknown`] for other extract write failures.
    fn run_backend(
        &self,
        source: &Path,
        dest: &Path,
        backend: &dyn ArchiveBackend,
    ) -> Result<Vec<ResourceHandle>> {
        let mut built: Option<Vec<ExtractedMember>> = None;
        let mut failure: Option<ArchiveError> = None;
        let mut pended: Option<PathBuf> = None;
        let outcome = driver::atomic_write(dest, |path| {
            pended = Some(path.to_path_buf());
            match extract_members(source, path, backend) {
                Ok(extracted) => built = Some(extracted),
                Err(error) => {
                    let _ = driver::fs::remove_dir_all(path);
                    failure = Some(error);
                    return Err(std::io::Error::other("archive extract failed"));
                }
            }
            Ok(())
        });
        match outcome {
            Ok(()) => {
                let handles =
                    streamed_handles(dest, built.unwrap_or_default().as_slice(), &self.resources)?;
                let total = handles.len();
                let _ = self.progress.send(Event::Unpacked {
                    archive: source.display().to_string(),
                    total,
                });
                Ok(handles)
            }
            Err(error) => {
                if let Some(path) = &pended {
                    let _ = driver::fs::remove_dir_all(path);
                }
                Err(failure.unwrap_or_else(|| ArchiveError::from_write_io(source, error)))
            }
        }
    }

    /// Pick one archive member by its archive-relative name.
    ///
    /// - Names are archive-relative with forward slashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::UnknownMember`] for unknown members.
    pub fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let members = self.extract(archive)?;
        let dest = self
            .temp_base
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        let wanted = dest.join(name);
        members
            .into_iter()
            .find(|member| member.canonical() == wanted)
            .ok_or_else(|| ArchiveError::UnknownMember {
                path: archive.canonical().to_path_buf(),
                name: name.to_owned(),
            })
    }
}

/// Extract one backend's members under a fresh pending folder.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed archive reads.
/// - [`ArchiveError::Write`] for extract write faults.
/// - [`ArchiveError::WriteUnknown`] for other extract write failures.
/// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
/// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
/// - [`ArchiveError::UnsupportedCompression`] for verified compression.
fn extract_members(
    source: &Path,
    pending: &Path,
    backend: &dyn ArchiveBackend,
) -> Result<Vec<ExtractedMember>> {
    driver::fs::create_dir_all(pending)
        .map_err(|error| ArchiveError::from_write_io(source, error))?;
    backend.unpack(source, pending)
}

/// Hash one source file with a stream.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn file_sha(source: &Path) -> Result<Sha> {
    let mut file =
        driver::fs::open(source).map_err(|error| ArchiveError::from_io(source, error))?;
    Sha::read(&mut file).map_err(|failure| ArchiveError::from_io(source, failure))
}

/// Prove one source holds a compressed archive.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] for plain sources.
/// - [`ArchiveError::Read`] for failed source reads.
fn check_compressed_source(source: &Path) -> Result<()> {
    match sniff(source)? {
        Shape::Zip => ZipBackend
            .names(source)
            .map(|_| ())
            .map_err(|_| not_archive(source)),
        Shape::Plain => Err(not_archive(source)),
        Shape::Tar => TarBackend
            .names(source)
            .map(|_| ())
            .map_err(|_| not_archive(source)),
    }
}

/// Build one non-archive error with the source path.
fn not_archive(source: &Path) -> ArchiveError {
    ArchiveError::NotArchive {
        path: source.to_path_buf(),
    }
}

/// Build extracted member handles from streamed hashes.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for containment failures.
fn streamed_handles(
    dest: &Path,
    members: &[ExtractedMember],
    resources: &Resources,
) -> Result<Vec<ResourceHandle>> {
    let mut handles = Vec::with_capacity(members.len());
    for member in members {
        handles.push(
            resources
                .cache(dest, &member.name, member.sha.clone())
                .map_err(from_resource)?,
        );
    }
    handles.sort_by(|left, right| left.canonical().cmp(right.canonical()));
    Ok(handles)
}

/// Sniffed archive shape.
enum Shape {
    /// Zip bytes under central-directory framing.
    Zip,
    /// Tar or gzip bytes under tar framing.
    Tar,
    /// Plain bytes for one tar attempt.
    Plain,
}

/// Sniff one source into its backend shape.
///
/// Zip magic routes zip, tar and gzip magic route tar, and plain bytes route
/// a plain tar attempt.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn sniff(source: &Path) -> Result<Shape> {
    let head = read_head(source)?;
    if driver::zip::valid(&head) {
        Ok(Shape::Zip)
    } else if driver::tar::handles(&head) {
        Ok(Shape::Tar)
    } else {
        Ok(Shape::Plain)
    }
}

/// Read the head window for every probe.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn read_head(source: &Path) -> Result<Vec<u8>> {
    let file = driver::fs::open(source).map_err(|error| ArchiveError::from_io(source, error))?;
    let mut head = Vec::new();
    std::io::Read::read_to_end(&mut file.take(HEAD_LEN), &mut head)
        .map_err(|error| ArchiveError::from_io(source, error))?;
    Ok(head)
}

/// Collect extracted handles back for one present extract folder.
///
/// - Names sort in ascending order.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed extract reads.
/// - [`ArchiveError::UnknownMember`] for missing members.
fn extracted_handles(
    dest: &Path,
    source: &Path,
    resources: &Resources,
) -> Result<Vec<ResourceHandle>> {
    let mut names = Vec::new();
    collect_extract_names(dest, dest, &mut names, source)?;
    names.sort();
    let mut handles = Vec::with_capacity(names.len());
    for name in &names {
        let path = dest.join(name);
        let sha = extract_file_sha(&path, source, name)?;
        handles.push(resources.cache(dest, name, sha).map_err(from_resource)?);
    }
    Ok(handles)
}

/// Hash one extracted member file with a stream.
///
/// # Errors
///
/// - [`ArchiveError::UnknownMember`] for missing members.
/// - [`ArchiveError::Read`] for failed member reads.
fn extract_file_sha(path: &Path, source: &Path, name: &str) -> Result<Sha> {
    let mut file =
        driver::fs::open(path).map_err(|error| extract_read_error(error, source, name))?;
    Sha::read(&mut file).map_err(|error| extract_read_error(error, source, name))
}

/// Collect archive-relative file names under one extract folder.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed extract listing.
fn collect_extract_names(
    dir: &Path,
    root: &Path,
    names: &mut Vec<String>,
    source: &Path,
) -> Result<()> {
    let entries =
        driver::fs::read_dir(dir).map_err(|error| ArchiveError::from_io(source, error))?;
    for path in entries {
        if driver::fs::is_dir(&path) {
            collect_extract_names(&path, root, names, source)?;
            continue;
        }
        if driver::fs::is_file(&path) {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ArchiveError::Read {
                    path: source.to_path_buf(),
                    fault: crate::faults::AccessFault::Unknown {
                        message: error.to_string(),
                    },
                })?;
            names.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Map one extracted member read failure into domain language.
fn extract_read_error(error: std::io::Error, source: &Path, name: &str) -> ArchiveError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ArchiveError::UnknownMember {
            path: source.to_path_buf(),
            name: name.to_owned(),
        },
        std::io::ErrorKind::PermissionDenied => ArchiveError::Read {
            path: source.to_path_buf(),
            fault: crate::faults::AccessFault::Denied,
        },
        _ => ArchiveError::from_io(source, error),
    }
}

/// Map one resource creation failure into archive language.
fn from_resource(error: ResourceError) -> ArchiveError {
    match error {
        ResourceError::Escape { path } => ArchiveError::Escape { path },
        ResourceError::Read { path, fault } => ArchiveError::Read { path, fault },
    }
}

/// Report whether a name wants tar members.
fn wants_tar(archive: &Path) -> bool {
    let lower = archive.as_os_str().to_string_lossy().to_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".tar")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    use confit_driver::fs::TestGuard;

    /// Deflate level feeding container fixtures.
    const LEVEL: u32 = 6;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            temp_base: dir.join("temp"),
            ..Default::default()
        }
    }

    fn test_store(dir: &Path) -> ArchiveStore {
        let roots = test_roots(dir);
        let resources = std::sync::Arc::new(Resources::new(&roots));
        let (sender, _) = crossbeam_channel::unbounded();
        ArchiveStore::new(&roots, resources, sender)
    }

    fn test_resources(dir: &Path) -> Resources {
        Resources::new(&test_roots(dir))
    }

    fn tar_gz_bytes(dir: &Path, members: &[(&str, &[u8])]) -> Vec<u8> {
        let dest = dir.join("scratch.tar.gz");
        let framed: Vec<driver::tar::BuildMember<'_>> = members
            .iter()
            .map(|(name, bytes)| driver::tar::BuildMember {
                name: (*name).to_owned(),
                len: bytes.len() as u64,
                reader: Box::new(std::io::Cursor::new(*bytes)),
                mode: 0o644,
            })
            .collect();
        let build = |path: &Path| {
            let sink = driver::fs::create(path)?;
            driver::tar::build_gzipped(sink, framed, LEVEL)
        };
        if let Err(error) = driver::atomic_write(&dest, build) {
            panic!("fixture builds: {error}");
        }
        driver::fs::read(&dest).unwrap()
    }

    fn evil_tar_gz_bytes() -> Vec<u8> {
        let data = b"x";
        let mut header = [0u8; 512];
        let name = b"../evil.txt";
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000644\0");
        let size = format!("{:07o}\0", data.len());
        header[124..124 + 8].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[262..264].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        let checksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        let mut raw = Vec::new();
        raw.extend_from_slice(&header);
        let mut padded = [0u8; 512];
        padded[..data.len()].copy_from_slice(data);
        raw.extend_from_slice(&padded);
        raw.extend_from_slice(&[0u8; 1024]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(&raw).unwrap();
        encoder.finish().unwrap()
    }

    fn write_archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            driver::fs::create_dir_all(parent).unwrap();
        }
        driver::fs::write(&path, bytes).unwrap();
        path
    }

    fn fixture_archive(
        store: &ArchiveStore,
        dir: &Path,
        name: &str,
        bytes: &[u8],
    ) -> ArchiveHandle {
        let path = write_archive(dir, name, bytes);
        let source = ResourceHandle::new(dir, path, Sha::hash(bytes)).unwrap();
        match store.archive(&source) {
            Ok(handle) => handle,
            Err(error) => panic!("source registers: {error}"),
        }
    }

    fn handle_sha(handles: &[ResourceHandle], name: &str) -> Sha {
        handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with(name))
            .unwrap()
            .sha256()
            .clone()
    }

    #[test]
    fn create_registers_compressed_source() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.tar.gz", &raw);
        let source = ResourceHandle::new(dir.path(), path.clone(), Sha::hash(&raw)).unwrap();
        match store.archive(&source) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha256(), &Sha::hash(&raw));
            }
            Err(error) => panic!("source registers: {error}"),
        }
    }

    #[test]
    fn create_rejects_plain_and_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        let source =
            ResourceHandle::new(dir.path(), path.clone(), Sha::hash(b"plain text")).unwrap();
        match store.archive(&source) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&path.display().to_string()),
                "error shows the source: {error}"
            ),
        }
        let missing = dir.path().join("absent.tar.gz");
        let absent =
            ResourceHandle::new(dir.path(), missing.clone(), Sha::hash(b"absent")).unwrap();
        match store.archive(&absent) {
            Ok(_) => panic!("absent source passes"),
            Err(error) => assert!(
                error.to_string().contains(&missing.display().to_string()),
                "error shows the source: {error}"
            ),
        }
    }

    #[test]
    fn members_lists_files_skipping_folders() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("members list: {error}"),
        }
    }

    #[test]
    fn extract_returns_extracted_handles_with_stable_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(
                dir.path(),
                &[
                    ("a.txt", b"same"),
                    ("b.txt", b"same"),
                    ("sub/c.txt", b"other"),
                ],
            ),
        );
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        let expected_extract = extract_base.join(archive.sha256().hex());
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        assert_eq!(handles.len(), 3);
        for handle in &handles {
            assert!(
                handle.canonical().starts_with(&expected_extract),
                "extract lands under temp extract plus handle sha: {}",
                handle.canonical().display()
            );
        }
        assert_eq!(handle_sha(&handles, "a.txt"), Sha::hash(b"same"));
        assert_eq!(handle_sha(&handles, "a.txt"), handle_sha(&handles, "b.txt"));
        assert_ne!(
            handle_sha(&handles, "a.txt"),
            handle_sha(&handles, "sub/c.txt")
        );
        assert_eq!(
            driver::fs::read(handles[0].canonical()).unwrap(),
            b"same".as_slice()
        );
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        let extracted = driver::fs::read_dir(&extract_base).unwrap();
        assert_eq!(extracted.len(), 1, "the extract leaves no stray pending");
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
    }

    #[test]
    fn reads_through_member_handle_naming_missing() {
        use crate::resources::error::ResourceError;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let picked = handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with("sub/b.txt"))
            .unwrap();
        match resources.open(picked) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"beta");
            }
            Err(error) => panic!("member opens: {error}"),
        }
        let extract_dir = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        let missing = match resources.cache(&extract_dir, "absent.txt", Sha::hash(b"absent")) {
            Ok(handle) => handle,
            Err(error) => panic!("absent creates: {error}"),
        };
        match resources.open(&missing) {
            Ok(_) => panic!("absent member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("absent.txt"),
                    "error shows the member: {text}"
                );
                assert!(
                    matches!(error, ResourceError::Read { .. }),
                    "absent reports loss: {text}"
                );
            }
        }
    }

    #[test]
    fn open_rejects_escaping_member() {
        use crate::resources::error::ResourceError;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let extract_dir = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        assert!(!handles.is_empty(), "extract creates handles");
        match resources.cache(&extract_dir, "../evil.txt", Sha::hash(b"x")) {
            Ok(_) => panic!("escaping member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("escapes"), "error reports escape: {text}");
                assert!(text.contains("evil.txt"), "error shows the member: {text}");
                assert!(
                    matches!(error, ResourceError::Escape { .. }),
                    "escape keeps variant: {text}"
                );
            }
        }
    }

    #[test]
    fn open_tracks_current_extract_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        match resources.open(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"alpha");
            }
            Err(error) => panic!("member opens from extract: {error}"),
        }
        driver::fs::write(handles[0].canonical(), b"patched").unwrap();
        match resources.open(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"patched");
            }
            Err(error) => panic!("member tracks extract: {error}"),
        }
    }

    #[test]
    fn extract_rejects_escaping_members() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "evil.tar.gz", &evil_tar_gz_bytes());
        match store.extract(&archive) {
            Ok(_) => panic!("escaping member passes"),
            Err(ArchiveError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        if driver::fs::exists(&extract_base) {
            let left = driver::fs::read_dir(&extract_base).unwrap().len();
            assert_eq!(left, 0);
        }
    }

    #[test]
    fn gzip_lists_and_opens_derived_member() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let archive = fixture_archive(&store, dir.path(), "note.gz", &encoder.finish().unwrap());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["note"]),
            Err(error) => panic!("gzip lists: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("gzip extracts: {error}"),
        };
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].sha256(), &Sha::hash(b"plain"));
        match resources.open(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"plain");
            }
            Err(error) => panic!("gzip opens: {error}"),
        }
    }

    #[test]
    fn members_rejects_missing_archive() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let missing = dir.path().join("absent.tar.gz");
        let handle = ArchiveHandle::new(&missing, Sha::hash(b"absent")).unwrap();
        match store.members(&handle) {
            Ok(_) => panic!("absent archive passes"),
            Err(error) => assert!(
                error.to_string().contains(&missing.display().to_string()),
                "error shows the archive: {error}"
            ),
        }
    }

    #[test]
    fn extract_keeps_one_extract_per_handle_sha() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]);
        let sha_a = Sha::hash(b"archive-a");
        let sha_b = Sha::hash(b"archive-b");
        assert_ne!(sha_a, sha_b);
        let path_a = write_archive(dir.path(), "a.tar.gz", &raw);
        let path_b = write_archive(dir.path(), "b.tar.gz", &raw);
        let first_source = ResourceHandle::new(dir.path(), path_a, sha_a.clone()).unwrap();
        let first = match store.archive(&first_source) {
            Ok(handle) => handle,
            Err(error) => panic!("first source registers: {error}"),
        };
        let second_source = ResourceHandle::new(dir.path(), path_b, sha_b.clone()).unwrap();
        let second = match store.archive(&second_source) {
            Ok(handle) => handle,
            Err(error) => panic!("second source registers: {error}"),
        };
        assert_eq!(first.sha256(), &sha_a);
        assert_eq!(second.sha256(), &sha_b);
        let first_handles = match store.extract(&first) {
            Ok(handles) => handles,
            Err(error) => panic!("first archive extracts: {error}"),
        };
        let second_handles = match store.extract(&second) {
            Ok(handles) => handles,
            Err(error) => panic!("second archive extracts: {error}"),
        };
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        assert!(
            first_handles[0]
                .canonical()
                .starts_with(extract_base.join(first.sha256().hex())),
            "first extract follows its handle: {}",
            first_handles[0].canonical().display()
        );
        assert!(
            second_handles[0]
                .canonical()
                .starts_with(extract_base.join(second.sha256().hex())),
            "second extract follows its handle: {}",
            second_handles[0].canonical().display()
        );
        assert_ne!(first_handles[0].canonical(), second_handles[0].canonical());
    }

    #[test]
    fn extract_and_open_streams_large_member() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8 * 1024, "payload spans many chunks");
        let archive = fixture_archive(
            &store,
            dir.path(),
            "big.tar.gz",
            &tar_gz_bytes(dir.path(), &[("big.bin", &raw)]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["big.bin"]),
            Err(error) => panic!("large members list: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("large archive extracts: {error}"),
        };
        assert_eq!(handles.len(), 1);
        match resources.open(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("large member opens: {error}"),
        }
    }

    fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        use ::zip::write::SimpleFileOptions;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ::zip::ZipWriter::new(cursor);
        for (name, bytes) in members {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default()
                        .compression_method(::zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn zip_bytes_with_dir() -> Vec<u8> {
        use ::zip::write::SimpleFileOptions;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ::zip::ZipWriter::new(cursor);
        writer
            .add_directory("sub/", SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file(
                "a.txt",
                SimpleFileOptions::default().compression_method(::zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"alpha").unwrap();
        writer
            .start_file(
                "sub/b.txt",
                SimpleFileOptions::default().compression_method(::zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"beta").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn evil_zip_bytes() -> Vec<u8> {
        use ::zip::write::SimpleFileOptions;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ::zip::ZipWriter::new(cursor);
        writer
            .start_file(
                "../evil.txt",
                SimpleFileOptions::default().compression_method(::zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"x").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn symlink_zip_bytes() -> Vec<u8> {
        use ::zip::write::SimpleFileOptions;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ::zip::ZipWriter::new(cursor);
        writer
            .add_symlink("link.txt", "a.txt", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_create_registers_and_rejects_plain() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = zip_bytes(&[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.zip", &raw);
        let source = ResourceHandle::new(dir.path(), path.clone(), Sha::hash(&raw)).unwrap();
        match store.archive(&source) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha256(), &Sha::hash(&raw));
            }
            Err(error) => panic!("zip source registers: {error}"),
        }
        let plain = write_archive(dir.path(), "note.txt", b"plain text");
        let plain_source =
            ResourceHandle::new(dir.path(), plain.clone(), Sha::hash(b"plain text")).unwrap();
        match store.archive(&plain_source) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&plain.display().to_string()),
                "error shows the source: {error}"
            ),
        }
    }

    #[test]
    fn zip_members_lists_files_skipping_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "fonts.zip", &zip_bytes_with_dir());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("zip members list: {error}"),
        }
    }

    #[test]
    fn zip_extract_unpacks_reuses_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.zip",
            &zip_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("zip archive extracts: {error}"),
        };
        assert_eq!(handles.len(), 2);
        assert_eq!(handle_sha(&handles, "a.txt"), Sha::hash(b"alpha"));
        assert_eq!(handle_sha(&handles, "sub/b.txt"), Sha::hash(b"beta"));
        let picked = handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with("sub/b.txt"))
            .unwrap();
        assert_eq!(driver::fs::read(picked.canonical()).unwrap(), b"beta");
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        let extracted = driver::fs::read_dir(&extract_base).unwrap();
        assert_eq!(extracted.len(), 1, "the extract leaves no stray pending");
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
        let evil = fixture_archive(&store, dir.path(), "evil.zip", &evil_zip_bytes());
        match store.extract(&evil) {
            Ok(_) => panic!("escaping member passes"),
            Err(ArchiveError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let evil_extract = extract_base.join(evil.sha256().hex());
        assert!(!driver::fs::exists(&evil_extract));
        let extracted = driver::fs::read_dir(&extract_base).unwrap();
        assert_eq!(extracted.len(), 1, "the failed unpack leaves no pending");
    }

    #[test]
    fn zip_symlink_skips_without_extract() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "link.zip", &symlink_zip_bytes());
        match store.members(&archive) {
            Ok(names) => assert!(names.is_empty(), "symlink skips: {names:?}"),
            Err(error) => panic!("symlink members list: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("symlink archive extracts: {error}"),
        };
        assert!(handles.is_empty());
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        let extract_dir = extract_base.join(archive.sha256().hex());
        if driver::fs::exists(&extract_dir) {
            let left = driver::fs::read_dir(&extract_dir).unwrap().len();
            assert_eq!(left, 0);
        }
        let extracted = driver::fs::read_dir(&extract_base).unwrap();
        assert_eq!(
            extracted.len(),
            1,
            "the empty extract leaves no stray pending"
        );
    }

    #[test]
    fn extract_reports_write_quota() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let store = test_store(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let raw = encoder.finish().unwrap();
        let archive = fixture_archive(&store, dir.path(), "note.gz", &raw);
        guard.fail_writes(std::io::ErrorKind::QuotaExceeded);
        match store.extract(&archive) {
            Ok(_) => panic!("capped disk passes"),
            Err(ArchiveError::Write { path, fault }) => {
                assert!(
                    matches!(fault, crate::faults::AccessFault::QuotaExceeded),
                    "extract keeps the fault"
                );
                assert_eq!(
                    path,
                    archive.canonical().to_path_buf(),
                    "extract keeps the source"
                );
            }
            Err(error) => panic!("wrong extract variant: {error}"),
        }
    }

    #[test]
    fn extract_announces_unpacked_with_counts() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let resources = std::sync::Arc::new(Resources::new(&roots));
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = ArchiveStore::new(&roots, resources, sender);
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("b.txt", b"beta")]),
        );
        match store.extract(&archive) {
            Ok(handles) => {
                assert_eq!(handles.len(), 2, "extract creates two members")
            }
            Err(error) => panic!("archive extracts: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(events.len(), 1, "unpack fires one fact");
        match &events[0] {
            Event::Unpacked { archive, total } => {
                assert_eq!(*total, 2, "unpack counts every member");
                assert!(
                    archive.ends_with("fonts.tar.gz"),
                    "unpack shows the archive: {archive}"
                );
            }
            other => panic!("unpack announces itself: {other:?}"),
        }
    }
}
