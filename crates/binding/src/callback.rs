//! Lua callbacks over args faces.

use std::marker::PhantomData;

use mlua::{Function, Lua, Value};

use crate::face::{Face, FaceData};
use crate::fault::FaceFault;
use crate::registry::Registry;
use crate::value::{Decode, Encode};

/// One Lua callback over its args face.
pub struct Callback<Args> {
    function: Function,
    marker: PhantomData<Args>,
}

/// Callback args shape.
pub trait CallbackArgs {
    /// Call parameters with borrowed plain values beside owned live tables.
    type Parameters<'args>;

    /// The callback return face.
    type Return: Face;

    /// Return the param count.
    fn arity() -> usize;

    /// Return the inline call face.
    fn call_face() -> String;

    /// Decode one Lua value into the callback function.
    ///
    /// # Errors
    ///
    /// - [`FaceFault`] for values outside the function shape.
    /// - [`FaceFault`] for fixed functions with param counts apart from the arity.
    fn decode_function(value: &Value, lua: &Lua) -> Result<Function, FaceFault>;

    /// Call the function with the encoded parameters.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for Lua failures.
    fn invoke(
        function: &Function,
        lua: &Lua,
        parameters: Self::Parameters<'_>,
    ) -> mlua::Result<Value>;
}

impl<Args> Clone for Callback<Args> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
            marker: PhantomData,
        }
    }
}

impl<Args> std::fmt::Debug for Callback<Args> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("Callback")
            .field(&self.function)
            .finish()
    }
}

impl<Args> Callback<Args> {
    /// Wrap one function as its callback.
    pub fn new(function: Function) -> Self {
        Self {
            function,
            marker: PhantomData,
        }
    }

    /// Return the stored function.
    pub fn as_function(&self) -> &Function {
        &self.function
    }
}

impl<Args: CallbackArgs> Callback<Args> {
    /// Decode one Lua value into the callback.
    ///
    /// # Errors
    ///
    /// - [`FaceFault`] for values outside the function shape.
    /// - [`FaceFault`] for fixed functions with param counts apart from the arity.
    pub fn decode(value: &Value, lua: &Lua) -> Result<Self, FaceFault> {
        Args::decode_function(value, lua).map(Self::new)
    }

    /// Call the function with the parameters.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for Lua failures.
    /// - [`mlua::Error`] for return values outside the decoded shape.
    pub fn call<R: Decode>(&self, lua: &Lua, parameters: Args::Parameters<'_>) -> mlua::Result<R> {
        let value = Args::invoke(self.as_function(), lua, parameters)?;
        R::decode(&value, lua).map_err(|mismatch| mlua::Error::external(mismatch.to_string()))
    }
}

impl<Args: CallbackArgs + Face> Face for Callback<Args> {
    fn face_name() -> String {
        Args::call_face()
    }

    fn face() -> FaceData {
        Args::face()
    }

    fn collect(collector: &mut Registry) {
        Args::collect(collector);
    }
}

impl<Args: CallbackArgs> Decode for Callback<Args> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, FaceFault> {
        Self::decode(value, lua)
    }
}

