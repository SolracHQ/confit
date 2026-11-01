//! Typed Lua callables.
//!
//! A [`Callable`] pairs a Lua function with the argument and return shapes of
//! its signature. Decoding checks the argument count, and each call encodes the
//! arguments and decodes the return value.

use std::marker::PhantomData;

use mlua::{Function, Lua, Value};

use crate::error::DecodeError;
use crate::generator::Generator;
use crate::shape::{Shape, ShapeData};
use crate::value::{Decode, Encode};

/// A Lua function with a typed signature.
///
/// The signature fixes the argument and return shapes used on every call.
pub struct Callable<Args> {
    function: Function,
    marker: PhantomData<Args>,
}

/// The argument and return shapes of a Lua callable.
///
/// The `Shape` derive builds this for a function shape.
pub trait Signature {
    /// The argument values for a call.
    type Args<'args>;

    /// The return shape.
    type Return: Shape;

    /// Returns the number of parameters.
    fn arg_count() -> usize;

    /// Returns the Lua signature text used in stubs.
    fn call_shape() -> String;

    /// Decodes a Lua value as a function and checks the argument count.
    ///
    /// # Errors
    ///
    /// - [`DecodeError`] for non-function values.
    /// - [`DecodeError`] for functions whose parameter count differs from the
    ///   signature.
    fn decode_function(value: &Value, lua: &Lua) -> Result<Function, DecodeError>;

    /// Calls the function and returns the raw Lua value.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for Lua failures.
    fn call(function: &Function, lua: &Lua, parameters: Self::Args<'_>) -> mlua::Result<Value>;
}

impl<Args> Clone for Callable<Args> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
            marker: PhantomData,
        }
    }
}

impl<Args> std::fmt::Debug for Callable<Args> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("Callable")
            .field(&self.function)
            .finish()
    }
}

impl<Args> Callable<Args> {
    /// Creates a callable around a Lua function.
    pub fn new(function: Function) -> Self {
        Self {
            function,
            marker: PhantomData,
        }
    }

    /// Returns the wrapped Lua function.
    pub fn function(&self) -> &Function {
        &self.function
    }
}

impl<Args: Signature> Callable<Args> {
    /// Decodes a Lua value as a callable and checks the argument count.
    ///
    /// # Errors
    ///
    /// - [`DecodeError`] for non-function values.
    /// - [`DecodeError`] for functions whose parameter count differs from the
    ///   signature.
    pub fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        Args::decode_function(value, lua).map(Self::new)
    }

    /// Calls the function and decodes the return value.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for Lua failures.
    /// - [`mlua::Error`] for a return value outside the decoded shape.
    pub fn call<R: Decode>(&self, lua: &Lua, parameters: Args::Args<'_>) -> mlua::Result<R> {
        let value = Args::call(self.function(), lua, parameters)?;
        R::decode(&value, lua).map_err(|mismatch| mlua::Error::external(mismatch.to_string()))
    }
}

impl<Args: Signature + Shape> Shape for Callable<Args> {
    fn name() -> String {
        Args::call_shape()
    }

    fn shape_data() -> ShapeData {
        Args::shape_data()
    }

    fn collect(collector: &mut Generator) {
        Args::collect(collector);
    }
}

impl<Args: Signature> Decode for Callable<Args> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        Self::decode(value, lua)
    }
}

