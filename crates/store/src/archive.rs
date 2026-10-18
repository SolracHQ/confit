//! Archive
//! Member listing and extraction for compressed archives.

pub mod error;
mod extract;
mod hashes;
mod tar;
mod zip;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;

use crate::StoreRoots;
use crate::faults::AccessFault;
use crate::handles::{ArchiveHandle, ResourceHandle};
use crate::resources::Resources;
use crate::resources::error::ResourceError;
use confit_driver as driver;
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
    /// - [`ArchiveError::Read`] for unreadable sources.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for verified compression.
    fn names(&self, source: &Path) -> Result<Vec<String>>;

    /// Extract decoded members under pending with hashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable sources.
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
    /// - [`ArchiveError::Read`] for unreadable sources.
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
    /// - [`ArchiveError::Read`] for unreadable archives.
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
    /// Members return as extracted resource handles.
    ///
    /// - Present folders skip.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable archives.
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
        if dest.is_dir() {
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
    /// - [`ArchiveError::Read`] for unreadable archives.
    /// - [`ArchiveError::Write`] for extract write faults.
    /// - [`ArchiveError::WriteUnknown`] for other extract write failures.
    fn run_backend(
        &self,
        source: &Path,
        dest: &Path,
        backend: &dyn ArchiveBackend,
    ) -> Result<Vec<ResourceHandle>> {
        let staged = driver::StagedDir::stage(dest)
            .map_err(|error| ArchiveError::from_write_io(source, error))?;
        let members = extract_members(source, staged.path(), backend)?;
        staged
            .persist(dest)
            .map_err(|error| ArchiveError::from_write_io(source, error))?;
        let handles = streamed_handles(dest, members.as_slice(), &self.resources)?;
        hashes::record(dest, &member_shas(&members));
        let total = handles.len();
        let _ = self.progress.send(Event::Unpacked {
            archive: source.display().to_string(),
            total,
        });
        Ok(handles)
    }

    /// Pick one archive member by its archive-relative name.
    ///
    /// - Names are archive-relative with forward slashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::UnknownMember`] for unknown members.
    pub fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let source = archive.canonical();
        let dest = self
            .temp_base
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        if dest.is_dir() {
            let wanted = dest.join(name);
            if wanted.is_file() {
                if let Some(sha) = hashes::fresh(&dest, name) {
                    return self
                        .resources
                        .cache(&dest, name, sha)
                        .map_err(from_resource);
                }
                let sha = extract_file_sha(&wanted, source, name)?;
                hashes::note(&dest, name, &sha);
                return self
                    .resources
                    .cache(&dest, name, sha)
                    .map_err(from_resource);
            }
        }
        let members = self.extract(archive)?;
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
/// - [`ArchiveError::Read`] for unreadable archives.
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
    std::fs::create_dir_all(pending).map_err(|error| ArchiveError::from_write_io(source, error))?;
    backend.unpack(source, pending)
}

/// Hash one source file.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for unreadable sources.
fn file_sha(source: &Path) -> Result<Sha> {
    let mut file =
        std::fs::File::open(source).map_err(|error| ArchiveError::from_io(source, error))?;
    Sha::read(&mut file).map_err(|failure| ArchiveError::from_io(source, failure))
}

/// Prove one source is a compressed archive.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] for plain sources.
/// - [`ArchiveError::Read`] for unreadable sources.
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

/// Build extracted member handles from member hashes.
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

