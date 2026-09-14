//! Behavior tests: lazy plugin loading plus scoped requires plus helper errors.
//!
//! Exercises `confit.plugin` through the lib API on tempfile project roots:
//! the embedded mise namespace, lazy external plugins from a tempfile plugins
//! folder, collision note-and-skip, traversal rejection, scoped require
//! across sibling files, `@` chunk names in sibling failures, and the
//! plan-domain `helpers.error` attribution.
//!
//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use confit::binding::evaluate;
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
    evaluate(dir.path(), profile, plugins)
}

/// Reads one env entry from merged rc.
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
    let merged = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let confit::model::state::document::DocumentData::Rc(data) = &merged.data else {
        panic!("rc data");
    };
    data.env
        .iter()
        .find(|entry| entry.spec.name == name)
        .expect("env entry")
        .spec
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
fn embedded_mise_loads_eagerly() {
    let profile = r#"
    local mise = confit.plugin.solrachq.mise
    local c = mise.package("demo", function(rc)
        rc:alias("ll", "eza -l")
    end)
    c:add_document(mise.activate())
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.name, "demo");
    let merged = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let confit::model::state::document::DocumentData::Rc(data) = &merged.data else {
        panic!("rc data");
    };
    let entry = data
        .aliases
        .iter()
        .find(|item| item.spec.name == "ll")
        .expect("alias");
    assert_eq!(entry.spec.value, "eza -l");
    assert!(
        matches!(&entry.when, Some(Condition::InPath { name }) if name == "demo"),
        "callback alias carries the in_path guard",
    );
}

#[test]
fn mise_declares_mise_toml_and_activation_entry() {
    let profile = r#"
    local mise = confit.plugin.solrachq.mise
    local c = mise.package("demo")
    c:add_document(mise.activate())
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.documents.len(), 2);
    assert!(
        config
            .documents
            .iter()
            .any(|item| item.path == "~/.config/mise/config.toml")
    );
    assert!(config.documents.iter().any(|item| item.path == "rc"));
    let merged = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let confit::model::state::document::DocumentData::Rc(data) = &merged.data else {
        panic!("rc data");
    };
    assert_eq!(data.init.len(), 1);
    match &data.init[0].spec {
        confit::model::state::rc::InitSpec::Eval { argv, .. } => {
            assert_eq!(
                argv.clone(),
                vec![
                    "mise".to_string(),
                    "activate".to_string(),
                    "{{shell}}".to_string()
                ]
            );
            assert!(
                data.init[0].when.is_none(),
                "activation stays unconditional"
            );
        }
        other => panic!("activation holds eval init, got {other:?}"),
    }
}

#[test]
fn mise_mistakes_are_plan_errors_with_attribution() {
    for snippet in [
        r#"confit.plugin.solrachq.mise.package(42)"#,
        r#"confit.plugin.solrachq.mise.package("")"#,
        r#"confit.plugin.solrachq.mise.package("demo", 42)"#,
        r#"confit.plugin.solrachq.mise.activate(42)"#,
    ] {
        let profile =
            format!("local c = {snippet}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}");
        let (dir, profile_path, _) = project(&profile, &[]);
        let err = run(&dir, &profile_path, None).expect_err("must fail");
        assert!(is_plan(&err), "mise mistake is plan: {err}");
        let message = format!("{err}");
        assert!(
            message.contains("mise"),
            "attributes plugin file: {message}"
        );
    }
}

#[test]
fn external_plugin_resolves_lazily() {
    let profile = r#"
    local c = confit.config("demo")
    c:add_document(confit.document.rc.env("GREETING", confit.plugin.acme.widget.GREETING))
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
    local mise = confit.plugin.solrachq.mise
    local c = mise.package("demo")
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, root) = project(
        profile,
        &[(
            "solrachq/mise/plugin.lua",
            r#"return { MARKER = "external" }"#,
        )],
    );
    let graph = run(&dir, &profile_path, Some(&root)).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    assert_eq!(graph.configs[0].name, "demo");
}

#[test]
fn merge_plugin_deep_merges_with_opts() {
    let profile = r#"
    local merge = confit.plugin.solrachq.merge
    local merged = merge({a = 1, n = {x = 1, y = 1}, l = {1, 2}}, {n = {y = 2}, l = {3}})
    assert(merged.a == 1 and merged.n.x == 1 and merged.n.y == 2 and #merged.l == 1 and merged.l[1] == 3, "deep merge")
    local appended = merge({l = {1, 2}}, {l = {2, 3}}, {list_append = true})
    assert(#appended.l == 4, "list append")
    local shallow = merge({n = {x = 1, y = 1}, keep = 1}, {n = {y = 2}}, {shallow = true})
    assert(shallow.n.x == nil and shallow.n.y == 2 and shallow.keep == 1, "shallow")
    local c = confit.config("demo")
    c:add_document(confit.document.structured("toml", { path = "demo.toml", data = merged }))
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
}

#[test]
fn merge_plugin_unknown_option_is_plan_error() {
    let profile = r#"
    local merge = confit.plugin.solrachq.merge
    local _ = merge({a = 1}, {b = 2}, {bogus = true})
    local c = confit.config("demo")
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    let err = run(&dir, &profile_path, None).expect_err("must fail");
    assert!(is_plan(&err), "unknown key is plan: {err}");
    assert!(format!("{err}").contains("bogus"), "names key: {err}");
}

#[test]
fn template_plugin_renders_text_documents() {
    let profile = r#"
    local c = confit.config("demo")
    c:add_document(confit.plugin.solrachq.template("demo.txt", { src = "greet.txt", vars = { name = "ada" } }))
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path, _) = project(profile, &[]);
    std::fs::write(dir.path().join("greet.txt"), "hi {{ name }}").expect("write template");
    let graph = run(&dir, &profile_path, None).expect("evaluate");
    let document = graph.configs[0]
        .documents
        .iter()
        .find(|item| item.path == "demo.txt")
        .expect("text document");
    let confit::model::state::document::DocumentData::Text { content } = &document.data else {
        panic!("text data");
    };
    assert_eq!(content, "hi ada");
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
    c:add_document(confit.document.rc.env("VALUE", confit.plugin.acme.multi.VALUE))
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
