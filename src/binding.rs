//! Binding
//!
//! This module runs user configuration and translates its failures. Executing Lua
//! stays apart from both the stdlib surface and the planning logic.

use std::path::Path;

use mlua::{Table, Value};

use crate::error::{Error, Result};
use crate::framework::ConfigBuilder;
use crate::model::state::config::ConfigContribution;

/// Evaluated profile graph: declared shells plus config contributions.
///
/// `shells` plus `configs` stay non-empty and keep profile order.
///
/// # Examples
///
/// A profile returning `{ shells = { "bash" }, configs = { web } }` evaluates to one shell and
/// the `web` contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGraph {
    /// Declared shell names, e.g. `["bash"]`.
    pub shells: Vec<String>,
    /// Config contributions in profile order.
    pub configs: Vec<ConfigContribution>,
}

/// Evaluates the profile file into a profile graph.
///
/// # Arguments
///
/// * `root` - project root for module resolution.
/// * `profile` - profile file path.
///
/// # Returns
///
/// Profile graph holding shells plus config contributions.
///
/// # Errors
///
/// Fails with `Error::Lua` for evaluation failures and `Error::Config` for malformed return tables.
///
/// # Examples
/// ```rust,no_run
/// use std::path::Path;
/// use confit::binding::evaluate;
///
/// let graph = match evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")) {
///     Ok(graph) => graph,
///     Err(error) => panic!("fixture evaluates: {error}"),
/// };
/// assert_eq!(graph.shells, vec!["bash"]);
/// ```
pub fn evaluate(root: &Path, profile: &Path) -> Result<ProfileGraph> {
    evaluate_with_plugins(root, profile, None)
}

/// Evaluates the profile file into a profile graph with plugins available.
///
/// # Arguments
///
/// * `root` - project root for module resolution.
/// * `profile` - profile file path.
/// * `plugins` - external plugins folder shaped `{user}/{name}/plugin.lua`,
///   empty keeps embedded defaults only.
///
/// # Returns
///
/// Profile graph holding shells plus config contributions.
///
/// # Errors
///
/// Fails with `Error::Lua` for evaluation failures and `Error::Config` for malformed return tables.
pub fn evaluate_with_plugins(
    root: &Path,
    profile: &Path,
    plugins: Option<&Path>,
) -> Result<ProfileGraph> {
    let lua = mlua::Lua::new();
    prepend_project_path(&lua, root)?;
    crate::framework::install_confit_with_plugins(&lua, root, plugins)?;
    let source = std::fs::read(profile)?;
    let returned: Value = lua
        .load(&source)
        .set_name(format!("@{}", profile.display()))
        .call(())
        .map_err(wrap_mlua)?;
    let table = match returned {
        Value::Table(table) => table,
        _ => {
            return Err(Error::Config(
                "profile must return a table with 'shells' and 'configs'".to_string(),
            ));
        }
    };
    Ok(ProfileGraph {
        shells: read_shells(&table)?,
        configs: read_configs(&table)?,
    })
}

/// Maps an mlua failure onto the crate error.
///
/// # Arguments
///
/// * `err` - mlua failure.
///
/// # Returns
///
/// Crate error for the failure.
fn wrap_mlua(err: mlua::Error) -> Error {
    if let Some(Error::Lua(message)) = err.downcast_ref::<Error>() {
        return Error::Lua(message.clone());
    }
    // Preserve plan-domain failures (root escapes, unknown merge
    // keys) raised inside resource callbacks.
    if let Some(Error::Plan(message)) = err.downcast_ref::<Error>() {
        return Error::Plan(message.clone());
    }
    Error::Lua(err.to_string())
}

/// Extends Lua module resolution with the project root.
///
/// # Arguments
///
/// * `lua` - Lua state receiving the extended path.
/// * `root` - project root for resolution.
///
/// # Errors
///
/// Fails with `Error::Lua` for package path access failures.
fn prepend_project_path(lua: &mlua::Lua, root: &Path) -> Result<()> {
    let package: Table = lua.globals().get("package").map_err(wrap_mlua)?;
    let previous: String = package.get("path").map_err(wrap_mlua)?;
    let root = root.display();
    package
        .set("path", format!("{root}/?.lua;{root}/?/init.lua;{previous}"))
        .map_err(wrap_mlua)?;
    Ok(())
}

