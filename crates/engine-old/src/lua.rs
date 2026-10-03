//! Lua
//!
//! Lua, JSON conversion, and marker helpers.

use std::collections::BTreeMap;

use mlua::{Function, Lua, Table, Value};
use serde_json::Value as Json;

use crate::error::{EngineError, FieldRef, Scope};

/// Lua table shape and conversion helpers.
///
pub(crate) trait TableExt {
    /// Converts one Lua table into JSON.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the field under converting.
    ///
    /// # Returns
    ///
    /// JSON object or array matching the table shape.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Shape`] for recursive tables and non-string keys.
    ///
    fn to_json(&self, scope: &Scope) -> mlua::Result<Json>;

    /// Reports the dense-array shape predicate used by conversion.
    ///
    /// # Returns
    ///
    /// True for dense arrays, else false.
    fn is_array(&self) -> bool;

    /// Reads one named string field from a table.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned string contents.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for missing and non-string fields.
    ///
    fn req_str(&self, scope: &Scope, field: &str) -> mlua::Result<String>;

    /// Reads one named table field from a table.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned table handle.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for missing and non-table fields.
    ///
    fn req_table(&self, scope: &Scope, field: &str) -> mlua::Result<Table>;

    /// Reads the table itself into a string-keyed object.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name naming the table.
    ///
    /// # Returns
    ///
    /// Object entries in canonical form.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Shape`] for recursive tables and non-object shapes.
    ///
    fn req_object(&self, scope: &Scope, field: &str) -> mlua::Result<BTreeMap<String, Json>>;
}

/// Lua value conversion helpers.
///
pub(crate) trait ValueExt {
    /// Converts one Lua value into JSON.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the field under converting.
    ///
    /// # Returns
    ///
    /// JSON matching the data-only shape.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Shape`] for recursive tables and non-data values.
    ///
    fn to_json(self, scope: &Scope) -> mlua::Result<Json>;

    /// Reads one string value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned string contents.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-string values.
    ///
    fn req_str(self, scope: &Scope, field: &str) -> mlua::Result<String>;

    /// Reads one table value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned table handle.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-table values.
    ///
    fn req_table(self, scope: &Scope, field: &str) -> mlua::Result<Table>;

    /// Reads one function value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned function handle.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-function values.
    ///
    fn req_func(self, scope: &Scope, field: &str) -> mlua::Result<Function>;

    /// Reads one integer value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `scope` - scope naming the constructor under reading.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Integer contents.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-integer values.
    ///
    fn req_int(self, scope: &Scope, field: &str) -> mlua::Result<i64>;

    /// Tests one value for string contents.
    ///
    /// # Arguments
    ///
    /// * `value` - value under testing.
    ///
    /// # Returns
    ///
    /// String contents for strings. Else None.
    ///
    fn opt_str(self) -> Option<String>;

    /// Tests one value for table contents.
    ///
    /// # Arguments
    ///
    /// * `value` - value under testing.
    ///
    /// # Returns
    ///
    /// Table handle for tables. Else None.
    ///
    fn opt_table(self) -> Option<Table>;

    /// Tests one value for function contents.
    ///
    /// # Arguments
    ///
    /// * `value` - value under testing.
    ///
    /// # Returns
    ///
    /// Function handle for functions. Else None.
    ///
    fn opt_func(self) -> Option<Function>;
}

/// JSON conversion helpers.
///
pub(crate) trait JsonExt {
    /// Converts one JSON value into Lua.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning new strings and tables.
    /// * `scope` - scope naming the caller under converting.
    ///
    /// # Returns
    ///
    /// Lua value matching the JSON shape.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Detail`] for non-finite numbers.
    ///
    fn to_lua(&self, lua: &Lua, scope: &Scope) -> mlua::Result<Value>;
}

impl TableExt for Table {
    fn to_json(&self, scope: &Scope) -> mlua::Result<Json> {
        if holds_cycle(&Value::Table(self.clone())) {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "holds a recursive table",
            }
            .into());
        }
        table_to_json_inner(self, scope)
    }

    fn is_array(&self) -> bool {
        let mut entries: Vec<(Value, Value)> = Vec::new();
        for pair in self.pairs::<Value, Value>() {
            match pair {
                Ok(pair) => entries.push(pair),
                Err(_) => return false,
            }
        }
        dense_order(&entries).is_some()
    }

    fn req_str(&self, scope: &Scope, field: &str) -> mlua::Result<String> {
        let value: Value = self.get(field)?;
        value.req_str(scope, field)
    }

    fn req_table(&self, scope: &Scope, field: &str) -> mlua::Result<Table> {
        let value: Value = self.get(field)?;
        value.req_table(scope, field)
    }

    fn req_object(&self, scope: &Scope, field: &str) -> mlua::Result<BTreeMap<String, Json>> {
        let json = self.to_json(&scope.slot(FieldRef::name(field)))?;
        match json {
            Json::Object(map) => Ok(map.into_iter().collect()),
            _ => Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be a table with string keys",
            }
            .into()),
        }
    }
}

