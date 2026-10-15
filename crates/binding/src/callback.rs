//! Callback
//! Lua callbacks over args faces.

use std::marker::PhantomData;

use mlua::{Function, Lua, Value};

use crate::face::Face;
use crate::registry::Registry;
use crate::value::{Decode, Encode, Mismatch};

/// One Lua callback over its args face.
///
/// One Lua function with its args marker.
pub struct Callback<Args> {
    function: Function,
    marker: PhantomData<Args>,
}

/// Callback args shape.
///
/// The function face with the call arity and the call encoding.
pub trait CallbackArgs {
    /// Call parameters with borrowed plain values beside owned live tables.
    type Parameters<'args>;

    /// Return the param count.
    fn arity() -> usize;

    /// Return the inline call face.
    fn call_face() -> String;

    /// Decode one Lua value into the callback function.
    ///
    /// # Errors
    ///
    /// - [`Mismatch`] for values outside the function shape.
    /// - [`Mismatch`] for fixed functions with param counts apart from the arity.
    fn decode_function(value: &Value, lua: &Lua) -> Result<Function, Mismatch>;

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
    /// - [`Mismatch`] for values outside the function shape.
    /// - [`Mismatch`] for fixed functions with param counts apart from the arity.
    pub fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
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

    fn face_stub() -> String {
        String::new()
    }

    fn kind() -> crate::face::FaceKind {
        crate::face::FaceKind::Function
    }

    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        Args::collect(collector)
    }
}

impl<Args: CallbackArgs> Decode for Callback<Args> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
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

    /// Marker face for callback tests.
    struct Sample;

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

    #[test]
    fn stored_function_answers_calls() {
        let lua = Lua::new();
        let function = lua.create_function(|_, value: String| Ok(value)).unwrap();
        let callback = Callback::<Sample>::new(function);
        let back: String = callback.as_function().call("held").unwrap();
        assert_eq!(back, "held");
        let again = callback.clone();
        let kept: String = again.as_function().call("kept").unwrap();
        assert_eq!(kept, "kept");
        assert!(format!("{:?}", callback).contains("Callback"));
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
        let stub = TestArgs::face_stub();
        assert!(stub.contains("@param route string Target route of the patched document."));
        assert!(stub.contains("@param document TestLive Live document under patching."));
        assert!(stub.contains("function TestArgs(route, document) end"));
        let mut collector = Registry::new();
        TestArgs::collect(&mut collector).unwrap();
        assert!(collector.contains("TestArgs"));
        assert!(collector.contains("TestLive"));
    }

    #[test]
    fn decode_rejects_non_functions_showing_the_shape() {
        let lua = Lua::new();
        let failure = Callback::<TestArgs>::decode(&Value::Integer(1), &lua).unwrap_err();
        let text = failure.to_string();
        assert!(text.contains("TestArgs"));
        assert!(text.contains("integer"));
    }

    #[test]
    fn decode_checks_fixed_arity_showing_params() {
        let lua = Lua::new();
        assert_eq!(TestArgs::arity(), 2);
        assert_eq!(SingleArgs::arity(), 1);
        let function: Function = lua
            .load("return function(first, second, third) end")
            .call(())
            .unwrap();
        let failure = Callback::<TestArgs>::decode(&Value::Function(function), &lua).unwrap_err();
        let text = failure.to_string();
        assert!(text.contains("TestArgs"));
        assert!(text.contains("2 params"));
        let single: Function = lua.load("return function(first) end").call(()).unwrap();
        let callback = Callback::<SingleArgs>::decode(&Value::Function(single), &lua).unwrap();
        let _ = callback.as_function();
        let pair: Function = lua
            .load("return function(first, second) end")
            .call(())
            .unwrap();
        let failure = Callback::<SingleArgs>::decode(&Value::Function(pair), &lua).unwrap_err();
        assert!(failure.to_string().contains("1 param"));
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
    fn single_field_calls_encode_one_param() {
        let lua = Lua::new();
        lua.load(
            r##"
            recorded = {}
            function filter(route)
                recorded.route = route
            end
            "##,
        )
        .exec()
        .unwrap();
        let function: Function = lua.globals().get("filter").unwrap();
        let callback = Callback::<SingleArgs>::decode(&Value::Function(function), &lua).unwrap();
        callback.call::<()>(&lua, (&"home".to_owned(),)).unwrap();
        let recorded: Table = lua.globals().get("recorded").unwrap();
        let route: String = recorded.get("route").unwrap();
        assert_eq!(route, "home");
    }

    #[test]
    fn call_decodes_boolean_returns() {
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
