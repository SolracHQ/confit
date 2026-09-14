//! Text
//!
//! Template rendering for Lua over the shared minijinja helper.

use std::collections::BTreeMap;

use mlua::{Lua, Value};
use serde_json::Value as Json;

use super::confit_table;

/// Installs the text namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
///
/// # Returns
///
/// Always succeeds.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let text = lua.create_table()?;
    text.set(
        "render",
        lua.create_function(|lua, args: (Value, Value)| render_impl(lua, args))?,
    )?;
    confit.set("text", text)?;
    Ok(())
}

/// Renders a template string with a vars table.
///
/// # Arguments
///
/// * `_lua` - state, unused for pure rendering.
/// * `args` - template string plus vars table.
///
/// # Returns
///
/// Rendered text.
///
/// # Errors
///
/// Fails with Lua errors for arguments of other shapes and with plan
/// errors for template syntax failures.
fn render_impl(_lua: &Lua, args: (Value, Value)) -> mlua::Result<String> {
    const CALLER: &str = "text.render";
    let (template, vars) = args;
    let template = match template {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(lua_err!("{CALLER}: field 'template' must be a string"));
        }
    };
    let table = match vars {
        Value::Table(table) => table,
        _ => {
            return Err(lua_err!("{CALLER}: field 'vars' must be a table"));
        }
    };
    let json = super::document::table_to_json(&table, "text.render field 'vars'")?;
    let facts: BTreeMap<String, Json> = match json {
        Json::Object(map) => map.into_iter().collect(),
        _ => {
            return Err(lua_err!(
                "{CALLER}: field 'vars' must be a table with string keys"
            ));
        }
    };
    crate::services::render::render_str(&template, &facts, "text.render: ")
        .map_err(mlua::Error::external)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Error;

    /// Builds a Lua state holding the text namespace.
    ///
    /// # Returns
    ///
    /// Lua state holding the namespace.
    fn setup() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install");
        lua
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
        if let Some(Error::Plan(_)) = err.downcast_ref::<Error>() {
            return true;
        }
        format!("{err}").contains("plan error:")
    }

    #[test]
    fn render_fills_slots() {
        let lua = setup();
        let out: String = lua
            .load(r#"return confit.text.render("hi {{ name }}", { name = "ada" })"#)
            .eval()
            .expect("evaluate");
        assert_eq!(out, "hi ada");
    }

    #[test]
    fn syntax_failure_is_plan_error() {
        let lua = setup();
        let err = lua
            .load(r#"return confit.text.render("{{ unclosed", {})"#)
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&err), "syntax is plan: {err}");
    }

    #[test]
    fn bad_shapes_are_lua_errors() {
        let lua = setup();
        for expr in [
            r#"return confit.text.render(42, {})"#,
            r#"return confit.text.render("hi", 42)"#,
            r#"return confit.text.render("hi {{ f() }}", { f = function() end })"#,
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(
                !is_plan_error(&err),
                "shape mistakes stay Lua domain: {expr}: {err}"
            );
        }
    }
}
