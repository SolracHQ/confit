//! Shell
//!
//! Condition constructors for Lua over shell-session facts.
//! Prefer plain Lua tables so the Lua dialect can build and inspect them without registry machinery.

use mlua::{Lua, Table, Value};
use serde::Deserialize;
use serde_json::Value as Json;

use super::confit_table;
use crate::error::Error;

/// Leaf `env_eq` opts in Lua shape.
///
/// Fields stay `Json` so type mistakes keep the hand-checked field
/// errors below; `deny_unknown_fields` rejects unknown keys as plan
/// errors naming the key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvEqOpts {
    /// Holds the variable name value.
    #[serde(default)]
    key: Option<Json>,
    /// Holds the expected value.
    #[serde(default)]
    value: Option<Json>,
}

/// Leaf `env_set` opts in Lua shape.
///
/// The field stays `Json` so type mistakes keep the hand-checked field
/// error below; `deny_unknown_fields` rejects unknown keys as plan
/// errors naming the key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvSetOpts {
    /// Holds the variable name value.
    #[serde(default)]
    key: Option<Json>,
}

/// Installs the shell namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
///
/// # Examples
///
/// ```rust
/// use confit::framework::shell::install;
/// use mlua::{Lua, Value};
///
/// let lua = Lua::new();
/// assert!(install(&lua).is_ok());
/// let result: mlua::Result<Value> =
///     lua.load(r#"return confit.shell.env_eq({key = "A", value = "b"})"#).eval();
/// assert!(result.is_ok());
/// assert!(matches!(&result, Ok(Value::Table(_))));
/// ```
pub fn install(lua: &Lua) -> mlua::Result<()> {
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

/// Builds a plan error naming constructor plus field.
fn field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    mlua::Error::external(Error::Plan(format!("{ctor}: field '{field}' {detail}")))
}

/// Reads a string from a Lua value.
fn take_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(ctor, field, "must be a string")),
    }
}

/// Builds an `env_eq` condition table.
///
/// Leaf opts parse through `EnvEqOpts` with denied unknown fields fed by
/// the shared table conversion, so unknown keys fail as plan errors
/// naming the key. Nested shapes (`all`, `any`, `nop`) stay hand parsed:
/// serde fights the recursive single-shape dispatch, so manual parsing
/// keeps the errors readable.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `opts` - raw opts value.
///
/// # Returns
///
/// Condition table shaped `{ env_eq = { key = "..", value = ".." } }`.
///
/// # Errors
///
/// Fails with plan errors for opts holding values of other shapes, for
/// unknown keys, and for fields holding values of other shapes.
fn env_eq_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.env_eq";
    let table = match opts {
        Value::Table(table) => table,
        _ => return Err(field_error(CTOR, "opts", "must be a table")),
    };
    let ctx = format!("{CTOR}: field 'opts'");
    let parsed: EnvEqOpts = serde_json::from_value(super::document::table_to_json(&table, &ctx)?)
        .map_err(|err| field_error(CTOR, "opts", &format!("{err}")))?;
    let key = match parsed.key {
        Some(Json::String(key)) => key,
        _ => return Err(field_error(CTOR, "key", "must be a string")),
    };
    let value = match parsed.value {
        Some(Json::String(value)) => value,
        _ => return Err(field_error(CTOR, "value", "must be a string")),
    };
    let inner = lua.create_table()?;
    inner.set("key", key)?;
    inner.set("value", value)?;
    let outer = lua.create_table()?;
    outer.set("env_eq", inner)?;
    Ok(outer)
}

