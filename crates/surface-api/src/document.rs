//! Documents
//! Body shapes with marker dispatch into owned values.

use mlua::{FromLua, Lua, LuaSerdeExt, Table, Value};
use mlua_extras::typed::{Type, Typed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use std::collections::HashMap;

use confit_model::document::{RcEntry, StructuredFormat};
use confit_model::routes::Route;
use confit_surface_face::mismatch::received;
use confit_surface_face::proxy::{Convert, ProxyType};
use confit_surface_face::translate::json_value;

use crate::error::ApiError;
use crate::handles::Handle;

/// One serializable value for null with scalars and tables.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct SerializableValue(pub serde_json::Value);

impl Typed for SerializableValue {
    fn ty() -> Type {
        json_value()
    }
}

impl FromLua for SerializableValue {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

/// Text body for plain text documents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextDocument {
    /// The exact file text.
    pub text: String,
    /// Unix permission bits with the umask default.
    #[serde(default)]
    pub mode: Mode,
    /// The presence-only satisfaction flag.
    pub unmanaged: Option<bool>,
}

confit_surface_face::typed!(TextDocument);

impl FromLua for TextDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        decode_shape(value)
    }
}

/// Structured body for rendered data documents.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredDocument {
    /// The serialization format name.
    pub format: StructuredFormat,
    /// The payload for rendering.
    pub data: BTreeMap<String, SerializableValue>,
}

confit_surface_face::typed!(StructuredDocument);

impl FromLua for StructuredDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        decode_shape(value)
    }
}

/// Link body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkDocument {
    /// The link target route.
    pub target: Route,
}

confit_surface_face::typed!(LinkDocument);

impl FromLua for LinkDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        decode_shape(value)
    }
}

/// Rc body.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RcDocument {
    /// Entries rendering before the guard.
    #[serde(default)]
    pub profile: Vec<RcEntry>,
    /// Entries rendering after the guard.
    #[serde(default)]
    pub config: Vec<RcEntry>,
    /// Entries rendering last.
    #[serde(default, rename = "final")]
    #[schemars(rename = "final")]
    pub final_entries: Vec<RcEntry>,
}

confit_surface_face::typed!(RcDocument);

/// Opaque body.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub struct OpaqueDocument {
    /// The opened input handle.
    pub handle: Handle,
    /// Unix permission bits with the umask default.
    pub mode: Mode,
    /// The presence-only satisfaction flag.
    pub unmanaged: bool,
}

confit_surface_face::typed!(OpaqueDocument);

impl FromLua for OpaqueDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let table = body_table(&value)?;
        check_keys(&table, &["handle", "mode", "unmanaged"])?;
        let item: Value = table.get("handle")?;
        let Some(data) = item.as_userdata() else {
            return Err(
                ApiError::mismatch("opaque handle", "a resource handle", received(&item)).into(),
            );
        };
        let handle = match data.borrow::<Handle>() {
            Ok(handle) => handle.clone(),
            Err(_) => {
                return Err(
                    ApiError::mismatch("opaque handle", "a resource handle", "userdata").into(),
                );
            }
        };
        let mode: Mode = decode_named_shape(table.get("mode")?, "mode")?;
        let unmanaged = read_unmanaged_field(&table)?.unwrap_or(false);
        Ok(Self {
            handle,
            mode,
            unmanaged,
        })
    }
}

/// One tree slot.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum TreeSlot {
    /// One file under its opened input handle.
    Handle(Handle),
    /// One subfolder.
    Tree(Tree),
}

confit_surface_face::typed!(TreeSlot);

/// One tree.
#[derive(Debug, Clone, PartialEq, Default, JsonSchema)]
pub struct Tree(pub HashMap<String, TreeSlot>);

confit_surface_face::typed!(Tree);

/// Tree body with a bare folder for an empty map.
#[derive(Debug, Clone, PartialEq, Default, JsonSchema)]
pub struct TreeDocument {
    /// The tree members for the build.
    pub members: Tree,
}

