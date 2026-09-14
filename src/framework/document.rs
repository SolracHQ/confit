//! Document
//!
//! Plain table constructors for Lua documents plus rc entries.
//! Structured plus text plus link plus rc shapes carry `__kind`
//! metatables. Entry tables carry `__kind` plus `__area`.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::error::Error;
use crate::model::state::condition::Condition;
use crate::model::state::document::Document;
use crate::model::state::document::DocumentData;
use crate::model::state::document::DocumentKind;
use crate::model::state::document::StructuredFormat;
use crate::model::state::rc::Lane;

use super::confit_table;

/// Entry builder opts in Lua shape.
///
/// `when` holds a condition table or a builder function result after
/// preprocessing; `lane` holds a lane name. Unknown keys fail as plan
/// errors through `deny_unknown_fields`, naming the key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryOpts {
    /// Holds the guard condition value, empty for unconditional entries.
    #[serde(default)]
    when: Option<Json>,
    /// Holds the lane name, empty for the default middle lane.
    #[serde(default)]
    lane: Option<String>,
}

/// Structured constructor args in Lua shape.
///
/// Unknown keys fail as plan errors through `deny_unknown_fields`,
/// naming the key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredArgs {
    /// Holds the destination path value.
    #[serde(default)]
    path: Option<Json>,
    /// Holds the structured data value.
    #[serde(default)]
    data: Option<Json>,
}

/// Rc sections wire shape for conversion.
///
/// Unknown keys fail as plan errors through `deny_unknown_fields`,
/// naming the key.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RcSectionsWire {
    /// Holds profile entries in listed order.
    #[serde(default)]
    profile: Vec<Json>,
    /// Holds config entries in listed order.
    #[serde(default)]
    config: Vec<Json>,
    /// Holds final entries in listed order.
    #[serde(rename = "final", default)]
    final_list: Vec<Json>,
}

/// Reads the `__kind` marker from a table metatable.
///
/// # Arguments
///
/// * `table` - table under inspection.
///
/// # Returns
///
/// Marker string, empty while absent.
pub(crate) fn table_kind(table: &Table) -> Option<String> {
    let meta = table.metatable()?;
    let value: Value = meta.get("__kind").ok()?;
    match value {
        Value::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// Reads the `__area` marker from a table metatable.
///
/// # Arguments
///
/// * `table` - table under inspection.
///
/// # Returns
///
/// Area string, empty while absent.
pub(crate) fn table_area(table: &Table) -> Option<String> {
    let meta = table.metatable()?;
    let value: Value = meta.get("__area").ok()?;
    match value {
        Value::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// Reads the `__format` marker from a table metatable.
///
/// # Arguments
///
/// * `table` - table under inspection.
///
/// # Returns
///
/// Format string, empty while absent.
pub(crate) fn table_format(table: &Table) -> Option<String> {
    let meta = table.metatable()?;
    let value: Value = meta.get("__format").ok()?;
    match value {
        Value::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// Sets a `__kind` metatable with one optional extra marker.
///
/// # Arguments
///
/// * `lua` - state owning the metatable.
/// * `table` - table receiving the marker.
/// * `kind` - marker value for `__kind`.
/// * `extra` - optional extra key plus value.
///
/// # Errors
///
/// Fails with mlua errors for metatable creation failures.
fn set_marker(
    lua: &Lua,
    table: &Table,
    kind: &str,
    extra: Option<(&str, &str)>,
) -> mlua::Result<()> {
    let meta = lua.create_table()?;
    meta.set("__kind", kind)?;
    if let Some((key, value)) = extra {
        meta.set(key, value)?;
    }
    table.set_metatable(Some(meta))?;
    Ok(())
}

/// Installs the document namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
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

/// Builds a structured document table from format plus args.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `args` - format value plus args-table value.
///
/// # Returns
///
/// Plain table holding path plus data with a structured marker.
///
/// # Errors
///
/// Fails with plan errors for formats outside `json`, `toml`, `yaml`,
/// for args holding values of other shapes, for unknown args keys, and
/// for paths or data holding values of other shapes.
fn structured_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.structured";
    let (format_value, args_value) = args;
    let format_name = match format_value {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(rc_field_error(
                CTOR,
                "format",
                "must be one of 'json', 'toml', or 'yaml'",
            ));
        }
    };
    let format = parse_format(&format_name, CTOR)?;
    let table = match args_value {
        Value::Table(table) => table,
        _ => {
            return Err(rc_field_error(CTOR, "args", "must be a table"));
        }
    };
    let ctx = format!("{CTOR}: field 'args'");
    let json = table_to_json(&table, &ctx)?;
    let parsed: StructuredArgs = serde_json::from_value(json)
        .map_err(|err| rc_field_error(CTOR, "args", &format!("{err}")))?;
    if parsed.data.is_none() {
        return Err(rc_field_error(
            CTOR,
            "data",
            "must be a table with string keys",
        ));
    }
    let path = match parsed.path {
        Some(Json::String(path)) => path,
        _ => {
            return Err(rc_field_error(CTOR, "path", "must be a string"));
        }
    };
    let data_value: Value = table.get("data")?;
    let data_table = match data_value {
        Value::Table(inner) => inner,
        _ => {
            return Err(rc_field_error(
                CTOR,
                "data",
                "must be a table with string keys",
            ));
        }
    };
    let data_ctx = format!("{CTOR}: field 'data'");
    let data_json = table_to_json(&data_table, &data_ctx)?;
    match data_json {
        Json::Object(_) => {}
        _ => {
            return Err(rc_field_error(
                CTOR,
                "data",
                "must be a table with string keys",
            ));
        }
    };
    let out = lua.create_table()?;
    out.set("path", path)?;
    out.set("data", data_table)?;
    set_marker(lua, &out, "structured", Some(("__format", format.name())))?;
    Ok(out)
}

/// Parses a structured format name.
///
/// # Arguments
///
/// * `name` - the raw format name.
/// * `ctor` - constructor name for errors.
///
/// # Returns
///
/// The structured format.
///
/// # Errors
///
/// Fails with plan errors for names outside the three formats.
fn parse_format(name: &str, ctor: &str) -> mlua::Result<StructuredFormat> {
    match name {
        "json" => Ok(StructuredFormat::Json),
        "toml" => Ok(StructuredFormat::Toml),
        "yaml" => Ok(StructuredFormat::Yaml),
        _ => Err(rc_field_error(
            ctor,
            "format",
            "must be one of 'json', 'toml', or 'yaml'",
        )),
    }
}

/// Builds a plain text document table from path plus content.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `args` - path value plus content value.
///
/// # Returns
///
/// Plain table holding path plus content with a text marker.
///
/// # Errors
///
/// Fails with Lua errors for arguments holding values of other shapes.
fn text_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.text";
    let (path, content) = args;
    let path = take_string(path, CTOR, "path")?;
    let content = take_string(content, CTOR, "content")?;
    let out = lua.create_table()?;
    out.set("path", path)?;
    out.set("content", content)?;
    set_marker(lua, &out, "text", None)?;
    Ok(out)
}

/// Builds a symlink document table from path plus target.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `args` - path value plus target value.
///
/// # Returns
///
/// Plain table holding path plus target with a link marker.
///
/// # Errors
///
/// Fails with Lua errors for arguments holding values of other shapes.
fn link_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.link";
    let (path, target) = args;
    let path = take_string(path, CTOR, "path")?;
    let target = take_string(target, CTOR, "target")?;
    let out = lua.create_table()?;
    out.set("path", path)?;
    out.set("target", target)?;
    set_marker(lua, &out, "link", None)?;
    Ok(out)
}

/// Builds a field error naming constructor plus field.
///
/// # Arguments
///
/// * `ctor` - constructor name.
/// * `field` - Lua field.
/// * `detail` - violated expectation.
///
/// # Returns
///
/// Lua domain error naming constructor plus field.
fn field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    mlua::Error::external(Error::Lua(format!("{ctor}: field '{field}' {detail}")))
}

/// Reads a string argument from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua value.
/// * `ctor` - constructor name for errors.
/// * `field` - argument name for errors.
///
/// # Returns
///
/// Argument string.
///
/// # Errors
///
/// Fails with Lua errors for values of other shapes.
fn take_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(ctor, field, "must be a string")),
    }
}

