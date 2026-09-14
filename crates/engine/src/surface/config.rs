//! Config
//!
//! Config userdata plus per-config contributions.

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods, Value};

use super::confit_table;
use super::document::{Declared, convert_document, convert_entry};
use super::patch::LuaPatch;
use crate::model::{ConfigData, StoredPatch};
use crate::values::{plan_error, read_marker};

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
}

impl UserData for ConfigBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("add_document", |_, this, value: Value| {
            let name = this.data.name.clone();
            let ctx = format!("config '{name}': field 'add_document'");
            match value {
                Value::Table(table) => match read_marker(&table, "__kind").as_deref() {
                    Some("rc-entry") => {
                        let entry = convert_entry(&table, &ctx)?;
                        this.data.rc.push(entry);
                        Ok(())
                    }
                    Some(_) => {
                        push_declared(&mut this.data, &table, &ctx)?;
                        Ok(())
                    }
                    None => Err(plan_error(format!(
                        "config '{name}': field 'add_document' must be a confit.document value or rc entry"
                    ))),
                },
                _ => Err(plan_error(format!(
                    "config '{name}': field 'add_document' must be a confit.document value or rc entry"
                ))),
            }
        });
        methods.add_method_mut("add_patch", |_, this, value: Value| {
            let name = this.data.name.clone();
            match value {
                Value::UserData(handle) => match handle.borrow::<LuaPatch>() {
                    Ok(patch) => {
                        let owned = patch.clone();
                        this.data.patches.push(StoredPatch {
                            target: owned.target,
                            format: owned.format,
                            callback: owned.callback,
                            priority: owned.priority,
                            owner: name,
                        });
                        Ok(())
                    }
                    Err(_) => Err(plan_error(format!(
                        "config '{name}': field 'add_patch' must be a confit.patch value"
                    ))),
                },
                _ => Err(plan_error(format!(
                    "config '{name}': field 'add_patch' must be a confit.patch value"
                ))),
            }
        });
    }
}

/// Pushes one document table into the contribution.
fn push_declared(data: &mut ConfigData, table: &Table, ctx: &str) -> mlua::Result<()> {
    match convert_document(table, ctx)? {
        Declared::Structured(decl) => data.structured.push(decl),
        Declared::Text(decl) => data.texts.push(decl),
        Declared::Link(decl) => data.links.push(decl),
        Declared::RcEntries(entries) => data.rc.extend(entries),
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
fn config_callback(lua: &Lua, name: Value) -> mlua::Result<AnyUserData> {
    let name = match name {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_error(
                "confit.config: field 'name' must be a string".to_string(),
            ));
        }
    };
    claim_name(lua, &name)?;
    lua.create_userdata(ConfigBuilder::new(name))
}

/// Claims one config name in the per-evaluation set.
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
    match taken {
        Value::Nil => {
            seen.set(name, true)?;
            Ok(())
        }
        _ => Err(plan_error(format!(
            "confit.config: config '{name}' is already defined"
        ))),
    }
}
