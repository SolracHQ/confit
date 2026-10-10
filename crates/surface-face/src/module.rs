//! Module verbs.

use std::borrow::Cow;
use std::marker::PhantomData;

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, MaybeSend, Table, Value};
use mlua_extras::typed::TypedMultiValue;
use mlua_extras::typed::generator::DefinitionBuilder;
use mlua_extras::typed::{Type, Typed};
use schemars::JsonSchema;

/// One verb declaration.
pub struct VerbDecl<'a> {
    /// The verb name.
    pub name: &'a str,
    /// The verb documentation.
    pub documentation: &'a str,
    /// The verb parameter docs.
    pub params: &'a [(&'a str, &'a str)],
    /// The verb return docs in order.
    pub rets: &'a [&'a str],
}

/// One module under building.
pub struct Module {
    /// The registered verbs.
    table: Table,
    /// The module face.
    face: DefinitionBuilder,
}

impl Module {
    /// Start one module with an empty table and a documented face.
    pub fn start(lua: &Lua, documentation: &'static str) -> mlua::Result<Self> {
        Ok(Self {
            table: lua.create_table()?,
            face: mlua_extras::typed::generator::Definition::start().document(documentation),
        })
    }

    /// Register one handle class under its trailing type name.
    pub fn register<T: Typed>(&mut self) {
        let face = std::mem::take(&mut self.face).register::<T>(class_name::<T>());
        self.face = face;
    }

    /// Register one alias face under its name.
    pub fn register_face(&mut self, name: impl Into<Cow<'static, str>>, face: Type) {
        let taken = std::mem::take(&mut self.face);
        self.face = taken.register_as(name, face);
    }

    /// Register one schema face under its trailing type name.
    pub fn register_schema<T: JsonSchema>(&mut self) {
        let face = crate::translate::face_of::<T>();
        let taken = std::mem::take(&mut self.face);
        self.face = taken.register_as(class_name::<T>(), face);
    }

    /// Declare one verb with its face.
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
        face = face.document(decl.documentation);
        for (pname, pdoc) in decl.params {
            face = face.param(*pname, *pdoc);
        }
        for ret in decl.rets {
            face = face.ret(*ret);
        }
        self.face = face.function::<P, R>(decl.name, ());
        Ok(())
    }

    /// Finish one module into its table with its face.
    pub fn finish(self) -> (Table, DefinitionBuilder) {
        (self.table, self.face)
    }
}

/// One named type ref with its face from the Rust type name.
pub struct Named<T>(PhantomData<T>);

impl<T> Typed for Named<T> {
    fn ty() -> Type {
        Type::named(class_name::<T>())
    }
}

/// Declare one union enum with its face over named member types.
///
/// Member faces form the enum face.
#[macro_export]
macro_rules! union_face {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident($member:ty)),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $variant($member)),*
        }
        impl ::mlua_extras::typed::Typed for $name {
            fn ty() -> ::mlua_extras::typed::Type {
                ::mlua_extras::typed::Type::union(vec![
                    $($crate::module::Named::<$member>::ty()),*
                ])
            }
        }
    };
}

/// Declare one table face from named member faces.
///
/// Required fields hold their face, optional fields union with nil. Field
/// names come from the declared idents, member faces come as type
/// expressions.
#[macro_export]
macro_rules! table_face {
    ($target:ty, { $($kind:ident $field:ident : $face:expr),* $(,)? }) => {
        impl ::mlua_extras::typed::Typed for $target {
            fn ty() -> ::mlua_extras::typed::Type {
                ::mlua_extras::typed::Type::table(vec![
                    $($crate::table_face!(@field $kind $field : $face)),*
                ])
            }
        }
    };
    (@field required $field:ident : $face:expr) => {
        (
            ::mlua_extras::typed::Index::Str(stringify!($field).into()),
            $face,
        )
    };
    (@field optional $field:ident : $face:expr) => {
        (
            ::mlua_extras::typed::Index::Str(stringify!($field).into()),
            ::mlua_extras::typed::Type::union(vec![
                $face,
                ::mlua_extras::typed::Type::nil(),
            ]),
        )
    };
}

/// Wrap one module table.
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

/// Return the trailing segment of one registered type name.
pub fn class_name<T>() -> String {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("Unknown")
        .to_owned()
}
