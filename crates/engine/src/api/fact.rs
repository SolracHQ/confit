//! Facts
//! Immediate host facts over closed names.

use confit_binding_macros::Face;

/// Api level of the Lua surface.
pub const API_VERSION: i64 = 1;

/// Closed host fact names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum FactName {
    /// Api level of the Lua surface.
    Version,
    /// Host operating system name.
    Os,
    /// Host processor architecture name.
    Arch,
}

confit_binding::verb! {
    /// Reads one host fact into its string.
    fn fact(
        /// The fact name under reading.
        name: FactName,
    ) -> (
        /// The host fact string.
        String,
    ) [call = fact_impl]
}

confit_binding::module! {
    namespace = "confit.fact",
    verb = fact,
}

/// Read one host fact into its string.
fn fact_impl(name: FactName) -> mlua::Result<(String,)> {
    match name {
        FactName::Version => Ok((API_VERSION.to_string(),)),
        FactName::Os => Ok((std::env::consts::OS.to_owned(),)),
        FactName::Arch => Ok((std::env::consts::ARCH.to_owned(),)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Value};

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let fact = Module::register(&lua).unwrap();
        lua.globals().set("fact", fact).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn names_match_case_insensitively() {
        let lua = test_lua();
        match run(&lua, r#"return fact("OS")"#) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), std::env::consts::OS),
            Ok(_) => panic!("fact names answer strings"),
            Err(error) => panic!("fact names fold case: {error}"),
        }
    }

    #[test]
    fn unknown_names_raise_showing_want() {
        let lua = test_lua();
        match run(&lua, r#"return fact("shell")"#) {
            Ok(_) => panic!("foreign fact passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("FactName"), "the text shows the face: {text}");
            }
        }
    }
}
