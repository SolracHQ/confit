//! Wanted and recorded comparison with three views.

use std::sync::Arc;

use confit_core::error::ContextualResult;
use confit_model::manifest::Manifest;
use serde::Serialize;

use crate::{entries::rows, fingerprint::read, view::View};

/// Wanted, recorded, and disk comparison with three views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// Wanted manifest.
    pub wanted: Arc<Manifest>,
    /// Recorded manifest.
    pub recorded: Arc<Manifest>,
    /// Wanted against recorded.
    pub desired: View,
    /// Recorded against disk.
    pub drift: View,
    /// Wanted against disk.
    pub effective: View,
}

impl Diff {
    /// Builds the comparison with three views.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    /// - [`confit_core::error::DocumentError::RenderFailed`]
    ///   when a manifest render fails.
    pub fn build(wanted: Manifest, recorded: Manifest) -> ContextualResult<Self> {
        let wanted = Arc::new(wanted);
        let recorded = Arc::new(recorded);
        let wanted_rows = rows(wanted.as_ref())?;
        let recorded_rows = rows(recorded.as_ref())?;
        let disk = read(wanted_rows.values().chain(recorded_rows.values()))?;
        Ok(Self {
            desired: View::desired(
                &wanted_rows,
                &recorded_rows,
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            )?,
            drift: View::drift(
                &recorded_rows,
                &disk,
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            )?,
            effective: View::effective(
                &wanted_rows,
                &recorded_rows,
                &disk,
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            )?,
            wanted,
            recorded,
        })
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::net::UnixListener;

    use confit_core::error::{DocumentError, ErrorCause};
    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_model::document::{
        Data, Document, FileMode, MemberKey, OpaqueDocument, StructuredDocument, StructuredFormat,
        TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use super::Diff;
    use crate::change::{Change, ChangeKind, Comparability, IncomparableReason, Side};
    use crate::content::ContentDiff;

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

    /// Finds one change by route display text.
    fn change<'a>(view: &'a [Change], route: &str) -> &'a Change {
        view.iter()
            .find(|found| found.path.display() == route)
            .unwrap()
    }