impl<Args: CallbackArgs> Encode for Callback<Args> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        self.as_function().encode(lua)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Function, Lua, Table, Value};

    use crate::face::Face;
    use crate::registry::Registry;
    use confit_binding_macros::Face;

    /// Live document marker for callback tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table)]
    struct TestLive {
        /// The document title.
        title: String,
    }

    /// Args face for callback tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(callback_shape)]
    struct TestArgs {
        /// Target route of the patched document.
        route: String,
        /// Live document under patching.
        #[face(live)]
        document: TestLive,
    }

    /// Single-field args face for callback tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(callback_shape)]
    struct SingleArgs {
        /// Target route of the candidate document.
        route: String,
    }

    /// Holder table for callback method tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table)]
    struct TestHolder {
        /// Run the patch over its inputs.
        apply: Callback<TestArgs>,
    }

    /// Filter args face for callback return tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(callback_shape, returns = bool)]
    struct FilterLike {
        /// Target route of the candidate document.
        route: String,
    }

    /// Holder table with filter beside apply for return tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table)]
    struct FilterHolder {
        /// Keep matching routes.
        filter: Callback<FilterLike>,
        /// Run the patch over its inputs.
        apply: Callback<TestArgs>,
    }

    #[test]
    fn stub_names_documented_params() {
        assert_eq!(TestArgs::face_name(), "TestArgs");
        assert_eq!(
            TestArgs::call_face(),
            "fun(route: string, document: TestLive)"
        );
        assert_eq!(SingleArgs::call_face(), "fun(route: string)");
        assert_eq!(
            Callback::<TestArgs>::face_name(),
            "fun(route: string, document: TestLive)"
        );
        let params = match TestArgs::face() {
            FaceData::Function { parameters, .. } => parameters,
            _ => panic!("args hold function data"),
        };
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "route");
        assert_eq!(params[1].name, "document");
        match TestArgs::face() {
            FaceData::Function { returns, .. } => assert!(returns.is_empty()),
            _ => panic!("args hold function data"),
        }
        let mut collector = Registry::new();
        TestHolder::collect(&mut collector);
        assert!(!collector.contains("TestArgs"));
        assert!(collector.contains("TestHolder"));
        assert!(collector.contains("TestLive"));
        let text = collector.stub_text();
        assert!(text.contains("@field apply fun(route: string, document: TestLive)"));
        assert!(text.contains("@param route string Target route of the patched document."));
        assert!(text.contains("function TestHolder.apply(route, document) end"));
        assert!(!text.contains("function TestArgs"));
        assert_eq!(
            text,
            "--- Holder table for callback method tests.\n--- @class TestHolder\n--- @field apply fun(route: string, document: TestLive) Run the patch over its inputs.\nTestHolder = {}\n\n--- Run the patch over its inputs.\n--- @param route string Target route of the patched document.\n--- @param document TestLive Live document under patching.\nfunction TestHolder.apply(route, document) end\n\n--- Live document marker for callback tests.\n--- @class TestLive\n--- @field title string The document title.\n"
        );
    }

    #[test]
    fn filter_stubs_show_boolean_returns_while_apply_stays_bare() {
        assert_eq!(FilterLike::call_face(), "fun(route: string): boolean");
        assert_eq!(
            Callback::<FilterLike>::face_name(),
            "fun(route: string): boolean"
        );
        match FilterLike::face() {
            FaceData::Function {
                parameters,
                returns,
                ..
            } => {
                assert_eq!(parameters.len(), 1);
                assert_eq!(parameters[0].name, "route");
                assert_eq!(returns.len(), 1);
                assert_eq!(returns[0].face.as_ref(), "boolean");
            }
            _ => panic!("expected function data for filter"),
        }
        let mut collector = Registry::new();
        FilterHolder::collect(&mut collector);
        let text = collector.stub_text();
        assert!(text.contains("@field filter fun(route: string): boolean"));
        assert!(text.contains("@field apply fun(route: string, document: TestLive)"));
        assert!(text.contains("function FilterHolder.filter(route) end"));
        assert!(text.contains("function FilterHolder.apply(route, document) end"));
        for block in text.split("\n\n") {
            if block.contains("function FilterHolder.filter(") {
                assert!(block.contains("--- @return boolean"));
            }
            if block.contains("function FilterHolder.apply(") {
                assert!(!block.contains("@return"));
            }
        }
    }

    #[test]
    fn decode_checks_fixed_arity_showing_params() {
        let lua = Lua::new();
        assert_eq!(TestArgs::arity(), 2);
        assert_eq!(SingleArgs::arity(), 1);
        assert!(
            Callback::<TestArgs>::decode(&Value::Integer(1), &lua).is_err(),
            "foreign value fails"
        );
        let function: Function = lua
            .load("return function(first, second, third) end")
            .call(())
            .unwrap();
        assert!(
            Callback::<TestArgs>::decode(&Value::Function(function), &lua).is_err(),
            "wrong arity fails"
        );
        let single: Function = lua.load("return function(first) end").call(()).unwrap();
        let callback = Callback::<SingleArgs>::decode(&Value::Function(single), &lua).unwrap();
        let _ = callback.as_function();
        let pair: Function = lua
            .load("return function(first, second) end")
            .call(())
            .unwrap();
        assert!(
            Callback::<SingleArgs>::decode(&Value::Function(pair), &lua).is_err(),
            "wrong arity fails"
        );
    }

    #[test]
    fn decode_accepts_variadic_and_native_functions() {
        let lua = Lua::new();
        let variadic: Function = lua.load("return function(...) end").call(()).unwrap();
        let decoded = Callback::<TestArgs>::decode(&Value::Function(variadic), &lua).unwrap();
        let _ = decoded.as_function();
        let native = lua.create_function(|_, _value: String| Ok(())).unwrap();
        let decoded = Callback::<TestArgs>::decode(&Value::Function(native), &lua).unwrap();
        let _ = decoded.as_function();
    }

    #[test]
    fn call_encodes_plain_values_beside_borrowed_tables() {
        let lua = Lua::new();
        lua.load(
            r##"
            recorded = {}
            function apply(route, document)
                recorded.count = select("#", route, document)
                recorded.route = route
                recorded.seen = document.title
                document.title = "patched"
            end
            "##,
        )
        .exec()
        .unwrap();
        let function: Function = lua.globals().get("apply").unwrap();
        let callback = Callback::<TestArgs>::decode(&Value::Function(function), &lua).unwrap();
        let document = lua.create_table().unwrap();
        document.set("title", "draft").unwrap();
        callback
            .call::<()>(&lua, (&"home".to_owned(), document.clone()))
            .unwrap();
        let recorded: Table = lua.globals().get("recorded").unwrap();
        let count: i64 = recorded.get("count").unwrap();
        assert_eq!(count, 2);
        let route: String = recorded.get("route").unwrap();
        assert_eq!(route, "home");
        let seen: String = recorded.get("seen").unwrap();
        assert_eq!(seen, "draft");
        let title: String = document.get("title").unwrap();
        assert_eq!(title, "patched");
    }

    #[test]
    fn call_decoded_boolean_returns() {
        let lua = Lua::new();
        lua.load(
            r##"
            function allow(route)
                return route == "home"
            end
            "##,
        )
        .exec()
        .unwrap();
        let function: Function = lua.globals().get("allow").unwrap();
        let callback = Callback::<SingleArgs>::decode(&Value::Function(function), &lua).unwrap();
        assert!(callback.call::<bool>(&lua, (&"home".to_owned(),)).unwrap());
        assert!(!callback.call::<bool>(&lua, (&"away".to_owned(),)).unwrap());
    }
}
