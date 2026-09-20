//! Drift
//!
//! Comparison results between recorded plans and disk.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{ManifestDocument, ManifestMember, StructuredFormat, Table};
use crate::fs::TreeMemberRead;
use crate::ids::{DocPath, ReadOutcome};
use crate::plan::{Bundle, opaque_label};

/// Manual edit behind one recorded path.
///
/// Old holds recorded content. New holds disk content.
/// Missing sides read as None, so an explicit null value
/// stays distinct from an absent key.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Structured or link leaf differs between recorded and disk.
    Key {
        /// Holds the recorded document path.
        path: DocPath,
        /// Holds the dotted leaf key, link uses `target`.
        key: String,
        /// Holds the recorded leaf value, None when the key reads absent.
        old: Option<serde_json::Value>,
        /// Holds the disk leaf value, None when the key reads absent.
        new: Option<serde_json::Value>,
    },
    /// Text content differs between recorded and disk.
    Hunk {
        /// Holds the recorded document path.
        path: DocPath,
        /// Holds the unified diff from recorded to disk.
        hunks: String,
    },
    /// Recorded path reads as absent on disk.
    Missing {
        /// Holds the recorded document path.
        path: DocPath,
    },
    /// Recorded path fails to read on disk.
    Unreadable {
        /// Holds the recorded document path.
        path: DocPath,
        /// Holds the raw failure detail from the read.
        reason: String,
    },
}

impl Drift {
    /// Reads the drift path.
    ///
    /// # Returns
    ///
    /// The recorded path for key, hunk, missing, plus unreadable entries.
    ///
    pub fn path(&self) -> &DocPath {
        match self {
            Self::Key { path, .. }
            | Self::Hunk { path, .. }
            | Self::Missing { path }
            | Self::Unreadable { path, .. } => path,
        }
    }

    /// Renders drift entries as display lines.
    ///
    /// Key edits read `~ path: key = old -> new`. Added keys
    /// read `+ path: key = new`, removed keys read
    /// `- path: key = old`. Hunks land verbatim. Missing and
    /// unreadable lines carry the outside config framing.
    ///
    /// # Arguments
    ///
    /// * `entries` - the drift entries under display.
    ///
    /// # Returns
    ///
    /// One or more display lines per drift entry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::drift::Drift;
    /// use confit_core::ids::DocPath;
    ///
    /// let entries = vec![
    ///     Drift::Key {
    ///         path: DocPath::new("app.toml"),
    ///         key: "tools.bat".to_string(),
    ///         old: Some(serde_json::json!("old")),
    ///         new: Some(serde_json::json!("new")),
    ///     },
    ///     Drift::Missing { path: DocPath::new("note") },
    /// ];
    /// assert_eq!(
    ///     Drift::lines(&entries),
    ///     vec![
    ///         "~ app.toml: tools.bat = old -> new".to_string(),
    ///         "note: manually deleted. changed outside config: add to config or the next apply loses them"
    ///             .to_string(),
    ///     ]
    /// );
    /// ```
    pub fn lines(entries: &[Drift]) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in entries {
            match entry {
                Drift::Key {
                    path,
                    key,
                    old,
                    new,
                } => match (old, new) {
                    (Some(old_value), Some(new_value)) => {
                        lines.push(format!(
                            "~ {}: {} = {} -> {}",
                            path.as_str(),
                            key,
                            leaf_text(old_value),
                            leaf_text(new_value)
                        ));
                    }
                    (Some(old_value), None) => {
                        lines.push(format!(
                            "- {}: {} = {}",
                            path.as_str(),
                            key,
                            leaf_text(old_value)
                        ));
                    }
                    (None, Some(new_value)) => {
                        lines.push(format!(
                            "+ {}: {} = {}",
                            path.as_str(),
                            key,
                            leaf_text(new_value)
                        ));
                    }
                    (None, None) => {}
                },
                Drift::Hunk { hunks, .. } => {
                    for line in hunks.lines() {
                        if line.starts_with("---")
                            || line.starts_with("+++")
                            || line.starts_with("@@")
                        {
                            continue;
                        }
                        lines.push(line.to_string());
                    }
                }
                Drift::Missing { path } => {
                    lines.push(format!(
                        "{}: manually deleted. changed outside config: add to config or the next apply loses them",
                        path.as_str()
                    ));
                }
                Drift::Unreadable { path, reason } => {
                    lines.push(format!(
                        "cannot read '{}': {reason}. changed outside config: add to config or the next apply loses them",
                        path.as_str()
                    ));
                }
            }
        }
        lines
    }
}

