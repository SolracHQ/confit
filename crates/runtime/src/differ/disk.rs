//! Disk inspection for difference runs.

use std::path::{Path, PathBuf};

use confit_core::error::FilesystemError;
use confit_core::error::Result;
use confit_core::error::locate::Locate;
use confit_core::sha::Sha;
use confit_driver as driver;
use confit_model::document::{Data, Document};
use confit_model::routes::Route;

use super::Difference;

/// Content hash.
pub(crate) struct Digest {
    /// The content hash.
    pub(crate) sha: Sha,
    /// The content byte count.
    pub(crate) len: u64,
}

impl Digest {
    /// Label one digest with its byte count.
    pub(crate) fn label(&self) -> String {
        self.sha.label(self.len)
    }
}

/// Disk read outcome.
enum DiskOutcome<T> {
    /// Missing destination.
    Absent,
    /// Unreadable destination.
    Unreadable {
        /// The fault cause for the failed read.
        fault: FilesystemError,
    },
    /// Directory blocks the destination.
    Blocked,
    /// Present destination.
    Present(T),
}

/// Present bytes.
pub(crate) struct FileBytes {
    /// The destination bytes.
    pub(crate) bytes: Vec<u8>,
    /// Unix permission bits.
    pub(crate) mode: Option<u32>,
}

/// Present digest.
pub(crate) struct FileDigest {
    /// The content digest.
    pub(crate) digest: Digest,
    /// Unix permission bits.
    pub(crate) mode: Option<u32>,
}

/// Kind-aware disk reader.
pub(crate) struct DiskReader;

impl DiskReader {
    /// Read one document destination into bytes, mapping terminal states.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    pub(crate) fn read_bytes(
        document: &Document,
        on_present: impl FnOnce(FileBytes) -> Result<Vec<Difference>>,
    ) -> Result<Vec<Difference>> {
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

    /// Digest one document destination into its hash, mapping terminal states.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    pub(crate) fn digest(
        document: &Document,
        on_present: impl FnOnce(FileDigest) -> Result<Vec<Difference>>,
    ) -> Result<Vec<Difference>> {
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

    /// Probe one tree member path under its expanded base.
    pub(crate) fn probe_member(
        base: &Path,
        relative: &str,
        path: &Route,
        on_present: impl FnOnce(FileDigest) -> Result<Vec<Difference>>,
    ) -> Result<Vec<Difference>> {
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
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskOutcome::Absent,
                Err(error) => DiskOutcome::Unreadable {
                    fault: FilesystemError::from(error),
                },
            }
        } else if full.is_dir() {
            DiskOutcome::Blocked
        } else {
            Self::digest_path(&full)
        };
        Self::mapped(outcome, path, on_present)
    }

    /// Map one read outcome into entries, running the closure on presence.
    fn mapped<T>(
        outcome: DiskOutcome<T>,
        path: &Route,
        on_present: impl FnOnce(T) -> Result<Vec<Difference>>,
    ) -> Result<Vec<Difference>> {
        Ok(match outcome {
            DiskOutcome::Absent => vec![Difference::Missing { path: path.clone() }],
            DiskOutcome::Unreadable { fault } => vec![Difference::Unreadable {
                path: path.clone(),
                reason: Self::reason(&fault),
            }],
            DiskOutcome::Blocked => vec![Difference::Blocked { path: path.clone() }],
            DiskOutcome::Present(payload) => return on_present(payload),
        })
    }

    /// Expand one document destination or fail the run.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    fn expanded(document: &Document) -> Result<PathBuf> {
        document.path.expand().route(document.path.display())
    }

    /// Show one unreadable fault for difference lines.
    pub(crate) fn reason(fault: &FilesystemError) -> String {
        match fault {
            FilesystemError::FileUnknown { message } => message.clone(),
            known => known.to_string(),
        }
    }

    /// Resolve one expanded path through its symlink target.
    fn resolve_target(expanded: &Path) -> PathBuf {
        match std::fs::read_link(expanded) {
            Ok(link) => Self::join_target(expanded, &link),
            Err(_) => expanded.to_path_buf(),
        }
    }

    /// Read one regular path into bytes with its mode.
    fn read_path(path: &Path) -> DiskOutcome<FileBytes> {
        match std::fs::read(path) {
            Ok(bytes) => DiskOutcome::Present(FileBytes {
                bytes,
                mode: Self::read_mode(path),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskOutcome::Absent,
            Err(error) => DiskOutcome::Unreadable {
                fault: FilesystemError::from(error),
            },
        }
    }

    /// Digest one regular path into its hash with its mode.
    fn digest_path(path: &Path) -> DiskOutcome<FileDigest> {
        let mode = Self::read_mode(path);
        match std::fs::File::open(path) {
            Ok(mut file) => match Sha::read_with_size(&mut file) {
                Ok((sha, len)) => DiskOutcome::Present(FileDigest {
                    digest: Digest { sha, len },
                    mode,
                }),
                Err(error) => DiskOutcome::Unreadable {
                    fault: FilesystemError::from(error),
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiskOutcome::Absent,
            Err(error) => DiskOutcome::Unreadable {
                fault: FilesystemError::from(error),
            },
        }
    }

    /// Join a link target against the link parent.
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

    /// Read unix permission bits for one path.
    fn read_mode(path: &Path) -> Option<u32> {
        if path.is_symlink() {
            return None;
        }
        driver::fs::mode(path).ok()
    }
}
