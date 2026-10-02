//! Orphan and member removal from resolved destinations.

use std::collections::BTreeSet;

use confit_model::document::Document;
use confit_model::progress::Event;
use confit_model::routes::Route;

use crate::Applier;
use crate::error::{Result, RuntimeError};

impl Applier {
    /// Removes recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Remove`] for removal faults.
    /// - [`RuntimeError::RemoveUnknown`] for unmapped
    ///   removal failures.
    pub fn remove_orphans(&self, recorded: &[Document], desired: &[Document]) -> Result<usize> {
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
            if self
                .disk
                .remove(&expanded)
                .map_err(|error| RuntimeError::from_remove_io(&expanded, error))?
            {
                self.emit_removed(&old.destination);
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Removes dropped tree members between recorded and desired manifests.
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
                if self
                    .disk
                    .remove(&path)
                    .map_err(|error| RuntimeError::from_remove_io(&path, error))?
                {
                    self.emit_removed(&old.destination.join(&member.relative));
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    fn emit_removed(&self, destination: &Route) {
        let _ = self.progress.send(Event::DocumentRemoved {
            path: destination.display(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;
    use confit_model::document::{Data, Document};
    use confit_model::progress::Event;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

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
            cache_base: dir.path().join("cache"),
            temp_base: dir.path().join("temp"),
        };
        let (sender, receiver) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots, sender.clone());
        let applier = Applier::with_stores(stores, sender);
        let path = dir.path().join("orphan.txt");
        applier.insert(&path, b"orphan bytes");
        let recorded = vec![text_doc(&path)];
        match applier.remove_orphans(&recorded, &[]) {
            Ok(removed) => assert_eq!(removed, 1, "orphan leaves disk"),
            Err(error) => panic!("orphans remove: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(events.len(), 1, "removal fires one fact");
        assert!(
            matches!(events[0], Event::DocumentRemoved { .. }),
            "removal announces itself"
        );
    }
}
