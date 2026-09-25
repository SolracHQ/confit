//! Runtime
//!
//! Condition constructors over runtime session facts.

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt};

/// Installs the runtime namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set("env_eq", lua.create_function(env_eq_impl)?)?;
    namespace.set("env_set", lua.create_function(env_set_impl)?)?;
    namespace.set("in_path", lua.create_function(in_path_impl)?)?;
    namespace.set("exists", lua.create_function(exists_impl)?)?;
    namespace.set("changed", lua.create_function(changed_impl)?)?;
    namespace.set("all", lua.create_function(all_impl)?)?;
    namespace.set("any", lua.create_function(any_impl)?)?;
    namespace.set("nop", lua.create_function(nop_impl)?)?;
    namespace.set("SHELL", "{{shell}}")?;
    confit.set("runtime", namespace)?;
    Ok(())
}

/// Builds an `env_eq` condition table.
fn env_eq_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.env_eq";
    let table = opts.req_table(CTOR, "opts")?;
    LeafConds::env_eq(lua, CTOR, table)
}

/// Builds an `env_set` condition table.
fn env_set_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.env_set";
    let table = opts.req_table(CTOR, "opts")?;
    LeafConds::env_set(lua, CTOR, table)
}

/// Builds an `in_path` condition table.
fn in_path_impl(lua: &Lua, name: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.in_path";
    let name = name.req_str(CTOR, "name")?;
    CondTables::leaf(lua, "in_path", "name", name)
}

/// Builds an `exists` condition table.
fn exists_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.exists";
    let route = req_condition_route(&path, CTOR, "path")?;
    CondTables::route_leaf(lua, "exists", &route)
}

/// Builds a `changed` condition table.
fn changed_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.changed";
    let route = req_condition_route(&path, CTOR, "path")?;
    CondTables::route_leaf(lua, "changed", &route)
}

/// Reads one condition route from a route value.
///
/// Plain strings refuse: evaluation cannot name an unexpanded path.
///
/// # Errors
///
/// Non-route values fail as plan errors.
fn req_condition_route(
    value: &Value,
    ctor: &str,
    field: &str,
) -> mlua::Result<confit_model::handles::Route> {
    if let Some(data) = value.as_userdata()
        && let Ok(route) = data.borrow::<super::handles::LuaRoute>()
    {
        return Ok(route.core().clone());
    }
    Err(plan_error(format!(
        "{ctor}: field '{field}' must be a confit.path value"
    )))
}

/// Builds an `all` condition table.
fn all_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.all";
    let table = conds.req_table(CTOR, "conds")?;
    CondTables::all(lua, CTOR, table)
}

/// Builds an `any` condition table.
fn any_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.any";
    let table = conds.req_table(CTOR, "conds")?;
    CondTables::any(lua, CTOR, table)
}

/// Builds a `nop` condition table.
fn nop_impl(lua: &Lua, cond: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.nop";
    CondTables::nop(lua, CTOR, cond)
}

/// Leaf condition builders holding domain validation.
struct LeafConds;

impl LeafConds {
    /// Builds an `env_eq` condition table from an opts table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `opts` - opts table holding key and value strings.
    ///
    /// # Returns
    ///
    /// Condition table in the `env_eq` shape.
    ///
    /// # Errors
    ///
    /// Unknown opts fields fail as plan errors. Non-string leaves fail as plan errors.
    ///
    fn env_eq(lua: &Lua, ctor: &str, opts: Table) -> mlua::Result<Table> {
        let json = Self::opts_json(&opts, ctor)?;
        check_keys(&json, ctor, &["key", "value"])?;
        let key = get_string(&json, ctor, "key")?;
        let value = get_string(&json, ctor, "value")?;
        let inner = lua.create_table()?;
        inner.set("key", key)?;
        inner.set("value", value)?;
        let outer = lua.create_table()?;
        outer.set("env_eq", inner)?;
        Ok(outer)
    }

    /// Builds an `env_set` condition table from an opts table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `opts` - opts table holding the key string.
    ///
    /// # Returns
    ///
    /// Condition table in the `env_set` shape.
    ///
    /// # Errors
    ///
    /// Unknown opts fields fail as plan errors. Non-string leaves fail as plan errors.
    ///
    fn env_set(lua: &Lua, ctor: &str, opts: Table) -> mlua::Result<Table> {
        let json = Self::opts_json(&opts, ctor)?;
        check_keys(&json, ctor, &["key"])?;
        let key = get_string(&json, ctor, "key")?;
        CondTables::leaf(lua, "env_set", "key", key)
    }

    /// Converts one opts table into JSON with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `opts` - opts table under converting.
    /// * `ctor` - error prefix naming the constructor.
    ///
    /// # Returns
    ///
    /// JSON matching the data-only shape.
    ///
    /// # Errors
    ///
    /// Recursive tables fail as plan errors. Non-data values fail as plan errors.
    ///
    fn opts_json(opts: &Table, ctor: &str) -> mlua::Result<Json> {
        opts.to_json(&format!("{ctor}: field 'opts'"))
            .map_err(|error| plan_error(format!("{ctor}: field 'opts' {error}")))
    }
}

