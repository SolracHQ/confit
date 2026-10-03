//! Convert
//!
//! Lua-to-model converters for documents and rc entries.

use mlua::{Table, Value};
use serde_json::Value as Json;

use super::Declared;
use crate::error::{EngineError, FieldRef, Scope};
use crate::lua::{TableExt, ValueExt, read_marker};
use crate::model::{
    LinkDecl, OpaqueDecl, RcEntryDecl, StructuredDecl, TextDecl, TreeDecl, TreeMemberDecl,
};
use crate::surface::handles::{LuaBlobHandle, LuaRoute};
use crate::surface::runtime::condition_from_json;
use confit_model::arg::Arg;
use confit_model::document::{RcEntry, RcOp, StructuredFormat};
use confit_model::routes::{Route, RouteBase};
use confit_store::handles::BlobHandle;

/// Converts one document table into registration form.
///
/// # Errors
///
/// - [`EngineError::Shape`] for missing markers.
/// - [`EngineError::Field`] for misshaped fields.
/// - [`EngineError::Repeat`] for repeated tree members.
/// - [`EngineError::UnknownKind`] for unknown kinds.
pub(crate) fn convert_document(table: &Table, scope: &Scope) -> mlua::Result<Declared> {
    let kind = match read_marker(table, "__kind") {
        Some(kind) => kind,
        None => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a confit.document value (missing '__kind')",
            }
            .into());
        }
    };
    match kind.as_str() {
        "structured" => {
            let format_name = read_marker(table, "__format").unwrap_or_default();
            let format =
                StructuredFormat::parse(&format_name).ok_or_else(|| EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("format"),
                    want: "must be one of 'json', 'toml', or 'yaml'",
                })?;
            let destination = req_destination(table, scope)?;
            let data_table = table
                .req_table(scope, "data")
                .map_err(|_| EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("data"),
                    want: "must be a table with string keys",
                })?;
            let data = data_table.req_object(scope, "data")?;
            Ok(Declared::Structured(StructuredDecl {
                destination,
                format,
                data,
            }))
        }
        "text" => {
            let destination = req_destination(table, scope)?;
            let content = table.req_str(scope, "content")?;
            let mode = read_mode(table, scope)?;
            let unmanaged = read_unmanaged(table, scope)?;
            Ok(Declared::Text(TextDecl {
                destination,
                content,
                mode,
                unmanaged,
            }))
        }
        "link" => {
            let destination = req_destination(table, scope)?;
            let target = table.req_str(scope, "target")?;
            Ok(Declared::Link(LinkDecl {
                destination,
                target,
            }))
        }
        "opaque" => {
            let destination = req_destination(table, scope)?;
            let blob = req_blob(table, scope)?;
            let size_value: Value = table.get("size")?;
            let size = size_value.req_int(scope, "size")?;
            if size < 0 {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("size"),
                    want: "must not be negative",
                }
                .into());
            }
            let mode = read_mode(table, scope)?;
            let unmanaged = read_unmanaged(table, scope)?;
            Ok(Declared::Opaque(OpaqueDecl {
                destination,
                blob,
                size: size as u64,
                mode,
                unmanaged,
            }))
        }
        "tree" => {
            let destination = req_destination(table, scope)?;
            let members_table = table.req_table(scope, "members")?;
            let len = members_table.raw_len();
            let mut members = Vec::with_capacity(len);
            for index in 1..=len {
                let item: Value = members_table.get(index)?;
                let member = item
                    .req_table(scope, "members")
                    .map_err(|_| EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::name("members"),
                        want: "must hold member tables",
                    })?;
                let rel = member.req_str(scope, "rel")?;
                super::check_rel(scope, &rel)?;
                if members.iter().any(|item: &TreeMemberDecl| item.rel == rel) {
                    return Err(EngineError::Repeat {
                        scope: scope.clone(),
                        collection: "tree",
                        item: rel.clone(),
                    }
                    .into());
                }
                let blob = req_blob(&member, scope)?;
                let size_value: Value = member.get("size")?;
                let size = size_value.req_int(scope, "size")?;
                if size < 0 {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::name("size"),
                        want: "must not be negative",
                    }
                    .into());
                }
                let mode_value: Value = member.get("mode")?;
                let mode = mode_value.req_int(scope, "mode")?;
                if !(0..=0o777).contains(&mode) {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::name("mode"),
                        want: "must hold permission bits",
                    }
                    .into());
                }
                members.push(TreeMemberDecl {
                    rel,
                    blob,
                    size: size as u64,
                    mode: mode as u32,
                });
            }
            Ok(Declared::Tree(TreeDecl {
                destination,
                members,
            }))
        }
        "rc" => {
            let mut entries = Vec::new();
            for section in ["profile", "config", "final"] {
                let list_value: Value = table.get(section)?;
                if list_value.is_nil() {
                    continue;
                }
                let list =
                    list_value
                        .req_table(scope, section)
                        .map_err(|_| EngineError::Field {
                            scope: scope.clone(),
                            field: FieldRef::name(section),
                            want: "must be a list of rc entry tables",
                        })?;
                let len = list.raw_len();
                for index in 1..=len {
                    let item: Value = list.get(index)?;
                    let entry_table =
                        item.req_table(scope, section)
                            .map_err(|_| EngineError::Field {
                                scope: scope.clone(),
                                field: FieldRef::name(section),
                                want: "must be a list of rc entry tables",
                            })?;
                    entries.push(convert_section_entry(
                        &entry_table,
                        section,
                        &scope.slot(FieldRef::name(section)),
                    )?);
                }
            }
            Ok(Declared::Rc(entries))
        }
        other => Err(EngineError::UnknownKind {
            scope: scope.clone(),
            what: "document kind",
            name: other.to_owned(),
        }
        .into()),
    }
}

