//! Change lists for wanted, recorded, and disk states.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::str::from_utf8;
use std::sync::Arc;

use confit_core::error::ContextualResult;
use confit_model::document::{Document, StructuredFormat};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use serde::Serialize;

use crate::{
    change::{Change, ChangeKind, Comparability, Side},
    content::ContentDiff,
    entries::{DataExt, Entry, Rank},
    fingerprint::{DiskBody, DiskOutcome, body, parse_table},
    keys::{Table, flatten},
    lines::diff,
};

const DETAIL_BYTE_LIMIT: u64 = 100 * 1024 * 1024;

/// One change list kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    /// Wanted against recorded.
    Desired,
    /// Recorded against disk.
    Drift,
    /// Wanted against disk.
    Effective,
}

/// One named change list with owned manifests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    /// Comparison kind.
    pub kind: ViewKind,
    /// Changes in route order.
    pub changes: Vec<Change>,
    /// Wanted manifest.
    #[serde(skip_serializing)]
    pub wanted: Arc<Manifest>,
    /// Recorded manifest.
    #[serde(skip_serializing)]
    pub recorded: Arc<Manifest>,
}

enum SideBody {
    Missing,
    Bytes(Box<[u8]>),
    Unreadable,
}

impl View {
    /// Builds one view.
    pub fn new(
        kind: ViewKind,
        changes: Vec<Change>,
        wanted: Arc<Manifest>,
        recorded: Arc<Manifest>,
    ) -> Self {
        Self {
            kind,
            changes,
            wanted,
            recorded,
        }
    }

    /// Returns the change for one route.
    pub fn find(&self, path: &Route) -> Option<&Change> {
        self.changes.iter().find(|change| change.path == *path)
    }

    /// Returns the wanted else recorded document.
    pub fn find_document(&self, path: &Route) -> Option<&Document> {
        self.wanted
            .documents
            .iter()
            .find(|document| document.path == *path)
            .or_else(|| {
                self.recorded
                    .documents
                    .iter()
                    .find(|document| document.path == *path)
            })
    }

    /// Returns the rendered difference for one route.
    ///
    /// - Returns the opaque difference when a detailed
    ///   difference is unavailable.
    pub fn detail(&self, path: &Route) -> Option<ContentDiff> {
        let found = self.find(path)?;
        if !matches!(found.comparability, Comparability::Comparable) {
            return None;
        }
        if found.group.is_some() {
            return Some(found.opaque());
        }
        let (before_doc, after_doc) = match self.kind {
            ViewKind::Desired => (
                self.recorded
                    .documents
                    .iter()
                    .find(|document| document.path == *path),
                self.wanted
                    .documents
                    .iter()
                    .find(|document| document.path == *path),
            ),
            ViewKind::Drift => (
                self.recorded
                    .documents
                    .iter()
                    .find(|document| document.path == *path),
                None,
            ),
            ViewKind::Effective => (
                None,
                self.wanted
                    .documents
                    .iter()
                    .find(|document| document.path == *path),
            ),
        };
        let rank = [before_doc, after_doc]
            .into_iter()
            .flatten()
            .map(|document| document.data.rank())
            .max()?;
        if rank == Rank::Opaque {
            return Some(found.opaque());
        }
        let before_body = match Self::load_side(self.kind, true, path, before_doc, after_doc) {
            SideBody::Unreadable => return Some(found.opaque()),
            SideBody::Missing => None,
            SideBody::Bytes(found) => Some(found),
        };
        let after_body = match Self::load_side(self.kind, false, path, before_doc, after_doc) {
            SideBody::Unreadable => return Some(found.opaque()),
            SideBody::Missing => None,
            SideBody::Bytes(found) => Some(found),
        };
        if rank == Rank::Structured {
            let before_format = before_doc.and_then(|document| document.data.format());
            let after_format = after_doc.and_then(|document| document.data.format());
            let before_table = Self::table_of(before_body.as_deref(), before_format);
            let after_table = Self::table_of(after_body.as_deref(), after_format);
            if let (Some(left), Some(right)) = (before_table, after_table) {
                return Some(ContentDiff::Structured {
                    keys: flatten(&left, &right),
                });
            }
        }
        let before_text = Self::text_of(before_body.as_deref());
        let after_text = Self::text_of(after_body.as_deref());
        match (before_text, after_text) {
            (Some(left), Some(right)) => Some(ContentDiff::Text {
                lines: diff(left, right),
            }),
            _ => Some(found.opaque()),
        }
    }