/// Condition table builders holding shared shapes.
struct CondTables;

impl CondTables {
    /// Wraps one string field into a one-shape condition table.
    ///
    /// The outer table holds one key naming the shape.
    ///
    /// # Returns
    ///
    /// Condition table in the named shape.
    ///
    fn leaf(lua: &Lua, shape: &str, field: &str, value: String) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        inner.set(field, value)?;
        let outer = lua.create_table()?;
        outer.set(shape, inner)?;
        Ok(outer)
    }

    /// Wraps one route into a one-shape condition table.
    ///
    /// The route lands as a base plus relative object, so the
    /// JSON shape matches serde and old string artifacts fail.
    ///
    /// # Errors
    ///
    /// Table builds fail as Lua errors.
    fn route_leaf(
        lua: &Lua,
        shape: &str,
        route: &confit_model::handles::Route,
    ) -> mlua::Result<Table> {
        let body = lua.create_table()?;
        body.set("base", route.base().name())?;
        body.set("relative", route.relative().to_string_lossy().into_owned())?;
        let inner = lua.create_table()?;
        inner.set("route", body)?;
        let outer = lua.create_table()?;
        outer.set(shape, inner)?;
        Ok(outer)
    }

    /// Builds an `all` condition table from a condition list.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `conds` - dense condition array table.
    ///
    /// # Returns
    ///
    /// Condition table in the `all` shape.
    ///
    /// # Errors
    ///
    /// Sparse lists fail as plan errors. Bad nested shapes fail as plan errors.
    ///
    fn all(lua: &Lua, ctor: &str, conds: Table) -> mlua::Result<Table> {
        let items = Self::take_tables(&conds, ctor, "conds")?;
        Self::join(lua, "all", items)
    }

    /// Builds an `any` condition table from a condition list.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `conds` - dense condition array table.
    ///
    /// # Returns
    ///
    /// Condition table in the `any` shape.
    ///
    /// # Errors
    ///
    /// Sparse lists fail as plan errors. Bad nested shapes fail as plan errors.
    ///
    fn any(lua: &Lua, ctor: &str, conds: Table) -> mlua::Result<Table> {
        let items = Self::take_tables(&conds, ctor, "conds")?;
        Self::join(lua, "any", items)
    }

    /// Builds a `nop` condition table from one nested condition.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `cond` - nested condition value.
    ///
    /// # Returns
    ///
    /// Condition table in the `nop` shape.
    ///
    /// # Errors
    ///
    /// Bad nested shapes fail as plan errors.
    ///
    fn nop(lua: &Lua, ctor: &str, cond: Value) -> mlua::Result<Table> {
        check_condition(&cond, ctor, "cond")?;
        let outer = lua.create_table()?;
        outer.set("nop", cond)?;
        Ok(outer)
    }

    /// Joins condition tables into one list-shaped condition table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `shape` - condition shape naming the list.
    /// * `items` - nested condition tables in order.
    ///
    /// # Returns
    ///
    /// Condition table holding the list.
    ///
    fn join(lua: &Lua, shape: &str, items: Vec<Table>) -> mlua::Result<Table> {
        let array = lua.create_table()?;
        for (position, item) in items.into_iter().enumerate() {
            array.set((position + 1) as i64, item)?;
        }
        let outer = lua.create_table()?;
        outer.set(shape, array)?;
        Ok(outer)
    }

    /// Collects nested condition tables in index order.
    ///
    /// # Arguments
    ///
    /// * `conds` - dense condition array table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `field` - field name naming the table.
    ///
    /// # Returns
    ///
    /// Nested condition tables in 1-based index order.
    ///
    /// # Errors
    ///
    /// Sparse lists fail as plan errors. Bad nested shapes fail as plan errors.
    ///
    fn take_tables(conds: &Table, ctor: &str, field: &str) -> mlua::Result<Vec<Table>> {
        let dense = || {
            plan_error(format!(
                "{ctor}: field '{field}' must be a dense condition array starting at 1"
            ))
        };
        let mut indexed: Vec<(i64, Value)> = Vec::new();
        for pair in conds.pairs::<Value, Value>() {
            let (key, value) = pair?;
            let Some(index) = key.as_integer() else {
                return Err(dense());
            };
            indexed.push((index, value));
        }
        indexed.sort_by_key(|(index, _)| *index);
        for (position, (index, _)) in indexed.iter().enumerate() {
            if *index != position as i64 + 1 {
                return Err(dense());
            }
        }
        let mut out = Vec::with_capacity(indexed.len());
        for (index, value) in indexed {
            let item_field = format!("{field}[{index}]");
            check_condition(&value, ctor, &item_field)?;
            let Some(item) = value.opt_table() else {
                return Err(plan_error(format!(
                    "{ctor}: field '{item_field}' must be a condition table"
                )));
            };
            out.push(item);
        }
        Ok(out)
    }
}

