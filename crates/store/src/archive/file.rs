//! File
//!
//! File-backed archive reads.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::handles::{ArchiveHandle, ResourceHandle, TrustedHandle};
use sha2::Digest as _;

use super::ArchiveStore;
use crate::StoreRoots;

/// Staging suffix for atomic archive unpacks.
const STAGING_SUFFIX: &str = ".part";

/// Unpack folder name under the temp base.
const EXTRACT_DIR: &str = "extract";

/// Copy chunk size for member streaming.
const ENTRY_CHUNK: usize = 8192;

/// Fallback mode for members without distinct bits.
const DEFAULT_MEMBER_MODE: u32 = 0o644;

/// Archive member with a streamed content hash.
struct BornMember {
    name: String,
    sha: String,
}

/// File-backed archive member listing and extraction.
///
/// Members read archive-relative with forward slashes.
#[derive(Debug, Clone)]
pub struct FileArchiveStore {
    temp_base: PathBuf,
}

impl FileArchiveStore {
    /// Builds a file-backed archive store under the temp base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            temp_base: roots.temp_base.clone(),
        }
    }
}

impl ArchiveStore for FileArchiveStore {
    fn archive(&self, source: &dyn TrustedHandle) -> Result<ArchiveHandle> {
        let path = source.canonical();
        check_compressed_source(path)?;
        ArchiveHandle::new(path.to_path_buf(), source.sha())
    }

    fn members(&self, archive: &ArchiveHandle) -> Result<Vec<String>> {
        let source = archive.canonical();
        if !peek_is_gzip(source)? {
            let file = std::fs::File::open(source).map_err(|error| {
                Error::Plan(format!("cannot read '{}': {error}", source.display()))
            })?;
            return stream_names(file, source);
        }
        let file = std::fs::File::open(source)
            .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
        match stream_names(flate2::read::GzDecoder::new(file), source) {
            Ok(names) => Ok(names),
            Err(_) if wants_tar(source) => Err(Error::Plan(format!(
                "cannot unpack '{}': not a tar archive",
                source.display()
            ))),
            Err(_) => Ok(vec![single_name(source)]),
        }
    }

    fn extract(&self, archive: &ArchiveHandle) -> Result<Vec<ResourceHandle>> {
        let source = archive.canonical();
        let dest = self.temp_base.join(EXTRACT_DIR).join(archive.sha());
        if dest.is_dir() {
            return spilled_handles(&dest, source);
        }
        let gzipped = peek_is_gzip(source)?;
        check_unpack_names(source, gzipped)?;
        let staging = staging_path(&dest);
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|error| unpack_failure(source, error))?;
        }
        std::fs::create_dir_all(&staging).map_err(|error| unpack_failure(source, error))?;
        if !gzipped {
            let born = match unpack_tar_file(source, &staging) {
                Ok(born) => born,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(error);
                }
            };
            return finish_unpack(source, &dest, &staging, &born);
        }
        match unpack_gzipped_tar(source, &staging) {
            Ok(born) => finish_unpack(source, &dest, &staging, &born),
            Err(_) if wants_tar(source) => {
                let _ = std::fs::remove_dir_all(&staging);
                Err(Error::Plan(format!(
                    "cannot unpack '{}': not a tar archive",
                    source.display()
                )))
            }
            Err(_) => {
                let _ = std::fs::remove_dir_all(&staging);
                std::fs::create_dir_all(&staging).map_err(|error| unpack_failure(source, error))?;
                let born = unpack_single_entry(source, &staging)?;
                finish_unpack(source, &dest, &staging, &born)
            }
        }
    }

    fn open_decompressed(&self, member: &ResourceHandle) -> Result<Box<dyn std::io::Read>> {
        check_member_handle(member)?;
        let path = member.canonical();
        let file = std::fs::File::open(path).map_err(|_| missing_member(path))?;
        Ok(Box::new(file) as Box<dyn std::io::Read>)
    }

    fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let members = self.extract(archive)?;
        let dest = self.temp_base.join(EXTRACT_DIR).join(archive.sha());
        let wanted = dest.join(name);
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

    fn mode(&self, member: &ResourceHandle) -> Result<u32> {
        use std::os::unix::fs::PermissionsExt;

        check_member_handle(member)?;
        let path = member.canonical();
        let metadata = std::fs::symlink_metadata(path).map_err(|_| missing_member(path))?;
        if metadata.file_type().is_symlink() {
            return Ok(DEFAULT_MEMBER_MODE);
        }
        Ok(metadata.permissions().mode() & 0o777)
    }
}

