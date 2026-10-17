//! Document
//! Desired state payloads reaching disk.

pub mod payload;
pub mod rc;

use std::collections::BTreeMap;

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

#[cfg(any(unix, test))]
use crate::routes::Route;
use confit_driver::sha::Sha;
use confit_store::handles::ResourceHandle;

pub use payload::Payload;
pub use rc::{RcData, RcEntry, RcSection};

/// Default unix permission bits for files without explicit modes.
pub const DEFAULT_FILE_MODE: u32 = 0o644;

/// Structured data values keyed by field name.
type DataFieldMap = BTreeMap<String, serde_json::Value>;

/// Serialization format for structured documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Face)]
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
    #[cfg(unix)]
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
            #[cfg(unix)]
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

/// One tree member.
///
/// Bare handles take the default mode. Tables refer to
/// the handle with a required mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(untagged)]
#[face(table, untagged, rename = "TreeMember")]
pub enum TreeMember {
    /// Bare handle with default mode.
    Handle(Payload),
    /// Handle with explicit mode.
    Full {
        /// The file payload.
        #[face(proxy = ResourceHandle, decode_hook = decode_payload)]
        handle: Payload,
        /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
        #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode)]
        mode: u32,
    },
}

impl TreeMember {
    /// Return the member payload.
    pub fn blob(&self) -> &Payload {
        match self {
            Self::Handle(blob) | Self::Full { handle: blob, .. } => blob,
        }
    }

    /// Return the unix permission bits for the member file.
    pub fn mode(&self) -> u32 {
        match self {
            Self::Handle(_) => DEFAULT_FILE_MODE,
            Self::Full { mode, .. } => *mode,
        }
    }
}

/// One document payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(rename_all = "snake_case")]
#[face(table, untagged, rename = "Document")]
pub enum Data {
    /// Structured Text Document.
    Structured {
        /// The serialization format.
        format: StructuredFormat,
        /// Structured content for the file.
        #[face(proxy = DataFieldMap, decode_hook = decode_payload_map)]
        data: serde_json::Value,
    },
    /// Plain Text Document.
    Text {
        /// The exact file text.
        #[face(rename = "text")]
        content: String,
        /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
        #[serde(default = "default_file_mode")]
        #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode, default = default_file_mode)]
        mode: u32,
        /// Existence-only document flag.
        #[serde(default)]
        #[face(default)]
        unmanaged: bool,
    },
    /// Symbolic Link Document.
    #[cfg(unix)]
    Link {
        /// The link target route.
        target: Route,
    },
    /// Shell RC Document.
    Rc(RcData),
    /// Binary Document.
    Opaque {
        /// The opaque file handle.
        #[face(proxy = ResourceHandle, decode_hook = decode_payload)]
        handle: Payload,
        /// File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
        #[serde(default = "default_file_mode")]
        #[face(proxy = String, decode_hook = decode_mode, encode_hook = render_mode, default = default_file_mode)]
        mode: u32,
        /// Existence-only document flag.
        #[serde(default)]
        #[face(default)]
        unmanaged: bool,
    },
    /// File Tree Document.
    Tree {
        /// Tree members keyed by relative path.
        members: BTreeMap<String, TreeMember>,
    },
}

impl Data {
    /// Return the kind label for this payload.
    pub fn kind(&self) -> DocumentKind {
        match self {
            Self::Structured { .. } => DocumentKind::Structured,
            Self::Text { .. } => DocumentKind::Text,
            #[cfg(unix)]
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
            #[cfg(unix)]
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => None,
            #[cfg(not(unix))]
            Self::Structured { .. } | Self::Rc(_) | Self::Tree { .. } => None,
        }
    }

    /// Return the unmanaged flag for this payload.
    pub fn unmanaged(&self) -> bool {
        match self {
            Self::Text { unmanaged, .. } | Self::Opaque { unmanaged, .. } => *unmanaged,
            #[cfg(unix)]
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => false,
            #[cfg(not(unix))]
            Self::Structured { .. } | Self::Rc(_) | Self::Tree { .. } => false,
        }
    }