    /// Builds the wanted against recorded view.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    pub(crate) fn desired(
        wanted_rows: &BTreeMap<Route, Entry>,
        recorded_rows: &BTreeMap<Route, Entry>,
        wanted: Arc<Manifest>,
        recorded: Arc<Manifest>,
    ) -> ContextualResult<Self> {
        let mut out = Vec::new();
        for path in union(wanted_rows, recorded_rows) {
            let before = recorded_rows.get(path);
            let after = wanted_rows.get(path);
            let unmanaged = before.is_some_and(|entry| entry.unmanaged)
                || after.is_some_and(|entry| entry.unmanaged);
            let group = after.or(before).and_then(|entry| entry.group.clone());
            if let Some(change) = Change::build(
                path,
                group,
                before.map(Side::from),
                after.map(Side::from),
                unmanaged,
            )? {
                out.push(change);
            }
        }
        Ok(Self::new(ViewKind::Desired, out, wanted, recorded))
    }

    /// Builds the recorded against disk view.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    pub(crate) fn drift(
        recorded_rows: &BTreeMap<Route, Entry>,
        disk: &BTreeMap<Route, DiskOutcome>,
        wanted: Arc<Manifest>,
        recorded: Arc<Manifest>,
    ) -> ContextualResult<Self> {
        let mut out = Vec::new();
        for (path, before) in recorded_rows {
            let change = match disk.get(path) {
                None => Change::build(
                    path,
                    before.group.clone(),
                    Some(Side::from(before)),
                    None,
                    before.unmanaged,
                )?,
                Some(DiskOutcome::Present(side)) => Change::build(
                    path,
                    before.group.clone(),
                    Some(Side::from(before)),
                    Some(*side),
                    before.unmanaged,
                )?,
                Some(DiskOutcome::Refused(reason)) => Some(Change::incomparable(
                    path,
                    before.group.clone(),
                    Some(Side::from(before)),
                    None,
                    ChangeKind::Change,
                    *reason,
                )?),
            };
            if let Some(found) = change {
                out.push(found);
            }
        }
        Ok(Self::new(ViewKind::Drift, out, wanted, recorded))
    }

    /// Builds the wanted against disk view.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    pub(crate) fn effective(
        wanted_rows: &BTreeMap<Route, Entry>,
        recorded_rows: &BTreeMap<Route, Entry>,
        disk: &BTreeMap<Route, DiskOutcome>,
        drifted: &BTreeSet<Route>,
        wanted: Arc<Manifest>,
        recorded: Arc<Manifest>,
    ) -> ContextualResult<Self> {
        let mut out = Vec::new();
        for path in union(wanted_rows, recorded_rows) {
            let wanted = wanted_rows.get(path);
            let recorded = recorded_rows.get(path);
            let seen = disk.get(path);
            let group = wanted.or(recorded).and_then(|entry| entry.group.clone());
            let change = match (wanted, seen) {
                (Some(entry), None) => {
                    Change::build(path, group, None, Some(Side::from(entry)), entry.unmanaged)?
                }
                (Some(entry), Some(DiskOutcome::Present(side))) => Change::build(
                    path,
                    group,
                    Some(*side),
                    Some(Side::from(entry)),
                    entry.unmanaged,
                )?,
                (Some(entry), Some(DiskOutcome::Refused(reason))) => Some(Change::incomparable(
                    path,
                    group,
                    None,
                    Some(Side::from(entry)),
                    ChangeKind::Change,
                    *reason,
                )?),
                (None, Some(DiskOutcome::Present(side))) if recorded.is_some() => Change::build(
                    path,
                    group,
                    Some(*side),
                    None,
                    recorded.is_some_and(|entry| entry.unmanaged),
                )?,
                (None, Some(DiskOutcome::Refused(reason))) if recorded.is_some() => Some(
                    Change::incomparable(path, group, None, None, ChangeKind::Removal, *reason)?,
                ),
                (None, _) => None,
            };
            if let Some(mut found) = change {
                found.drifted = drifted.contains(&found.path);
                out.push(found);
            }
        }
        Ok(Self::new(ViewKind::Effective, out, wanted, recorded))
    }

    fn load_side(
        view: ViewKind,
        before_side: bool,
        path: &Route,
        before_doc: Option<&Document>,
        after_doc: Option<&Document>,
    ) -> SideBody {
        let from_manifest = match view {
            ViewKind::Desired => true,
            ViewKind::Drift => before_side,
            ViewKind::Effective => !before_side,
        };
        if from_manifest {
            let document = if before_side { before_doc } else { after_doc };
            match document {
                None => SideBody::Missing,
                Some(found) => match found.render() {
                    Ok(bytes) if (bytes.len() as u64) <= DETAIL_BYTE_LIMIT => {
                        SideBody::Bytes(bytes.into_boxed_slice())
                    }
                    _ => SideBody::Unreadable,
                },
            }
        } else {
            match before_doc.or(after_doc).map(|probe| body(path, probe)) {
                Some(DiskBody::Bytes(bytes)) => SideBody::Bytes(bytes),
                Some(DiskBody::Absent) => SideBody::Missing,
                _ => SideBody::Unreadable,
            }
        }
    }

    fn table_of(body: Option<&[u8]>, format: Option<StructuredFormat>) -> Option<Table> {
        match (body, format) {
            (None, _) => Some(Table::new()),
            (Some(bytes), Some(format)) => parse_table(format, bytes),
            (Some(_), None) => None,
        }
    }

    fn text_of(body: Option<&[u8]>) -> Option<&str> {
        match body {
            None => Some(""),
            Some(bytes) => from_utf8(bytes).ok(),
        }
    }
}

