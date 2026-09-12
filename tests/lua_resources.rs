//! Behavior tests: resources, artifact constructors, and append.
//!
//! Exercises the Lua surface through the lib API on tempfile project
//! roots: loads for all three formats, merge through constructors into
//! appended artifacts, and the `examples/1-structured_resource` plus
//! `examples/2-templated_resource` fixtures end to end. Error-domain checks assert root escapes and unknown merge keys
//! surface as plan errors, bad Lua types as Lua errors, and kind
//! mismatches on one path as merge errors.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use confit::actions::plan;
use confit::error::Error;
use confit::model::state::artifact::ArtifactData;
use confit::model::state::artifact::ArtifactKind;
use confit::services::plan::evaluate_profile;
use confit::services::render::render_artifact;

/// Fixture root inside the repo.
fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Write `files` into a temp project root; returns root and profile path.
fn project(files: &[(&str, &str)], profile: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write");
    }
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(&profile_path, profile).expect("write profile");
    (dir, profile_path)
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
fn loads_merge_and_constructors_land_in_plan() {
    let files = [
        ("base.toml", "x = 1\n[n]\ny = 1\n"),
        ("extra.json", r#"{"j": true, "n": {"z": 2}}"#),
        ("extra.yaml", "y: 9\n"),
    ];
    let profile = r#"
    local base = confit.resources.load_toml("base.toml")
    local extra = confit.resources.load_json("extra.json")
    local merged = confit.resources.merge(base, extra)
    local over = confit.resources.load_yaml("extra.yaml")
    merged = confit.resources.merge(merged, over)
    local t = confit.tool("demo", {})
    t:append_artifact(confit.artifact.toml("demo.toml", merged))
    t:append_artifact(confit.artifact.file("demo.txt", "hi"))
    t:append_artifact(confit.artifact.link("demo.link", "target"))
    return { shells = { "bash" }, tools = { t } }
    "#;
    let (dir, profile_path) = project(&files, profile);
    let graph = evaluate_profile(dir.path(), &profile_path).expect("evaluate");
    assert_eq!(graph.tools.len(), 1);
    assert_eq!(graph.tools[0].artifacts.len(), 3);

    let plan = plan(&graph, "root", "profile").expect("plan");
    let toml = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "demo.toml")
        .expect("toml artifact");
    assert_eq!(toml.kind, ArtifactKind::Toml);
    let ArtifactData::Toml(table) = &toml.data else {
        panic!("toml artifact holds toml data");
    };
    assert_eq!(table.get("x").and_then(serde_json::Value::as_i64), Some(1));
    assert_eq!(
        table.get("j").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(table.get("y").and_then(serde_json::Value::as_i64), Some(9));
    let nested = table.get("n").expect("nested table");
    assert_eq!(nested.get("y").and_then(serde_json::Value::as_i64), Some(1));
    assert_eq!(nested.get("z").and_then(serde_json::Value::as_i64), Some(2));
    assert!(!toml.data_hash.is_empty());
    let tools: Vec<&str> = toml
        .contributions
        .iter()
        .map(|contribution| contribution.tool.as_str())
        .collect();
    assert_eq!(tools, vec!["demo"]);

    let file = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "demo.txt")
        .expect("file artifact");
    assert_eq!(
        file.data,
        ArtifactData::File {
            content: "hi".to_string()
        }
    );
    let link = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "demo.link")
        .expect("link artifact");
    assert_eq!(
        link.data,
        ArtifactData::Link {
            target: "target".to_string()
        }
    );
}

#[test]
fn same_key_appends_merge_across_tools() {
    let profile = r#"
    local a = confit.tool("a", {})
    a:append_artifact(confit.artifact.toml("shared.toml", {x = 1, n = {y = 1}}))
    local b = confit.tool("b", {})
    b:append_artifact(confit.artifact.toml("shared.toml", {n = {z = 2}}))
    return { shells = { "bash" }, tools = { a, b } }
    "#;
    let (dir, profile_path) = project(&[], profile);
    let graph = evaluate_profile(dir.path(), &profile_path).expect("evaluate");
    let plan = plan(&graph, "root", "profile").expect("plan");
    let shared: Vec<_> = plan
        .artifacts
        .iter()
        .filter(|artifact| artifact.path == "shared.toml")
        .collect();
    assert_eq!(shared.len(), 1);
    let ArtifactData::Toml(table) = &shared[0].data else {
        panic!("shared artifact holds toml data");
    };
    let nested = table.get("n").expect("nested");
    assert_eq!(nested.get("y").and_then(serde_json::Value::as_i64), Some(1));
    assert_eq!(nested.get("z").and_then(serde_json::Value::as_i64), Some(2));
    let tools: Vec<&str> = shared[0]
        .contributions
        .iter()
        .map(|contribution| contribution.tool.as_str())
        .collect();
    assert_eq!(tools, vec!["a", "b"]);
}