    #[test]
    fn desired_view_reports_additions_changes_and_removals() {
        let dir = tempfile::tempdir().unwrap();
        let desired = manifest(vec![
            text(literal(dir.path(), "fresh"), "new"),
            text(literal(dir.path(), "edited"), "after"),
            text(literal(dir.path(), "same"), "kept"),
        ]);
        let previous = manifest(vec![
            text(literal(dir.path(), "edited"), "before"),
            text(literal(dir.path(), "same"), "kept"),
            text(literal(dir.path(), "stale"), "old"),
        ]);
        let diff = Diff::build(desired, previous).unwrap();
        assert_eq!(diff.desired.len(), 3);
        assert_eq!(
            change(&diff.desired, &literal(dir.path(), "fresh").display()).kind,
            ChangeKind::Addition
        );
        assert_eq!(
            change(&diff.desired, &literal(dir.path(), "edited").display()).kind,
            ChangeKind::Change
        );
        assert_eq!(
            change(&diff.desired, &literal(dir.path(), "stale").display()).kind,
            ChangeKind::Removal
        );
    }
    #[test]
    fn empty_previous_reports_an_addition() {
        let dir = tempfile::tempdir().unwrap();
        let desired = manifest(vec![text(literal(dir.path(), "fresh"), "new")]);
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert_eq!(diff.desired.len(), 1);
        assert_eq!(diff.desired[0].kind, ChangeKind::Addition);
        assert!(diff.drift.is_empty(), "empty recorded means empty drift");
        assert_eq!(diff.effective.len(), 1);
    }
    #[test]
    fn changes_carry_the_resolved_host_path() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "fresh");
        let diff = Diff::build(
            manifest(vec![text(route.clone(), "new")]),
            manifest(Vec::new()),
        )
        .unwrap();
        assert_eq!(diff.effective.len(), 1);
        let found = &diff.effective[0];
        assert_eq!(found.expanded, route.resolve().unwrap());
        assert_eq!(found.expanded, dir.path().join("fresh"));
        std::fs::create_dir(dir.path().join("blocked")).unwrap();
        let blocked = literal(dir.path(), "blocked");
        let previous = manifest(vec![text(blocked.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let rugged = &diff.drift[0];
        assert_eq!(rugged.expanded, blocked.resolve().unwrap());
        assert_eq!(rugged.expanded, dir.path().join("blocked"));
    }
    #[test]
    fn drift_reports_a_removal_and_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let gone = literal(dir.path(), "gone");
        let edited = literal(dir.path(), "edited");
        let kept = literal(dir.path(), "kept");
        write_file(&dir.path().join("edited"), b"disk");
        write_file(&dir.path().join("kept"), b"kept");
        write_file(&dir.path().join("stray"), b"disk only");
        let previous = manifest(vec![
            text(gone.clone(), "old"),
            text(edited.clone(), "recorded"),
            text(kept.clone(), "kept"),
        ]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 2);
        assert_eq!(
            change(&diff.drift, &gone.display()).kind,
            ChangeKind::Removal
        );
        assert_eq!(
            change(&diff.drift, &edited.display()).kind,
            ChangeKind::Change
        );
        assert!(
            diff.drift.iter().all(|found| found
                .path
                .display()
                != dir.path().join("stray").display().to_string()),
            "drift skips disk-only paths"
        );
    }
    #[test]
    fn effective_view_drives_apply() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = literal(dir.path(), "fresh");
        let edited = literal(dir.path(), "edited");
        let retired = literal(dir.path(), "retired");
        write_file(&dir.path().join("edited"), b"disk");
        write_file(&dir.path().join("retired"), b"recorded");
        write_file(&dir.path().join("stray"), b"disk only");
        let desired = manifest(vec![
            text(fresh.clone(), "new"),
            text(edited.clone(), "wanted"),
        ]);
        let previous = manifest(vec![
            text(edited.clone(), "recorded"),
            text(retired.clone(), "recorded"),
        ]);
        let diff = Diff::build(desired, previous).unwrap();
        assert_eq!(diff.effective.len(), 3);
        let addition = change(&diff.effective, &fresh.display());
        assert_eq!(addition.kind, ChangeKind::Addition);
        assert_eq!(addition.before, None);
        let update = change(&diff.effective, &edited.display());
        assert_eq!(update.kind, ChangeKind::Change);
        assert_eq!(
            update.after,
            Some(Side::new(Sha::hash(b"wanted"), FileMode::default()))
        );
        assert_eq!(
            update.before,
            Some(Side::new(Sha::hash(b"disk"), FileMode::default()))
        );
        let removal = change(&diff.effective, &retired.display());
        assert_eq!(removal.kind, ChangeKind::Removal);
        assert_eq!(removal.after, None);
        assert_eq!(
            removal.before,
            Some(Side::new(Sha::hash(b"recorded"), FileMode::default()))
        );
    }
    #[test]
    fn reformatted_json_reports_the_desired_addition() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "config.json");
        let mut data = BTreeMap::new();
        data.insert("alpha".to_owned(), serde_json::json!([1, 2]));
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data,
            }),
        )]);
        write_file(&dir.path().join("config.json"), b"{  \"alpha\" : [1, 2] }");
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert!(
            diff.effective.is_empty(),
            "formatting changes leave the view empty"
        );
        assert!(diff.drift.is_empty(), "empty recorded means empty drift");
        assert_eq!(diff.desired.len(), 1, "the wanted row still surfaces");
        assert_eq!(diff.desired[0].kind, ChangeKind::Addition);
    }
    #[test]
    fn commented_toml_reports_the_desired_addition() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "config.toml");
        let mut data = BTreeMap::new();
        data.insert("alpha".to_owned(), serde_json::json!(1));
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Toml,
                data,
            }),
        )]);
        write_file(
            &dir.path().join("config.toml"),
            b"# remark\nalpha   =   1\n",
        );
        let diff = Diff::build(desired, manifest(Vec::new())).unwrap();
        assert!(diff.effective.is_empty(), "comments leave the view empty");
    }
    #[test]
    fn mode_only_difference_is_a_change_with_both_modes() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "script");
        write_file(&dir.path().join("script"), b"kept");
        std::fs::set_permissions(
            dir.path().join("script"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let previous = manifest(vec![text(route.clone(), "kept")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o644)))
        );
        assert_eq!(
            found.after,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o600)))
        );
    }
    #[test]
    fn unmanaged_documents_check_existence_only() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "marker");
        write_file(&dir.path().join("marker"), b"other bytes");
        let previous = manifest(vec![Document::new(
            route.clone(),
            Data::Text(TextDocument {
                content: "recorded".to_owned(),
                mode: FileMode::default(),
                unmanaged: true,
            }),
        )]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert!(diff.drift.is_empty(), "content leaves drift empty");
        std::fs::remove_file(dir.path().join("marker")).unwrap();
        let diff = Diff::build(manifest(Vec::new()), manifest(Vec::new())).unwrap();
        assert!(diff.drift.is_empty(), "empty recorded means empty drift");
        let previous = manifest(vec![Document::new(
            route,
            Data::Text(TextDocument {
                content: "recorded".to_owned(),
                mode: FileMode::default(),
                unmanaged: true,
            }),
        )]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        assert_eq!(diff.drift[0].kind, ChangeKind::Removal);
    }
    #[test]
    fn tree_members_flatten_to_grouped_opaques() {
        let dir = tempfile::tempdir().unwrap();
        let tree = literal(dir.path(), "shared");
        std::fs::create_dir(dir.path().join("shared")).unwrap();
        write_file(&dir.path().join("shared/one.txt"), b"changed");
        write_file(&dir.path().join("shared/two.txt"), b"kept");
        let handle = |bytes: &[u8]| ResourceRef::new(Sha::hash(bytes));
        let previous = manifest(vec![Document::new(
            tree.clone(),
            Data::Tree(TreeDocument {
                members: [
                    (
                        MemberKey::new("one.txt".to_owned()).unwrap(),
                        TreeMember::Handle(handle(b"recorded")),
                    ),
                    (
                        MemberKey::new("two.txt".to_owned()).unwrap(),
                        TreeMember::Handle(handle(b"kept")),
                    ),
                ]
                .into_iter()
                .collect(),
            }),
        )]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.path, tree.join("one.txt"));
        assert_eq!(found.group, Some(tree.clone()));
        assert_eq!(found.kind, ChangeKind::Change);
        let detail = diff.drift.detail(&tree.join("one.txt")).unwrap();
        match detail {
            ContentDiff::Opaque { before, after } => {
                assert_eq!(before, Some(Sha::hash(b"recorded")));
                assert_eq!(after, Some(Sha::hash(b"changed")));
            }
            other => {
                panic!("expected an opaque pair, found: {other:?}")
            }
        }
    }
    #[test]
    fn directory_on_disk_is_an_incomparable_row() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "file");
        std::fs::create_dir(dir.path().join("file")).unwrap();
        let previous = manifest(vec![text(route.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.comparability,
            Comparability::Incomparable(IncomparableReason::DirThere)
        );
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"recorded"), FileMode::default()))
        );
        assert_eq!(found.after, None);
    }
    #[test]
    fn unreadable_disk_is_an_incomparable_row() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "locked");
        write_file(&dir.path().join("locked"), b"recorded");
        std::fs::set_permissions(
            dir.path().join("locked"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let previous = manifest(vec![text(route.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.comparability,
            Comparability::Incomparable(IncomparableReason::Unreadable)
        );
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"recorded"), FileMode::default()))
        );
        assert_eq!(found.after, None);
    }
    #[test]
    fn blocked_ancestor_is_an_incomparable_row() {
        let dir = tempfile::tempdir().unwrap();
        write_file(&dir.path().join("blocker"), b"file");
        let route = literal(dir.path(), "blocker/child");
        let previous = manifest(vec![text(route.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.comparability,
            Comparability::Incomparable(IncomparableReason::BlockedByDir)
        );
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"recorded"), FileMode::default()))
        );
        assert_eq!(found.after, None);
    }
    #[test]
    fn special_file_on_disk_is_an_incomparable_row() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "socket");
        let held = UnixListener::bind(dir.path().join("socket")).unwrap();
        let previous = manifest(vec![text(route.clone(), "recorded")]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        drop(held);
        assert_eq!(diff.drift.len(), 1);
        let found = &diff.drift[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.comparability,
            Comparability::Incomparable(IncomparableReason::BlockedBySpecialFile)
        );
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"recorded"), FileMode::default()))
        );
        assert_eq!(found.after, None);
    }
    #[test]
    fn build_fails_on_unrenderable_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let document = Document::new(
            literal(dir.path(), "broken.toml"),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Toml,
                data: [("empty".to_owned(), serde_json::Value::Null)]
                    .into_iter()
                    .collect(),
            }),
        );
        let error = match Diff::build(manifest(vec![document]), manifest(Vec::new())) {
            Ok(_) => panic!("the broken manifest builds"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error.cause(),
                ErrorCause::DocumentError(DocumentError::RenderFailed)
            ),
            "the broken manifest fails the execution: {error}"
        );
    }
    #[test]
    fn refused_reads_emit_incomparable_rows_with_reasons() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("dir-there")).unwrap();
        write_file(&dir.path().join("blocker"), b"file");
        let held = UnixListener::bind(dir.path().join("special")).unwrap();
        write_file(&dir.path().join("locked"), b"recorded");
        std::fs::set_permissions(
            dir.path().join("locked"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let previous = manifest(vec![
            text(literal(dir.path(), "dir-there"), "recorded"),
            text(literal(dir.path(), "blocker/child"), "recorded"),
            text(literal(dir.path(), "special"), "recorded"),
            text(literal(dir.path(), "locked"), "recorded"),
        ]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        drop(held);
        assert_eq!(diff.drift.len(), 4);
        for (name, reason) in [
            ("dir-there", IncomparableReason::DirThere),
            ("blocker/child", IncomparableReason::BlockedByDir),
            ("special", IncomparableReason::BlockedBySpecialFile),
            ("locked", IncomparableReason::Unreadable),
        ] {
            let found = change(&diff.drift, &literal(dir.path(), name).display());
            assert_eq!(
                found.kind,
                ChangeKind::Change,
                "the {name} row keeps its kind"
            );
            assert_eq!(
                found.comparability,
                Comparability::Incomparable(reason),
                "the {name} row keeps its reason"
            );
            assert_eq!(
                found.before,
                Some(Side::new(Sha::hash(b"recorded"), FileMode::default())),
                "the {name} row keeps its recorded side"
            );
            assert_eq!(found.after, None);
        }
    }
    #[test]
    fn opaque_content_and_mode_change_carries_both_hashes_and_modes() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "blob");
        let opaque = |bytes: &[u8], mode: u32| {
            Document::new(
                route.clone(),
                Data::Opaque(OpaqueDocument {
                    handle: ResourceRef::new(Sha::hash(bytes)),
                    mode: FileMode::new(mode),
                    unmanaged: false,
                }),
            )
        };
        let diff = Diff::build(
            manifest(vec![opaque(b"after", 0o755)]),
            manifest(vec![opaque(b"before", 0o644)]),
        )
        .unwrap();
        assert_eq!(diff.desired.len(), 1);
        let found = &diff.desired[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"before"), FileMode::new(0o644)))
        );
        assert_eq!(
            found.after,
            Some(Side::new(Sha::hash(b"after"), FileMode::new(0o755)))
        );
    }
    #[test]
    fn opaque_mode_only_change_keeps_same_hash_with_new_modes() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "blob");
        let opaque = |mode: u32| {
            Document::new(
                route.clone(),
                Data::Opaque(OpaqueDocument {
                    handle: ResourceRef::new(Sha::hash(b"kept")),
                    mode: FileMode::new(mode),
                    unmanaged: false,
                }),
            )
        };
        let diff =
            Diff::build(manifest(vec![opaque(0o755)]), manifest(vec![opaque(0o644)])).unwrap();
        assert_eq!(diff.desired.len(), 1);
        let found = &diff.desired[0];
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o644)))
        );
        assert_eq!(
            found.after,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o755)))
        );
    }
    #[test]
    fn tree_member_mode_only_change_keeps_same_hash_with_new_modes() {
        let dir = tempfile::tempdir().unwrap();
        let tree = literal(dir.path(), "shared");
        let handle = ResourceRef::new(Sha::hash(b"kept"));
        let members = |member: TreeMember| {
            Document::new(
                tree.clone(),
                Data::Tree(TreeDocument {
                    members: [(MemberKey::new("one.txt".to_owned()).unwrap(), member)]
                        .into_iter()
                        .collect(),
                }),
            )
        };
        let diff = Diff::build(
            manifest(vec![members(TreeMember::Full {
                handle,
                mode: FileMode::new(0o755),
            })]),
            manifest(vec![members(TreeMember::Handle(handle))]),
        )
        .unwrap();
        assert_eq!(diff.desired.len(), 1);
        let found = &diff.desired[0];
        assert_eq!(found.path, tree.join("one.txt"));
        assert_eq!(found.kind, ChangeKind::Change);
        assert_eq!(
            found.before,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o644)))
        );
        assert_eq!(
            found.after,
            Some(Side::new(Sha::hash(b"kept"), FileMode::new(0o755)))
        );
    }
}
