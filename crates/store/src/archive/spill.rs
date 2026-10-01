//! Spill
//!
//! Member spilling with hashes and containment under
//! staging folders.

use std::path::{Path, PathBuf};

use confit_model::sha::{Sha, ShaWriter};

use super::ArchiveBackend;
use super::error::{ArchiveError, Result};
use confit_driver::{self as driver};

/// Archive member with a streamed content hash.
pub(crate) struct BornMember {
    pub(crate) name: String,
    pub(crate) sha: Sha,
}

/// Maps one stream failure into its archive error.
pub(crate) fn from_stream(source: &Path, error: std::io::Error) -> ArchiveError {
    match error.kind() {
        std::io::ErrorKind::StorageFull
        | std::io::ErrorKind::ReadOnlyFilesystem
        | std::io::ErrorKind::QuotaExceeded
        | std::io::ErrorKind::FileTooLarge => ArchiveError::from_write_io(source, error),
        std::io::ErrorKind::Other
        | std::io::ErrorKind::InvalidInput
        | std::io::ErrorKind::InvalidData
        | std::io::ErrorKind::UnexpectedEof => ArchiveError::CorruptedArchive {
            path: source.to_path_buf(),
        },
        _ => ArchiveError::from_io(source, error),
    }
}

/// Spills one decoded member stream under staging with a hash.
///
/// Parents arrive created, and bytes stream through the digest
/// writer into the spill file in one pass.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping member names.
/// - [`ArchiveError::Missing`] for missing paths.
/// - [`ArchiveError::Denied`] for denied paths.
/// - [`ArchiveError::Write`] for spill write faults.
/// - [`ArchiveError::Unknown`] for other spill failures.
pub(crate) fn spill_entry(
    source: &Path,
    staging: &Path,
    name: &str,
    reader: impl std::io::Read,
) -> Result<BornMember> {
    check_member_path(name, source)?;
    let path = staging.join(name);
    if let Some(parent) = path.parent()
        && let Err(error) = driver::fs::create_dir_all(parent)
    {
        let _ = driver::fs::remove_dir_all(staging);
        return Err(ArchiveError::from_write_io(source, error));
    }
    let file = match driver::fs::create(&path) {
        Ok(file) => file,
        Err(error) => {
            let _ = driver::fs::remove_dir_all(staging);
            return Err(ArchiveError::from_write_io(source, error));
        }
    };
    let mut writer = ShaWriter::new(file);
    if let Err(error) = driver::copy_stream(reader, &mut writer) {
        let _ = driver::fs::remove_dir_all(staging);
        return Err(from_stream(source, error));
    }
    Ok(BornMember {
        name: name.to_owned(),
        sha: writer.digest(),
    })
}

/// Rejects escaping member names before one unpack.
///
/// Decoder failures pass through untouched, so the unpack
/// fallback still decides tar-shaped names over non-tar
/// bytes.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping members.
/// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
/// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
/// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
pub(crate) fn check_backend_names(source: &Path, backend: &dyn ArchiveBackend) -> Result<()> {
    for name in &backend.names(source)? {
        check_member_path(name, source)?;
    }
    Ok(())
}

/// Rejects member paths escaping the unpack folder.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for empty, absolute, and dot-dot members.
pub(crate) fn check_member_path(name: &str, archive: &Path) -> Result<()> {
    if name.is_empty() {
        return Err(ArchiveError::Escape {
            path: archive.to_path_buf(),
        });
    }
    if Path::new(name).is_absolute() {
        return Err(ArchiveError::Escape {
            path: PathBuf::from(name),
        });
    }
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(ArchiveError::Escape {
                path: archive.join(name),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faults::AccessFault;

    #[test]
    fn stream_names_disk_kinds_before_corrupt() {
        let source = Path::new("fonts.tar.gz");
        let error = from_stream(
            source,
            std::io::Error::new(std::io::ErrorKind::StorageFull, "disk failed"),
        );
        match &error {
            ArchiveError::Write { path: found, fault } => {
                assert_eq!(found, source, "spill keeps the source");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "spill keeps the fault: {error}"
                );
            }
            other => panic!("wrong spill variant: {other}"),
        }
        let error = from_stream(
            source,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "torn bytes"),
        );
        assert!(
            matches!(error, ArchiveError::CorruptedArchive { .. }),
            "torn bytes stay corrupt: {error}"
        );
    }
}
