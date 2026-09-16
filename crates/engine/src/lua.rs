//! Lua
//!
//! Lua plus JSON conversion plus marker helpers.

use std::collections::BTreeMap;

use mlua::{Function, Lua, Table, Value};
use serde_json::Value as Json;

use crate::error::plan_error;

/// Lua table shape plus conversion helpers.
///
pub(crate) trait TableExt {
    /// Converts one Lua table into JSON.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the field.
    ///
    /// # Returns
    ///
    /// JSON object or array matching the table shape.
    ///
    /// # Errors
    ///
    /// Recursive tables fail as plan errors. Non-string keys fail as plan errors.
    ///
    fn to_json(&self, ctx: &str) -> mlua::Result<Json>;

    /// Reports a dense Lua array shape.
    ///
    /// Mirrors conversion: dense integer keys from 1 read as arrays,
    /// everything else reads as objects.
    ///
    /// # Arguments
    ///
    /// * `table` - table under testing.
    ///
    /// # Returns
    ///
    /// True for dense arrays. False for objects, empties, and errors.
    fn is_array(&self) -> bool;

    /// Reads one named string field from a table.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned string contents.
    ///
    /// # Errors
    ///
    /// Missing plus non-string fields fail as plan errors.
    ///
    fn req_str(&self, ctx: &str, field: &str) -> mlua::Result<String>;

    /// Reads one named byte field from a table.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Raw byte contents without text conversion.
    ///
    /// # Errors
    ///
    /// Missing plus non-string fields fail as plan errors.
    ///
    fn req_bytes(&self, ctx: &str, field: &str) -> mlua::Result<Vec<u8>>;

    /// Reads one named table field from a table.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned table handle.
    ///
    /// # Errors
    ///
    /// Missing plus non-table fields fail as plan errors.
    ///
    fn req_table(&self, ctx: &str, field: &str) -> mlua::Result<Table>;

    /// Reads the table itself into a string-keyed object.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name naming the table.
    ///
    /// # Returns
    ///
    /// Object entries in canonical form.
    ///
    /// # Errors
    ///
    /// Recursive tables fail as plan errors. Non-object shapes fail as plan errors.
    ///
    fn req_object(&self, ctx: &str, field: &str) -> mlua::Result<BTreeMap<String, Json>>;

    /// Reads the table itself into a dense string array.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name naming the table.
    ///
    /// # Returns
    ///
    /// String items in 1-based index order.
    ///
    /// # Errors
    ///
    /// Non-integer keys fail as plan errors. Non-string items fail as plan errors.
    /// Sparse arrays fail as plan errors.
    ///
    fn req_string_array(&self, ctx: &str, field: &str) -> mlua::Result<Vec<String>>;
}

/// Lua value conversion helpers.
///
pub(crate) trait ValueExt {
    /// Converts one Lua value into JSON.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the field.
    ///
    /// # Returns
    ///
    /// JSON matching the data-only shape.
    ///
    /// # Errors
    ///
    /// Recursive tables fail as plan errors. Non-data values fail as plan errors.
    ///
    fn to_json(self, ctx: &str) -> mlua::Result<Json>;

    /// Reads one string value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned string contents.
    ///
    /// # Errors
    ///
    /// Non-string values fail as plan errors.
    ///
    fn req_str(self, ctx: &str, field: &str) -> mlua::Result<String>;

    /// Reads one table value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned table handle.
    ///
    /// # Errors
    ///
    /// Non-table values fail as plan errors.
    ///
    fn req_table(self, ctx: &str, field: &str) -> mlua::Result<Table>;

    /// Reads one function value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Cloned function handle.
    ///
    /// # Errors
    ///
    /// Non-function values fail as plan errors.
    ///
    fn req_func(self, ctx: &str, field: &str) -> mlua::Result<Function>;

    /// Reads one integer value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Integer contents.
    ///
    /// # Errors
    ///
    /// Non-integer values fail as plan errors.
    ///
    fn req_int(self, ctx: &str, field: &str) -> mlua::Result<i64>;

