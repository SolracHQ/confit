//! Document
//! Desired state payloads reaching disk.

pub mod rc;

use std::collections::BTreeMap;

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use mlua::{Lua, Table, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::routes::Route;
use confit_store::Stores;
use confit_store::blob::BlobSource;
use confit_store::handles::{BlobRef, ResourceHandle};

pub use rc::{RcData, RcEntry, RcSection};

/// Default unix permission bits for files without explicit modes.
pub const DEFAULT_FILE_MODE: u32 = 0o644;

/// Member handles keyed by relative path.
type MemberHandleMap = BTreeMap<String, ResourceHandle>;

/// Structured data values keyed by field name.
type DataFieldMap = BTreeMap<String, serde_json::Value>;

/// Serialization format for structured documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Face, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[face(table, string)]
pub enum StructuredFormat {
    /// JSON format.
    Json,
    /// TOML format.
    Toml,
    /// YAML format.
    Yaml,
}

impl StructuredFormat {
    /// Return the lowercase format name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    /// Parse a format name in any letter case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

impl std::fmt::Display for StructuredFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Document materialization kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Face)]
#[serde(rename_all = "lowercase")]
#[face(table, string)]
pub enum DocumentKind {
    /// Structured data file with a serialization format.
    Structured,
    /// Plain text file from declaration content.
    Text,
    /// Symlink placement from declaration target.
    Link,
    /// Per-shell rc data object.
    Rc,
    /// Raw binary file from declaration bytes.
    Opaque,
    /// Managed file set from one archive under one folder.
    Tree,
}

impl DocumentKind {
    /// Return the lowercase kind name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::Text => "text",
            Self::Link => "link",
            Self::Rc => "rc",
            Self::Opaque => "opaque",
            Self::Tree => "tree",
        }
    }
}

impl std::fmt::Display for DocumentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// One persisted tree member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(deny_unknown_fields)]
#[face(table)]
pub struct TreeMember {
    /// The content-addressed member bytes identity.
    #[face(proxy = ResourceHandle, convert = decode_opaque_blob, encode = render_blob)]
    pub blob: BlobRef,
    /// Unix permission bits for the member file.
    pub mode: u32,
}

/// Persisted document payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(rename_all = "snake_case")]
#[face(table, untagged, rename = "Document")]
pub enum Data {
    /// Structured Text Document.
    Structured {
        /// The serialization format.
        format: StructuredFormat,
        /// Structured content for the file.
        #[face(proxy = DataFieldMap, convert = decode_payload_map)]
        data: serde_json::Value,
    },
    /// Plain Text Document.
    Text {
        /// The exact file text.
        #[face(rename = "text")]
        content: String,
        /// Unix permission bits.
        #[serde(default = "default_file_mode")]
        #[face(proxy = String, convert = decode_mode, encode = render_mode, default = default_file_mode)]
        mode: u32,
        /// Existence-only document flag.
        #[serde(default)]
        #[face(default)]
        unmanaged: bool,
    },
    /// Symbolic Link Document.
    Link {
        /// The link target route.
        target: Route,
    },
    /// Shell RC Document.
    Rc(RcData),
    /// Binary Document.
    Opaque {
        /// The resource handle holding the file bytes.
        #[face(
            rename = "handle",
            proxy = ResourceHandle,
            convert = decode_opaque_blob,
            encode = render_blob
        )]
        blob: BlobRef,
        /// Raw byte count of the file content.
        #[face(convert = decode_opaque_size)]
        size: u64,
        /// Unix permission bits.
        #[serde(default = "default_file_mode")]
        #[face(proxy = String, convert = decode_mode, encode = render_mode, default = default_file_mode)]
        mode: u32,
        /// Existence-only document flag.
        #[serde(default)]
        #[face(default)]
        unmanaged: bool,
    },
    /// File Tree Document.
    Tree {
        /// Tree members keyed by relative path.
        #[face(
            proxy = MemberHandleMap,
            convert = decode_tree_members
        )]
        members: BTreeMap<String, TreeMember>,
    },
}

impl Data {
    /// Return the kind label for this payload.
    pub fn kind(&self) -> DocumentKind {
        match self {
            Self::Structured { .. } => DocumentKind::Structured,
            Self::Text { .. } => DocumentKind::Text,
            Self::Link { .. } => DocumentKind::Link,
            Self::Rc(_) => DocumentKind::Rc,
            Self::Opaque { .. } => DocumentKind::Opaque,
            Self::Tree { .. } => DocumentKind::Tree,
        }
    }