/// Converts a Lua value into JSON.
///
/// # Arguments
///
/// * `value` - Lua value.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON holding value data.
///
/// # Errors
///
/// Fails with Lua errors for values carrying executable shapes.
fn lua_to_json(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(lua_err!("{ctx} must be a finite number")),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json(&table, ctx),
        Value::Function(_) => Err(lua_err!("{ctx} must be data-only (function not allowed)")),
        Value::UserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::LightUserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::Thread(_) => Err(lua_err!("{ctx} must be data-only (thread not allowed)")),
        Value::Error(_) => Err(lua_err!("{ctx} must be data-only")),
        Value::Other(_) => Err(lua_err!("{ctx} must be data-only")),
    }
}

/// Converts a Lua table into a JSON value.
///
/// Shared with the shell leaf opts parsing, so one conversion feeds
/// every serde opts struct.
///
/// # Arguments
///
/// * `table` - Lua table.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON array for dense arrays, else JSON object.
///
/// # Errors
///
/// Fails with Lua errors for keys of other shapes and for values carrying executable shapes.
pub(crate) fn table_to_json(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if entries.is_empty() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    let mut all_integer = true;
    for (key, value) in &entries {
        match key {
            Value::Integer(index) => indexed.push((*index, value.clone())),
            _ => {
                all_integer = false;
                break;
            }
        }
    }
    if all_integer {
        indexed.sort_by_key(|(index, _)| *index);
        let dense = indexed
            .iter()
            .enumerate()
            .all(|(position, (index, _))| *index == position as i64 + 1);
        if dense {
            let mut items = Vec::with_capacity(indexed.len());
            for (index, value) in &indexed {
                let child = format!("{ctx}[{index}]");
                items.push(lua_to_json(value.clone(), &child)?);
            }
            return Ok(Json::Array(items));
        }
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(lua_err!("{ctx} must be a table with string keys"));
            }
        };
        let child = format!("{ctx}.{name}");
        map.insert(name, lua_to_json(value.clone(), &child)?);
    }
    Ok(Json::Object(map))
}

/// Installs the rc entry constructors on the document namespace.
///
/// # Arguments
///
/// * `lua` - state owning the callbacks.
/// * `namespace` - document table receiving the `rc` subtable.
///
/// # Errors
///
/// Fails with mlua errors for table creation plus registration failures.
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

/// Builds a plan error naming constructor plus field.
fn rc_field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    mlua::Error::external(Error::Plan(format!("{ctor}: field '{field}' {detail}")))
}

/// Reads a string from a Lua value as a plan error.
fn take_rc_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(rc_field_error(ctor, field, "must be a string")),
    }
}

/// Builds the single rc document table from section lists.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `sections` - raw sections value holding `profile`, `config`, `final` lists.
///
/// # Returns
///
/// Plain table holding section lists with an rc marker.
///
/// # Errors
///
/// Fails with plan errors for sections holding values of other shapes.
fn rc_new_impl(lua: &Lua, sections: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.new";
    let table = match sections {
        Value::Table(table) => table,
        _ => return Err(rc_field_error(CTOR, "sections", "must be a table")),
    };
    let out = lua.create_table()?;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        out.set(key, value)?;
    }
    set_marker(lua, &out, "rc", None)?;
    Ok(out)
}

