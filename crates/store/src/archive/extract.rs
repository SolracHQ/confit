//! Member extraction with hashes and containment.

use std::path::{Path, PathBuf};

use confit_driver::sha::{Sha, ShaWriter};

use super::ArchiveBackend;
use super::error::{ArchiveError, Result};
use confit_driver::{self as driver};

/// Archive member with a streamed content hash.
pub(crate) struct ExtractedMember {
    pub(crate) name: String,
    pub(crate) sha: Sha,
}

/// Map one stream failure into its archive error.
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

/// Extract one decoded member stream under pending with a hash.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping member names.
/// - [`ArchiveError::Read`] for failed source reads.
/// - [`ArchiveError::Write`] for extract write faults.
/// - [`ArchiveError::WriteUnknown`] for other extract write failures.
pub(crate) fn extract_entry(
    source: &Path,
    pending: &Path,
    name: &str,
    reader: impl std::io::Read,
) -> Result<ExtractedMember> {
    check_member_path(name, source)?;
    let path = pending.join(name);
    if let Err(error) = driver::fs::create_parent(&path) {
        let _ = driver::fs::remove_dir_all(pending);
        return Err(ArchiveError::from_write_io(source, error));
    }
    let file = match driver::fs::create(&path) {
        Ok(file) => file,
        Err(error) => {
            let _ = driver::fs::remove_dir_all(pending);
            return Err(ArchiveError::from_write_io(source, error));
        }
    };
    let mut writer = ShaWriter::new(file);
    if let Err(error) = driver::copy_stream(reader, &mut writer) {
        let _ = driver::fs::remove_dir_all(pending);
        return Err(from_stream(source, error));
    }
    Ok(ExtractedMember {
        name: name.to_owned(),
        sha: writer.digest(),
    })
}

/// Reject escaping member names before one unpack.
///
/// - Decoder failures pass through untouched.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping members.
/// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
/// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
/// - [`ArchiveError::UnsupportedCompression`] for verified compression.
pub(crate) fn check_backend_names(source: &Path, backend: &dyn ArchiveBackend) -> Result<()> {
    for name in &backend.names(source)? {
        check_member_path(name, source)?;
    }
    Ok(())
}

/// Reject member paths escaping the unpack folder.
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
                assert_eq!(found, source, "extract keeps the source");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "extract keeps the fault: {error}"
                );
            }
            other => panic!("wrong extract variant: {other}"),
        }
        let error = from_stream(
            source,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt bytes"),
        );
        assert!(
            matches!(error, ArchiveError::CorruptedArchive { .. }),
            "corrupt bytes stay corrupt: {error}"
        );
    }
}
