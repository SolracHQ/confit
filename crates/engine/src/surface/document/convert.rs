//! Convert
//!
//! Lua-to-model converters for documents and rc entries.

use mlua::{Table, Value};
use serde_json::Value as Json;

use super::Declared;
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt, read_marker};
use crate::model::{
    LinkDecl, OpaqueDecl, RcEntryDecl, SecretDecl, StructuredDecl, TextDecl, TreeDecl,
    TreeMemberDecl,
};
use crate::surface::handles::{LuaBlobHandle, LuaRoute};
use crate::surface::runtime::condition_from_json;
use confit_core::arg::Arg;
use confit_core::document::{PathOp, RcEntry, RcOp, StructuredFormat};
use confit_core::handles::{BlobHandle, Route, RouteBase};

/// Converts one document table into registration form.
pub(crate) fn convert_document(table: &Table, ctx: &str) -> mlua::Result<Declared> {
    let kind = match read_marker(table, "__kind") {
        Some(kind) => kind,
        None => {
            return Err(plan_error(format!(
                "{ctx} must be a confit.document value (missing '__kind')"
            )));
        }
    };
    match kind.as_str() {
        "structured" => {
            let format_name = read_marker(table, "__format").unwrap_or_default();
            let format = StructuredFormat::parse(&format_name).ok_or_else(|| {
                plan_error(format!(
                    "{ctx}: field 'format' must be one of 'json', 'toml', or 'yaml'"
                ))
            })?;
            let destination = req_destination(table, ctx)?;
            let data_table = table.req_table(ctx, "data").map_err(|_| {
                plan_error(format!(
                    "{ctx}: field 'data' must be a table with string keys"
                ))
            })?;
            let data = data_table.req_object(ctx, "data")?;
            Ok(Declared::Structured(StructuredDecl {
                destination,
                format,
                data,
            }))
        }
        "text" => {
            let destination = req_destination(table, ctx)?;
            let content = table.req_str(ctx, "content")?;
            let mode = read_mode(table, ctx)?;
            let unmanaged = read_unmanaged(table, ctx)?;
            Ok(Declared::Text(TextDecl {
                destination,
                content,
                mode,
                unmanaged,
            }))
        }
        "link" => {
            let destination = req_destination(table, ctx)?;
            let target = table.req_str(ctx, "target")?;
            Ok(Declared::Link(LinkDecl {
                destination,
                target,
            }))
        }
        "opaque" => {
            let destination = req_destination(table, ctx)?;
            let blob = req_blob(table, ctx)?;
            let size_value: Value = table.get("size")?;
            let size = size_value.req_int(ctx, "size")?;
            if size < 0 {
                return Err(plan_error(format!(
                    "{ctx}: field 'size' must not be negative"
                )));
            }
            let mode = read_mode(table, ctx)?;
            let unmanaged = read_unmanaged(table, ctx)?;
            Ok(Declared::Opaque(OpaqueDecl {
                destination,
                blob,
                size: size as u64,
                mode,
                unmanaged,
            }))
        }
        "tree" => {
            let destination = req_destination(table, ctx)?;
            let members_table = table.req_table(ctx, "members")?;
            let len = members_table.raw_len();
            let mut members = Vec::with_capacity(len);
            for index in 1..=len {
                let item: Value = members_table.get(index)?;
                let member = item.req_table(ctx, "members").map_err(|_| {
                    plan_error(format!("{ctx}: field 'members' must hold member tables"))
                })?;
                let rel = member.req_str(ctx, "rel")?;
                super::check_rel(ctx, &rel)?;
                if members.iter().any(|item: &TreeMemberDecl| item.rel == rel) {
                    return Err(plan_error(format!(
                        "{ctx}: tree keeps '{rel}' more than once"
                    )));
                }
                let blob = req_blob(&member, ctx)?;
                let size_value: Value = member.get("size")?;
                let size = size_value.req_int(ctx, "size")?;
                if size < 0 {
                    return Err(plan_error(format!(
                        "{ctx}: field 'size' must not be negative"
                    )));
                }
                let mode_value: Value = member.get("mode")?;
                let mode = mode_value.req_int(ctx, "mode")?;
                if !(0..=0o777).contains(&mode) {
                    return Err(plan_error(format!(
                        "{ctx}: field 'mode' must hold permission bits"
                    )));
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
        "secret" => {
            let destination = req_destination(table, ctx)?;
            let argv_value: Value = table.get("argv")?;
            let argv = crate::surface::hook::read_slots(&argv_value, ctx, "argv")?;
            if argv.is_empty() {
                return Err(plan_error(format!("{ctx}: field 'argv' must not be empty")));
            }
            let mode = read_mode(table, ctx)?;
            Ok(Declared::Secret(SecretDecl {
                destination,
                argv,
                mode,
            }))
        }
        "rc" => {
            let mut entries = Vec::new();
            for section in ["profile", "config", "final"] {
                let list_value: Value = table.get(section)?;
                if list_value.is_nil() {
                    continue;
                }
                let list = list_value.req_table(ctx, section).map_err(|_| {
                    plan_error(format!(
                        "{ctx}: field '{section}' must be a list of rc entry tables"
                    ))
                })?;
                let len = list.raw_len();
                for index in 1..=len {
                    let item: Value = list.get(index)?;
                    let entry_table = item.req_table(ctx, section).map_err(|_| {
                        plan_error(format!(
                            "{ctx}: field '{section}' must be a list of rc entry tables"
                        ))
                    })?;
                    entries.push(convert_section_entry(
                        &entry_table,
                        section,
                        &format!("{ctx}: field '{section}'"),
                    )?);
                }
            }
            Ok(Declared::Rc(entries))
        }
        other => Err(plan_error(format!("{ctx} unknown document kind '{other}'"))),
    }
}

/// Reads one destination route from a document table.
fn req_destination(table: &Table, ctx: &str) -> mlua::Result<Route> {
    let value: Value = table.get("path")?;
    super::super::handles::req_route(&value, ctx, "path")
}

/// Reads one sealed blob handle from a document table.
fn req_blob(table: &Table, ctx: &str) -> mlua::Result<BlobHandle> {
    let value: Value = table.get("blob")?;
    let Some(data) = value.as_userdata() else {
        return Err(plan_error(format!(
            "{ctx}: field 'blob' must be a blob handle"
        )));
    };
    match data.borrow::<LuaBlobHandle>() {
        Ok(handle) => Ok(handle.core().clone()),
        Err(_) => Err(plan_error(format!(
            "{ctx}: field 'blob' must be a blob handle"
        ))),
    }
}

/// Reads the stamped mode bits from a document table.
///
/// # Arguments
///
/// * `table` - document table carrying the mode marker.
/// * `ctx` - error prefix naming the constructor.
///
/// # Returns
///
/// Mode bits for stamped tables, holding `None` for default handling.
///
/// # Errors
///
/// Corrupt mode stamps fail as plan errors.
///
fn read_mode(table: &Table, ctx: &str) -> mlua::Result<Option<u32>> {
    let Some(text) = read_marker(table, "__mode") else {
        return Ok(None);
    };
    text.parse::<u32>()
        .map(Some)
        .map_err(|_| plan_error(format!("{ctx}: field 'mode' holds a corrupt stamp")))
}

/// Reads the unmanaged flag from a document table.
///
/// # Arguments
///
/// * `table` - document table carrying the unmanaged field.
/// * `ctx` - error prefix naming the constructor.
///
/// # Returns
///
/// The flag, holding false for missing fields.
///
/// # Errors
///
/// Non-boolean flags fail as plan errors.
///
fn read_unmanaged(table: &Table, ctx: &str) -> mlua::Result<bool> {
    let value: Value = table.get("unmanaged")?;
    match value {
        Value::Nil => Ok(false),
        Value::Boolean(flag) => Ok(flag),
        _ => Err(plan_error(format!(
            "{ctx}: field 'unmanaged' must be a boolean"
        ))),
    }
}

/// Converts one section bucket rc entry table into registration form.
fn convert_section_entry(table: &Table, section: &str, ctx: &str) -> mlua::Result<RcEntryDecl> {
    let json = translate_entry(table, ctx)?;
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
/// as data. Anything else fails naming the field.
pub(crate) fn translate_entry(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut found: Option<&'static str> = None;
    for key in ["env", "path", "alias", "eval", "cmd", "source"] {
        let value: Value = table.get(key)?;
        if !value.is_nil() {
            if found.is_some() {
                return Err(plan_error(format!(
                    "{ctx} holds more than one rc entry kind"
                )));
            }
            found = Some(key);
        }
    }
    let key = found.ok_or_else(|| plan_error(format!("{ctx} unknown rc entry kind")))?;
    let inner_value: Value = table.get(key)?;
    let Some(inner) = inner_value.opt_table() else {
        return Err(plan_error(format!("{ctx} holds no rc entry table")));
    };
    for pair in table.pairs::<Value, Value>() {
        let (field, _) = pair?;
        let Some(name) = field.opt_str() else {
            return Err(plan_error(format!("{ctx} holds a non-string field")));
        };
        if name != key && name != "when" {
            return Err(plan_error(format!("{ctx} holds unknown field '{name}'")));
        }
    }
    let mut object = serde_json::Map::new();
    object.insert(key.to_string(), translate_op(key, &inner, ctx)?);
    let when_value: Value = table.get("when")?;
    if !when_value.is_nil() {
        let Some(guard) = when_value.opt_table() else {
            return Err(plan_error(format!(
                "{ctx}: field 'when' must be a condition table"
            )));
        };
        object.insert(
            "when".to_string(),
            guard.to_json(&format!("{ctx}: field 'when'"))?,
        );
    }
    Ok(Json::Object(object))
}

/// Translates one op inner table into canonical JSON.
fn translate_op(key: &str, inner: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut map = serde_json::Map::new();
    match key {
        "env" => {
            check_inner(inner, &["name", "value"], ctx)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", ctx)?),
            );
            map.insert(
                "value".to_string(),
                Json::String(inner_string(inner, "value", ctx)?),
            );
        }
        "path" => {
            check_inner(inner, &["name", "dir", "op"], ctx)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", ctx)?),
            );
            let dir_value: Value = inner.get("dir")?;
            map.insert("dir".to_string(), translate_slot(dir_value, "dir", ctx)?);
            let op_value: Value = inner.get("op")?;
            let is_prepend = op_value.opt_str().is_some_and(|text| text == "prepend");
            if !is_prepend {
                return Err(plan_error(format!("{ctx}: field 'op' must be 'prepend'")));
            }
            map.insert("op".to_string(), Json::String("prepend".to_string()));
        }
        "alias" => {
            check_inner(inner, &["name", "expansion"], ctx)?;
            map.insert(
                "name".to_string(),
                Json::String(inner_string(inner, "name", ctx)?),
            );
            map.insert(
                "expansion".to_string(),
                Json::String(inner_string(inner, "expansion", ctx)?),
            );
        }
        "eval" | "cmd" => {
            check_inner(inner, &["argv"], ctx)?;
            let argv_value: Value = inner.get("argv")?;
            map.insert(
                "argv".to_string(),
                translate_slots(argv_value, "argv", ctx)?,
            );
        }
        _ => {
            check_inner(inner, &["path"], ctx)?;
            let path_value: Value = inner.get("path")?;
            map.insert("path".to_string(), translate_slot(path_value, "path", ctx)?);
        }
    }
    Ok(Json::Object(map))
}

/// Translates one dense string-or-route array value into JSON.
///
/// Strings pass through intact. Route userdata and live
/// route tables render as route objects. Density follows
/// the hook slot reader.
fn translate_slots(value: Value, field: &str, ctx: &str) -> mlua::Result<Json> {
    const DENSE: &str = "must be a dense string-or-route array";
    let Some(list) = value.opt_table() else {
        return Err(plan_error(format!("{ctx}: field '{field}' {DENSE}")));
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in list.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(plan_error(format!("{ctx}: field '{field}' {DENSE}")));
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(plan_error(format!(
                "{ctx}: field '{field}' must be a dense string-or-route array starting at 1"
            )));
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (_, item) in indexed {
        out.push(translate_slot(item, field, ctx)?);
    }
    Ok(Json::Array(out))
}

/// Translates one route slot value into canonical JSON.
///
/// Strings pass through intact. Route userdata renders as
/// a route object. Route tables from live roundtrips
/// normalize into route objects.
fn translate_slot(value: Value, field: &str, ctx: &str) -> mlua::Result<Json> {
    if let Some(text) = value.clone().opt_str() {
        return Ok(Json::String(text));
    }
    if let Some(data) = value.as_userdata() {
        if let Ok(route) = data.borrow::<LuaRoute>() {
            return serde_json::to_value(route.core().clone()).map_err(|error| {
                plan_error(format!("{ctx}: field '{field}' failed to render: {error}"))
            });
        }
        return Err(plan_error(format!(
            "{ctx}: field '{field}' must be a string or a confit.path value"
        )));
    }
    if let Some(table) = value.opt_table() {
        return translate_route_table(&table, field, ctx);
    }
    Err(plan_error(format!(
        "{ctx}: field '{field}' must be a string or a confit.path value"
    )))
}

/// Normalizes one live route table into a route object.
fn translate_route_table(table: &Table, field: &str, ctx: &str) -> mlua::Result<Json> {
    let json = table.to_json(&format!("{ctx}: field '{field}'"))?;
    let Json::Object(map) = &json else {
        return Err(plan_error(format!(
            "{ctx}: field '{field}' must be a string or a confit.path value"
        )));
    };
    match (
        map.get("base").and_then(Json::as_str),
        map.get("relative").and_then(Json::as_str),
    ) {
        (Some(_), Some(_)) if map.len() == 2 => Ok(json),
        _ => Err(plan_error(format!(
            "{ctx}: field '{field}' must be a string or a confit.path value"
        ))),
    }
}

/// Rejects unknown keys on an op inner table.
fn check_inner(inner: &Table, known: &[&str], ctx: &str) -> mlua::Result<()> {
    for pair in inner.pairs::<Value, Value>() {
        let (field, _) = pair?;
        let Some(name) = field.opt_str() else {
            return Err(plan_error(format!("{ctx} holds a non-string field")));
        };
        if !known.contains(&name.as_str()) {
            return Err(plan_error(format!("{ctx} holds unknown field '{name}'")));
        }
    }
    Ok(())
}

/// Reads one required string field from an op inner table.
fn inner_string(inner: &Table, field: &str, ctx: &str) -> mlua::Result<String> {
    let value: Value = inner.get(field)?;
    value
        .opt_str()
        .ok_or_else(|| plan_error(format!("{ctx}: field '{field}' must be a string")))
}

/// Reads the single op key from one entry object.
fn op_key(json: &Json, ctx: &str) -> mlua::Result<&'static str> {
    let object = match json {
        Json::Object(map) => map,
        _ => return Err(plan_error(format!("{ctx} holds no rc entry table"))),
    };
    let mut found: Option<&'static str> = None;
    for key in ["env", "path", "alias", "eval", "cmd", "source"] {
        if object.contains_key(key) {
            if found.is_some() {
                return Err(plan_error(format!(
                    "{ctx} holds more than one rc entry kind"
                )));
            }
            found = Some(key);
        }
    }
    found.ok_or_else(|| plan_error(format!("{ctx} unknown rc entry kind")))
}

/// Derives the slot key and display name for one entry.
pub(crate) fn entry_slot(
    json: &Json,
    section: &str,
    caller: &str,
) -> mlua::Result<Option<(String, String, &'static str)>> {
    let ctx = format!("{caller}: invalid rc entry for section '{section}'");
    let object = match json {
        Json::Object(map) => map,
        _ => return Err(plan_error(ctx)),
    };
    let when_text = match object.get("when") {
        Some(when) => crate::lua::json_text(when, &ctx)?,
        None => "null".to_string(),
    };
    let key = op_key(json, &ctx)?;
    if matches!(key, "eval" | "cmd" | "source") {
        return Ok(None);
    }
    let inner = match object.get(key) {
        Some(Json::Object(map)) => map,
        _ => return Err(plan_error(ctx)),
    };
    let name = match inner.get("name").and_then(Json::as_str) {
        Some(name) => name.to_string(),
        None => return Err(plan_error(ctx)),
    };
    Ok(Some((name.clone(), format!("{name}/{when_text}"), key)))
}

/// Pushes one live entry JSON into the core rc lists.
pub(crate) fn push_live_entry(
    profile: &mut Vec<RcEntry>,
    config: &mut Vec<RcEntry>,
    finals: &mut Vec<RcEntry>,
    section: &str,
    json: &Json,
    ctx: &str,
) -> mlua::Result<()> {
    let object = match json {
        Json::Object(map) => map,
        _ => {
            return Err(plan_error(format!(
                "{ctx}: invalid rc entry for section '{section}'"
            )));
        }
    };
    let when = match object.get("when") {
        None | Some(Json::Null) => None,
        Some(raw) => {
            let cond = condition_from_json(raw, &format!("{ctx}: field 'when'"))?;
            if cond.holds_changed() {
                return Err(plan_error(format!(
                    "{ctx}: field 'when' holds 'changed' (hooks only)"
                )));
            }
            Some(cond)
        }
    };
    let key = op_key(
        json,
        &format!("{ctx}: invalid rc entry for section '{section}'"),
    )?;
    let op = match key {
        "env" => {
            let inner = entry_object(object, key, section, ctx)?;
            check_fields(inner, &["name", "value"], section, ctx)?;
            RcOp::Env {
                name: entry_string(inner, "name", section, ctx)?,
                value: entry_string(inner, "value", section, ctx)?,
            }
        }
        "path" => {
            let inner = entry_object(object, key, section, ctx)?;
            check_fields(inner, &["name", "dir", "op"], section, ctx)?;
            RcOp::Path {
                name: entry_string(inner, "name", section, ctx)?,
                dir: entry_route(inner, "dir", section, ctx)?,
                op: match inner.get("op").and_then(Json::as_str) {
                    Some("prepend") => PathOp::Prepend,
                    _ => {
                        return Err(plan_error(format!(
                            "{ctx}: invalid rc entry for section '{section}'"
                        )));
                    }
                },
            }
        }
        "alias" => {
            let inner = entry_object(object, key, section, ctx)?;
            check_fields(inner, &["name", "expansion"], section, ctx)?;
            RcOp::Alias {
                name: entry_string(inner, "name", section, ctx)?,
                expansion: entry_string(inner, "expansion", section, ctx)?,
            }
        }
        "eval" | "cmd" => {
            let inner = entry_object(object, key, section, ctx)?;
            check_fields(inner, &["argv"], section, ctx)?;
            let argv = entry_args(inner, section, ctx)?;
            if key == "eval" {
                RcOp::Eval { argv }
            } else {
                RcOp::Cmd { argv }
            }
        }
        _ => {
            let inner = entry_object(object, key, section, ctx)?;
            check_fields(inner, &["path"], section, ctx)?;
            RcOp::Source {
                path: entry_route(inner, "path", section, ctx)?,
            }
        }
    };
    check_fields(object, &[key, "when"], section, ctx)?;
    let entry = RcEntry { op, when };
    match section {
        "profile" => profile.push(entry),
        "config" => config.push(entry),
        "final" => finals.push(entry),
        _ => {
            return Err(plan_error(format!(
                "{ctx}: invalid rc entry for section '{section}'"
            )));
        }
    }
    Ok(())
}

/// Reads one op inner object from an entry object.
fn entry_object<'a>(
    object: &'a serde_json::Map<String, Json>,
    key: &str,
    section: &str,
    ctx: &str,
) -> mlua::Result<&'a serde_json::Map<String, Json>> {
    match object.get(key) {
        Some(Json::Object(map)) => Ok(map),
        _ => Err(plan_error(format!(
            "{ctx}: invalid rc entry for section '{section}'"
        ))),
    }
}

