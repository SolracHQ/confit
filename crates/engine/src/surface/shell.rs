//! Shell
//!
//! Condition constructors over shell session facts.

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::values::{plan_error, table_to_json};

/// Installs the shell namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set("env_eq", lua.create_function(env_eq_impl)?)?;
    namespace.set("env_set", lua.create_function(env_set_impl)?)?;
    namespace.set("in_path", lua.create_function(in_path_impl)?)?;
    namespace.set("exists", lua.create_function(exists_impl)?)?;
    namespace.set("all", lua.create_function(all_impl)?)?;
    namespace.set("any", lua.create_function(any_impl)?)?;
    namespace.set("nop", lua.create_function(nop_impl)?)?;
    namespace.set("SHELL", "{{shell}}")?;
    confit.set("shell", namespace)?;
    Ok(())
}

/// Builds a field error naming constructor plus field.
fn field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    plan_error(format!("{ctor}: field '{field}' {detail}"))
}

/// Reads a string from a Lua value.
fn take_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(ctor, field, "must be a string")),
    }
}

/// Builds an `env_eq` condition table.
fn env_eq_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.env_eq";
    let table = as_opts_table(opts, CTOR)?;
    let json = table_to_json(&table, &format!("{CTOR}: field 'opts'"))
        .map_err(|error| field_error(CTOR, "opts", &error.to_string()))?;
    check_keys(&json, CTOR, &["key", "value"])?;
    let key = get_string(&json, CTOR, "key")?;
    let value = get_string(&json, CTOR, "value")?;
    wrap(lua, "env_eq", &[("key", key), ("value", value)])
}

/// Builds an `env_set` condition table.
fn env_set_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.env_set";
    let table = as_opts_table(opts, CTOR)?;
    let json = table_to_json(&table, &format!("{CTOR}: field 'opts'"))
        .map_err(|error| field_error(CTOR, "opts", &error.to_string()))?;
    check_keys(&json, CTOR, &["key"])?;
    let key = get_string(&json, CTOR, "key")?;
    wrap(lua, "env_set", &[("key", key)])
}

/// Builds an `in_path` condition table.
fn in_path_impl(lua: &Lua, name: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.in_path";
    let name = take_string(name, CTOR, "name")?;
    wrap(lua, "in_path", &[("name", name)])
}

/// Builds an `exists` condition table.
fn exists_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.exists";
    let path = take_string(path, CTOR, "path")?;
    wrap(lua, "exists", &[("path", path)])
}

/// Builds an `all` condition table.
fn all_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.all";
    let items = take_conditions(conds, CTOR)?;
    let array = lua.create_table()?;
    for (position, item) in items.into_iter().enumerate() {
        array.set((position + 1) as i64, item)?;
    }
    let outer = lua.create_table()?;
    outer.set("all", array)?;
    Ok(outer)
}

/// Builds an `any` condition table.
fn any_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.any";
    let items = take_conditions(conds, CTOR)?;
    let array = lua.create_table()?;
    for (position, item) in items.into_iter().enumerate() {
        array.set((position + 1) as i64, item)?;
    }
    let outer = lua.create_table()?;
    outer.set("any", array)?;
    Ok(outer)
}

/// Builds a `nop` condition table.
fn nop_impl(lua: &Lua, cond: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.nop";
    check_condition(&cond, CTOR, "cond")?;
    let outer = lua.create_table()?;
    outer.set("nop", cond)?;
    Ok(outer)
}

/// Wraps string fields into a one-shape condition table.
fn wrap(lua: &Lua, shape: &str, fields: &[(&str, String)]) -> mlua::Result<Table> {
    let inner = lua.create_table()?;
    for (key, value) in fields {
        inner.set(*key, value.as_str())?;
    }
    let outer = lua.create_table()?;
    outer.set(shape, inner)?;
    Ok(outer)
}

/// Reads the opts table from a raw value.
fn as_opts_table(opts: Value, ctor: &str) -> mlua::Result<Table> {
    match opts {
        Value::Table(table) => Ok(table),
        _ => Err(field_error(ctor, "opts", "must be a table")),
    }
}

/// Rejects unknown keys on a leaf opts object.
fn check_keys(json: &Json, ctor: &str, known: &[&str]) -> mlua::Result<()> {
    let map = match json {
        Json::Object(map) => map,
        _ => return Err(field_error(ctor, "opts", "must be a table")),
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(field_error(ctor, "opts", &format!("unknown field '{key}'")));
        }
    }
    Ok(())
}

/// Reads one required string field from a leaf object.
fn get_string(json: &Json, ctor: &str, field: &str) -> mlua::Result<String> {
    match json.get(field) {
        Some(Json::String(value)) => Ok(value.clone()),
        _ => Err(field_error(ctor, field, "must be a string")),
    }
}

