//! Pipeline fold behavior: canonical order plus patch ops plus warnings.
//!
//! Exercises the live pipeline through the lib API on tempfile roots:
//! shuffled configs yield identical documents, local op order is preserved,
//! structured errors name the path, and text conflicts warn or fail under
//! strict mode. Uses memory fakes only and never touches `$HOME`.
//!
//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use confit::binding::ProfileGraph;
use confit::binding::evaluate as binding_evaluate;
use confit::error::Error;
use confit::model::dto::warning::WarningKind;
use confit::model::state::document::DocumentData;
use confit::services::plan::build_plan;

/// Write `profile` into a temp root and evaluate it.
fn evaluate(profile: &str) -> ProfileGraph {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(&profile_path, profile).expect("write profile");
    let graph = binding_evaluate(dir.path(), &profile_path, None).expect("evaluate");
    // Keep the temp dir alive through evaluation only; the graph owns its data.
    std::mem::forget(dir);
    graph
}

/// Plan with strict mode off and return documents plus warnings.
fn plan(
    graph: &ProfileGraph,
) -> (
    Vec<confit::model::state::document::Document>,
    Vec<confit::model::dto::warning::PlanWarning>,
) {
    let root = PathBuf::from("root");
    let _ = root;
    let built = build_plan(graph, "root", "profile", false).expect("plan");
    (built.0.documents, built.1)
}

#[test]
fn shuffled_configs_produce_identical_documents() {
    let base = r#"
        local aaa = confit.config("aaa")
        aaa:add_patch(confit.patch.structured("json", "app.json", function(d)
            d:set("x", 1)
        end):priority(confit.priority.HIGH))
        local zzz = confit.config("zzz")
        zzz:add_patch(confit.patch.structured("json", "app.json", function(d)
            d:set("x", 2)
        end):priority(confit.priority.LOW))
    "#;
    let forward = format!(
        "{base}\nreturn {{ shells = {{ \"bash\" }}, documents = {{ confit.document.structured(\"json\", {{ path = \"app.json\", data = {{}} }}) }}, configs = {{ aaa, zzz }} }}"
    );
    let backward = format!(
        "{base}\nreturn {{ shells = {{ \"bash\" }}, documents = {{ confit.document.structured(\"json\", {{ path = \"app.json\", data = {{}} }}) }}, configs = {{ zzz, aaa }} }}"
    );
    let first = evaluate(&forward);
    let second = evaluate(&backward);
    let (first_docs, _) = plan(&first);
    let (second_docs, _) = plan(&second);
    let first_json = first_docs
        .iter()
        .find(|document| document.path == "app.json")
        .expect("json document");
    let second_json = second_docs
        .iter()
        .find(|document| document.path == "app.json")
        .expect("json document");
    assert_eq!(first_json.data, second_json.data);
    let DocumentData::Structured { data, .. } = &first_json.data else {
        panic!("structured data");
    };
    assert_eq!(data.get("x").and_then(serde_json::Value::as_i64), Some(1));
}

#[test]
fn local_op_order_is_preserved() {
    let graph = evaluate(
        r#"
        local c = confit.config("demo")
        c:add_patch(confit.patch.structured("json", "app.json", function(d)
            d:append("items", 1)
            d:append("items", 2)
            d:append("items", 3)
        end))
        return {
            shells = { "bash" },
            documents = { confit.document.structured("json", { path = "app.json", data = {} }) },
            configs = { c },
        }
        "#,
    );
    let (docs, _) = plan(&graph);
    let document = docs
        .iter()
        .find(|document| document.path == "app.json")
        .expect("json document");
    let DocumentData::Structured { data, .. } = &document.data else {
        panic!("structured data");
    };
    assert_eq!(data.get("items"), Some(&serde_json::json!([1, 2, 3])));
}

#[test]
fn set_over_foreign_keeps_first_writer() {
    let graph = evaluate(
        r#"
        local aaa = confit.config("aaa")
        aaa:add_patch(confit.patch.structured("json", "app.json", function(d)
          d:set("a.b", "base")
        end))
        local zzz = confit.config("zzz")
        zzz:add_patch(confit.patch.structured("json", "app.json", function(d)
          d:set("a.b", "challenger")
        end))
        return {
          shells = { "bash" },
          documents = { confit.document.structured("json", { path = "app.json", data = {} }) },
          configs = { aaa, zzz },
        }
        "#,
    );
    let (docs, _) = plan(&graph);
    let document = docs
        .iter()
        .find(|item| item.path == "app.json")
        .expect("json");
    let DocumentData::Structured { data, .. } = &document.data else {
        panic!("structured");
    };
    assert_eq!(
        data.get("a")
            .and_then(|item| item.get("b"))
            .and_then(|item| item.as_str()),
        Some("base")
    );
}

#[test]
fn append_on_nonlist_errors_naming_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(
        &profile_path,
        r#"
        local c = confit.config("tool")
        c:add_patch(confit.patch.structured("json", "app.json", function(d)
          d:set("user.theme", "catppuccin")
          d:append("user.theme", "x")
        end))
        return {
          shells = { "bash" },
          documents = { confit.document.structured("json", { path = "app.json", data = {} }) },
          configs = { c },
        }
        "#,
    )
    .expect("write");
    let err = binding_evaluate(dir.path(), &profile_path, None).expect_err("must fail");
    match err {
        Error::Plan(message) => {
            assert!(message.contains("user.theme"), "names path: {message}");
        }
        other => panic!("expected plan error, got {other:?}"),
    }
}

