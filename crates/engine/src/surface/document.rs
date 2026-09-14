//! Document
//!
//! Document plus rc entry tables for Lua.

use std::collections::BTreeMap;

use mlua::{Function, Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use super::shell::{check_condition_json, condition_from_json};
use crate::model::{LinkDecl, RcEntryDecl, StructuredDecl, TextDecl};
use crate::values::{
    lua_to_json, plan_error, read_marker, set_marker, table_to_json, take_string_array,
};
use confit_core::document::{PathOp, RcData, RcEntry, RcOp, StructuredFormat};

/// Declared document in registration form.
pub(crate) enum Declared {
    /// Structured declaration.
    Structured(StructuredDecl),
    /// Text declaration.
    Text(TextDecl),
    /// Link declaration.
    Link(LinkDecl),
    /// Rc entries for the shared rc document.
    RcEntries(Vec<RcEntryDecl>),
}

/// Installs the document plus rc namespaces on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set(
        "structured",
        lua.create_function(|lua, args: (Value, Value)| structured_impl(lua, args))?,
    )?;
    namespace.set(
        "text",
        lua.create_function(|lua, args: (Value, Value)| text_impl(lua, args))?,
    )?;
    namespace.set(
        "link",
        lua.create_function(|lua, args: (Value, Value)| link_impl(lua, args))?,
    )?;
    install_rc(lua, &namespace)?;
    confit.set("document", namespace)?;
    Ok(())
}

/// Installs the rc entry constructors on the document namespace.
fn install_rc(lua: &Lua, namespace: &Table) -> mlua::Result<()> {
    let rc = lua.create_table()?;
    rc.set(
        "new",
        lua.create_function(|lua, sections: Value| rc_new_impl(lua, sections))?,
    )?;
    rc.set(
        "alias",
        lua.create_function(|lua, args: (Value, Value, Value)| rc_alias_impl(lua, args))?,
    )?;
    rc.set(
        "env",
        lua.create_function(|lua, args: (Value, Value, Value)| rc_env_impl(lua, args))?,
    )?;
    rc.set(
        "profile",
        lua.create_function(|lua, args: (Value, Value, Value)| rc_profile_impl(lua, args))?,
    )?;
    rc.set(
        "profile_path",
        lua.create_function(|lua, args: (Value, Value)| rc_profile_path_impl(lua, args))?,
    )?;
    rc.set(
        "path_entry",
        lua.create_function(|lua, args: (Value, Value)| rc_path_entry_impl(lua, args))?,
    )?;
    rc.set(
        "eval",
        lua.create_function(|lua, args: (Value, Value)| rc_eval_impl(lua, args))?,
    )?;
    rc.set(
        "cmd",
        lua.create_function(|lua, args: (Value, Value)| rc_cmd_impl(lua, args))?,
    )?;
    rc.set(
        "source",
        lua.create_function(|lua, args: (Value, Value)| rc_source_impl(lua, args))?,
    )?;
    namespace.set("rc", rc)
}

/// Builds a structured document table from format plus args.
fn structured_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.structured";
    const KNOWN: &str = "'json', 'toml', or 'yaml'";
    let (format_value, args_value) = args;
    let format_name = match format_value {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'format' must be one of {KNOWN}"
            )));
        }
    };
    let format = StructuredFormat::parse(&format_name)
        .ok_or_else(|| plan_error(format!("{CTOR}: field 'format' must be one of {KNOWN}")))?;
    let table = match args_value {
        Value::Table(table) => table,
        _ => return Err(plan_error(format!("{CTOR}: field 'args' must be a table"))),
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) if text.to_string_lossy() == "path" => {}
            Value::String(text) if text.to_string_lossy() == "data" => {}
            Value::String(text) => {
                let name = text.to_string_lossy();
                return Err(plan_error(format!(
                    "{CTOR}: field 'args' unknown field '{name}'"
                )));
            }
            _ => {
                return Err(plan_error(format!(
                    "{CTOR}: field 'args' must hold string keys"
                )));
            }
        }
    }
    let out = lua.create_table()?;
    let path: Value = table.get("path")?;
    out.set("path", path)?;
    let data: Value = table.get("data")?;
    out.set("data", data)?;
    set_marker(lua, &out, "structured", Some(("__format", format.name())))?;
    Ok(out)
}