/// Rejects unknown keys on a leaf opts object.
fn check_keys(json: &Json, ctor: &str, known: &[&str]) -> mlua::Result<()> {
    let Some(map) = json.as_object() else {
        return Err(plan_error(format!("{ctor}: field 'opts' must be a table")));
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(plan_error(format!(
                "{ctor}: field 'opts' unknown field '{key}'"
            )));
        }
    }
    Ok(())
}

/// Reads one required string field from a leaf object.
fn get_string(json: &Json, ctor: &str, field: &str) -> mlua::Result<String> {
    let Some(value) = json.get(field).and_then(Json::as_str) else {
        return Err(plan_error(format!(
            "{ctor}: field '{field}' must be a string"
        )));
    };
    Ok(value.to_string())
}

/// Validates one condition value in any nested position.
fn check_condition(value: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let Some(table) = value.as_table() else {
        return Err(plan_error(format!(
            "{ctor}: field '{field}' must be a condition table"
        )));
    };
    let json = table
        .to_json(&format!("{ctor}: field '{field}'"))
        .map_err(|error| plan_error(format!("{ctor}: field '{field}' {error}")))?;
    check_condition_json(&json, &format!("{ctor}: field '{field}'"))
        .map_err(|detail| plan_error(format!("{ctor}: field '{field}' {detail}")))
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
            check_route(inner, ctx)?;
            Ok(())
        }
        "changed" => {
            check_route(inner, ctx)?;
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

/// Validates one route condition inner object.
///
/// The inner object holds one `route` object with base plus relative.
///
/// # Errors
///
/// Non-objects, unknown fields, and bad bases fail with detail.
fn check_route(inner: &Json, ctx: &str) -> Result<(), String> {
    let map = match inner {
        Json::Object(map) => map,
        _ => return Err(format!("{ctx} must be a condition table")),
    };
    if map.len() != 1 || !map.contains_key("route") {
        return Err(format!("{ctx} must hold one 'route' field"));
    }
    let Some(body) = map.get("route") else {
        return Err(format!("{ctx} must hold one 'route' field"));
    };
    check_route_body(body, ctx)
}

/// Validates one route body object.
///
/// Bases name home, config, data, cache, or literal. Relative
/// paths read non-empty.
///
/// # Errors
///
/// Bad shapes fail with detail.
fn check_route_body(body: &Json, ctx: &str) -> Result<(), String> {
    let map = match body {
        Json::Object(map) => map,
        _ => return Err(format!("{ctx} field 'route' must be a route table")),
    };
    if map.len() != 2 {
        return Err(format!("{ctx} field 'route' must hold base plus relative"));
    }
    let base = match map.get("base").and_then(Json::as_str) {
        Some(base) => base,
        None => return Err(format!("{ctx} field 'route' must hold base plus relative")),
    };
    if !matches!(base, "home" | "config" | "data" | "cache" | "literal") {
        return Err(format!("{ctx} field 'route' holds unknown base '{base}'"));
    }
    match map.get("relative").and_then(Json::as_str) {
        Some(relative) if !relative.is_empty() => Ok(()),
        _ => Err(format!(
            "{ctx} field 'route' must hold a non-empty relative path"
        )),
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

/// Parses one route condition body into the core shape.
///
/// Old string artifacts fail here: the body must hold a route object.
///
/// # Errors
///
/// Non-object bodies and bad routes fail as plan errors.
fn route_from_json(inner: &Json, ctx: &str) -> mlua::Result<confit_model::handles::Route> {
    let body = match inner.get("route") {
        Some(body) => body.clone(),
        None => {
            return Err(plan_error(format!("{ctx} must hold one 'route' field")));
        }
    };
    serde_json::from_value(body).map_err(|error| plan_error(format!("{ctx} {error}")))
}

/// Parses one condition array into core shapes.
///
/// # Errors
///
/// Non-arrays and bad members fail as plan errors.
fn conditions_from_array(
    inner: &Json,
    ctx: &str,
) -> mlua::Result<Vec<confit_model::condition::Condition>> {
    let Json::Array(items) = inner else {
        return Err(plan_error(format!("{ctx} must be a dense condition array")));
    };
    let mut out = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        out.push(condition_from_json(
            item,
            &format!("{ctx}[{}]", position + 1),
        )?);
    }
    Ok(out)
}

/// Parses one condition JSON value into the core shape.
pub(crate) fn condition_from_json(
    json: &Json,
    ctx: &str,
) -> mlua::Result<confit_model::condition::Condition> {
    use confit_model::condition::Condition;
    check_condition_json(json, ctx).map_err(crate::error::plan_error)?;
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
            route: route_from_json(inner, ctx)?,
        }),
        "changed" => Ok(Condition::Changed {
            route: route_from_json(inner, ctx)?,
        }),
        "all" => Ok(Condition::All(conditions_from_array(inner, ctx)?)),
        "any" => Ok(Condition::Any(conditions_from_array(inner, ctx)?)),
        "nop" => Ok(Condition::Not(Box::new(condition_from_json(
            inner,
            &format!("{ctx}.nop"),
        )?))),
        other => Err(plan_error(format!(
            "{ctx} unknown condition shape '{other}'"
        ))),
    }
}
