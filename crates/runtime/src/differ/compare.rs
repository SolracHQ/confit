//! Recorded against disk comparison.

use std::collections::BTreeMap;
use std::path::Path;

use confit_core::sha::Sha;
use confit_model::document::{Data, Document, StructuredFormat, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;

use super::Difference;
use super::DifferenceOrder;
use super::disk::{DiskReader, FileBytes, FileDigest};
use super::ordered::Ordered;
use crate::manifest::status::mode;
use crate::render::render_document;
use confit_core::error::ContextualResult;
use confit_core::error::{ContextualCause, DiffWarning, WithContext, contextual};

/// Recorded against disk comparator.
pub(crate) struct Comparator<'a> {
    /// Read capabilities for one run.
    stores: &'a Stores,
    /// Difference side order.
    ordered: Ordered,
    /// Non-blocking warnings for failed states.
    warnings: Vec<DiffWarning>,
}

impl<'a> Comparator<'a> {
    /// Build one comparator over shared stores with its side order.
    pub(crate) fn new(stores: &'a Stores, order: DifferenceOrder) -> Self {
        Self {
            stores,
            ordered: Ordered::new(order),
            warnings: Vec::new(),
        }
    }

    /// Report manual edits between recorded documents and disk.
    ///
    /// - Entries follow recorded destination order.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    pub(crate) fn run(
        &mut self,
        manifest: &crate::manifest::Manifest,
    ) -> ContextualResult<(Vec<Difference>, Vec<DiffWarning>)> {
        let mut out = Vec::new();
        for document in &manifest.documents {
            out.extend(self.compare_one(document)?);
        }
        Ok((out, std::mem::take(&mut self.warnings)))
    }

    /// Collect difference entries for one recorded document.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    fn compare_one(&mut self, document: &Document) -> ContextualResult<Vec<Difference>> {
        if let Data::Tree(found) = &document.data {
            return self.compare_tree(&document.path, &found.members);
        }
        if matches!(document.data, Data::Opaque(_)) {
            return self.compare_digest_handle(document);
        }
        DiskReader::digest(document, |FileDigest { digest, mode }| {
            let recorded = match render_document(document) {
                Ok(found) => found,
                Err(leaf) => {
                    let contextual = contextual(leaf).with_document(document.path.display());
                    let reason = contextual.cause().to_string();
                    self.warnings
                        .push(DiffWarning::warn(document.path.display(), contextual));
                    return Ok(vec![Difference::Unreadable {
                        path: document.path.clone(),
                        reason,
                    }]);
                }
            };
            if digest.len == recorded.len() as u64 && digest.sha == Sha::hash(&recorded) {
                let mut out = Vec::new();
                self.append_mode(document, mode, &mut out);
                return Ok(out);
            }
            DiskReader::read_bytes(document, |FileBytes { bytes, mode }| {
                Ok(self.compare_rendered(document, &recorded, &bytes, mode))
            })
        })
    }

    /// Collect difference entries for one tree destination walk.
    ///
    /// - Only manifest members produce entries.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    fn compare_tree(
        &mut self,
        destination: &Route,
        members: &BTreeMap<String, TreeMember>,
    ) -> ContextualResult<Vec<Difference>> {
        let base = destination.expand().with_document(destination.display())?;
        let mut out = Vec::new();
        for (relative, member) in members {
            out.extend(self.compare_member(destination, &base, relative, member)?);
        }
        Ok(out)
    }

    /// Collect difference entries for one tree member.
    fn compare_member(
        &mut self,
        destination: &Route,
        base: &Path,
        relative: &str,
        member: &TreeMember,
    ) -> ContextualResult<Vec<Difference>> {
        let member_path = destination.join(relative);
        DiskReader::probe_member(
            base,
            relative,
            &member_path,
            |FileDigest { digest, mode }| {
                let mut out = Vec::new();
                let resolved = member.handle().sha();
                if *resolved != digest.sha {
                    let recorded = self.recorded_label(&member_path, resolved);
                    let disk = digest.label();
                    out.push(self.digest(destination, relative, &recorded, &disk));
                }
                if let Some(seen) = mode
                    && seen != member.mode()
                {
                    out.push(self.ordered.mode_key(
                        destination,
                        format!("{relative}:mode"),
                        member.mode(),
                        seen,
                    ));
                }
                Ok(out)
            },
        )
    }

    /// Collect difference entries for one opaque handle against disk.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
    fn compare_digest_handle(&mut self, document: &Document) -> ContextualResult<Vec<Difference>> {
        let Data::Opaque(found) = &document.data else {
            return Ok(Vec::new());
        };
        let sha = found.handle.sha();
        let mode = found.mode;
        DiskReader::digest(document, |FileDigest { digest, mode: seen }| {
            if *sha != digest.sha {
                let recorded = self.recorded_label(&document.path, sha);
                let disk = digest.label();
                return Ok(vec![self.digest(
                    &document.path,
                    "content",
                    &recorded,
                    &disk,
                )]);
            }
            if let Some(seen) = seen
                && seen != mode
            {
                return Ok(vec![self.ordered.mode_key(
                    &document.path,
                    "mode".to_string(),
                    mode,
                    seen,
                )]);
            }
            Ok(Vec::new())
        })
    }

    /// Collect difference entries for one present disk path.
    ///
    /// - Unmanaged documents stay quiet.
    fn compare_rendered(
        &self,
        document: &Document,
        recorded: &[u8],
        disk: &[u8],
        disk_mode: Option<u32>,
    ) -> Vec<Difference> {
        let mut out = match &document.data {
            Data::Structured(found) => {
                self.structured(&document.path, &found.data, found.format, recorded, disk)
            }
            Data::Text(found) => {
                if found.unmanaged {
                    Vec::new()
                } else {
                    self.text(&document.path, recorded, disk)
                }
            }
            Data::Rc(_) => self.text(&document.path, recorded, disk),
            Data::Link(found) => {
                let disk_target = String::from_utf8_lossy(disk);
                if disk_target.as_ref() != found.target.display() {
                    vec![self.ordered.key(
                        &document.path,
                        "target",
                        &found.target.display(),
                        &disk_target,
                    )]
                } else {
                    Vec::new()
                }
            }
            Data::Opaque(found) => {
                if found.unmanaged {
                    Vec::new()
                } else {
                    self.digest_rendered(&document.path, recorded, disk)
                }
            }
            Data::Tree(_) => Vec::new(),
        };
        self.append_mode(document, disk_mode, &mut out);
        out
    }

    /// Append one mode entry for mode-carrying documents.
    fn append_mode(&self, document: &Document, disk_mode: Option<u32>, out: &mut Vec<Difference>) {
        let wanted = mode(document);
        if let Some(wanted) = wanted
            && let Some(seen) = disk_mode
            && wanted != seen
        {
            out.push(
                self.ordered
                    .mode_key(&document.path, "mode".to_string(), wanted, seen),
            );
        }
    }

    /// Report structured entries with text fallback for unparsable disk.
    fn structured(
        &self,
        path: &Route,
        recorded: &BTreeMap<String, serde_json::Value>,
        format: StructuredFormat,
        recorded_bytes: &[u8],
        disk: &[u8],
    ) -> Vec<Difference> {
        if let Some(table) = Self::parse_table(format, disk) {
            self.ordered.structured(path, recorded, &table)
        } else {
            vec![self.ordered.text_diff(path, recorded_bytes, disk)]
        }
    }

    /// Report text entries for equal-checked bytes.
    fn text(&self, path: &Route, recorded: &[u8], disk: &[u8]) -> Vec<Difference> {
        if recorded == disk {
            Vec::new()
        } else {
            vec![self.ordered.text_diff(path, recorded, disk)]
        }
    }

    /// Report one digest entry for opaque rendered bytes.
    fn digest_rendered(&self, path: &Route, recorded: &[u8], disk: &[u8]) -> Vec<Difference> {
        if recorded == disk {
            Vec::new()
        } else {
            vec![self.ordered.key(
                path,
                "content",
                &Sha::hash(recorded).label(recorded.len() as u64),
                &Sha::hash(disk).label(disk.len() as u64),
            )]
        }
    }

    /// Build one digest key entry from two labels.
    fn digest(&self, path: &Route, key: &str, recorded: &str, disk: &str) -> Difference {
        self.ordered.key(path, key, recorded, disk)
    }

    /// Label one recorded hash with its pool byte count.
    ///
    /// - Missing pool entries fall back to the bare hash.
    fn recorded_label(&mut self, route: &Route, sha: &Sha) -> String {
        use std::io::Read as _;

        let pool_path = self.stores.pool().path(sha);
        match self.stores.pool().open(sha) {
            Ok(mut reader) => {
                let mut body = Vec::new();
                match reader.read_to_end(&mut body) {
                    Ok(_) => sha.label(body.len() as u64),
                    Err(fault) => {
                        let contextual = ContextualCause::from(fault).with_cache_path(pool_path);
                        self.warnings
                            .push(DiffWarning::warn(route.display(), contextual));
                        sha.hex()
                    }
                }
            }
            Err(contextual) => {
                self.warnings
                    .push(DiffWarning::warn(route.display(), contextual));
                sha.hex()
            }
        }
    }

    /// Parse disk bytes to a map by format.
    fn parse_table(
        format: StructuredFormat,
        disk: &[u8],
    ) -> Option<BTreeMap<String, serde_json::Value>> {
        let text = std::str::from_utf8(disk).ok()?;
        let value: serde_json::Value = match format {
            StructuredFormat::Json => serde_json::from_str(text).ok()?,
            StructuredFormat::Toml => toml::from_str(text).ok()?,
            StructuredFormat::Yaml => noyalib::from_str(text).ok()?,
        };
        if let serde_json::Value::Object(map) = value {
            Some(map.into_iter().collect())
        } else {
            None
        }
    }
}
