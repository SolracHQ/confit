//! Freeze leftover documents into finished model documents.

use std::collections::BTreeMap;

use confit_model::document::{Data, Document as ModelDocument, ManifestMember, RcData};
use confit_model::routes::Route;
use confit_store::blob::BlobSource;
use confit_surface_api::{ApiError, Document, Tree, TreeSlot};

use super::Assembler;
use crate::error::{EngineError, Result};

impl Assembler<'_> {
    /// Freeze one destination with body pair into its model document.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Surface`] for blob open failures.
    pub(super) fn placed_document(
        &self,
        destination: &Route,
        body: &Document,
    ) -> Result<ModelDocument> {
        let data = match body {
            Document::Text(found) => Data::Text {
                content: found.text.clone(),
                mode: found.mode.clone().into_inner(),
                unmanaged: found.unmanaged.unwrap_or(false),
            },
            Document::Structured(found) => Data::Structured {
                format: found.format,
                data: found
                    .data
                    .iter()
                    .map(|(key, value)| (key.clone(), value.0.clone()))
                    .collect(),
            },
            Document::Link(found) => Data::Link {
                target: found.target.display(),
            },
            Document::Rc(found) => Data::Rc(RcData::new(
                found.profile.clone(),
                found.config.clone(),
                found.final_entries.clone(),
            )),
            Document::Opaque(found) => {
                let opened = self
                    .stores()
                    .blobs()
                    .put(BlobSource::Handle(found.handle.handle()))
                    .map_err(ApiError::from)
                    .map_err(EngineError::from)?;
                let size = self
                    .stores()
                    .blobs()
                    .len(&opened)
                    .map_err(ApiError::from)
                    .map_err(EngineError::from)?;
                Data::Opaque {
                    blob: opened.to_ref(),
                    size,
                    mode: found.mode.clone().into_inner(),
                    unmanaged: found.unmanaged,
                }
            }
            Document::Tree(found) => {
                let mut members = Vec::new();
                self.collect_tree(&found.members, String::new(), &mut members)?;
                members.sort_by(|left, right| left.relative.cmp(&right.relative));
                Data::Tree { members }
            }
        };
        Ok(ModelDocument::new(destination.clone(), data))
    }

    /// Collect nested tree slots into opened members.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Surface`] for blob open failures.
    pub(super) fn collect_tree(
        &self,
        slots: &Tree,
        prefix: String,
        members: &mut Vec<ManifestMember>,
    ) -> Result<()> {
        for (name, slot) in &slots.0 {
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                [prefix.as_str(), "/", name.as_str()].concat()
            };
            match slot {
                TreeSlot::Handle(handle) => {
                    let opened = self
                        .stores()
                        .blobs()
                        .put(BlobSource::Handle(handle.handle()))
                        .map_err(ApiError::from)
                        .map_err(EngineError::from)?;
                    let mode = self
                        .stores()
                        .resources()
                        .mode(handle.handle())
                        .map_err(ApiError::from)
                        .map_err(EngineError::from)?;
                    members.push(ManifestMember {
                        relative,
                        blob: opened.to_ref(),
                        mode,
                    });
                }
                TreeSlot::Tree(nested) => self.collect_tree(nested, relative, members)?,
            }
        }
        Ok(())
    }
}

/// Reject rc guards with hooks-only changed leaves.
///
/// # Errors
///
/// - [`EngineError::ChangedInRc`] for guards with changed leaves.
pub(super) fn check_rc_guards(finished: &BTreeMap<Route, ModelDocument>) -> Result<()> {
    for (destination, document) in finished {
        let Data::Rc(found) = &document.data else {
            continue;
        };
        let display = destination.display();
        for (section, entries) in [
            ("profile", &found.profile),
            ("config", &found.config),
            ("final", &found.final_entries),
        ] {
            for entry in entries {
                if entry.when.as_ref().is_some_and(|gate| gate.holds_changed()) {
                    return Err(EngineError::ChangedInRc {
                        display: display.clone(),
                        section: section.to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Fill hashes over finished documents.
///
/// # Errors
///
/// - [`EngineError::Model`] for documents failing hashes.
pub(super) fn fill_hashes(finished: &mut BTreeMap<Route, ModelDocument>) -> Result<()> {
    for document in finished.values_mut() {
        let name = document.destination.display();
        document
            .fill_hash()
            .map_err(|error| confit_model::error::Error::Unhashable {
                document: name,
                reason: error.to_string(),
            })?;
    }
    Ok(())
}
