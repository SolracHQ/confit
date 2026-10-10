//! Destination writes with orphan removal.

use std::collections::BTreeSet;
use std::path::Path;

use confit_driver as driver;
use confit_model::document::{Data, Document};
use confit_model::progress::{Event, ProgressSender};
use confit_model::routes::Route;
use confit_store::Stores;

use crate::disk::{copy_blob, remove, write_tree};
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
        let blobs = self.stores.blobs();
        let total = documents.len();
        let mut done = 0;
        let mut written = 0;
        for document in documents {
            let expanded = document.destination.expand();
            if document.data.unmanaged()
                && driver::fs::exists(&expanded)
                && !changed.contains(&document.destination)
            {
                continue;
            }
            if let Data::Tree { members } = &document.data {
                let count = write_tree(&expanded, members, blobs.as_ref())?;
                driver::fs::set_mode_if_present(&expanded, document.mode())
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                done += 1;
                self.emit_written(&document.destination, done, total);
                written += count;
                continue;
            }
            if let Data::Opaque { blob, .. } = &document.data {
                if let Err(error) = driver::fs::remove_link(&expanded) {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                copy_blob(&expanded, &expanded, blob, blobs.as_ref())?;
                driver::fs::set_mode_if_present(&expanded, document.mode())
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error))?;
                done += 1;
                self.emit_written(&document.destination, done, total);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                Data::Link { target } => driver::fs::write_link(&expanded, Path::new(target))
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error)),
                Data::Text { .. } | Data::Structured { .. } | Data::Rc(_) => {
                    if let Err(error) = driver::fs::remove_link(&expanded) {
                        return Err(RuntimeError::from_write_io(&expanded, error));
                    }
                    let bytes = render_document(document)?;
                    if let Err(error) = driver::fs::create_parent(&expanded) {
                        return Err(RuntimeError::from_write_io(&expanded, error));
                    }
                    driver::fs::write(&expanded, &bytes)
                        .map_err(|error| RuntimeError::from_write_io(&expanded, error))
                }
                Data::Tree { .. } | Data::Opaque { .. } => {
                    continue;
                }
            };
            outcome?;
            driver::fs::set_mode_if_present(&expanded, document.mode())
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

impl Writer {
    /// Remove recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Remove`] for removal faults.
    /// - [`RuntimeError::RemoveUnknown`] for unmapped
    ///   removal failures.
    pub fn remove_orphans(&self, recorded: &[Document], desired: &[Document]) -> Result<usize> {
        let total = recorded.len();
        let mut done = 0;
        let mut removed = 0;
        for old in recorded {
            if old.data.tree_members().is_some() {
                continue;
            }
            let kept = desired
                .iter()
                .any(|document| document.destination == old.destination);
            if kept {
                continue;
            }
            let expanded = old.destination.expand();
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
            .filter_map(|old| old.data.tree_members())
            .map(|members| members.len())
            .sum();
        let mut done = 0;
        let mut removed = 0;
        for old in recorded {
            let Some(old_members) = old.data.tree_members() else {
                continue;
            };
            let new_rels: BTreeSet<&str> = desired
                .iter()
                .filter(|document| document.destination == old.destination)
                .filter_map(|document| document.data.tree_members())
                .flat_map(|members| members.iter().map(|member| member.relative.as_str()))
                .collect();
            let dest = old.destination.expand();
            for member in old_members {
                if new_rels.contains(member.relative.as_str()) {
                    continue;
                }
                let path = dest.join(&member.relative);
                if remove(&path).map_err(|error| RuntimeError::from_remove_io(&path, error))? {
                    done += 1;
                    self.emit_removed(&old.destination.join(&member.relative), done, total);
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
    use confit_driver::fs::TestGuard;
    use confit_model::progress::Event;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &std::path::Path) -> Document {
        Document::new(
            literal(path),
            Data::Text {
                content: "orphan bytes".into(),
                mode: None,
                unmanaged: false,
            },
        )
    }

    #[test]
    fn remove_orphans_announces_document_removed() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
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
        let path = dir.path().join("orphan.txt");
        driver::fs::create_parent(&path).unwrap();
        driver::fs::write(&path, b"orphan bytes").unwrap();
        let recorded = vec![text_doc(&path)];
        match writer.remove_orphans(&recorded, &[]) {
            Ok(removed) => assert_eq!(removed, 1, "orphan leaves disk"),
            Err(error) => panic!("orphans remove: {error}"),
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
    fn batch_moves_remove_counters_across_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
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
        driver::fs::create_parent(&first).unwrap();
        driver::fs::write(&first, b"orphan bytes").unwrap();
        driver::fs::write(&second, b"orphan bytes").unwrap();
        let recorded = vec![text_doc(&first), text_doc(&second)];
        match writer.remove_orphans(&recorded, &[]) {
            Ok(removed) => assert_eq!(removed, 2, "both orphans leave disk"),
            Err(error) => panic!("orphans remove: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::DocumentRemoved {
                    path: first.display().to_string(),
                    done: 1,
                    total: 2,
                },
                Event::DocumentRemoved {
                    path: second.display().to_string(),
                    done: 2,
                    total: 2,
                },
            ]
        );
    }

    #[test]
    fn batch_moves_write_counters_across_documents() {
        use std::collections::BTreeSet;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
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
