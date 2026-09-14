//! Behavior tests: resources, document constructors, and config attach.
//!
//! Exercises the Lua surface through the lib API on tempfile project
//! roots: loads for all three formats plus text, merge plugin through
//! constructors into config documents, and the
//! `examples/1-structured_resource` plus `examples/2-templated_resource`
//! fixtures end to end. Error-domain checks assert root escapes and
//! unknown merge keys surface as plan errors, bad Lua types as Lua errors,
//! and kind mismatches on one path as merge errors.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use confit::binding::evaluate;
use confit::error::Error;
use confit::model::state::document::DocumentData;
use confit::model::state::document::DocumentKind;
use confit::model::state::document::StructuredFormat;
use confit::services::plan::build_plan;

mod common;

/// Fixture root inside the repo.
fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Write `files` into a temp project root; returns root and profile path.
fn project(files: &[(&str, &str)], profile: &str) -> (tempfile::TempDir, PathBuf) {
    common::project(files, profile)
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
        ("note.txt", "hi\n"),
    ];
    let profile = r#"
    local merge = confit.plugin.solrachq.merge
    local base = confit.resources.load_toml("base.toml")
    local extra = confit.resources.load_json("extra.json")
    local merged = merge(base, extra)
    local over = confit.resources.load_yaml("extra.yaml")
    merged = merge(merged, over)
    local text = confit.resources.load_text("note.txt")
    local c = confit.config("demo")
    c:add_document(confit.document.structured("toml", { path = "demo.toml", data = merged }))
    c:add_document(confit.document.text("demo.txt", "hi"))
    c:add_document(confit.document.link("demo.link", "target"))
    assert(text == "hi\n", "load_text reads bytes")
    return { shells = { "bash" }, configs = { c } }
    "#;
    let (dir, profile_path) = project(&files, profile);
    let graph = evaluate(dir.path(), &profile_path, None).expect("evaluate");
    assert_eq!(graph.configs.len(), 1);
    assert_eq!(graph.configs[0].documents.len(), 3);

    let plan = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    let toml = plan
        .documents
        .iter()
        .find(|document| document.path == "demo.toml")
        .expect("toml document");
    assert_eq!(toml.kind, DocumentKind::Structured);
    let DocumentData::Structured {
        format,
        data: table,
    } = &toml.data
    else {
        panic!("toml document holds structured data");
    };
    assert_eq!(*format, StructuredFormat::Toml);
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

    let file = plan
        .documents
        .iter()
        .find(|document| document.path == "demo.txt")
        .expect("file document");
    assert_eq!(
        file.data,
        DocumentData::Text {
            content: "hi".to_string()
        }
    );
    let link = plan
        .documents
        .iter()
        .find(|document| document.path == "demo.link")
        .expect("link document");
    assert_eq!(
        link.data,
        DocumentData::Link {
            target: "target".to_string()
        }
    );
}

#[test]
fn same_path_across_configs_is_a_repeat_error() {
    let profile = r#"
    local a = confit.config("a")
    a:add_document(confit.document.structured("toml", { path = "shared.toml", data = {x = 1, n = {y = 1}} }))
    local b = confit.config("b")
    b:add_document(confit.document.structured("toml", { path = "shared.toml", data = {n = {z = 2}} }))
    return { shells = { "bash" }, configs = { a, b } }
    "#;
    let (dir, profile_path) = project(&[], profile);
    let err = evaluate(dir.path(), &profile_path, None).expect_err("must fail");
    match err {
        Error::Config(message) => {
            assert!(message.contains("shared.toml"), "names path: {message}");
        }
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

#[test]
fn structured_resource_fixture_end_to_end() {
    let root = manifest().join("examples/1-structured_resource");
    let profile = root.join("profile.lua");
    let graph = evaluate(&root, &profile, None).expect("fixture evaluates");
    assert_eq!(graph.shells, vec!["bash"]);
    assert_eq!(graph.configs.len(), 1);
    assert_eq!(graph.configs[0].name, "starship");
    assert_eq!(graph.configs[0].documents.len(), 3);

    let plan = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    let starship = plan
        .documents
        .iter()
        .find(|document| {
            document.kind == DocumentKind::Structured && document.path.ends_with("starship.toml")
        })
        .expect("starship document");
    let DocumentData::Structured {
        format,
        data: table,
    } = &starship.data
    else {
        panic!("starship document holds structured data");
    };
    assert_eq!(*format, StructuredFormat::Toml);
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
        .documents
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc document");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc document holds rc data");
    };
    assert_eq!(
        data.aliases
            .iter()
            .find(|entry| entry.spec.name == "s")
            .map(|entry| entry.spec.value.as_str()),
        Some("starship")
    );

    let mise = plan
        .documents
        .iter()
        .find(|document| document.path == "~/.config/mise/config.toml")
        .expect("mise document");
    let DocumentData::Structured {
        format,
        data: table,
    } = &mise.data
    else {
        panic!("mise document holds structured data");
    };
    assert_eq!(*format, StructuredFormat::Toml);
    assert!(
        table
            .get("tools")
            .and_then(|tools| tools.get("starship"))
            .is_some(),
        "mise document lists starship"
    );
}