impl<Args: Signature> Encode for Callable<Args> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        self.function().encode(lua)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Function, Lua, Table, Value};

    use crate::Shape;
    use crate::generator::Generator;

    /// Live process marker for callable tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(table)]
    struct Process {
        /// The process title.
        title: String,
    }

    /// Args shape for callable tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(function)]
    struct Args {
        /// Target name of the run process.
        name: String,
        /// Live process under running.
        #[shape(borrowed)]
        process: Process,
    }

    /// Single-field args shape for callable tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(function)]
    struct SingleArgs {
        /// Target name of the pending process.
        name: String,
    }

    /// Table holding a callable for method tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(table)]
    struct CallTable {
        /// Runs the process over its inputs.
        run: Callable<Args>,
    }

    /// Arguments for the keep callable.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(function, returns = bool)]
    struct KeepArgs {
        /// Target name of the pending process.
        name: String,
    }

    /// Table holding keep beside run for return tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(table)]
    struct KeepTable {
        /// Keeps matching names.
        keep: Callable<KeepArgs>,
        /// Runs the process over its inputs.
        run: Callable<Args>,
    }

    #[test]
    fn stub_names_described_params() {
        assert_eq!(Args::name(), "Args");
        assert_eq!(Args::call_shape(), "fun(name: string, process: Process)");
        assert_eq!(SingleArgs::call_shape(), "fun(name: string)");
        assert_eq!(
            Callable::<Args>::name(),
            "fun(name: string, process: Process)"
        );
        let params = match Args::shape_data() {
            ShapeData::Function { parameters, .. } => parameters,
            _ => panic!("args hold function data"),
        };
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "name");
        assert_eq!(params[1].name, "process");
        match Args::shape_data() {
            ShapeData::Function { returns, .. } => assert!(returns.is_empty()),
            _ => panic!("args hold function data"),
        }
        let mut collector = Generator::new();
        CallTable::collect(&mut collector);
        assert!(!collector.contains("Args"));
        assert!(collector.contains("CallTable"));
        assert!(collector.contains("Process"));
        let text = collector.generate();
        assert!(text.contains("@field run fun(name: string, process: Process)"));
        assert!(text.contains("@param name string Target name of the run process."));
        assert!(text.contains("function CallTable.run(name, process) end"));
        assert!(!text.contains("function Args"));
        assert_eq!(
            text,
            "--- Table holding a callable for method tests.\n--- @class CallTable\n--- @field run fun(name: string, process: Process) Runs the process over its inputs.\nCallTable = {}\n\n--- Runs the process over its inputs.\n--- @param name string Target name of the run process.\n--- @param process Process Live process under running.\nfunction CallTable.run(name, process) end\n\n--- Live process marker for callable tests.\n--- @class Process\n--- @field title string The process title.\n"
        );
    }

    #[test]
    fn keep_shape_stubs_show_boolean_returns_while_run_stays_bare() {
        assert_eq!(KeepArgs::call_shape(), "fun(name: string): boolean");
        assert_eq!(Callable::<KeepArgs>::name(), "fun(name: string): boolean");
        match KeepArgs::shape_data() {
            ShapeData::Function {
                parameters,
                returns,
                ..
            } => {
                assert_eq!(parameters.len(), 1);
                assert_eq!(parameters[0].name, "name");
                assert_eq!(returns.len(), 1);
                assert_eq!(returns[0].shape.as_ref(), "boolean");
            }
            _ => panic!("expected function data for keep"),
        }
        let mut collector = Generator::new();
        KeepTable::collect(&mut collector);
        let text = collector.generate();
        assert!(text.contains("@field keep fun(name: string): boolean"));
        assert!(text.contains("@field run fun(name: string, process: Process)"));
        assert!(text.contains("function KeepTable.keep(name) end"));
        assert!(text.contains("function KeepTable.run(name, process) end"));
        for block in text.split("\n\n") {
            if block.contains("function KeepTable.keep(") {
                assert!(block.contains("--- @return boolean"));
            }
            if block.contains("function KeepTable.run(") {
                assert!(!block.contains("@return"));
            }
        }
    }

    #[test]
    fn decode_checks_fixed_arity_showing_params() {
        let lua = Lua::new();
        assert_eq!(Args::arg_count(), 2);
        assert_eq!(SingleArgs::arg_count(), 1);
        assert!(
            Callable::<Args>::decode(&Value::Integer(1), &lua).is_err(),
            "foreign value fails"
        );
        let function: Function = lua
            .load("return function(first, second, third) end")
            .call(())
            .unwrap();
        assert!(
            Callable::<Args>::decode(&Value::Function(function), &lua).is_err(),
            "wrong arity fails"
        );
        let single: Function = lua.load("return function(first) end").call(()).unwrap();
        let callable = Callable::<SingleArgs>::decode(&Value::Function(single), &lua).unwrap();
        let _ = callable.function();
        let pair: Function = lua
            .load("return function(first, second) end")
            .call(())
            .unwrap();
        assert!(
            Callable::<SingleArgs>::decode(&Value::Function(pair), &lua).is_err(),
            "wrong arity fails"
        );
    }

    #[test]
    fn decode_accepts_variadic_and_native_functions() {
        let lua = Lua::new();
        let variadic: Function = lua.load("return function(...) end").call(()).unwrap();
        let decoded = Callable::<Args>::decode(&Value::Function(variadic), &lua).unwrap();
        let _ = decoded.function();
        let native = lua.create_function(|_, _value: String| Ok(())).unwrap();
        let decoded = Callable::<Args>::decode(&Value::Function(native), &lua).unwrap();
        let _ = decoded.function();
    }

    #[test]
    fn call_encodes_plain_values_beside_borrowed_tables() {
        let lua = Lua::new();
        lua.load(
            r##"
            recorded = {}
            function run(name, process)
                recorded.count = select("#", name, process)
                recorded.name = name
                recorded.seen = process.title
                process.title = "changed"
            end
            "##,
        )
        .exec()
        .unwrap();
        let function: Function = lua.globals().get("run").unwrap();
        let callable = Callable::<Args>::decode(&Value::Function(function), &lua).unwrap();
        let process = lua.create_table().unwrap();
        process.set("title", "draft").unwrap();
        callable
            .call::<()>(&lua, (&"home".to_owned(), process.clone()))
            .unwrap();
        let recorded: Table = lua.globals().get("recorded").unwrap();
        let count: i64 = recorded.get("count").unwrap();
        assert_eq!(count, 2);
        let name: String = recorded.get("name").unwrap();
        assert_eq!(name, "home");
        let seen: String = recorded.get("seen").unwrap();
        assert_eq!(seen, "draft");
        let title: String = process.get("title").unwrap();
        assert_eq!(title, "changed");
    }

    #[test]
    fn call_decoded_boolean_returns() {
        let lua = Lua::new();
        lua.load(
            r##"
            function allow(name)
                return name == "home"
            end
            "##,
        )
        .exec()
        .unwrap();
        let function: Function = lua.globals().get("allow").unwrap();
        let callable = Callable::<SingleArgs>::decode(&Value::Function(function), &lua).unwrap();
        assert!(callable.call::<bool>(&lua, (&"home".to_owned(),)).unwrap());
        assert!(!callable.call::<bool>(&lua, (&"away".to_owned(),)).unwrap());
    }
}
