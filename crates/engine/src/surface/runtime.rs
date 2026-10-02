//! Runtime
//!
//! Condition constructors over runtime session facts.

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::error::{EngineError, FieldRef, Scope};
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
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped options.
fn env_eq_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.env_eq";
    let scope = Scope::method(CTOR);
    let table = opts.req_table(&scope, "opts")?;
    LeafConds::env_eq(lua, &scope, table)
}

/// Builds an `env_set` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped options.
fn env_set_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.env_set";
    let scope = Scope::method(CTOR);
    let table = opts.req_table(&scope, "opts")?;
    LeafConds::env_set(lua, &scope, table)
}

/// Builds an `in_path` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string names.
fn in_path_impl(lua: &Lua, name: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.in_path";
    let scope = Scope::method(CTOR);
    let name = name.req_str(&scope, "name")?;
    CondTables::leaf(lua, "in_path", "name", name)
}

/// Builds an `exists` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route paths.
fn exists_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.exists";
    let scope = Scope::method(CTOR);
    let route = req_condition_route(&path, &scope, "path")?;
    CondTables::route_leaf(lua, "exists", &route)
}

/// Builds a `changed` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route paths.
fn changed_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.changed";
    let scope = Scope::method(CTOR);
    let route = req_condition_route(&path, &scope, "path")?;
    CondTables::route_leaf(lua, "changed", &route)
}

/// Reads one condition route from a route value.
///
/// Plain strings refuse: evaluation cannot name an unexpanded path.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route values.
fn req_condition_route(
    value: &Value,
    scope: &Scope,
    field: &str,
) -> mlua::Result<confit_model::routes::Route> {
    if let Some(data) = value.as_userdata()
        && let Ok(route) = data.borrow::<super::handles::LuaRoute>()
    {
        return Ok(route.core().clone());
    }
    Err(EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name(field),
        want: "must be a confit.path value",
    }
    .into())
}

/// Builds an `all` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for sparse lists.
fn all_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.all";
    let scope = Scope::method(CTOR);
    let table = conds.req_table(&scope, "conds")?;
    CondTables::all(lua, &scope, table)
}

/// Builds an `any` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for sparse lists.
fn any_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.any";
    let scope = Scope::method(CTOR);
    let table = conds.req_table(&scope, "conds")?;
    CondTables::any(lua, &scope, table)
}

/// Builds a `nop` condition table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped conditions.
fn nop_impl(lua: &Lua, cond: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.runtime.nop";
    CondTables::nop(lua, &Scope::method(CTOR), cond)
}

/// Leaf condition builders holding domain validation.
struct LeafConds;

impl LeafConds {
    /// Builds an `env_eq` condition table from an opts table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::OptUnknown`] for unknown opts fields.
    /// - [`EngineError::Field`] for non-string leaves.
    ///
    fn env_eq(lua: &Lua, scope: &Scope, opts: Table) -> mlua::Result<Table> {
        let json = Self::opts_json(&opts, scope)?;
        check_keys(&json, scope, &["key", "value"])?;
        let key = get_string(&json, scope, "key")?;
        let value = get_string(&json, scope, "value")?;
        let inner = lua.create_table()?;
        inner.set("key", key)?;
        inner.set("value", value)?;
        let outer = lua.create_table()?;
        outer.set("env_eq", inner)?;
        Ok(outer)
    }

    /// Builds an `env_set` condition table from an opts table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::OptUnknown`] for unknown opts fields.
    /// - [`EngineError::Field`] for non-string leaves.
    ///
    fn env_set(lua: &Lua, scope: &Scope, opts: Table) -> mlua::Result<Table> {
        let json = Self::opts_json(&opts, scope)?;
        check_keys(&json, scope, &["key"])?;
        let key = get_string(&json, scope, "key")?;
        CondTables::leaf(lua, "env_set", "key", key)
    }

