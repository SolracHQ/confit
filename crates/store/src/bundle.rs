//! Bundle
//!
//! Portable bundle archives holding manifests and blobs.

pub mod error;
mod pack;
mod unpack;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_model::document::{BlobRef, ManifestDocument};
use confit_model::hook::Hook;
use confit_model::manifest::Manifest;
use confit_model::plan::{DocumentStatus, Summary};

use crate::archive::ArchiveStore;
use crate::blob::BlobStore;
use error::{BundleError, Result};

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Gzip level for the outer bundle tar.
pub(crate) const BUNDLE_GZIP_LEVEL: u32 = 0;

/// Bundle manifest file name inside the archive.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// Bundle blob folder prefix inside the archive.
pub(crate) const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Bundle format version written by every bundle build.
///
pub const BUNDLE_VERSION: u32 = 7;

/// Versioned desired state written by bundle builds.
///
/// The manifest holds version, documents, and
/// hooks as the only document language. The blob map holds
/// blob refs under content hashes beside it.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Holds the portable manifest as the only document language.
    pub manifest: Manifest,
    /// Holds blob refs under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, BlobRef>,
}

/// Portable bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
/// Writes source blob bytes from the pool with fallback
/// across homes. Reads stage entries through the archive
/// spill into the cache.
#[derive(Debug, Clone)]
pub struct BundleStore {
    pub(crate) archives: Arc<ArchiveStore>,
    pub(crate) blobs: Arc<BlobStore>,
}

impl Bundle {
    /// Builds an empty manifest with the current version.
    ///
    /// # Returns
    ///
    /// The bundle holding version and empty documents.
    ///
    pub fn empty() -> Self {
        Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: Vec::new(),
            },
            blobs: BTreeMap::new(),
        }
    }

    /// Builds the desired state bundle from documents.
    ///
    /// Fills data hashes, then sorts documents by destination.
    /// The caller holds one document per destination.
    /// Counts generate through `summary` against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `documents` - desired documents in pipeline order, unique per destination.
    /// * `hooks` - desired hooks in declaration order, merged downstream.
    ///
    /// # Returns
    ///
    /// The built bundle.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Unhashable`] for hash failures.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::routes::{Route, RouteBase};
    /// use confit_store::bundle::Bundle;
    ///
    /// let document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// let outcome = Bundle::build(vec![document], Vec::new());
    /// let previous = Bundle::empty();
    /// assert!(matches!(outcome, Ok(bundle) if bundle.summary(&previous).create == 1));
    /// ```
    pub fn build(mut documents: Vec<ManifestDocument>, hooks: Vec<Hook>) -> Result<Self> {
        for document in &mut documents {
            document
                .fill_hash()
                .map_err(|error| BundleError::Unhashable {
                    document: document.destination.display(),
                    reason: error.to_string(),
                })?;
        }
        documents.sort_by_key(|document| document.destination.display());
        Ok(Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents,
                hooks,
            },
            blobs: BTreeMap::new(),
        })
    }

    /// Counts lifecycle states against a previous manifest.
    ///
    /// Opaque kind changes count as updates, other kind changes
    /// count as create and delete.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous manifest with filled hashes.
    ///
    /// # Returns
    ///
    /// Create, update, and delete counts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::routes::{Route, RouteBase};
    /// use confit_store::bundle::Bundle;
    ///
    /// let mut previous = Bundle::empty();
    /// previous.manifest.documents = vec![ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// )];
    /// let bundle = Bundle::build(
    ///     vec![ManifestDocument::new(
    ///         Route::new(RouteBase::Home, "note").unwrap(),
    ///         ManifestData::Text { content: "changed".into(), mode: None, unmanaged: false},
    ///     )],
    ///     Vec::new(),
    /// );
    /// assert!(matches!(bundle, Ok(bundle) if bundle.summary(&previous).update == 1));
    /// ```
    pub fn summary(&self, previous: &Bundle) -> Summary {
        let mut summary = Summary {
            create: 0,
            update: 0,
            delete: 0,
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for document in &self.manifest.documents {
            seen.insert(document.key());
            match document.status(&previous.manifest) {
                DocumentStatus::Create => summary.create += 1,
                DocumentStatus::Update => summary.update += 1,
                DocumentStatus::Unchanged => {}
            }
        }
        for recorded in &previous.manifest.documents {
            if !seen.contains(&recorded.key()) && !recorded.superseded_by(&self.manifest.documents)
            {
                summary.delete += 1;
            }
        }
        summary
    }
}

impl BundleStore {
    /// Builds a bundle store over shared archive and blob stores.
    ///
    /// Both handles arrive shared from store construction.
    pub fn new(archives: Arc<ArchiveStore>, blobs: Arc<BlobStore>) -> Self {
        Self { archives, blobs }
    }
}

/// Ensures one bundle destination carries the bundle extension.
///
/// Bare paths gain the suffix, so creators always emit
/// bundles. Slot outputs never pass here and keep their
/// own names.
pub fn ensure_bundle_extension(dest: &Path) -> PathBuf {
    if dest
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(BUNDLE_EXTENSION))
    {
        dest.to_path_buf()
    } else {
        let mut name = dest.as_os_str().to_owned();
        name.push(".cb");
        PathBuf::from(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_output_gains_cb_suffix() {
        let out = ensure_bundle_extension(Path::new("plan"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("bare output gains suffix"),
        }
    }

    #[test]
    fn cb_suffix_stays_unchanged() {
        let out = ensure_bundle_extension(Path::new("plan.cb"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("cb output stays unchanged"),
        }
    }

    #[test]
    fn cb_suffix_match_reads_case_insensitive() {
        let out = ensure_bundle_extension(Path::new("plan.CB"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.CB"),
            None => panic!("uppercase cb stays untouched"),
        }
    }
}
