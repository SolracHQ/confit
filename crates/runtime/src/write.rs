//! Document writes to resolved destinations.

use std::collections::BTreeSet;
use std::path::Path;

use confit_model::document::{Data, Document};
use confit_model::progress::Event;
use confit_model::routes::Route;

use crate::Applier;
use crate::error::{Result, RuntimeError};

impl Applier {
    /// Writes every document to its resolved destination.
    ///
    /// Present unmanaged documents stay untouched while
    /// their destination reads absent from the changed set.
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
        let mut written = 0;
        for document in documents {
            let expanded = document.destination.expand();
            if document.data.unmanaged()
                && self.disk.exists(&expanded)
                && !changed.contains(&document.destination)
            {
                continue;
            }
            if let Data::Tree { members } = &document.data {
                let count = self.disk.write_tree(&expanded, members, blobs.as_ref())?;
                if let Some(mode) = document.mode()
                    && let Err(error) = self.disk.set_mode(&expanded, mode)
                {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                self.emit_written(&document.destination);
                written += count;
                continue;
            }
            if let Data::Opaque { blob, .. } = &document.data {
                if let Err(error) = self.disk.clear_link(&expanded) {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                self.disk.write_blob(&expanded, blob, blobs.as_ref())?;
                if let Some(mode) = document.mode()
                    && let Err(error) = self.disk.set_mode(&expanded, mode)
                {
                    return Err(RuntimeError::from_write_io(&expanded, error));
                }
                self.emit_written(&document.destination);
                written += 1;
                continue;
            }
            let outcome = match &document.data {
                Data::Link { target } => self
                    .disk
                    .write_link(&expanded, Path::new(target))
                    .map_err(|error| RuntimeError::from_write_io(&expanded, error)),
                Data::Text { .. } | Data::Structured { .. } | Data::Rc(_) => {
                    if let Err(error) = self.disk.clear_link(&expanded) {
                        return Err(RuntimeError::from_write_io(&expanded, error));
                    }
                    let bytes = self.render_document(document)?;
                    self.disk
                        .write_bytes(&expanded, &bytes)
                        .map_err(|error| RuntimeError::from_write_io(&expanded, error))
                }
                Data::Tree { .. } | Data::Opaque { .. } => {
                    continue;
                }
            };
            outcome?;
            if let Some(mode) = document.mode()
                && let Err(error) = self.disk.set_mode(&expanded, mode)
            {
                return Err(RuntimeError::from_write_io(&expanded, error));
            }
            self.emit_written(&document.destination);
            written += 1;
        }
        Ok(written)
    }

    fn emit_written(&self, destination: &Route) {
        let _ = self.progress.send(Event::DocumentWritten {
            path: destination.display(),
        });
    }
}
