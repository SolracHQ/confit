//! Text
//!
//! Template rendering over minijinja slots.

use std::collections::BTreeMap;

use mlua::{Lua, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::values::{plan_error, table_to_json};

/// Installs the text namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let text = lua.create_table()?;
    text.set(
        "render",
        lua.create_function(|_, args: (Value, Value)| render_impl(args))?,
    )?;
    confit.set("text", text)?;
    Ok(())
}

/// Renders one template string with a vars table.
fn render_impl(args: (Value, Value)) -> mlua::Result<String> {
    const CALLER: &str = "text.render";
    let (template, vars) = args;
    let template = match template {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_error(format!(
                "{CALLER}: field 'template' must be a string"
            )));
        }
    };
    let table = match vars {
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(format!(
                "{CALLER}: field 'vars' must be a table"
            )));
        }
    };
    let json = table_to_json(&table, "text.render field 'vars'")
        .map_err(|error| plan_error(format!("{CALLER}: {error}")))?;
    let facts: BTreeMap<String, Json> = match json {
        Json::Object(map) => map.into_iter().collect(),
        _ => {
            return Err(plan_error(format!(
                "{CALLER}: field 'vars' must be a table with string keys"
            )));
        }
    };
    render(&template, &facts, "text.render: ")
}

/// Renders one template string with JSON facts.
pub(crate) fn render(
    template: &str,
    facts: &BTreeMap<String, Json>,
    prefix: &str,
) -> mlua::Result<String> {
    let env = minijinja::Environment::new();
    env.render_str(template, facts)
        .map_err(|error| plan_error(format!("{prefix}{error}")))
}
