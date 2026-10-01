//! Archive
//!
//! Member listing and extraction for compressed archives.

pub mod error;
mod spill;
mod tar;
mod zip;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_model::sha::Sha;

use crate::StoreRoots;
use crate::handles::{ArchiveHandle, ResourceHandle, TrustedHandle};
use crate::resources::Resources;
use crate::resources::error::ResourceError;
use confit_driver::{self as driver};
use error::{ArchiveError, Result};
use spill::{BornMember, check_backend_names, check_member_path};
use tar::TarBackend;
use zip::ZipBackend;

/// Unpack folder name under the temp base.
const EXTRACT_DIR: &str = "extract";

/// Head window feeding every probe, wide enough for tar magic.
const HEAD_LEN: u64 = 512;

/// Streaming backend behind one sniffed archive shape.
///
/// Names list archive-relative members with forward
/// slashes. Unpack spills decoded members under staging
/// with per-entry hashes.
trait ArchiveBackend {
    /// Lists member names without reading content.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
    fn names(&self, source: &Path) -> Result<Vec<String>>;

    /// Spills decoded members under staging with hashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>>;
}

/// File-backed archive member listing and extraction.
///
/// Members read archive-relative with forward slashes.
#[derive(Debug, Clone)]
pub struct ArchiveStore {
    temp_base: PathBuf,
    resources: Arc<Resources>,
}