/// Converts a document table into a model document.
///
/// # Arguments
///
/// * `table` - declared document table.
/// * `ctx` - error context naming the caller.
///
/// # Returns
///
/// Model document holding kind plus path plus data.
///
/// # Errors
///
/// Fails with plan errors for unknown kinds plus bad shapes.
pub(crate) fn document_table_to_document(table: &Table, ctx: &str) -> mlua::Result<Document> {
    let kind = match table_kind(table) {
        Some(kind) => kind,
        None => {
            return Err(plan_err!(
                "{ctx} must be a confit.document value (missing '__kind')"
            ));
        }
    };
    match kind.as_str() {
        "structured" => {
            let format_name = match table_format(table) {
                Some(name) => name,
                None => {
                    return Err(plan_err!("{ctx} structured document misses '__format'"));
                }
            };
            let format = match format_name.as_str() {
                "json" => StructuredFormat::Json,
                "toml" => StructuredFormat::Toml,
                "yaml" => StructuredFormat::Yaml,
                _ => {
                    return Err(plan_err!(
                        "{ctx}: field 'format' must be one of 'json', 'toml', or 'yaml'"
                    ));
                }
            };
            let path_value: Value = table.get("path")?;
            let path = match path_value {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_err!("{ctx}: field 'path' must be a string"));
                }
            };
            let data_value: Value = table.get("data")?;
            let data_table = match data_value {
                Value::Table(inner) => inner,
                _ => {
                    return Err(plan_err!(
                        "{ctx}: field 'data' must be a table with string keys"
                    ));
                }
            };
            let data_ctx = format!("{ctx}: field 'data'");
            let data_json = table_to_json(&data_table, &data_ctx).map_err(|err| {
                let message = err.to_string();
                plan_err!("{ctx}: field 'data' {message}")
            })?;
            let data = match data_json {
                Json::Object(map) => map.into_iter().collect::<BTreeMap<String, Json>>(),
                _ => {
                    return Err(plan_err!(
                        "{ctx}: field 'data' must be a table with string keys"
                    ));
                }
            };
            Ok(Document {
                kind: DocumentKind::Structured,
                path,
                data: DocumentData::Structured { format, data },
                data_hash: String::new(),
            })
        }
        "text" => {
            let path_value: Value = table.get("path")?;
            let path = match path_value {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_err!("{ctx}: field 'path' must be a string"));
                }
            };
            let content_value: Value = table.get("content")?;
            let content = match content_value {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_err!("{ctx}: field 'content' must be a string"));
                }
            };
            Ok(Document {
                kind: DocumentKind::Text,
                path,
                data: DocumentData::Text { content },
                data_hash: String::new(),
            })
        }
        "link" => {
            let path_value: Value = table.get("path")?;
            let path = match path_value {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_err!("{ctx}: field 'path' must be a string"));
                }
            };
            let target_value: Value = table.get("target")?;
            let target = match target_value {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_err!("{ctx}: field 'target' must be a string"));
                }
            };
            Ok(Document {
                kind: DocumentKind::Link,
                path,
                data: DocumentData::Link { target },
                data_hash: String::new(),
            })
        }
        "rc" => {
            let data = rc_table_to_data(table, ctx)?;
            Ok(Document {
                kind: DocumentKind::Rc,
                path: "rc".to_string(),
                data: DocumentData::Rc(data),
                data_hash: String::new(),
            })
        }
        other => Err(plan_err!("{ctx} unknown document kind '{other}'")),
    }
}

/// Converts an rc document table into rc data.
///
/// Sections convert through `deny_unknown_fields`, so unknown section
/// keys fail as plan errors naming the key.
///
/// # Arguments
///
/// * `table` - rc document table.
/// * `ctx` - error context naming the caller.
///
/// # Returns
///
/// Rc data holding section entries.
///
/// # Errors
///
/// Fails with plan errors for unknown sections plus bad entry shapes.
pub(crate) fn rc_table_to_data(
    table: &Table,
    ctx: &str,
) -> mlua::Result<crate::model::state::rc::RcData> {
    for section in ["profile", "config", "final"] {
        let value: Value = table.get(section)?;
        match value {
            Value::Nil => {}
            Value::Table(_) => {}
            _ => {
                return Err(plan_err!(
                    "{ctx}: field '{section}' must be a list of rc entry tables"
                ));
            }
        }
    }
    let json = table_to_json(table, ctx).map_err(|err| {
        let message = err.to_string();
        plan_err!("{ctx}: {message}")
    })?;
    let wire: RcSectionsWire =
        serde_json::from_value(json).map_err(|err| plan_err!("{ctx}: {err}"))?;
    let mut data = crate::model::state::rc::RcData::default();
    for mut entry_json in wire.config {
        stamp_normal_priority(&mut entry_json);
        let entry: crate::model::state::rc::AliasEntry = serde_json::from_value(entry_json)
            .map_err(|err| plan_err!("{ctx}: invalid rc entry for section 'config': {err}"))?;
        data.aliases.push(entry);
    }
    for mut entry_json in wire.profile {
        stamp_normal_priority(&mut entry_json);
        if entry_json.get("op").is_some() {
            let entry: crate::model::state::rc::ProfileEntry = serde_json::from_value(entry_json)
                .map_err(|err| {
                plan_err!("{ctx}: invalid rc entry for section 'profile': {err}")
            })?;
            data.profile.push(entry);
        } else {
            let entry: crate::model::state::rc::EnvEntry = serde_json::from_value(entry_json)
                .map_err(|err| plan_err!("{ctx}: invalid rc entry for section 'profile': {err}"))?;
            data.env.push(entry);
        }
    }
    for mut entry_json in wire.final_list {
        stamp_normal_priority(&mut entry_json);
        let entry: crate::model::state::rc::InitEntry = serde_json::from_value(entry_json)
            .map_err(|err| plan_err!("{ctx}: invalid rc entry for section 'final': {err}"))?;
        data.init.push(entry);
    }
    Ok(data)
}

/// Stamps normal priority into an entry JSON while absent.
///
/// # Arguments
///
/// * `entry` - entry JSON under stamping, mutated in place.
fn stamp_normal_priority(entry: &mut Json) {
    if let Some(object) = entry.as_object_mut()
        && !object.contains_key("priority")
    {
        object.insert("priority".to_string(), Json::String("normal".to_string()));
    }
}