#[test]
fn missing_intermediate_errors_naming_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(
        &profile_path,
        r#"
        local c = confit.config("tool")
        c:add_patch(confit.patch.structured("json", "app.json", function(d)
          d:set("a", 1)
          d:set("a.b", 2)
        end))
        return {
          shells = { "bash" },
          documents = { confit.document.structured("json", { path = "app.json", data = {} }) },
          configs = { c },
        }
        "#,
    )
    .expect("write");
    let err = binding_evaluate(dir.path(), &profile_path, None).expect_err("must fail");
    match err {
        Error::Plan(message) => {
            assert!(message.contains("a.b"), "names path: {message}");
        }
        other => panic!("expected plan error, got {other:?}"),
    }
}

#[test]
fn indexed_set_writes_single_element() {
    let graph = evaluate(
        r#"
        local c = confit.config("tool")
        c:add_patch(confit.patch.structured("json", "app.json", function(d)
          d:append("items", 1)
          d:set("items[0]", 9)
        end))
        return {
          shells = { "bash" },
          documents = { confit.document.structured("json", { path = "app.json", data = {} }) },
          configs = { c },
        }
        "#,
    );
    let (docs, _) = plan(&graph);
    let document = docs
        .iter()
        .find(|item| item.path == "app.json")
        .expect("json");
    let DocumentData::Structured { data, .. } = &document.data else {
        panic!("structured");
    };
    assert_eq!(data.get("items"), Some(&serde_json::json!([9])));
}

#[test]
fn text_conflict_warns_naming_both_owners() {
    let graph = evaluate(
        r#"
        local a = confit.config("aaa")
        a:add_document(confit.document.text("note.txt", "one"))
        local z = confit.config("zzz")
        z:add_document(confit.document.text("note.txt", "two"))
        return { shells = { "bash" }, configs = { a, z } }
        "#,
    );
    let built = build_plan(&graph, "root", "profile", false).expect("plan");
    assert_eq!(built.1.len(), 1);
    let warning = &built.1[0];
    assert_eq!(warning.path, "note.txt");
    match &warning.kind {
        WarningKind::DeclarationConflict { owners } => {
            assert!(owners.contains(&"aaa".to_string()), "names aaa: {owners:?}");
            assert!(owners.contains(&"zzz".to_string()), "names zzz: {owners:?}");
        }
        other => panic!("expected declaration conflict, got {other:?}"),
    }
    let line = confit::presentation::warning_line(warning);
    assert!(line.contains("note.txt"), "names path: {line}");
    assert!(line.contains("aaa"), "names aaa: {line}");
    assert!(line.contains("zzz"), "names zzz: {line}");
}

#[test]
fn strict_flag_escalates_text_conflict_to_error() {
    let graph = evaluate(
        r#"
        local a = confit.config("aaa")
        a:add_document(confit.document.text("note.txt", "one"))
        local z = confit.config("zzz")
        z:add_document(confit.document.text("note.txt", "two"))
        return { shells = { "bash" }, configs = { a, z } }
        "#,
    );
    let err = build_plan(&graph, "root", "profile", true).expect_err("strict must fail");
    match err {
        Error::Plan(message) => {
            assert!(message.contains("note.txt"), "names path: {message}");
            assert!(message.contains("aaa"), "names aaa: {message}");
            assert!(message.contains("zzz"), "names zzz: {message}");
        }
        other => panic!("expected plan error, got {other:?}"),
    }
}

#[test]
fn identical_text_declarations_stay_silent() {
    let graph = evaluate(
        r#"
        local a = confit.config("aaa")
        a:add_document(confit.document.text("note.txt", "same"))
        local z = confit.config("zzz")
        z:add_document(confit.document.text("note.txt", "same"))
        return { shells = { "bash" }, configs = { a, z } }
        "#,
    );
    let built = build_plan(&graph, "root", "profile", false).expect("plan");
    assert!(built.1.is_empty());
    let built_strict = build_plan(&graph, "root", "profile", true).expect("strict silent");
    assert!(built_strict.1.is_empty());
}

#[test]
fn rc_patch_canonical_order_ignores_config_order() {
    let patch = r#"
        local aaa = confit.config("aaa")
        aaa:add_patch(confit.patch.rc(function(d)
            d:append("config", confit.document.rc.alias("cat", "bat"))
        end):priority(confit.priority.HIGH))
        local zzz = confit.config("zzz")
        zzz:add_patch(confit.patch.rc(function(d)
            d:append("config", confit.document.rc.alias("cat", "eza"))
        end):priority(confit.priority.LOW))
    "#;
    let forward =
        format!("{patch}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ aaa, zzz }} }}");
    let backward =
        format!("{patch}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ zzz, aaa }} }}");
    let first = plan(&evaluate(&forward)).0;
    let second = plan(&evaluate(&backward)).0;
    let first_rc = first
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc");
    let second_rc = second
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc");
    assert_eq!(first_rc.data, second_rc.data);
    let DocumentData::Rc(data) = &first_rc.data else {
        panic!("rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].spec.value, "bat");
}

#[test]
fn structured_format_mismatch_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(
        &profile_path,
        r#"
        local c = confit.config("demo")
        c:add_patch(confit.patch.structured("json", "app.toml", function(d)
            d:set("x", 1)
        end))
        return {
            shells = { "bash" },
            documents = { confit.document.structured("toml", { path = "app.toml", data = {} }) },
            configs = { c },
        }
        "#,
    )
    .expect("write");
    let err = binding_evaluate(dir.path(), &profile_path, None).expect_err("must fail");
    match err {
        Error::Merge(message) | Error::Plan(message) => {
            assert!(message.contains("app.toml"), "names path: {message}");
        }
        other => panic!("expected merge or plan error, got {other:?}"),
    }
}
