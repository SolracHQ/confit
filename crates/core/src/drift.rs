//! Drift
//!
//! Comparison results between recorded plans and disk.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{Document, StructuredFormat, Table, TreeMember};
use crate::fs::TreeMemberRead;
use crate::ids::{DocPath, ReadOutcome};
use crate::plan::{Plan, opaque_label};

/// Manual edit behind one recorded path.
///
/// Old holds recorded content. New holds disk content.
/// Null marks a missing leaf side.
///
/// # Examples
///
/// ```text
/// use confit_core::drift::Drift;
/// use confit_core::ids::DocPath;
///
/// let drift = Drift::Missing { path: DocPath::new("x") };
/// assert!(matches!(drift, Drift::Missing { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Structured or link leaf differs between recorded and disk.
    Key {
        /// Holds the recorded document path.
        path: DocPath,
        /// Holds the dotted leaf key, link uses `target`.
        key: String,
        /// Holds the recorded leaf value, Null when absent.
        old: serde_json::Value,
        /// Holds the disk leaf value, Null when absent.
        new: serde_json::Value,
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
    /// Renders drift entries as display lines.
    ///
    /// Key edits read `~ path: key = old -> new`.
    /// Hunks land verbatim. Missing and unreadable lines
    /// carry the outside config framing.
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
    /// ```text
    /// use confit_core::drift::Drift;
    /// use confit_core::ids::DocPath;
    ///
    /// let entries = vec![Drift::Missing { path: DocPath::new("note") }];
    /// assert!(matches!(Drift::lines(&entries).len(), 1));
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
                } => {
                    lines.push(format!(
                        "~ {}: {} = {} -> {}",
                        path.as_str(),
                        key,
                        leaf_text(old),
                        leaf_text(new)
                    ));
                }
                Drift::Hunk { hunks, .. } => {
                    for line in hunks.lines() {
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

impl Plan {
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
    ///
    /// # Returns
    ///
    /// Drift entries in recorded path order.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::{DocPath, ReadOutcome};
    /// use confit_core::plan::Plan;
    /// use std::collections::BTreeMap;
    ///
    /// let mut previous = Plan::empty();
    /// previous.documents = vec![Document::new(
    ///     DocPath::new("note"),
    ///     DocumentData::Text { content: "hi".into() },
    /// )];
    /// let drifts = previous.drift(&|_| ReadOutcome::Absent, &|_| BTreeMap::new());
    /// assert!(matches!(drifts.len(), 1));
    /// ```
    pub fn drift(
        &self,
        snapshot: &dyn Fn(&DocPath) -> ReadOutcome,
        snapshot_tree: &dyn Fn(&DocPath) -> BTreeMap<String, TreeMemberRead>,
    ) -> Vec<Drift> {
        let mut out = Vec::new();
        for document in &self.documents {
            if let Some(members) = document.data.tree_members() {
                out.extend(document.tree_drift(members, &snapshot_tree(&document.path)));
                continue;
            }
            let recorded_bytes = match document.bytes() {
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
                    out.extend(document.disk_drift(&recorded_bytes, &disk, mode));
                }
            }
        }
        out
    }
}

impl Document {
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
    ) -> Vec<Drift> {
        use crate::document::{DocumentData, render_mode};

        let mut out = match &self.data {
            DocumentData::Structured { format, data } => {
                if let Some(disk_table) = parse_disk_table(*format, disk) {
                    structured_keys(&self.path, data, &disk_table)
                } else {
                    push_hunk(&self.path, recorded, disk)
                }
            }
            DocumentData::Text { .. } | DocumentData::Rc(_) => {
                if recorded != disk {
                    push_hunk(&self.path, recorded, disk)
                } else {
                    Vec::new()
                }
            }
            DocumentData::Link { target } => {
                let disk_target = String::from_utf8_lossy(disk);
                if disk_target.as_ref() != target {
                    vec![Drift::Key {
                        path: self.path.clone(),
                        key: "target".to_string(),
                        old: serde_json::Value::String(target.clone()),
                        new: serde_json::Value::String(disk_target.into_owned()),
                    }]
                } else {
                    Vec::new()
                }
            }
            DocumentData::Opaque { .. } => {
                if recorded != disk {
                    vec![Drift::Key {
                        path: self.path.clone(),
                        key: "content".to_string(),
                        old: serde_json::Value::String(opaque_label(recorded)),
                        new: serde_json::Value::String(opaque_label(disk)),
                    }]
                } else {
                    Vec::new()
                }
            }
            DocumentData::Tree { .. } => Vec::new(),
        };
        if let Some(wanted) = self.mode()
            && let Some(seen) = disk_mode
            && wanted != seen
        {
            out.push(Drift::Key {
                path: self.path.clone(),
                key: "mode".to_string(),
                old: serde_json::Value::String(render_mode(wanted)),
                new: serde_json::Value::String(render_mode(seen)),
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
    /// * `disk` - the relative disk reads under comparing.
    ///
    /// # Returns
    ///
    /// Drift entries in manifest order.
    pub(crate) fn tree_drift(
        &self,
        members: &[TreeMember],
        disk: &BTreeMap<String, TreeMemberRead>,
    ) -> Vec<Drift> {
        use crate::document::render_mode;

        let mut out = Vec::new();
        for member in members {
            let member_path = DocPath::new(format!("{}/{}", self.path.as_str(), member.rel));
            match disk.get(&member.rel) {
                None => out.push(Drift::Missing { path: member_path }),
                Some(TreeMemberRead::Unreadable { reason }) => out.push(Drift::Unreadable {
                    path: member_path,
                    reason: reason.clone(),
                }),
                Some(TreeMemberRead::Present { bytes, mode }) => {
                    if *bytes != member.content {
                        out.push(Drift::Key {
                            path: self.path.clone(),
                            key: member.rel.clone(),
                            old: serde_json::Value::String(opaque_label(&member.content)),
                            new: serde_json::Value::String(opaque_label(bytes)),
                        });
                    }
                    if let Some(seen) = mode
                        && *seen != member.mode
                    {
                        out.push(Drift::Key {
                            path: self.path.clone(),
                            key: format!("{}:mode", member.rel),
                            old: serde_json::Value::String(render_mode(member.mode)),
                            new: serde_json::Value::String(render_mode(*seen)),
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

/// Builds one unified hunk from recorded bytes to disk bytes.
fn push_hunk(path: &DocPath, recorded: &[u8], disk: &[u8]) -> Vec<Drift> {
    let old = String::from_utf8_lossy(recorded);
    let new = String::from_utf8_lossy(disk);
    let patch = diffy::create_patch(&old, &new);
    vec![Drift::Hunk {
        path: path.clone(),
        hunks: patch.to_string(),
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
        let old_value = old_flat
            .get(&key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let new_value = new_flat
            .get(&key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
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
    use crate::document::DocumentData;

    fn text_doc(path: &str, content: &str) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Text {
                content: content.to_string(),
                mode: None,
            },
        )
    }

    fn structured_doc(path: &str, pairs: &[(&str, serde_json::Value)]) -> Document {
        let data: Table = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect();
        Document::new(
            DocPath::new(path),
            DocumentData::Structured {
                format: StructuredFormat::Toml,
                data,
            },
        )
    }

    fn opaque_doc(path: &str, bytes: &[u8]) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Opaque {
                content: bytes.to_vec(),
                mode: None,
            },
        )
    }

    fn with_hashes(documents: Vec<Document>) -> Plan {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = docs;
        previous
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
                assert_eq!(old, serde_json::json!("old"));
                assert_eq!(new, serde_json::json!("new"));
            }
            _ => panic!("changed key reports old and new"),
        }
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
        let mut recorded = vec![Document::new(
            DocPath::new("shortcut"),
            DocumentData::Link {
                target: "old-dest".to_string(),
            },
        )];
        for document in &mut recorded {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = recorded;
        let drifts = previous.drift(
            &|_| ReadOutcome::Present {
                bytes: b"new-dest".to_vec(),
                mode: None,
            },
            &|_| BTreeMap::new(),
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Key { key, old, new, .. } => {
                assert_eq!(key, "target");
                assert_eq!(old, &serde_json::json!("old-dest"));
                assert_eq!(new, &serde_json::json!("new-dest"));
            }
            _ => panic!("link drift uses target key"),
        }
    }

    #[test]
    fn opaque_drift_reports_hash_plus_size() {
        let recorded = with_hashes(vec![opaque_doc("bin", &[0xFF, 0x00])]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: vec![0xFF, 0x01],
                mode: None,
            },
            &|_| BTreeMap::new(),
        );
        assert_eq!(drifts.len(), 1);
        match &drifts[0] {
            Drift::Key { key, old, new, .. } => {
                assert_eq!(key, "content");
                assert!(
                    old.as_str()
                        .map(|item| item.starts_with("sha256:"))
                        .unwrap_or(false)
                );
                assert!(
                    old.as_str()
                        .map(|item| item.contains("(2 bytes)"))
                        .unwrap_or(false)
                );
                assert!(
                    new.as_str()
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
        let recorded = with_hashes(vec![opaque_doc("bin", &[0xFF, 0x00])]);
        let drifts = recorded.drift(
            &|_| ReadOutcome::Present {
                bytes: vec![0xFF, 0x00],
                mode: None,
            },
            &|_| BTreeMap::new(),
        );
        assert!(drifts.is_empty());
    }

    #[test]
    fn drift_lines_cover_all_kinds() {
        let entries = vec![
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.bat".to_string(),
                old: serde_json::json!("old"),
                new: serde_json::json!("new"),
            },
            Drift::Missing {
                path: DocPath::new("gone"),
            },
        ];
        let lines = Drift::lines(&entries);
        assert_eq!(lines[0], "~ app.toml: tools.bat = old -> new");
        assert!(lines[1].contains("manually deleted"));
    }

    fn tree_doc() -> Document {
        use crate::document::TreeMember;

        Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree {
                members: vec![
                    TreeMember {
                        rel: "changed.ttf".into(),
                        content: vec![1],
                        mode: 0o644,
                    },
                    TreeMember {
                        rel: "gone.ttf".into(),
                        content: vec![2],
                        mode: 0o644,
                    },
                    TreeMember {
                        rel: "remode.ttf".into(),
                        content: vec![3],
                        mode: 0o644,
                    },
                ],
            },
        )
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
        let recorded = with_hashes(vec![tree_doc()]);
        let drifts = recorded.drift(&|_| ReadOutcome::Absent, &|_| tree_disk());
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

        let recorded = with_hashes(vec![tree_doc()]);
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
        let drifts = recorded.drift(&|_| ReadOutcome::Absent, &|_| disk.clone());
        assert!(drifts.is_empty());
    }
}