/// Builds an `env_set` condition table.
///
/// Leaf opts parse through `EnvSetOpts` with denied unknown fields, as
/// `env_eq` does above.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `opts` - raw opts value.
///
/// # Returns
///
/// Condition table shaped `{ env_set = { key = ".." } }`.
///
/// # Errors
///
/// Fails with plan errors for opts holding values of other shapes, for
/// unknown keys, and for fields holding values of other shapes.
fn env_set_impl(lua: &Lua, opts: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.env_set";
    let table = match opts {
        Value::Table(table) => table,
        _ => return Err(field_error(CTOR, "opts", "must be a table")),
    };
    let ctx = format!("{CTOR}: field 'opts'");
    let parsed: EnvSetOpts = serde_json::from_value(super::document::table_to_json(&table, &ctx)?)
        .map_err(|err| field_error(CTOR, "opts", &format!("{err}")))?;
    let key = match parsed.key {
        Some(Json::String(key)) => key,
        _ => return Err(field_error(CTOR, "key", "must be a string")),
    };
    let inner = lua.create_table()?;
    inner.set("key", key)?;
    let outer = lua.create_table()?;
    outer.set("env_set", inner)?;
    Ok(outer)
}

/// Builds an `in_path` condition table from a binary name.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `name` - raw binary name value.
///
/// # Returns
///
/// Condition table shaped `{ in_path = { name = ".." } }`.
///
/// # Errors
///
/// Fails with plan errors for names holding values of other shapes.
fn in_path_impl(lua: &Lua, name: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.in_path";
    let name = take_string(name, CTOR, "name")?;
    let inner = lua.create_table()?;
    inner.set("name", name)?;
    let outer = lua.create_table()?;
    outer.set("in_path", inner)?;
    Ok(outer)
}

/// Builds an `exists` condition table from a path.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `path` - raw path value.
///
/// # Returns
///
/// Condition table shaped `{ exists = { path = ".." } }`.
///
/// # Errors
///
/// Fails with plan errors for paths holding values of other shapes.
fn exists_impl(lua: &Lua, path: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.exists";
    let path = take_string(path, CTOR, "path")?;
    let inner = lua.create_table()?;
    inner.set("path", path)?;
    let outer = lua.create_table()?;
    outer.set("exists", inner)?;
    Ok(outer)
}

/// Builds an `all` condition table.
fn all_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.all";
    let table = match conds {
        Value::Table(table) => table,
        _ => return Err(field_error(CTOR, "conds", "must be a table")),
    };
    let items = take_conditions(&table, CTOR, "conds")?;
    let array = lua.create_table()?;
    for (position, item) in items.into_iter().enumerate() {
        let index = position + 1;
        array.set(index, item)?;
    }
    let outer = lua.create_table()?;
    outer.set("all", array)?;
    Ok(outer)
}

/// Builds an `any` condition table.
fn any_impl(lua: &Lua, conds: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.any";
    let table = match conds {
        Value::Table(table) => table,
        _ => return Err(field_error(CTOR, "conds", "must be a table")),
    };
    let items = take_conditions(&table, CTOR, "conds")?;
    let array = lua.create_table()?;
    for (position, item) in items.into_iter().enumerate() {
        let index = position + 1;
        array.set(index, item)?;
    }
    let outer = lua.create_table()?;
    outer.set("any", array)?;
    Ok(outer)
}

/// Builds a `nop` condition table.
fn nop_impl(lua: &Lua, cond: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.shell.nop";
    validate_condition(&cond, CTOR, "cond")?;
    let outer = lua.create_table()?;
    outer.set("nop", cond)?;
    Ok(outer)
}

/// Collects nested condition tables in index order.
fn take_conditions(table: &Table, ctor: &str, field: &str) -> mlua::Result<Vec<Table>> {
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::Integer(index) => indexed.push((index, value)),
            _ => {
                return Err(field_error(
                    ctor,
                    field,
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
                field,
                "must be a dense condition array starting at 1",
            ));
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (index, value) in indexed {
        let nested = format!("{field}[{index}]");
        validate_condition(&value, ctor, &nested)?;
        match value {
            Value::Table(item) => out.push(item),
            _ => return Err(field_error(ctor, &nested, "must be a condition table")),
        }
    }
    Ok(out)
}

/// Validates a value as one condition table.
fn validate_condition(value: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match value {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "must be a condition table")),
    };
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if entries.len() != 1 {
        return Err(field_error(
            ctor,
            field,
            "must be a condition table with one shape",
        ));
    }
    let (key, inner) = &entries[0];
    let name = match key {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(field_error(
                ctor,
                field,
                "must be a condition table with one shape",
            ));
        }
    };
    match name.as_str() {
        "env_eq" => validate_env_eq(inner, ctor, field),
        "env_set" => validate_env_set(inner, ctor, field),
        "in_path" => validate_in_path(inner, ctor, field),
        "exists" => validate_exists(inner, ctor, field),
        "all" => validate_condition_array(inner, ctor, &format!("{field}.all")),
        "any" => validate_condition_array(inner, ctor, &format!("{field}.any")),
        "nop" => validate_condition(inner, ctor, &format!("{field}.nop")),
        _ => Err(field_error(
            ctor,
            field,
            "must be a condition table with one shape",
        )),
    }
}

