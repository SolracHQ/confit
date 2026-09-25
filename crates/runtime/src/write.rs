//! Document writes to resolved destinations.

use std::collections::BTreeSet;
use std::path::Path;

use confit_model::document::{ManifestData, ManifestDocument};
use confit_model::error::{Error, Result};
use confit_model::handles::Route;

use crate::Applier;
use crate::render::render_document;

impl Applier {
    /// Writes every document to its resolved destination.
    ///
    /// Present unmanaged documents stay untouched while
    /// their destination reads absent from the changed set.
    ///
    /// # Errors
    ///
    /// Render, command, and io failures surface as plan or io errors.
    pub fn write_documents(
        &self,
        documents: &[ManifestDocument],
        changed: &BTreeSet<Route>,
        on_written: Option<&dyn Fn(&Route)>,
    ) -> Result<usize> {
        let blobs = self.stores.blobs();
        let mut written = 0;
        for document in documents {
            let expanded = self.resolve(&document.destination);
            if document.data.unmanaged()
                && self.disk.exists(&expanded)
                && !changed.contains(&document.destination)
            {
                continue;
            }
            if let ManifestData::Tree { members } = &document.data {
                let count = self.disk.write_tree(&expanded, members, blobs.as_ref())?;
                if let Some(mode) = document.mode()
                    && let Err(error) = self.disk.set_mode(&expanded, mode)
                {
                    return Err(Error::Plan(format!(
                        "cannot set mode '{}': {error}",
                        expanded.display()
                    )));
                }
                if let Some(notify) = on_written {
                    notify(&document.destination);
                }
                written += count;
                continue;
            }
            if let ManifestData::Opaque { blob, .. } = &document.data {
                if let Err(error) = self.disk.clear_link(&expanded) {
                    return Err(Error::Plan(format!(
                        "cannot remove link '{}': {error}",
                        expanded.display()
                    )));
                }
                self.disk.write_blob(&expanded, blob, blobs.as_ref())?;
                if let Some(mode) = document.mode()
                    && let Err(error) = self.disk.set_mode(&expanded, mode)
                {
                    return Err(Error::Plan(format!(
                        "cannot set mode '{}': {error}",
                        expanded.display()
                    )));
                }
                if let Some(notify) = on_written {
                    notify(&document.destination);
                }
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                ManifestData::Link { target } => self
                    .disk
                    .write_link(&expanded, Path::new(target))
                    .map_err(Error::from),
                ManifestData::Text { .. }
                | ManifestData::Structured { .. }
                | ManifestData::Rc(_) => {
                    if let Err(error) = self.disk.clear_link(&expanded) {
                        return Err(Error::Plan(format!(
                            "cannot remove link '{}': {error}",
                            expanded.display()
                        )));
                    }
                    let bytes = render_document(document, self)?;
                    self.disk
                        .write_bytes(&expanded, &bytes)
                        .map_err(Error::from)
                }
                ManifestData::Tree { .. } | ManifestData::Opaque { .. } => {
                    continue;
                }
            };
            if let Err(error) = outcome {
                return Err(Error::Plan(format!(
                    "cannot write '{}': {error}",
                    expanded.display()
                )));
            }
            if let Some(mode) = document.mode()
                && let Err(error) = self.disk.set_mode(&expanded, mode)
            {
                return Err(Error::Plan(format!(
                    "cannot set mode '{}': {error}",
                    expanded.display()
                )));
            }
            if let Some(notify) = on_written {
                notify(&document.destination);
            }
            written += 1;
        }
        Ok(written)
    }
}