impl Deref for View {
    type Target = [Change];

    /// Borrows the changes.
    fn deref(&self) -> &[Change] {
        &self.changes
    }
}

fn union<'a>(
    left: &'a BTreeMap<Route, Entry>,
    right: &'a BTreeMap<Route, Entry>,
) -> BTreeSet<&'a Route> {
    left.keys().chain(right.keys()).collect()
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt as _;

    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_model::document::{
        Data, Document, FileMode, LinkDocument, OpaqueDocument, RcData, RcEntry,
        StructuredDocument, StructuredFormat, TextDocument,
    };
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use crate::content::{ContentDiff, LineDiff, LineTag};
    use crate::diff::Diff;

    /// Builds one literal route under the given folder.
    fn literal(dir: &std::path::Path, name: &str) -> Route {
        Route::new(RouteBase::Literal, dir.join(name)).unwrap()
    }

    /// Builds one empty manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    /// Builds one text document with default mode.
    fn text(route: Route, content: &str) -> Document {
        Document::new(
            route,
            Data::Text(TextDocument {
                content: content.to_owned(),
                mode: FileMode::default(),
                unmanaged: false,
            }),
        )
    }

    /// Writes one disk file with fixed permission bits.
    fn write_file(path: &std::path::Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn detail_skips_incomparable_rows() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "file");
        std::fs::create_dir(dir.path().join("file")).unwrap();
        let previous = manifest(vec![text(route.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.detail(&route), None);
        assert_eq!(diff.effective.detail(&route), None);
    }
    #[test]
    fn opaque_detail_carries_the_hash_pair() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "blob");
        write_file(&dir.path().join("blob"), b"disk bytes");
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Opaque(OpaqueDocument {
                handle: ResourceRef::new(Sha::hash(b"pool bytes")),
                mode: FileMode::default(),
                unmanaged: false,
            }),
        )]);
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert_eq!(diff.effective.len(), 1);
        match diff.effective.detail(&route).unwrap() {
            ContentDiff::Opaque { before, after } => {
                assert_eq!(before, Some(Sha::hash(b"disk bytes")));
                assert_eq!(after, Some(Sha::hash(b"pool bytes")));
            }
            other => {
                panic!("expected an opaque pair, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_reports_nested_key_changes() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "config.json");
        let table = |pairs: &[(&str, serde_json::Value)]| {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect()
        };
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: table(&[
                    ("same", serde_json::json!(1)),
                    ("nested", serde_json::json!({"keep": true, "edit": 2})),
                    ("fresh", serde_json::json!("new")),
                ]),
            }),
        )]);
        let previous = manifest(vec![Document::new(
            route.clone(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: table(&[
                    ("same", serde_json::json!(1)),
                    ("nested", serde_json::json!({"keep": true, "edit": 1})),
                    ("stale", serde_json::json!("old")),
                ]),
            }),
        )]);
        let diff = Diff::build(desired, previous).unwrap();
        match diff.desired.detail(&route).unwrap() {
            ContentDiff::Structured { keys } => {
                let names: Vec<&str> = keys.iter().map(|found| found.key.as_str()).collect();
                assert_eq!(names, vec!["fresh", "nested.edit", "stale"]);
                assert_eq!(keys[1].before, Some(serde_json::json!(1)));
                assert_eq!(keys[1].after, Some(serde_json::json!(2)));
            }
            other => {
                panic!("expected structured keys, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_reports_text_lines_with_tags() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "note");
        write_file(&dir.path().join("note"), b"one\ntwo\n");
        let previous = manifest(vec![text(route.clone(), "one\nthree\n")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        match diff.drift.detail(&route).unwrap() {
            ContentDiff::Text { lines } => {
                assert_eq!(
                    lines,
                    vec![
                        LineDiff {
                            tag: LineTag::Removed,
                            text: "three".to_owned(),
                        },
                        LineDiff {
                            tag: LineTag::Added,
                            text: "two".to_owned(),
                        },
                    ]
                );
            }
            other => {
                panic!("expected text lines, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_diffs_links_as_text_over_targets() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "current");
        std::os::unix::fs::symlink("old-target", dir.path().join("current")).unwrap();
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Link(LinkDocument {
                target: Route::new(RouteBase::Literal, "new-target").unwrap(),
            }),
        )]);
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert_eq!(diff.effective.len(), 1);
        match diff.effective.detail(&route).unwrap() {
            ContentDiff::Text { lines } => {
                assert_eq!(lines.len(), 2);
                assert_eq!(lines[0].tag, LineTag::Removed);
                assert_eq!(lines[0].text, "old-target");
                assert_eq!(lines[1].tag, LineTag::Added);
                assert_eq!(lines[1].text, "new-target");
            }
            other => {
                panic!("expected link text lines, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_falls_back_to_text_when_disk_skips_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "config.json");
        let mut data = BTreeMap::new();
        data.insert("alpha".to_owned(), serde_json::json!(1));
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data,
            }),
        )]);
        write_file(&dir.path().join("config.json"), b"not json {{{");
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert_eq!(diff.effective.len(), 1);
        match diff.effective.detail(&route).unwrap() {
            ContentDiff::Text { lines } => {
                assert!(!lines.is_empty())
            }
            other => {
                panic!("expected a text fallback, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_reports_rc_changes_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "shell");
        let entry = |value: &str| RcEntry::Env {
            name: "EDITOR".to_owned(),
            value: value.to_owned(),
            when: None,
        };
        let previous = manifest(vec![Document::new(
            route.clone(),
            Data::Rc(RcData::new(vec![entry("nano")], Vec::new(), Vec::new())),
        )]);
        write_file(&dir.path().join("shell"), b"export EDITOR=vim\n");
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        match diff.drift.detail(&route).unwrap() {
            ContentDiff::Text { lines } => {
                assert_eq!(lines.len(), 2)
            }
            other => {
                panic!("expected rc text lines, found: {other:?}")
            }
        }
    }
    #[test]
    fn detail_skips_unknown_paths() {
        let dir = tempfile::tempdir().unwrap();
        let diff = Diff::build(manifest(Vec::new()), manifest(Vec::new())).unwrap();
        assert_eq!(diff.effective.detail(&literal(dir.path(), "absent")), None);
    }
}
