//! Streaming disk states.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::error::{ContextualResult, DocumentError, Require, WithContext, contextual};
use confit_core::sha::Sha;
use confit_driver::fs::mode;
use confit_model::document::{Data, Document, FileMode, StructuredDocument, StructuredFormat};
use confit_model::routes::Route;

use crate::{
    change::{IncomparableReason, Side},
    entries::Entry,
};

const DETAIL_BYTE_LIMIT: u64 = 100 * 1024 * 1024;

/// One disk lookup result for an entry route.
pub(crate) enum DiskOutcome {
    /// Present path.
    Present(Side),
    /// Refused path.
    Refused(IncomparableReason),
}

/// One disk body state for detail loads.
pub(crate) enum DiskBody {
    /// Missing or unreadable path.
    Absent,
    /// Body above the detail limit.
    OverLimit,
    /// Transient body bytes.
    Bytes(Box<[u8]>),
}

impl Entry {
    /// Returns the disk outcome for one entry route.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a
    ///   base folder is missing.
    fn side(&self) -> ContextualResult<Option<DiskOutcome>> {
        if self.group.is_some() {
            return self.member_side();
        }
        if self.link {
            return self.link_side();
        }
        let expanded = self
            .path
            .resolve()
            .ok_or_else(|| {
                contextual(DocumentError::ParseFailed {
                    require: Require::BaseFolder,
                })
            })
            .with_document(self.path.display())?;
        let target = resolve_target(&expanded);
        Ok(self.file_side(&target))
    }

    /// Returns the disk outcome for one tree member path.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a
    ///   base folder is missing.
    fn member_side(&self) -> ContextualResult<Option<DiskOutcome>> {
        if self.group.is_none() {
            return Ok(None);
        }
        let full = self
            .path
            .resolve()
            .ok_or_else(|| {
                contextual(DocumentError::ParseFailed {
                    require: Require::BaseFolder,
                })
            })
            .with_document(self.path.display())?;
        if full.is_symlink() {
            match std::fs::read_link(&full) {
                Ok(link) => {
                    return Ok(Some(DiskOutcome::Present(Side::new(
                        Sha::hash(link.as_os_str().as_encoded_bytes()),
                        FileMode::default(),
                    ))));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(None);
                }
                Err(error) => {
                    return Ok(Some(DiskOutcome::Refused(reason_for(&error, &full))));
                }
            }
        }
        Ok(self.file_side(&full))
    }

    /// Returns the disk outcome for one link destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] when a
    ///   base folder is missing.
    fn link_side(&self) -> ContextualResult<Option<DiskOutcome>> {
        let expanded = self
            .path
            .resolve()
            .ok_or_else(|| {
                contextual(DocumentError::ParseFailed {
                    require: Require::BaseFolder,
                })
            })
            .with_document(self.path.display())?;
        match std::fs::read_link(&expanded) {
            Ok(link) => Ok(Some(DiskOutcome::Present(Side::new(
                Sha::hash(link.as_os_str().as_encoded_bytes()),
                FileMode::default(),
            )))),
            Err(_) if expanded.is_dir() => {
                Ok(Some(DiskOutcome::Refused(IncomparableReason::DirThere)))
            }
            Err(_) => Ok(self.file_side(&expanded)),
        }
    }

    /// Returns the disk outcome for one regular destination.
    fn file_side(&self, path: &Path) -> Option<DiskOutcome> {
        if path.is_dir() {
            return Some(DiskOutcome::Refused(IncomparableReason::DirThere));
        }
        if is_special(path) {
            return Some(DiskOutcome::Refused(
                IncomparableReason::BlockedBySpecialFile,
            ));
        }
        match self.format {
            Some(format) => self.canonical_side(path, format),
            None => self.stream_side(path),
        }
    }

    /// Returns the canonical disk outcome for one destination.
    fn canonical_side(&self, path: &Path, format: StructuredFormat) -> Option<DiskOutcome> {
        let bytes = match load_limited(path, DETAIL_BYTE_LIMIT) {
            Ok(Some(found)) => found,
            Ok(None) => return self.stream_side(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return None;
            }
            Err(error) => {
                return Some(DiskOutcome::Refused(reason_for(&error, path)));
            }
        };
        let sha = match parse_table(format, &bytes) {
            Some(table) => synthetic_hash(self, format, table, &bytes),
            None => Sha::hash(&bytes),
        };
        Some(DiskOutcome::Present(Side::new(sha, read_mode(path))))
    }

