//! Module
//!
//! Dual-registered module verbs. One declaration installs the Lua
//! function with recording its stub face, so neither restates the
//! other.

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, MaybeSend, Table, Value};
use mlua_extras::typed::Typed;
use mlua_extras::typed::TypedMultiValue;
use mlua_extras::typed::generator::DefinitionBuilder;

/// One verb declaration holding docs with its name.
pub struct VerbDecl<'a> {
    /// The verb name.
    pub name: &'a str,
    /// The verb documentation.
    pub doc: &'a str,
    /// Holds parameter names with documentation.
    pub params: &'a [(&'a str, &'a str)],
    /// Holds return documentation in order.
    pub rets: &'a [&'a str],
}

/// One module under building holding its table with its face.
pub struct Module {
    /// Holds the module table.
    table: Table,
    /// The module face.
    face: DefinitionBuilder,
}

impl Module {
    /// Starts one module holding an empty table with a documented face.
    pub fn start(lua: &Lua, doc: &'static str) -> mlua::Result<Self> {
        Ok(Self {
            table: lua.create_table()?,
            face: mlua_extras::typed::generator::Definition::start().document(doc),
        })
    }

    /// Registers one handle class under its trailing type name.
    pub fn register<T: Typed>(&mut self) {
        let face = std::mem::take(&mut self.face).register::<T>(class_name::<T>());
        self.face = face;
    }

    /// Declares one verb with its face.
    ///
    /// The generic signature drives the installed function with the
    /// rendered face together, and the closure bound rejects drift
    /// between them.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for failed function and table writes.
    pub fn verb<P, R, F>(&mut self, lua: &Lua, decl: VerbDecl<'_>, f: F) -> mlua::Result<()>
    where
        P: TypedMultiValue + FromLuaMulti,
        R: TypedMultiValue + IntoLuaMulti,
        F: Fn(&Lua, P) -> mlua::Result<R> + MaybeSend + 'static,
    {
        self.table.set(decl.name, lua.create_function(f)?)?;
        let mut face = std::mem::take(&mut self.face);
        face = face.document(decl.doc);
        for (pname, pdoc) in decl.params {
            face = face.param(*pname, *pdoc);
        }
        for ret in decl.rets {
            face = face.ret(*ret);
        }
        self.face = face.function::<P, R>(decl.name, ());
        Ok(())
    }

    /// Finishes one module into its table with its face.
    pub fn finish(self) -> (Table, DefinitionBuilder) {
        (self.table, self.face)
    }
}

/// Wraps one module table behind a read-only proxy.
///
/// Writes fail naming the read-only table.
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

/// Names one registered type after its trailing segment.
pub fn class_name<T>() -> String {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("Unknown")
        .to_owned()
}
