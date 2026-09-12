//! Binding
//!
//! This module runs user configuration and translates its failures. Executing Lua
//! stays apart from both the stdlib surface and the planning logic.

use std::path::Path;

use mlua::{Table, Value};

use crate::error::{Error, Result};
use crate::framework::{ToolBuilder, ToolContribution};

/// Evaluated profile graph: declared shells plus tool contributions.
///
/// `shells` is non-empty; `tools` keeps profile order.
///
/// # Examples
///
/// A profile returning `{ shells = { "bash" }, tools = { bat } }` evaluates to one shell and
/// the `bat` contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGraph {
    /// Declared shell names, e.g. `["bash"]`.
    pub shells: Vec<String>,
    /// Tool contributions in profile order.
    pub tools: Vec<ToolContribution>,
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
/// Profile graph holding shells plus tool contributions.
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
    let lua = mlua::Lua::new();
    prepend_project_path(&lua, root)?;
    crate::framework::install_confit(&lua, root)?;
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
                "profile must return a table with 'shells' and 'tools'".to_string(),
            ));
        }
    };
    Ok(ProfileGraph {
        shells: read_shells(&table)?,
        tools: read_tools(&table)?,
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

/// Reads the profile tools field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Tool contributions in profile order.
///
/// # Errors
///
/// Fails with `Error::Config` for unreadable fields and for entries holding values of other shapes, naming the index.
fn read_tools(profile: &Table) -> Result<Vec<ToolContribution>> {
    let raw: Value = profile
        .get("tools")
        .map_err(|err| Error::Config(format!("profile: field 'tools' unreadable: {err}")))?;
    let list = match raw {
        Value::Table(list) => list,
        _ => {
            return Err(Error::Config(
                "profile: field 'tools' must be an array of tools".to_string(),
            ));
        }
    };
    let len = list.raw_len();
    let mut tools = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list
            .get(index)
            .map_err(|err| Error::Config(format!("profile: tools[{index}] unreadable: {err}")))?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ToolBuilder>() {
                Ok(builder) => tools.push(builder.contribution().clone()),
                Err(_) => {
                    return Err(Error::Config(format!(
                        "profile: tools[{index}] must be a tool (expected tool userdata)"
                    )));
                }
            },
            _ => {
                return Err(Error::Config(format!(
                    "profile: tools[{index}] must be a tool (expected tool userdata)"
                )));
            }
        }
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::InitEntryShape;

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
    fn minimal_tool_matches_fixture_shape() {
        let graph = run(
            &[(
                "tools/bat.lua",
                r#"
                local bat = confit.tool("bat", {
                  install = confit.mise.package({ name = "bat" }),
                })
                bat:alias("cat", "bat")
                return bat
                "#,
            )],
            r#"
            local bat = require("tools.bat")
            return {
              shells = { "bash" },
              tools = { bat },
            }
            "#,
        )
        .expect("evaluate");
        assert_eq!(graph.shells, vec!["bash"]);
        assert_eq!(graph.tools.len(), 1);
        let tool = &graph.tools[0];
        assert_eq!(tool.tool, "bat");
        assert!(tool.tags.is_empty());
        assert_eq!(
            tool.mise,
            Some(crate::framework::MiseSpec {
                name: "bat".to_string(),
                version: "latest".to_string(),
            })
        );
        assert_eq!(tool.aliases, vec![("cat".to_string(), "bat".to_string())]);
        assert!(tool.envs.is_empty());
        assert!(tool.profile_entries.is_empty());
        assert!(tool.profile_paths.is_empty());
        assert!(tool.inits.is_empty());
    }

    #[test]
    fn every_contribution_kind_lands() {
        let graph = run(
            &[],
            r#"
            local t = confit.tool("demo", {
              install = confit.mise.package({ name = "demo", version = "1.2.3" }),
            })
            t:alias("ll", "eza -l")
            t:env("EDITOR", "hx")
            t:profile("PATH", "/home/u/.cargo/bin")
            t:profile_path("/home/u/.local/bin")
            t:init({ eval = { "zoxide", "init", "bash" } })
            t:init({ cmd = { "task", "--completion", "bash" } })
            t:init({ source = "~/.cargo/env" })
            return { shells = { "bash", "zsh" }, tools = { t } }
            "#,
        )
        .expect("evaluate");
        assert_eq!(graph.shells, vec!["bash", "zsh"]);
        let tool = &graph.tools[0];
        assert_eq!(tool.tool, "demo");
        assert_eq!(tool.mise.as_ref().unwrap().version, "1.2.3");
        assert_eq!(tool.aliases, vec![("ll".to_string(), "eza -l".to_string())]);
        assert_eq!(tool.envs, vec![("EDITOR".to_string(), "hx".to_string())]);
        assert_eq!(
            tool.profile_entries,
            vec![("PATH".to_string(), "/home/u/.cargo/bin".to_string())]
        );
        assert_eq!(tool.profile_paths, vec!["/home/u/.local/bin"]);
        assert_eq!(
            tool.inits,
            vec![
                InitEntryShape::Eval(vec![
                    "zoxide".to_string(),
                    "init".to_string(),
                    "bash".to_string()
                ]),
                InitEntryShape::Cmd(vec![
                    "task".to_string(),
                    "--completion".to_string(),
                    "bash".to_string()
                ]),
                InitEntryShape::Source("~/.cargo/env".to_string()),
            ]
        );
    }

    #[test]
    fn function_value_names_tool_and_field() {
        let err = run(
            &[],
            r#"
            local t = confit.tool("bad", {})
            t:alias("x", function() end)
            return { shells = { "bash" }, tools = { t } }
            "#,
        )
        .expect_err("function value must fail");
        match err {
            Error::Lua(message) => {
                assert!(message.contains("bad"), "names tool: {message}");
                assert!(message.contains("alias"), "names field: {message}");
            }
            other => panic!("expected Error::Lua, got {other:?}"),
        }
    }

    #[test]
    fn malformed_return_tables_are_config_errors() {
        for profile in [
            "return {}",
            r#"return { shells = { "bash" } }"#,
            r#"return { shells = {}, tools = {} }"#,
            r#"return { shells = { "bash" }, tools = { "nope" } }"#,
            r#"return { shells = "bash", tools = {} }"#,
            "return 42",
        ] {
            let err = run(&[], profile).expect_err("must fail: {profile}");
            match err {
                Error::Config(message) => {
                    if profile.contains("\"nope\"") {
                        assert!(message.contains("tools[1]"), "names index: {message}");
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
                local g = confit.tool("greet", {})
                g:env("HELLO", "world")
                return g
                "#,
            )],
            r#"
            local greet = require("tools.greet")
            return { shells = { "bash" }, tools = { greet } }
            "#,
        )
        .expect("evaluate");
        assert_eq!(
            graph.tools[0].envs,
            vec![("HELLO".to_string(), "world".to_string())]
        );
    }

    #[test]
    fn init_and_mise_validation() {
        for (field, profile) in [
            (
                "init",
                r#"
                local t = confit.tool("demo", {})
                t:init({ eval = { "a" }, cmd = { "b" } })
                return { shells = { "bash" }, tools = { t } }
                "#,
            ),
            (
                "init.eval",
                r#"
                local t = confit.tool("demo", {})
                t:init({ eval = "nope" })
                return { shells = { "bash" }, tools = { t } }
                "#,
            ),
            (
                "init.eval",
                r#"
                local t = confit.tool("demo", {})
                t:init({ eval = { function() end } })
                return { shells = { "bash" }, tools = { t } }
                "#,
            ),
            (
                "init",
                r#"
                local t = confit.tool("demo", {})
                t:init({ source = { "x" } })
                return { shells = { "bash" }, tools = { t } }
                "#,
            ),
            (
                "mise.package",
                r#"
                local t = confit.tool("demo", {
                  install = confit.mise.package({}),
                })
                return { shells = { "bash" }, tools = { t } }
                "#,
            ),
        ] {
            let err = run(&[], profile).expect_err("must fail: {field}");
            match err {
                Error::Lua(message) => assert!(
                    message.contains("demo") || message.contains(field),
                    "names tool/field: {message}"
                ),
                other => panic!("expected Error::Lua for {field}, got {other:?}"),
            }
        }
    }
}
