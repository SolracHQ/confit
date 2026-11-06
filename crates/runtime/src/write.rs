//! Destination writes with deleted removal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::path::PathBuf;

use confit_core::error::kind::ErrorKind;
use confit_core::error::{ContextualCause, Terminal, TerminalResult, WithContext};
use confit_driver as driver;
use confit_events::{ApplyOutcome, ToUi};
use confit_model::document::{Data, Document, MemberKey, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;
use crossbeam_channel::Sender;

/// The writer for document destinations.
pub struct Writer {
    /// The stores that serve the writes for one run.
    pub stores: Stores,
    /// The progress channel for write events.
    pub to_ui: Sender<ToUi>,
}

impl Writer {
    /// Returns the file count for the whole apply.
    ///
    /// - The count covers every desired document, every recorded
    ///   destination, and every recorded tree member.
    pub fn apply_total(desired: &[Document], recorded: &[Document]) -> usize {
        let members: usize = recorded
            .iter()
            .filter_map(|old| match &old.data {
                Data::Tree(found) => Some(found.members.len()),
                Data::Text(_)
                | Data::Structured(_)
                | Data::Link(_)
                | Data::Rc(_)
                | Data::Opaque(_) => None,
            })
            .sum();
        desired.len() + recorded.len() + members
    }

    /// Writes the documents to their resolved destinations.
    ///
    /// - A present unmanaged document changes only when its destination is in
    ///   the changed set.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a base is unresolvable or a write fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when a base is unresolvable, a write
    ///   fails, or a render fails.
    pub fn write_documents(
        &self,
        documents: &[Document],
        changed: &BTreeSet<Route>,
    ) -> TerminalResult<usize> {
        let mut written = 0;
        for document in documents {
            let expanded = expand_destination(document)?;
            let unmanaged = match &document.data {
                Data::Text(found) => found.unmanaged,
                Data::Opaque(found) => found.unmanaged,
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => false,
            };
            if unmanaged && expanded.exists() && !changed.contains(&document.path) {
                continue;
            }
            if let Data::Tree(found) = &document.data {
                let count =
                    self.write_tree_members(&expanded, &document.path.display(), &found.members)?;
                self.emit_apply(&document.path, ApplyOutcome::Added);
                written += count;
                continue;
            }
            if let Data::Opaque(found) = &document.data {
                use std::io::Write as _;

                driver::fs::remove_link(&expanded).terminal(ErrorKind::WriteDestination)?;
                let handle = &found.handle;
                let mut reader = self
                    .stores
                    .resources()
                    .open(handle)
                    .with_document(document.path.display())
                    .terminal(ErrorKind::WriteDestination)?;
                driver::StagedFile::write(&expanded, |file| {
                    std::io::copy(&mut reader, file)?;
                    file.flush()
                })
                .terminal(ErrorKind::WriteDestination)?;
                driver::fs::set_mode(&expanded, Some(found.mode.bits()))
                    .terminal(ErrorKind::WriteDestination)?;
                self.emit_apply(&document.path, ApplyOutcome::Added);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                Data::Link(found) => {
                    let link = found.target.display();
                    driver::fs::write_link(&expanded, Path::new(&link))
                        .terminal(ErrorKind::WriteDestination)
                }
                Data::Text(_) | Data::Structured(_) | Data::Rc(_) => {
                    driver::fs::remove_link(&expanded).terminal(ErrorKind::WriteDestination)?;
                    let bytes = document
                        .render()
                        .map_err(ContextualCause::from)
                        .with_fs_path(&expanded)
                        .with_document(document.path.display())
                        .terminal(ErrorKind::WriteDestination)?;
                    driver::fs::create_parent(&expanded).terminal(ErrorKind::WriteDestination)?;
                    std::fs::write(&expanded, &bytes)
                        .map_err(ContextualCause::from)
                        .with_fs_path(&expanded)
                        .terminal(ErrorKind::WriteDestination)
                }
                Data::Tree(_) | Data::Opaque(_) => {
                    continue;
                }
            };
            outcome?;
            let mode = match &document.data {
                Data::Text(found) => Some(found.mode.bits()),
                Data::Opaque(found) => Some(found.mode.bits()),
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
            };
            driver::fs::set_mode(&expanded, mode).terminal(ErrorKind::WriteDestination)?;
            self.emit_apply(&document.path, ApplyOutcome::Added);
            written += 1;
        }
        Ok(written)
    }

    /// Sends one progress event for an applied destination.
    fn emit_apply(&self, destination: &Route, outcome: ApplyOutcome) {
        let _ = self.to_ui.send(ToUi::ApplyProgress {
            route: destination.clone(),
            outcome,
        });
    }

    /// Writes the members of one tree destination one by one.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when a member is unreadable or cannot be written.
    fn write_tree_members(
        &self,
        dest: &Path,
        route: &str,
        members: &BTreeMap<MemberKey, TreeMember>,
    ) -> TerminalResult<usize> {
        use std::io::Write as _;

        for (relative, member) in members {
            let path = dest.join(relative.as_str());
            let handle = member.handle();
            let mut reader = self
                .stores
                .resources()
                .open(handle)
                .with_document(route.to_owned())
                .terminal(ErrorKind::WriteDestination)?;
            driver::StagedFile::write(&path, |file| {
                std::io::copy(&mut reader, file)?;
                file.flush()
            })
            .terminal(ErrorKind::WriteDestination)?;
            driver::fs::set_mode(&path, Some(member.mode().bits()))
                .terminal(ErrorKind::WriteDestination)?;
        }
        Ok(1)
    }
}

/// Resolves the destination of one document.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a base folder is absent.
fn expand_destination(document: &Document) -> TerminalResult<PathBuf> {
    document
        .path
        .expand()
        .with_document(document.path.display())
        .terminal(ErrorKind::Evaluate)
}

/// Removes one backend path and reports whether it held a file.
fn remove(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

impl Writer {
    /// Removes the recorded destinations that the desired documents lack.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a base is unresolvable or a removal fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when a base is unresolvable or a removal
    ///   fails.
    pub fn remove_deleted(
        &self,
        recorded: &[Document],
        desired: &[Document],
    ) -> TerminalResult<usize> {
        let mut removed = 0;
        for old in recorded {
            if matches!(old.data, Data::Tree(_)) {
                continue;
            }
            let kept = desired.iter().any(|document| document.path == old.path);
            if kept {
                continue;
            }
            let expanded = expand_destination(old)?;
            if remove(&expanded)
                .map_err(ContextualCause::from)
                .with_fs_path(&expanded)
                .terminal(ErrorKind::RemoveDestination)?
            {
                self.emit_apply(&old.path, ApplyOutcome::Removed);
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Removes the tree members that the desired manifests drop.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a base is unresolvable or a removal fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when a base is unresolvable or a removal
    ///   fails.
    pub fn remove_tree_members(
        &self,
        recorded: &[Document],
        desired: &[Document],
    ) -> TerminalResult<usize> {
        let mut removed = 0;
        for old in recorded {
            let Data::Tree(found) = &old.data else {
                continue;
            };
            let old_members = &found.members;
            let new_rels: BTreeSet<&str> = desired
                .iter()
                .filter(|document| document.path == old.path)
                .filter_map(|document| match &document.data {
                    Data::Tree(found) => Some(&found.members),
                    Data::Structured(_)
                    | Data::Text(_)
                    | Data::Link(_)
                    | Data::Rc(_)
                    | Data::Opaque(_) => None,
                })
                .flat_map(|members| members.keys().map(MemberKey::as_str))
                .collect();
            let dest = expand_destination(old)?;
            for relative in old_members.keys() {
                if new_rels.contains(relative.as_str()) {
                    continue;
                }
                let path = dest.join(relative.as_str());
                if remove(&path)
                    .map_err(ContextualCause::from)
                    .with_fs_path(&path)
                    .terminal(ErrorKind::RemoveDestination)?
                {
                    self.emit_apply(&old.path.join(relative.as_str()), ApplyOutcome::Removed);
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{FileMode, TextDocument};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &Path) -> Document {
        Document {
            path: literal(path),
            data: Data::Text(TextDocument {
                content: "deleted bytes".into(),
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        }
    }

    #[test]
    fn remove_deleted_announces_a_removed_document() {
        let dir = tempfile::tempdir().unwrap();
        let roots = StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        };
        let (sender, receiver) = crossbeam_channel::unbounded();
        let writer = Writer {
            stores: Stores::new(roots, sender.clone()),
            to_ui: sender,
        };
        let path = dir.path().join("deleted.txt");
        driver::fs::create_parent(&path).unwrap();
        std::fs::write(&path, b"deleted bytes").unwrap();
        let recorded = vec![text_doc(&path)];
        match writer.remove_deleted(&recorded, &[]) {
            Ok(removed) => assert_eq!(removed, 1, "the removed document leaves the disk"),
            Err(error) => panic!("the removal fails: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![ToUi::ApplyProgress {
                route: literal(&path),
                outcome: ApplyOutcome::Removed,
            }]
        );
    }

    #[test]
    fn write_documents_counts_each_document() {
        use std::collections::BTreeSet;

        let dir = tempfile::tempdir().unwrap();
        let roots = StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        };
        let (sender, receiver) = crossbeam_channel::unbounded();
        let writer = Writer {
            stores: Stores::new(roots, sender.clone()),
            to_ui: sender,
        };
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        let documents = vec![text_doc(&first), text_doc(&second)];
        match writer.write_documents(&documents, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 2, "both documents land on the disk"),
            Err(error) => panic!("the documents fail to write: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                ToUi::ApplyProgress {
                    route: literal(&first),
                    outcome: ApplyOutcome::Added,
                },
                ToUi::ApplyProgress {
                    route: literal(&second),
                    outcome: ApplyOutcome::Added,
                },
            ]
        );
    }

    #[test]
    fn apply_total_counts_documents_and_tree_members() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let recorded = dir.path().join("recorded.txt");
        let desired = vec![text_doc(&first)];
        let recorded = vec![text_doc(&recorded)];
        assert_eq!(Writer::apply_total(&desired, &recorded), 2);
    }
}
