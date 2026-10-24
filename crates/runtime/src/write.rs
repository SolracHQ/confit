//! Destination writes with deleted removal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::path::PathBuf;

use confit_driver as driver;
use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;
use confit_model::document::{Data, Document, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;
use confit_store::faults::AccessFault;
use confit_store::resources::error::ResourceError;

use crate::error::{Result, RuntimeError};
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
    /// - [`RuntimeError::MissingContent`] for dangling tree
    ///   content hashes.
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
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => false,
            };
            if unmanaged && expanded.exists() && !changed.contains(&document.path) {
                continue;
            }
            if let Data::Tree(found) = &document.data {
                let count = self.write_tree_members(&expanded, &found.members)?;
                done += 1;
                self.emit_written(&document.path, done, total);
                written += count;
                continue;
            }
            if let Data::Opaque(found) = &document.data {
                use std::io::Write as _;

                if let Err(error) = driver::fs::remove_link(&expanded) {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                let handle = &found.handle;
                let mut reader = self
                    .stores
                    .resources()
                    .open(handle)
                    .map_err(|error| map_resource(error, handle.sha(), &expanded, &expanded))?;
                driver::StagedFile::write(&expanded, |file| {
                    std::io::copy(&mut reader, file)?;
                    file.flush()
                })
                .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                driver::fs::set_mode(&expanded, Some(found.mode))
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                done += 1;
                self.emit_written(&document.path, done, total);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
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
                Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
            };
            driver::fs::set_mode(&expanded, mode)
                .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
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
    /// - [`RuntimeError::MissingContent`] for dangling member
    ///   hashes with the destination and the member.
    /// - [`RuntimeError::Write`] for unwritable members
    ///   and mode failures.
    /// - [`RuntimeError::WriteUnknown`] for unmapped
    ///   member failures.
    fn write_tree_members(
        &self,
        dest: &Path,
        members: &BTreeMap<String, TreeMember>,
    ) -> Result<usize> {
        use std::io::Write as _;

        for (relative, member) in members {
            let path = dest.join(relative);
            let handle = member.handle();
            let mut reader = self
                .stores
                .resources()
                .open(handle)
                .map_err(|error| map_resource(error, handle.sha(), dest, &path))?;
            driver::StagedFile::write(&path, |file| {
                std::io::copy(&mut reader, file)?;
                file.flush()
            })
            .map_err(|error| RuntimeError::from_write_io(&path, error))?;
            driver::fs::set_mode(&path, Some(member.mode()))
                .map_err(|error| RuntimeError::from_write_io(&path, error))?;
        }
        Ok(1)
    }
}

/// Resolve one document destination or fail the write.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn expand_destination(document: &Document) -> Result<PathBuf> {
    document
        .path
        .expand()
        .map_err(|source| RuntimeError::Expand {
            route: document.path.clone(),
            source,
        })
}

/// Remove one backend path and report whether anything remains.
fn remove(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

/// Map one resource open failure at the destination into runtime language.
fn map_resource(error: ResourceError, sha: &Sha, dest: &Path, target: &Path) -> RuntimeError {
    match error {
        ResourceError::Read {
            fault: AccessFault::Missing,
            ..
        } => {
            log::error!("missing content '{}' for '{}'", sha.hex(), target.display());
            RuntimeError::MissingContent {
                dest: dest.to_path_buf(),
                sha: sha.clone(),
                member: target.to_path_buf(),
            }
        }
        ResourceError::Mismatch {
            expected, found, ..
        } => RuntimeError::Write {
            path: target.to_path_buf(),
            fault: AccessFault::Corrupt { expected, found },
        },
        ResourceError::Read { fault, .. } => RuntimeError::Write {
            path: target.to_path_buf(),
            fault,
        },
        ResourceError::Write { fault, .. } => RuntimeError::Write {
            path: target.to_path_buf(),
            fault,
        },
        ResourceError::WriteUnknown { message, .. } => {
            log::error!("resource write failed: {message}");
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message,
            }
        }
        other => RuntimeError::WriteUnknown {
            path: target.to_path_buf(),
            message: other.to_string(),
        },
    }
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
            let kept = desired.iter().any(|document| document.path == old.path);
            if kept {
                continue;
            }
            let expanded = expand_destination(old)?;
            if remove(&expanded).map_err(|error| RuntimeError::from_remove_io(&expanded, error))? {
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
                if remove(&path).map_err(|error| RuntimeError::from_remove_io(&path, error))? {
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
    use confit_driver::progress::Event;
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
