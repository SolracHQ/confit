//! Userdata
//! Method glue for bound userdata handles.

use std::borrow::Cow;

use crate::error::Result;
use crate::registry::Registry;

/// Collect one face with every referenced face.
pub type CollectFn = fn(&mut Registry) -> Result<()>;

/// One userdata method parameter with its Lua face.
#[derive(Clone, Debug)]
pub struct MethodParameter {
    /// The Lua parameter name.
    pub name: &'static str,
    /// The parameter face expression.
    pub face: Cow<'static, str>,
    /// The parameter documentation.
    pub documentation: &'static str,
    /// The face collector.
    pub collect: CollectFn,
}

impl PartialEq for MethodParameter {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.face == other.face
            && self.documentation == other.documentation
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One userdata method return with its Lua face.
#[derive(Clone, Debug)]
pub struct MethodReturn {
    /// The return face expression.
    pub face: Cow<'static, str>,
    /// The return documentation.
    pub documentation: &'static str,
    /// The face collector.
    pub collect: CollectFn,
}

impl PartialEq for MethodReturn {
    fn eq(&self, other: &Self) -> bool {
        self.face == other.face
            && self.documentation == other.documentation
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One userdata method with its Lua faces.
#[derive(Clone, Debug, PartialEq)]
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

impl MethodDescriptor {
    /// Collect every referenced face.
    ///
    /// - Params in order, returns after.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    pub fn collect_referenced(&self, collector: &mut Registry) -> Result<()> {
        for parameter in &self.parameters {
            (parameter.collect)(collector)?;
        }
        for returned in &self.returns {
            (returned.collect)(collector)?;
        }
        Ok(())
    }
}

/// Bound userdata class.
///
/// - Every implementor answers one stable class name.
/// - Every implementor answers its method descriptors.
/// - Collection registers the class with every
///   referenced face exactly once.
pub trait UserdataClass: mlua::UserData {
    /// Return the class name.
    fn class_name() -> &'static str;
    /// Return the collected method descriptors.
    fn methods() -> Vec<MethodDescriptor>;
    /// Register the class with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> Result<()>;
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
        assert!(
            lua.load("return roster:members(42)")
                .eval::<Vec<String>>()
                .is_err(),
            "foreign param fails"
        );
        assert!(
            lua.load("return roster:members()")
                .eval::<Vec<String>>()
                .is_err(),
            "missing param fails"
        );
    }

    /// Cache fixture.
    #[derive(Clone, Debug, PartialEq, Face)]
    #[face(userdata)]
    enum CacheHandle {
        Cached(String),
        Missing,
    }

    #[face_impl]
    impl CacheHandle {
        /// Answer a constant.
        ///
        /// # Returns
        ///
        /// - Constant value.
        pub fn tag(&self) -> u64 {
            7
        }
    }

    #[test]
    fn enum_value_round_trips_through_userdata() {
        use crate::value::{Decode, Encode};

        let lua = Lua::new();
        for handle in [
            CacheHandle::Cached("route".to_owned()),
            CacheHandle::Missing,
        ] {
            let encoded = handle.encode(&lua).unwrap();
            assert!(matches!(encoded, mlua::Value::UserData(_)));
            let back = CacheHandle::decode(&encoded, &lua).unwrap();
            assert_eq!(back, handle);
        }
    }

    #[test]
    fn enum_decode_refuses_foreign_shapes() {
        use crate::value::Decode;

        let lua = Lua::new();
        assert!(
            CacheHandle::decode(&mlua::Value::Nil, &lua).is_err(),
            "nil fails"
        );
        let table = lua.create_table().unwrap();
        assert!(
            CacheHandle::decode(&mlua::Value::Table(table), &lua).is_err(),
            "table fails"
        );
        let foreign = Roster {
            seed: "base".to_owned(),
        };
        let held = lua.create_userdata(foreign).unwrap();
        assert!(
            CacheHandle::decode(&mlua::Value::UserData(held), &lua).is_err(),
            "foreign userdata fails"
        );
    }
}