/// Reads a dense string array from a Lua table.
///
/// # Arguments
///
/// * `table` - Lua table holding the array.
/// * `what` - error message naming the field.
///
/// # Returns
///
/// String values in index order.
///
/// # Errors
///
/// Fails with `Error::Config` for gaps in index order and for entries holding values of other shapes.
fn read_string_array(table: &Table, what: &'static str) -> Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|_| Error::Config(what.to_string()))?;
        let index = match key {
            Value::Integer(index) => index,
            _ => return Err(Error::Config(what.to_string())),
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => return Err(Error::Config(what.to_string())),
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(Error::Config(what.to_string()));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Reads the profile shells field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Declared shell names.
///
/// # Errors
///
/// Fails with `Error::Config` for absent fields and for empty arrays and for malformed entries.
fn read_shells(profile: &Table) -> Result<Vec<String>> {
    const WHAT: &str = "profile: field 'shells' must be a non-empty string array";
    let raw: Value = profile
        .get("shells")
        .map_err(|_| Error::Config(WHAT.to_string()))?;
    let list = match raw {
        Value::Table(list) => list,
        _ => return Err(Error::Config(WHAT.to_string())),
    };
    let shells = read_string_array(&list, WHAT)?;
    if shells.is_empty() {
        return Err(Error::Config(WHAT.to_string()));
    }
    Ok(shells)
}

/// Reads the profile configs field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Config contributions in profile order.
///
/// # Errors
///
/// Fails with `Error::Config` for absent fields and for empty arrays and for malformed entries, naming the index.
fn read_configs(profile: &Table) -> Result<Vec<ConfigContribution>> {
    const WHAT: &str = "profile: field 'configs' must be a non-empty array of configs";
    let raw: Value = profile
        .get("configs")
        .map_err(|_| Error::Config(WHAT.to_string()))?;
    let list = match raw {
        Value::Nil => return Err(Error::Config(WHAT.to_string())),
        Value::Table(list) => list,
        _ => {
            return Err(Error::Config(WHAT.to_string()));
        }
    };
    let len = list.raw_len();
    let mut configs = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list
            .get(index)
            .map_err(|err| Error::Config(format!("profile: configs[{index}] unreadable: {err}")))?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ConfigBuilder>() {
                Ok(builder) => configs.push(builder.contribution().clone()),
                Err(_) => {
                    return Err(Error::Config(format!(
                        "profile: configs[{index}] must be a config (expected config userdata)"
                    )));
                }
            },
            _ => {
                return Err(Error::Config(format!(
                    "profile: configs[{index}] must be a config (expected config userdata)"
                )));
            }
        }
    }
    if configs.is_empty() {
        return Err(Error::Config(WHAT.to_string()));
    }
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::state::rc::InitEntry;

    /// Builds a temp project root holding fixture files.
    ///
    /// # Arguments
    ///
    /// * `files` - relative paths plus contents.
    ///
    /// # Returns
    ///
    /// Temp directory holding the written files.
    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(path, contents).expect("write");
        }
        dir
    }

    /// Evaluates a profile fixture inside a temp root.
    ///
    /// # Arguments
    ///
    /// * `files` - sibling project files beside the profile.
    /// * `profile` - profile source written as profile.lua.
    ///
    /// # Returns
    ///
    /// Evaluated profile graph.
    ///
    /// # Errors
    ///
    /// Fails with evaluation errors from malformed fixtures.
    fn run(files: &[(&str, &str)], profile: &str) -> Result<ProfileGraph> {
        let mut all = vec![("profile.lua", profile)];
        all.extend_from_slice(files);
        let dir = project(&all);
        evaluate(dir.path(), &dir.path().join("profile.lua"))
    }

    #[test]
    fn minimal_config_matches_fixture_shape() {
        let graph = run(
            &[(
                "tools/bat.lua",
                r#"
                local bat = confit.config("bat")
                bat:add_artifact(confit.artifact.rc.alias("cat", "bat"))
                return bat
                "#,
            )],
            r#"
            local bat = require("tools.bat")
            return {
              shells = { "bash" },
              configs = { bat },
            }
            "#,
        )
        .expect("evaluate");
        assert_eq!(graph.shells, vec!["bash"]);
        assert_eq!(graph.configs.len(), 1);
        let config = &graph.configs[0];
        assert_eq!(config.name, "bat");
        assert_eq!(config.aliases.len(), 1);
        assert_eq!(config.aliases[0].name, "cat");
        assert_eq!(config.aliases[0].value, "bat");
        assert!(config.envs.is_empty());
        assert!(config.profile.is_empty());
        assert!(config.inits.is_empty());
    }

    #[test]
    fn every_contribution_kind_lands() {
        let graph = run(
            &[],
            r#"
            local c = confit.config("demo")
            c:add_artifact(confit.artifact.rc.alias("ll", "eza -l"))
            c:add_artifact(confit.artifact.rc.env("EDITOR", "hx"))
            c:add_artifact(confit.artifact.rc.profile("PATH", "/home/u/.cargo/bin"))
            c:add_artifact(confit.artifact.rc.profile_path("/home/u/.local/bin"))
            c:add_artifact(confit.artifact.rc.init({ eval = { "zoxide", "init", "bash" } }))
            c:add_artifact(confit.artifact.rc.init({ cmd = { "task", "--completion", "bash" } }))
            c:add_artifact(confit.artifact.rc.init({ source = "~/.cargo/env" }))
            return { shells = { "bash", "zsh" }, configs = { c } }
            "#,
        )
        .expect("evaluate");
        assert_eq!(graph.shells, vec!["bash", "zsh"]);
        let config = &graph.configs[0];
        assert_eq!(config.name, "demo");
        assert_eq!(config.aliases.len(), 1);
        assert_eq!(config.aliases[0].name, "ll");
        assert_eq!(config.envs.len(), 1);
        assert_eq!(config.envs[0].name, "EDITOR");
        assert_eq!(config.profile.len(), 2);
        assert_eq!(config.profile[0].name, "PATH");
        assert_eq!(config.profile[0].value, "/home/u/.cargo/bin");
        assert_eq!(config.profile[1].name, "PATH");
        assert_eq!(config.profile[1].value, "/home/u/.local/bin");
        assert_eq!(config.inits.len(), 3);
        assert!(matches!(config.inits[0], InitEntry::Eval { .. }));
        assert!(matches!(config.inits[1], InitEntry::Cmd { .. }));
        assert!(matches!(config.inits[2], InitEntry::Source { .. }));
    }

    #[test]
    fn malformed_return_tables_are_config_errors() {
        for profile in [
            "return {}",
            r#"return { shells = {}, configs = {} }"#,
            r#"return { shells = { "bash" } }"#,
            r#"return { configs = { confit.config("a") } }"#,
            r#"return { shells = { "bash" }, configs = {} }"#,
            r#"return { shells = { "bash" }, configs = { "nope" } }"#,
            r#"return { shells = "bash", configs = {} }"#,
            r#"return { shells = { "bash" }, configs = "nope" }"#,
            "return 42",
        ] {
            let err = run(&[], profile).expect_err("must fail: {profile}");
            match err {
                Error::Config(message) => {
                    if profile.contains("configs = { \"nope\" }") {
                        assert!(message.contains("configs[1]"), "names index: {message}");
                    }
                }
                other => panic!("expected Error::Config for {profile}, got {other:?}"),
            }
        }
    }

    #[test]
    fn require_resolves_sibling_module_via_root() {
        let graph = run(
            &[(
                "tools/greet.lua",
                r#"
                local g = confit.config("greet")
                g:add_artifact(confit.artifact.rc.env("HELLO", "world"))
                return g
                "#,
            )],
            r#"
            local greet = require("tools.greet")
            return { shells = { "bash" }, configs = { greet } }
            "#,
        )
        .expect("evaluate");
        assert_eq!(graph.configs[0].envs.len(), 1);
        assert_eq!(graph.configs[0].envs[0].name, "HELLO");
        assert_eq!(graph.configs[0].envs[0].value, "world");
    }
}