/// Reads one argv array from an exec inner object.
///
/// Strings run verbatim. Route objects resolve at apply
/// time through the workspace.
fn entry_args(
    inner: &serde_json::Map<String, Json>,
    section: &str,
    ctx: &str,
) -> mlua::Result<Vec<Arg>> {
    let invalid = || plan_error(format!("{ctx}: invalid rc entry for section '{section}'"));
    match inner.get("argv") {
        Some(Json::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let slot: Arg = serde_json::from_value(item.clone()).map_err(|_| invalid())?;
                out.push(slot);
            }
            Ok(out)
        }
        _ => Err(invalid()),
    }
}

/// Reads one route slot from an entry inner object.
///
/// Strings parse as route displays, raw paths fall back
/// to literals. Route objects decode through serde.
fn entry_route(
    inner: &serde_json::Map<String, Json>,
    field: &str,
    section: &str,
    ctx: &str,
) -> mlua::Result<Route> {
    let invalid = || plan_error(format!("{ctx}: invalid rc entry for section '{section}'"));
    match inner.get(field) {
        Some(Json::String(text)) => Route::parse(text)
            .or_else(|_| Route::new(RouteBase::Literal, text.clone()).map_err(|_| invalid())),
        Some(value @ Json::Object(_)) => {
            serde_json::from_value::<Route>(value.clone()).map_err(|_| invalid())
        }
        _ => Err(invalid()),
    }
}

/// Reads one required string field from an entry object.
fn entry_string(
    object: &serde_json::Map<String, Json>,
    field: &str,
    section: &str,
    ctx: &str,
) -> mlua::Result<String> {
    match object.get(field).and_then(Json::as_str) {
        Some(value) => Ok(value.to_string()),
        None => Err(plan_error(format!(
            "{ctx}: invalid rc entry for section '{section}'"
        ))),
    }
}

/// Rejects unknown keys on an entry object.
fn check_fields(
    object: &serde_json::Map<String, Json>,
    known: &[&str],
    section: &str,
    ctx: &str,
) -> mlua::Result<()> {
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            return Err(plan_error(format!(
                "{ctx}: invalid rc entry for section '{section}'"
            )));
        }
    }
    Ok(())
}
