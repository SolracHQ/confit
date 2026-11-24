//! Object shapes backed by Lua userdata.
//!
//! An object shape exposes Rust values to Lua as userdata with fields and
//! methods. The `Shape` derive builds field access from `get` and `set`. The
//! `lua_methods` attribute builds the method table from an `impl` block.

use std::borrow::Cow;

use crate::generator::Generator;

/// Adds a shape and every shape it references to a generator.
pub type CollectFn = fn(&mut Generator);

/// One field exposed on a Lua object.
///
/// The derive builds a field from each Rust field that carries `get` or `set`.
/// The field carries the Lua key, the type expression, the access flags, and
/// the collector for any shape the type references.
#[derive(Clone, Debug)]
pub struct ObjectField {
    /// The Lua key.
    pub name: &'static str,
    /// The stub description.
    pub description: &'static str,
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// Whether `__index` reads the field.
    pub readable: bool,
    /// Whether `__newindex` writes the field.
    pub writable: bool,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for ObjectField {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.shape == other.shape
            && self.readable == other.readable
            && self.writable == other.writable
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

impl ObjectField {
    /// Adds every shape referenced by the field to the generator.
    pub fn collect_referenced(&self, collector: &mut Generator) {
        (self.collect)(collector);
    }
}

/// One parameter in an object method.
#[derive(Clone, Debug)]
pub struct MethodParameter {
    /// The Lua parameter label.
    pub name: &'static str,
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// The stub description.
    pub description: &'static str,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for MethodParameter {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.shape == other.shape
            && self.description == other.description
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One return value in an object method.
#[derive(Clone, Debug)]
pub struct MethodReturn {
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// The stub description.
    pub description: &'static str,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for MethodReturn {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.description == other.description
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One method exposed on a Lua object.
#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    /// The Lua method name.
    pub name: &'static str,
    /// The stub description.
    pub description: &'static str,
    /// The method parameters in declaration order.
    pub parameters: Vec<MethodParameter>,
    /// The return values in order.
    pub returns: Vec<MethodReturn>,
}

impl Method {
    /// Adds every shape referenced by the method to the generator.
    pub fn collect_referenced(&self, collector: &mut Generator) {
        for parameter in &self.parameters {
            (parameter.collect)(collector);
        }
        for returned in &self.returns {
            (returned.collect)(collector);
        }
    }
}

/// One Lua metamethod operator exposed on an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Operator {
    /// The LuaLS operator name.
    pub lua: &'static str,
    /// The operator operands in declaration order.
    pub parameters: Vec<MethodParameter>,
    /// The return values in order.
    pub returns: Vec<MethodReturn>,
}

impl Operator {
    /// Adds every shape referenced by the operator to the generator.
    pub fn collect_referenced(&self, collector: &mut Generator) {
        for parameter in &self.parameters {
            (parameter.collect)(collector);
        }
        for returned in &self.returns {
            (returned.collect)(collector);
        }
    }
}

/// A Rust type that Lua sees as userdata.
///
/// The `lua_methods` attribute builds this implementation for an object block.
pub trait Object: mlua::UserData {
    /// Returns the Lua type name.
    fn name() -> &'static str;
    /// Returns the methods exposed on the object.
    fn methods() -> Vec<Method>;
    /// Returns the fields exposed on the object.
    fn fields() -> Vec<ObjectField>;
    /// Returns the operators exposed on the object.
    fn operators() -> Vec<Operator>;
    /// Adds the object and every referenced shape to the generator.
    fn collect(collector: &mut Generator);
}

/// The fields of a Rust type that Lua reads and writes through userdata.
///
/// The `Shape` derive builds this implementation for an object struct or enum.
/// Generated glue names the trait through `::lua_binding::object::ObjectFields`.
pub trait ObjectFields: Sized {
    /// Registers every exposed field on the userdata metatable.
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F);
    /// Returns the exposed fields in declaration order.
    fn field_metadata() -> Vec<ObjectField>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::LuaTrace;
    use crate::{Shape, lua_globals, lua_methods};
    use mlua::Lua;

    /// Roster handle for method tests.
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct Roster {
        seed: String,
    }

    #[lua_methods]
    impl Roster {
        /// Lists members with the prefix.
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

        /// Returns the seed.
        ///
        /// # Returns
        ///
        /// - The stored seed.
        pub fn seed_name(&self) -> String {
            self.seed.clone()
        }
    }

    #[test]
    fn generated_methods_return_calls_through_lua() {
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
        assert_eq!(methods[0].parameters[0].shape, "string");
        assert_eq!(
            methods[0].parameters[0].description,
            "Filter members by prefix."
        );
        assert_eq!(methods[0].returns.len(), 1);
        assert_eq!(methods[0].returns[0].shape, "string[]");
        assert_eq!(
            methods[0].returns[0].description,
            "Member names with the seed first."
        );
        assert!(methods[1].parameters.is_empty());
        assert_eq!(methods[1].returns.len(), 1);
        assert_eq!(methods[1].returns[0].shape, "string");
        assert_eq!(methods[1].returns[0].description, "The stored seed.");
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

    /// Blast handle for metamethod tests.
    #[derive(Clone, Shape)]
    #[shape(traced, object)]
    struct Boom {
        message: String,
        #[shape(lua_trace)]
        trace: LuaTrace,
    }

    #[lua_methods]
    impl Boom {
        /// Returns the message.
        ///
        /// # Returns
        ///
        /// - The message text.
        fn message(&self) -> String {
            self.message.clone()
        }

        /// Renders the error line.
        ///
        /// # Returns
        ///
        /// - The rendered error line.
        #[operator(__tostring)]
        fn render(&self) -> String {
            format!("boom: {}", self.message)
        }
    }

    /// Blast source for global constructor tests.
    struct BoomSource;

    #[lua_globals("demo")]
    impl BoomSource {
        /// Builds a blast handle.
        ///
        /// # Arguments
        ///
        /// - [message] The blast message text.
        ///
        /// # Returns
        ///
        /// - The blast handle.
        pub fn boom(lua: &Lua, message: String) -> Boom {
            Boom::traced(
                lua,
                Boom {
                    message,
                    trace: LuaTrace::default(),
                },
            )
        }
    }

    #[test]
    fn tostring_operator_renders_through_lua() {
        let methods = Boom::methods();
        let names: Vec<&str> = methods.iter().map(|method| method.name).collect();
        assert_eq!(names, vec!["message"]);
        let lua = Lua::new();
        BoomSource::install(&lua, &lua.globals()).unwrap();
        let rendered: String = lua
            .load("return tostring(boom('loud failure'))")
            .eval()
            .unwrap();
        assert_eq!(rendered, "boom: loud failure");
    }

    /// Cache fixture.
    #[derive(Clone, Debug, PartialEq, Shape)]
    #[shape(object)]
    enum CacheHandle {
        Cached(String),
        Missing,
    }

    #[lua_methods]
    impl CacheHandle {
        /// Returns a constant.
        ///
        /// # Returns
        ///
        /// - Constant value.
        pub fn tag(&self) -> u64 {
            7
        }
    }

    #[test]
    fn enum_value_round_trips_through_object() {
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
    fn enum_decode_rejects_wrong_shapes() {
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
            "wrong userdata fails"
        );
    }

    /// Label table for field collection tests.
    #[derive(Clone, Shape)]
    #[shape(table)]
    struct Label {
        /// The label text.
        text: String,
    }

    /// Object holding a referenced field shape.
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct Tagged {
        /// The held label.
        #[shape(get)]
        label: Label,
    }

    #[lua_methods]
    impl Tagged {}

    #[test]
    fn object_field_collects_referenced_shape() {
        let mut generator = Generator::new();
        <Tagged as Shape>::collect(&mut generator);
        assert!(
            generator.contains("Label"),
            "field shapes enter the generator"
        );
    }
}
