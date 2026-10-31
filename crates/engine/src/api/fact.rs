//! Immediate host facts over closed names.

use confit_binding_macros::{Face, face_impl};
use confit_core::version::VERSION;
use mlua::{Lua, Value};

/// Closed host fact names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum FactName {
    /// Binary version triplet.
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
    /// - The host fact value.
    pub fn fact(lua: &Lua, name: FactName) -> mlua::Result<Value> {
        match name {
            FactName::Version => {
                let version = lua.create_sequence_from(VERSION)?;
                Ok(Value::Table(version))
            }
            FactName::Os => Ok(Value::String(lua.create_string(std::env::consts::OS)?)),
            FactName::Arch => Ok(Value::String(lua.create_string(std::env::consts::ARCH)?)),
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
            Ok(_) => panic!("fact names return strings"),
            Err(error) => panic!("fact names fold case: {error}"),
        }
    }

    #[test]
    fn version_reads_binary_triplet() {
        let lua = test_lua();
        match run(&lua, r#"return fact("version")"#) {
            Ok(Value::Table(found)) => {
                let major: u8 = found.get(1).unwrap();
                let minor: u8 = found.get(2).unwrap();
                let fix: u8 = found.get(3).unwrap();
                assert_eq!([major, minor, fix], VERSION, "version matches the binary");
            }
            _ => panic!("version reads a table"),
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
