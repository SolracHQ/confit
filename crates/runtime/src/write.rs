//! Destination writes with deleted removal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::path::PathBuf;

use confit_core::error::Terminal;
use confit_core::error::{DocumentError, Error, ErrorCause, ErrorContext, EvalDetail, Outcome};
use confit_core::progress::{Event, ProgressSender};
use confit_driver as driver;
use confit_model::document::{Data, Document, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;

use crate::render::render_document;

/// Destination writer.
pub struct Writer {
    /// Write capabilities for one run.
    pub stores: Stores,
    /// Write events.
    pub progress: ProgressSender,
}

impl Writer {
    /// Write every document to its resolved destination.
    ///
    /// - Present unmanaged documents stay untouched while their destination
    ///   is absent from the changed set.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unresolvable bases, write faults,
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] for unresolvable bases, write faults,
    ///   and render faults.
    pub fn write_documents(
        &self,
        documents: &[Document],
        changed: &BTreeSet<Route>,
    ) -> Outcome<usize> {
        let total = documents.len();
        let mut done = 0;
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
                done += 1;
                self.emit_written(&document.path, done, total);
                written += count;
                continue;
            }
            if let Data::Opaque(found) = &document.data {
                use std::io::Write as _;

                driver::fs::remove_link(&expanded).write_destination(&expanded)?;
                let handle = &found.handle;
                let mut reader = self
                    .stores
                    .resources()
                    .open(handle)
                    .map_err(|error| error.route(document.path.display()))
                    .write_destination(&expanded)?;
                driver::StagedFile::write(&expanded, |file| {
                    std::io::copy(&mut reader, file)?;
                    file.flush()
                })
                .write_destination(&expanded)?;
                driver::fs::set_mode(&expanded, Some(found.mode)).write_destination(&expanded)?;
                done += 1;
                self.emit_written(&document.path, done, total);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                Data::Link(found) => {
                    let link = found.target.display();
                    driver::fs::write_link(&expanded, Path::new(&link)).write_destination(&expanded)
                }
                Data::Text(_) | Data::Structured(_) | Data::Rc(_) => {
                    driver::fs::remove_link(&expanded).write_destination(&expanded)?;
                    let bytes = render_document(document).map_err(|leaf| {
                        Error::write_destination(
                            expanded.clone(),
                            ErrorContext::from(leaf).route(document.path.display()),
                        )
                    })?;
                    driver::fs::create_parent(&expanded).write_destination(&expanded)?;
                    std::fs::write(&expanded, &bytes).write_destination(&expanded)
                }
                Data::Tree(_) | Data::Opaque(_) => {
                    continue;
                }
            };
            outcome?;
            let mode = match &document.data {
                Data::Text(found) => Some(found.mode),
                Data::Opaque(found) => Some(found.mode),
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
            };
            driver::fs::set_mode(&expanded, mode).write_destination(&expanded)?;
            done += 1;
            self.emit_written(&document.path, done, total);
            written += 1;
        }
        Ok(written)
    }

    fn emit_written(&self, destination: &Route, done: usize, total: usize) {
        let _ = self.progress.send(Event::DocumentWritten {
            path: destination.display(),
            done,
            total,
        });
    }

    /// Write one tree destination member by member.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] for unreadable members and unwritable members.
    fn write_tree_members(
        &self,
        dest: &Path,
        route: &str,
        members: &BTreeMap<String, TreeMember>,
    ) -> Outcome<usize> {
        use std::io::Write as _;

        for (relative, member) in members {
            let path = dest.join(relative);
            let handle = member.handle();
            let mut reader = self
                .stores
                .resources()
                .open(handle)
                .map_err(|error| error.route(route))
                .write_destination(&path)?;
            driver::StagedFile::write(&path, |file| {
                std::io::copy(&mut reader, file)?;
                file.flush()
            })
            .write_destination(&path)?;
            driver::fs::set_mode(&path, Some(member.mode())).write_destination(&path)?;
        }
        Ok(1)
    }
}