/// Proves one source holds a compressed archive.
///
/// Gzip magic plus tar readability under current fallback rules.
///
/// # Errors
///
/// Plain and undecodable sources fail as plan errors naming
/// the source.
fn check_compressed_source(source: &Path) -> Result<()> {
    if !peek_is_gzip(source)? {
        return Err(not_archive(source));
    }
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    match stream_names(flate2::read::GzDecoder::new(file), source) {
        Ok(_) => Ok(()),
        Err(_) if wants_tar(source) => Err(not_archive(source)),
        Err(_) => verify_gzip_body(source),
    }
}

/// Proves one gzip body decodes.
///
/// # Errors
///
/// Undecodable sources fail as plan errors naming the source.
fn verify_gzip_body(source: &Path) -> Result<()> {
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    std::io::copy(
        &mut flate2::read::GzDecoder::new(file),
        &mut std::io::sink(),
    )
    .map_err(|_| not_archive(source))?;
    Ok(())
}

/// Builds one non-archive error naming the source.
fn not_archive(source: &Path) -> Error {
    Error::Plan(format!(
        "cannot archive '{}': not a compressed archive",
        source.display()
    ))
}

/// Builds born member handles from streamed hashes.
///
/// The spill folder roots containment for every member.
///
/// # Errors
///
/// Containment failures surface as plan errors.
fn streamed_handles(dest: &Path, members: &[BornMember]) -> Result<Vec<ResourceHandle>> {
    let mut handles = Vec::with_capacity(members.len());
    for member in members {
        handles.push(ResourceHandle::new(
            dest,
            dest.join(&member.name),
            member.sha.clone(),
        )?);
    }
    handles.sort_by(|left, right| left.canonical().cmp(right.canonical()));
    Ok(handles)
}

/// Moves one staging spill into place.
///
/// A present destination wins the rename race.
///
/// # Errors
///
/// Rename failures surface as plan errors naming the source.
fn finish_unpack(
    source: &Path,
    dest: &Path,
    staging: &Path,
    born: &[BornMember],
) -> Result<Vec<ResourceHandle>> {
    match std::fs::rename(staging, dest) {
        Ok(()) => streamed_handles(dest, born),
        Err(_) if dest.is_dir() => {
            let _ = std::fs::remove_dir_all(staging);
            streamed_handles(dest, born)
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(staging);
            Err(unpack_failure(source, error))
        }
    }
}

/// Reports gzip magic for one source path.
///
/// # Errors
///
/// Missing and unreadable files fail as plan errors naming
/// the archive.
fn peek_is_gzip(source: &Path) -> Result<bool> {
    let mut file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    let mut magic = [0u8; 2];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(magic == [0x1f, 0x8b]),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(Error::Plan(format!(
            "cannot read '{}': {error}",
            source.display()
        ))),
    }
}

/// Unpacks plain tar members from disk with streaming hashes.
///
/// Each entry checks escape before its bytes spill.
///
/// # Errors
///
/// Decoder and spill failures surface as plan errors naming
/// the source.
fn unpack_tar_file(source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    unpack_tar_entries(file, source, staging)
}

/// Unpacks gzipped tar members from disk with streaming hashes.
///
/// Each entry checks escape before its bytes spill.
///
/// # Errors
///
/// Decoder and spill failures surface as plan errors naming
/// the source.
fn unpack_gzipped_tar(source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    unpack_tar_entries(flate2::read::GzDecoder::new(file), source, staging)
}

