//! Values
//!
//! Lua plus JSON conversion plus marker helpers.

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

/// Builds a plan domain error from a message.
pub(crate) fn plan_error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::external(confit_core::error::Error::Plan(message.into()))
}

/// Converts one Lua value into JSON.
pub(crate) fn lua_to_json(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(plan_error(format!("{ctx} must be a finite number"))),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json(&table, ctx),
        Value::Function(_) => Err(plan_error(format!(
            "{ctx} must be data-only (function not allowed)"
        ))),
        Value::UserData(_) | Value::LightUserData(_) => Err(plan_error(format!(
            "{ctx} must be data-only (userdata not allowed)"
        ))),
        Value::Thread(_) => Err(plan_error(format!(
            "{ctx} must be data-only (thread not allowed)"
        ))),
        Value::Error(_) | Value::Other(_) => Err(plan_error(format!("{ctx} must be data-only"))),
    }
}

/// Converts one Lua table into JSON.
pub(crate) fn table_to_json(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if entries.is_empty() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    let mut integers_only = true;
    for (key, value) in &entries {
        match key {
            Value::Integer(index) => indexed.push((*index, value.clone())),
            _ => {
                integers_only = false;
                break;
            }
        }
    }
    if integers_only {
        indexed.sort_by_key(|(index, _)| *index);
        let dense = indexed
            .iter()
            .enumerate()
            .all(|(position, (index, _))| *index == position as i64 + 1);
        if dense {
            let mut items = Vec::with_capacity(indexed.len());
            for (index, value) in &indexed {
                items.push(lua_to_json(value.clone(), &format!("{ctx}[{index}]"))?);
            }
            return Ok(Json::Array(items));
        }
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_error(format!(
                    "{ctx} must be a table with string keys"
                )));
            }
        };
        let child = format!("{ctx}.{name}");
        map.insert(name, lua_to_json(value.clone(), &child)?);
    }
    Ok(Json::Object(map))
}

/// Converts one JSON value into Lua.
pub(crate) fn json_to_lua(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    match json {
        Json::Null => Ok(Value::Nil),
        Json::Bool(flag) => Ok(Value::Boolean(*flag)),
        Json::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(Value::Integer(integer))
            } else if let Some(float) = number.as_f64() {
                Ok(Value::Number(float))
            } else {
                Err(plan_error(
                    "patch: value must be a finite number".to_string(),
                ))
            }
        }
        Json::String(text) => Ok(Value::String(lua.create_string(text.as_str())?)),
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (position, item) in items.iter().enumerate() {
                table.set((position + 1) as i64, json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        Json::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.set(key.as_str(), json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

/// Stamps a `__kind` marker plus one extra marker on a table.
pub(crate) fn set_marker(
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

/// Reads one marker value from a table metatable.
pub(crate) fn read_marker(table: &Table, key: &str) -> Option<String> {
    let meta = table.metatable()?;
    let value: Value = meta.get(key).ok()?;
    match value {
        Value::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// Reads a dense string array from a Lua table.
pub(crate) fn take_string_array(
    table: &Table,
    ctor: &str,
    field: &str,
) -> mlua::Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let index = match key {
            Value::Integer(index) => index,
            _ => {
                return Err(plan_error(format!(
                    "{ctor}: field '{field}' must be a string array"
                )));
            }
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_error(format!(
                    "{ctor}: field '{field}' entry [{index}] must be a string"
                )));
            }
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in (1i64..).zip(indexed.iter()) {
        if *index != position {
            return Err(plan_error(format!(
                "{ctor}: field '{field}' must be a dense string array starting at 1"
            )));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// One parsed patch path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    /// Table key naming the level.
    pub(crate) key: String,
    /// List index for indexed keys.
    pub(crate) index: Option<usize>,
}

/// Parses one dotted patch path with single indices.
pub(crate) fn parse_path(path: &str) -> mlua::Result<Vec<Segment>> {
    if path.is_empty() {
        return Err(plan_error("invalid path '': empty path".to_string()));
    }
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return Err(plan_error(format!("invalid path '{path}': empty segment")));
        }
        let (key, index) = match part.find('[') {
            None => {
                if part.contains(']') {
                    return Err(plan_error(format!("invalid path '{path}': bad index")));
                }
                (part.to_string(), None)
            }
            Some(open) => {
                let key = part[..open].to_string();
                if key.is_empty() || !part.ends_with(']') || part[open + 1..].contains('[') {
                    return Err(plan_error(format!("invalid path '{path}': bad index")));
                }
                let inner = &part[open + 1..part.len() - 1];
                if inner.is_empty() || !inner.chars().all(|item| item.is_ascii_digit()) {
                    return Err(plan_error(format!("invalid path '{path}': bad index")));
                }
                match inner.parse::<usize>() {
                    Ok(index) => (key, Some(index)),
                    Err(_) => return Err(plan_error(format!("invalid path '{path}': bad index"))),
                }
            }
        };
        segments.push(Segment { key, index });
    }
    Ok(segments)
}

/// Flattens JSON into dotted leaf entries.
pub(crate) fn flatten_json(
    value: &Json,
    prefix: &str,
    out: &mut std::collections::BTreeMap<String, Json>,
) {
    match value {
        Json::Object(map) => {
            for (key, item) in map {
                let child = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(item, &child, out);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_json(item, &format!("{prefix}[{index}]"), out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

/// Finds one plan message walking nested error sources.
pub(crate) fn find_plan(error: &mlua::Error) -> Option<String> {
    use std::error::Error as StdError;
    let mut current: Option<&dyn StdError> = Some(error);
    while let Some(node) = current {
        if let Some(domain) = node.downcast_ref::<confit_core::error::Error>()
            && let confit_core::error::Error::Plan(message) = domain
        {
            return Some(message.clone());
        }
        current = node.source();
    }
    None
}

/// Renders one JSON value in canonical string form.
pub(crate) fn json_text(value: &Json) -> mlua::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| plan_error(format!("value failed to render: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Lua {
        Lua::new()
    }

    #[test]
    fn paths_parse_dotted_shapes() {
        let parsed = match parse_path("a.b[0]") {
            Ok(parsed) => parsed,
            Err(error) => panic!("path parses: {error}"),
        };
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].index, Some(0));
        assert!(parse_path("").is_err());
        assert!(parse_path("a..b").is_err());
        assert!(parse_path("a[1][2]").is_err());
    }

    #[test]
    fn tables_reject_executable_values() {
        let lua = state();
        let table = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("table builds: {error}"),
        };
        let callback = match lua.load("return function() end").eval::<Value>() {
            Ok(callback) => callback,
            Err(error) => panic!("callback loads: {error}"),
        };
        if table.set("run", callback).is_err() {
            panic!("entry stores");
        }
        let error = match table_to_json(&table, "ctx") {
            Ok(_) => panic!("function passes"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("function not allowed"));
    }
}
