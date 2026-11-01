//! Disk inspection for difference runs.

use std::path::{Path, PathBuf};

use confit_core::error::ContextualResult;
use confit_core::error::{ContextualCause, ErrorCause, FilesystemError, WithContext};
use confit_core::sha::Sha;
use confit_driver as driver;
use confit_model::document::{Data, Document};
use confit_model::routes::Route;

use super::Difference;

/// One content hash for a body of bytes.
pub(crate) struct Digest {
    /// The content hash.
    pub(crate) sha: Sha,
    /// The content byte count.
    pub(crate) len: u64,
}

impl Digest {
    /// Returns the digest label with its byte count.
    pub(crate) fn label(&self) -> String {
        self.sha.label(self.len)
    }
}

/// The state of one destination on disk.
enum DiskOutcome<T> {
    /// An absent destination.
    Absent,
    /// An unreadable destination with its located fault.
    Unreadable {
        /// The fault cause for the failed read.
        fault: Box<ContextualCause>,
    },
    /// A directory that blocks the destination.
    Blocked,
    /// A destination that the read finds present.
    Present(T),
}

/// The bytes of one present file.
pub(crate) struct FileBytes {
    /// The destination bytes.
    pub(crate) bytes: Vec<u8>,
    /// The Unix permission bits of the file.
    pub(crate) mode: Option<u32>,
}

/// The digest of one present file.
pub(crate) struct FileDigest {
    /// The content digest.
    pub(crate) digest: Digest,
    /// The Unix permission bits of the file.
    pub(crate) mode: Option<u32>,
}

/// A reader of document destinations on disk.
pub(crate) struct DiskReader;

impl DiskReader {
    /// Reads one document destination into bytes.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    pub(crate) fn read_bytes(
        document: &Document,
        on_present: impl FnOnce(FileBytes) -> ContextualResult<Vec<Difference>>,
    ) -> ContextualResult<Vec<Difference>> {
        let expanded = Self::expanded(document)?;
        let outcome = if matches!(document.data, Data::Link(_)) {
            match driver::fs::read_link_bytes(&expanded) {
                Ok(bytes) => DiskOutcome::Present(FileBytes { bytes, mode: None }),
                Err(_) if expanded.is_dir() => DiskOutcome::Blocked,
                Err(_) => Self::read_path(&expanded),
            }
        } else {
            let target = Self::resolve_target(&expanded);
            if target.is_dir() {
                DiskOutcome::Blocked
            } else {
                Self::read_path(&target)
            }
        };
        Self::mapped(outcome, &document.path, on_present)
    }

    /// Reads one document destination into its content hash.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    pub(crate) fn digest(
        document: &Document,
        on_present: impl FnOnce(FileDigest) -> ContextualResult<Vec<Difference>>,
    ) -> ContextualResult<Vec<Difference>> {
        let expanded = Self::expanded(document)?;
        let outcome = if matches!(document.data, Data::Link(_)) {
            match driver::fs::read_link_bytes(&expanded) {
                Ok(bytes) => {
                    let len = bytes.len() as u64;
                    DiskOutcome::Present(FileDigest {
                        digest: Digest {
                            sha: Sha::hash(&bytes),
                            len,
                        },
                        mode: None,
                    })
                }
                Err(_) if expanded.is_dir() => DiskOutcome::Blocked,
                Err(_) => Self::digest_path(&expanded),
            }
        } else {
            let target = Self::resolve_target(&expanded);
            if target.is_dir() {
                DiskOutcome::Blocked
            } else {
                Self::digest_path(&target)
            }
        };
        Self::mapped(outcome, &document.path, on_present)
    }

    /// Reads one tree member path under its expanded base.
    pub(crate) fn probe_member(
        base: &Path,
        relative: &str,
        path: &Route,
        on_present: impl FnOnce(FileDigest) -> ContextualResult<Vec<Difference>>,
    ) -> ContextualResult<Vec<Difference>> {
        let full = base.join(relative);
        let outcome = if full.is_symlink() {
            match driver::fs::read_link_bytes(&full) {
                Ok(bytes) => {
                    let len = bytes.len() as u64;
                    DiskOutcome::Present(FileDigest {
                        digest: Digest {
                            sha: Sha::hash(&bytes),
                            len,
                        },
                        mode: None,
                    })
                }
                Err(contextual)
                    if matches!(
                        contextual.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ) =>
                {
                    DiskOutcome::Absent
                }
                Err(contextual) => DiskOutcome::Unreadable {
                    fault: Box::new(contextual),
                },
            }
        } else if full.is_dir() {
            DiskOutcome::Blocked
        } else {
            Self::digest_path(&full)
        };
        Self::mapped(outcome, path, on_present)
    }

    /// Maps one read outcome into difference entries.
    fn mapped<T>(
        outcome: DiskOutcome<T>,
        path: &Route,
        on_present: impl FnOnce(T) -> ContextualResult<Vec<Difference>>,
    ) -> ContextualResult<Vec<Difference>> {
        Ok(match outcome {
            DiskOutcome::Absent => vec![Difference::Missing { path: path.clone() }],
            DiskOutcome::Unreadable { fault } => vec![Difference::Unreadable {
                path: path.clone(),
                reason: fault.cause().to_string(),
            }],
            DiskOutcome::Blocked => vec![Difference::Blocked { path: path.clone() }],
            DiskOutcome::Present(payload) => return on_present(payload),
        })
    }

    /// Expands the destination route of one document.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
    fn expanded(document: &Document) -> ContextualResult<PathBuf> {
        document
            .path
            .expand()
            .with_document(document.path.display())
    }

    /// Resolves one expanded path to its symlink target.
    fn resolve_target(expanded: &Path) -> PathBuf {
        match std::fs::read_link(expanded) {
            Ok(link) => Self::join_target(expanded, &link),
            Err(_) => expanded.to_path_buf(),
        }
    }

    /// Reads one regular path into bytes with its permission bits.
    fn read_path(path: &Path) -> DiskOutcome<FileBytes> {
        match std::fs::read(path) {
            Ok(bytes) => DiskOutcome::Present(FileBytes {
                bytes,
                mode: Self::read_mode(path),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskOutcome::Absent,
            Err(error) => DiskOutcome::Unreadable {
                fault: Box::new(ContextualCause::from(error).with_fs_path(path)),
            },
        }
    }

    /// Reads one regular path into its content hash with its permission bits.
    fn digest_path(path: &Path) -> DiskOutcome<FileDigest> {
        let mode = Self::read_mode(path);
        match std::fs::File::open(path) {
            Ok(mut file) => match Sha::read_with_size(&mut file) {
                Ok((sha, len)) => DiskOutcome::Present(FileDigest {
                    digest: Digest { sha, len },
                    mode,
                }),
                Err(error) => DiskOutcome::Unreadable {
                    fault: Box::new(ContextualCause::from(error).with_fs_path(path)),
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskOutcome::Absent,
            Err(error) => DiskOutcome::Unreadable {
                fault: Box::new(ContextualCause::from(error).with_fs_path(path)),
            },
        }
    }

    /// Joins a link target to the folder that holds the link.
    fn join_target(link: &Path, target: &Path) -> PathBuf {
        if target.is_absolute() {
            return target.to_path_buf();
        }
        let mut out = match link.parent() {
            Some(parent) => parent.to_path_buf(),
            None => PathBuf::new(),
        };
        for part in target.components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                _ => out.push(part),
            }
        }
        out
    }

    /// Reads the Unix permission bits of one path.
    fn read_mode(path: &Path) -> Option<u32> {
        if path.is_symlink() {
            return None;
        }
        driver::fs::mode(path).ok()
    }
}
