//! Config
//!
//! Config userdata plus per-config contributions for Lua.

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods, Value};

use super::confit_table;
use super::document::{document_table_to_document, push_entry_table, table_kind};
use super::patch::LuaPatch;
use crate::model::state::config::ConfigContribution;
use crate::model::state::document::Document;

/// Named-registry key holding the per-evaluation config name set.
///
/// `install` stores an empty table under this key so `confit.config` repeats fail as plan
/// errors naming the repeated config.
const SEEN_KEY: &str = "confit.config.names";

/// Stored patch handle plus owner for live execution.
///
/// Holds the Lua handle carrying target plus format plus callback plus
/// priority, stamped with the contributing config name at `add_patch`.
#[derive(Debug, Clone)]
pub struct StoredPatch {
    /// Holds the patch handle.
    pub handle: LuaPatchHandle,
    /// Holds the contributing config name.
    pub owner: String,
}

/// Cloneable patch handle data for storage.
///
/// Holds target plus format plus priority plus callback. Functions
/// clone by reference on the same state.
#[derive(Debug, Clone)]
pub struct LuaPatchHandle {
    /// Holds the target document key.
    pub target: String,
    /// Holds the structured format, empty for rc.
    pub format: Option<crate::model::state::document::StructuredFormat>,
    /// Holds the callback receiving the wrapper.
    pub callback: mlua::Function,
    /// Holds the merge priority.
    pub priority: crate::model::state::level::Level,
}

/// Rust-owned builder behind the `confit.config` userdata.
///
/// Mutated in place by `add_document`; profile parsing snapshots it
/// into a [`ConfigContribution`] via [`ConfigBuilder::contribution`].
#[derive(Debug, Clone)]
pub struct ConfigBuilder {
    /// Accumulated contribution.
    contribution: ConfigContribution,
    /// Patch handles for live execution, in declaration order.
    handles: Vec<StoredPatch>,
}

impl ConfigBuilder {
    /// Creates a builder holding an initial contribution.
    ///
    /// # Arguments
    ///
    /// * `name` - config name.
    ///
    /// # Returns
    ///
    /// Builder holding the initial contribution.
    fn new(name: String) -> Self {
        Self {
            contribution: ConfigContribution {
                name,
                ..ConfigContribution::default()
            },
            handles: Vec::new(),
        }
    }

    /// Returns the accumulated contribution.
    ///
    /// # Returns
    ///
    /// Accumulated contribution.
    pub fn contribution(&self) -> &ConfigContribution {
        &self.contribution
    }

    /// Returns patch handles for live execution.
    ///
    /// # Returns
    ///
    /// Handles in declaration order with owners.
    pub fn handles(&self) -> &[StoredPatch] {
        &self.handles
    }
}

/// Installs the config constructor on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the constructor.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(SEEN_KEY, lua.create_table()?)?;
    let confit = confit_table(lua)?;
    confit.set("config", lua.create_function(config_callback)?)?;
    Ok(())
}

/// Builds a config userdata value for `confit.config`.
///
/// # Arguments
///
/// * `lua` - Lua state holding the userdata.
/// * `name` - raw config name value.
///
/// # Returns
///
/// Config userdata value.
///
/// # Errors
///
/// Fails with plan errors for values of other shapes and for repeated names.
pub(crate) fn config_callback(lua: &Lua, name: Value) -> mlua::Result<AnyUserData> {
    let name = match name {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_err!("confit.config: field 'name' must be a string"));
        }
    };
    claim_name(lua, &name)?;
    lua.create_userdata(ConfigBuilder::new(name))
}

/// Builds a plan error naming config plus field.
fn config_field_error(config: &str, field: &str, detail: &str) -> mlua::Error {
    plan_err!("config '{config}': field '{field}' {detail}")
}

/// Claims a config name in the per-evaluation set.
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
        _ => Err(plan_err!(
            "confit.config: config '{name}' is already defined"
        )),
    }
}

/// Appends one rc entry table into the per-config rc document.
///
/// Creates the rc document while absent, then pushes the entry into
/// the matching RcData lists. Routing follows the `__area` metatable.
///
/// # Arguments
///
/// * `documents` - declared documents under extension, mutated in place.
/// * `table` - entry table under conversion.
/// * `ctx` - error context naming the caller.
///
/// # Errors
///
/// Fails with plan errors for missing areas plus bad entry shapes.
fn push_rc_table(documents: &mut Vec<Document>, table: &Table, ctx: &str) -> mlua::Result<()> {
    push_entry_table(documents, table, ctx)
}

impl UserData for ConfigBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("add_document", |_, this, value: Value| {
            let config = this.contribution.name.clone();
            let ctx = format!("config '{config}': field 'add_document'");
            match value {
                Value::Table(table) => match table_kind(&table) {
                    Some(kind) if kind == "rc-entry" => {
                        push_rc_table(&mut this.contribution.documents, &table, &ctx)?;
                        Ok(())
                    }
                    Some(kind)
                        if matches!(kind.as_str(), "rc" | "structured" | "text" | "link") =>
                    {
                        let document = document_table_to_document(&table, &ctx)?;
                        this.contribution.documents.push(document);
                        Ok(())
                    }
                    Some(kind) => Err(plan_err!("{ctx} unknown document kind '{kind}'")),
                    None => Err(config_field_error(
                        &config,
                        "add_document",
                        "must be a confit.document value or rc entry",
                    )),
                },
                _ => Err(config_field_error(
                    &config,
                    "add_document",
                    "must be a confit.document value or rc entry",
                )),
            }
        });
        methods.add_method_mut("add_patch", |_, this, value: Value| {
            let config = this.contribution.name.clone();
            match value {
                Value::UserData(handle) => match handle.borrow::<LuaPatch>() {
                    Ok(patch) => {
                        let owned = patch.clone();
                        this.contribution
                            .patches
                            .push(owned.clone().into_model(&config));
                        this.handles.push(StoredPatch {
                            handle: LuaPatchHandle {
                                target: owned.target.clone(),
                                format: owned.format,
                                callback: owned.callback.clone(),
                                priority: owned.priority,
                            },
                            owner: config,
                        });
                        Ok(())
                    }
                    Err(_) => Err(config_field_error(
                        &config,
                        "add_patch",
                        "must be a confit.patch value",
                    )),
                },
                _ => Err(config_field_error(
                    &config,
                    "add_patch",
                    "must be a confit.patch value",
                )),
            }
        });
    }
}
