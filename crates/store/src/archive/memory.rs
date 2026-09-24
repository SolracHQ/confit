//! Memory
//!
//! Memory archive store for tests.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::handles::{ArchiveHandle, ResourceHandle, Sha, TrustedHandle};

use super::ArchiveStore;

/// Fake spill folder name for memory unpacks.
const SPILL_DIR: &str = "memory-extract";

/// Fallback mode for memory spills holding no mode bits.
const DEFAULT_MEMBER_MODE: u32 = 0o644;

/// Memory archive store for tests.
///
/// Members ride spills keyed by source hash.
#[derive(Debug, Default)]
pub struct MemoryArchiveStore {
    spills: Mutex<HashMap<PathBuf, HashMap<String, Vec<u8>>>>,
}

/// Archive member with raw bytes.
struct SeedMember {
    name: String,
    content: Vec<u8>,
}

impl MemoryArchiveStore {
    /// Builds an empty memory archive store.
    pub fn new() -> Self {
        Self {
            spills: Mutex::new(HashMap::new()),
        }
    }
}

impl ArchiveStore for MemoryArchiveStore {
    fn archive(&self, source: &dyn TrustedHandle) -> Result<ArchiveHandle> {
        let path = source.canonical();
        let bytes = read_source_bytes(path)?;
        check_compressed(&bytes, path)?;
        ArchiveHandle::new(path.to_path_buf(), source.sha().clone())
    }

    fn members(&self, archive: &ArchiveHandle) -> Result<Vec<String>> {
        let source = archive.canonical();
        let bytes = read_source_bytes(source)?;
        let members = parse_members(&bytes, source)?;
        Ok(members.into_iter().map(|member| member.name).collect())
    }

    fn extract(&self, archive: &ArchiveHandle) -> Result<Vec<ResourceHandle>> {
        let source = archive.canonical();
        let spill = spill_path(archive);
        let seeded = match self.spills.lock() {
            Ok(guard) => guard.get(&spill).cloned(),
            Err(poisoned) => poisoned.into_inner().get(&spill).cloned(),
        };
        if let Some(members) = seeded {
            return born_handles(&spill, &members);
        }
        let bytes = read_source_bytes(source)?;
        let parsed = parse_members(&bytes, source)?;
        for member in &parsed {
            check_member_name(&member.name, source)?;
        }
        let staged: HashMap<String, Vec<u8>> = parsed
            .into_iter()
            .map(|member| (member.name, member.content))
            .collect();
        match self.spills.lock() {
            Ok(mut guard) => guard.insert(spill.clone(), staged.clone()),
            Err(poisoned) => poisoned.into_inner().insert(spill.clone(), staged.clone()),
        };
        born_handles(&spill, &staged)
    }

    fn open_decompressed(&self, member: &ResourceHandle) -> Result<Box<dyn std::io::Read>> {
        check_member_handle(member)?;
        let path = member.canonical();
        let guard = match self.spills.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (spill, members) in guard.iter() {
            let Ok(relative) = path.strip_prefix(spill) else {
                continue;
            };
            let name = relative.to_string_lossy().replace('\\', "/");
            if let Some(bytes) = members.get(&name) {
                return Ok(Box::new(std::io::Cursor::new(bytes.clone())) as Box<dyn std::io::Read>);
            }
        }
        Err(missing_member(path))
    }

    fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let members = self.extract(archive)?;
        let spill = spill_path(archive);
        let wanted = spill.join(name);
        members
            .into_iter()
            .find(|member| member.canonical() == wanted)
            .ok_or_else(|| {
                Error::Plan(format!(
                    "cannot unpack '{}': unknown member '{name}'",
                    archive.canonical().display()
                ))
            })
    }

    fn mode(&self, _member: &ResourceHandle) -> Result<u32> {
        Ok(DEFAULT_MEMBER_MODE)
    }
}