/// Validates an `env_eq` payload table.
fn validate_env_eq(inner: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match inner {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "entry 'env_eq' must be a table")),
    };
    let key: Value = table.get("key")?;
    let value: Value = table.get("value")?;
    match key {
        Value::String(_) => (),
        _ => {
            return Err(field_error(
                ctor,
                field,
                "entry 'env_eq' field 'key' must be a string",
            ));
        }
    }
    match value {
        Value::String(_) => Ok(()),
        _ => Err(field_error(
            ctor,
            field,
            "entry 'env_eq' field 'value' must be a string",
        )),
    }
}

/// Validates an `env_set` payload table.
fn validate_env_set(inner: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match inner {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "entry 'env_set' must be a table")),
    };
    let key: Value = table.get("key")?;
    match key {
        Value::String(_) => Ok(()),
        _ => Err(field_error(
            ctor,
            field,
            "entry 'env_set' field 'key' must be a string",
        )),
    }
}

/// Validates an `in_path` payload table.
///
/// # Arguments
///
/// * `inner` - raw payload value.
/// * `ctor` - constructor name for errors.
/// * `field` - field name for errors.
///
/// # Errors
///
/// Fails with plan errors for payloads holding values of other shapes.
fn validate_in_path(inner: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match inner {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "entry 'in_path' must be a table")),
    };
    let name: Value = table.get("name")?;
    match name {
        Value::String(_) => Ok(()),
        _ => Err(field_error(
            ctor,
            field,
            "entry 'in_path' field 'name' must be a string",
        )),
    }
}

/// Validates an `exists` payload table.
///
/// # Arguments
///
/// * `inner` - raw payload value.
/// * `ctor` - constructor name for errors.
/// * `field` - field name for errors.
///
/// # Errors
///
/// Fails with plan errors for payloads holding values of other shapes.
fn validate_exists(inner: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match inner {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "entry 'exists' must be a table")),
    };
    let path: Value = table.get("path")?;
    match path {
        Value::String(_) => Ok(()),
        _ => Err(field_error(
            ctor,
            field,
            "entry 'exists' field 'path' must be a string",
        )),
    }
}