/// Resolve one document destination or fail the write.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unresolvable base folders.
fn expand_destination(document: &Document) -> Outcome<PathBuf> {
    match document.path.expand() {
        Ok(found) => Ok(found),
        Err(ErrorContext::Leaf(ErrorCause::DocumentError(DocumentError::ParseFailed {
            input,
            require,
        }))) => Err(Error::evaluate(
            EvalDetail::Parse { input, require },
            Some(document.path.display()),
        )),
        Err(_) => unreachable!("route expand fails parsing alone"),
    }
}

/// Remove one backend path and report whether anything remains.
fn remove(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

impl Writer {
    /// Remove recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unresolvable bases and removal faults.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] for unresolvable bases and removal faults.
    pub fn remove_deleted(&self, recorded: &[Document], desired: &[Document]) -> Outcome<usize> {
        let total = recorded.len();
        let mut done = 0;
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
            if remove(&expanded).remove_destination(&expanded)? {
                done += 1;
                self.emit_removed(&old.path, done, total);
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Remove dropped tree members between recorded and desired manifests.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unresolvable bases and removal faults.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] for unresolvable bases and removal faults.
    pub fn remove_tree_members(
        &self,
        recorded: &[Document],
        desired: &[Document],
    ) -> Outcome<usize> {
        let total: usize = recorded
            .iter()
            .filter_map(|old| match &old.data {
                Data::Tree(found) => Some(&found.members),
                Data::Structured(_)
                | Data::Text(_)
                | Data::Link(_)
                | Data::Rc(_)
                | Data::Opaque(_) => None,
            })
            .map(|members| members.len())
            .sum();
        let mut done = 0;
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
                .flat_map(|members| members.keys().map(String::as_str))
                .collect();
            let dest = expand_destination(old)?;
            for relative in old_members.keys() {
                if new_rels.contains(relative.as_str()) {
                    continue;
                }
                let path = dest.join(relative);
                if remove(&path).remove_destination(&path)? {
                    done += 1;
                    self.emit_removed(&old.path.join(relative), done, total);
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    fn emit_removed(&self, destination: &Route, done: usize, total: usize) {
        let _ = self.progress.send(Event::DocumentRemoved {
            path: destination.display(),
            done,
            total,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::progress::Event;
    use confit_model::document::TextDocument;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &std::path::Path) -> Document {
        Document {
            path: literal(path),
            data: Data::Text(TextDocument {
                content: "deleted bytes".into(),
                mode: 0o644,
                unmanaged: false,
            }),
        }
    }

    #[test]
    fn remove_deleted_announces_document_removed() {
        let dir = tempfile::tempdir().unwrap();
        let roots = StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        };
        let (sender, receiver) = crossbeam_channel::unbounded();
        let writer = Writer {
            stores: Stores::new(roots, sender.clone()),
            progress: sender,
        };
        let path = dir.path().join("deleted.txt");
        driver::fs::create_parent(&path).unwrap();
        std::fs::write(&path, b"deleted bytes").unwrap();
        let recorded = vec![text_doc(&path)];
        match writer.remove_deleted(&recorded, &[]) {
            Ok(removed) => assert_eq!(removed, 1, "deleted leaves disk"),
            Err(error) => panic!("deleted remove: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::DocumentRemoved {
                path: path.display().to_string(),
                done: 1,
                total: 1,
            }]
        );
    }

    #[test]
    fn batch_moves_write_counters_across_documents() {
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
            progress: sender,
        };
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        let documents = vec![text_doc(&first), text_doc(&second)];
        match writer.write_documents(&documents, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 2, "both documents land on disk"),
            Err(error) => panic!("documents write: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::DocumentWritten {
                    path: first.display().to_string(),
                    done: 1,
                    total: 2,
                },
                Event::DocumentWritten {
                    path: second.display().to_string(),
                    done: 2,
                    total: 2,
                },
            ]
        );
    }
}
