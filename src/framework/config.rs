//! Config
//!
//! Config userdata plus per-config contributions for Lua.

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods, Value};

use super::LuaArtifact;
use super::artifact::{RcEntry, RcPayload, RcSpec};
use super::confit_table;
use crate::model::state::config::{ConfigContribution, PendingArtifact};
use crate::model::state::rc::{AliasEntry, EnvEntry, InitEntry, PathOp, ProfileEntry};

/// Named-registry key holding the per-evaluation config name set.
///
/// `install` stores an empty table under this key so `confit.config` repeats fail as plan
/// errors naming the repeated config.
const SEEN_KEY: &str = "confit.config.names";

/// Rust-owned builder behind the `confit.config` userdata.
///
/// Mutated in place by `add_artifact`; profile parsing snapshots it
/// into a [`ConfigContribution`] via [`ConfigBuilder::contribution`].
#[derive(Debug, Clone)]
pub struct ConfigBuilder {
    /// Accumulated contribution.
    contribution: ConfigContribution,
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

/// Converts one rc entry handle into contribution entries.
fn push_rc_handle(contribution: &mut ConfigContribution, entry: &RcEntry) {
    let when = entry.when.clone();
    let priority = entry.priority;
    match &entry.payload {
        RcPayload::Alias { name, value } => contribution.aliases.push(AliasEntry {
            name: name.clone(),
            value: value.clone(),
            when,
            priority,
        }),
        RcPayload::Env { name, value } => contribution.envs.push(EnvEntry {
            name: name.clone(),
            value: value.clone(),
            when,
            priority,
        }),
        RcPayload::Profile { name, value } => contribution.profile.push(ProfileEntry {
            name: name.clone(),
            value: value.clone(),
            op: PathOp::Prepend,
            when,
            priority,
        }),
        RcPayload::ProfilePath { dir } => contribution.profile.push(ProfileEntry {
            name: "PATH".to_string(),
            value: dir.clone(),
            op: PathOp::Prepend,
            when,
            priority,
        }),
        RcPayload::Init { spec } => contribution.inits.push(match spec {
            RcSpec::Eval(argv) => InitEntry::Eval {
                argv: argv.clone(),
                when,
                priority,
            },
            RcSpec::Cmd(argv) => InitEntry::Cmd {
                argv: argv.clone(),
                when,
                priority,
            },
            RcSpec::Source(path) => InitEntry::Source {
                path: path.clone(),
                when,
                priority,
            },
        }),
    }
}

impl UserData for ConfigBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("add_artifact", |_, this, value: Value| {
            let config = this.contribution.name.clone();
            match value {
                Value::UserData(handle) => {
                    if let Ok(item) = handle.borrow::<LuaArtifact>() {
                        this.contribution.artifacts.push(PendingArtifact {
                            kind: item.kind,
                            path: item.path.clone(),
                            data: item.data.clone(),
                            priority: item.priority,
                        });
                        return Ok(());
                    }
                    if let Ok(entry) = handle.borrow::<RcEntry>() {
                        push_rc_handle(&mut this.contribution, &entry);
                        return Ok(());
                    }
                    Err(config_field_error(
                        &config,
                        "add_artifact",
                        "must be a confit.artifact value or rc entry",
                    ))
                }
                _ => Err(config_field_error(
                    &config,
                    "add_artifact",
                    "must be a confit.artifact value or rc entry",
                )),
            }
        });
    }
}