confit_surface_face::typed!(TreeDocument);

impl FromLua for TreeDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let table = body_table(&value)?;
        check_keys(&table, &["members"])?;
        let files: Value = table.get("members")?;
        let Some(files) = files.as_table() else {
            return Err(
                ApiError::mismatch("tree members", "a files table", received(&files)).into(),
            );
        };
        Ok(Self {
            members: Tree(decode_members(files)?),
        })
    }
}

/// Decode one members table into slots by single name.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for foreign keys, handles and folders.
fn decode_members(files: &Table) -> mlua::Result<HashMap<String, TreeSlot>> {
    let mut members = HashMap::new();
    for pair in files.pairs::<Value, Value>() {
        let (key, found) = pair?;
        let Value::String(text) = key else {
            return Err(
                ApiError::mismatch("tree files key", "a single file name", received(&key)).into(),
            );
        };
        let name = text.to_string_lossy();
        check_single_name(&name)?;
        if let Some(data) = found.as_userdata() {
            match data.borrow::<Handle>() {
                Ok(handle) => {
                    members.insert(name, TreeSlot::Handle(handle.clone()));
                    continue;
                }
                Err(_) => {
                    return Err(ApiError::mismatch(
                        "tree files entry",
                        "a resource handle or a folder table",
                        "userdata",
                    )
                    .into());
                }
            }
        }
        if let Some(nested) = found.as_table() {
            members.insert(name, TreeSlot::Tree(Tree(decode_members(nested)?)));
            continue;
        }
        return Err(ApiError::mismatch(
            "tree files entry",
            "a resource handle or a folder table",
            received(&found),
        )
        .into());
    }
    Ok(members)
}

/// Reject tree keys outside single names.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for empty and compound names.
fn check_single_name(name: &str) -> mlua::Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(ApiError::mismatch(
            "tree files key",
            "a single file name",
            ["'", name, "'"].concat(),
        )
        .into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err(ApiError::mismatch(
            "tree files key",
            "a single file name",
            ["'", name, "'"].concat(),
        )
        .into());
    }
    Ok(())
}

// The union face sits beside its manual FromLua twin.
confit_surface_face::union_face! {
    /// One document body in any shape.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Document {
        /// Plain text payload.
        Text(TextDocument),
        /// Rendered data payload.
        Structured(StructuredDocument),
        /// Symlink payload.
        Link(LinkDocument),
        /// Per-shell rc payload.
        Rc(RcDocument),
        /// Raw binary payload.
        Opaque(OpaqueDocument),
        /// Managed file set payload.
        Tree(TreeDocument),
    }
}

impl FromLua for Document {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = match &value {
            Value::Table(found) => found.clone(),
            _ => {
                return Err(ApiError::mismatch("body", "a body table", received(&value)).into());
            }
        };
        let keys = body_keys(&table)?;
        let families = family_count(&keys);
        if families != 1 {
            return Err(ApiError::mismatch(
                "body",
                "one of 'text', 'format' with 'data', 'target', 'handle', 'profile' with 'config' with 'final' or 'members'",
                keys.join(", "),
            )
            .into());
        }
        if keys.contains(&"text".to_owned()) {
            return Ok(Self::Text(TextDocument::from_lua(value, lua)?));
        }
        if keys.contains(&"format".to_owned()) || keys.contains(&"data".to_owned()) {
            return Ok(Self::Structured(StructuredDocument::from_lua(value, lua)?));
        }
        if keys.contains(&"target".to_owned()) {
            return Ok(Self::Link(LinkDocument::from_lua(value, lua)?));
        }
        if keys.contains(&"handle".to_owned()) {
            return Ok(Self::Opaque(OpaqueDocument::from_lua(value, lua)?));
        }
        if keys.contains(&"members".to_owned()) {
            return Ok(Self::Tree(TreeDocument::from_lua(value, lua)?));
        }
        Ok(Self::Rc(RcDocument::from_lua(value, lua)?))
    }
}