/// Reads one destination route from a document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for missing route paths.
fn req_destination(table: &Table, scope: &Scope) -> mlua::Result<Route> {
    let value: Value = table.get("path")?;
    super::super::handles::req_route(&value, scope, "path")
}

/// Reads one sealed blob handle from a document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-blob handles.
fn req_blob(table: &Table, scope: &Scope) -> mlua::Result<BlobHandle> {
    let value: Value = table.get("blob")?;
    let Some(data) = value.as_userdata() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("blob"),
            want: "must be a blob handle",
        }
        .into());
    };
    match data.borrow::<LuaBlobHandle>() {
        Ok(handle) => Ok(handle.core().clone()),
        Err(_) => Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("blob"),
            want: "must be a blob handle",
        }
        .into()),
    }
}

/// Reads the stamped mode bits from a document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for corrupt mode stamps.
///
fn read_mode(table: &Table, scope: &Scope) -> mlua::Result<Option<u32>> {
    let Some(text) = read_marker(table, "__mode") else {
        return Ok(None);
    };
    text.parse::<u32>().map(Some).map_err(|_| {
        EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("mode"),
            want: "holds a corrupt stamp",
        }
        .into()
    })
}

/// Reads the unmanaged flag from a document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-boolean flags.
///
fn read_unmanaged(table: &Table, scope: &Scope) -> mlua::Result<bool> {
    let value: Value = table.get("unmanaged")?;
    match value {
        Value::Nil => Ok(false),
        Value::Boolean(flag) => Ok(flag),
        _ => Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("unmanaged"),
            want: "must be a boolean",
        }
        .into()),
    }
}

/// Converts one section bucket rc entry table into registration form.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped entries.
/// - [`EngineError::UnknownField`] for unknown fields.
/// - [`EngineError::Field`] for misshaped fields.
fn convert_section_entry(table: &Table, section: &str, scope: &Scope) -> mlua::Result<RcEntryDecl> {
    let json = translate_entry(table, scope)?;
    Ok(RcEntryDecl {
        section: section.to_string(),
        json,
    })
}

/// Translates one rc entry table into canonical JSON.
///
/// Strings pass through intact. Route userdata renders as
/// route objects. Dense argv arrays translate item by item
/// through the hook slot reader. Condition guards convert
/// as data.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped entries.
/// - [`EngineError::UnknownBare`] for missing kinds.
/// - [`EngineError::UnknownField`] for unknown fields.
/// - [`EngineError::Field`] for misshaped guards.
pub(crate) fn translate_entry(table: &Table, scope: &Scope) -> mlua::Result<Json> {
    let mut found: Option<&'static str> = None;
    for key in ["env", "path", "alias", "eval", "cmd", "source"] {
        let value: Value = table.get(key)?;
        if !value.is_nil() {
            if found.is_some() {
                return Err(EngineError::Shape {
                    scope: scope.clone(),
                    want: "holds more than one rc entry kind",
                }
                .into());
            }
            found = Some(key);
        }
    }
    let key = found.ok_or_else(|| EngineError::UnknownBare {
        scope: scope.clone(),
        what: "rc entry kind",
    })?;
    let inner_value: Value = table.get(key)?;
    let Some(inner) = inner_value.opt_table() else {
        return Err(EngineError::Shape {
            scope: scope.clone(),
            want: "holds no rc entry table",
        }
        .into());
    };
    for pair in table.pairs::<Value, Value>() {
        let (field, _) = pair?;
        let Some(name) = field.opt_str() else {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "holds a non-string field",
            }
            .into());
        };
        if name != key && name != "when" {
            return Err(EngineError::UnknownField {
                scope: scope.clone(),
                name,
            }
            .into());
        }
    }
    let mut object = serde_json::Map::new();
    object.insert(key.to_string(), translate_op(key, &inner, scope)?);
    let when_value: Value = table.get("when")?;
    if !when_value.is_nil() {
        let Some(guard) = when_value.opt_table() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("when"),
                want: "must be a condition table",
            }
            .into());
        };
        object.insert(
            "when".to_string(),
            guard.to_json(&scope.slot(FieldRef::name("when")))?,
        );
    }
    Ok(Json::Object(object))
}