    /// Return the unix permission bits for this payload.
    pub fn mode(&self) -> Option<u32> {
        match self {
            Self::Text { mode, .. } | Self::Opaque { mode, .. } => Some(*mode),
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => None,
        }
    }

    /// Return the unmanaged flag for this payload.
    pub fn unmanaged(&self) -> bool {
        match self {
            Self::Text { unmanaged, .. } | Self::Opaque { unmanaged, .. } => *unmanaged,
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => false,
        }
    }

    /// Return the tree members for this payload.
    pub fn tree_members(&self) -> Option<&BTreeMap<String, TreeMember>> {
        match self {
            Self::Tree { members } => Some(members),
            _ => None,
        }
    }

    /// Collect every blob ref in document order.
    pub fn blob_refs(&self) -> Vec<&BlobRef> {
        match self {
            Self::Opaque { blob, .. } => vec![blob],
            Self::Tree { members } => members.values().map(|member| &member.blob).collect(),
            Self::Structured { .. } | Self::Text { .. } | Self::Link { .. } | Self::Rc(_) => {
                Vec::new()
            }
        }
    }
}

/// Count changed members between two tree manifests.
pub fn tree_changed(
    old: &BTreeMap<String, TreeMember>,
    new: &BTreeMap<String, TreeMember>,
) -> usize {
    let mut changed = 0;
    for (relative, member) in new {
        match old.get(relative) {
            Some(previous) if previous == member => {
                continue;
            }
            _ => changed += 1,
        }
    }
    for relative in old.keys() {
        if !new.contains_key(relative) {
            changed += 1;
        }
    }
    changed
}

/// Render mode bits as octal digits.
pub fn render_mode(mode: u32) -> String {
    format!("{mode:o}")
}

/// Render one blob ref as its content hash.
fn render_blob(blob: BlobRef) -> String {
    blob.sha256.hex()
}

/// Return the default unix permission bits for decode.
fn default_file_mode() -> u32 {
    DEFAULT_FILE_MODE
}

/// Decode mode text into permission bits.
///
/// # Errors
///
/// - [`Mismatch`] for values outside octal or symbolic text.
fn decode_mode(lua: &Lua, value: &Value) -> std::result::Result<u32, Mismatch> {
    let text = String::decode(value, lua).map_err(|error| Mismatch {
        path: "mode".to_owned(),
        want: "expected a string".to_owned(),
        found: error.found,
    })?;
    parse_mode_bits(text.as_str()).map_or_else(
        || {
            Err(Mismatch {
                path: "mode".to_owned(),
                want: "octal like '755' or symbolic like 'rwxr-xr-x'".to_owned(),
                found: ["'", text.as_str(), "'"].concat(),
            })
        },
        Ok,
    )
}

/// Parse octal or symbolic permission text.
fn parse_mode_bits(text: &str) -> Option<u32> {
    if text.starts_with('d') {
        return None;
    }
    match text.len() {
        9 => {
            let mut bits: u32 = 0;
            for (index, byte) in text.as_bytes().iter().enumerate() {
                let bit: u32 = match (index % 3, byte) {
                    (0, b'r') => 4,
                    (1, b'w') => 2,
                    (2, b'x') => 1,
                    (_, b'-') => 0,
                    _ => return None,
                };
                bits |= bit << ((2 - index / 3) * 3);
            }
            Some(bits)
        }
        3 | 4 => {
            let body = match text.len() {
                3 => text,
                _ => text.strip_prefix('0')?,
            };
            if text.len() == 4 && !text.starts_with('0') {
                return None;
            }
            if !body.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
                return None;
            }
            u32::from_str_radix(body, 8).ok()
        }
        _ => None,
    }
}

/// Decode one structured payload into its data map.
///
/// # Errors
///
/// - [`Mismatch`] for values outside data maps.
fn decode_payload_map(
    lua: &Lua,
    value: &Value,
) -> std::result::Result<serde_json::Value, Mismatch> {
    let data = serde_json::Value::decode(value, lua).map_err(|error| Mismatch {
        path: "Document.Structured.data".to_owned(),
        want: "a data map".to_owned(),
        found: error.found,
    })?;
    if data.is_object() {
        Ok(data)
    } else {
        Err(Mismatch {
            path: "Document.Structured.data".to_owned(),
            want: "a data map".to_owned(),
            found: data.to_string(),
        })
    }
}