/// Unpacks tar entries with per-entry streaming hashes.
///
/// Skipped entries drain to a sink. Escape checks run before
/// each spill. Chunk copies feed the member hash.
///
/// # Errors
///
/// Decoder and spill failures surface as plan errors naming
/// the source.
fn unpack_tar_entries<R: std::io::Read>(
    reader: R,
    source: &Path,
    staging: &Path,
) -> Result<Vec<BornMember>> {
    use std::io::Write as _;

    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| unpack_failure(source, error))?;
    let mut born = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| unpack_failure(source, error))?;
        let Some(name) = entry_name(&entry, source)? else {
            std::io::copy(&mut entry, &mut std::io::sink())
                .map_err(|error| unpack_failure(source, error))?;
            continue;
        };
        check_member_path(&name, source)?;
        let path = staging.join(&name);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            let _ = std::fs::remove_dir_all(staging);
            return Err(unpack_failure(source, error));
        }
        let mut out = match std::fs::File::create(&path) {
            Ok(out) => out,
            Err(error) => {
                let _ = std::fs::remove_dir_all(staging);
                return Err(unpack_failure(source, error));
            }
        };
        let mut hasher = sha2::Sha256::new();
        let mut chunk = [0u8; ENTRY_CHUNK];
        loop {
            let read = match entry.read(&mut chunk) {
                Ok(read) => read,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(staging);
                    return Err(unpack_failure(source, error));
                }
            };
            if read == 0 {
                break;
            }
            hasher.update(&chunk[..read]);
            if let Err(error) = out.write_all(&chunk[..read]) {
                let _ = std::fs::remove_dir_all(staging);
                return Err(unpack_failure(source, error));
            }
        }
        let sha: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        born.push(BornMember { name, sha });
    }
    Ok(born)
}

/// Unpacks one single-file gzip member with a streaming hash.
///
/// # Errors
///
/// Decoder and spill failures surface as plan errors naming
/// the source.
fn unpack_single_entry(source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
    use std::io::Write as _;

    let name = single_name(source);
    check_member_path(&name, source)?;
    let path = staging.join(&name);
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        let _ = std::fs::remove_dir_all(staging);
        return Err(unpack_failure(source, error));
    }
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut out = match std::fs::File::create(&path) {
        Ok(out) => out,
        Err(error) => {
            let _ = std::fs::remove_dir_all(staging);
            return Err(unpack_failure(source, error));
        }
    };
    let mut hasher = sha2::Sha256::new();
    let mut chunk = [0u8; ENTRY_CHUNK];
    loop {
        let read = match decoder.read(&mut chunk) {
            Ok(read) => read,
            Err(error) => {
                let _ = std::fs::remove_dir_all(staging);
                return Err(unpack_failure(source, error));
            }
        };
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
        if let Err(error) = out.write_all(&chunk[..read]) {
            let _ = std::fs::remove_dir_all(staging);
            return Err(unpack_failure(source, error));
        }
    }
    let sha: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(vec![BornMember { name, sha }])
}

/// Reads born handles back for one present spill.
///
/// Names sort, so repeats share order with fresh unpacks.
///
/// # Errors
///
/// Unreadable spills and missing members fail as plan errors.
fn spilled_handles(dest: &Path, source: &Path) -> Result<Vec<ResourceHandle>> {
    let mut names = Vec::new();
    collect_spill_names(dest, dest, &mut names, source)?;
    names.sort();
    let mut handles = Vec::with_capacity(names.len());
    for name in &names {
        let path = dest.join(name);
        let sha = spill_file_sha(&path, source, name)?;
        handles.push(ResourceHandle::new(dest, path, sha)?);
    }
    Ok(handles)
}

/// Hashes one spilled member file with a stream.
///
/// # Errors
///
/// Unreadable members fail as plan errors naming archive
/// and member.
fn spill_file_sha(path: &Path, source: &Path, name: &str) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|_| missing_spilled_member(source, name))?;
    let mut hasher = sha2::Sha256::new();
    let mut chunk = [0u8; ENTRY_CHUNK];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| missing_spilled_member(source, name))?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Collects archive-relative member names under one spill folder.
///
/// Folders skip; files read archive-relative with forward slashes.
///
/// # Errors
///
/// Listing failures surface as plan errors naming the source.
fn collect_spill_names(
    dir: &Path,
    root: &Path,
    names: &mut Vec<String>,
    source: &Path,
) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| unpack_failure(source, error))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| unpack_failure(source, error))?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_spill_names(&path, root, names, source)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| unpack_failure(source, error))?;
            names.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Builds one missing spilled member error naming archive and member.
