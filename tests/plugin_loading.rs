//! Behavior tests: lazy plugin loading plus scoped requires plus helper errors.
//!
//! Exercises `confit.plugin` through the lib API on tempfile project roots:
//! the embedded mise_package, lazy external plugins from a tempfile plugins
//! folder, collision note-and-skip, traversal rejection, scoped require
//! across sibling files, `@` chunk names in sibling failures, and the
//! plan-domain `helpers.error` attribution.
//!
//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use confit::binding::evaluate_with_plugins;
use confit::error::Error;
use confit::model::state::condition::Condition;

/// Writes a profile plus plugin files into a temp project root.
///
/// # Arguments
///
/// * `profile` - profile source written as profile.lua.
/// * `plugins` - paths plus contents under the `plugins` folder.
///
/// # Returns
///
/// Temp directory plus profile path plus plugins folder.
fn project(profile: &str, plugins: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(&profile_path, profile).expect("write profile");
    let root = dir.path().join("plugins");
    for (name, contents) in plugins {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write");
    }
    (dir, profile_path, root)
}

/// Evaluates a profile with an optional plugins folder.
///
/// # Arguments
///
/// * `dir` - temp project root.
/// * `profile` - profile path.
/// * `plugins` - plugins folder, empty keeps embedded defaults only.
///
/// # Returns
///
/// Evaluated profile graph.
///
/// # Errors
///
/// Fails with evaluation errors from malformed fixtures.
fn run(
    dir: &tempfile::TempDir,
    profile: &Path,
    plugins: Option<&Path>,
) -> Result<confit::binding::ProfileGraph, Error> {
    evaluate_with_plugins(dir.path(), profile, plugins)
}

/// Reads one env entry from the first config contribution.
///
/// # Arguments
///
/// * `graph` - evaluated profile graph.
/// * `name` - env entry name.
///
/// # Returns
///
/// Env entry value.
fn env_of(graph: &confit::binding::ProfileGraph, name: &str) -> String {
    graph.configs[0]
        .envs
        .iter()
        .find(|entry| entry.name == name)
        .expect("env entry")
        .value
        .clone()
}

/// True when `err` is the crate plan-domain error.
fn is_plan(err: &Error) -> bool {
    matches!(err, Error::Plan(_))
}

/// True when `err` is the crate Lua-domain error.
fn is_lua(err: &Error) -> bool {
    matches!(err, Error::Lua(_))
}

#[test]
fn embedded_mise_package_loads_eagerly() {
    let profile = r#"
    local mise_package = confit.plugin.solrachq.mise_package
    local c = mise_package("demo", function(rc)
        rc:alias("ll", "eza -l")
    end)
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.name, "demo");
    assert_eq!(config.aliases.len(), 1);
    assert_eq!(config.aliases[0].name, "ll");
    assert_eq!(config.aliases[0].value, "eza -l");
    assert!(
        matches!(&config.aliases[0].when, Some(Condition::InPath { name }) if name == "demo"),
        "callback alias carries the in_path guard",
    );
}

#[test]
fn mise_package_declares_mise_toml_and_template_activation() {
    let profile = r#"
    local mise_package = confit.plugin.solrachq.mise_package
    local c = mise_package("demo")
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.artifacts.len(), 1);
    assert_eq!(config.artifacts[0].path, "~/.config/mise/config.toml");
    assert_eq!(config.inits.len(), 1);
    match &config.inits[0] {
        confit::model::state::rc::InitEntry::Eval { argv, when, .. } => {
            assert_eq!(
                argv,
                &vec![
                    "mise".to_string(),
                    "activate".to_string(),
                    "{{shell}}".to_string()
                ]
            );
            assert!(when.is_none(), "activation stays unconditional");
        }
        other => panic!("activation holds eval init, got {other:?}"),
    }
}

#[test]
fn mise_package_mistakes_are_plan_errors_with_attribution() {
    for snippet in [
        r#"confit.plugin.solrachq.mise_package(42)"#,
        r#"confit.plugin.solrachq.mise_package("")"#,
        r#"confit.plugin.solrachq.mise_package("demo", 42)"#,
    ] {
        let profile =
            format!("local c = {snippet}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}");
        let (dir, profile_path, _) = project(&profile, &[]);
        let err = run(&dir, &profile_path, None).expect_err("must fail");
        assert!(is_plan(&err), "mise_package mistake is plan: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("mise_package"),
            "attributes plugin file: {message}"
        );
    }
}