impl ArchiveStore {
    /// Builds a file-backed archive store under the temp base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots, resources: Arc<Resources>) -> Self {
        Self {
            temp_base: roots.temp_base.clone(),
            resources,
        }
    }

    /// Seals one trusted source as a verified archive.
    ///
    /// The proof seals at birth; downstream code trusts the type.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::NotArchive`] for non-archives.
    pub fn archive(&self, source: &dyn TrustedHandle) -> Result<ArchiveHandle> {
        let path = source.canonical();
        check_compressed_source(path)?;
        ArchiveHandle::new(path.to_path_buf(), source.sha().clone()).map_err(|_| not_archive(path))
    }

    /// Seals one file path as a verified archive.
    ///
    /// The proof seals at birth and downstream code trusts the type.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed source reads.
    /// - [`ArchiveError::NotArchive`] for non-archive sources.
    pub fn seal(&self, source: &Path) -> Result<ArchiveHandle> {
        let sha = file_sha(source)?;
        check_compressed_source(source)?;
        ArchiveHandle::new(source.to_path_buf(), sha).map_err(|_| not_archive(source))
    }

    /// Lists member names without reading content.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
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

    /// Unpacks one archive once into the spill folder.
    ///
    /// Present folders skip, so repeated unpacks share bytes.
    /// Members return as born resource handles hashed at spill time.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
    /// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
    /// - [`ArchiveError::Write`] for spill write faults.
    /// - [`ArchiveError::WriteUnknown`] for other spill write failures.
    /// - [`ArchiveError::NotArchive`] for non-archives.
    pub fn extract(&self, archive: &ArchiveHandle) -> Result<Vec<ResourceHandle>> {
        let source = archive.canonical();
        let dest = self.temp_base.join(EXTRACT_DIR).join(archive.sha().hex());
        if driver::fs::metadata(&dest).is_ok_and(|facts| facts.is_dir()) {
            return spilled_handles(&dest, source, &self.resources);
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

    /// Unpacks one backend spill through staging into place.
    ///
    /// Members spill one entry at a time under staging before
    /// the atomic rename, and a present destination wins the
    /// rename race.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for failed archive reads.
    /// - [`ArchiveError::Write`] for spill write faults.
    /// - [`ArchiveError::WriteUnknown`] for other spill write failures.
    fn run_backend(
        &self,
        source: &Path,
        dest: &Path,
        backend: &dyn ArchiveBackend,
    ) -> Result<Vec<ResourceHandle>> {
        let mut built: Option<Vec<BornMember>> = None;
        let mut failure: Option<ArchiveError> = None;
        let mut staged: Option<PathBuf> = None;
        let outcome = driver::atomic_write(dest, |path| {
            staged = Some(path.to_path_buf());
            match spill_members(source, path, backend) {
                Ok(born) => built = Some(born),
                Err(error) => {
                    let _ = driver::fs::remove_dir_all(path);
                    failure = Some(error);
                    return Err(std::io::Error::other("archive spill failed"));
                }
            }
            Ok(())
        });
        match outcome {
            Ok(()) => streamed_handles(dest, built.unwrap_or_default().as_slice(), &self.resources),
            Err(error) => {
                if let Some(path) = &staged {
                    let _ = driver::fs::remove_dir_all(path);
                }
                Err(failure.unwrap_or_else(|| ArchiveError::from_write_io(source, error)))
            }
        }
    }

    /// Picks one archive member by its archive-relative name.
    ///
    /// Names read archive-relative with forward slashes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::UnknownMember`] for unknown members.
    pub fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let members = self.extract(archive)?;
        let dest = self.temp_base.join(EXTRACT_DIR).join(archive.sha().hex());
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

/// Spills one backend's members under a fresh staging folder.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed archive reads.
/// - [`ArchiveError::Write`] for spill write faults.
/// - [`ArchiveError::WriteUnknown`] for other spill write failures.
/// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
/// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
/// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
fn spill_members(
    source: &Path,
    staging: &Path,
    backend: &dyn ArchiveBackend,
) -> Result<Vec<BornMember>> {
    driver::fs::create_dir_all(staging)
        .map_err(|error| ArchiveError::from_write_io(source, error))?;
    backend.unpack(source, staging)
}

/// Hashes one source file with a stream.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn file_sha(source: &Path) -> Result<Sha> {
    let mut file =
        driver::fs::open(source).map_err(|error| ArchiveError::from_io(source, error))?;
    Sha::read(&mut file).map_err(|failure| ArchiveError::from_io(source, failure))
}

/// Proves one source holds a compressed archive.
///
/// Gzip magic, tar readability, zip magic, and zip
/// readability under current fallback rules.
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

/// Builds one non-archive error holding the source path.
fn not_archive(source: &Path) -> ArchiveError {
    ArchiveError::NotArchive {
        path: source.to_path_buf(),
    }
}

/// Builds born member handles from streamed hashes.
///
/// The spill folder roots containment for every member.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for containment failures.
fn streamed_handles(
    dest: &Path,
    members: &[BornMember],
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

/// Sniffed archive shape behind one source.
enum Shape {
    /// Zip bytes under central-directory framing.
    Zip,
    /// Tar or gzip bytes under tar framing.
    Tar,
    /// Plain bytes attempting tar and failing loud.
    Plain,
}

/// Sniffs one source into its backend shape.
///
/// Fallback order reads zip, then tar, with name
/// policy last: zip magic routes zip, tar or gzip
/// magic routes tar, plain bytes route a plain tar
/// attempt while tar names refuse bare singles.
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

/// Reads the head window feeding every probe.
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

/// Reads born handles back for one present spill.
///
/// Names sort, so repeats share order with fresh unpacks.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed spill reads.
/// - [`ArchiveError::UnknownMember`] for missing members.
fn spilled_handles(
    dest: &Path,
    source: &Path,
    resources: &Resources,
) -> Result<Vec<ResourceHandle>> {
    let mut names = Vec::new();
    collect_spill_names(dest, dest, &mut names, source)?;
    names.sort();
    let mut handles = Vec::with_capacity(names.len());
    for name in &names {
        let path = dest.join(name);
        let sha = spill_file_sha(&path, source, name)?;
        handles.push(resources.cache(dest, name, sha).map_err(from_resource)?);
    }
    Ok(handles)
}

/// Hashes one spilled member file with a stream.
///
/// # Errors
///
/// - [`ArchiveError::UnknownMember`] for missing members.
/// - [`ArchiveError::Read`] for failed member reads.
fn spill_file_sha(path: &Path, source: &Path, name: &str) -> Result<Sha> {
    let mut file = driver::fs::open(path).map_err(|error| spill_read_error(error, source, name))?;
    Sha::read(&mut file).map_err(|error| spill_read_error(error, source, name))
}

/// Collects archive-relative member names under one spill folder.
///
/// Folders skip; files read archive-relative with forward slashes.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed spill listing.
fn collect_spill_names(
    dir: &Path,
    root: &Path,
    names: &mut Vec<String>,
    source: &Path,
) -> Result<()> {
    let entries =
        driver::fs::read_dir(dir).map_err(|error| ArchiveError::from_io(source, error))?;
    for path in entries {
        if driver::fs::metadata(&path)
            .map(|facts| facts.is_dir())
            .unwrap_or(false)
        {
            collect_spill_names(&path, root, names, source)?;
            continue;
        }
        if driver::fs::metadata(&path)
            .map(|facts| facts.is_file())
            .unwrap_or(false)
        {
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

/// Maps one spilled member read failure into domain language.
fn spill_read_error(error: std::io::Error, source: &Path, name: &str) -> ArchiveError {
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

/// Maps one resource birth failure into archive language.
fn from_resource(error: ResourceError) -> ArchiveError {
    match error {
        ResourceError::Escape { path } => ArchiveError::Escape { path },
        ResourceError::Read { path, fault } => ArchiveError::Read { path, fault },
    }
}

/// Reports true while a name wants tar members.
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
        ArchiveStore::new(&roots, resources)
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

    struct TestSource(PathBuf, Sha);

    impl TrustedHandle for TestSource {
        fn canonical(&self) -> &Path {
            &self.0
        }

        fn sha(&self) -> &Sha {
            &self.1
        }
    }

    fn born_archive(store: &ArchiveStore, dir: &Path, name: &str, bytes: &[u8]) -> ArchiveHandle {
        let path = write_archive(dir, name, bytes);
        match store.archive(&TestSource(path, Sha::hash(bytes))) {
            Ok(handle) => handle,
            Err(error) => panic!("source seals: {error}"),
        }
    }

    fn handle_sha(handles: &[ResourceHandle], name: &str) -> Sha {
        handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with(name))
            .unwrap()
            .sha()
            .clone()
    }

    #[test]
    fn birth_seals_proof_for_compressed_source() {
        use crate::handles::ArchiveProof;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.tar.gz", &raw);
        match store.archive(&TestSource(path.clone(), Sha::hash(&raw))) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha(), &Sha::hash(&raw));
                assert_eq!(handle.proof(), ArchiveProof::Compressed);
            }
            Err(error) => panic!("source seals: {error}"),
        }
    }

    #[test]
    fn birth_rejects_plain_and_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        match store.archive(&TestSource(path.clone(), Sha::hash(b"plain text"))) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&path.display().to_string()),
                "error names the source: {error}"
            ),
        }
        let missing = dir.path().join("absent.tar.gz");
        match store.archive(&TestSource(missing.clone(), Sha::hash(b"absent"))) {
            Ok(_) => panic!("absent source passes"),
            Err(error) => assert!(
                error.to_string().contains(&missing.display().to_string()),
                "error names the source: {error}"
            ),
        }
    }

    #[test]
    fn members_lists_files_skipping_folders() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = born_archive(
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
    fn extract_returns_born_handles_with_stable_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = born_archive(
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
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        let expected_spill = spill_base.join(archive.sha().hex());
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        assert_eq!(handles.len(), 3);
        for handle in &handles {
            assert!(
                handle.canonical().starts_with(&expected_spill),
                "spill lands under temp extract plus handle sha: {}",
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
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        let spills = driver::fs::read_dir(&spill_base).unwrap();
        assert_eq!(spills.len(), 1, "the spill leaves no stray staging");
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
        let archive = born_archive(
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
        let spill = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha().hex());
        let missing = match resources.cache(&spill, "absent.txt", Sha::hash(b"absent")) {
            Ok(handle) => handle,
            Err(error) => panic!("absent births: {error}"),
        };
        match resources.open(&missing) {
            Ok(_) => panic!("absent member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("absent.txt"),
                    "error names the member: {text}"
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
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let spill = dir
            .path()
            .join("temp")
            .join(EXTRACT_DIR)
            .join(archive.sha().hex());
        assert!(!handles.is_empty(), "spill births handles");
        match resources.cache(&spill, "../evil.txt", Sha::hash(b"x")) {
            Ok(_) => panic!("escaping member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("escapes"), "error reports escape: {text}");
                assert!(text.contains("evil.txt"), "error names the member: {text}");
                assert!(
                    matches!(error, ResourceError::Escape { .. }),
                    "escape keeps variant: {text}"
                );
            }
        }
    }

    #[test]
    fn open_tracks_live_spill_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = born_archive(
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
            Err(error) => panic!("member opens from spill: {error}"),
        }
        driver::fs::write(handles[0].canonical(), b"patched").unwrap();
        match resources.open(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"patched");
            }
            Err(error) => panic!("member tracks spill: {error}"),
        }
    }

    #[test]
    fn extract_rejects_escaping_members() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = born_archive(&store, dir.path(), "evil.tar.gz", &evil_tar_gz_bytes());
        match store.extract(&archive) {
            Ok(_) => panic!("escaping member passes"),
            Err(ArchiveError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        if driver::fs::exists(&spill_base) {
            let left = driver::fs::read_dir(&spill_base).unwrap().len();
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
        let archive = born_archive(&store, dir.path(), "note.gz", &encoder.finish().unwrap());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["note"]),
            Err(error) => panic!("gzip lists: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("gzip extracts: {error}"),
        };
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].sha(), &Sha::hash(b"plain"));
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
                "error names the archive: {error}"
            ),
        }
    }

    #[test]
    fn extract_keeps_one_spill_per_handle_sha() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(dir.path(), &[("a.txt", b"alpha")]);
        let sha_a = Sha::hash(b"archive-a");
        let sha_b = Sha::hash(b"archive-b");
        assert_ne!(sha_a, sha_b);
        let path_a = write_archive(dir.path(), "a.tar.gz", &raw);
        let path_b = write_archive(dir.path(), "b.tar.gz", &raw);
        let first = match store.archive(&TestSource(path_a, sha_a.clone())) {
            Ok(handle) => handle,
            Err(error) => panic!("first source seals: {error}"),
        };
        let second = match store.archive(&TestSource(path_b, sha_b.clone())) {
            Ok(handle) => handle,
            Err(error) => panic!("second source seals: {error}"),
        };
        assert_eq!(first.sha(), &sha_a);
        assert_eq!(second.sha(), &sha_b);
        let first_handles = match store.extract(&first) {
            Ok(handles) => handles,
            Err(error) => panic!("first archive extracts: {error}"),
        };
        let second_handles = match store.extract(&second) {
            Ok(handles) => handles,
            Err(error) => panic!("second archive extracts: {error}"),
        };
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        assert!(
            first_handles[0]
                .canonical()
                .starts_with(spill_base.join(first.sha().hex())),
            "first spill follows its handle: {}",
            first_handles[0].canonical().display()
        );
        assert!(
            second_handles[0]
                .canonical()
                .starts_with(spill_base.join(second.sha().hex())),
            "second spill follows its handle: {}",
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
        let archive = born_archive(
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
    fn zip_birth_seals_and_rejects_plain() {
        use crate::handles::ArchiveProof;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = zip_bytes(&[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.zip", &raw);
        match store.archive(&TestSource(path.clone(), Sha::hash(&raw))) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha(), &Sha::hash(&raw));
                assert_eq!(handle.proof(), ArchiveProof::Compressed);
            }
            Err(error) => panic!("zip source seals: {error}"),
        }
        let plain = write_archive(dir.path(), "note.txt", b"plain text");
        match store.archive(&TestSource(plain.clone(), Sha::hash(b"plain text"))) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&plain.display().to_string()),
                "error names the source: {error}"
            ),
        }
    }

    #[test]
    fn zip_members_lists_files_skipping_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = born_archive(&store, dir.path(), "fonts.zip", &zip_bytes_with_dir());
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
        let archive = born_archive(
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
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        let spills = driver::fs::read_dir(&spill_base).unwrap();
        assert_eq!(spills.len(), 1, "the spill leaves no stray staging");
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
        let evil = born_archive(&store, dir.path(), "evil.zip", &evil_zip_bytes());
        match store.extract(&evil) {
            Ok(_) => panic!("escaping member passes"),
            Err(ArchiveError::Escape { .. }) => {}
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let evil_spill = spill_base.join(evil.sha().hex());
        assert!(!driver::fs::exists(&evil_spill));
        let spills = driver::fs::read_dir(&spill_base).unwrap();
        assert_eq!(spills.len(), 1, "the failed unpack leaves no staging");
    }

    #[test]
    fn zip_symlink_skips_without_spill() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let archive = born_archive(&store, dir.path(), "link.zip", &symlink_zip_bytes());
        match store.members(&archive) {
            Ok(names) => assert!(names.is_empty(), "symlink skips: {names:?}"),
            Err(error) => panic!("symlink members list: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("symlink archive extracts: {error}"),
        };
        assert!(handles.is_empty());
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        let spill = spill_base.join(archive.sha().hex());
        if driver::fs::exists(&spill) {
            let left = driver::fs::read_dir(&spill).unwrap().len();
            assert_eq!(left, 0);
        }
        let spills = driver::fs::read_dir(&spill_base).unwrap();
        assert_eq!(spills.len(), 1, "the empty spill leaves no stray staging");
    }

    #[test]
    fn extract_reports_write_quota() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let store = test_store(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let raw = encoder.finish().unwrap();
        let archive = born_archive(&store, dir.path(), "note.gz", &raw);
        guard.fail_writes(std::io::ErrorKind::QuotaExceeded);
        match store.extract(&archive) {
            Ok(_) => panic!("capped disk passes"),
            Err(ArchiveError::Write { path, fault }) => {
                assert!(
                    matches!(fault, crate::faults::AccessFault::QuotaExceeded),
                    "spill keeps the fault"
                );
                assert_eq!(
                    path,
                    archive.canonical().to_path_buf(),
                    "spill keeps the source"
                );
            }
            Err(error) => panic!("wrong spill variant: {error}"),
        }
    }
}