/// Builds one recorded-to-desired unified hunk for plan updates.
///
/// Headers read `--- recorded` plus `+++ desired`, matching the
/// order-named headers from hunk drift.
///
/// # Arguments
///
/// * `old` - the recorded text under display.
/// * `new` - the desired text under display.
///
/// # Returns
///
/// Render-ready content lines from recorded to desired.
/// File markers never leave this function.
///
/// # Examples
///
/// ```rust
/// use confit_core::drift::recorded_hunk;
///
/// let hunks = recorded_hunk("old\n", "new\n");
/// assert!(hunks.contains("-old"));
/// assert!(hunks.contains("+new"));
/// ```
pub fn recorded_hunk(old: &str, new: &str) -> String {
    content_hunk(old, new)
}

/// One drift side order selecting old/new assignment.
///
/// RecordedFirst keeps recorded bytes as old, disk as new.
/// DiskFirst keeps disk bytes as old, recorded as new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftOrder {
    /// Recorded bytes read as old, disk as new.
    RecordedFirst,
    /// Disk bytes read as old, recorded as new.
    DiskFirst,
}

impl Bundle {
    /// Reports manual edits between recorded documents and disk.
    ///
    /// Renders each recorded document then snapshots its path.
    /// Absent paths report missing. Unreadable paths report
    /// the failure detail. Structured documents diff leaf by
    /// leaf. Text documents diff with unified hunks. Link
    /// documents compare target strings. Opaque documents
    /// compare raw bytes with hash plus size values. Tree
    /// documents walk their destination folder member by
    /// member, ignoring hand-placed extras.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - the disk reader mapping paths to outcomes.
    /// * `snapshot_tree` - the disk walker mapping destination
    ///   folders to relative member reads.
    /// * `order` - the side order under assigning old and new
    ///
    /// # Returns
    ///
    /// Drift entries in recorded path order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::drift::{Drift, DriftOrder};
    /// use confit_core::ids::{DocPath, ReadOutcome};
    /// use confit_core::plan::Bundle;
    /// use std::collections::BTreeMap;
    ///
    /// let mut previous = Bundle::empty();
    /// previous.manifest.documents = vec![ManifestDocument::new(
    ///     DocPath::new("note"),
    ///     ManifestData::Text { content: "hi".into(), mode: None },
    /// )];
    /// let drifts = previous.drift(&|_| ReadOutcome::Absent, &|_| BTreeMap::new(), DriftOrder::RecordedFirst);
    /// assert_eq!(
    ///     Drift::lines(&drifts),
    ///     vec![
    ///         "note: manually deleted. changed outside config: add to config or the next apply loses them"
    ///             .to_string()
    ///     ]
    /// );
    /// ```
    pub fn drift(
        &self,
        snapshot: &dyn Fn(&DocPath) -> ReadOutcome,
        snapshot_tree: &dyn Fn(&DocPath) -> BTreeMap<String, TreeMemberRead>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        let mut out = Vec::new();
        for document in &self.manifest.documents {
            if let Some(members) = document.data.tree_members() {
                out.extend(document.tree_drift(
                    members,
                    &self.blobs,
                    &snapshot_tree(&document.path),
                    order,
                ));
                continue;
            }
            let recorded_bytes = match document.bytes(&self.blobs) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            match snapshot(&document.path) {
                ReadOutcome::Absent => out.push(Drift::Missing {
                    path: document.path.clone(),
                }),
                ReadOutcome::Unreadable { reason } => out.push(Drift::Unreadable {
                    path: document.path.clone(),
                    reason,
                }),
                ReadOutcome::Present { bytes: disk, mode } => {
                    out.extend(document.disk_drift(&recorded_bytes, &disk, mode, order));
                }
            }
        }
        out
    }
}