/// Validates entry JSON for one area plus returns stamped JSON.
///
/// # Arguments
///
/// * `area` - entry area from the metatable.
/// * `json` - entry JSON under validation plus stamping.
/// * `ctx` - error context naming the caller.
///
/// # Returns
///
/// Stamped entry JSON.
///
/// # Errors
///
/// Fails with plan errors for shape mismatches plus unknown areas.
fn entry_json_for_area(area: &str, mut json: Json, ctx: &str) -> mlua::Result<Json> {
    stamp_normal_priority(&mut json);
    match area {
        "alias" => {
            let _: crate::model::state::rc::AliasEntry = serde_json::from_value(json.clone())
                .map_err(|err| plan_err!("{ctx}: invalid alias entry: {err}"))?;
            Ok(json)
        }
        "env" => {
            let _: crate::model::state::rc::EnvEntry = serde_json::from_value(json.clone())
                .map_err(|err| plan_err!("{ctx}: invalid env entry: {err}"))?;
            Ok(json)
        }
        "profile" => {
            let _: crate::model::state::rc::ProfileEntry = serde_json::from_value(json.clone())
                .map_err(|err| plan_err!("{ctx}: invalid profile entry: {err}"))?;
            Ok(json)
        }
        "init" => {
            let _: crate::model::state::rc::InitEntry = serde_json::from_value(json.clone())
                .map_err(|err| plan_err!("{ctx}: invalid init entry: {err}"))?;
            Ok(json)
        }
        other => Err(plan_err!("{ctx} unknown entry area '{other}'")),
    }
}

/// Pushes one rc entry table into the per-config rc document.
///
/// Creates the rc document while absent, then pushes the entry into
/// the matching RcData lists. Routing follows the `__area` metatable.
///
/// # Arguments
///
/// * `documents` - declared documents under extension, mutated in place.
/// * `table` - entry table under conversion.
/// * `ctx` - error context naming the caller.
///
/// # Errors
///
/// Fails with plan errors for missing areas plus bad entry shapes.
pub(crate) fn push_entry_table(
    documents: &mut Vec<Document>,
    table: &Table,
    ctx: &str,
) -> mlua::Result<()> {
    let area = match table_area(table) {
        Some(area) => area,
        None => {
            return Err(plan_err!("{ctx} entry misses '__area'"));
        }
    };
    let json = table_to_json(table, ctx).map_err(|err| {
        let message = err.to_string();
        plan_err!("{ctx}: {message}")
    })?;
    let json = entry_json_for_area(&area, json, ctx)?;
    let position = documents
        .iter()
        .position(|item| matches!(item.kind, DocumentKind::Rc) && item.path == "rc");
    let index = match position {
        Some(index) => index,
        None => {
            documents.push(Document {
                kind: DocumentKind::Rc,
                path: "rc".to_string(),
                data: DocumentData::Rc(crate::model::state::rc::RcData::default()),
                data_hash: String::new(),
            });
            documents.len() - 1
        }
    };
    if let DocumentData::Rc(data) = &mut documents[index].data {
        match area.as_str() {
            "alias" => {
                let entry: crate::model::state::rc::AliasEntry = serde_json::from_value(json)
                    .map_err(|err| plan_err!("{ctx}: invalid alias entry: {err}"))?;
                data.aliases.push(entry);
            }
            "env" => {
                let entry: crate::model::state::rc::EnvEntry = serde_json::from_value(json)
                    .map_err(|err| plan_err!("{ctx}: invalid env entry: {err}"))?;
                data.env.push(entry);
            }
            "profile" => {
                let entry: crate::model::state::rc::ProfileEntry = serde_json::from_value(json)
                    .map_err(|err| plan_err!("{ctx}: invalid profile entry: {err}"))?;
                data.profile.push(entry);
            }
            "init" => {
                let entry: crate::model::state::rc::InitEntry = serde_json::from_value(json)
                    .map_err(|err| plan_err!("{ctx}: invalid init entry: {err}"))?;
                data.init.push(entry);
            }
            other => {
                return Err(plan_err!("{ctx} unknown entry area '{other}'"));
            }
        }
    }
    Ok(())
}

/// Converts one rc entry table into area plus stamped JSON.
///
/// # Arguments
///
/// * `table` - entry table under conversion.
/// * `ctx` - error context naming the caller.
///
/// # Returns
///
/// Area plus stamped entry JSON.
///
/// # Errors
///
/// Fails with plan errors for missing areas plus bad entry shapes.
pub(crate) fn entry_table_to_json(table: &Table, ctx: &str) -> mlua::Result<(String, Json)> {
    let area = match table_area(table) {
        Some(area) => area,
        None => {
            return Err(plan_err!("{ctx} entry misses '__area'"));
        }
    };
    let kind = table_kind(table);
    if kind.as_deref() != Some("rc-entry") {
        return Err(plan_err!("{ctx} must be an rc entry table"));
    }
    let json = table_to_json(table, ctx).map_err(|err| {
        let message = err.to_string();
        plan_err!("{ctx}: {message}")
    })?;
    let json = entry_json_for_area(&area, json, ctx)?;
    Ok((area, json))
}

/// Builds one rc alias entry table.
fn rc_alias_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.alias";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, _) = rc_opts_table(lua, CTOR, opts)?;
    let out = lua.create_table()?;
    out.set("name", name)?;
    out.set("value", value)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "alias")))?;
    Ok(out)
}

/// Builds one rc env entry table.
fn rc_env_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.env";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, _) = rc_opts_table(lua, CTOR, opts)?;
    let out = lua.create_table()?;
    out.set("name", name)?;
    out.set("value", value)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "env")))?;
    Ok(out)
}

/// Builds one rc profile entry table.
fn rc_profile_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.profile";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, _) = rc_opts_table(lua, CTOR, opts)?;
    let out = lua.create_table()?;
    out.set("name", name)?;
    out.set("value", value)?;
    out.set("op", "prepend")?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "profile")))?;
    Ok(out)
}

/// Builds one rc profile_path entry table.
fn rc_profile_path_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.profile_path";
    let (dir, opts) = args;
    let dir = take_rc_string(dir, CTOR, "dir")?;
    let (when, _) = rc_opts_table(lua, CTOR, opts)?;
    let out = lua.create_table()?;
    out.set("name", "PATH")?;
    out.set("value", dir)?;
    out.set("op", "prepend")?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "profile")))?;
    Ok(out)
}

/// Builds one rc path_entry table as PATH prepend sugar.
fn rc_path_entry_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.path_entry";
    let (dir, opts) = args;
    let dir = take_rc_string(dir, CTOR, "dir")?;
    let (when, _) = rc_opts_table(lua, CTOR, opts)?;
    let out = lua.create_table()?;
    out.set("name", "PATH")?;
    out.set("value", dir)?;
    out.set("op", "prepend")?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "profile")))?;
    Ok(out)
}

