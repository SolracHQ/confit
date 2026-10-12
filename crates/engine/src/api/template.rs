//! Templates rendered into text.

use mlua::{Lua, Value};

confit_binding::verb! {
    /// Renders one template string with plain vars.
    fn render(
        lua: &Lua,
        /// The template text under rendering.
        template: String,
        /// Plain vars for the template.
        vars: Value,
    ) -> (
        /// The rendered text, else nil.
        Option<String>,
        /// The failure reason, else nil.
        Option<String>,
    ) [call = render_impl]
}

confit_binding::module! {
    namespace = "confit.template",
    verb = render,
}

/// Render one template string with plain vars.
///
/// Missing vars render empty. Syntax failures take nil with reason.
///
/// # Errors
///
/// - [`mlua::Error`] for uses outside installed stores.
fn render_impl(
    lua: &Lua,
    template: String,
    vars: Value,
) -> mlua::Result<(Option<String>, Option<String>)> {
    use mlua::LuaSerdeExt as _;
    let context: serde_json::Value = match lua.from_value(vars) {
        Ok(found) => found,
        Err(error) => {
            return Ok((None, Some(format!("cannot render template: {error}"))));
        }
    };
    let mut environment = minijinja::Environment::new();
    environment.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    let context = minijinja::Value::from(minijinja::value::Serde(context));
    match environment.render_str(&template, context) {
        Ok(text) => Ok((Some(text), None)),
        Err(error) => Ok((None, Some(format!("cannot render template: {error}")))),
    }
}
