//! Destination writes with deleted removal.

use std::collections::BTreeSet;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use confit_driver as driver;
use confit_driver::progress::{Event, ProgressSender};
use confit_model::document::Data;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::disk::{copy_blob, remove, write_tree};
use crate::error::{Result, RuntimeError};
use crate::manifest::Document;
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
    /// - [`RuntimeError::Expand`] for unresolvable base folders.
    /// - [`RuntimeError::Write`] for write faults.
    /// - [`RuntimeError::WriteUnknown`] for unmapped
    ///   write failures.
    /// - [`RuntimeError::MissingBlob`] for dangling tree
    ///   blob hashes.
    /// - [`RuntimeError::Refusal`] with [`RuntimeError::Render`]
    ///   through rendering.
    pub fn write_documents(
        &self,
        documents: &[Document],
        changed: &BTreeSet<Route>,
    ) -> Result<usize> {
        let total = documents.len();
        let mut done = 0;
        let mut written = 0;
        for document in documents {
            let expanded = expand_destination(document)?;
            let unmanaged = match &document.data {
                Data::Text(found) => found.unmanaged,
                Data::Opaque(found) => found.unmanaged,
                #[cfg(unix)]
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => false,
                #[cfg(not(unix))]
                Data::Structured(_) | Data::Rc(_) | Data::Tree(_) => false,
            };
            if unmanaged && expanded.exists() && !changed.contains(&document.destination) {
                continue;
            }
            if let Data::Tree(found) = &document.data {
                let count = write_tree(&expanded, &found.members, &self.stores)?;
                driver::fs::set_mode(&expanded, None)
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                done += 1;
                self.emit_written(&document.destination, done, total);
                written += count;
                continue;
            }
            if let Data::Opaque(found) = &document.data {
                if let Err(error) = driver::fs::remove_link(&expanded) {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                copy_blob(&expanded, &expanded, &found.handle, &self.stores)?;
                driver::fs::set_mode(&expanded, Some(found.mode))
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                done += 1;
                self.emit_written(&document.destination, done, total);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                #[cfg(unix)]
                Data::Link(found) => {
                    let link = found.target.display();
                    driver::fs::write_link(&expanded, Path::new(&link))
                        .map_err(|error| RuntimeError::from_write_io(&expanded, error))
                }
                Data::Text(_) | Data::Structured(_) | Data::Rc(_) => {
                    if let Err(error) = driver::fs::remove_link(&expanded) {
                        return Err(RuntimeError::from_write_io(&expanded, error));
                    }
                    let bytes = render_document(document)?;
                    if let Err(error) = driver::fs::create_parent(&expanded) {
                        return Err(RuntimeError::from_write_io(&expanded, error));
                    }
                    std::fs::write(&expanded, &bytes)
                        .map_err(|error| RuntimeError::from_write_io(&expanded, error))
                }
                Data::Tree(_) | Data::Opaque(_) => {
                    continue;
                }
            };
            outcome?;
            let mode = match &document.data {
                Data::Text(found) => Some(found.mode),
                Data::Opaque(found) => Some(found.mode),
                #[cfg(unix)]
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
                #[cfg(not(unix))]
                Data::Structured(_) | Data::Rc(_) | Data::Tree(_) => None,
            };
            driver::fs::set_mode(&expanded, mode)
                .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
            done += 1;
            self.emit_written(&document.destination, done, total);
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
}

/// Resolve one document destination or fail the write.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn expand_destination(document: &Document) -> Result<PathBuf> {
    document
        .destination
        .expand()
        .map_err(|source| RuntimeError::Expand {
            route: document.destination.clone(),
            source,
        })
}

impl Writer {
    /// Remove recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Expand`] for unresolvable base folders.
    /// - [`RuntimeError::Remove`] for removal faults.
    /// - [`RuntimeError::RemoveUnknown`] for unmapped
    ///   removal failures.
    pub fn remove_deleted(&self, recorded: &[Document], desired: &[Document]) -> Result<usize> {
        let total = recorded.len();
        let mut done = 0;
        let mut removed = 0;
        for old in recorded {
            if matches!(old.data, Data::Tree(_)) {
                continue;
            }
            let kept = desired
                .iter()
                .any(|document| document.destination == old.destination);
            if kept {
                continue;
            }
            let expanded = expand_destination(old)?;
            if remove(&expanded).map_err(|error| RuntimeError::from_remove_io(&expanded, error))? {
                done += 1;
                self.emit_removed(&old.destination, done, total);
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Remove dropped tree members between recorded and desired manifests.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Expand`] for unresolvable base folders.
    /// - [`RuntimeError::Remove`] for removal faults.
    /// - [`RuntimeError::RemoveUnknown`] for unmapped
    ///   removal failures.
    pub fn remove_tree_members(
        &self,
        recorded: &[Document],
        desired: &[Document],
    ) -> Result<usize> {
        let total: usize = recorded
            .iter()
            .filter_map(|old| match &old.data {
                Data::Tree(found) => Some(&found.members),
                #[cfg(unix)]
                Data::Structured(_)
                | Data::Text(_)
                | Data::Link(_)
                | Data::Rc(_)
                | Data::Opaque(_) => None,
                #[cfg(not(unix))]
                Data::Structured(_) | Data::Text(_) | Data::Rc(_) | Data::Opaque(_) => None,
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
                .filter(|document| document.destination == old.destination)
                .filter_map(|document| match &document.data {
                    Data::Tree(found) => Some(&found.members),
                    #[cfg(unix)]
                    Data::Structured(_)
                    | Data::Text(_)
                    | Data::Link(_)
                    | Data::Rc(_)
                    | Data::Opaque(_) => None,
                    #[cfg(not(unix))]
                    Data::Structured(_) | Data::Text(_) | Data::Rc(_) | Data::Opaque(_) => None,
                })
                .flat_map(|members| members.keys().map(String::as_str))
                .collect();
            let dest = expand_destination(old)?;
            for relative in old_members.keys() {
                if new_rels.contains(relative.as_str()) {
                    continue;
                }
                let path = dest.join(relative);
                if remove(&path).map_err(|error| RuntimeError::from_remove_io(&path, error))? {
                    done += 1;
                    self.emit_removed(&old.destination.join(relative), done, total);
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
    use confit_driver::progress::Event;
    use confit_model::document::TextDocument;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &std::path::Path) -> Document {
        Document::new(
            literal(path),
            Data::Text(TextDocument {
                content: "deleted bytes".into(),
                mode: 0o644,
                unmanaged: false,
            }),
        )
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