/// Collect member hashes keyed by archive-relative name.
fn member_shas(members: &[ExtractedMember]) -> std::collections::BTreeMap<String, Sha> {
    members
        .iter()
        .map(|member| (member.name.clone(), member.sha.clone()))
        .collect()
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
/// - [`ArchiveError::Read`] for unreadable sources.
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
/// - [`ArchiveError::Read`] for unreadable sources.
fn read_head(source: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(source).map_err(|error| ArchiveError::from_io(source, error))?;
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
/// - [`ArchiveError::Read`] for unreadable extracted files.
/// - [`ArchiveError::UnknownMember`] for missing members.
fn extracted_handles(
    dest: &Path,
    source: &Path,
    resources: &Resources,
) -> Result<Vec<ResourceHandle>> {
    let mut names = Vec::new();
    collect_extract_names(dest, dest, &mut names, source)?;
    names.sort();
    if let Some(handles) = fresh_handles(dest, &names, resources)? {
        return Ok(handles);
    }
    let mut pairs = std::collections::BTreeMap::new();
    let mut handles = Vec::with_capacity(names.len());
    for name in &names {
        let path = dest.join(name);
        let sha = extract_file_sha(&path, source, name)?;
        handles.push(
            resources
                .cache(dest, name, sha.clone())
                .map_err(from_resource)?,
        );
        pairs.insert(name.clone(), sha);
    }
    hashes::record(dest, &pairs);
    Ok(handles)
}

/// Build handles from recorded hashes.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for unreadable extracted files.
fn fresh_handles(
    dest: &Path,
    names: &[String],
    resources: &Resources,
) -> Result<Option<Vec<ResourceHandle>>> {
    let mut handles = Vec::with_capacity(names.len());
    for name in names {
        match hashes::fresh(dest, name) {
            Some(sha) => {
                handles.push(resources.cache(dest, name, sha).map_err(from_resource)?);
            }
            None => return Ok(None),
        }
    }
    Ok(Some(handles))
}

/// Hash one extracted member file.
///
/// # Errors
///
/// - [`ArchiveError::UnknownMember`] for missing members.
/// - [`ArchiveError::Read`] for unreadable members.
fn extract_file_sha(path: &Path, source: &Path, name: &str) -> Result<Sha> {
    let mut file =
        std::fs::File::open(path).map_err(|error| extract_read_error(error, source, name))?;
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
        if path.is_dir() {
            collect_extract_names(&path, root, names, source)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ArchiveError::Read {
                    path: source.to_path_buf(),
                    fault: AccessFault::Unknown {
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
            fault: AccessFault::Denied,
        },
        _ => ArchiveError::from_io(source, error),
    }
}

/// Map one resource creation failure into archive language.
fn from_resource(error: ResourceError) -> ArchiveError {
    match error {
        ResourceError::Escape { path, .. } => ArchiveError::Escape { path },
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
        let build = |path: &Path| {
            let sink = std::fs::File::create(path)?;
            let mut builder = driver::tar::TarBuilder::create_gzipped(sink, LEVEL);
            for (name, bytes) in members {
                let mut reader = std::io::Cursor::new(*bytes);
                builder.member(name, bytes.len() as u64, 0o644, &mut reader)?;
            }
            builder.finish_gzipped()?;
            Ok(())
        };
        if let Err(error) = driver::atomic_write(&dest, build) {
            panic!("fixture builds: {error}");
        }
        std::fs::read(&dest).unwrap()
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
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, bytes).unwrap();
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

    fn assert_no_pending(extract_base: &Path) {
        for entry in driver::fs::read_dir(extract_base).unwrap() {
            assert!(
                !entry.to_string_lossy().contains(".part"),
                "the extract leaves no stray pending: {}",
                entry.display()
            );
        }
    }

    #[test]
    fn create_registers_compressed_source() {
        let dir = tempfile::tempdir().unwrap();
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
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        let source =
            ResourceHandle::new(dir.path(), path.clone(), Sha::hash(b"plain text")).unwrap();
        match store.archive(&source) {
            Ok(_) => panic!("plain source passes"),
            Err(ArchiveError::NotArchive { .. }) => {}
            Err(error) => panic!("wrong plain variant: {error}"),
        }
        let missing = dir.path().join("absent.tar.gz");
        let absent =
            ResourceHandle::new(dir.path(), missing.clone(), Sha::hash(b"absent")).unwrap();
        match store.archive(&absent) {
            Ok(_) => panic!("absent source passes"),
            Err(ArchiveError::Read { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn members_lists_files_skipping_folders() {
        let dir = tempfile::tempdir().unwrap();
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
                "extract lands under temp extract and handle sha: {}",
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
            std::fs::read(handles[0].canonical()).unwrap(),
            b"same".as_slice()
        );
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        assert_no_pending(&extract_base);
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
    }

    #[test]
    fn handle_opens_present_and_missing_members() {
        use crate::resources::error::ResourceError;

        let dir = tempfile::tempdir().unwrap();
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
            Err(ResourceError::Read { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn open_rejects_escaping_member() {
        use crate::resources::error::ResourceError;

        let dir = tempfile::tempdir().unwrap();
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
            Err(ResourceError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
    }

    #[test]
    fn open_tracks_current_extract_bytes() {
        let dir = tempfile::tempdir().unwrap();
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
        std::fs::write(handles[0].canonical(), b"patched").unwrap();
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
    fn extract_member_resolves_present_rejects_absent() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.extract(&archive) {
            Ok(_) => {}
            Err(error) => panic!("archive extracts: {error}"),
        }
        let extract_dir = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex());
        std::fs::remove_file(extract_dir.join("sub").join("b.txt")).unwrap();
        match store.extract_member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(handle.sha256(), &Sha::hash(b"alpha")),
            Err(error) => panic!("present member resolves: {error}"),
        }
        match store.extract_member(&archive, "absent.txt") {
            Ok(_) => panic!("absent member passes"),
            Err(ArchiveError::UnknownMember { name, .. }) => assert_eq!(name, "absent.txt"),
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    fn extract_manifest(dir: &Path, archive: &ArchiveHandle) -> PathBuf {
        dir.join("temp")
            .join(EXTRACT_DIR)
            .join(format!("{}.json", archive.sha256().hex()))
    }

    #[test]
    fn unpack_records_member_hashes_beside_extract() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.extract(&archive) {
            Ok(_) => {}
            Err(error) => panic!("archive extracts: {error}"),
        }
        let text = match std::fs::read_to_string(extract_manifest(dir.path(), &archive)) {
            Ok(text) => text,
            Err(error) => panic!("manifest load: {error}"),
        };
        assert!(
            text.contains(&Sha::hash(b"alpha").hex()),
            "manifest records hashes: {text}"
        );
        assert!(text.contains("sub/b.txt"), "manifest records names: {text}");
    }

    #[test]
    fn touched_members_rehash_and_refresh_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]),
        );
        match store.extract_member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(handle.sha256(), &Sha::hash(b"alpha")),
            Err(error) => panic!("member resolves: {error}"),
        }
        let member = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha256().hex())
            .join("a.txt");
        std::fs::write(&member, b"omega").unwrap();
        match store.extract_member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(handle.sha256(), &Sha::hash(b"omega")),
            Err(error) => panic!("touched member rehashes: {error}"),
        }
        let text = match std::fs::read_to_string(extract_manifest(dir.path(), &archive)) {
            Ok(text) => text,
            Err(error) => panic!("manifest load: {error}"),
        };
        assert!(
            text.contains(&Sha::hash(b"omega").hex()),
            "records refresh on touch: {text}"
        );
    }

    #[test]
    fn bad_manifests_fall_back_to_rehash() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]),
        );
        match store.extract(&archive) {
            Ok(_) => {}
            Err(error) => panic!("archive extracts: {error}"),
        }
        std::fs::write(extract_manifest(dir.path(), &archive), b"{bad").unwrap();
        match store.extract_member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(handle.sha256(), &Sha::hash(b"alpha")),
            Err(error) => panic!("bad manifest still resolves: {error}"),
        }
    }

    #[test]
    fn extract_rejects_escaping_members() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "evil.tar.gz", &evil_tar_gz_bytes());
        match store.extract(&archive) {
            Ok(_) => panic!("escaping member passes"),
            Err(ArchiveError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        if extract_base.exists() {
            let left = driver::fs::read_dir(&extract_base).unwrap().len();
            assert_eq!(left, 0);
        }
    }

    #[test]
    fn gzip_lists_and_opens_derived_member() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
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
        let store = test_store(dir.path());
        let missing = dir.path().join("absent.tar.gz");
        let handle = ArchiveHandle::new(&missing, Sha::hash(b"absent")).unwrap();
        match store.members(&handle) {
            Ok(_) => panic!("absent archive passes"),
            Err(ArchiveError::Read { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn extract_keeps_one_extract_per_handle_sha() {
        let dir = tempfile::tempdir().unwrap();
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
            Err(ArchiveError::NotArchive { .. }) => {}
            Err(error) => panic!("wrong plain variant: {error}"),
        }
    }

    #[test]
    fn zip_members_lists_files_skipping_dirs() {
        let dir = tempfile::tempdir().unwrap();
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
        assert_eq!(std::fs::read(picked.canonical()).unwrap(), b"beta");
        let extract_base = dir.path().join("temp").join(EXTRACT_DIR);
        assert_no_pending(&extract_base);
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
        assert!(!evil_extract.exists());
        assert_no_pending(&extract_base);
    }

    #[test]
    fn zip_symlink_skips_without_extract() {
        let dir = tempfile::tempdir().unwrap();
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
        if extract_dir.exists() {
            let left = driver::fs::read_dir(&extract_dir).unwrap().len();
            assert_eq!(left, 0);
        }
        assert_no_pending(&extract_base);
    }

    #[test]
    fn extract_maps_write_quota() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let raw = encoder.finish().unwrap();
        let archive = fixture_archive(&store, dir.path(), "note.gz", &raw);
        let source = archive.canonical().to_path_buf();
        let error = std::io::Error::new(std::io::ErrorKind::QuotaExceeded, "capped disk");
        match ArchiveError::from_write_io(&source, error) {
            ArchiveError::Write { path, fault } => {
                assert!(
                    matches!(fault, AccessFault::QuotaExceeded),
                    "mapping keeps the fault"
                );
                assert_eq!(path, source, "mapping keeps the source");
            }
            error => panic!("wrong mapping variant: {error}"),
        }
    }

    #[test]
    fn extract_announces_unpacked_with_counts() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
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
