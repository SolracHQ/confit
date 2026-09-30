//! Atomic file writes and verbatim streaming copies
//! over unique staging names.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Staging suffix for sibling scratch files.
const STAGING_SUFFIX: &str = ".part";

/// Shared streaming chunk for verbatim copies.
const COPY_CHUNK: usize = 8192;

/// Sequence feeding unique staging names per process.
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// Derives the sibling scratch path for one destination.
///
/// Names carry process and sequence, so concurrent
/// writers share no file.
///
/// # Errors
///
/// Staging derivation never fails.
pub fn stage_path(dest: &Path) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut staging = dest.as_os_str().to_owned();
    staging.push(format!("{STAGING_SUFFIX}-{}-{seq}", std::process::id()));
    PathBuf::from(staging)
}

/// Writes one file atomically through a sibling rename.
///
/// Parent folders build first. Closure failure removes
/// staging and reports the closure error. A rename
/// failure with a present destination reports success
/// with the winner standing, matching the unpack race.
///
/// # Errors
///
/// - Parent build, closure, and rename failures fail
///   as io errors.
pub fn atomic_write(
    dest: &Path,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        super::create_dir_all(parent)?;
    }
    let staging = stage_path(dest);
    if let Err(error) = write(&staging) {
        let _ = super::remove_file(&staging);
        return Err(error);
    }
    match super::rename(&staging, dest) {
        Ok(()) => Ok(()),
        Err(_) if super::metadata(dest).is_ok() => {
            let _ = super::remove_file(&staging);
            Ok(())
        }
        Err(error) => {
            let _ = super::remove_file(&staging);
            Err(error)
        }
    }
}

/// Streams one reader into one writer verbatim.
///
/// Hashing stays in store wrappers; this verb moves
/// bytes alone and reports the count.
///
/// # Errors
///
/// - Read and write and flush failures fail as io
///   errors.
pub fn copy_stream(
    mut reader: impl std::io::Read,
    mut writer: impl std::io::Write,
) -> std::io::Result<u64> {
    let mut chunk = [0u8; COPY_CHUNK];
    let mut wrote = 0u64;
    loop {
        let used = reader.read(&mut chunk)?;
        if used == 0 {
            writer.flush()?;
            return Ok(wrote);
        }
        writer.write_all(&chunk[..used])?;
        wrote += used as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn atomic_write_round_trip() {
        let _guard = super::super::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        atomic_write(&dest, |staging| {
            super::super::write(staging, b"atomic bytes")
        })
        .unwrap();
        assert_eq!(super::super::read(&dest).unwrap(), b"atomic bytes");
    }

    #[test]
    fn atomic_write_failure_cleans_staging() {
        let _guard = super::super::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        let outcome = atomic_write(&dest, |_| {
            Err(std::io::Error::other("scripted write failure"))
        });
        assert!(outcome.is_err(), "closure failure reports loud");
        assert!(
            !super::super::exists(&dest),
            "failed write leaves no destination"
        );
        let parent = dest.parent().unwrap().to_path_buf();
        let leftovers = super::super::read_dir(&parent).unwrap_or_default();
        assert!(leftovers.is_empty(), "failed write leaves no staging entry");
    }

    #[test]
    fn atomic_write_overwrites_present_destination() {
        let _guard = super::super::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        super::super::create_dir_all(dest.parent().unwrap()).unwrap();
        super::super::write(&dest, b"winner").unwrap();
        atomic_write(&dest, |staging| super::super::write(staging, b"newcomer")).unwrap();
        assert_eq!(super::super::read(&dest).unwrap(), b"newcomer");
    }

    #[test]
    fn copy_stream_moves_bytes_verbatim() {
        let payload: Vec<u8> = (0..20_000u32).map(|n| (n % 251) as u8).collect();
        let mut out = Vec::new();
        let wrote = copy_stream(Cursor::new(&payload), &mut out).unwrap();
        assert_eq!(wrote, payload.len() as u64);
        assert_eq!(out, payload);
    }
}