/// Translates one entry inner table into canonical JSON.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped entries.
/// - [`EngineError::UnknownField`] for unknown fields.
/// - [`EngineError::Field`] for misshaped fields.
fn translate_op(key: &str, inner: &Table, scope: &Scope) -> mlua::Result<Json> {
    let mut map = serde_json::Map::new();
    match key {
        "env" => {
            check_inner(inner, &["name", "value"], scope)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", scope)?),
            );
            map.insert(
                "value".to_string(),
                Json::String(inner_string(inner, "value", scope)?),
            );
        }
        "path" => {
            check_inner(inner, &["name", "dir"], scope)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", scope)?),
            );
            let dir_value: Value = inner.get("dir")?;
            map.insert("dir".to_string(), translate_slot(dir_value, "dir", scope)?);
        }
        "alias" => {
            check_inner(inner, &["name", "expansion"], scope)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", scope)?),
            );
            map.insert(
                "expansion".to_string(),
                Json::String(inner_string(inner, "expansion", scope)?),
            );
        }
        "eval" | "cmd" => {
            check_inner(inner, &["argv"], scope)?;
            let argv_value: Value = inner.get("argv")?;
            map.insert(
                "argv".to_string(),
                translate_slots(argv_value, "argv", scope)?,
            );
        }
        _ => {
            check_inner(inner, &["path"], scope)?;
            let path_value: Value = inner.get("path")?;
            map.insert(
                "path".to_string(),
                translate_slot(path_value, "path", scope)?,
            );
        }
    }
    Ok(Json::Object(map))
}

/// Translates one dense string-or-route array value into JSON.
///
/// Strings pass through intact. Route userdata and live
/// route tables render as route objects. Density follows
/// the hook slot reader.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-dense arrays.
fn translate_slots(value: Value, field: &str, scope: &Scope) -> mlua::Result<Json> {
    let field = FieldRef::name(field);
    let Some(list) = value.opt_table() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a dense string-or-route array",
        }
        .into());
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in list.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a dense string-or-route array",
            }
            .into());
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a dense string-or-route array starting at 1",
            }
            .into());
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (_, item) in indexed {
        out.push(translate_slot(item, &field.to_string(), scope)?);
    }
    Ok(Json::Array(out))
}

/// Translates one route slot value into canonical JSON.
///
/// Strings pass through intact. Route userdata renders as
/// a route object. Route tables from live roundtrips
/// normalize into route objects.
///
/// # Errors
///
/// - [`EngineError::SlotRender`] for render failures.
/// - [`EngineError::Field`] for non-route values.
fn translate_slot(value: Value, field: &str, scope: &Scope) -> mlua::Result<Json> {
    if let Some(text) = value.clone().opt_str() {
        return Ok(Json::String(text));
    }
    if let Some(data) = value.as_userdata() {
        if let Ok(route) = data.borrow::<LuaRoute>() {
            return serde_json::to_value(route.core().clone()).map_err(|error| {
                EngineError::SlotRender {
                    scope: scope.clone(),
                    field: FieldRef::name(field),
                    reason: error.to_string(),
                }
                .into()
            });
        }
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name(field),
            want: "must be a string or a confit.path value",
        }
        .into());
    }
    if let Some(table) = value.opt_table() {
        return translate_route_table(&table, field, scope);
    }
    Err(EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name(field),
        want: "must be a string or a confit.path value",
    }
    .into())
}

/// Normalizes one live route table into a route object.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route tables.
fn translate_route_table(table: &Table, field: &str, scope: &Scope) -> mlua::Result<Json> {
    let field = FieldRef::name(field);
    let json = table.to_json(&scope.slot(field.clone()))?;
    let Json::Object(map) = &json else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a string or a confit.path value",
        }
        .into());
    };
    match (
        map.get("base").and_then(Json::as_str),
        map.get("relative").and_then(Json::as_str),
    ) {
        (Some(_), Some(_)) if map.len() == 2 => Ok(json),
        _ => Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a string or a confit.path value",
        }
        .into()),
    }
}