    /// Return the tree members for this payload.
    pub fn tree_members(&self) -> Option<&BTreeMap<String, TreeMember>> {
        match self {
            Self::Tree { members } => Some(members),
            _ => None,
        }
    }

    /// Collect every payload in document order.
    pub fn payloads(&self) -> Vec<&Payload> {
        match self {
            Self::Opaque { handle: blob, .. } => vec![blob],
            Self::Tree { members } => members.values().map(TreeMember::blob).collect(),
            #[cfg(unix)]
            Self::Structured { .. } | Self::Text { .. } | Self::Link { .. } | Self::Rc(_) => {
                Vec::new()
            }
            #[cfg(not(unix))]
            Self::Structured { .. } | Self::Text { .. } | Self::Rc(_) => Vec::new(),
        }
    }

    /// Collect every sealed content hash in document order.
    pub fn blob_refs(&self) -> Vec<&Sha> {
        self.payloads()
            .into_iter()
            .filter_map(Payload::resolved)
            .collect()
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
pub fn render_mode(_lua: &Lua, mode: &u32) -> mlua::Result<String> {
    Ok(format!("{:o}", mode))
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
    parse_mode_bits(&text).map_or_else(
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

/// Decode one handle value into its payload.
///
/// # Errors
///
/// - [`Mismatch`] for values outside resource handles.
fn decode_payload(lua: &Lua, value: &Value) -> std::result::Result<Payload, Mismatch> {
    decode_handle(lua, value, "handle").map(Payload::Handle)
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

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding::value::{Decode, Encode};
    use confit_store::Stores;

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
        let (_directory, lua, stores) = opened_lua();
        let handle = stores.resources().resource("note.txt").unwrap();
        let source = Data::Opaque {
            handle: Payload::Handle(handle),
            mode: 0o644,
            unmanaged: false,
        };
        let encoded = source.encode(&lua).unwrap();
        let Value::Table(table) = encoded else {
            panic!("opaque encodes a table");
        };
        let rendered: String = table.get("mode").unwrap();
        assert_eq!(rendered, "644");
        let _: mlua::AnyUserData = table.get("handle").unwrap();
    }

    #[cfg(unix)]
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
    fn mixed_markers_fail_showing_keys() {
        let lua = test_lua();
        #[cfg(unix)]
        {
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
        }
        #[cfg(not(unix))]
        {
            let text = body_error(&lua, r#"{ text = "hi", nope = 1 }"#);
            assert!(
                text.contains("Document_Text"),
                "the text shows the arm: {text}"
            );
        }
        let bare = body_error(&lua, r#"{ nope = 1 }"#);
        assert!(bare.contains("nope"), "the error shows the key: {bare}");
    }

    fn opened_lua() -> (tempfile::TempDir, Lua, Stores) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.txt"), b"hello").unwrap();
        std::fs::write(root.join("other.txt"), b"world").unwrap();
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

    fn handle_of(lua: &Lua, stores: &Stores, _directory: &tempfile::TempDir, name: &str) -> String {
        let global = format!("handle_{}", name.replace('.', "_"));
        let handle = match stores.resources().resource(name) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        };
        lua.globals().set(global.clone(), handle).unwrap();
        global
    }

    #[test]
    fn opaque_decodes_from_handles() {
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
                r#"{ members = { ["top.txt"] = "hi" } }"#.to_owned(),
                "TreeMember",
            ),
            (
                format!(r#"{{ members = {{ ["sub"] = {{ ["nested.txt"] = {note} }} }} }}"#),
                "TreeMember",
            ),
        ] {
            let text = body_error(&lua, &body);
            assert!(text.contains(want), "the text shows the fault: {text}");
        }
    }

    #[test]
    fn decode_bare_handle_with_default_mode() {
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let body = format!(r#"{{ members = {{ ["top.txt"] = {note} }} }}"#);
        let documents = documents_of(&lua, &body);
        match documents.1 {
            Data::Tree { members } => {
                assert_eq!(members.len(), 1, "bare body keeps one member");
                let member = match members.get("top.txt") {
                    Some(found) => found,
                    None => panic!("bare member keeps its key"),
                };
                assert!(
                    matches!(member, TreeMember::Handle(_)),
                    "bare handle decodes to Handle"
                );
                assert_eq!(member.mode(), 0o644, "bare handle takes the default mode");
            }
            _ => panic!("tree body decodes"),
        }
    }

    #[test]
    fn decode_full_table_with_explicit_mode() {
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let body =
            format!(r#"{{ members = {{ ["top.txt"] = {{ handle = {note}, mode = "755" }} }} }}"#);
        let documents = documents_of(&lua, &body);
        match documents.1 {
            Data::Tree { members } => {
                let member = match members.get("top.txt") {
                    Some(found) => found,
                    None => panic!("full member keeps its key"),
                };
                match member {
                    TreeMember::Full { mode, .. } => {
                        assert_eq!(*mode, 0o755, "table mode parses octal digits")
                    }
                    _ => panic!("table decodes to Full"),
                }
                assert_eq!(member.mode(), 0o755, "full member keeps its mode");
            }
            _ => panic!("tree body decodes"),
        }
    }

    #[test]
    fn decode_mixed_members_in_one_table() {
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let other = handle_of(&lua, &stores, &directory, "other.txt");
        let body = format!(
            r#"{{
            members = {{
                ["top.txt"] = {note},
                ["sub/nested.txt"] = {{ handle = {other}, mode = "755" }},
            }},
        }}"#
        );
        let documents = documents_of(&lua, &body);
        match documents.1 {
            Data::Tree { members } => {
                assert_eq!(members.len(), 2, "mixed body keeps both members");
                let bare = match members.get("top.txt") {
                    Some(found) => found,
                    None => panic!("bare member keeps its key"),
                };
                assert!(
                    matches!(bare, TreeMember::Handle(_)),
                    "bare entry decodes to Handle"
                );
                assert_eq!(bare.mode(), 0o644, "bare entry takes the default mode");
                let full = match members.get("sub/nested.txt") {
                    Some(found) => found,
                    None => panic!("full member keeps its key"),
                };
                assert!(
                    matches!(full, TreeMember::Full { .. }),
                    "table entry decodes to Full"
                );
                assert_eq!(full.mode(), 0o755, "table entry keeps its mode");
            }
            _ => panic!("tree body decodes"),
        }
    }

    #[test]
    fn reject_table_without_mode() {
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let text = body_error(
            &lua,
            &format!(r#"{{ members = {{ ["top.txt"] = {{ handle = {note} }} }} }}"#),
        );
        assert!(
            text.contains("TreeMember"),
            "the text shows the fault: {text}"
        );
    }

    #[test]
    fn round_trip_sealed_members_through_serde() {
        let full_blob = confit_driver::sha::Sha::hash(b"full bytes");
        let bare_blob = confit_driver::sha::Sha::hash(b"bare bytes");
        let bare = TreeMember::Handle(Payload::Resolved(bare_blob));
        let full = TreeMember::Full {
            handle: Payload::Resolved(full_blob),
            mode: 0o755,
        };
        let bare_text = match serde_json::to_string(&bare) {
            Ok(found) => found,
            Err(error) => panic!("sealed bare serializes: {error}"),
        };
        assert!(
            !bare_text.contains("mode"),
            "sealed bare carries zero mode keys: {bare_text}"
        );
        match serde_json::from_str::<TreeMember>(&bare_text) {
            Ok(TreeMember::Handle(_)) => {}
            Ok(_) => panic!("sealed bare decodes back to Handle"),
            Err(error) => panic!("sealed bare decodes: {error}"),
        }
        let full_text = match serde_json::to_string(&full) {
            Ok(found) => found,
            Err(error) => panic!("sealed full serializes: {error}"),
        };
        match serde_json::from_str::<TreeMember>(&full_text) {
            Ok(back) => assert_eq!(back, full, "sealed full survives serde"),
            Err(error) => panic!("sealed full decodes: {error}"),
        }
        let source = Data::Tree {
            members: BTreeMap::from([
                ("top.txt".to_owned(), bare),
                ("sub/nested.txt".to_owned(), full),
            ]),
        };
        let text = match serde_json::to_string(&source) {
            Ok(found) => found,
            Err(error) => panic!("sealed tree serializes: {error}"),
        };
        match serde_json::from_str::<Data>(&text) {
            Ok(back) => assert_eq!(back, source, "sealed tree survives serde"),
            Err(error) => panic!("sealed tree decodes: {error}"),
        }
    }

    fn no_blob_files(directory: &tempfile::TempDir) -> bool {
        let cache = directory.path().join("temp").join("blobs");
        !cache.exists()
            || confit_driver::fs::read_dir(&cache)
                .unwrap_or_default()
                .is_empty()
    }

    #[test]
    fn opaque_decodes_handles_without_writes() {
        let (directory, lua, stores) = opened_lua();
        let note = handle_of(&lua, &stores, &directory, "note.txt");
        let documents = documents_of(
            &lua,
            &format!(r#"{{ handle = {note}, mode = "644", unmanaged = true }}"#),
        );
        match documents.1 {
            Data::Opaque {
                handle: Payload::Handle(_),
                ..
            } => {}
            _ => panic!("opaque body decodes to a handle"),
        }
        assert!(no_blob_files(&directory), "decode writes zero blob files");
    }

    #[test]
    fn tree_decodes_handles_without_writes() {
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
                assert!(
                    members
                        .values()
                        .all(|member| matches!(member.blob(), Payload::Handle(_))),
                    "every member holds a handle"
                );
            }
            _ => panic!("tree body decodes to handles"),
        }
        assert!(no_blob_files(&directory), "decode writes zero blob files");
    }

    #[test]
    fn ref_tables_fail_where_handles_belong() {
        let (_directory, lua, _stores) = opened_lua();
        let text = body_error(&lua, r#"{ handle = { sha256 = "aa" }, mode = "644" }"#);
        assert!(
            text.contains("a resource handle"),
            "the text shows the want: {text}"
        );
        let members = body_error(&lua, r#"{ members = { ["top.txt"] = { sha256 = "aa" } } }"#);
        assert!(
            members.contains("TreeMember"),
            "the text shows the want: {members}"
        );
    }

    #[test]
    fn sealed_documents_refuse_lua_encode() {
        let lua = test_lua();
        let digest = confit_driver::sha::Sha::hash(b"confit sealed fixture");
        let source = Data::Opaque {
            handle: Payload::Resolved(digest.clone()),
            mode: 0o644,
            unmanaged: false,
        };
        match source.encode(&lua) {
            Ok(_) => panic!("sealed documents encode"),
            Err(error) => assert!(
                error.to_string().contains("resolved"),
                "the error shows the arm: {error}"
            ),
        }
    }

    #[test]
    fn unsealed_documents_refuse_serialize() {
        let (_directory, _lua, stores) = opened_lua();
        let handle = match stores.resources().resource("note.txt") {
            Ok(found) => found,
            Err(error) => panic!("project opens: {error}"),
        };
        let unsealed = Data::Opaque {
            handle: Payload::Handle(handle),
            mode: 0o644,
            unmanaged: false,
        };
        match serde_json::to_string(&unsealed) {
            Ok(_) => panic!("unsealed documents serialize"),
            Err(error) => assert!(
                error.to_string().contains("unsealed"),
                "the error shows the state: {error}"
            ),
        }
        let digest = confit_driver::sha::Sha::hash(b"confit sealed fixture");
        let sealed = Data::Opaque {
            handle: Payload::Resolved(digest.clone()),
            mode: 0o644,
            unmanaged: false,
        };
        let text = serde_json::to_string(&sealed).unwrap();
        assert!(
            text.contains(&digest.hex()),
            "sealed json carries the hex: {text}"
        );
    }
}