/// Builds a plain text document table.
fn text_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.text";
    let (path, content) = args;
    let path = take_string_value(path, CTOR, "path")?;
    let content = take_string_value(content, CTOR, "content")?;
    let out = lua.create_table()?;
    out.set("path", path)?;
    out.set("content", content)?;
    set_marker(lua, &out, "text", None)?;
    Ok(out)
}

/// Builds a symlink document table.
fn link_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.link";
    let (path, target) = args;
    let path = take_string_value(path, CTOR, "path")?;
    let target = take_string_value(target, CTOR, "target")?;
    let out = lua.create_table()?;
    out.set("path", path)?;
    out.set("target", target)?;
    set_marker(lua, &out, "link", None)?;
    Ok(out)
}

/// Reads a string argument from a Lua value.
fn take_string_value(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(plan_error(format!(
            "{ctor}: field '{field}' must be a string"
        ))),
    }
}

/// Builds the single rc document table from section lists.
fn rc_new_impl(lua: &Lua, sections: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.new";
    let table = match sections {
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'sections' must be a table"
            )));
        }
    };
    let out = lua.create_table()?;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_error(format!(
                    "{CTOR}: field 'sections' must hold section names"
                )));
            }
        };
        if let Err(error) = RcData::check_section_name(&name) {
            return Err(plan_error(error.to_string()));
        }
        out.set(name.as_str(), value)?;
    }
    set_marker(lua, &out, "rc", None)?;
    Ok(out)
}

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
            let path = match table.get::<Value>("path")? {
                Value::String(text) => text.to_string_lossy(),
                _ => return Err(plan_error(format!("{ctx}: field 'path' must be a string"))),
            };
            let data_table = match table.get::<Value>("data")? {
                Value::Table(inner) => inner,
                _ => {
                    return Err(plan_error(format!(
                        "{ctx}: field 'data' must be a table with string keys"
                    )));
                }
            };
            let data_ctx = format!("{ctx}: field 'data'");
            let data_json = table_to_json(&data_table, &data_ctx)?;
            let data = match data_json {
                Json::Object(map) => map.into_iter().collect::<BTreeMap<String, Json>>(),
                _ => {
                    return Err(plan_error(format!(
                        "{ctx}: field 'data' must be a table with string keys"
                    )));
                }
            };
            Ok(Declared::Structured(StructuredDecl { path, format, data }))
        }
        "text" => {
            let path = match table.get::<Value>("path")? {
                Value::String(text) => text.to_string_lossy(),
                _ => return Err(plan_error(format!("{ctx}: field 'path' must be a string"))),
            };
            let content = match table.get::<Value>("content")? {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_error(format!(
                        "{ctx}: field 'content' must be a string"
                    )));
                }
            };
            Ok(Declared::Text(TextDecl { path, content }))
        }
        "link" => {
            let path = match table.get::<Value>("path")? {
                Value::String(text) => text.to_string_lossy(),
                _ => return Err(plan_error(format!("{ctx}: field 'path' must be a string"))),
            };
            let target = match table.get::<Value>("target")? {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_error(format!(
                        "{ctx}: field 'target' must be a string"
                    )));
                }
            };
            Ok(Declared::Link(LinkDecl { path, target }))
        }
        "rc" => {
            let mut entries = Vec::new();
            for section in ["profile", "config", "final"] {
                let list = match table.get::<Value>(section)? {
                    Value::Nil => continue,
                    Value::Table(list) => list,
                    _ => {
                        return Err(plan_error(format!(
                            "{ctx}: field '{section}' must be a list of rc entry tables"
                        )));
                    }
                };
                let len = list.raw_len();
                for index in 1..=len {
                    let item: Value = list.get(index)?;
                    let entry_table = match item {
                        Value::Table(entry) => entry,
                        _ => {
                            return Err(plan_error(format!(
                                "{ctx}: field '{section}' must be a list of rc entry tables"
                            )));
                        }
                    };
                    entries.push(convert_section_entry(
                        &entry_table,
                        section,
                        &format!("{ctx}: field '{section}'"),
                    )?);
                }
            }
            Ok(Declared::RcEntries(entries))
        }
        other => Err(plan_error(format!("{ctx} unknown document kind '{other}'"))),
    }
}