fn missing_spilled_member(source: &Path, name: &str) -> Error {
    Error::Plan(format!(
        "cannot unpack '{}': missing member '{name}'",
        source.display()
    ))
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

/// Builds one unpack plan error naming the archive.
fn unpack_failure(archive: &Path, error: impl std::fmt::Display) -> Error {
    Error::Plan(format!("cannot unpack '{}': {error}", archive.display()))
}

/// Rejects escaping member names before one unpack.
///
/// Names stream with content sunk. Decoder failures pass
/// through untouched so the unpack fallback still decides
/// tar-shaped names over non-tar bytes.
///
/// # Errors
///
/// Escaping members fail as plan errors naming the member.
fn check_unpack_names(source: &Path, gzipped: bool) -> Result<()> {
    let file = std::fs::File::open(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    let names = if gzipped {
        match stream_names(flate2::read::GzDecoder::new(file), source) {
            Ok(names) => names,
            Err(_) => return Ok(()),
        }
    } else {
        stream_names(file, source)?
    };
    for name in &names {
        check_member_path(name, source)?;
    }
    Ok(())
}

/// Derives the staging folder beside one unpack destination.
///
/// Staging rides beside the destination with the staging suffix.
fn staging_path(dest: &Path) -> PathBuf {
    let mut staging = dest.as_os_str().to_owned();
    staging.push(STAGING_SUFFIX);
    PathBuf::from(staging)
}

/// Lists file member names from a tar stream without keeping content.
///
/// Entry bytes stream to a sink, so listings hold names only.
///
/// # Errors
///
/// Malformed archives fail as plan errors naming the archive.
fn stream_names<R: std::io::Read>(reader: R, archive: &Path) -> Result<Vec<String>> {
    let mut reader = tar::Archive::new(reader);
    let entries = reader
        .entries()
        .map_err(|error| unpack_failure(archive, error))?;
    let mut names = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| unpack_failure(archive, error))?;
        if let Some(name) = entry_name(&entry, archive)? {
            names.push(name);
        }
        std::io::copy(&mut entry, &mut std::io::sink())
            .map_err(|error| unpack_failure(archive, error))?;
    }
    Ok(names)
}

/// Reads the file name for one tar entry.
///
/// Folders, non-files, and empty names skip as absent.
///
/// # Errors
///
/// Undecodable entry paths fail as plan errors naming the
/// archive.
fn entry_name<R: std::io::Read>(
    entry: &tar::Entry<'_, R>,
    archive: &Path,
) -> Result<Option<String>> {
    let kind = entry.header().entry_type();
    if kind.is_dir() || !kind.is_file() {
        return Ok(None);
    }
    let name = entry
        .path()
        .map_err(|error| unpack_failure(archive, error))?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

/// Rejects member paths escaping the unpack folder.
///
/// # Errors
///
/// Empty, absolute, and dot-dot members fail as plan errors.
fn check_member_path(name: &str, archive: &Path) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Plan(format!(
            "cannot unpack '{}': empty member path",
            archive.display()
        )));
    }
    if Path::new(name).is_absolute() {
        return Err(Error::Plan(format!(
            "cannot unpack '{}': member '{name}' escapes",
            archive.display()
        )));
    }
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(Error::Plan(format!(
                "cannot unpack '{}': member '{name}' escapes",
                archive.display()
            )));
        }
    }
    Ok(())
}

/// Reports true while a name wants tar members.
fn wants_tar(archive: &Path) -> bool {
    let lower = archive.as_os_str().to_string_lossy().to_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".tar")
}

