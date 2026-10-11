//! Atomic file writes and exact streaming copies over unique pending names.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Pending suffix for sibling scratch files.
const PENDING_SUFFIX: &str = ".part";

/// Streaming chunk for exact copies.
const COPY_CHUNK: usize = 8192;

/// Sequence for unique pending names per process.
static PENDING_SEQ: AtomicU64 = AtomicU64::new(0);

/// Derive the sibling scratch path for one destination.
pub fn pending_path(dest: &Path) -> PathBuf {
    let seq = PENDING_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut pending = dest.as_os_str().to_owned();
    pending.push(format!("{PENDING_SUFFIX}-{}-{seq}", std::process::id()));
    PathBuf::from(pending)
}

/// Write one file atomically through a sibling rename.
///
/// - Parent folders build first.
/// - Closure failures remove pending and report the closure error.
/// - Rename failures with a present destination report success with the
///   present entry kept.
///
/// # Errors
///
/// - Parent build, closure, and rename failures fail
///   as io errors.
pub fn atomic_write(
    dest: &Path,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    crate::fs::create_parent(dest)?;
    let pending = pending_path(dest);
    if let Err(error) = write(&pending) {
        let _ = crate::fs::remove_file(&pending);
        return Err(error);
    }
    match crate::fs::rename(&pending, dest) {
        Ok(()) => Ok(()),
        Err(_) if crate::fs::metadata(dest).is_ok() => {
            let _ = crate::fs::remove_file(&pending);
            Ok(())
        }
        Err(error) => {
            let _ = crate::fs::remove_file(&pending);
            Err(error)
        }
    }
}

/// Stream one reader into one writer exactly.
///
/// # Errors
///
/// - Read and write failures fail as io errors.
pub fn copy_stream(
    mut reader: impl std::io::Read,
    mut writer: impl std::io::Write,
) -> std::io::Result<u64> {
    let mut chunk = [0u8; COPY_CHUNK];
    let mut wrote = 0u64;
    loop {
        let used = reader.read(&mut chunk)?;
        if used == 0 {
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
        let _guard = crate::fs::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        atomic_write(&dest, |pending| crate::fs::write(pending, b"atomic bytes")).unwrap();
        assert_eq!(crate::fs::read(&dest).unwrap(), b"atomic bytes");
    }

    #[test]
    fn atomic_write_failure_cleans_pending() {
        let _guard = crate::fs::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        let outcome = atomic_write(&dest, |_| {
            Err(std::io::Error::other("scripted write failure"))
        });
        assert!(outcome.is_err(), "closure failure reports loud");
        assert!(
            !crate::fs::exists(&dest),
            "failed write leaves no destination"
        );
        let parent = dest.parent().unwrap().to_path_buf();
        let leftovers = crate::fs::read_dir(&parent).unwrap_or_default();
        assert!(leftovers.is_empty(), "failed write leaves no pending entry");
    }

    #[test]
    fn atomic_write_overwrites_present_destination() {
        let _guard = crate::fs::TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        crate::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        crate::fs::write(&dest, b"winner").unwrap();
        atomic_write(&dest, |pending| crate::fs::write(pending, b"newcomer")).unwrap();
        assert_eq!(crate::fs::read(&dest).unwrap(), b"newcomer");
    }

    #[test]
    fn copy_stream_moves_bytes_exact() {
        let payload: Vec<u8> = (0..20_000u32).map(|n| (n % 251) as u8).collect();
        let mut out = Vec::new();
        let wrote = copy_stream(Cursor::new(&payload), &mut out).unwrap();
        assert_eq!(wrote, payload.len() as u64);
        assert_eq!(out, payload);
    }
}