    /// Converts one opts table into JSON with a uniform shape error.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Shape`] for recursive tables.
    ///
    fn opts_json(opts: &Table, scope: &Scope) -> mlua::Result<Json> {
        opts.to_json(&scope.slot(FieldRef::name("opts")))
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
    /// The route lands as a base and relative object, so the
    /// JSON shape matches serde and old string artifacts fail.
    ///
    /// # Errors
    ///
    /// Table builds fail as Lua errors.
    fn route_leaf(
        lua: &Lua,
        shape: &str,
        route: &confit_model::routes::Route,
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
    /// # Errors
    ///
    /// - [`EngineError::Field`] for sparse lists.
    ///
    fn all(lua: &Lua, scope: &Scope, conds: Table) -> mlua::Result<Table> {
        let items = Self::take_tables(&conds, scope, "conds")?;
        Self::join(lua, "all", items)
    }

    /// Builds an `any` condition table from a condition list.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for sparse lists.
    ///
    fn any(lua: &Lua, scope: &Scope, conds: Table) -> mlua::Result<Table> {
        let items = Self::take_tables(&conds, scope, "conds")?;
        Self::join(lua, "any", items)
    }

    /// Builds a `nop` condition table from one nested condition.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped conditions.
    ///
    fn nop(lua: &Lua, scope: &Scope, cond: Value) -> mlua::Result<Table> {
        check_condition(&cond, scope, FieldRef::name("cond"))?;
        let outer = lua.create_table()?;
        outer.set("nop", cond)?;
        Ok(outer)
    }

    /// Joins condition tables into one list-shaped condition table.
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
    /// # Errors
    ///
    /// - [`EngineError::Field`] for sparse lists and
    ///   misshaped members.
    ///
    fn take_tables(conds: &Table, scope: &Scope, field: &'static str) -> mlua::Result<Vec<Table>> {
        let dense = || EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name(field),
            want: "must be a dense condition array starting at 1",
        };
        let mut indexed: Vec<(i64, Value)> = Vec::new();
        for pair in conds.pairs::<Value, Value>() {
            let (key, value) = pair?;
            let Some(index) = key.as_integer() else {
                return Err(dense().into());
            };
            indexed.push((index, value));
        }
        indexed.sort_by_key(|(index, _)| *index);
        for (position, (index, _)) in indexed.iter().enumerate() {
            if *index != position as i64 + 1 {
                return Err(dense().into());
            }
        }
        let mut out = Vec::with_capacity(indexed.len());
        for (index, value) in indexed {
            let item = FieldRef::index(field, index as usize);
            check_condition(&value, scope, item.clone())?;
            let Some(item) = value.opt_table() else {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: item,
                    want: "must be a condition table",
                }
                .into());
            };
            out.push(item);
        }
        Ok(out)
    }
}

/// Rejects unknown keys on a leaf opts object.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-table options.
/// - [`EngineError::OptUnknown`] for unknown fields.
fn check_keys(json: &Json, scope: &Scope, known: &[&str]) -> mlua::Result<()> {
    let field = FieldRef::name("opts");
    let Some(map) = json.as_object() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a table",
        }
        .into());
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(EngineError::OptUnknown {
                scope: scope.clone(),
                field,
                name: key.clone(),
            }
            .into());
        }
    }
    Ok(())
}

/// Reads one required string field from a leaf object.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string fields.
fn get_string(json: &Json, scope: &Scope, field: &str) -> mlua::Result<String> {
    let Some(value) = json.get(field).and_then(Json::as_str) else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name(field),
            want: "must be a string",
        }
        .into());
    };
    Ok(value.to_string())
}

/// Validates one condition value in any nested position.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-table conditions.
fn check_condition(value: &Value, scope: &Scope, field: FieldRef) -> mlua::Result<()> {
    let Some(table) = value.as_table() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a condition table",
        }
        .into());
    };
    let nested = scope.slot(field);
    let json = table.to_json(&nested)?;
    check_condition_json(&json, &nested)?;
    Ok(())
}

/// Validates one condition JSON shape recursively.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped tables and arrays.
/// - [`EngineError::UnknownKind`] for unknown shapes.
pub(crate) fn check_condition_json(json: &Json, scope: &Scope) -> crate::error::Result<()> {
    let map = match json {
        Json::Object(map) if map.len() == 1 => map,
        _ => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table with one shape",
            });
        }
    };
    let (shape, inner) = match map.iter().next() {
        Some(pair) => pair,
        None => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table with one shape",
            });
        }
    };
    match shape.as_str() {
        "env_eq" => {
            check_leaf(inner, scope, &["key", "value"])?;
            Ok(())
        }
        "env_set" => {
            check_leaf(inner, scope, &["key"])?;
            Ok(())
        }
        "in_path" => {
            check_leaf(inner, scope, &["name"])?;
            Ok(())
        }
        "exists" => {
            check_route(inner, scope)?;
            Ok(())
        }
        "changed" => {
            check_route(inner, scope)?;
            Ok(())
        }
        "all" | "any" => {
            let items = match inner {
                Json::Array(items) => items,
                _ => {
                    return Err(EngineError::Shape {
                        scope: scope.clone(),
                        want: "must be a dense condition array",
                    });
                }
            };
            for (position, item) in items.iter().enumerate() {
                check_condition_json(item, &scope.entry(position + 1))?;
            }
            Ok(())
        }
        "nop" => check_condition_json(inner, &scope.key("nop")),
        other => Err(EngineError::UnknownKind {
            scope: scope.clone(),
            what: "condition shape",
            name: other.to_owned(),
        }),
    }
}

/// Validates one route condition inner object.
///
/// The inner object holds one `route` object with base and relative.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped route holders.
fn check_route(inner: &Json, scope: &Scope) -> crate::error::Result<()> {
    let map = match inner {
        Json::Object(map) => map,
        _ => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table",
            });
        }
    };
    if map.len() != 1 || !map.contains_key("route") {
        return Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must hold one 'route' field",
        });
    }
    let Some(body) = map.get("route") else {
        return Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must hold one 'route' field",
        });
    };
    check_route_body(body, scope)
}