/// Converts one bare rc entry table into registration form.
pub(crate) fn convert_entry(table: &Table, ctx: &str) -> mlua::Result<RcEntryDecl> {
    let json = table_to_json(table, ctx)?;
    let section = default_section(&json, ctx)?.to_string();
    Ok(RcEntryDecl { section, json })
}

/// Converts one section bucket rc entry table into registration form.
fn convert_section_entry(table: &Table, section: &str, ctx: &str) -> mlua::Result<RcEntryDecl> {
    let json = table_to_json(table, ctx)?;
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

/// Reads the default section for one bare entry object.
fn default_section(json: &Json, ctx: &str) -> mlua::Result<&'static str> {
    match op_key(json, ctx)? {
        "env" | "path" => Ok("profile"),
        "alias" => Ok("config"),
        _ => Ok("final"),
    }
}

/// Derives the slot key plus display name for one entry.
pub(crate) fn entry_slot(
    json: &Json,
    section: &str,
) -> mlua::Result<Option<(String, String, &'static str)>> {
    let ctx = format!("invalid rc entry for section '{section}'");
    let object = match json {
        Json::Object(map) => map,
        _ => return Err(plan_error(ctx)),
    };
    let when_text = match object.get("when") {
        Some(when) => crate::values::json_text(when)?,
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
                "invalid rc entry for section '{section}'"
            )));
        }
    };
    let when = match object.get("when") {
        None | Some(Json::Null) => None,
        Some(raw) => Some(condition_from_json(raw, &format!("{ctx}: field 'when'"))?),
    };
    let key = op_key(json, &format!("invalid rc entry for section '{section}'"))?;
    let op = match key {
        "env" => {
            let inner = entry_object(object, key, section)?;
            check_fields(inner, &["name", "value"], section)?;
            RcOp::Env {
                name: entry_string(inner, "name", section)?,
                value: entry_string(inner, "value", section)?,
            }
        }
        "path" => {
            let inner = entry_object(object, key, section)?;
            check_fields(inner, &["name", "dir", "op"], section)?;
            RcOp::Path {
                name: entry_string(inner, "name", section)?,
                dir: entry_string(inner, "dir", section)?,
                op: match inner.get("op").and_then(Json::as_str) {
                    Some("prepend") => PathOp::Prepend,
                    Some("append") => PathOp::Append,
                    _ => {
                        return Err(plan_error(format!(
                            "invalid rc entry for section '{section}'"
                        )));
                    }
                },
            }
        }
        "alias" => {
            let inner = entry_object(object, key, section)?;
            check_fields(inner, &["name", "expansion"], section)?;
            RcOp::Alias {
                name: entry_string(inner, "name", section)?,
                expansion: entry_string(inner, "expansion", section)?,
            }
        }
        "eval" | "cmd" => {
            let inner = entry_object(object, key, section)?;
            check_fields(inner, &["argv"], section)?;
            let argv = entry_argv(inner, section)?;
            if key == "eval" {
                RcOp::Eval { argv }
            } else {
                RcOp::Cmd { argv }
            }
        }
        _ => {
            let inner = entry_object(object, key, section)?;
            check_fields(inner, &["path"], section)?;
            RcOp::Source {
                path: entry_string(inner, "path", section)?,
            }
        }
    };
    check_fields(object, &[key, "when"], section)?;
    let entry = RcEntry { op, when };
    match section {
        "profile" => profile.push(entry),
        "config" => config.push(entry),
        "final" => finals.push(entry),
        _ => {
            return Err(plan_error(format!(
                "invalid rc entry for section '{section}'"
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
) -> mlua::Result<&'a serde_json::Map<String, Json>> {
    match object.get(key) {
        Some(Json::Object(map)) => Ok(map),
        _ => Err(plan_error(format!(
            "invalid rc entry for section '{section}'"
        ))),
    }
}

/// Reads one argv array from an exec inner object.
fn entry_argv(inner: &serde_json::Map<String, Json>, section: &str) -> mlua::Result<Vec<String>> {
    match inner.get("argv") {
        Some(Json::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::String(text) => out.push(text.clone()),
                    _ => {
                        return Err(plan_error(format!(
                            "invalid rc entry for section '{section}'"
                        )));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(plan_error(format!(
            "invalid rc entry for section '{section}'"
        ))),
    }
}

/// Reads one required string field from an entry object.
fn entry_string(
    object: &serde_json::Map<String, Json>,
    field: &str,
    section: &str,
) -> mlua::Result<String> {
    match object.get(field).and_then(Json::as_str) {
        Some(value) => Ok(value.to_string()),
        None => Err(plan_error(format!(
            "invalid rc entry for section '{section}'"
        ))),
    }
}

/// Rejects unknown keys on an entry object.
fn check_fields(
    object: &serde_json::Map<String, Json>,
    known: &[&str],
    section: &str,
) -> mlua::Result<()> {
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            return Err(plan_error(format!(
                "invalid rc entry for section '{section}'"
            )));
        }
    }
    Ok(())
}

/// Builds one rc alias entry table.
fn rc_alias_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.alias";
    let (name, value, opts) = args;
    let name = take_string_value(name, CTOR, "name")?;
    let value = take_string_value(value, CTOR, "value")?;
    let when = resolve_guard(lua, CTOR, opts)?;
    let inner = lua.create_table()?;
    inner.set("name", name)?;
    inner.set("expansion", value)?;
    tagged_entry(lua, "alias", inner, when)
}

/// Builds one rc env entry table.
fn rc_env_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.env";
    let (name, value, opts) = args;
    let name = take_string_value(name, CTOR, "name")?;
    let value = take_string_value(value, CTOR, "value")?;
    let when = resolve_guard(lua, CTOR, opts)?;
    let inner = lua.create_table()?;
    inner.set("name", name)?;
    inner.set("value", value)?;
    tagged_entry(lua, "env", inner, when)
}

/// Builds one rc profile entry table.
fn rc_profile_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.profile";
    let (name, value, opts) = args;
    let name = take_string_value(name, CTOR, "name")?;
    let value = take_string_value(value, CTOR, "value")?;
    let when = resolve_guard(lua, CTOR, opts)?;
    path_table(lua, name, value, when)
}

