//! Prompts one line.

use confit_events::EventHandles;
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
    /// - The input line, else nil.
    /// - The failure reason, else nil.
    pub fn input(lua: &Lua, prompt: String) -> (Option<String>, Option<String>) {
        let Some(handles) = lua.app_data_ref::<EventHandles>() else {
            return (None, Some("the confit input is not installed".to_owned()));
        };
        match handles.ask(prompt) {
            Some(line) => (Some(line), None),
            None => (None, Some("the input prompt was cancelled".to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_events::protocol::{FromUi, ToUi};
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
        let _ = lua.set_app_data(EventHandles::new(to_ui, from_ui));
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
        let source = r#"
            local line, reason = input("Name? ")
            assert(line == "hello", "the input line matches the answer")
            assert(reason == nil, "the reason stays nil on an answer")
            return line
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the input reader returns the line: {error}"),
        }
    }

    #[test]
    fn input_returns_nil_with_a_reason_on_cancellation() {
        let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        std::thread::spawn(move || {
            for message in incoming {
                if matches!(message, ToUi::Prompt(_)) && outgoing.send(FromUi::Close).is_err() {
                    break;
                }
            }
        });
        let lua = Lua::new();
        let _ = lua.set_app_data(EventHandles::new(to_ui, from_ui));
        let input = Input::add(&lua).unwrap();
        lua.globals().set("input", input).unwrap();
        let source = r#"
            local line, reason = input("Name? ")
            assert(line == nil, "the cancelled line stays nil")
            assert(reason ~= nil, "the cancellation carries a reason")
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the input reader keeps a cancellation soft: {error}"),
        }
    }
}
