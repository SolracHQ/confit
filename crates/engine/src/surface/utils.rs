//! Utils
//!
//! Template rendering and table shape checks over Lua values.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::error::{EngineError, Scope};
use crate::lua::{TableExt, ValueExt, holds_cycle};

/// Installs the utils namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let utils = lua.create_table()?;
    utils.set(
        "render",
        lua.create_function(|_, args: (Value, Value)| render_impl(args))?,
    )?;
    utils.set(
        "holds_cycle",
        lua.create_function(|_, value: Value| Ok(holds_cycle(&value)))?,
    )?;
    utils.set(
        "is_array",
        lua.create_function(|_, value: Value| {
            Ok(matches!(value, Value::Table(table) if table.is_array()))
        })?,
    )?;
    confit.set("utils", utils)?;
    Ok(())
}

/// Renders one template string with a vars table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
/// - [`EngineError::NestScope`] for template failures.
fn render_impl(args: (Value, Value)) -> mlua::Result<String> {
    const CALLER: &str = "confit.utils.render";
    let scope = Scope::method(CALLER);
    let (template_value, vars_value) = args;
    let template = template_value.req_str(&scope, "template")?;
    let vars = vars_value.req_table(&scope, "vars")?;
    TextRender::render(&scope, template, vars)
}

/// Template renderer holding domain validation.
struct TextRender;

impl TextRender {
    /// Renders one template string with a vars table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::NestScope`] for template failures.
    ///
    fn render(scope: &Scope, template: String, vars: Table) -> mlua::Result<String> {
        let facts = vars.req_object(scope, "vars")?;
        render(&template, &facts, scope)
    }
}

/// Renders one template string with JSON facts.
///
/// # Errors
///
/// - [`EngineError::NestScope`] for template failures.
pub(crate) fn render(
    template: &str,
    facts: &BTreeMap<String, Json>,
    scope: &Scope,
) -> mlua::Result<String> {
    let env = minijinja::Environment::new();
    env.render_str(template, facts).map_err(|error| {
        EngineError::NestScope {
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

    fn install_state() -> Lua {
        let lua = state();
        match install(&lua) {
            Ok(()) => lua,
            Err(error) => panic!("utils install: {error}"),
        }
    }

    fn eval_bool(lua: &Lua, script: &str) -> bool {
        match lua.load(script).eval::<bool>() {
            Ok(flag) => flag,
            Err(error) => panic!("script evaluates: {error}"),
        }
    }

    #[test]
    fn sparse_list_reads_false_through_lua() {
        let lua = install_state();
        let array = eval_bool(
            &lua,
            r#"local t = {} t[1] = "a" t[3] = "c" return confit.utils.is_array(t)"#,
        );
        assert!(!array);
    }
}
