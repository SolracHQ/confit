//! Module
//! Shared namespace registration with stub collection.

use mlua::{Lua, Table, Value};

/// Wrap one module table as read-only under its dotted name.
///
/// # Errors
///
/// - [`mlua::Error`] for failed table and metatable writes.
pub fn readonly(lua: &Lua, name: &'static str, inner: Table) -> mlua::Result<Value> {
    let proxy = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", inner)?;
    let message = format!("cannot write '{name}': read-only table");
    let reject = lua.create_function(move |_, _: (Table, Value, Value)| {
        Err::<(), _>(mlua::Error::RuntimeError(message.clone()))
    })?;
    meta.set("__newindex", reject)?;
    meta.set("__metatable", false)?;
    proxy.set_metatable(Some(meta))?;
    Ok(Value::Table(proxy))
}

/// Declare one Lua module with its verbs.
///
/// - The namespace reads the dotted Lua path.
/// - Single verbs register the bare function value.
/// - Verb lists register the read-only table.
/// - Every arm collects its verb faces transitively.
#[macro_export]
macro_rules! module {
    (
        namespace = $namespace:literal,
        verb = $verb:ident $(,)?
    ) => {
        /// Bound module face for one namespace.
        pub struct Module;

        impl Module {
            /// The dotted Lua path.
            pub const NAMESPACE: &'static str = $namespace;
            /// The verb names in registration order.
            pub const VERBS: &[&str] = &[stringify!($verb)];

            /// Build the bare verb function value.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn register(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                $verb::function(lua).map(::mlua::Value::Function)
            }

            /// Collect the verb face with every referenced face.
            ///
            /// # Errors
            ///
            /// - [`$crate::error::BindingError`] for repeated registration.
            pub fn stubs(
                collector: &mut $crate::registry::Registry,
            ) -> $crate::error::Result<()> {
                <$verb::Verb as $crate::verb::Verb>::collect(collector)
            }
        }
    };
    (
        namespace = $namespace:literal,
        verbs = [$($verb:ident),* $(,)?] $(,)?
    ) => {
        /// Bound module face for one namespace.
        pub struct Module;

        impl Module {
            /// The dotted Lua path.
            pub const NAMESPACE: &'static str = $namespace;
            /// The verb names in registration order.
            pub const VERBS: &[&str] = &[$(stringify!($verb)),*];

            /// Build the read-only verb table.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn register(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                let table = lua.create_table()?;
                $(table.set(stringify!($verb), $verb::function(lua)?)?;)*
                $crate::module::readonly(lua, Self::NAMESPACE, table)
            }

            /// Collect every verb face with all referenced faces.
            ///
            /// # Errors
            ///
            /// - [`$crate::error::BindingError`] for repeated registration.
            pub fn stubs(
                collector: &mut $crate::registry::Registry,
            ) -> $crate::error::Result<()> {
                $(<$verb::Verb as $crate::verb::Verb>::collect(collector)?;)*
                Ok(())
            }
        }
    };
}