    /// Returns the streaming disk outcome for one path.
    fn stream_side(&self, path: &Path) -> Option<DiskOutcome> {
        let mode = read_mode(path);
        match std::fs::File::open(path) {
            Ok(mut file) => match Sha::read_with_size(&mut file) {
                Ok((sha, _)) => Some(DiskOutcome::Present(Side::new(sha, mode))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some(DiskOutcome::Refused(reason_for(&error, path))),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(DiskOutcome::Refused(reason_for(&error, path))),
        }
    }
}

/// Reads the disk state for each entry route.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base
///   folder is missing.
pub(crate) fn read<'a>(
    rows: impl IntoIterator<Item = &'a Entry>,
) -> ContextualResult<BTreeMap<Route, DiskOutcome>> {
    let mut out = BTreeMap::new();
    for entry in rows {
        if let Some(found) = entry.side()? {
            out.insert(entry.path.clone(), found);
        }
    }
    Ok(out)
}

/// Loads one disk body under the detail limit.
pub(crate) fn body(route: &Route, document: &Document) -> DiskBody {
    if matches!(document.data, Data::Link(_)) {
        return link_body(route);
    }
    let Some(expanded) = route.resolve() else {
        return DiskBody::Absent;
    };
    let target = resolve_target(&expanded);
    if target.is_dir() {
        return DiskBody::Absent;
    }
    file_body(&target)
}

/// Maps one IO error to its incomparable reason.
fn reason_for(error: &std::io::Error, path: &Path) -> IncomparableReason {
    if is_special(path) {
        return IncomparableReason::BlockedBySpecialFile;
    }
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => IncomparableReason::Unreadable,
        std::io::ErrorKind::NotADirectory => IncomparableReason::BlockedByDir,
        std::io::ErrorKind::IsADirectory => IncomparableReason::DirThere,
        _ => {
            if path.is_dir() {
                IncomparableReason::DirThere
            } else {
                IncomparableReason::Unreadable
            }
        }
    }
}

/// Returns whether one path is a socket, fifo, or device.
fn is_special(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt as _;

    match std::fs::symlink_metadata(path) {
        Ok(facts) => {
            let kind = facts.file_type();
            kind.is_socket() || kind.is_fifo() || kind.is_block_device() || kind.is_char_device()
        }
        Err(_) => false,
    }
}

/// Returns the disk body for one link destination.
fn link_body(route: &Route) -> DiskBody {
    let Some(expanded) = route.resolve() else {
        return DiskBody::Absent;
    };
    match std::fs::read_link(&expanded) {
        Ok(link) => DiskBody::Bytes(link.as_os_str().as_encoded_bytes().into()),
        Err(_) if expanded.is_dir() => DiskBody::Absent,
        Err(_) => file_body(&expanded),
    }
}

/// Returns the disk body for one regular path.
fn file_body(path: &Path) -> DiskBody {
    match load_limited(path, DETAIL_BYTE_LIMIT) {
        Ok(Some(bytes)) => DiskBody::Bytes(bytes),
        Ok(None) => DiskBody::OverLimit,
        Err(_) => DiskBody::Absent,
    }
}

/// Streams one file under the limit.
fn load_limited(path: &Path, limit: u64) -> std::io::Result<Option<Box<[u8]>>> {
    use std::io::Read as _;

    let file = std::fs::File::open(path)?;
    let mut capped = file.take(limit.saturating_add(1));
    let mut out = Vec::new();
    capped.read_to_end(&mut out)?;
    if out.len() as u64 > limit {
        return Ok(None);
    }
    Ok(Some(out.into_boxed_slice()))
}

/// Parses disk bytes into a table.
pub(crate) fn parse_table(
    format: StructuredFormat,
    bytes: &[u8],
) -> Option<BTreeMap<String, serde_json::Value>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value: serde_json::Value = match format {
        StructuredFormat::Json => serde_json::from_str(text).ok()?,
        StructuredFormat::Toml => toml::from_str(text).ok()?,
        StructuredFormat::Yaml => noyalib::from_str(text).ok()?,
    };
    match value {
        serde_json::Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

/// Hashes one parsed disk table through the document render.
fn synthetic_hash(
    entry: &Entry,
    format: StructuredFormat,
    table: BTreeMap<String, serde_json::Value>,
    bytes: &[u8],
) -> Sha {
    let probe = Document::new(
        entry.path.clone(),
        Data::Structured(StructuredDocument {
            format,
            data: table,
        }),
    );
    match probe.hash() {
        Ok(sha) => sha,
        Err(_) => Sha::hash(bytes),
    }
}

/// Resolves one expanded path to its symlink target.
fn resolve_target(expanded: &Path) -> PathBuf {
    match std::fs::read_link(expanded) {
        Ok(link) => join_target(expanded, &link),
        Err(_) => expanded.to_path_buf(),
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

/// Reads the permission bits, defaulted for links and failures.
fn read_mode(path: &Path) -> FileMode {
    if path.is_symlink() {
        return FileMode::default();
    }
    match mode(path) {
        Ok(bits) => FileMode::new(bits),
        Err(_) => FileMode::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_limited_round_trips_small_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body.txt");
        std::fs::write(&path, b"payload").unwrap();
        match load_limited(&path, 64) {
            Ok(Some(found)) => assert_eq!(&*found, b"payload"),
            other => {
                panic!("expected the body bytes, found: {}", other.is_ok())
            }
        }
    }

    #[test]
    fn load_limited_skips_bodies_above_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.txt");
        std::fs::write(&path, b"123456789").unwrap();
        match load_limited(&path, 4) {
            Ok(None) => {}
            _ => panic!("bodies above the limit stay absent"),
        }
    }

    #[test]
    fn load_limited_reports_missing_paths_as_not_found() {
        let dir = tempfile::tempdir().unwrap();
        match load_limited(&dir.path().join("absent.txt"), 64) {
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound)
            }
            _ => panic!("missing paths fail with not found"),
        }
    }
}