/// Validates an `all` or `any` payload array.
fn validate_condition_array(inner: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    let table = match inner {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "must be a condition array")),
    };
    take_conditions(table, ctor, field).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a Lua state holding the shell namespace.
    fn setup() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install");
        lua
    }

    /// Reports plan domain status for a Lua failure.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(domain) = err.downcast_ref::<Error>() {
            return matches!(domain, Error::Plan(_));
        }
        format!("{err}").contains("plan error:")
    }

    #[test]
    fn shell_constant_holds_template_slot() {
        let lua = setup();
        let slot: String = lua
            .load(r#"return confit.shell.SHELL"#)
            .eval()
            .expect("slot");
        assert_eq!(slot, "{{shell}}");
    }

    #[test]
    fn env_eq_builds_tagged_table() {
        let lua = setup();
        let key: String = lua
            .load(r#"return confit.shell.env_eq({key = "A", value = "b"}).env_eq.key"#)
            .eval()
            .expect("key");
        assert_eq!(key, "A");
        let value: String = lua
            .load(r#"return confit.shell.env_eq({key = "A", value = "b"}).env_eq.value"#)
            .eval()
            .expect("value");
        assert_eq!(value, "b");
    }

    #[test]
    fn env_set_builds_tagged_table() {
        let lua = setup();
        let key: String = lua
            .load(r#"return confit.shell.env_set({key = "SSH_TTY"}).env_set.key"#)
            .eval()
            .expect("key");
        assert_eq!(key, "SSH_TTY");
    }

    #[test]
    fn in_path_and_exists_build_tagged_tables() {
        let lua = setup();
        let name: String = lua
            .load(r#"return confit.shell.in_path("bat").in_path.name"#)
            .eval()
            .expect("name");
        assert_eq!(name, "bat");
        let path: String = lua
            .load(r#"return confit.shell.exists("/bin").exists.path"#)
            .eval()
            .expect("path");
        assert_eq!(path, "/bin");
        let nested: String = lua
            .load(
                r#"local s = confit.shell
                local c = s.all({s.in_path("bat"), s.exists("/bin")})
                return c.all[2].exists.path"#,
            )
            .eval()
            .expect("nested");
        assert_eq!(nested, "/bin");
    }

    #[test]
    fn all_any_nop_nest_sibling_tables() {
        let lua = setup();
        let first: String = lua
            .load(
                r#"local s = confit.shell
                local c = s.all({s.env_eq({key = "A", value = "b"}), s.env_set({key = "C"})})
                return c.all[1].env_eq.key"#,
            )
            .eval()
            .expect("all first");
        assert_eq!(first, "A");
        let second: String = lua
            .load(
                r#"local s = confit.shell
                local c = s.any({s.env_eq({key = "A", value = "b"}), s.env_set({key = "C"})})
                return c.any[2].env_set.key"#,
            )
            .eval()
            .expect("any second");
        assert_eq!(second, "C");
        let negated: String = lua
            .load(
                r#"local s = confit.shell
                return s.nop(s.env_set({key = "C"})).nop.env_set.key"#,
            )
            .eval()
            .expect("nop");
        assert_eq!(negated, "C");
    }

    #[test]
    fn leaf_mistakes_fail_as_named_plan_errors() {
        let lua = setup();
        for (ctor, field, expr) in [
            (
                "confit.shell.env_eq",
                "key",
                r#"return confit.shell.env_eq({value = "b"})"#,
            ),
            (
                "confit.shell.env_eq",
                "value",
                r#"return confit.shell.env_eq({key = "A", value = 42})"#,
            ),
            (
                "confit.shell.env_eq",
                "opts",
                r#"return confit.shell.env_eq(42)"#,
            ),
            (
                "confit.shell.env_set",
                "key",
                r#"return confit.shell.env_set({})"#,
            ),
            (
                "confit.shell.env_set",
                "key",
                r#"return confit.shell.env_set({key = 42})"#,
            ),
            (
                "confit.shell.in_path",
                "name",
                r#"return confit.shell.in_path(42)"#,
            ),
            (
                "confit.shell.exists",
                "path",
                r#"return confit.shell.exists({})"#,
            ),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            let message = format!("{err}");
            assert!(message.contains(ctor), "names {ctor}: {expr}: {err}");
            assert!(message.contains(field), "names {field}: {expr}: {err}");
        }
    }

    #[test]
    fn nesting_mistakes_fail_as_named_plan_errors() {
        let lua = setup();
        for (ctor, field, expr) in [
            (
                "confit.shell.all",
                "conds",
                r#"return confit.shell.all(42)"#,
            ),
            (
                "confit.shell.all",
                "conds[1]",
                r#"return confit.shell.all({"nope"})"#,
            ),
            (
                "confit.shell.any",
                "conds",
                r#"return confit.shell.any({key = "A"})"#,
            ),
            (
                "confit.shell.any",
                "conds[2]",
                r#"local s = confit.shell return s.any({s.env_set({key = "A"}), 42})"#,
            ),
            ("confit.shell.nop", "cond", r#"return confit.shell.nop(42)"#),
            (
                "confit.shell.nop",
                "cond",
                r#"return confit.shell.nop({bogus = {}})"#,
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
