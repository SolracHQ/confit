//! Facts
//! Immediate host facts over closed names.

use confit_binding_macros::{Face, face_impl};

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

/// Fact namespace.
pub struct Fact;

#[face_impl(namespace = "confit.fact")]
impl Fact {
    /// Read one host fact.
    ///
    /// # Arguments
    ///
    /// - [name] The fact name under reading.
    ///
    /// # Returns
    ///
    /// - The host fact string.
    pub fn fact(name: FactName) -> mlua::Result<(String,)> {
        match name {
            FactName::Version => Ok((API_VERSION.to_string(),)),
            FactName::Os => Ok((std::env::consts::OS.to_owned(),)),
            FactName::Arch => Ok((std::env::consts::ARCH.to_owned(),)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Value};

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let fact = Fact::register(&lua).unwrap();
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
        assert!(
            run(&lua, r#"return fact("shell")"#).is_err(),
            "foreign fact fails"
        );
    }
}