/// Validates one route body object.
///
/// Bases name home, config, data, cache, or literal. Relative
/// paths read non-empty.
///
/// # Errors
///
/// - [`EngineError::InnerField`] for misshaped route bodies.
/// - [`EngineError::RouteBase`] for unknown bases.
fn check_route_body(body: &Json, scope: &Scope) -> crate::error::Result<()> {
    let route = FieldRef::name("route");
    let map = match body {
        Json::Object(map) => map,
        _ => {
            return Err(EngineError::InnerField {
                scope: scope.clone(),
                field: route,
                want: "must be a route table",
            });
        }
    };
    if map.len() != 2 {
        return Err(EngineError::InnerField {
            scope: scope.clone(),
            field: route,
            want: "must hold base plus relative",
        });
    }
    let base = match map.get("base").and_then(Json::as_str) {
        Some(base) => base,
        None => {
            return Err(EngineError::InnerField {
                scope: scope.clone(),
                field: route,
                want: "must hold base plus relative",
            });
        }
    };
    if !matches!(base, "home" | "config" | "data" | "cache" | "literal") {
        return Err(EngineError::RouteBase {
            scope: scope.clone(),
            base: base.to_owned(),
        });
    }
    match map.get("relative").and_then(Json::as_str) {
        Some(relative) if !relative.is_empty() => Ok(()),
        _ => Err(EngineError::InnerField {
            scope: scope.clone(),
            field: route,
            want: "must hold a non-empty relative path",
        }),
    }
}

/// Validates one leaf condition inner object.
///
/// # Errors
///
/// - [`EngineError::Shape`] for non-object leaves.
/// - [`EngineError::UnknownKind`] for unknown fields.
/// - [`EngineError::InnerField`] for non-string leaves.
fn check_leaf(inner: &Json, scope: &Scope, known: &[&str]) -> crate::error::Result<()> {
    let map = match inner {
        Json::Object(map) => map,
        _ => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table",
            });
        }
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(EngineError::UnknownKind {
                scope: scope.clone(),
                what: "field",
                name: key.clone(),
            });
        }
    }
    for field in known {
        match map.get(*field) {
            Some(Json::String(_)) => {}
            _ => {
                return Err(EngineError::InnerField {
                    scope: scope.clone(),
                    field: FieldRef::name(field),
                    want: "must be a string",
                });
            }
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
/// - [`EngineError::Shape`] for missing route fields.
/// - [`EngineError::NestBare`] for route builds.
fn route_from_json(inner: &Json, scope: &Scope) -> mlua::Result<confit_model::routes::Route> {
    let body = match inner.get("route") {
        Some(body) => body.clone(),
        None => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must hold one 'route' field",
            }
            .into());
        }
    };
    serde_json::from_value(body).map_err(|error| {
        EngineError::NestBare {
            scope: scope.clone(),
            reason: error.to_string(),
        }
        .into()
    })
}

/// Parses one condition array into core shapes.
///
/// # Errors
///
/// - [`EngineError::Shape`] for non-array shapes.
fn conditions_from_array(
    inner: &Json,
    scope: &Scope,
) -> mlua::Result<Vec<confit_model::condition::Condition>> {
    let Json::Array(items) = inner else {
        return Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must be a dense condition array",
        }
        .into());
    };
    let mut out = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        out.push(condition_from_json(item, &scope.entry(position + 1))?);
    }
    Ok(out)
}

/// Parses one condition JSON value into the core shape.
///
/// # Errors
///
/// - [`EngineError::Shape`] for misshaped conditions.
/// - [`EngineError::InnerField`] for non-string leaves.
/// - [`EngineError::UnknownKind`] for unknown shapes.
pub(crate) fn condition_from_json(
    json: &Json,
    scope: &Scope,
) -> mlua::Result<confit_model::condition::Condition> {
    use confit_model::condition::Condition;
    check_condition_json(json, scope)?;
    let map = match json {
        Json::Object(map) => map,
        _ => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table",
            }
            .into());
        }
    };
    let (shape, inner) = match map.iter().next() {
        Some(pair) => pair,
        None => {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a condition table",
            }
            .into());
        }
    };
    let leaf = |field: &str| -> mlua::Result<String> {
        match inner.get(field).and_then(Json::as_str) {
            Some(value) => Ok(value.to_string()),
            None => Err(EngineError::InnerField {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be a string",
            }
            .into()),
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
            route: route_from_json(inner, scope)?,
        }),
        "changed" => Ok(Condition::Changed {
            route: route_from_json(inner, scope)?,
        }),
        "all" => Ok(Condition::All(conditions_from_array(inner, scope)?)),
        "any" => Ok(Condition::Any(conditions_from_array(inner, scope)?)),
        "nop" => Ok(Condition::Not(Box::new(condition_from_json(
            inner,
            &scope.key("nop"),
        )?))),
        other => Err(EngineError::UnknownKind {
            scope: scope.clone(),
            what: "condition shape",
            name: other.to_owned(),
        }
        .into()),
    }
}