/// Register one handle value into its blob ref.
///
/// # Errors
///
/// - [`Mismatch`] for values outside resource handles and pool failures.
fn decode_opaque_blob(lua: &Lua, value: &Value) -> std::result::Result<BlobRef, Mismatch> {
    let handle = decode_handle(lua, value, "handle")?;
    let stores = stores_of(lua)?;
    stores
        .blobs()
        .put(BlobSource::Handle(&handle))
        .map(|opened| opened.to_ref())
        .map_err(|error| Mismatch {
            path: "handle".to_owned(),
            want: "a decoded body".to_owned(),
            found: error.to_string(),
        })
}

/// Read one handle value into its raw byte count.
///
/// # Errors
///
/// - [`Mismatch`] for values outside resource handles and pool failures.
fn decode_opaque_size(lua: &Lua, table: &Table) -> std::result::Result<u64, Mismatch> {
    let value = confit_binding::value::read_field(table, "Document_Opaque", "handle")?;
    let handle = decode_handle(lua, &value, "handle")?;
    let stores = stores_of(lua)?;
    let opened = stores.resources().open(&handle).map_err(|error| Mismatch {
        path: "handle".to_owned(),
        want: "a decoded body".to_owned(),
        found: error.to_string(),
    })?;
    opened.len().map_err(|error| Mismatch {
        path: "handle".to_owned(),
        want: "a decoded body".to_owned(),
        found: error.to_string(),
    })
}

/// Decode one members table into member entries with pool registration.
///
/// # Errors
///
/// - [`Mismatch`] for misshaped tables, handles, paths and pool failures.
fn decode_tree_members(
    lua: &Lua,
    value: &Value,
) -> std::result::Result<BTreeMap<String, TreeMember>, Mismatch> {
    let source = confit_binding::value::table_value(value, "members")?;
    let mut pairs: Vec<(String, Value)> = Vec::new();
    for pair in source.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| Mismatch {
            path: "members".to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::String(text) => pairs.push((text.to_string_lossy(), item)),
            _ => {
                return Err(Mismatch {
                    path: "members".to_owned(),
                    want: "a string key".to_owned(),
                    found: key.type_name().to_owned(),
                });
            }
        }
    }
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let stores = stores_of(lua)?;
    let mut members = BTreeMap::new();
    for (relative, item) in &pairs {
        check_member_key(relative)?;
        let handle = decode_handle(lua, item, &["members.", relative.as_str()].concat())?;
        let opened = stores
            .blobs()
            .put(BlobSource::Handle(&handle))
            .map_err(|error| Mismatch {
                path: ["members.", relative.as_str()].concat(),
                want: "a decoded body".to_owned(),
                found: error.to_string(),
            })?;
        let mode = stores.resources().mode(&handle).map_err(|error| Mismatch {
            path: ["members.", relative.as_str()].concat(),
            want: "a decoded body".to_owned(),
            found: error.to_string(),
        })?;
        members.insert(
            relative.clone(),
            TreeMember {
                blob: opened.to_ref(),
                mode,
            },
        );
    }
    Ok(members)
}

/// Decode one handle value into its resource handle.
///
/// # Errors
///
/// - [`Mismatch`] for values outside resource handles.
fn decode_handle(
    lua: &Lua,
    value: &Value,
    path: &str,
) -> std::result::Result<ResourceHandle, Mismatch> {
    ResourceHandle::decode(value, lua).map_err(|error| Mismatch {
        path: path.to_owned(),
        want: "a resource handle".to_owned(),
        found: error.found,
    })
}

/// Reject tree keys outside relative member paths.
///
/// # Errors
///
/// - [`Mismatch`] for empty, absolute, and dot-dot keys.
fn check_member_key(relative: &str) -> std::result::Result<(), Mismatch> {
    if relative.is_empty() || relative.starts_with('/') {
        return Err(Mismatch {
            path: "tree files key".to_owned(),
            want: "a relative member path".to_owned(),
            found: ["'", relative, "'"].concat(),
        });
    }
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(Mismatch {
                path: "tree files key".to_owned(),
                want: "a relative member path".to_owned(),
                found: ["'", relative, "'"].concat(),
            });
        }
    }
    Ok(())
}

