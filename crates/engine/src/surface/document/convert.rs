//! Convert
//!
//! Lua-to-model converters for documents plus rc entries.

use mlua::{Table, Value};
use serde_json::Value as Json;

use super::Declared;
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt, read_marker};
use crate::model::{
    LinkDecl, OpaqueDecl, RcEntryDecl, StructuredDecl, TextDecl, TreeDecl, TreeMemberDecl,
};
use crate::surface::runtime::condition_from_json;
use confit_core::document::{PathOp, RcEntry, RcOp, StructuredFormat};

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
            let path = table.req_str(ctx, "path")?;
            let data_table = table.req_table(ctx, "data").map_err(|_| {
                plan_error(format!(
                    "{ctx}: field 'data' must be a table with string keys"
                ))
            })?;
            let data = data_table.req_object(ctx, "data")?;
            Ok(Declared::Structured(StructuredDecl { path, format, data }))
        }
        "text" => {
            let path = table.req_str(ctx, "path")?;
            let content = table.req_str(ctx, "content")?;
            let mode = read_mode(table, ctx)?;
            let unmanaged = read_unmanaged(table, ctx)?;
            Ok(Declared::Text(TextDecl {
                path,
                content,
                mode,
                unmanaged,
            }))
        }
        "link" => {
            let path = table.req_str(ctx, "path")?;
            let target = table.req_str(ctx, "target")?;
            Ok(Declared::Link(LinkDecl { path, target }))
        }
        "opaque" => {
            let path = table.req_str(ctx, "path")?;
            let source = table.req_str(ctx, "source")?;
            let mode = read_mode(table, ctx)?;
            let unmanaged = read_unmanaged(table, ctx)?;
            Ok(Declared::Opaque(OpaqueDecl {
                path,
                source: std::path::PathBuf::from(source),
                mode,
                unmanaged,
            }))
        }
        "tree" => {
            let path = table.req_str(ctx, "path")?;
            let members_table = table.req_table(ctx, "members")?;
            let len = members_table.raw_len();
            let mut members = Vec::with_capacity(len);
            for index in 1..=len {
                let item: Value = members_table.get(index)?;
                let member = item.req_table(ctx, "members").map_err(|_| {
                    plan_error(format!("{ctx}: field 'members' must hold member tables"))
                })?;
                let rel = member.req_str(ctx, "rel")?;
                let source = member.req_str(ctx, "source")?;
                let mode_value: Value = member.get("mode")?;
                let mode = mode_value.req_int(ctx, "mode")?;
                if !(0..=0o777).contains(&mode) {
                    return Err(plan_error(format!(
                        "{ctx}: field 'mode' must hold permission bits"
                    )));
                }
                members.push(TreeMemberDecl {
                    rel,
                    source: std::path::PathBuf::from(source),
                    mode: mode as u32,
                });
            }
            Ok(Declared::Tree(TreeDecl { path, members }))
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
    let json = table.to_json(ctx)?;
    op_key(&json, ctx)?;
    Ok(RcEntryDecl {
        section: section.to_string(),
        json,
    })
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

/// Derives the slot key plus display name for one entry.
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
                dir: entry_string(inner, "dir", section, ctx)?,
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
            let argv = entry_argv(inner, section, ctx)?;
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
                path: entry_string(inner, "path", section, ctx)?,
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
fn entry_argv(
    inner: &serde_json::Map<String, Json>,
    section: &str,
    ctx: &str,
) -> mlua::Result<Vec<String>> {
    match inner.get("argv") {
        Some(Json::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::String(text) => out.push(text.clone()),
                    _ => {
                        return Err(plan_error(format!(
                            "{ctx}: invalid rc entry for section '{section}'"
                        )));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(plan_error(format!(
            "{ctx}: invalid rc entry for section '{section}'"
        ))),
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