/// Builds born member handles hashed at spill time.
///
/// The spill folder roots containment for every member.
///
/// # Errors
///
/// Containment failures surface as plan errors.
fn born_handles(spill: &Path, members: &HashMap<String, Vec<u8>>) -> Result<Vec<ResourceHandle>> {
    let mut handles = Vec::with_capacity(members.len());
    for (name, bytes) in members {
        handles.push(ResourceHandle::new(
            spill,
            spill.join(name),
            Sha::hash(bytes),
        )?);
    }
    handles.sort_by(|left, right| left.canonical().cmp(right.canonical()));
    Ok(handles)
}

/// Fake spill folder for one archive.
fn spill_path(archive: &ArchiveHandle) -> PathBuf {
    PathBuf::from(SPILL_DIR).join(archive.sha().hex())
}

/// Reads raw source bytes from disk.
///
/// # Errors
///
/// Missing and unreadable files fail as plan errors naming
/// the source.
fn read_source_bytes(source: &Path) -> Result<Vec<u8>> {
    std::fs::read(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))
}

/// Proves one source holds a compressed archive.
///
/// Gzip magic plus tar readability under current fallback rules.
///
/// # Errors
///
/// Plain and undecodable sources fail as plan errors naming
/// the source.
fn check_compressed(bytes: &[u8], source: &Path) -> Result<()> {
    if !is_gzip(bytes) {
        return Err(not_archive(source));
    }
    match parse_members(bytes, source) {
        Ok(_) => Ok(()),
        Err(_) => Err(not_archive(source)),
    }
}

/// Builds one non-archive error naming the source.
fn not_archive(source: &Path) -> Error {
    Error::Plan(format!(
        "cannot archive '{}': not a compressed archive",
        source.display()
    ))
}

/// Reads members with content through transparent decompression.
///
/// Single-file gzip archives read one derived member.
///
/// # Errors
///
/// Decoder failures and tar-shaped names over non-tar bytes
/// fail as plan errors naming the source.
fn parse_members(bytes: &[u8], source: &Path) -> Result<Vec<SeedMember>> {
    if !is_gzip(bytes) {
        return tar_members(bytes, source);
    }
    let flat = gunzip(bytes, source)?;
    match tar_members(&flat, source) {
        Ok(members) => Ok(members),
        Err(_) if wants_tar(source) => Err(Error::Plan(format!(
            "cannot unpack '{}': not a tar archive",
            source.display()
        ))),
        Err(_) => Ok(vec![SeedMember {
            name: single_name(source),
            content: flat,
        }]),
    }
}

/// Reads file members with content for tar bytes.
///
/// # Errors
///
/// Malformed archives fail as plan errors naming the source.
fn tar_members(bytes: &[u8], source: &Path) -> Result<Vec<SeedMember>> {
    let mut reader = tar::Archive::new(bytes);
    let entries = reader
        .entries()
        .map_err(|error| unpack_failure(source, error))?;
    let mut members = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| unpack_failure(source, error))?;
        let Some(name) = entry_name(&entry, source)? else {
            continue;
        };
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|error| unpack_failure(source, error))?;
        members.push(SeedMember { name, content });
    }
    Ok(members)
}

