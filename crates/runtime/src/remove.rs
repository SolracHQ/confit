//! Orphan and member removal from resolved destinations.

use std::collections::BTreeSet;

use confit_model::document::ManifestDocument;
use confit_model::error::{Error, Result};

use crate::Applier;

impl Applier {
    /// Removes recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    pub fn remove_orphans(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize> {
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
            let expanded = self.resolve(&old.destination);
            if self.disk.remove(&expanded).map_err(Error::from)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Removes dropped tree members between recorded and desired manifests.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    pub fn remove_tree_members(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
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
            let dest = self.resolve(&old.destination);
            for member in old_members {
                if new_rels.contains(member.relative.as_str()) {
                    continue;
                }
                let path = dest.join(&member.relative);
                if self.disk.remove(&path).map_err(Error::from)? {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}