/// Builds one rc profile path entry table.
fn rc_profile_path_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.profile_path";
    let (dir, opts) = args;
    let dir = take_string_value(dir, CTOR, "dir")?;
    let when = resolve_guard(lua, CTOR, opts)?;
    path_table(lua, "PATH".to_string(), dir, when)
}

/// Builds one rc path entry table as PATH prepend sugar.
fn rc_path_entry_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.path_entry";
    let (dir, opts) = args;
    let dir = take_string_value(dir, CTOR, "dir")?;
    let when = resolve_guard(lua, CTOR, opts)?;
    path_table(lua, "PATH".to_string(), dir, when)
}

/// Builds one shared PATH prepend entry table.
fn path_table(lua: &Lua, name: String, dir: String, when: Option<Table>) -> mlua::Result<Table> {
    let inner = lua.create_table()?;
    inner.set("name", name)?;
    inner.set("dir", dir)?;
    inner.set("op", "prepend")?;
    tagged_entry(lua, "path", inner, when)
}

/// Wraps one op inner table plus guard into an entry table.
fn tagged_entry(lua: &Lua, shape: &str, inner: Table, when: Option<Table>) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    out.set(shape, inner)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", None)?;
    Ok(out)
}

/// Resolves the `when` guard from opts.
fn resolve_guard(lua: &Lua, ctor: &str, opts: Value) -> mlua::Result<Option<Table>> {
    resolve_opts(lua, ctor, opts)
}

/// Builds one rc eval init entry table.
fn rc_eval_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.eval";
    let (argv_value, opts) = args;
    let argv = take_argv(argv_value, CTOR)?;
    let when = resolve_opts(lua, CTOR, opts)?;
    init_table(lua, "eval", argv, None, when)
}

