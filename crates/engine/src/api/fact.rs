//! Returns the host facts.

use confit_core::version::VERSION;
use lua_binding::{Shape, lua_module};
use mlua::{Lua, Value};

/// The host facts available to query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Shape)]
#[shape(table, str_alias)]
pub enum FactName {
    /// The binary version triplet.
    Version,
    /// The host operating system name.
    Os,
    /// The host processor architecture name.
    Arch,
}

pub struct Fact;

#[lua_module("confit.fact")]
impl Fact {
    /// Returns one host fact.
    ///
    /// # Arguments
    ///
    /// - [name] A fact name in any letter case.
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

    fn fresh_lua() -> Lua {
        let lua = Lua::new();
        let fact = Fact::add(&lua).unwrap();
        lua.globals().set("fact", fact).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn fact_names_match_in_any_case() {
        let lua = fresh_lua();
        match run(&lua, r#"return fact("OS")"#) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), std::env::consts::OS),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the fact reader matches the case: {error}"),
        }
    }

    #[test]
    fn version_returns_the_binary_triplet() {
        let lua = fresh_lua();
        match run(&lua, r#"return fact("version")"#) {
            Ok(Value::Table(found)) => {
                let major: u8 = found.get(1).unwrap();
                let minor: u8 = found.get(2).unwrap();
                let fix: u8 = found.get(3).unwrap();
                assert_eq!(
                    [major, minor, fix],
                    VERSION,
                    "the version matches the binary"
                );
            }
            _ => panic!("expected a table"),
        }
    }

    #[test]
    fn fact_rejects_unknown_names() {
        let lua = fresh_lua();
        assert!(
            run(&lua, r#"return fact("shell")"#).is_err(),
            "the fact reader rejects an unknown name"
        );
    }
}