#[test]
fn templated_resource_fixture_evaluates() {
    let root = manifest().join("examples/2-templated_resource");
    let profile = root.join("profile.lua");
    let graph = evaluate(&root, &profile, None).expect("fixture evaluates");
    assert_eq!(graph.shells, vec!["bash"]);
    assert_eq!(graph.configs.len(), 1);
    assert_eq!(graph.configs[0].name, "starship");
    let plan = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    let starship = plan
        .documents
        .iter()
        .find(|document| document.path.ends_with("starship.toml"))
        .expect("starship document");
    let DocumentData::Text { content } = &starship.data else {
        panic!("starship document holds text data");
    };
    assert!(
        content.contains("command_timeout = 10000"),
        "template fills timeout: {content}"
    );
    assert!(
        content.contains("palette = \"catppuccin\""),
        "template fills palette: {content}"
    );
}

#[test]
fn escapes_and_unknown_keys_are_plan_errors() {
    let (dir, _) = project(
        &[("inside.toml", "x = 1\n")],
        r#"local c = confit.config("demo") return { shells = { "bash" }, configs = { c } }"#,
    );
    let root = dir.path().to_path_buf();
    for lua_case in [
        r#"local c = confit.config("demo")
           local _ = confit.resources.load_toml("../escape.toml")
           return { shells = { "bash" }, configs = { c } }"#,
        r#"local c = confit.config("demo")
           local _ = confit.resources.load_json("/abs.json")
           return { shells = { "bash" }, configs = { c } }"#,
        r#"local c = confit.config("demo")
           local _ = confit.plugin.solrachq.merge({a = 1}, {b = 2}, {bogus = true})
           return { shells = { "bash" }, configs = { c } }"#,
    ] {
        let path = root.join("profile.lua");
        std::fs::write(&path, lua_case).expect("write profile");
        let err = evaluate(&root, &path, None).expect_err("must fail");
        assert!(is_plan(&err), "plan domain: {err}");
    }
}

#[test]
fn bad_lua_types_are_lua_errors() {
    let (dir, _) = project(&[], "");
    let root = dir.path().to_path_buf();
    for lua_case in [
        r#"local c = confit.config("demo")
           c:add_document(confit.document.structured("toml", {path = "p", data = {f = function() end}}))
           return { shells = { "bash" }, configs = { c } }"#,
        r#"local c = confit.config("demo")
           c:add_document(confit.document.structured("json", {path = "p", data = {{function() end}}}))
           return { shells = { "bash" }, configs = { c } }"#,
    ] {
        let path = root.join("profile.lua");
        std::fs::write(&path, lua_case).expect("write profile");
        let err = evaluate(&root, &path, None).expect_err("must fail");
        assert!(is_lua(&err), "Lua domain: {err}");
    }
}

#[test]
fn kind_mismatch_on_one_path_stays_merge_error() {
    use confit::binding::ProfileGraph;
    use confit::model::state::config::ConfigContribution;
    use confit::model::state::document::{Document, Table};

    let table: Table = serde_json::from_value(serde_json::json!({"x": 1})).unwrap();
    let graph = ProfileGraph {
        shells: vec!["bash".into()],
        documents: Vec::new(),
        configs: vec![
            ConfigContribution {
                name: "a".into(),
                documents: vec![Document {
                    kind: DocumentKind::Structured,
                    path: "shared".into(),
                    data: DocumentData::Structured {
                        format: StructuredFormat::Toml,
                        data: table,
                    },
                    data_hash: String::new(),
                }],
                patches: Vec::new(),
            },
            ConfigContribution {
                name: "b".into(),
                documents: vec![Document {
                    kind: DocumentKind::Text,
                    path: "shared".into(),
                    data: DocumentData::Text {
                        content: "x".into(),
                    },
                    data_hash: String::new(),
                }],
                patches: Vec::new(),
            },
        ],
        merged: Vec::new(),
    };
    let err = build_plan(&graph, "root", "profile", false).expect_err("must fail");
    match err {
        Error::Merge(message) => {
            assert!(message.contains("shared"), "names path: {message}");
        }
        other => panic!("expected Error::Merge, got {other:?}"),
    }
}