/// Derives the single-file member name from an archive path.
fn single_name(archive: &Path) -> String {
    let base = archive
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

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::ids::sha256_hex;
    use std::io::Write as _;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            temp_base: dir.join("temp"),
            ..Default::default()
        }
    }

    fn test_store(dir: &Path) -> FileArchiveStore {
        FileArchiveStore::new(&test_roots(dir))
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
        std::fs::write(&path, bytes).unwrap();
        path
    }

    struct TestSource(PathBuf, String);

    impl TrustedHandle for TestSource {
        fn canonical(&self) -> &Path {
            &self.0
        }

        fn sha(&self) -> &str {
            &self.1
        }
    }

    fn born_archive(
        store: &FileArchiveStore,
        dir: &Path,
        name: &str,
        bytes: &[u8],
    ) -> ArchiveHandle {
        let path = write_archive(dir, name, bytes);
        match store.archive(&TestSource(path, sha256_hex(bytes))) {
            Ok(handle) => handle,
            Err(error) => panic!("source seals: {error}"),
        }
    }

    fn handle_sha(handles: &[ResourceHandle], name: &str) -> String {
        handles
            .iter()
            .find(|handle| handle.canonical().to_string_lossy().ends_with(name))
            .unwrap()
            .sha()
            .to_string()
    }

    #[test]
    fn birth_seals_proof_for_compressed_source() {
        use confit_core::handles::ArchiveProof;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.tar.gz", &raw);
        match store.archive(&TestSource(path.clone(), sha256_hex(&raw))) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha(), sha256_hex(&raw));
                assert_eq!(handle.proof(), ArchiveProof::Compressed);
            }
            Err(error) => panic!("source seals: {error}"),
        }
    }

    #[test]
    fn birth_rejects_plain_and_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        match store.archive(&TestSource(path.clone(), sha256_hex(b"plain text"))) {
            Ok(_) => panic!("plain source passes"),
            Err(error) => assert!(
                error.to_string().contains(&path.display().to_string()),
                "error names the source: {error}"
            ),
        }
        let missing = dir.path().join("absent.tar.gz");
        match store.archive(&TestSource(missing.clone(), sha256_hex(b"absent"))) {
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
        let store = test_store(dir.path());
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
        let store = test_store(dir.path());
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
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        let expected_spill = spill_base.join(archive.sha());
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
        assert_eq!(handle_sha(&handles, "a.txt"), sha256_hex(b"same"));
        assert_eq!(handle_sha(&handles, "a.txt"), handle_sha(&handles, "b.txt"));
        assert_ne!(
            handle_sha(&handles, "a.txt"),
            handle_sha(&handles, "sub/c.txt")
        );
        assert_eq!(
            std::fs::read(handles[0].canonical()).unwrap(),
            b"same".as_slice()
        );
        let spill = handles[0].canonical().parent().unwrap().to_path_buf();
        let mut staging = spill.as_os_str().to_owned();
        staging.push(STAGING_SUFFIX);
        assert!(!PathBuf::from(staging).exists());
        match store.extract(&archive) {
            Ok(reused) => assert_eq!(reused, handles),
            Err(error) => panic!("repeat unpack skips: {error}"),
        }
    }

    #[test]
    fn open_reads_through_member_handle_naming_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
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
        let root = handles[0].canonical().parent().unwrap().to_path_buf();
        let missing =
            ResourceHandle::new(&root, root.join("absent.txt"), sha256_hex(b"absent")).unwrap();
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
        let store = test_store(dir.path());
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
        let evil = ResourceHandle::new(&root, root.join("../evil.txt"), sha256_hex(b"x")).unwrap();
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
    fn open_tracks_live_spill_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
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
        match store.open_decompressed(&handles[0]) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"alpha");
            }
            Err(error) => panic!("member opens from spill: {error}"),
        }
        std::fs::write(handles[0].canonical(), b"patched").unwrap();
        match store.open_decompressed(&handles[0]) {
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
        let store = test_store(dir.path());
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
        let spill_base = dir.path().join("temp").join(EXTRACT_DIR);
        if spill_base.exists() {
            let left = std::fs::read_dir(&spill_base).unwrap().count();
            assert_eq!(left, 0);
        }
    }

    #[test]
    fn single_gzip_lists_and_opens_derived_member() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
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
        assert_eq!(handles[0].sha(), sha256_hex(b"plain"));
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
    fn members_rejects_missing_archive() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let missing = dir.path().join("absent.tar.gz");
        let handle = ArchiveHandle::new(&missing, sha256_hex(b"absent")).unwrap();
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
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let sha_a = sha256_hex(b"archive-a");
        let sha_b = sha256_hex(b"archive-b");
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
        assert_eq!(first.sha(), sha_a);
        assert_eq!(second.sha(), sha_b);
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
                .starts_with(spill_base.join(first.sha())),
            "first spill follows its handle: {}",
            first_handles[0].canonical().display()
        );
        assert!(
            second_handles[0]
                .canonical()
                .starts_with(spill_base.join(second.sha())),
            "second spill follows its handle: {}",
            second_handles[0].canonical().display()
        );
        assert_ne!(first_handles[0].canonical(), second_handles[0].canonical());
    }

    #[test]
    fn extract_and_open_streams_large_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > ENTRY_CHUNK);
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