/// Builds one rc eval init entry table.
fn rc_eval_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.eval";
    let (argv_value, opts) = args;
    let argv = take_argv(argv_value, CTOR, "argv")?;
    let (when, lane) = rc_opts_table(lua, CTOR, opts)?;
    let argv_table = lua.create_table()?;
    for (position, item) in argv.iter().enumerate() {
        argv_table.set((position + 1) as i64, item.as_str())?;
    }
    let inner = lua.create_table()?;
    inner.set("argv", argv_table)?;
    if !lane.is_middle() {
        inner.set("lane", lane_name(lane))?;
    }
    let out = lua.create_table()?;
    out.set("eval", inner)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "init")))?;
    Ok(out)
}

/// Builds one rc cmd init entry table.
fn rc_cmd_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.cmd";
    let (argv_value, opts) = args;
    let argv = take_argv(argv_value, CTOR, "argv")?;
    let (when, lane) = rc_opts_table(lua, CTOR, opts)?;
    let argv_table = lua.create_table()?;
    for (position, item) in argv.iter().enumerate() {
        argv_table.set((position + 1) as i64, item.as_str())?;
    }
    let inner = lua.create_table()?;
    inner.set("argv", argv_table)?;
    if !lane.is_middle() {
        inner.set("lane", lane_name(lane))?;
    }
    let out = lua.create_table()?;
    out.set("cmd", inner)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "init")))?;
    Ok(out)
}

/// Builds one rc source init entry table.
fn rc_source_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.source";
    let (path_value, opts) = args;
    let path = take_rc_string(path_value, CTOR, "path")?;
    let (when, lane) = rc_opts_table(lua, CTOR, opts)?;
    let inner = lua.create_table()?;
    inner.set("path", path)?;
    if !lane.is_middle() {
        inner.set("lane", lane_name(lane))?;
    }
    let out = lua.create_table()?;
    out.set("source", inner)?;
    if let Some(guard) = when {
        out.set("when", guard)?;
    }
    set_marker(lua, &out, "rc-entry", Some(("__area", "init")))?;
    Ok(out)
}

/// Names the lane string for init tables.
///
/// # Arguments
///
/// * `lane` - lane under naming.
///
/// # Returns
///
/// Lowercase lane name.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::First => "first",
        Lane::Middle => "middle",
        Lane::Last => "last",
    }
}

/// Reads a dense string argv array from a Lua value.
///
/// Bare argv arrays stay hand parsed: dense-array errors name the index,
/// a shape serde cannot render.
///
/// # Arguments
///
/// * `value` - raw argv value.
/// * `ctor` - constructor name for errors.
/// * `field` - argument name for errors.
///
/// # Returns
///
/// Argument strings in order.
///
/// # Errors
///
/// Fails with plan errors for values of other shapes and for sparse or
/// non-string items.
fn take_argv(value: Value, ctor: &str, field: &str) -> mlua::Result<Vec<String>> {
    let table = match value {
        Value::Table(table) => table,
        _ => {
            return Err(rc_field_error(
                ctor,
                field,
                "must be a dense string array starting at 1",
            ));
        }
    };
    take_model_string_array(&table, ctor, field)
}

/// Resolves the optional `when` guard plus `lane` from an opts value.
///
/// Opts parse through `EntryOpts` with denied unknown fields fed by
/// `table_to_json`, so unknown keys fail as plan errors naming the key.
/// Function guards run first against the shell namespace, then the
/// resulting table joins the same path.
///
/// # Arguments
///
/// * `lua` - state owning `when` function calls.
/// * `ctor` - constructor name for errors.
/// * `opts` - raw opts value.
///
/// # Returns
///
/// Guard table plus lane, defaulting to unconditional middle.
///
/// # Errors
///
/// Fails with plan errors for opts holding values of other shapes, for
/// unknown keys, for unparsable guards, and for unknown lane names.
fn rc_opts_table(lua: &Lua, ctor: &str, opts: Value) -> mlua::Result<(Option<Table>, Lane)> {
    let table = match opts {
        Value::Nil => return Ok((None, Lane::Middle)),
        Value::Table(table) => table,
        _ => return Err(rc_field_error(ctor, "opts", "must be a table")),
    };
    let when_value: Value = table.get("when")?;
    if let Value::Function(func) = when_value {
        let resolved = call_when_function(lua, ctor, &func)?;
        table.set("when", resolved)?;
    }
    let ctx = format!("{ctor}: field 'opts'");
    let json = table_to_json(&table, &ctx).map_err(|err| {
        let message = err.to_string();
        rc_field_error(ctor, "opts", &message)
    })?;
    let parsed: EntryOpts = serde_json::from_value(json)
        .map_err(|err| rc_field_error(ctor, "opts", &format!("{err}")))?;
    let lane = match parsed.lane {
        None => Lane::Middle,
        Some(name) => Lane::parse(&name).ok_or_else(|| {
            rc_field_error(ctor, "lane", "must be one of 'first', 'middle', or 'last'")
        })?,
    };
    let when_table = match parsed.when {
        None | Some(Json::Null) => None,
        Some(raw) => {
            serde_json::from_value::<Condition>(raw)
                .map_err(|err| rc_field_error(ctor, "when", &format!("{err}")))?;
            let guard: Table = table
                .get("when")
                .map_err(|_| rc_field_error(ctor, "when", "must be a condition table"))?;
            Some(guard)
        }
    };
    Ok((when_table, lane))
}