/// Reads the file name for one tar entry.
///
/// Folders, non-files, and empty names skip as absent.
///
/// # Errors
///
/// Undecodable entry paths fail as plan errors naming the
/// source.
fn entry_name<R: std::io::Read>(
    entry: &tar::Entry<'_, R>,
    source: &Path,
) -> Result<Option<String>> {
    let kind = entry.header().entry_type();
    if kind.is_dir() || !kind.is_file() {
        return Ok(None);
    }
    let name = entry
        .path()
        .map_err(|error| unpack_failure(source, error))?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

/// Builds one unpack plan error naming the source.
fn unpack_failure(source: &Path, error: impl std::fmt::Display) -> Error {
    Error::Plan(format!("cannot unpack '{}': {error}", source.display()))
}

/// Builds one missing member error naming the member path.
fn missing_member(member: &Path) -> Error {
    Error::Plan(format!(
        "cannot unpack '{}': missing member",
        member.display()
    ))
}

/// Rejects member paths escaping the unpack folder.
///
/// # Errors
///
/// Empty, absolute, and dot-dot members fail as plan errors.
fn check_member_name(name: &str, source: &Path) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Plan(format!(
            "cannot unpack '{}': empty member path",
            source.display()
        )));
    }
    if Path::new(name).is_absolute() {
        return Err(Error::Plan(format!(
            "cannot unpack '{}': member '{name}' escapes",
            source.display()
        )));
    }
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(Error::Plan(format!(
                "cannot unpack '{}': member '{name}' escapes",
                source.display()
            )));
        }
    }
    Ok(())
}

/// Rejects member paths escaping the unpack folder.
///
/// # Errors
///
/// Dot-dot members fail as plan errors naming the member.
fn check_member_handle(member: &ResourceHandle) -> Result<()> {
    let escapes = member
        .canonical()
        .components()
        .any(|segment| matches!(segment, std::path::Component::ParentDir));
    if escapes {
        return Err(Error::Plan(format!(
            "cannot unpack '{}': member escapes",
            member.canonical().display()
        )));
    }
    Ok(())
}

/// Reports true for gzip magic bytes.
fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

/// Reports true while a name wants tar members.
fn wants_tar(source: &Path) -> bool {
    let lower = source.as_os_str().to_string_lossy().to_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".tar")
}

/// Derives the single-file member name from a source path.
fn single_name(source: &Path) -> String {
    let base = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    if base.to_lowercase().ends_with(".gz") && base.len() > 3 {
        base[..base.len() - 3].to_string()
    } else if base.is_empty() {
        "file".to_string()
    } else {
        base
    }
}