impl ManifestDocument {
    /// Collects drift entries for one present disk path.
    ///
    /// Modes compare only while the recorded document carries
    /// one. Recorded None skips the mode check, keeping umask
    /// default files quiet.
    ///
    /// # Arguments
    ///
    /// * `recorded` - the rendered recorded bytes.
    /// * `disk` - the disk bytes under comparing.
    /// * `disk_mode` - the disk permission bits, holding None
    ///   while the backend holds no mode.
    /// * `order` - the side order under assigning old and new
    ///
    /// # Returns
    ///
    /// Drift entries for the path, empty while bytes plus
    /// recorded modes agree.
    pub(crate) fn disk_drift(
        &self,
        recorded: &[u8],
        disk: &[u8],
        disk_mode: Option<u32>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        use crate::document::{ManifestData, render_mode};

        let (first, second) = match order {
            DriftOrder::RecordedFirst => (recorded, disk),
            DriftOrder::DiskFirst => (disk, recorded),
        };
        let mut out = match &self.data {
            ManifestData::Structured { format, data } => {
                if let Some(disk_table) = parse_disk_table(*format, disk) {
                    match order {
                        DriftOrder::RecordedFirst => structured_keys(&self.path, data, &disk_table),
                        DriftOrder::DiskFirst => structured_keys(&self.path, &disk_table, data),
                    }
                } else {
                    push_hunk(&self.path, first, second)
                }
            }
            ManifestData::Text { .. } | ManifestData::Rc(_) => {
                if recorded != disk {
                    push_hunk(&self.path, first, second)
                } else {
                    Vec::new()
                }
            }
            ManifestData::Link { target } => {
                let disk_target = String::from_utf8_lossy(disk);
                if disk_target.as_ref() != target {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(target.clone()),
                            serde_json::Value::String(disk_target.into_owned()),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(disk_target.into_owned()),
                            serde_json::Value::String(target.clone()),
                        ),
                    };
                    vec![Drift::Key {
                        path: self.path.clone(),
                        key: "target".to_string(),
                        old: Some(old),
                        new: Some(new),
                    }]
                } else {
                    Vec::new()
                }
            }
            ManifestData::Opaque { .. } => {
                if recorded != disk {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(opaque_label(recorded)),
                            serde_json::Value::String(opaque_label(disk)),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(opaque_label(disk)),
                            serde_json::Value::String(opaque_label(recorded)),
                        ),
                    };
                    vec![Drift::Key {
                        path: self.path.clone(),
                        key: "content".to_string(),
                        old: Some(old),
                        new: Some(new),
                    }]
                } else {
                    Vec::new()
                }
            }
            ManifestData::Tree { .. } => Vec::new(),
        };
        if let Some(wanted) = self.mode()
            && let Some(seen) = disk_mode
            && wanted != seen
        {
            let (old, new) = match order {
                DriftOrder::RecordedFirst => (
                    serde_json::Value::String(render_mode(wanted)),
                    serde_json::Value::String(render_mode(seen)),
                ),
                DriftOrder::DiskFirst => (
                    serde_json::Value::String(render_mode(seen)),
                    serde_json::Value::String(render_mode(wanted)),
                ),
            };
            out.push(Drift::Key {
                path: self.path.clone(),
                key: "mode".to_string(),
                old: Some(old),
                new: Some(new),
            });
        }
        out
    }

    /// Collects drift entries for one tree destination walk.
    ///
    /// Members compare by relative path against the disk
    /// reads. Missing members report missing under their
    /// joined path. Changed bytes report hash plus size
    /// labels under the member key. Changed modes report
    /// under the member mode key. Disk extras stay quiet,
    /// hand-placed files never drift.
    ///
    /// # Arguments
    ///
    /// * `members` - the recorded tree members under comparing.
    /// * `blobs` - the raw blob bytes under content hashes.
    /// * `disk` - the relative disk reads under comparing.
    /// * `order` - the side order under assigning old and new
    ///
    /// # Returns
    ///
    /// Drift entries in manifest order.
    pub(crate) fn tree_drift(
        &self,
        members: &[ManifestMember],
        blobs: &BTreeMap<String, Vec<u8>>,
        disk: &BTreeMap<String, TreeMemberRead>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        use crate::document::render_mode;

        let mut out = Vec::new();
        for member in members {
            let member_path = DocPath::new(format!("{}/{}", self.path.as_str(), member.relative));
            let recorded = match blobs.get(&member.blob) {
                Some(bytes) => bytes,
                None => continue,
            };
            match disk.get(&member.relative) {
                None => out.push(Drift::Missing { path: member_path }),
                Some(TreeMemberRead::Unreadable { reason }) => out.push(Drift::Unreadable {
                    path: member_path,
                    reason: reason.clone(),
                }),
                Some(TreeMemberRead::Present { bytes, mode }) => {
                    if *bytes != *recorded {
                        let (old, new) = match order {
                            DriftOrder::RecordedFirst => (
                                serde_json::Value::String(opaque_label(recorded)),
                                serde_json::Value::String(opaque_label(bytes)),
                            ),
                            DriftOrder::DiskFirst => (
                                serde_json::Value::String(opaque_label(bytes)),
                                serde_json::Value::String(opaque_label(recorded)),
                            ),
                        };
                        out.push(Drift::Key {
                            path: self.path.clone(),
                            key: member.relative.clone(),
                            old: Some(old),
                            new: Some(new),
                        });
                    }
                    if let Some(seen) = mode
                        && *seen != member.mode
                    {
                        let (old, new) = match order {
                            DriftOrder::RecordedFirst => (
                                serde_json::Value::String(render_mode(member.mode)),
                                serde_json::Value::String(render_mode(*seen)),
                            ),
                            DriftOrder::DiskFirst => (
                                serde_json::Value::String(render_mode(*seen)),
                                serde_json::Value::String(render_mode(member.mode)),
                            ),
                        };
                        out.push(Drift::Key {
                            path: self.path.clone(),
                            key: format!("{}:mode", member.relative),
                            old: Some(old),
                            new: Some(new),
                        });
                    }
                }
            }
        }
        out
    }
}