/// Rejects unknown keys on an entry inner table.
///
/// # Errors
///
/// - [`EngineError::Shape`] for non-string fields.
/// - [`EngineError::UnknownField`] for unknown fields.
fn check_inner(inner: &Table, known: &[&str], scope: &Scope) -> mlua::Result<()> {
    for pair in inner.pairs::<Value, Value>() {
        let (field, _) = pair?;
        let Some(name) = field.opt_str() else {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "holds a non-string field",
            }
            .into());
        };
        if !known.contains(&name.as_str()) {
            return Err(EngineError::UnknownField {
                scope: scope.clone(),
                name,
            }
            .into());
        }
    }
    Ok(())
}

/// Reads one required string field from an entry inner table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string fields.
fn inner_string(inner: &Table, field: &str, scope: &Scope) -> mlua::Result<String> {
    let value: Value = inner.get(field)?;
    value.opt_str().ok_or_else(|| {
        EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name(field),
            want: "must be a string",
        }
        .into()
    })
}

/// Reads the single entry kind from one entry object.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped entries.
/// - [`EngineError::UnknownBare`] for missing kinds.
fn entry_kind(json: &Json, scope: &Scope) -> mlua::Result<&'static str> {
    let object = match json {
        Json::Object(map) => map,
        _ => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "holds no rc entry table",
            }
            .into());
        }
    };
    let mut found: Option<&'static str> = None;
    for key in ["env", "path", "alias", "eval", "cmd", "source"] {
        if object.contains_key(key) {
            if found.is_some() {
                return Err(EngineError::Shape {
                    scope: scope.clone(),
                    want: "holds more than one rc entry kind",
                }
                .into());
            }
            found = Some(key);
        }
    }
    found.ok_or_else(|| {
        EngineError::UnknownBare {
            scope: scope.clone(),
            what: "rc entry kind",
        }
        .into()
    })
}

/// Derives the slot key and display name for one entry.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
/// - [`EngineError::RenderDetail`] for render failures.
/// - [`EngineError::Shape`] for misshaped entries.
/// - [`EngineError::UnknownBare`] for missing kinds.
pub(crate) fn entry_slot(
    json: &Json,
    section: &str,
    scope: &Scope,
) -> mlua::Result<Option<(String, String, &'static str)>> {
    let invalid = || EngineError::RcEntry {
        scope: scope.clone(),
        section: section.to_owned(),
    };
    let object = match json {
        Json::Object(map) => map,
        _ => return Err(invalid().into()),
    };
    let when_text = match object.get("when") {
        Some(when) => crate::lua::json_text(when, scope)?,
        None => "null".to_string(),
    };
    let kind = entry_kind(json, scope)?;
    if matches!(kind, "eval" | "cmd" | "source") {
        return Ok(None);
    }
    let inner = match object.get(kind) {
        Some(Json::Object(map)) => map,
        _ => return Err(invalid().into()),
    };
    let name = match inner.get("name").and_then(Json::as_str) {
        Some(name) => name.to_string(),
        None => return Err(invalid().into()),
    };
    Ok(Some((name.clone(), format!("{name}/{when_text}"), kind)))
}