/// Decompresses one gzip body.
///
/// # Errors
///
/// Decoder failures surface as plan errors naming the source.
fn gunzip(bytes: &[u8], source: &Path) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut flat = Vec::new();
    decoder
        .read_to_end(&mut flat)
        .map_err(|error| unpack_failure(source, error))?;
    Ok(flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSource(PathBuf, Sha);

    impl TrustedHandle for TestSource {
        fn canonical(&self) -> &Path {
            &self.0
        }

        fn sha(&self) -> &Sha {
            &self.1
        }
    }

    fn tar_gz_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        let mut builder = tar::Builder::new(encoder);
        for (name, bytes) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn evil_tar_gz_bytes() -> Vec<u8> {
        use std::io::Write as _;

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

    fn write_source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn born_archive(
        store: &MemoryArchiveStore,
        dir: &Path,
        name: &str,
        bytes: &[u8],
    ) -> ArchiveHandle {
        let path = write_source(dir, name, bytes);
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
        use confit_core::handles::ArchiveProof;

        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let raw = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let path = write_source(dir.path(), "fonts.tar.gz", &raw);
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
    fn birth_rejects_plain_source_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let path = write_source(dir.path(), "note.txt", b"plain text");
        match store.archive(&TestSource(path.clone(), Sha::hash(b"plain text"))) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&path.display().to_string()),
                "error names the source: {error}"
            ),
        }
    }

    #[test]
    fn members_lists_names_without_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("members list: {error}"),
        }
    }

    #[test]
    fn extract_returns_born_handles_with_stable_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[
                ("a.txt", b"same"),
                ("b.txt", b"same"),
                ("sub/c.txt", b"other"),
            ]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        assert_eq!(handles.len(), 3);
        let expected_spill = PathBuf::from(SPILL_DIR).join(archive.sha().hex());
        for handle in &handles {
            assert!(
                handle.canonical().starts_with(&expected_spill),
                "spill lands under handle sha: {}",
                handle.canonical().display()
            );
        }
        assert_eq!(handle_sha(&handles, "a.txt"), Sha::hash(b"same"));
        assert_eq!(handle_sha(&handles, "a.txt"), handle_sha(&handles, "b.txt"));
        assert_ne!(
            handle_sha(&handles, "a.txt"),
            handle_sha(&handles, "sub/c.txt")
        );
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
    }

    #[test]
    fn open_reads_through_member_handle() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let picked = handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with("sub/b.txt"))
            .unwrap();
        match store.open_decompressed(picked) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"beta");
            }
            Err(error) => panic!("member opens: {error}"),
        }
    }

    #[test]
    fn open_names_missing_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let root = handles[0].canonical().parent().unwrap().to_path_buf();
        let missing =
            ResourceHandle::new(&root, root.join("absent.txt"), Sha::hash(b"absent")).unwrap();
        match store.open_decompressed(&missing) {
            Ok(_) => panic!("absent member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("absent.txt"),
                    "error names the member: {text}"
                );
            }
        }
    }

    #[test]
    fn open_rejects_escaping_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha")]),
        );
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("archive extracts: {error}"),
        };
        let root = handles[0].canonical().parent().unwrap().to_path_buf();
        let evil = ResourceHandle::new(&root, root.join("../evil.txt"), Sha::hash(b"x")).unwrap();
        match store.open_decompressed(&evil) {
            Ok(_) => panic!("escaping member passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("escapes"), "error reports escape: {text}");
                assert!(text.contains("evil.txt"), "error names the member: {text}");
            }
        }
    }

    #[test]
    fn extract_rejects_escaping_members() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let archive = born_archive(&store, dir.path(), "evil.tar.gz", &evil_tar_gz_bytes());
        let expected = format!(
            "cannot unpack '{}': member '../evil.txt' escapes",
            archive.canonical().display()
        );
        match store.extract(&archive) {
            Ok(_) => panic!("escaping member passes"),
            Err(error) => assert!(
                error.to_string().contains(&expected),
                "error names source plus member: {error}"
            ),
        }
    }

    #[test]
    fn single_gzip_lists_and_opens_derived_member() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let archive = born_archive(&store, dir.path(), "note.gz", &encoder.finish().unwrap());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["note"]),
            Err(error) => panic!("single gzip lists: {error}"),
        }
        let handles = match store.extract(&archive) {
            Ok(handles) => handles,
            Err(error) => panic!("single gzip extracts: {error}"),
        };
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].sha(), &Sha::hash(b"plain"));
        match store.open_decompressed(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"plain");
            }
            Err(error) => panic!("single gzip opens: {error}"),
        }
    }

    #[test]
    fn extract_keeps_one_spill_per_handle_sha() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let raw = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let sha_a = Sha::hash(b"archive-a");
        let sha_b = Sha::hash(b"archive-b");
        assert_ne!(sha_a, sha_b);
        let path_a = write_source(dir.path(), "a.tar.gz", &raw);
        let path_b = write_source(dir.path(), "b.tar.gz", &raw);
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
        let expected_a = PathBuf::from(SPILL_DIR).join(first.sha().hex());
        let expected_b = PathBuf::from(SPILL_DIR).join(second.sha().hex());
        assert!(
            first_handles[0].canonical().starts_with(&expected_a),
            "first spill follows its handle: {}",
            first_handles[0].canonical().display()
        );
        assert!(
            second_handles[0].canonical().starts_with(&expected_b),
            "second spill follows its handle: {}",
            second_handles[0].canonical().display()
        );
        assert_ne!(first_handles[0].canonical(), second_handles[0].canonical());
    }

    #[test]
    fn extract_and_open_streams_large_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryArchiveStore::new();
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8192);
        let archive = born_archive(
            &store,
            dir.path(),
            "big.tar.gz",
            &tar_gz_bytes(&[("big.bin", &raw)]),
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
        match store.open_decompressed(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("large member opens: {error}"),
        }
    }
}