impl Document {
    /// Return the lowercase kind name for this body.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Structured(_) => "structured",
            Self::Link(_) => "link",
            Self::Rc(_) => "rc",
            Self::Opaque(_) => "opaque",
            Self::Tree(_) => "tree",
        }
    }
}

/// Collect the sorted string keys of one body table.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string keys.
fn body_keys(table: &Table) -> mlua::Result<Vec<String>> {
    let mut keys = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) => keys.push(text.to_string_lossy()),
            _ => {
                return Err(ApiError::mismatch("body key", "a string key", received(&key)).into());
            }
        }
    }
    keys.sort();
    Ok(keys)
}

/// Count marker families present in sorted keys.
fn family_count(keys: &[String]) -> usize {
    let mut count = 0;
    if keys.contains(&"text".to_owned()) {
        count += 1;
    }
    if keys.contains(&"format".to_owned()) || keys.contains(&"data".to_owned()) {
        count += 1;
    }
    if keys.contains(&"target".to_owned()) {
        count += 1;
    }
    if keys.contains(&"handle".to_owned()) {
        count += 1;
    }
    if keys.contains(&"profile".to_owned())
        || keys.contains(&"config".to_owned())
        || keys.contains(&"final".to_owned())
    {
        count += 1;
    }
    if keys.contains(&"members".to_owned()) {
        count += 1;
    }
    count
}

/// Decode one body table from its value.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-table bodies.
fn body_table(value: &Value) -> mlua::Result<Table> {
    match value {
        Value::Table(found) => Ok(found.clone()),
        _ => Err(ApiError::mismatch("body", "a body table", received(value)).into()),
    }
}

/// Reject unknown keys with the key.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for unknown keys.
fn check_keys(table: &Table, known: &[&str]) -> mlua::Result<()> {
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) => {
                let name = text.to_string_lossy();
                if !known.contains(&name.as_str()) {
                    return Err(ApiError::mismatch(
                        "body key",
                        "a known body key",
                        ["'", name.as_str(), "'"].concat(),
                    )
                    .into());
                }
            }
            _ => {
                return Err(ApiError::mismatch("body key", "a string key", received(&key)).into());
            }
        }
    }
    Ok(())
}

/// Decode one pure-data shape with its failure path.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped shapes.
pub(crate) fn decode_shape<Shape>(value: Value) -> mlua::Result<Shape>
where
    Shape: for<'de> Deserialize<'de>,
{
    serde_path_to_error::deserialize(mlua::serde::Deserializer::new(value)).map_err(|error| {
        let detail = error.inner().to_string();
        let field = error.path().to_string();
        ApiError::mismatch(
            if field.is_empty() {
                "body".to_owned()
            } else {
                field
            },
            detail.clone(),
            detail,
        )
        .into()
    })
}

/// Decode one pure-data shape under its prefix with its failure path.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped shapes.
pub(crate) fn decode_named_shape<Shape>(value: Value, prefix: &str) -> mlua::Result<Shape>
where
    Shape: for<'de> Deserialize<'de>,
{
    serde_path_to_error::deserialize(mlua::serde::Deserializer::new(value)).map_err(|error| {
        let detail = error.inner().to_string();
        let field = named_field(prefix, &error.path().to_string());
        ApiError::mismatch(field, detail.clone(), detail).into()
    })
}

/// Return one prefixed field for its serde path.
fn named_field(prefix: &str, path: &str) -> String {
    let trimmed = path.strip_prefix('.').unwrap_or(path);
    if trimmed.is_empty() {
        prefix.to_owned()
    } else if trimmed.starts_with('[') {
        [prefix, trimmed].concat()
    } else {
        [prefix, " ", trimmed].concat()
    }
}

/// One mode string for bit conversion.
pub struct ParseMode;

impl Convert<Option<String>, Option<u32>> for ParseMode {
    type Error = ApiError;