impl ValueExt for Value {
    fn to_json(self, scope: &Scope) -> mlua::Result<Json> {
        if holds_cycle(&self) {
            return Err(EngineError::Shape {
                scope: scope.clone(),
                want: "holds a recursive table",
            }
            .into());
        }
        lua_to_json_inner(self, scope)
    }

    fn req_str(self, scope: &Scope, field: &str) -> mlua::Result<String> {
        match self {
            Value::String(text) => Ok(text.to_string_lossy()),
            _ => Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be a string",
            }
            .into()),
        }
    }

    fn req_table(self, scope: &Scope, field: &str) -> mlua::Result<Table> {
        match self {
            Value::Table(table) => Ok(table),
            _ => Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be a table",
            }
            .into()),
        }
    }

    fn req_func(self, scope: &Scope, field: &str) -> mlua::Result<Function> {
        match self {
            Value::Function(func) => Ok(func),
            _ => Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be a function",
            }
            .into()),
        }
    }

    fn req_int(self, scope: &Scope, field: &str) -> mlua::Result<i64> {
        match self {
            Value::Integer(index) => Ok(index),
            _ => Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name(field),
                want: "must be an integer",
            }
            .into()),
        }
    }

    fn opt_str(self) -> Option<String> {
        match self {
            Value::String(text) => Some(text.to_string_lossy()),
            _ => None,
        }
    }

    fn opt_table(self) -> Option<Table> {
        match self {
            Value::Table(table) => Some(table),
            _ => None,
        }
    }

    fn opt_func(self) -> Option<Function> {
        match self {
            Value::Function(func) => Some(func),
            _ => None,
        }
    }
}

impl JsonExt for Json {
    fn to_lua(&self, lua: &Lua, scope: &Scope) -> mlua::Result<Value> {
        match self {
            Json::Null => Ok(Value::Nil),
            Json::Bool(flag) => Ok(Value::Boolean(*flag)),
            Json::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Ok(Value::Integer(integer))
                } else if let Some(float) = number.as_f64() {
                    Ok(Value::Number(float))
                } else {
                    Err(EngineError::Detail {
                        scope: scope.clone(),
                        want: "value must be a finite number",
                    }
                    .into())
                }
            }
            Json::String(text) => Ok(Value::String(lua.create_string(text.as_str())?)),
            Json::Array(items) => {
                let table = lua.create_table()?;
                for (position, item) in items.iter().enumerate() {
                    table.set((position + 1) as i64, item.to_lua(lua, scope)?)?;
                }
                Ok(Value::Table(table))
            }
            Json::Object(map) => {
                let table = lua.create_table()?;
                for (key, item) in map {
                    table.set(key.as_str(), item.to_lua(lua, scope)?)?;
                }
                Ok(Value::Table(table))
            }
        }
    }
}

/// Converts one Lua value into JSON without a cycle precheck.
fn lua_to_json_inner(value: Value, scope: &Scope) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(EngineError::Shape {
                scope: scope.clone(),
                want: "must be a finite number",
            }
            .into()),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json_inner(&table, scope),
        Value::Function(_) => Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must be data-only (function not allowed)",
        }
        .into()),
        Value::UserData(_) | Value::LightUserData(_) => Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must be data-only (userdata not allowed)",
        }
        .into()),
        Value::Thread(_) => Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must be data-only (thread not allowed)",
        }
        .into()),
        Value::Error(_) | Value::Other(_) => Err(EngineError::Shape {
            scope: scope.clone(),
            want: "must be data-only",
        }
        .into()),
    }
}

