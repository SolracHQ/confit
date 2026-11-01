//! Prompts one line.

use confit_ui::UiHandles;
use mlua::Lua;

use lua_binding::lua_module;

pub struct Input;

#[lua_module("confit.input")]
impl Input {
    /// Asks for one line.
    ///
    /// # Arguments
    ///
    /// - [prompt] The prompt text.
    ///
    /// # Returns
    ///
    /// - The input line.
    pub fn input(lua: &Lua, prompt: String) -> mlua::Result<String> {
        let handles = handles_of(lua)?;
        handles.ask(prompt).map_err(mlua::Error::external)
    }
}

/// Returns the renderer handles.
fn handles_of(lua: &Lua) -> mlua::Result<UiHandles> {
    match lua.app_data_ref::<UiHandles>() {
        Some(handles) => Ok(handles.clone()),
        None => Err(mlua::Error::RuntimeError(
            "the confit input is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_ui::{FromUi, ToUi};
    use mlua::Value;

    fn fresh_lua(replies: Vec<String>) -> Lua {
        let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        std::thread::spawn(move || {
            let mut replies = replies.into_iter();
            for message in incoming {
                if matches!(message, ToUi::Prompt(_)) {
                    let line = replies.next().unwrap_or_default();
                    if outgoing.send(FromUi::Answer(line)).is_err() {
                        break;
                    }
                }
            }
        });
        let lua = Lua::new();
        let _ = lua.set_app_data(UiHandles { to_ui, from_ui });
        let input = Input::add(&lua).unwrap();
        lua.globals().set("input", input).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn input_returns_the_asked_line() {
        let lua = fresh_lua(vec!["hello".to_owned()]);
        match run(&lua, r#"return input("Name? ")"#) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the input reader returns the line: {error}"),
        }
    }
}