/// Collects nested condition tables in index order.
fn take_conditions(conds: Value, ctor: &str) -> mlua::Result<Vec<Table>> {
    let table = match conds {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, "conds", "must be a table")),
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::Integer(index) => indexed.push((index, value)),
            _ => {
                return Err(field_error(
                    ctor,
                    "conds",
                    "must be a dense condition array starting at 1",
                ));
            }
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(field_error(
                ctor,
                "conds",
                "must be a dense condition array starting at 1",
            ));
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (index, value) in indexed {
        check_condition(&value, ctor, &format!("conds[{index}]"))?;
        match value {
            Value::Table(item) => out.push(item),
            _ => {
                return Err(field_error(
                    ctor,
                    &format!("conds[{index}]"),
                    "must be a condition table",
                ));
            }
        }
    }
    Ok(out)
}

/// Validates one condition value in any nested position.
fn check_condition(value: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match value {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "must be a condition table")),
    };
    let json = table_to_json(table, &format!("{ctor}: field '{field}'"))
        .map_err(|error| field_error(ctor, field, &error.to_string()))?;
    check_condition_json(&json, &format!("{ctor}: field '{field}'"))
        .map_err(|detail| field_error(ctor, field, &detail))
}

/// Validates one condition JSON shape recursively.
pub(crate) fn check_condition_json(json: &Json, ctx: &str) -> Result<(), String> {
    let map = match json {
        Json::Object(map) if map.len() == 1 => map,
        _ => return Err(format!("{ctx} must be a condition table with one shape")),
    };
    let (shape, inner) = match map.iter().next() {
        Some(pair) => pair,
        None => return Err(format!("{ctx} must be a condition table with one shape")),
    };
    match shape.as_str() {
        "env_eq" => {
            check_leaf(inner, ctx, &["key", "value"])?;
            Ok(())
        }
        "env_set" => {
            check_leaf(inner, ctx, &["key"])?;
            Ok(())
        }
        "in_path" => {
            check_leaf(inner, ctx, &["name"])?;
            Ok(())
        }
        "exists" => {
            check_leaf(inner, ctx, &["path"])?;
            Ok(())
        }
        "all" | "any" => {
            let items = match inner {
                Json::Array(items) => items,
                _ => return Err(format!("{ctx} must be a dense condition array")),
            };
            for (position, item) in items.iter().enumerate() {
                check_condition_json(item, &format!("{ctx}[{}]", position + 1))?;
            }
            Ok(())
        }
        "nop" => check_condition_json(inner, &format!("{ctx}.nop")),
        other => Err(format!("{ctx} unknown condition shape '{other}'")),
    }
}

/// Validates one leaf condition inner object.
fn check_leaf(inner: &Json, ctx: &str, known: &[&str]) -> Result<(), String> {
    let map = match inner {
        Json::Object(map) => map,
        _ => return Err(format!("{ctx} must be a condition table")),
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(format!("{ctx} unknown field '{key}'"));
        }
    }
    for field in known {
        match map.get(*field) {
            Some(Json::String(_)) => {}
            _ => return Err(format!("{ctx} field '{field}' must be a string")),
        }
    }
    Ok(())
}

/// Parses one condition JSON value into the core shape.
pub(crate) fn condition_from_json(
    json: &Json,
    ctx: &str,
) -> mlua::Result<confit_core::document::Condition> {
    use confit_core::document::Condition;
    check_condition_json(json, ctx).map_err(crate::values::plan_error)?;
    let map = match json {
        Json::Object(map) => map,
        _ => return Err(plan_error(format!("{ctx} must be a condition table"))),
    };
    let (shape, inner) = match map.iter().next() {
        Some(pair) => pair,
        None => return Err(plan_error(format!("{ctx} must be a condition table"))),
    };
    let leaf = |field: &str| -> mlua::Result<String> {
        match inner.get(field).and_then(Json::as_str) {
            Some(value) => Ok(value.to_string()),
            None => Err(plan_error(format!(
                "{ctx} field '{field}' must be a string"
            ))),
        }
    };
    match shape.as_str() {
        "env_eq" => Ok(Condition::EnvEq {
            key: leaf("key")?,
            value: leaf("value")?,
        }),
        "env_set" => Ok(Condition::EnvSet { key: leaf("key")? }),
        "in_path" => Ok(Condition::InPath {
            name: leaf("name")?,
        }),
        "exists" => Ok(Condition::Exists {
            path: leaf("path")?,
        }),
        "all" => {
            let mut out = Vec::new();
            if let Json::Array(items) = inner {
                for (position, item) in items.iter().enumerate() {
                    out.push(condition_from_json(
                        item,
                        &format!("{ctx}[{}]", position + 1),
                    )?);
                }
            }
            Ok(Condition::All(out))
        }
        "any" => {
            let mut out = Vec::new();
            if let Json::Array(items) = inner {
                for (position, item) in items.iter().enumerate() {
                    out.push(condition_from_json(
                        item,
                        &format!("{ctx}[{}]", position + 1),
                    )?);
                }
            }
            Ok(Condition::Any(out))
        }
        "nop" => Ok(Condition::Not(Box::new(condition_from_json(
            inner,
            &format!("{ctx}.nop"),
        )?))),
        other => Err(plan_error(format!(
            "{ctx} unknown condition shape '{other}'"
        ))),
    }
}