/// Renders one scalar leaf value as display text.
fn leaf_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

/// Builds render-ready hunk content from first text to second text.
///
/// File markers never leave this function. The `-` plus `+`
/// sides carry the direction, so headers add nothing.
///
/// # Arguments
///
/// * `first` - the old text under diffing.
/// * `second` - the new text under diffing.
///
/// # Returns
///
/// Content lines alone, additions plus removals plus context.
fn content_hunk(first: &str, second: &str) -> String {
    diffy::create_patch(first, second)
        .to_string()
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds one hunk from first bytes to second bytes.
///
/// Callers pass sides in display order, so direction rides
/// the bytes alone.
fn push_hunk(path: &DocPath, first: &[u8], second: &[u8]) -> Vec<Drift> {
    let old = String::from_utf8_lossy(first);
    let new = String::from_utf8_lossy(second);
    let hunks = content_hunk(&old, &new);
    vec![Drift::Hunk {
        path: path.clone(),
        hunks,
    }]
}

/// Parses disk bytes to a table by format.
fn parse_disk_table(format: StructuredFormat, disk: &[u8]) -> Option<Table> {
    let text = std::str::from_utf8(disk).ok()?;
    match format {
        StructuredFormat::Json => serde_json::from_str(text).ok(),
        StructuredFormat::Toml => toml::from_str(text).ok(),
        StructuredFormat::Yaml => noyalib::from_str(text).ok(),
    }
}

/// Diffs two tables leaf by leaf into key entries.
fn structured_keys(path: &DocPath, old: &Table, new: &Table) -> Vec<Drift> {
    let mut old_flat = BTreeMap::new();
    flatten_table(old, "", &mut old_flat);
    let mut new_flat = BTreeMap::new();
    flatten_table(new, "", &mut new_flat);
    let mut keys = BTreeSet::new();
    keys.extend(old_flat.keys().cloned());
    keys.extend(new_flat.keys().cloned());
    let mut out = Vec::new();
    for key in keys {
        let old_value = old_flat.get(&key).cloned();
        let new_value = new_flat.get(&key).cloned();
        if old_value != new_value {
            out.push(Drift::Key {
                path: path.clone(),
                key: key.clone(),
                old: old_value,
                new: new_value,
            });
        }
    }
    out
}

/// Flattens one table into dotted leaf entries.
fn flatten_table(table: &Table, prefix: &str, out: &mut BTreeMap<String, serde_json::Value>) {
    for (key, value) in table {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        flatten_value(&full, value, out);
    }
}

/// Flattens one value into dotted leaf entries.
fn flatten_value(
    key: &str,
    value: &serde_json::Value,
    out: &mut BTreeMap<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (inner, item) in map {
                flatten_value(&format!("{key}.{inner}"), item, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_value(&format!("{key}[{index}]"), item, out);
            }
        }
        _ => {
            out.insert(key.to_string(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ManifestData, ManifestDocument};

    fn text_doc(path: &str, content: &str) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Text {
                content: content.to_string(),
                mode: None,
            },
        )
    }

    fn structured_doc(path: &str, pairs: &[(&str, serde_json::Value)]) -> ManifestDocument {
        let data: Table = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect();
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Structured {
                format: StructuredFormat::Toml,
                data,
            },
        )
    }

    fn opaque_doc(path: &str, bytes: &[u8]) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Opaque {
                blob: crate::plan::sha256_hex(bytes),
                mode: None,
            },
        )
    }

    fn with_hashes(documents: Vec<ManifestDocument>) -> Bundle {
        with_blobs(documents, BTreeMap::new())
    }

    fn with_blobs(documents: Vec<ManifestDocument>, blobs: BTreeMap<String, Vec<u8>>) -> Bundle {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = docs;
        previous.blobs = blobs;
        previous
    }

    fn opaque_blobs(pairs: &[&[u8]]) -> BTreeMap<String, Vec<u8>> {
        pairs
            .iter()
            .map(|bytes| (crate::plan::sha256_hex(bytes), bytes.to_vec()))
            .collect()
    }

    #[test]
    fn drift_emits_key_diffs_for_changed_added_removed() {
        let recorded = with_hashes(vec![structured_doc(
            "app.toml",
            &[
                ("kept", serde_json::json!("same")),
                ("changed", serde_json::json!("old")),
                ("removed", serde_json::json!("gone")),
            ],
        )]);
        let disk = b"kept = \"same\"\nchanged = \"new\"\nadded = \"fresh\"\n";
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: disk.to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        let mut keys: Vec<String> = drifts
            .iter()
            .map(|entry| match entry {
                Drift::Key { key, .. } => key.clone(),
                _ => panic!("structured drift uses keys"),
            })
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["added", "changed", "removed"]);
        let changed = drifts
            .iter()
            .find(|entry| matches!(entry, Drift::Key { key, .. } if key == "changed"))
            .cloned();
        match changed {
            Some(Drift::Key { old, new, .. }) => {
                assert_eq!(old, Some(serde_json::json!("old")));
                assert_eq!(new, Some(serde_json::json!("new")));
            }
            _ => panic!("changed key reports old and new"),
        }
    }

    #[test]
    fn drift_separates_null_value_from_missing_key() {
        let data: Table = [
            ("kept_null".to_string(), serde_json::Value::Null),
            ("gone".to_string(), serde_json::json!("x")),
            ("gone_null".to_string(), serde_json::Value::Null),
        ]
        .into_iter()
        .collect();
        let recorded = with_hashes(vec![ManifestDocument::new(
            DocPath::new("app.json"),
            ManifestData::Structured {
                format: StructuredFormat::Json,
                data,
            },
        )]);
        let disk = b"{\"kept_null\": null, \"fresh\": \"y\", \"fresh_null\": null}";
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: disk.to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        let mut keys: Vec<String> = drifts
            .iter()
            .map(|entry| match entry {
                Drift::Key { key, .. } => key.clone(),
                _ => panic!("structured drift uses keys"),
            })
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["fresh", "fresh_null", "gone", "gone_null"]);
        for entry in &drifts {
            match entry {
                Drift::Key { key, old, new, .. } if key == "gone_null" => {
                    assert_eq!(old, &Some(serde_json::Value::Null));
                    assert_eq!(new, &None);
                }
                Drift::Key { key, old, new, .. } if key == "fresh_null" => {
                    assert_eq!(old, &None);
                    assert_eq!(new, &Some(serde_json::Value::Null));
                }
                Drift::Key { .. } => {}
                _ => panic!("structured drift uses keys"),
            }
        }
        let lines = Drift::lines(&drifts);
        assert!(
            lines
                .iter()
                .any(|line| line == "- app.json: gone_null = null"),
            "removed null reads as removal: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "+ app.json: fresh_null = null"),
            "added null reads as addition: {lines:?}"
        );
    }

    #[test]
    fn drift_emits_hunks_for_text() {
        let recorded = with_hashes(vec![text_doc("note", "hello\n")]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: b"hello world\n".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Hunk { path, hunks } => {
                assert_eq!(path.as_str(), "note");
                assert!(hunks.contains("hello"));
            }
            _ => panic!("text drift uses hunks"),
        }
    }

    #[test]
    fn drift_compares_link_targets() {
        let mut recorded = vec![ManifestDocument::new(
            DocPath::new("shortcut"),
            ManifestData::Link {
                target: "old-dest".to_string(),
            },
        )];
        for document in &mut recorded {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = recorded;
        let drifts = previous.drift(
            &|_| ReadOutcome::Present {
                bytes: b"new-dest".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Key { key, old, new, .. } => {
                assert_eq!(key, "target");
                assert_eq!(old, &Some(serde_json::json!("old-dest")));
                assert_eq!(new, &Some(serde_json::json!("new-dest")));
            }
            _ => panic!("link drift uses target key"),
        }
    }

    #[test]
    fn opaque_drift_reports_hash_plus_size() {
        let recorded = with_blobs(
            vec![opaque_doc("bin", &[0xFF, 0x00])],
            opaque_blobs(&[&[0xFF, 0x00]]),
        );
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: vec![0xFF, 0x01],
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Key { key, old, new, .. } => {
                assert_eq!(key, "content");
                assert!(
                    old.as_ref()
                        .and_then(|value| value.as_str())
                        .map(|item| item.starts_with("sha256:"))
                        .unwrap_or(false)
                );
                assert!(
                    old.as_ref()
                        .and_then(|value| value.as_str())
                        .map(|item| item.contains("(2 bytes)"))
                        .unwrap_or(false)
                );
                assert!(
                    new.as_ref()
                        .and_then(|value| value.as_str())
                        .map(|item| item.starts_with("sha256:"))
                        .unwrap_or(false)
                );
                assert!(old != new);
            }
            _ => panic!("opaque drift uses content key"),
        }
    }

    #[test]
    fn opaque_drift_stays_quiet_on_equal_bytes() {
        let recorded = with_blobs(
            vec![opaque_doc("bin", &[0xFF, 0x00])],
            opaque_blobs(&[&[0xFF, 0x00]]),
        );
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: vec![0xFF, 0x00],
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert!(drifts.is_empty());
    }

    #[test]
    fn drift_lines_cover_all_kinds() {
        let entries = vec![
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.bat".to_string(),
                old: Some(serde_json::json!("old")),
                new: Some(serde_json::json!("new")),
            },
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.gone".to_string(),
                old: Some(serde_json::json!("old")),
                new: None,
            },
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.fresh".to_string(),
                old: None,
                new: Some(serde_json::json!("new")),
            },
            Drift::Missing {
                path: DocPath::new("gone"),
            },
        ];
        let lines = Drift::lines(&entries);
        assert_eq!(lines[0], "~ app.toml: tools.bat = old -> new");
        assert_eq!(lines[1], "- app.toml: tools.gone = old");
        assert_eq!(lines[2], "+ app.toml: tools.fresh = new");
        assert!(lines[3].contains("manually deleted"));
    }

    fn tree_doc() -> ManifestDocument {
        use crate::document::ManifestMember;

        ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![
                    ManifestMember {
                        relative: "changed.ttf".into(),
                        blob: crate::plan::sha256_hex(&[1]),
                        mode: 0o644,
                    },
                    ManifestMember {
                        relative: "gone.ttf".into(),
                        blob: crate::plan::sha256_hex(&[2]),
                        mode: 0o644,
                    },
                    ManifestMember {
                        relative: "remode.ttf".into(),
                        blob: crate::plan::sha256_hex(&[3]),
                        mode: 0o644,
                    },
                ],
            },
        )
    }

    fn tree_blobs() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            (crate::plan::sha256_hex(&[1]), vec![1]),
            (crate::plan::sha256_hex(&[2]), vec![2]),
            (crate::plan::sha256_hex(&[3]), vec![3]),
        ])
    }

    fn tree_disk() -> BTreeMap<String, crate::fs::TreeMemberRead> {
        use crate::fs::TreeMemberRead;

        BTreeMap::from([
            (
                "changed.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![9],
                    mode: Some(0o644),
                },
            ),
            (
                "remode.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![3],
                    mode: Some(0o600),
                },
            ),
            (
                "hand.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![7],
                    mode: Some(0o644),
                },
            ),
        ])
    }

    #[test]
    fn tree_drift_reports_missing_changed_mode() {
        let recorded = with_blobs(vec![tree_doc()], tree_blobs());
        let drifts = recorded.drift(
            &|_| ReadOutcome::Absent,
            &|_| tree_disk(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(drifts.len(), 3);
        assert!(matches!(&drifts[0], Drift::Key { key, .. } if key == "changed.ttf"));
        assert!(matches!(&drifts[1], Drift::Missing { path } if path.as_str() == "fonts/gone.ttf"));
        assert!(matches!(&drifts[2], Drift::Key { key, .. } if key == "remode.ttf:mode"));
        let lines = Drift::lines(&drifts);
        assert!(lines[0].starts_with("~ fonts: changed.ttf = sha256:"));
        assert!(lines[1].contains("manually deleted"));
        assert!(lines[2].starts_with("~ fonts: remode.ttf:mode = "));
    }

    #[test]
    fn tree_drift_stays_quiet_on_equal_manifest() {
        use crate::fs::TreeMemberRead;

        let recorded = with_blobs(vec![tree_doc()], tree_blobs());
        let disk = BTreeMap::from([
            (
                "changed.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![1],
                    mode: Some(0o644),
                },
            ),
            (
                "gone.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![2],
                    mode: Some(0o644),
                },
            ),
            (
                "remode.ttf".to_string(),
                TreeMemberRead::Present {
                    bytes: vec![3],
                    mode: Some(0o644),
                },
            ),
        ]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Absent,
            &|_| disk.clone(),
            DriftOrder::RecordedFirst,
        );
        assert!(drifts.is_empty());
    }

    #[test]
    fn drift_order_swaps_key_sides() {
        let recorded = with_hashes(vec![structured_doc(
            "app.toml",
            &[("name", serde_json::json!("desired"))],
        )]);
        let disk = b"name = \"disk\"\n";
        let disk_first = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: disk.to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::DiskFirst,
        );
        assert_eq!(disk_first.len(), 1);
        match &disk_first[0] {
            Drift::Key { old, new, .. } => {
                assert_eq!(old, &Some(serde_json::json!("disk")));
                assert_eq!(new, &Some(serde_json::json!("desired")));
            }
            _ => panic!("disk first swaps key sides"),
        }
        let recorded_first = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: disk.to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(recorded_first.len(), 1);
        match &recorded_first[0] {
            Drift::Key { old, new, .. } => {
                assert_eq!(old, &Some(serde_json::json!("desired")));
                assert_eq!(new, &Some(serde_json::json!("disk")));
            }
            _ => panic!("recorded first keeps key sides"),
        }
    }

    #[test]
    fn drift_order_directs_hunks() {
        let recorded = with_hashes(vec![text_doc("note", "desired\n")]);
        let disk_first = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: b"disk\n".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::DiskFirst,
        );
        assert_eq!(disk_first.len(), 1);
        match &disk_first[0] {
            Drift::Hunk { hunks, .. } => {
                assert!(hunks.contains("-disk"));
                assert!(hunks.contains("+desired"));
            }
            _ => panic!("disk first directs hunks"),
        }
        let recorded_first = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: b"disk\n".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(recorded_first.len(), 1);
        match &recorded_first[0] {
            Drift::Hunk { hunks, .. } => {
                assert!(hunks.contains("-desired"));
                assert!(hunks.contains("+disk"));
            }
            _ => panic!("recorded first directs hunks"),
        }
    }

    #[test]
    fn steady_order_directs_recorded_to_disk() {
        let recorded = with_hashes(vec![text_doc("note", "recorded\n")]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: b"disk\n".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::RecordedFirst,
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Hunk { hunks, .. } => {
                assert!(
                    hunks.contains("-recorded"),
                    "steady hunk removes recorded: {hunks}"
                );
                assert!(hunks.contains("+disk"), "steady hunk adds disk: {hunks}");
                assert!(
                    !hunks.contains("+recorded"),
                    "steady hunk never adds recorded: {hunks}"
                );
                assert!(
                    !hunks.contains("-disk"),
                    "steady hunk never removes disk: {hunks}"
                );
            }
            _ => panic!("steady order directs recorded to disk"),
        }
    }

    #[test]
    fn first_run_order_directs_disk_to_desired() {
        let recorded = with_hashes(vec![text_doc("note", "desired\n")]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: b"disk\n".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
            DriftOrder::DiskFirst,
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Hunk { hunks, .. } => {
                assert!(
                    hunks.contains("-disk"),
                    "first-run hunk removes disk: {hunks}"
                );
                assert!(
                    hunks.contains("+desired"),
                    "first-run hunk adds desired: {hunks}"
                );
                assert!(
                    !hunks.contains("+disk"),
                    "first-run hunk never adds disk: {hunks}"
                );
                assert!(
                    !hunks.contains("-desired"),
                    "first-run hunk never removes desired: {hunks}"
                );
            }
            _ => panic!("first-run order directs disk to desired"),
        }
    }

    #[test]
    fn recorded_hunk_holds_content_without_markers() {
        let hunks = recorded_hunk("old\n", "new\n");
        assert!(
            hunks.contains("-old"),
            "rc hunk removes old content: {hunks}"
        );
        assert!(hunks.contains("+new"), "rc hunk adds new content: {hunks}");
        assert!(
            !hunks.lines().any(|line| {
                line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@")
            }),
            "rc hunk renders no markers: {hunks}"
        );
    }
}