#[test]
fn external_plugin_resolves_lazily() {
    let profile = r#"
    local c = confit.config("demo")
    c:add_artifact(confit.artifact.rc.env("GREETING", confit.plugin.acme.widget.GREETING))
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[(
            "acme/widget/plugin.lua",
            r#"return { GREETING = "hello-widget" }"#,
        )],
    );
    let graph = run(&dir, &profile_path, Some(&root)).expect("evaluate");
    assert_eq!(env_of(&graph, "GREETING"), "hello-widget");
}

#[test]
fn unaccessed_external_plugins_stay_unloaded() {
    let profile =
        r#"local c = confit.config("demo") return { shells = { "bash" }, configs = { c } }"#;
    let (dir, profile_path, root) = project(
        profile,
        &[("broken/oops/plugin.lua", "this is not lua at all !!!")],
    );
    run(&dir, &profile_path, Some(&root)).expect("broken plugin stays unloaded");
}

#[test]
fn collision_keeps_embedded_default() {
    let profile = r#"
    local mise_package = confit.plugin.solrachq.mise_package
    local c = mise_package("demo")
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[(
            "solrachq/mise_package/plugin.lua",
            r#"return { MARKER = "external" }"#,
        )],
    );
    let graph = run(&dir, &profile_path, Some(&root)).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    assert_eq!(graph.configs[0].name, "demo");
}

#[test]
fn traversal_is_plan_error() {
    let loader = r#"
    local c = confit.config("demo")
    local _ = confit.plugin.evil.evil
    return { shells = { "bash" }, configs = { c } }
    "#;
    for plugins in [
        vec![(
            "evil/evil/plugin.lua",
            r#"local _ = require("../escape")
        return {}"#,
        )],
        vec![(
            "evil/evil/plugin.lua",
            r#"local _ = require("/abs")
        return {}"#,
        )],
    ] {
        let (dir, profile_path, root) = project(loader, &plugins);
        let err = run(&dir, &profile_path, Some(&root)).expect_err("must fail");
        assert!(is_plan(&err), "traversal is plan: {err}");
    }

    let index = r#"
    local c = confit.config("demo")
    local _ = confit.plugin[".."]
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(index, &[]);
    let err = run(&dir, &profile_path, Some(&root)).expect_err("must fail");
    assert!(is_plan(&err), "index traversal is plan: {err}");
}

#[test]
fn scoped_require_shares_sibling_files() {
    let profile = r#"
    local c = confit.config("demo")
    c:add_artifact(confit.artifact.rc.env("VALUE", confit.plugin.acme.multi.VALUE))
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[
            (
                "acme/multi/plugin.lua",
                r#"
                local first = require("util")
                local second = require("util")
                assert(first == second, "sibling cache shares one value")
                return { VALUE = first.base .. "-suffix" }
                "#,
            ),
            ("acme/multi/util.lua", r#"return { base = "shared" }"#),
        ],
    );
    let graph = run(&dir, &profile_path, Some(&root)).expect("evaluate");
    assert_eq!(env_of(&graph, "VALUE"), "shared-suffix");
}

#[test]
fn sibling_failures_carry_file_positions() {
    let profile = r#"
    local c = confit.config("demo")
    local _ = confit.plugin.acme.fragile.VALUE
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[
            (
                "acme/fragile/plugin.lua",
                r#"local u = require("util")
                return { VALUE = u.base }"#,
            ),
            ("acme/fragile/util.lua", r#"error("marker-boom")"#),
        ],
    );
    let err = run(&dir, &profile_path, Some(&root)).expect_err("must fail");
    assert!(is_lua(&err), "sibling failure is Lua: {err}");
    let message = format!("{err}");
    assert!(
        message.contains("util.lua"),
        "names sibling file: {message}"
    );
    assert!(
        message.contains("marker-boom"),
        "carries message: {message}"
    );
}

#[test]
fn helpers_error_is_plan_error_with_attribution() {
    let profile = r#"
    local c = confit.config("demo")
    local _ = confit.plugin.acme.failer
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[(
            "acme/failer/plugin.lua",
            r#"confit.plugin.helpers.error("bad input")"#,
        )],
    );
    let err = run(&dir, &profile_path, Some(&root)).expect_err("must fail");
    assert!(is_plan(&err), "helpers.error is plan: {err}");
    let message = format!("{err}");
    assert!(message.contains("bad input"), "carries message: {message}");
    assert!(
        message.contains("failer/plugin.lua"),
        "attributes plugin file: {message}"
    );

    let direct = r#"
    local c = confit.config("demo")
    confit.plugin.helpers.error(42)
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(direct, &[]);
    let err = run(&dir, &profile_path, Some(&root)).expect_err("must fail");
    assert!(is_lua(&err), "non-string message is Lua: {err}");
    assert!(format!("{err}").contains("message"), "names field: {err}");
}