/// Read the shared stores from one Lua instance.
///
/// # Errors
///
/// - [`Mismatch`] for an uninstalled api.
fn stores_of(lua: &Lua) -> std::result::Result<Stores, Mismatch> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.clone()),
        None => Err(Mismatch {
            path: "Document".to_owned(),
            want: "a decoded body".to_owned(),
            found: "confit api is not installed".to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding::value::{Decode, Encode};

    use crate::routes::RouteBase;

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn body_of(lua: &Lua, body: &str) -> Value {
        let source = format!("return {body}");
        lua.load(&source).call(()).unwrap()
    }

    fn documents_of(lua: &Lua, body: &str) -> (Route, Data) {
        let route = Route::new(RouteBase::Home, "subject").unwrap();
        let value = body_of(lua, body);
        match Data::decode(&value, lua) {
            Ok(decoded) => (route, decoded),
            Err(error) => panic!("body decodes: {error}"),
        }
    }

    fn body_error(lua: &Lua, body: &str) -> String {
        let value = body_of(lua, body);
        match Data::decode(&value, lua) {
            Ok(_) => panic!("foreign body passes: {body}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn text_decodes_mode_with_unmanaged() {
        let lua = test_lua();
        let documents = documents_of(&lua, r#"{ text = "hi", mode = "644", unmanaged = true }"#);
        match documents.1 {
            Data::Text {
                content,
                mode,
                unmanaged,
            } => {
                assert_eq!(content, "hi");
                assert_eq!(mode, 0o644);
                assert!(unmanaged);
            }
            _ => panic!("text body decodes"),
        }
        let symbolic = documents_of(&lua, r#"{ text = "hi", mode = "rwxr-xr-x" }"#);
        match symbolic.1 {
            Data::Text {
                mode, unmanaged, ..
            } => {
                assert_eq!(mode, 0o755);
                assert!(!unmanaged);
            }
            _ => panic!("symbolic mode decodes"),
        }
        let refused = body_error(&lua, r#"{ text = "hi", mode = 644 }"#);
        assert!(
            refused.contains("expected a string"),
            "the error shows the fault: {refused}"
        );
        for (body, want) in [
            (r#"{ text = 1 }"#, "a string"),
            (r#"{ text = "hi", mode = "zzz" }"#, "octal like '755'"),
            (r#"{ text = "hi", unmanaged = 1 }"#, "a boolean"),
            (r#"{ text = "hi", bogus = 1 }"#, "bogus"),
        ] {
            let text = body_error(&lua, body);
            assert!(text.contains(want), "the text shows the fault: {text}");
        }
    }

    #[test]
    fn structured_decodes_format_with_data() {
        let lua = test_lua();
        let documents = documents_of(&lua, r#"{ format = "json", data = { alpha = "one" } }"#);
        match documents.1 {
            Data::Structured { format, data } => {
                assert_eq!(format, StructuredFormat::Json);
                assert_eq!(
                    data.get("alpha").and_then(|item| item.as_str()),
                    Some("one")
                );
            }
            _ => panic!("structured body decodes"),
        }
        let text = body_error(&lua, r#"{ format = "json" }"#);
        assert!(text.contains("data"), "the text shows the field: {text}");
    }

    #[test]
    fn structured_live_views_keep_payload_maps_through_encode() {
        let lua = test_lua();
        let source = Data::Structured {
            format: StructuredFormat::Json,
            data: serde_json::json!({"tools": {"bat": "latest"}, "count": 2}),
        };
        let encoded = source.encode(&lua).unwrap();
        let back = Data::decode(&encoded, &lua).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn text_mode_round_trips_through_strings() {
        let lua = test_lua();
        for (mode, text) in [(0o644, "644"), (0o755, "755")] {
            let source = Data::Text {
                content: "hi".to_owned(),
                mode,
                unmanaged: true,
            };
            let encoded = source.encode(&lua).unwrap();
            let Value::Table(table) = encoded else {
                panic!("text encodes a table");
            };
            let rendered: String = table.get("mode").unwrap();
            assert_eq!(rendered, text);
            let back = Data::decode(&Value::Table(table), &lua).unwrap();
            assert_eq!(back, source);
        }
    }

    #[test]
    fn opaque_mode_encodes_strings() {
        let lua = test_lua();
        let digest = confit_driver::sha::Sha::hash(b"confit opaque fixture");
        let source = Data::Opaque {
            blob: BlobRef::new(digest.clone(), digest),
            size: 5,
            mode: 0o644,
            unmanaged: false,
        };
        let encoded = source.encode(&lua).unwrap();
        let Value::Table(table) = encoded else {
            panic!("opaque encodes a table");
        };
        let rendered: String = table.get("mode").unwrap();
        assert_eq!(rendered, "644");
    }

    #[test]
    fn link_decodes_target_route() {
        let lua = test_lua();
        let documents = documents_of(
            &lua,
            r#"{ target = { base = "home", relative = "linked" } }"#,
        );
        assert_eq!(documents.0.display(), "home('subject')");
        match documents.1 {
            Data::Link { target } => assert_eq!(target.display(), "home('linked')"),
            _ => panic!("link body decodes"),
        }
        let text = body_error(&lua, r#"{ target = "linked" }"#);
        assert!(text.contains("a table"), "the text shows the want: {text}");
    }

    #[test]
    fn mixed_markers_fail_naming_keys() {
        let lua = test_lua();
        let text = body_error(
            &lua,
            r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
        );
        assert!(
            text.contains("Document_Text"),
            "the text shows the arm: {text}"
        );
        assert!(
            text.contains("Document_Link"),
            "the text shows the arm: {text}"
        );
        let bare = body_error(&lua, r#"{ nope = 1 }"#);
        assert!(bare.contains("nope"), "the error shows the key: {bare}");
    }

    fn opened_lua() -> (tempfile::TempDir, Lua, Stores) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        confit_driver::fs::create_dir_all(&root).unwrap();
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        confit_driver::fs::write(&root.join("other.txt"), b"world").unwrap();
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(
            confit_store::StoreRoots {
                config_base: directory.path().join("config"),
                temp_base: directory.path().join("temp"),
                project_root: directory.path().join("project"),
            },
            sender,
        );
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        (directory, lua, stores)
    }

    fn handle_of(lua: &Lua, stores: &Stores, directory: &tempfile::TempDir, name: &str) -> String {
        let global = format!("handle_{}", name.replace('.', "_"));
        let path = directory.path().join("project").join(name);
        let handle = match stores.resources().resource(&path) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        };
        lua.globals().set(global.clone(), handle).unwrap();
        global
    }

    #[test]
    fn opaque_decodes_from_handles() {
        let _guard = confit_driver::fs::TestGuard::install();
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let documents = documents_of(
            &lua,
            &format!(r#"{{ handle = {note}, mode = "644", unmanaged = true }}"#),
        );
        match documents.1 {
            Data::Opaque {
                mode, unmanaged, ..
            } => {
                assert_eq!(mode, 0o644);
                assert!(unmanaged, "unmanaged flag carries");
            }
            _ => panic!("opaque body decodes"),
        }
        let refused = body_error(&lua, &format!(r#"{{ handle = {note}, mode = 644 }}"#));
        assert!(
            refused.contains("expected a string"),
            "the error shows the fault: {refused}"
        );
        for (body, want) in [
            (
                format!(r#"{{ handle = {note}, mode = "zzz" }}"#),
                "octal like '755'".to_owned(),
            ),
            (
                r#"{ handle = "note.txt" }"#.to_owned(),
                "a resource handle".to_owned(),
            ),
        ] {
            let text = body_error(&lua, &body);
            assert!(
                text.contains(want.as_str()),
                "the text shows the fault: {text}"
            );
        }
    }

    #[test]
    fn tree_decodes_flat_paths_into_members() {
        let _guard = confit_driver::fs::TestGuard::install();
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let other = handle_of(&lua, &stores, &directory, "other.txt");
        let body = format!(
            r#"{{
            members = {{
                ["top.txt"] = {note},
                ["sub/nested.txt"] = {other},
            }},
        }}"#
        );
        let documents = documents_of(&lua, &body);
        match documents.1 {
            Data::Tree { members } => {
                assert_eq!(members.len(), 2);
                let relatives: Vec<&str> = members.keys().map(String::as_str).collect();
                assert_eq!(relatives, vec!["sub/nested.txt", "top.txt"]);
            }
            _ => panic!("tree body decodes"),
        }
        let bare = documents_of(&lua, r#"{ members = {} }"#);
        match bare.1 {
            Data::Tree { members } => assert!(members.is_empty(), "empty builds bare"),
            _ => panic!("bare folder decodes"),
        }
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        for (body, want) in [
            (
                format!(r#"{{ members = {{ [""] = {note} }} }}"#),
                "a relative member path",
            ),
            (
                format!(r#"{{ members = {{ ["/top.txt"] = {note} }} }}"#),
                "a relative member path",
            ),
            (
                format!(r#"{{ members = {{ ["sub//nested.txt"] = {note} }} }}"#),
                "a relative member path",
            ),
            (
                format!(r#"{{ members = {{ ["sub/../nested.txt"] = {note} }} }}"#),
                "a relative member path",
            ),
            (
                r#"{ members = { ["top.txt"] = "hi" } }"#.to_owned(),
                "a resource handle",
            ),
            (
                format!(r#"{{ members = {{ ["sub"] = {{ ["nested.txt"] = {note} }} }} }}"#),
                "a resource handle",
            ),
        ] {
            let text = body_error(&lua, &body);
            assert!(text.contains(want), "the text shows the fault: {text}");
        }
    }
}
