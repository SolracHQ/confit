//! Renders templates into text.

use mlua::{Lua, Value};

use lua_binding::lua_module;

pub struct Render;

#[lua_module("confit.render")]
impl Render {
    /// Renders one template string with plain vars.
    ///
    /// # Arguments
    ///
    /// - [template] The template text under rendering.
    /// - [vars] Plain vars for the template.
    ///
    /// # Returns
    ///
    /// - The rendered text, else nil.
    /// - The failure reason, else nil.
    pub fn render(
        lua: &Lua,
        template: String,
        vars: Value,
    ) -> mlua::Result<(Option<String>, Option<String>)> {
        use mlua::LuaSerdeExt as _;
        let context: serde_json::Value = match lua.from_value(vars) {
            Ok(found) => found,
            Err(error) => {
                return Ok((None, Some(format!("the template fails to render: {error}"))));
            }
        };
        let mut environment = minijinja::Environment::new();
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        let context = minijinja::Value::from(minijinja::value::Serde(context));
        match environment.render_str(&template, context) {
            Ok(text) => Ok((Some(text), None)),
            Err(error) => Ok((None, Some(format!("the template fails to render: {error}")))),
        }
    }
}