/// Reads a dense string array into model shape.
///
/// # Arguments
///
/// * `table` - Lua table holding the array.
/// * `ctor` - constructor name for errors.
/// * `field` - field name for errors.
///
/// # Returns
///
/// String values in index order.
///
/// # Errors
///
/// Fails with plan errors for sparse arrays and for items holding values
/// of other shapes.
fn take_model_string_array(table: &Table, ctor: &str, field: &str) -> mlua::Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let index = match key {
            Value::Integer(index) => index,
            _ => return Err(rc_field_error(ctor, field, "must be a string array")),
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(rc_field_error(
                    ctor,
                    field,
                    &format!("entry [{index}] must be a string"),
                ));
            }
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in (1i64..).zip(indexed.iter()) {
        if *index != position {
            return Err(rc_field_error(
                ctor,
                field,
                "must be a dense string array starting at 1",
            ));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Calls a `when` builder function with the shell namespace.
///
/// # Arguments
///
/// * `lua` - state holding the shell namespace.
/// * `ctor` - constructor name for errors.
/// * `func` - builder function under call.
///
/// # Returns
///
/// Condition table returned by the function.
///
/// # Errors
///
/// Fails with plan errors while the shell table stays absent and for
/// function failures.
fn call_when_function(lua: &Lua, ctor: &str, func: &mlua::Function) -> mlua::Result<Table> {
    let shell = match lua.globals().get::<Value>("confit")? {
        Value::Table(confit) => match confit.get::<Value>("shell")? {
            Value::Table(shell) => shell,
            _ => return Err(rc_field_error(ctor, "when", "needs the confit.shell table")),
        },
        _ => return Err(rc_field_error(ctor, "when", "needs the confit.shell table")),
    };
    match func.call::<Table>(shell) {
        Ok(table) => Ok(table),
        Err(err) => {
            let is_plan = err
                .downcast_ref::<Error>()
                .is_some_and(|domain| matches!(domain, Error::Plan(_)));
            if is_plan {
                return Err(err);
            }
            Err(rc_field_error(
                ctor,
                "when",
                &format!("function failed: {err}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a Lua state holding document constructors.
    ///
    /// # Returns
    ///
    /// Lua state holding the constructors.
    fn setup() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install");
        lua
    }

    /// Builds a Lua state holding document plus shell namespaces.
    ///
    /// # Returns
    ///
    /// Lua state holding both namespaces.
    fn setup_rc() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install document");
        crate::framework::shell::install(&lua).expect("install shell");
        lua
    }

    /// Builds a document model from a Lua chunk returning a table.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning one document table.
    ///
    /// # Returns
    ///
    /// Model document holding chunk data.
    fn eval_document(lua: &Lua, expr: &str) -> Document {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::Table(table) => document_table_to_document(&table, "test").expect("convert"),
            other => panic!("expected document table, got {other:?}"),
        }
    }

    /// Reads one rc entry table from a Lua chunk.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning one entry table.
    ///
    /// # Returns
    ///
    /// Entry table holding chunk data.
    fn eval_rc_entry(lua: &Lua, expr: &str) -> Table {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::Table(table) => table,
            other => panic!("expected rc entry table, got {other:?}"),
        }
    }

    /// Reports plan domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for plan domain failures, false for other failures.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(domain) = err.downcast_ref::<Error>() {
            return matches!(domain, Error::Plan(_));
        }
        format!("{err}").contains("plan error:")
    }

    /// Reports Lua domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for Lua domain failures, false for other failures.
    fn is_lua_error(err: &mlua::Error) -> bool {
        if err.downcast_ref::<Error>().is_some() {
            return true;
        }
        format!("{err}").contains("lua error:")
    }

    #[test]
    fn constructors_cover_all_kinds() {
        let lua = setup();
        let structured = eval_document(
            &lua,
            r#"return confit.document.structured("toml", {path = "a.toml", data = {x = 1}})"#,
        );
        assert_eq!(structured.kind, DocumentKind::Structured);
        assert_eq!(structured.path, "a.toml");
        match structured.data {
            DocumentData::Structured {
                format: StructuredFormat::Toml,
                data: table,
            } => {
                assert_eq!(table.get("x").and_then(Json::as_i64), Some(1));
            }
            other => panic!("expected toml data, got {other:?}"),
        }
        let text = eval_document(&lua, r#"return confit.document.text("a.txt", "hi")"#);
        assert_eq!(text.kind, DocumentKind::Text);
        assert_eq!(text.path, "a.txt");
        assert_eq!(
            text.data,
            DocumentData::Text {
                content: "hi".to_string()
            }
        );
        let link = eval_document(&lua, r#"return confit.document.link("a.link", "target")"#);
        assert_eq!(link.kind, DocumentKind::Link);
        assert_eq!(link.path, "a.link");
        let rc = eval_document(&lua, r#"return confit.document.rc.new({})"#);
        assert_eq!(rc.kind, DocumentKind::Rc);
        assert_eq!(rc.path, "rc");
        match rc.data {
            DocumentData::Rc(data) => {
                assert!(data.profile.is_empty());
                assert!(data.env.is_empty());
                assert!(data.aliases.is_empty());
                assert!(data.init.is_empty());
            }
            other => panic!("expected rc data, got {other:?}"),
        }
    }

    #[test]
    fn document_tables_carry_kind_markers() {
        let lua = setup_rc();
        let value: Value = lua
            .load(r#"return confit.document.structured("json", {path = "a.json", data = {}})"#)
            .eval()
            .expect("evaluate");
        let table = match value {
            Value::Table(table) => table,
            other => panic!("expected table, got {other:?}"),
        };
        assert_eq!(table_kind(&table).as_deref(), Some("structured"));
        assert_eq!(table_format(&table).as_deref(), Some("json"));
        let text: Value = lua
            .load(r#"return confit.document.text("a.txt", "hi")"#)
            .eval()
            .expect("evaluate");
        let text_table = match text {
            Value::Table(table) => table,
            other => panic!("expected table, got {other:?}"),
        };
        assert_eq!(table_kind(&text_table).as_deref(), Some("text"));
        let rc: Value = lua
            .load(r#"return confit.document.rc.new({})"#)
            .eval()
            .expect("evaluate");
        let rc_table = match rc {
            Value::Table(table) => table,
            other => panic!("expected table, got {other:?}"),
        };
        assert_eq!(table_kind(&rc_table).as_deref(), Some("rc"));
        let entry = eval_rc_entry(&lua, r#"return confit.document.rc.alias("cat", "bat")"#);
        assert_eq!(table_kind(&entry).as_deref(), Some("rc-entry"));
        assert_eq!(table_area(&entry).as_deref(), Some("alias"));
    }

    #[test]
    fn structured_rejects_unknown_format() {
        let lua = setup();
        let err = lua
            .load(r#"return confit.document.structured("ini", {path = "a", data = {}})"#)
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("confit.document.structured"),
            "names ctor: {message}"
        );
        assert!(message.contains("format"), "names field: {message}");
    }

    #[test]
    fn structured_rejects_unknown_args_keys() {
        let lua = setup();
        let err = lua
            .load(
                r#"return confit.document.structured("toml", {path = "a", data = {}, bogus = 1})"#,
            )
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("confit.document.structured"),
            "names ctor: {message}"
        );
        assert!(message.contains("bogus"), "names key: {message}");
    }

    #[test]
    fn structured_names_bad_path_and_data() {
        let lua = setup();
        for (field, expr) in [
            (
                "path",
                r#"return confit.document.structured("toml", {data = {}})"#,
            ),
            (
                "path",
                r#"return confit.document.structured("toml", {path = 42, data = {}})"#,
            ),
            (
                "data",
                r#"return confit.document.structured("toml", {path = "a"})"#,
            ),
            (
                "data",
                r#"return confit.document.structured("toml", {path = "a", data = {1, 2}})"#,
            ),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            assert!(
                format!("{err}").contains(field),
                "names field {field}: {expr}: {err}"
            );
        }
    }

    #[test]
    fn data_only_validation_names_field() {
        let lua = setup();
        for (field, expr) in [
            (
                "data",
                r#"return confit.document.structured("toml", {path = "p", data = {f = function() end}})"#,
            ),
            ("path", r#"return confit.document.text(42, "hi")"#),
            ("content", r#"return confit.document.text("p", 42)"#),
            ("target", r#"return confit.document.link("p", {})"#),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_lua_error(&err), "Lua domain: {expr}: {err}");
            assert!(
                format!("{err}").contains(field),
                "names field {field}: {expr}: {err}"
            );
        }
    }

    #[test]
    fn rc_new_sections_land_in_flat_lists() {
        let lua = setup_rc();
        let value: Value = lua
            .load(
                r#"return confit.document.rc.new({
                    profile = { confit.document.rc.path_entry("/bin") },
                    config = { confit.document.rc.alias("cat", "bat") },
                    final = { confit.document.rc.eval({"starship", "init"}) },
                })"#,
            )
            .eval()
            .expect("evaluate");
        let table = match value {
            Value::Table(table) => table,
            other => panic!("expected table, got {other:?}"),
        };
        let document = document_table_to_document(&table, "test").expect("convert");
        match document.data {
            DocumentData::Rc(data) => {
                assert_eq!(data.profile.len(), 1);
                assert_eq!(data.profile[0].spec.value, "/bin");
                assert_eq!(data.aliases.len(), 1);
                assert_eq!(data.aliases[0].spec.name, "cat");
                assert_eq!(data.init.len(), 1);
            }
            other => panic!("expected rc data, got {other:?}"),
        }
    }

    #[test]
    fn rc_new_unknown_sections_fail_at_conversion() {
        let lua = setup_rc();
        let value: Value = lua
            .load(r#"return confit.document.rc.new({bogus = {}})"#)
            .eval()
            .expect("rc.new stays thin");
        let table = match value {
            Value::Table(table) => table,
            other => panic!("expected table, got {other:?}"),
        };
        let err = document_table_to_document(&table, "test add_document").expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        assert!(format!("{err}").contains("bogus"), "names section: {err}");
    }

    #[test]
    fn rc_new_bad_section_items_fail_at_conversion() {
        let lua = setup_rc();
        for (section, expr) in [
            (
                "config",
                r#"return confit.document.rc.new({config = "nope"})"#,
            ),
            (
                "config",
                r#"return confit.document.rc.new({config = {"nope"}})"#,
            ),
            (
                "profile",
                r#"return confit.document.rc.new({profile = {42}})"#,
            ),
        ] {
            let value: Value = lua.load(expr).eval().expect("rc.new stays thin");
            let table = match value {
                Value::Table(table) => table,
                other => panic!("expected table, got {other:?}"),
            };
            let err =
                document_table_to_document(&table, "test add_document").expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            assert!(
                format!("{err}").contains(section),
                "names section {section}: {expr}: {err}"
            );
        }
    }

    #[test]
    fn entry_builders_default_to_unconditional() {
        let lua = setup_rc();
        for expr in [
            r#"return confit.document.rc.alias("cat", "bat")"#,
            r#"return confit.document.rc.env("A", "1")"#,
            r#"return confit.document.rc.profile("P", "v")"#,
            r#"return confit.document.rc.profile_path("/bin")"#,
            r#"return confit.document.rc.path_entry("/bin")"#,
            r#"return confit.document.rc.eval({"a"})"#,
            r#"return confit.document.rc.cmd({"a"})"#,
            r#"return confit.document.rc.source("x")"#,
        ] {
            let entry = eval_rc_entry(&lua, expr);
            let when: Value = entry.get("when").expect("when field");
            assert!(matches!(when, Value::Nil), "{expr}");
        }
    }

    #[test]
    fn entry_tables_hold_model_shapes() {
        let lua = setup_rc();
        let alias = eval_rc_entry(&lua, r#"return confit.document.rc.alias("cat", "bat")"#);
        let name: String = alias.get("name").expect("name");
        assert_eq!(name, "cat");
        let profile = eval_rc_entry(&lua, r#"return confit.document.rc.profile("P", "v")"#);
        let op: String = profile.get("op").expect("op");
        assert_eq!(op, "prepend");
        let path_entry = eval_rc_entry(&lua, r#"return confit.document.rc.path_entry("/bin")"#);
        let pname: String = path_entry.get("name").expect("name");
        assert_eq!(pname, "PATH");
        assert_eq!(table_area(&path_entry).as_deref(), Some("profile"));
        let (_, json) = entry_table_to_json(
            &eval_rc_entry(&lua, r#"return confit.document.rc.eval({"a"})"#),
            "test",
        )
        .expect("init converts");
        assert!(json.get("eval").is_some(), "holds eval: {json}");
    }

    #[test]
    fn init_builders_store_lane_with_middle_default() {
        let lua = setup_rc();
        let (_, first) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"return confit.document.rc.eval({"a"}, {lane = "first"})"#,
            ),
            "test",
        )
        .expect("convert");
        assert_eq!(
            first
                .get("eval")
                .and_then(|inner| inner.get("lane"))
                .and_then(Json::as_str),
            Some("first")
        );
        let (_, last) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"return confit.document.rc.cmd({"a"}, {lane = "last"})"#,
            ),
            "test",
        )
        .expect("convert");
        assert_eq!(
            last.get("cmd")
                .and_then(|inner| inner.get("lane"))
                .and_then(Json::as_str),
            Some("last")
        );
        let (_, sourced) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"return confit.document.rc.source("x", {lane = "first"})"#,
            ),
            "test",
        )
        .expect("convert");
        assert_eq!(
            sourced
                .get("source")
                .and_then(|inner| inner.get("lane"))
                .and_then(Json::as_str),
            Some("first")
        );
        let (_, middle) = entry_table_to_json(
            &eval_rc_entry(&lua, r#"return confit.document.rc.eval({"a"})"#),
            "test",
        )
        .expect("convert");
        assert!(
            middle
                .get("eval")
                .and_then(|inner| inner.get("lane"))
                .is_none(),
            "middle omits lane: {middle}"
        );
    }

    #[test]
    fn entry_opts_reject_unknown_keys() {
        let lua = setup_rc();
        let err = lua
            .load(r#"return confit.document.rc.alias("cat", "bat", {priority = 5})"#)
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("confit.document.rc.alias"),
            "names ctor: {message}"
        );
        assert!(message.contains("priority"), "names key: {message}");
    }

    #[test]
    fn entry_builders_reject_bad_lane() {
        let lua = setup_rc();
        let err = lua
            .load(r#"return confit.document.rc.eval({"a"}, {lane = "sideways"})"#)
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("confit.document.rc.eval"),
            "names ctor: {message}"
        );
        assert!(message.contains("lane"), "names field: {message}");
    }

    #[test]
    fn rc_when_table_and_function_parse_to_model_shape() {
        let lua = setup_rc();
        let (_, entry) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"return confit.document.rc.env("A", "1", {when = confit.shell.env_eq({key = "K", value = "v"})})"#,
            ),
            "test",
        )
        .expect("convert");
        assert_eq!(
            entry.get("when"),
            Some(&serde_json::json!({"env_eq": {"key": "K", "value": "v"}}))
        );

        let (_, nested) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"local s = confit.shell
                return confit.document.rc.env("A", "1", {when = function(c)
                    return c.all({c.env_set({key = "X"}), c.nop(c.env_set({key = "Y"}))})
                end})"#,
            ),
            "test",
        )
        .expect("convert");
        assert!(
            nested
                .get("when")
                .and_then(|item| item.get("all"))
                .is_some()
        );

        let (_, tuned) = entry_table_to_json(
            &eval_rc_entry(
                &lua,
                r#"return confit.document.rc.alias("cat", "bat", {when = {in_path = {name = "bat"}}})"#,
            ),
            "test",
        )
        .expect("convert");
        assert_eq!(
            tuned.get("when"),
            Some(&serde_json::json!({"in_path": {"name": "bat"}}))
        );
    }

    #[test]
    fn when_method_is_gone() {
        let lua = setup_rc();
        let err = lua
            .load(
                r#"return confit.document.rc.alias("cat", "bat"):when({in_path = {name = "bat"}})"#,
            )
            .eval::<Value>()
            .expect_err("method gone");
        let message = format!("{err}");
        assert!(
            message.contains("when") || message.contains("attempt"),
            "names failure: {message}"
        );
    }

    #[test]
    fn rc_mistakes_fail_as_named_plan_errors() {
        let lua = setup_rc();
        for (ctor, field, expr) in [
            (
                "confit.document.rc.alias",
                "name",
                r#"return confit.document.rc.alias(42, "bat")"#,
            ),
            (
                "confit.document.rc.alias",
                "value",
                r#"return confit.document.rc.alias("cat", 42)"#,
            ),
            (
                "confit.document.rc.alias",
                "opts",
                r#"return confit.document.rc.alias("cat", "bat", 42)"#,
            ),
            (
                "confit.document.rc.alias",
                "when",
                r#"return confit.document.rc.alias("cat", "bat", {when = 42})"#,
            ),
            (
                "confit.document.rc.env",
                "when",
                r#"return confit.document.rc.env("A", "1", {when = {bogus = {}}})"#,
            ),
            (
                "confit.document.rc.env",
                "when",
                r#"return confit.document.rc.env("A", "1", {when = {env_eq = {key = "K"}}})"#,
            ),
            (
                "confit.document.rc.profile",
                "when",
                r#"return confit.document.rc.profile("P", "v", {when = {in_path = {name = 42}}})"#,
            ),
            (
                "confit.document.rc.profile_path",
                "dir",
                r#"return confit.document.rc.profile_path(42)"#,
            ),
            (
                "confit.document.rc.profile_path",
                "when",
                r#"return confit.document.rc.profile_path("/bin", {when = {exists = {}}})"#,
            ),
            (
                "confit.document.rc.path_entry",
                "dir",
                r#"return confit.document.rc.path_entry(42)"#,
            ),
            (
                "confit.document.rc.eval",
                "argv",
                r#"return confit.document.rc.eval("nope")"#,
            ),
            (
                "confit.document.rc.eval",
                "argv",
                r#"return confit.document.rc.eval({"a", 42})"#,
            ),
            (
                "confit.document.rc.cmd",
                "argv",
                r#"return confit.document.rc.cmd({cmd = {"b"}})"#,
            ),
            (
                "confit.document.rc.source",
                "path",
                r#"return confit.document.rc.source(42)"#,
            ),
            (
                "confit.document.rc.source",
                "when",
                r#"return confit.document.rc.source("x", {when = function(s) return 42 end})"#,
            ),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            let message = format!("{err}");
            assert!(message.contains(ctor), "names {ctor}: {expr}: {err}");
            assert!(message.contains(field), "names {field}: {expr}: {err}");
        }
    }
}
