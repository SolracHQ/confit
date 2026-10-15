//! Userdata
//! Method glue for bound userdata handles.

use std::borrow::Cow;

use crate::registry::Registry;

/// One userdata method parameter with its Lua face.
pub struct MethodParameter {
    /// The Lua parameter name.
    pub name: &'static str,
    /// The parameter face expression.
    pub face: Cow<'static, str>,
    /// The parameter documentation.
    pub documentation: &'static str,
}

/// One userdata method return with its Lua face.
pub struct MethodReturn {
    /// The return face expression.
    pub face: Cow<'static, str>,
    /// The return documentation.
    pub documentation: &'static str,
}

/// One userdata method with its Lua faces.
pub struct MethodDescriptor {
    /// The method name.
    pub name: &'static str,
    /// The method documentation.
    pub documentation: &'static str,
    /// The method parameters in declaration order.
    pub parameters: Vec<MethodParameter>,
    /// The method return faces in order.
    pub returns: Vec<MethodReturn>,
}

/// Bound userdata class.
///
/// - Every implementor answers one stable class name.
/// - Every implementor answers one stub block.
/// - Collection registers the class with every
///   referenced face exactly once.
pub trait UserdataClass: mlua::UserData {
    /// Return the class name.
    fn class_name() -> &'static str;
    /// Return the stub block.
    fn class_stub() -> String;
    /// Return the collected method descriptors.
    fn methods() -> Vec<MethodDescriptor>;
    /// Register the class with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding_macros::{Face, face_impl};
    use mlua::Lua;

    /// Roster handle for method tests.
    #[derive(Clone, Face)]
    #[face(userdata)]
    struct Roster {
        seed: String,
    }

    #[face_impl]
    impl Roster {
        /// List members with the prefix.
        ///
        /// # Arguments
        ///
        /// - [prefix] Filter members by prefix.
        ///
        /// # Returns
        ///
        /// - Member names with the seed first.
        pub fn members(&self, lua: &Lua, prefix: String) -> mlua::Result<Vec<String>> {
            let _ = lua;
            Ok(vec![self.seed.clone(), prefix])
        }

        /// Answer the seed.
        ///
        /// # Returns
        ///
        /// - The stored seed.
        pub fn seed_name(&self) -> String {
            self.seed.clone()
        }
    }

    #[test]
    fn generated_methods_answer_calls_through_lua() {
        let methods = Roster::methods();
        let names: Vec<&str> = methods.iter().map(|method| method.name).collect();
        assert_eq!(names, vec!["members", "seed_name"]);
        assert_eq!(
            methods[0]
                .parameters
                .iter()
                .map(|parameter| parameter.name)
                .collect::<Vec<_>>(),
            vec!["prefix"]
        );
        assert_eq!(methods[0].parameters[0].face, "string");
        assert_eq!(
            methods[0].parameters[0].documentation,
            "Filter members by prefix."
        );
        assert_eq!(methods[0].returns.len(), 1);
        assert_eq!(methods[0].returns[0].face, "string[]");
        assert_eq!(
            methods[0].returns[0].documentation,
            "Member names with the seed first."
        );
        assert!(methods[1].parameters.is_empty());
        assert_eq!(methods[1].returns.len(), 1);
        assert_eq!(methods[1].returns[0].face, "string");
        assert_eq!(methods[1].returns[0].documentation, "The stored seed.");
        let lua = Lua::new();
        lua.globals()
            .set(
                "roster",
                Roster {
                    seed: "base".to_owned(),
                },
            )
            .unwrap();
        let members: Vec<String> = lua.load("return roster:members('ab')").eval().unwrap();
        assert_eq!(members, vec!["base".to_owned(), "ab".to_owned()]);
        let seed: String = lua.load("return roster:seed_name()").eval().unwrap();
        assert_eq!(seed, "base".to_owned());
    }

    #[test]
    fn generated_methods_name_bad_params() {
        let lua = Lua::new();
        lua.globals()
            .set(
                "roster",
                Roster {
                    seed: "base".to_owned(),
                },
            )
            .unwrap();
        let failure = lua
            .load("return roster:members(42)")
            .eval::<Vec<String>>()
            .unwrap_err();
        assert!(failure.to_string().contains("members.prefix"));
        let missing = lua
            .load("return roster:members()")
            .eval::<Vec<String>>()
            .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("missing param 'members.prefix'")
        );
    }
}