/// Builds one rc cmd init entry table.
fn rc_cmd_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.cmd";
    let (argv_value, opts) = args;
    let argv = take_argv(argv_value, CTOR)?;
    let when = resolve_opts(lua, CTOR, opts)?;
    init_table(lua, "cmd", argv, None, when)
}

/// Builds one rc source init entry table.
fn rc_source_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.source";
    let (path_value, opts) = args;
    let path = take_string_value(path_value, CTOR, "path")?;
    let when = resolve_opts(lua, CTOR, opts)?;
    init_table(lua, "source", Vec::new(), Some(path), when)
}

/// Builds one shared init entry table.
fn init_table(
    lua: &Lua,
    shape: &str,
    argv: Vec<String>,
    source: Option<String>,
    when: Option<Table>,
) -> mlua::Result<Table> {
    let inner = lua.create_table()?;
    if shape == "source" {
        inner.set("path", source.unwrap_or_default())?;
    } else {
        let argv_table = lua.create_table()?;
        for (position, item) in argv.iter().enumerate() {
            argv_table.set((position + 1) as i64, item.as_str())?;
        }
        inner.set("argv", argv_table)?;
    }
    let out = lua.create_table()?;
    out.set(shape, inner)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", None)?;
    Ok(out)
}

/// Reads one dense string argv array from a Lua value.
fn take_argv(value: Value, ctor: &str) -> mlua::Result<Vec<String>> {
    let table = match value {
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(format!(
                "{ctor}: field 'argv' must be a dense string array starting at 1"
            )));
        }
    };
    take_string_array(&table, ctor, "argv")
}

/// Resolves the `when` guard from an opts value.
fn resolve_opts(lua: &Lua, ctor: &str, opts: Value) -> mlua::Result<Option<Table>> {
    let table = match opts {
        Value::Nil => return Ok(None),
        Value::Table(table) => table,
        _ => return Err(plan_error(format!("{ctor}: field 'opts' must be a table"))),
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) if text.to_string_lossy() == "when" => {}
            Value::String(text) => {
                let name = text.to_string_lossy();
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' unknown field '{name}'"
                )));
            }
            _ => {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' must hold string keys"
                )));
            }
        }
    }
    let when_value: Value = table.get("when")?;
    if let Value::Function(func) = when_value {
        let resolved = call_when_function(lua, ctor, &func)?;
        table.set("when", resolved)?;
    }
    let guard_value: Value = table.get("when")?;
    let guard = match guard_value {
        Value::Nil => None,
        Value::Table(guard) => {
            let json = lua_to_json(
                Value::Table(guard.clone()),
                &format!("{ctor}: field 'when'"),
            )
            .map_err(|error| plan_error(format!("{ctor}: field 'when' {error}")))?;
            check_condition_json(&json, &format!("{ctor}: field 'when'"))
                .map_err(|detail| plan_error(format!("{ctor}: field 'when' {detail}")))?;
            Some(guard)
        }
        _ => {
            return Err(plan_error(format!(
                "{ctor}: field 'when' must be a condition table"
            )));
        }
    };
    Ok(guard)
}

/// Calls one `when` builder function with the shell namespace.
fn call_when_function(lua: &Lua, ctor: &str, func: &Function) -> mlua::Result<Table> {
    let shell = match lua.globals().get::<Value>("confit")? {
        Value::Table(confit) => match confit.get::<Value>("shell")? {
            Value::Table(shell) => shell,
            _ => {
                return Err(plan_error(format!(
                    "{ctor}: field 'when' needs the confit.shell table"
                )));
            }
        },
        _ => {
            return Err(plan_error(format!(
                "{ctor}: field 'when' needs the confit.shell table"
            )));
        }
    };
    match func.call::<Table>(shell) {
        Ok(table) => Ok(table),
        Err(error) => {
            if crate::values::find_plan(&error).is_some() {
                return Err(error);
            }
            Err(plan_error(format!("{ctor}: field 'when' failed: {error}")))
        }
    }
}
