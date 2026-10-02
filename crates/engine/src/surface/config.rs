//! Config
//!
//! Config userdata and per-config contributions.

use mlua::{AnyUserData, Lua, MultiValue, Table, UserData, UserDataMethods, Value};

use super::confit_table;
use super::document::Declared;
use super::document::convert::convert_document;
use super::hook::convert_hook;
use super::patch::LuaPatch;
use crate::error::{EngineError, FieldRef, Scope};
use crate::lua::{ValueExt, read_marker};
use crate::model::{ConfigData, RequireDecl, StoredPatch};

/// Registry key holding the per-evaluation config name set.
const SEEN_KEY: &str = "confit.config.names";

/// Rust-owned builder behind the config userdata.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigBuilder {
    /// Accumulated contribution.
    data: ConfigData,
}

impl ConfigBuilder {
    /// Creates one builder holding an initial name.
    fn new(name: String) -> Self {
        Self {
            data: ConfigData {
                name,
                ..ConfigData::default()
            },
        }
    }

    /// Reads the accumulated contribution.
    pub(crate) fn contribution(&self) -> &ConfigData {
        &self.data
    }

    /// Records one document table in the contribution.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-document values.
    ///
    pub(crate) fn add_document(&mut self, value: Value) -> mlua::Result<()> {
        let name = self.data.name.clone();
        let scope = Scope::config(&name);
        let field = FieldRef::name("add_document");
        let method = scope.slot(field.clone());
        let Some(table) = value.opt_table() else {
            return Err(EngineError::Field {
                scope,
                field,
                want: "must be a confit.document value",
            }
            .into());
        };
        let marker = read_marker(&table, "__kind");
        let Some(kind) = marker.as_deref() else {
            return Err(EngineError::Field {
                scope,
                field,
                want: "must be a confit.document value",
            }
            .into());
        };
        if kind == "rc-entry" {
            return Err(EngineError::Field {
                scope,
                field,
                want: "must be a confit.document value (got rc entry)",
            }
            .into());
        }
        push_declared(&mut self.data, &table, &method)
    }

    /// Records one patch handle in the contribution.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-patch values.
    ///
    pub(crate) fn add_patch(&mut self, value: Value) -> mlua::Result<()> {
        let name = self.data.name.clone();
        let domain = || EngineError::Field {
            scope: Scope::config(&name),
            field: FieldRef::name("add_patch"),
            want: "must be a confit.patch value",
        };
        let Some(handle) = value.as_userdata() else {
            return Err(domain().into());
        };
        let owned = handle.borrow::<LuaPatch>().map_err(|_| domain())?.clone();
        self.data.patches.push(StoredPatch {
            target: owned.target,
            format: owned.format,
            callback: owned.callback,
            priority: owned.priority,
            order: 0,
            owner: name,
        });
        Ok(())
    }
    /// Records one hook table in the contribution.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-hook values.
    ///
    pub(crate) fn add_hook(&mut self, value: Value) -> mlua::Result<()> {
        let name = self.data.name.clone();
        let scope = Scope::config(&name);
        let field = FieldRef::name("add_hook");
        let method = scope.slot(field.clone());
        let Some(table) = value.opt_table() else {
            return Err(EngineError::Field {
                scope,
                field,
                want: "must be a confit.hook value",
            }
            .into());
        };
        if read_marker(&table, "__kind").as_deref() != Some("hook") {
            return Err(EngineError::Field {
                scope,
                field,
                want: "must be a confit.hook value",
            }
            .into());
        }
        let hook = convert_hook(&table, &method)?;
        self.data.hooks.push(hook);
        Ok(())
    }
    /// Records one required sibling config in the contribution.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Arity`] for wrong arity.
    /// - [`EngineError::Field`] for misshaped names and hints.
    ///
    pub(crate) fn require(&mut self, args: MultiValue) -> mlua::Result<()> {
        let name = self.data.name.clone();
        let scope = Scope::config(&name).slot(FieldRef::name("require"));
        let collected: Vec<Value> = args.into_iter().collect();
        let (name_value, hint_value) = match collected.as_slice() {
            [first] => (first.clone(), Value::Nil),
            [first, second] => (first.clone(), second.clone()),
            _ => {
                return Err(EngineError::Arity {
                    scope: scope.clone(),
                    want: "(name, hint?)",
                }
                .into());
            }
        };
        let target = name_value.req_str(&scope, "name")?;
        if target.is_empty() {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("name"),
                want: "must not be empty",
            }
            .into());
        }
        let hint = match hint_value {
            Value::Nil => None,
            Value::String(text) => Some(text.to_string_lossy()),
            _ => {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("hint"),
                    want: "must be a string",
                }
                .into());
            }
        };
        self.data.requires.push(RequireDecl { target, hint });
        Ok(())
    }
}

impl UserData for ConfigBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("add_document", |_, this, value: Value| {
            this.add_document(value)
        });
        methods.add_method_mut("add_patch", |_, this, value: Value| this.add_patch(value));
        methods.add_method_mut("add_hook", |_, this, value: Value| this.add_hook(value));
        methods.add_method_mut("require", |_, this, args: MultiValue| this.require(args));
    }
}

/// Pushes one document table into the contribution.
///
/// # Errors
///
/// - [`EngineError::Shape`] for repeated rc bases.
fn push_declared(data: &mut ConfigData, table: &Table, scope: &Scope) -> mlua::Result<()> {
    match convert_document(table, scope)? {
        Declared::Structured(decl) => data.structured.push(decl),
        Declared::Text(decl) => data.texts.push(decl),
        Declared::Link(decl) => data.links.push(decl),
        Declared::Opaque(decl) => data.opaques.push(decl),
        Declared::Tree(decl) => data.trees.push(decl),
        Declared::Rc(entries) => {
            if data.rc_base.is_some() {
                return Err(EngineError::Shape {
                    scope: scope.clone(),
                    want: "declares the rc base more than once",
                }
                .into());
            }
            data.rc_base = Some(entries);
        }
    }
    Ok(())
}

/// Installs the config constructor on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(SEEN_KEY, lua.create_table()?)?;
    let confit = confit_table(lua)?;
    confit.set("config", lua.create_function(config_callback)?)?;
    Ok(())
}

/// Builds one config userdata value.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string names.
/// - [`EngineError::Defined`] for repeated names.
fn config_callback(lua: &Lua, name: Value) -> mlua::Result<AnyUserData> {
    let name = name.req_str(&Scope::method("confit.config"), "name")?;
    claim_name(lua, &name)?;
    lua.create_userdata(ConfigBuilder::new(name))
}

/// Claims one config name in the per-evaluation set.
///
/// # Errors
///
/// - [`EngineError::Defined`] for repeated names.
fn claim_name(lua: &Lua, name: &str) -> mlua::Result<()> {
    let seen: Table = match lua.named_registry_value(SEEN_KEY) {
        Ok(table) => table,
        Err(_) => {
            let table = lua.create_table()?;
            lua.set_named_registry_value(SEEN_KEY, table.clone())?;
            table
        }
    };
    let taken: Value = seen.get(name)?;
    if taken.is_nil() {
        seen.set(name, true)?;
        return Ok(());
    }
    Err(EngineError::Defined {
        name: name.to_owned(),
    }
    .into())
}