/// Pushes one live entry JSON into the core rc lists.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
/// - [`EngineError::Field`] for misshaped guards.
pub(crate) fn push_live_entry(
    profile: &mut Vec<RcEntry>,
    config: &mut Vec<RcEntry>,
    finals: &mut Vec<RcEntry>,
    section: &str,
    json: &Json,
    scope: &Scope,
) -> mlua::Result<()> {
    let invalid = || EngineError::RcEntry {
        scope: scope.clone(),
        section: section.to_owned(),
    };
    let object = match json {
        Json::Object(map) => map,
        _ => {
            return Err(invalid().into());
        }
    };
    let when = match object.get("when") {
        None | Some(Json::Null) => None,
        Some(raw) => {
            let cond = condition_from_json(raw, &scope.slot(FieldRef::name("when")))?;
            if cond.holds_changed() {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("when"),
                    want: "holds 'changed' (hooks only)",
                }
                .into());
            }
            Some(cond)
        }
    };
    let kind = entry_kind(json, scope)?;
    let op = match kind {
        "env" => {
            let inner = entry_object(object, kind, section, scope)?;
            check_fields(inner, &["name", "value"], section, scope)?;
            RcOp::Env {
                name: entry_string(inner, "name", section, scope)?,
                value: entry_string(inner, "value", section, scope)?,
            }
        }
        "path" => {
            let inner = entry_object(object, kind, section, scope)?;
            check_fields(inner, &["name", "dir"], section, scope)?;
            RcOp::Path {
                name: entry_string(inner, "name", section, scope)?,
                dir: entry_route(inner, "dir", section, scope)?,
            }
        }
        "alias" => {
            let inner = entry_object(object, kind, section, scope)?;
            check_fields(inner, &["name", "expansion"], section, scope)?;
            RcOp::Alias {
                name: entry_string(inner, "name", section, scope)?,
                expansion: entry_string(inner, "expansion", section, scope)?,
            }
        }
        "eval" | "cmd" => {
            let inner = entry_object(object, kind, section, scope)?;
            check_fields(inner, &["argv"], section, scope)?;
            let argv = entry_args(inner, section, scope)?;
            if kind == "eval" {
                RcOp::Eval { argv }
            } else {
                RcOp::Cmd { argv }
            }
        }
        _ => {
            let inner = entry_object(object, kind, section, scope)?;
            check_fields(inner, &["path"], section, scope)?;
            RcOp::Source {
                path: entry_route(inner, "path", section, scope)?,
            }
        }
    };
    check_fields(object, &[kind, "when"], section, scope)?;
    let entry = RcEntry { op, when };
    match section {
        "profile" => profile.push(entry),
        "config" => config.push(entry),
        "final" => finals.push(entry),
        _ => {
            return Err(invalid().into());
        }
    }
    Ok(())
}

/// Reads one entry inner object from an entry object.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
fn entry_object<'a>(
    object: &'a serde_json::Map<String, Json>,
    kind: &str,
    section: &str,
    scope: &Scope,
) -> mlua::Result<&'a serde_json::Map<String, Json>> {
    match object.get(kind) {
        Some(Json::Object(map)) => Ok(map),
        _ => Err(EngineError::RcEntry {
            scope: scope.clone(),
            section: section.to_owned(),
        }
        .into()),
    }
}

/// Reads one argv array from an exec inner object.
///
/// Strings run verbatim. Route objects resolve at apply
/// time through the workspace.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
fn entry_args(
    inner: &serde_json::Map<String, Json>,
    section: &str,
    scope: &Scope,
) -> mlua::Result<Vec<Arg>> {
    let invalid = || EngineError::RcEntry {
        scope: scope.clone(),
        section: section.to_owned(),
    };
    match inner.get("argv") {
        Some(Json::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let slot: Arg = serde_json::from_value(item.clone()).map_err(|_| invalid())?;
                out.push(slot);
            }
            Ok(out)
        }
        _ => Err(invalid().into()),
    }
}

/// Reads one route slot from an entry inner object.
///
/// Strings parse as route displays, raw paths fall back
/// to literals. Route objects decode through serde.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
fn entry_route(
    inner: &serde_json::Map<String, Json>,
    field: &str,
    section: &str,
    scope: &Scope,
) -> mlua::Result<Route> {
    let invalid = || EngineError::RcEntry {
        scope: scope.clone(),
        section: section.to_owned(),
    };
    match inner.get(field) {
        Some(Json::String(text)) => Route::parse(text)
            .or_else(|_| Route::new(RouteBase::Literal, text.clone()).map_err(|_| invalid()))
            .map_err(mlua::Error::from),
        Some(value @ Json::Object(_)) => {
            serde_json::from_value::<Route>(value.clone()).map_err(|_| mlua::Error::from(invalid()))
        }
        _ => Err(invalid().into()),
    }
}

/// Reads one required string field from an entry object.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
fn entry_string(
    object: &serde_json::Map<String, Json>,
    field: &str,
    section: &str,
    scope: &Scope,
) -> mlua::Result<String> {
    match object.get(field).and_then(Json::as_str) {
        Some(value) => Ok(value.to_string()),
        None => Err(EngineError::RcEntry {
            scope: scope.clone(),
            section: section.to_owned(),
        }
        .into()),
    }
}

/// Rejects unknown keys on an entry object.
///
/// # Errors
///
/// - [`EngineError::RcEntry`] for bad rc entries.
fn check_fields(
    object: &serde_json::Map<String, Json>,
    known: &[&str],
    section: &str,
    scope: &Scope,
) -> mlua::Result<()> {
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            return Err(EngineError::RcEntry {
                scope: scope.clone(),
                section: section.to_owned(),
            }
            .into());
        }
    }
    Ok(())
}
