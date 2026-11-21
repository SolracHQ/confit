//! Destination writes driven by effective rows.

use std::path::{Path, PathBuf};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    ContextualCause, EvalError, Terminal, TerminalResult, WithContext, contextual, error,
};
use confit_differ::{
    change::{Change, ChangeKind, Comparability},
    view::View,
};
use confit_driver as driver;
use confit_events::TaskReporter;
use confit_events::protocol::{Applied, ApplyOutcome, RowUpdate};
use confit_model::document::{Data, Document, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;

/// One write target for an effective row.
enum Target<'a> {
    /// One whole document.
    Whole(&'a Document),
    /// One member of a tree document.
    Member(&'a TreeMember),
}

/// The writer for document destinations.
pub struct Writer {
    /// The trusted roots and content caches.
    pub stores: Stores,
}

impl Writer {
    /// Builds a writer.
    pub fn new(stores: Stores) -> Self {
        Self { stores }
    }

    /// Returns the row count for the whole apply.
    pub fn apply_total(view: &View) -> usize {
        view.len()
    }

    /// Writes and removes the effective rows.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a row
    ///   references a missing document.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when a write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when a removal fails.
    pub fn apply(&self, view: &View, reporter: &TaskReporter) -> (Applied, TerminalResult<()>) {
        let mut applied = Applied {
            added: 0,
            changed: 0,
            removed: 0,
            changes: Vec::new(),
        };
        let total = view.len();
        let mut completed = 0;
        for change in view.iter() {
            match self.apply_row(view, change, &mut applied) {
                Ok(Some((path, outcome))) => {
                    completed += 1;
                    Self::emit_apply(reporter, &path, outcome, completed, total);
                }
                Ok(None) => {}
                Err(error) => {
                    completed += 1;
                    applied.push(&change.expanded, ApplyOutcome::Failed);
                    Self::emit_failure(reporter, change, completed, total);
                    return (applied, Err(error));
                }
            }
        }
        (applied, Ok(()))
    }

    /// Applies one effective row and returns its write outcome.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a row
    ///   references a missing document.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when a write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when a removal fails.
    fn apply_row(
        &self,
        view: &View,
        change: &Change,
        applied: &mut Applied,
    ) -> TerminalResult<Option<(PathBuf, ApplyOutcome)>> {
        if matches!(change.comparability, Comparability::Incomparable(_)) {
            self.attempt_change(view, change, applied)?;
            return Ok(None);
        }
        match change.kind {
            ChangeKind::Addition | ChangeKind::Change => {
                let (path, outcome) = self.write_change(view, change)?;
                if outcome == ApplyOutcome::Added {
                    applied.added += 1;
                } else {
                    applied.changed += 1;
                }
                applied.push(&path, outcome);
                Ok(Some((path, outcome)))
            }
            ChangeKind::Removal => {
                let Some(path) = self.remove_change(change)? else {
                    return Ok(None);
                };
                applied.removed += 1;
                applied.push(&path, ApplyOutcome::Removed);
                Ok(Some((path, ApplyOutcome::Removed)))
            }
        }
    }

    /// Writes one addition or change row and returns its outcome.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the row
    ///   references a missing document.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the write fails.
    fn write_change(
        &self,
        view: &View,
        change: &Change,
    ) -> TerminalResult<(PathBuf, ApplyOutcome)> {
        let Some(found) = target(view, change) else {
            return error(
                ErrorKind::Evaluate,
                contextual(EvalError::ChangedReferencesMissing)
                    .with_document(change.path.display()),
            );
        };
        let path = self.write_target(change, found)?;
        let outcome = match change.kind {
            ChangeKind::Addition => ApplyOutcome::Added,
            ChangeKind::Change => ApplyOutcome::Changed,
            ChangeKind::Removal => ApplyOutcome::Added,
        };
        Ok((path, outcome))
    }

    /// Removes one removal row and reports whether a file left.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the removal fails.
    fn remove_change(&self, change: &Change) -> TerminalResult<Option<PathBuf>> {
        let (expanded, removed) = self.remove_path(change)?;
        if removed {
            Ok(Some(expanded))
        } else {
            Ok(None)
        }
    }

    /// Attempts one incomparable row without an update.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when a write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when a removal fails.
    fn attempt_change(
        &self,
        view: &View,
        change: &Change,
        applied: &mut Applied,
    ) -> TerminalResult<()> {
        match target(view, change) {
            Some(found) => {
                let path = self.write_target(change, found)?;
                applied.added += 1;
                applied.push(&path, ApplyOutcome::Added);
            }
            None => {
                let (expanded, removed) = self.remove_path(change)?;
                if removed {
                    applied.removed += 1;
                    applied.push(&expanded, ApplyOutcome::Removed);
                }
            }
        }
        Ok(())
    }

    /// Writes one resolved target to its row path.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the write fails.
    fn write_target(&self, change: &Change, found: Target<'_>) -> TerminalResult<PathBuf> {
        match found {
            Target::Whole(document) => self.write_document(document, &change.expanded),
            Target::Member(member) => {
                let expanded = change.expanded.clone();
                self.write_member(&expanded, &change.path.display(), member)?;
                Ok(expanded)
            }
        }
    }

    /// Removes one row path from disk and reports whether a file leaves.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the removal fails.
    fn remove_path(&self, change: &Change) -> TerminalResult<(PathBuf, bool)> {
        let expanded = change.expanded.clone();
        let removed = remove(&expanded)
            .map_err(ContextualCause::from)
            .with_fs_path(&expanded)
            .terminal(ErrorKind::RemoveDestination)?;
        Ok((expanded, removed))
    }

    /// Writes one document to its resolved destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the write fails.
    fn write_document(&self, document: &Document, expanded: &Path) -> TerminalResult<PathBuf> {
        if let Data::Opaque(found) = &document.data {
            use std::io::Write as _;

            driver::fs::remove_link(expanded).terminal(ErrorKind::WriteDestination)?;
            let handle = &found.handle;
            let mut reader = self
                .stores
                .resources()
                .open(handle)
                .with_document(document.path.display())
                .terminal(ErrorKind::WriteDestination)?;
            driver::StagedFile::write(expanded, |file| {
                std::io::copy(&mut reader, file)?;
                file.flush()
            })
            .terminal(ErrorKind::WriteDestination)?;
            driver::fs::set_mode(expanded, Some(found.mode.bits()))
                .terminal(ErrorKind::WriteDestination)?;
            return Ok(expanded.to_path_buf());
        }
        match &document.data {
            Data::Link(found) => {
                let link = found.target.display();
                driver::fs::write_link(expanded, Path::new(&link))
                    .terminal(ErrorKind::WriteDestination)
            }
            Data::Text(_) | Data::Structured(_) | Data::Rc(_) => {
                driver::fs::remove_link(expanded).terminal(ErrorKind::WriteDestination)?;
                let bytes = document
                    .render()
                    .map_err(ContextualCause::from)
                    .with_fs_path(expanded)
                    .with_document(document.path.display())
                    .terminal(ErrorKind::WriteDestination)?;
                driver::fs::create_parent(expanded).terminal(ErrorKind::WriteDestination)?;
                std::fs::write(expanded, &bytes)
                    .map_err(ContextualCause::from)
                    .with_fs_path(expanded)
                    .with_document(document.path.display())
                    .terminal(ErrorKind::WriteDestination)
            }
            Data::Tree(_) | Data::Opaque(_) => {
                return Ok(expanded.to_path_buf());
            }
        }?;
        let mode = match &document.data {
            Data::Text(found) => Some(found.mode.bits()),
            Data::Opaque(found) => Some(found.mode.bits()),
            Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
        };
        driver::fs::set_mode(expanded, mode).terminal(ErrorKind::WriteDestination)?;
        Ok(expanded.to_path_buf())
    }

    /// Writes one tree member to its destination path.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the member is unreadable or
    ///   cannot be written.
    fn write_member(&self, path: &Path, route: &str, member: &TreeMember) -> TerminalResult<()> {
        use std::io::Write as _;

        let mut reader = self
            .stores
            .resources()
            .open(member.handle())
            .with_document(route.to_owned())
            .terminal(ErrorKind::WriteDestination)?;
        driver::StagedFile::write(path, |file| {
            std::io::copy(&mut reader, file)?;
            file.flush()
        })
        .terminal(ErrorKind::WriteDestination)?;
        driver::fs::set_mode(path, Some(member.mode().bits()))
            .terminal(ErrorKind::WriteDestination)?;
        Ok(())
    }

    /// Sends one progress event for an applied destination.
    fn emit_apply(
        reporter: &TaskReporter,
        path: &Path,
        outcome: ApplyOutcome,
        done: usize,
        total: usize,
    ) {
        reporter.progress(
            done,
            Some(total),
            Some(RowUpdate {
                path: path.display().to_string(),
                outcome,
            }),
        );
    }

    /// Sends one progress event for the row that fails the execution.
    fn emit_failure(reporter: &TaskReporter, change: &Change, done: usize, total: usize) {
        reporter.progress(
            done,
            Some(total),
            Some(RowUpdate {
                path: failure_path(change),
                outcome: ApplyOutcome::Failed,
            }),
        );
    }
}

/// Returns the attempted path for a failed row.
fn failure_path(change: &Change) -> String {
    change.expanded.display().to_string()
}

/// Removes one backend path and reports whether it held a file.
fn remove(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

/// Returns the write target that one effective row holds.
fn target<'a>(view: &'a View, change: &'a Change) -> Option<Target<'a>> {
    let Some(group) = &change.group else {
        return view
            .wanted
            .documents
            .iter()
            .find(|document| document.path == change.path)
            .map(Target::Whole);
    };
    let document = view
        .wanted
        .documents
        .iter()
        .find(|document| document.path == *group)?;
    let Data::Tree(found) = &document.data else {
        return None;
    };
    let name = member_relative(group, &change.path)?;
    found
        .members
        .iter()
        .find(|(key, _)| key.as_str() == name)
        .map(|(_, member)| Target::Member(member))
}

/// Returns the member name of one row path under its tree route.
fn member_relative<'a>(group: &Route, path: &'a Route) -> Option<&'a str> {
    if group.base() != path.base() {
        return None;
    }
    path.relative_path()
        .strip_prefix(group.relative_path())
        .ok()?
        .to_str()
}
#[cfg(test)]
mod tests {
    use super::*;
    use confit_differ::diff::Diff;
    use confit_events::EventHandles;
    use confit_events::protocol::{FromUi, TaskKind, TaskState, ToUi};
    use confit_model::document::{FileMode, LinkDocument, MemberKey, TextDocument, TreeDocument};
    use confit_model::manifest::Manifest;
    use confit_model::routes::RouteBase;
    use confit_store::StoreRoots;

    /// Builds one literal route under the given folder.
    fn literal(dir: &std::path::Path, name: &str) -> Route {
        Route::new(RouteBase::Literal, dir.join(name)).unwrap()
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

    /// Builds one empty manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    /// Builds the diff for two manifests and panics on failure.
    fn build(desired: Manifest, previous: Manifest) -> Diff {
        match Diff::build(desired, previous) {
            Ok(diff) => diff,
            Err(error) => panic!("the diff fails to build: {error}"),
        }
    }

    /// Builds one writer over a scratch folder with its event receiver.
    fn writer_in(
        dir: &std::path::Path,
    ) -> (Writer, EventHandles, crossbeam_channel::Receiver<ToUi>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let (_, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(sender, from_ui);
        let roots = StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        };
        let stores = Stores::new(roots, handles.clone());
        let writer = Writer::new(stores);
        (writer, handles, receiver)
    }

    /// Applies one view and panics on failure.
    fn apply(
        writer: &Writer,
        handles: &EventHandles,
        receiver: &crossbeam_channel::Receiver<ToUi>,
        view: &View,
    ) -> Applied {
        let total = Writer::apply_total(view);
        let reporter = handles.start_task(TaskKind::Write, Some(total));
        let _ = receiver.try_recv();
        match writer.apply(view, &reporter) {
            (applied, Ok(())) => applied,
            (_, Err(error)) => panic!("the apply fails: {error}"),
        }
    }

    /// Builds one write update event for an applied destination.
    fn write_update(
        path: &std::path::Path,
        outcome: ApplyOutcome,
        done: usize,
        total: usize,
    ) -> ToUi {
        ToUi::Task {
            kind: TaskKind::Write,
            state: TaskState::Progress {
                done,
                total: Some(total),
                row: Some(RowUpdate {
                    path: path.display().to_string(),
                    outcome,
                }),
            },
        }
    }

    #[test]
    fn effective_rows_write_additions_changes_and_remove_removals() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = literal(dir.path(), "fresh");
        let edited = literal(dir.path(), "edited");
        let retired = literal(dir.path(), "retired");
        std::fs::write(dir.path().join("edited"), b"disk").unwrap();
        std::fs::write(dir.path().join("retired"), b"recorded").unwrap();
        let desired = manifest(vec![
            text(fresh.clone(), "new"),
            text(edited.clone(), "wanted"),
        ]);
        let previous = manifest(vec![
            text(edited.clone(), "recorded"),
            text(retired.clone(), "recorded"),
        ]);
        let diff = build(desired, previous);
        assert_eq!(Writer::apply_total(&diff.effective), 3);
        let (writer, handles, receiver) = writer_in(dir.path());
        let applied = apply(&writer, &handles, &receiver, &diff.effective);
        assert_eq!(applied.added, 1);
        assert_eq!(applied.changed, 1);
        assert_eq!(applied.removed, 1);
        assert_eq!(
            std::fs::read(dir.path().join("fresh")).unwrap(),
            b"new",
            "the addition lands on disk"
        );
        assert_eq!(
            std::fs::read(dir.path().join("edited")).unwrap(),
            b"wanted",
            "the change lands on disk"
        );
        assert!(
            !dir.path().join("retired").exists(),
            "the removal leaves the disk"
        );
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                write_update(&dir.path().join("edited"), ApplyOutcome::Changed, 1, 3,),
                write_update(&dir.path().join("fresh"), ApplyOutcome::Added, 2, 3,),
                write_update(&dir.path().join("retired"), ApplyOutcome::Removed, 3, 3,),
            ]
        );
    }

    #[test]
    fn absent_removal_skips_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let gone = literal(dir.path(), "gone");
        let previous = manifest(vec![text(gone, "recorded")]);
        let diff = build(manifest(Vec::new()), previous);
        assert_eq!(Writer::apply_total(&diff.effective), 0);
        let (writer, handles, receiver) = writer_in(dir.path());
        let applied = apply(&writer, &handles, &receiver, &diff.effective);
        assert_eq!(applied.added, 0);
        assert_eq!(applied.changed, 0);
        assert_eq!(applied.removed, 0);
        assert!(
            receiver.try_iter().next().is_none(),
            "only applied rows update"
        );
    }

    #[test]
    fn identical_content_skips_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "kept");
        std::fs::write(dir.path().join("kept"), b"kept").unwrap();
        let held = manifest(vec![text(route, "kept")]);
        let diff = build(held.clone(), held);
        assert_eq!(Writer::apply_total(&diff.effective), 0);
        let (writer, handles, receiver) = writer_in(dir.path());
        let applied = apply(&writer, &handles, &receiver, &diff.effective);
        assert_eq!(applied.added, 0);
        assert_eq!(applied.changed, 0);
        assert_eq!(applied.removed, 0);
        assert!(
            receiver.try_iter().next().is_none(),
            "only applied rows update"
        );
    }

    #[test]
    fn tree_write_updates_only_the_changed_member() {
        let dir = tempfile::tempdir().unwrap();
        let tree = literal(dir.path(), "shared");
        std::fs::create_dir(dir.path().join("shared")).unwrap();
        std::fs::write(dir.path().join("shared/one.txt"), b"changed").unwrap();
        std::fs::write(dir.path().join("shared/two.txt"), b"kept").unwrap();
        let (writer, handles, receiver) = writer_in(dir.path());
        let store = |bytes: &[u8]| match writer.stores.resources().store(bytes) {
            Ok(found) => found,
            Err(error) => panic!("the pool fails to store: {error}"),
        };
        let member = |bytes: &[u8]| TreeMember::Handle(store(bytes));
        let tree_doc = |one: &[u8]| {
            Document::new(
                tree.clone(),
                Data::Tree(TreeDocument {
                    members: [
                        (MemberKey::new("one.txt".to_owned()).unwrap(), member(one)),
                        (
                            MemberKey::new("two.txt".to_owned()).unwrap(),
                            member(b"kept"),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
            )
        };
        let held = manifest(vec![tree_doc(b"wanted")]);
        let diff = build(held.clone(), held);
        assert_eq!(Writer::apply_total(&diff.effective), 1);
        let applied = apply(&writer, &handles, &receiver, &diff.effective);
        assert_eq!(applied.added, 0);
        assert_eq!(applied.changed, 1);
        assert_eq!(applied.removed, 0);
        assert_eq!(
            std::fs::read(dir.path().join("shared/one.txt")).unwrap(),
            b"wanted",
            "the changed member lands on disk"
        );
        assert_eq!(
            std::fs::read(dir.path().join("shared/two.txt")).unwrap(),
            b"kept",
            "the kept member stays untouched"
        );
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![write_update(
                &dir.path().join("shared/one.txt"),
                ApplyOutcome::Changed,
                1,
                1,
            )]
        );
    }

    #[test]
    fn incomparable_write_attempt_replaces_the_socket_link() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "socket");
        std::fs::write(dir.path().join("new-target"), b"target").unwrap();
        let held = std::os::unix::net::UnixListener::bind(dir.path().join("socket")).unwrap();
        let desired = manifest(vec![Document::new(
            route.clone(),
            Data::Link(LinkDocument {
                target: Route::new(RouteBase::Literal, "new-target").unwrap(),
            }),
        )]);
        let diff = build(desired, manifest(Vec::new()));
        assert_eq!(Writer::apply_total(&diff.effective), 1);
        drop(held);
        let (writer, handles, receiver) = writer_in(dir.path());
        let applied = apply(&writer, &handles, &receiver, &diff.effective);
        assert_eq!(applied.added, 1);
        assert_eq!(applied.changed, 0);
        assert_eq!(applied.removed, 0);
        assert_eq!(
            std::fs::read_link(dir.path().join("socket")).unwrap(),
            std::path::PathBuf::from("new-target"),
            "the attempt writes the link"
        );
        assert!(
            receiver.try_iter().next().is_none(),
            "only applied rows update"
        );
    }

    #[test]
    fn incomparable_block_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(dir.path(), "file");
        std::fs::create_dir(dir.path().join("file")).unwrap();
        let desired = manifest(vec![text(route, "wanted")]);
        let diff = build(desired, manifest(Vec::new()));
        let (writer, handles, receiver) = writer_in(dir.path());
        let total = Writer::apply_total(&diff.effective);
        let reporter = handles.start_task(TaskKind::Write, Some(total));
        let _ = receiver.try_recv();
        let (applied, result) = writer.apply(&diff.effective, &reporter);
        assert!(result.is_err(), "the blocked write fails at write time");
        assert_eq!(applied.changes.len(), 1);
        assert_eq!(applied.changes[0].outcome, ApplyOutcome::Failed);
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![write_update(
                &dir.path().join("file"),
                ApplyOutcome::Failed,
                1,
                1,
            )]
        );
    }

    #[test]
    fn failed_row_follows_an_applied_row() {
        let dir = tempfile::tempdir().unwrap();
        let good = literal(dir.path(), "aaa-good");
        let blocked = literal(dir.path(), "zzz-blocked");
        std::fs::create_dir(dir.path().join("zzz-blocked")).unwrap();
        let desired = manifest(vec![text(good, "new"), text(blocked, "wanted")]);
        let diff = build(desired, manifest(Vec::new()));
        assert_eq!(Writer::apply_total(&diff.effective), 2);
        let (writer, handles, receiver) = writer_in(dir.path());
        let total = Writer::apply_total(&diff.effective);
        let reporter = handles.start_task(TaskKind::Write, Some(total));
        let _ = receiver.try_recv();
        let (applied, result) = writer.apply(&diff.effective, &reporter);
        assert!(result.is_err(), "the blocked row fails the execution");
        assert_eq!(applied.changes.len(), 2);
        assert_eq!(applied.changes[1].outcome, ApplyOutcome::Failed);
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                write_update(&dir.path().join("aaa-good"), ApplyOutcome::Added, 1, 2,),
                write_update(&dir.path().join("zzz-blocked"), ApplyOutcome::Failed, 2, 2,),
            ]
        );
    }
}