    /// Reads one dense string array value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// String items in 1-based index order.
    ///
    /// # Errors
    ///
    /// Non-table values fail as plan errors. Non-integer keys fail as plan errors.
    /// Non-string items fail as plan errors. Sparse arrays fail as plan errors.
    ///
    fn req_string_array(self, ctx: &str, field: &str) -> mlua::Result<Vec<String>>;

    /// Reads one raw byte value with a uniform shape error.
    ///
    /// # Arguments
    ///
    /// * `ctx` - error prefix naming the constructor.
    /// * `field` - field name under reading.
    ///
    /// # Returns
    ///
    /// Raw byte contents without text conversion.
    ///
    /// # Errors
    ///
    /// Non-string values fail as plan errors.
    ///
    fn req_bytes(self, ctx: &str, field: &str) -> mlua::Result<Vec<u8>>;

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
    /// * `lua` - state owning new strings plus tables.
    /// * `ctx` - error prefix naming the caller.
    ///
    /// # Returns
    ///
    /// Lua value matching the JSON shape.
    ///
    /// # Errors
    ///
    /// Non-finite numbers fail as plan errors.
    ///
    fn to_lua(&self, lua: &Lua, ctx: &str) -> mlua::Result<Value>;
}

impl TableExt for Table {
    fn to_json(&self, ctx: &str) -> mlua::Result<Json> {
        if holds_cycle(&Value::Table(self.clone())) {
            return Err(plan_error(format!("{ctx} holds a recursive table")));
        }
        table_to_json_inner(self, ctx)
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

    fn req_str(&self, ctx: &str, field: &str) -> mlua::Result<String> {
        let value: Value = self.get(field)?;
        value.req_str(ctx, field)
    }

    fn req_table(&self, ctx: &str, field: &str) -> mlua::Result<Table> {
        let value: Value = self.get(field)?;
        value.req_table(ctx, field)
    }

    fn req_bytes(&self, ctx: &str, field: &str) -> mlua::Result<Vec<u8>> {
        let value: Value = self.get(field)?;
        value.req_bytes(ctx, field)
    }

    fn req_object(&self, ctx: &str, field: &str) -> mlua::Result<BTreeMap<String, Json>> {
        let json = self
            .to_json(&format!("{ctx} field '{field}'"))
            .map_err(|error| plan_error(format!("{ctx}: {error}")))?;
        match json {
            Json::Object(map) => Ok(map.into_iter().collect()),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be a table with string keys"
            ))),
        }
    }

    fn req_string_array(&self, ctx: &str, field: &str) -> mlua::Result<Vec<String>> {
        let mut indexed: Vec<(i64, String)> = Vec::new();
        for pair in self.pairs::<Value, Value>() {
            let (key, value) = pair?;
            let index = key.req_int(ctx, field).map_err(|_| {
                plan_error(format!("{ctx}: field '{field}' must be a string array"))
            })?;
            let item = value.req_str(ctx, field).map_err(|_| {
                plan_error(format!(
                    "{ctx}: field '{field}' entry [{index}] must be a string"
                ))
            })?;
            indexed.push((index, item));
        }
        indexed.sort_by_key(|(index, _)| *index);
        for (position, (index, _)) in (1i64..).zip(indexed.iter()) {
            if *index != position {
                return Err(plan_error(format!(
                    "{ctx}: field '{field}' must be a dense string array starting at 1"
                )));
            }
        }
        Ok(indexed.into_iter().map(|(_, item)| item).collect())
    }
}

impl ValueExt for Value {
    fn to_json(self, ctx: &str) -> mlua::Result<Json> {
        if holds_cycle(&self) {
            return Err(plan_error(format!("{ctx} holds a recursive table")));
        }
        lua_to_json_inner(self, ctx)
    }