#[test]
fn structured_resource_fixture_end_to_end() {
    let root = manifest().join("examples/1-structured_resource");
    let profile = root.join("profile.lua");
    let graph = evaluate_profile(&root, &profile).expect("fixture evaluates");
    assert_eq!(graph.shells, vec!["bash"]);
    assert_eq!(graph.tools.len(), 1);
    assert_eq!(graph.tools[0].tool, "starship");
    assert_eq!(graph.tools[0].artifacts.len(), 1);

    let plan = plan(&graph, "root", "profile").expect("plan");
    let starship = plan
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == ArtifactKind::Toml && artifact.path.ends_with("starship.toml")
        })
        .expect("starship artifact");
    let ArtifactData::Toml(table) = &starship.data else {
        panic!("starship artifact holds toml data");
    };
    assert_eq!(
        table
            .get("command_timeout")
            .and_then(serde_json::Value::as_i64),
        Some(10000)
    );
    assert_eq!(
        table.get("palette").and_then(serde_json::Value::as_str),
        Some("catppuccin")
    );

    let rc = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let ArtifactData::Rc(data) = &rc.data else {
        panic!("rc artifact holds rc data");
    };
    assert_eq!(data.aliases.get("s").map(String::as_str), Some("starship"));

    let mise = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.config/mise/config.toml")
        .expect("mise artifact");
    let ArtifactData::Toml(table) = &mise.data else {
        panic!("mise artifact holds toml data");
    };
    assert!(
        table
            .get("tools")
            .and_then(|tools| tools.get("starship"))
            .is_some(),
        "mise artifact lists starship"
    );
}

#[test]
fn templated_resource_fixture_end_to_end() {
    let root = manifest().join("examples/2-templated_resource");
    let profile = root.join("profile.lua");
    let graph = evaluate_profile(&root, &profile).expect("fixture evaluates");
    assert_eq!(graph.shells, vec!["bash"]);
    assert_eq!(graph.tools.len(), 1);
    assert_eq!(graph.tools[0].tool, "starship");
    assert_eq!(graph.tools[0].artifacts.len(), 1);

    let plan = plan(&graph, "root", "profile").expect("plan");
    let starship = plan
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == ArtifactKind::Template && artifact.path.ends_with("starship.toml")
        })
        .expect("starship artifact");
    let ArtifactData::Template { src, vars } = &starship.data else {
        panic!("starship artifact holds template data");
    };
    assert_eq!(src, "resources/starship.toml.j2");
    assert_eq!(
        vars.get("timeout").and_then(serde_json::Value::as_i64),
        Some(10000)
    );
    assert_eq!(
        vars.get("palette").and_then(serde_json::Value::as_str),
        Some("catppuccin")
    );

    let fs = confit::repository::OsFilesystem;
    let rendered = render_artifact(&starship.data, &starship.blame, &fs, &root).expect("render");
    assert_eq!(
        rendered,
        b"command_timeout = 10000\npalette = \"catppuccin\"".to_vec()
    );
}

#[test]
fn escapes_and_unknown_keys_are_plan_errors() {
    let (dir, _) = project(
        &[("inside.toml", "x = 1\n")],
        r#"return { shells = { "bash" }, tools = {} }"#,
    );
    let root = dir.path().to_path_buf();
    for lua_case in [
        r#"local t = confit.tool("demo", {})
           local _ = confit.resources.load_toml("../escape.toml")
           return { shells = { "bash" }, tools = { t } }"#,
        r#"local t = confit.tool("demo", {})
           local _ = confit.resources.load_json("/abs.json")
           return { shells = { "bash" }, tools = { t } }"#,
        r#"local t = confit.tool("demo", {})
           local _ = confit.resources.merge({a = 1}, {b = 2}, {bogus = true})
           return { shells = { "bash" }, tools = { t } }"#,
    ] {
        let path = root.join("profile.lua");
        std::fs::write(&path, lua_case).expect("write profile");
        let err = evaluate_profile(&root, &path).expect_err("must fail");
        assert!(is_plan(&err), "plan domain: {err}");
    }
}

#[test]
fn bad_lua_types_are_lua_errors() {
    let (dir, _) = project(&[], "");
    let root = dir.path().to_path_buf();
    for lua_case in [
        r#"local t = confit.tool("demo", {})
           t:append_artifact(confit.artifact.toml("p", {f = function() end}))
           return { shells = { "bash" }, tools = { t } }"#,
        r#"local t = confit.tool("demo", {})
           t:append_artifact("nope")
           return { shells = { "bash" }, tools = { t } }"#,
    ] {
        let path = root.join("profile.lua");
        std::fs::write(&path, lua_case).expect("write profile");
        let err = evaluate_profile(&root, &path).expect_err("must fail");
        assert!(is_lua(&err), "Lua domain: {err}");
    }
}

#[test]
fn kind_mismatch_on_one_path_stays_merge_error() {
    let profile = r#"
    local a = confit.tool("a", {})
    a:append_artifact(confit.artifact.toml("shared", {x = 1}))
    local b = confit.tool("b", {})
    b:append_artifact(confit.artifact.json("shared", {x = 1}))
    return { shells = { "bash" }, tools = { a, b } }
    "#;
    let (dir, profile_path) = project(&[], profile);
    let graph = evaluate_profile(dir.path(), &profile_path).expect("evaluate");
    let err = plan(&graph, "root", "profile").expect_err("must fail");
    match err {
        Error::Merge(message) => {
            assert!(message.contains("shared"), "names path: {message}");
        }
        other => panic!("expected Error::Merge, got {other:?}"),
    }
}