/// Orders dense array positions, else None.
///
/// Empty tables read as objects, matching conversion.
///
/// # Arguments
///
/// * `entries` - collected key and value pairs.
///
/// # Returns
///
/// Value positions in 1-based order for dense integer keys.
fn dense_order(entries: &[(Value, Value)]) -> Option<Vec<usize>> {
    if entries.is_empty() {
        return None;
    }
    let mut indexed: Vec<(i64, usize)> = Vec::with_capacity(entries.len());
    for (position, (key, _)) in entries.iter().enumerate() {
        match key {
            Value::Integer(index) => indexed.push((*index, position)),
            _ => return None,
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    let dense = indexed
        .iter()
        .enumerate()
        .all(|(slot, (index, _))| *index == slot as i64 + 1);
    dense.then(|| indexed.iter().map(|(_, position)| *position).collect())
}

/// Converts one Lua table into JSON without a cycle precheck.
fn table_to_json_inner(table: &Table, scope: &Scope) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if let Some(order) = dense_order(&entries) {
        let mut items = Vec::with_capacity(order.len());
        for (slot, position) in order.iter().enumerate() {
            let (_, value) = &entries[*position];
            items.push(lua_to_json_inner(value.clone(), &scope.entry(slot + 1))?);
        }
        return Ok(Json::Array(items));
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(EngineError::Shape {
                    scope: scope.clone(),
                    want: "must be a table with string keys",
                }
                .into());
            }
        };
        map.insert(
            name.clone(),
            lua_to_json_inner(value.clone(), &scope.key(&name))?,
        );
    }
    Ok(Json::Object(map))
}

/// Reports true when a Lua value holds a table cycle.
///
/// # Arguments
///
/// * `value` - value to inspect for ancestor cycles.
///
/// # Returns
///
/// True when one table reaches itself through keys or values.
///
pub(crate) fn holds_cycle(value: &Value) -> bool {
    let mut stack = Vec::new();
    holds_cycle_inner(value, &mut stack)
}

/// Walks one value against the ancestor stack.
fn holds_cycle_inner(value: &Value, stack: &mut Vec<usize>) -> bool {
    let Value::Table(table) = value else {
        return false;
    };
    let ptr = table.to_pointer() as usize;
    if stack.contains(&ptr) {
        return true;
    }
    stack.push(ptr);
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = match pair {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        if holds_cycle_inner(&key, stack) || holds_cycle_inner(&item, stack) {
            return true;
        }
    }
    stack.pop();
    false
}

/// Stamps a `__kind` marker and one extra marker on a table.
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

/// Renders one JSON value in canonical string form.
///
/// # Errors
///
/// - [`EngineError::RenderDetail`] for serializer failures.
pub(crate) fn json_text(value: &Json, scope: &Scope) -> mlua::Result<String> {
    serde_json::to_string(value).map_err(|error| {
        EngineError::RenderDetail {
            scope: scope.clone(),
            reason: error.to_string(),
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Lua {
        Lua::new()
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
        let error = match table.to_json(&Scope::method("ctx")) {
            Ok(_) => panic!("function passes"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("function not allowed"));
    }

    #[test]
    fn self_cycle_reports_true() {
        let lua = state();
        let table = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("table builds: {error}"),
        };
        if table.set("self", table.clone()).is_err() {
            panic!("entry stores");
        }
        assert!(holds_cycle(&Value::Table(table)));
    }

    #[test]
    fn nested_value_cycle_reports_true() {
        let lua = state();
        let first = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("first builds: {error}"),
        };
        let second = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("second builds: {error}"),
        };
        if first.set("link", second.clone()).is_err() {
            panic!("link stores");
        }
        if second.set("link", first.clone()).is_err() {
            panic!("backlink stores");
        }
        assert!(holds_cycle(&Value::Table(first)));
    }

    #[test]
    fn key_cycle_reports_true() {
        let lua = state();
        let root = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("root builds: {error}"),
        };
        let key = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("key builds: {error}"),
        };
        if key.set("back", root.clone()).is_err() {
            panic!("back stores");
        }
        if root.set(Value::Table(key), 1).is_err() {
            panic!("key stores");
        }
        assert!(holds_cycle(&Value::Table(root)));
    }

    #[test]
    fn sibling_shared_tables_read_clean_and_convert() {
        let lua = state();
        let shared = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("shared builds: {error}"),
        };
        if shared.set("x", 1).is_err() {
            panic!("field stores");
        }
        let base = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("base builds: {error}"),
        };
        if base.set("p", shared.clone()).is_err() {
            panic!("first ref stores");
        }
        if base.set("q", shared.clone()).is_err() {
            panic!("second ref stores");
        }
        assert!(!holds_cycle(&Value::Table(base.clone())));
        let json = match base.to_json(&Scope::method("ctx")) {
            Ok(json) => json,
            Err(error) => panic!("shared converts: {error}"),
        };
        assert!(matches!(json, Json::Object(_)));
    }

    #[test]
    fn recursive_table_fails_conversion_as_shape_error() {
        let lua = state();
        let table = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("table builds: {error}"),
        };
        if table.set("self", table.clone()).is_err() {
            panic!("entry stores");
        }
        let error = match table.to_json(&Scope::method("ctx")) {
            Ok(_) => panic!("cycle passes"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("recursive"));
    }
}