    fn req_str(self, ctx: &str, field: &str) -> mlua::Result<String> {
        match self {
            Value::String(text) => Ok(text.to_string_lossy()),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be a string"
            ))),
        }
    }

    fn req_table(self, ctx: &str, field: &str) -> mlua::Result<Table> {
        match self {
            Value::Table(table) => Ok(table),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be a table"
            ))),
        }
    }

    fn req_func(self, ctx: &str, field: &str) -> mlua::Result<Function> {
        match self {
            Value::Function(func) => Ok(func),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be a function"
            ))),
        }
    }

    fn req_int(self, ctx: &str, field: &str) -> mlua::Result<i64> {
        match self {
            Value::Integer(index) => Ok(index),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be an integer"
            ))),
        }
    }

    fn req_string_array(self, ctx: &str, field: &str) -> mlua::Result<Vec<String>> {
        self.req_table(ctx, field)?.req_string_array(ctx, field)
    }

    fn req_bytes(self, ctx: &str, field: &str) -> mlua::Result<Vec<u8>> {
        match self {
            Value::String(text) => Ok(text.as_bytes().to_vec()),
            _ => Err(plan_error(format!(
                "{ctx}: field '{field}' must be a string"
            ))),
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
    fn to_lua(&self, lua: &Lua, ctx: &str) -> mlua::Result<Value> {
        match self {
            Json::Null => Ok(Value::Nil),
            Json::Bool(flag) => Ok(Value::Boolean(*flag)),
            Json::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Ok(Value::Integer(integer))
                } else if let Some(float) = number.as_f64() {
                    Ok(Value::Number(float))
                } else {
                    Err(plan_error(format!("{ctx}: value must be a finite number")))
                }
            }
            Json::String(text) => Ok(Value::String(lua.create_string(text.as_str())?)),
            Json::Array(items) => {
                let table = lua.create_table()?;
                for (position, item) in items.iter().enumerate() {
                    table.set((position + 1) as i64, item.to_lua(lua, ctx)?)?;
                }
                Ok(Value::Table(table))
            }
            Json::Object(map) => {
                let table = lua.create_table()?;
                for (key, item) in map {
                    table.set(key.as_str(), item.to_lua(lua, ctx)?)?;
                }
                Ok(Value::Table(table))
            }
        }
    }
}

/// Converts one Lua value into JSON without a cycle precheck.
fn lua_to_json_inner(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(plan_error(format!("{ctx} must be a finite number"))),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json_inner(&table, ctx),
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

/// Orders dense array positions, else None.
///
/// Empty tables read as objects, matching conversion.
///
/// # Arguments
///
/// * `entries` - collected key plus value pairs.
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
fn table_to_json_inner(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if let Some(order) = dense_order(&entries) {
        let mut items = Vec::with_capacity(order.len());
        for (slot, position) in order.iter().enumerate() {
            let (_, value) = &entries[*position];
            items.push(lua_to_json_inner(
                value.clone(),
                &format!("{ctx}[{}]", slot + 1),
            )?);
        }
        return Ok(Json::Array(items));
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
        map.insert(name, lua_to_json_inner(value.clone(), &child)?);
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

/// Renders one JSON value in canonical string form.
pub(crate) fn json_text(value: &Json, ctx: &str) -> mlua::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| plan_error(format!("{ctx}: value failed to render: {error}")))
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
        let error = match table.to_json("ctx") {
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
        let json = match base.to_json("ctx") {
            Ok(json) => json,
            Err(error) => panic!("shared converts: {error}"),
        };
        assert!(matches!(json, Json::Object(_)));
    }

    #[test]
    fn recursive_table_fails_conversion_as_plan_error() {
        let lua = state();
        let table = match lua.create_table() {
            Ok(table) => table,
            Err(error) => panic!("table builds: {error}"),
        };
        if table.set("self", table.clone()).is_err() {
            panic!("entry stores");
        }
        let error = match table.to_json("ctx") {
            Ok(_) => panic!("cycle passes"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("recursive"));
    }
}