    fn convert(found: Option<String>) -> Result<Option<u32>, ApiError> {
        match found {
            None => Ok(None),
            Some(raw) => match parse_mode_bits(&raw) {
                Some(bits) => Ok(Some(bits)),
                None => Err(ApiError::mismatch(
                    "mode",
                    "octal like '755' or symbolic like 'rwxr-xr-x'",
                    ["'", raw.as_str(), "'"].concat(),
                )),
            },
        }
    }
}

/// Mode presentation with its bits.
pub type Mode = ProxyType<Option<String>, Option<u32>, ParseMode>;

/// Decode one optional unmanaged flag.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-boolean flags.
fn read_unmanaged_field(table: &Table) -> mlua::Result<Option<bool>> {
    let item: Value = table.get("unmanaged")?;
    match item {
        Value::Nil => Ok(None),
        Value::Boolean(flag) => Ok(Some(flag)),
        _ => Err(ApiError::mismatch("unmanaged flag", "a boolean", received(&item)).into()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::decode_config;
    use confit_surface_face::module::Named;
    use mlua_extras::typed::{Type, Typed};

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn config_of(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    fn documents_of(lua: &Lua, body: &str) -> (confit_model::routes::Route, Document) {
        let source = [
            "return { name = \"tools/test\", documents = { [{ base = \"home\", relative = \"subject\" }] = ",
            body,
            " } }",
        ]
        .concat();
        let value = config_of(lua, &source);
        match decode_config(lua, &value) {
            Ok(decoded) => {
                assert_eq!(decoded.documents.len(), 1);
                decoded.documents.into_iter().next().unwrap()
            }
            Err(error) => panic!("body decodes: {error}"),
        }
    }

    fn body_error(lua: &Lua, body: &str) -> String {
        let source = [
            "return { name = \"tools/test\", documents = { [{ base = \"home\", relative = \"subject\" }] = ",
            body,
            " } }",
        ]
        .concat();
        let value = config_of(lua, &source);
        match decode_config(lua, &value) {
            Ok(_) => panic!("foreign body passes: {body}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn body_face_unions_member_faces() {
        assert_eq!(
            Document::ty(),
            Type::union(vec![
                Named::<TextDocument>::ty(),
                Named::<StructuredDocument>::ty(),
                Named::<LinkDocument>::ty(),
                Named::<RcDocument>::ty(),
                Named::<OpaqueDocument>::ty(),
                Named::<TreeDocument>::ty(),
            ])
        );
    }

    #[test]
    fn text_decodes_mode_with_unmanaged() {
        let lua = test_lua();
        let documents = documents_of(&lua, r#"{ text = "hi", mode = "644", unmanaged = true }"#);
        match documents.1 {
            Document::Text(found) => {
                assert_eq!(found.text, "hi");
                assert_eq!(found.mode.into_inner(), Some(0o644));
                assert_eq!(found.unmanaged, Some(true));
            }
            _ => panic!("text body decodes"),
        }
        let symbolic = documents_of(&lua, r#"{ text = "hi", mode = "rwxr-xr-x" }"#);
        match symbolic.1 {
            Document::Text(found) => {
                assert_eq!(found.mode.into_inner(), Some(0o755));
                assert_eq!(found.unmanaged, None);
            }
            _ => panic!("symbolic mode decodes"),
        }
        let refused = body_error(&lua, r#"{ text = "hi", mode = 644 }"#);
        assert!(
            refused.contains("expected a string"),
            "the error shows the fault: {refused}"
        );
        for (body, want) in [
            (r#"{ text = 1 }"#, "expected a string"),
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
            Document::Structured(found) => {
                assert_eq!(found.format, StructuredFormat::Json);
                assert_eq!(
                    found.data.get("alpha").and_then(|item| item.0.as_str()),
                    Some("one")
                );
            }
            _ => panic!("structured body decodes"),
        }
        let text = body_error(&lua, r#"{ format = "json" }"#);
        assert!(text.contains("data"), "the text shows the field: {text}");
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
            Document::Link(found) => assert_eq!(found.target.display(), "home('linked')"),
            _ => panic!("link body decodes"),
        }
        let text = body_error(&lua, r#"{ target = "linked" }"#);
        assert!(
            text.contains("a route table"),
            "the text shows the want: {text}"
        );
    }

    #[test]
    fn mixed_markers_fail_naming_keys() {
        let lua = test_lua();
        let text = body_error(
            &lua,
            r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
        );
        assert!(text.contains("text"), "the text shows the marker: {text}");
        assert!(text.contains("target"), "the text shows the marker: {text}");
        let bare = body_error(&lua, r#"{ nope = 1 }"#);
        assert!(bare.contains("nope"), "the error shows the key: {bare}");
    }

    fn opened_lua() -> (tempfile::TempDir, Lua) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        confit_driver::fs::create_dir_all(&root).unwrap();
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        confit_driver::fs::write(&root.join("other.txt"), b"world").unwrap();
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = confit_store::Stores::new(
            confit_store::StoreRoots {
                config_base: directory.path().join("config"),
                temp_base: directory.path().join("temp"),
                project_root: directory.path().join("project"),
            },
            sender,
        );
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let resource = crate::api::builtin(&lua, "confit.resource").unwrap();
        lua.globals().set("resource", resource).unwrap();
        (directory, lua)
    }

    #[test]
    fn opaque_decodes_from_handles() {
        let _guard = confit_driver::fs::TestGuard::install();
        let (_held, lua) = opened_lua();
        let documents = documents_of(
            &lua,
            r#"{ handle = assert(resource("note.txt")), mode = "644", unmanaged = true }"#,
        );
        match documents.1 {
            Document::Opaque(found) => {
                assert_eq!(found.mode.into_inner(), Some(0o644));
                assert!(found.unmanaged, "unmanaged flag carries");
            }
            _ => panic!("opaque body decodes"),
        }
        let refused = body_error(
            &lua,
            r#"{ handle = assert(resource("note.txt")), mode = 644 }"#,
        );
        assert!(
            refused.contains("expected a string"),
            "the error shows the fault: {refused}"
        );
        for (body, want) in [
            (
                r#"{ handle = assert(resource("note.txt")), mode = "zzz" }"#,
                "octal like '755'",
            ),
            (r#"{ handle = "note.txt" }"#, "a resource handle"),
        ] {
            let text = body_error(&lua, body);
            assert!(text.contains(want), "the text shows the fault: {text}");
        }
    }

    #[test]
    fn tree_nests_tables_into_slots() {
        let _guard = confit_driver::fs::TestGuard::install();
        let (_held, lua) = opened_lua();
        let body = r#"{
            members = {
                ["top.txt"] = assert(resource("note.txt")),
                ["sub"] = { ["nested.txt"] = assert(resource("other.txt")) },
            },
        }"#;
        let documents = documents_of(&lua, body);
        match documents.1 {
            Document::Tree(found) => {
                assert_eq!(found.members.0.len(), 2);
                assert!(
                    matches!(found.members.0.get("top.txt"), Some(TreeSlot::Handle(_))),
                    "files hold handles"
                );
                match found.members.0.get("sub") {
                    Some(TreeSlot::Tree(nested)) => {
                        assert_eq!(nested.0.len(), 1);
                        assert!(
                            matches!(nested.0.get("nested.txt"), Some(TreeSlot::Handle(_))),
                            "nested files hold handles"
                        );
                    }
                    _ => panic!("folders nest tables"),
                }
            }
            _ => panic!("tree body decodes"),
        }
        let bare = documents_of(&lua, r#"{ members = {} }"#);
        match bare.1 {
            Document::Tree(found) => assert!(found.members.0.is_empty(), "empty builds bare"),
            _ => panic!("bare folder decodes"),
        }
        for (body, want) in [
            (
                r#"{ members = { ["a/b"] = assert(resource("note.txt")) } }"#,
                "a single file name",
            ),
            (
                r#"{ members = { ["top.txt"] = "hi" } }"#,
                "a resource handle or a folder table",
            ),
        ] {
            let text = body_error(&lua, body);
            assert!(text.contains(want), "the text shows the fault: {text}");
        }
    }
}
