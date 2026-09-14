//! Behavior tests: the `confit.config` primitive end to end.
//!
//! Exercises config construction through the lib API on tempfile project
//! roots: rc entries plus text documents into `ProfileGraph`, patches
//! through `add_patch` with owner stamping and priority levels, and
//! malformed shapes as named plan errors. Uses memory-free evaluation
//! only; never touches `$HOME`.
//!
//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use confit::binding::ProfileGraph;
use confit::error::Error;
use confit::model::state::condition::Condition;
use confit::model::state::document::DocumentData;
use confit::model::state::level::Level;
use confit::services::plan::build_plan;

mod common;

/// Evaluate `profile` inside a temp root.
fn evaluate(files: &[(&str, &str)], profile: &str) -> Result<ProfileGraph, Error> {
    let (dir, profile_path) = common::project(files, profile);
    confit::binding::evaluate(dir.path(), &profile_path, None)
}

/// Evaluate expecting success.
fn ok(files: &[(&str, &str)], profile: &str) -> ProfileGraph {
    evaluate(files, profile).expect("evaluate")
}

/// Evaluate expecting a plan error; returns its message.
fn plan_err(files: &[(&str, &str)], profile: &str) -> String {
    match evaluate(files, profile) {
        Err(Error::Plan(message)) => message,
        Err(other) => panic!("expected plan error, got {other:?}"),
        Ok(_) => panic!("expected plan error, got success"),
    }
}

#[test]
fn config_alias_env_when_and_text_land_in_graph() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_document(confit.document.rc.alias("ll", "eza -l"))
        c:add_document(confit.document.rc.alias("cat", "bat", {when = {in_path = {name = "bat"}}}))
        c:add_document(confit.document.rc.env("EDITOR", "hx", {when = confit.shell.env_set({key = "SSH_TTY"})}))
        c:add_document(confit.document.rc.profile("MY_VAR", "value"))
        c:add_document(confit.document.rc.profile_path("/bin", {when = {exists = {path = "/bin"}}}))
        c:add_document(confit.document.rc.path_entry("/sbin"))
        c:add_document(confit.document.rc.eval({"mise", "activate", "bash"}))
        c:add_document(confit.document.rc.cmd({"task", "--completion"}, {lane = "last"}))
        c:add_document(confit.document.rc.source("~/.cargo/env", {when = function(s) return s.env_eq({key = "A", value = "b"}) end}))
        c:add_document(confit.document.text("web.txt", "hi"))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.name, "web");
    assert!(config.patches.is_empty());
    assert_eq!(config.documents.len(), 2);
    let rc = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc data");
    };
    assert_eq!(data.aliases.len(), 2);
    assert_eq!(data.env.len(), 1);
    assert_eq!(data.profile.len(), 3);
    assert_eq!(data.init.len(), 3);
    assert!(config.documents.iter().any(|item| item.path == "web.txt"));
}

#[test]
fn config_plan_renders_rc_and_text() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_document(confit.document.rc.alias("ll", "eza -l"))
        c:add_document(confit.document.rc.env("EDITOR", "hx", {when = confit.shell.env_set({key = "SSH_TTY"})}))
        c:add_document(confit.document.text("web.txt", "hi"))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    let planned = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    assert_eq!(planned.documents.len(), 2);
    let rc = planned
        .documents
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc document");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc document holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].spec.name, "ll");
    assert_eq!(data.aliases[0].spec.value, "eza -l");
    assert_eq!(data.env.len(), 1);
    assert_eq!(data.env[0].spec.name, "EDITOR");
    assert!(matches!(data.env[0].when, Some(Condition::EnvSet { .. })));
    let file = planned
        .documents
        .iter()
        .find(|document| document.path == "web.txt")
        .expect("text document");
    assert!(matches!(
        file.data,
        DocumentData::Text { ref content } if content == "hi"
    ));
    assert!(!file.data_hash.is_empty());
}

#[test]
fn entries_default_to_normal_priority() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_document(confit.document.rc.alias("plain", "a"))
        c:add_document(confit.document.rc.alias("guarded", "b", {when = confit.shell.env_set({key = "X"})}))
        c:add_document(confit.document.rc.env("E", "1", {lane = "first"}))
        c:add_document(confit.document.text("web.txt", "hi"))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    let config = &graph.configs[0];
    assert!(config.patches.is_empty());
    assert_eq!(config.documents.len(), 2);
    let rc = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc data");
    };
    assert_eq!(data.aliases.len(), 2);
    assert_eq!(data.env.len(), 1);
}

#[test]
fn tied_entries_resolve_by_config_name_in_plan() {
    let graph = ok(
        &[],
        r#"
        local low = confit.config("aaa")
        low:add_document(confit.document.rc.alias("cat", "low"))
        local high = confit.config("zzz")
        high:add_document(confit.document.rc.alias("cat", "high"))
        return { shells = { "bash" }, configs = { low, high } }
        "#,
    );
    let planned = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    let rc = planned
        .documents
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc document");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc document holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].spec.value, "low");
    assert_eq!(data.aliases[0].priority, Level::Normal);
}

#[test]
fn patch_handles_land_with_owner_and_priority() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        local p = confit.patch.structured("json", "user.json", function(data)
          data:set("user.theme", "catppuccin")
          data:append("user.extensions", "com.microsoft.extensions.python")
        end)
        p:priority(confit.priority.HIGH)
        c:add_patch(p)
        local q = confit.patch.rc(function(document)
          document:append("config", confit.document.rc.alias("cat", "bat"))
        end)
        c:add_patch(q)
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    let config = &graph.configs[0];
    assert_eq!(config.patches.len(), 2);
    let structured = &config.patches[0];
    assert_eq!(structured.document, "user.json");
    assert_eq!(structured.owner, "web");
    assert_eq!(structured.priority, Level::High);
    let rc = &config.patches[1];
    assert_eq!(rc.document, "rc");
    assert_eq!(rc.owner, "web");
    assert_eq!(rc.priority, Level::Normal);
    let merged = graph
        .merged
        .iter()
        .find(|item| item.path == "user.json")
        .expect("user json");
    let DocumentData::Structured { data, .. } = &merged.data else {
        panic!("structured");
    };
    assert_eq!(
        data.get("user")
            .and_then(|item| item.get("theme"))
            .and_then(|item| item.as_str()),
        Some("catppuccin")
    );
}

#[test]
fn patch_priority_levels_cover_every_level() {
    for (name, level) in [
        ("MINOR", Level::Minor),
        ("LOW", Level::Low),
        ("NORMAL", Level::Normal),
        ("HIGH", Level::High),
        ("MAJOR", Level::Major),
    ] {
        let graph = ok(
            &[],
            &format!(
                "local c = confit.config(\"web\")\nlocal p = confit.patch.rc(function(_) end)\np:priority(confit.priority.{name})\nc:add_patch(p)\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
            ),
        );
        assert_eq!(graph.configs[0].patches[0].priority, level, "{name}");
    }
}

#[test]
fn rc_new_empty_is_valid_and_sections_land() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_document(confit.document.rc.alias("ll", "eza -l"))
        return {
          shells = { "bash" },
          documents = { confit.document.rc.new({}) },
          configs = { c },
        }
        "#,
    );
    assert_eq!(graph.documents.len(), 1);
    assert_eq!(graph.documents[0].path, "rc");

    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_document(confit.document.rc.alias("ll", "eza -l"))
        c:add_document(confit.document.rc.new({
          profile = { confit.document.rc.path_entry("/bin") },
          config = { confit.document.rc.alias("cat", "bat") },
          final = { confit.document.rc.eval({"starship", "init"}) },
        }))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    assert_eq!(graph.configs[0].documents.len(), 2);
    let rc = graph
        .merged
        .iter()
        .find(|item| item.path == "rc")
        .expect("rc");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc data");
    };
    assert_eq!(data.profile.len(), 1);
    assert_eq!(data.aliases.len(), 2);
    assert_eq!(data.init.len(), 1);
}

#[test]
fn integer_priority_is_an_unknown_key_error() {
    for (field, snippet) in [
        (
            "priority",
            r#"confit.document.rc.alias("a", "b", { priority = 5 })"#,
        ),
        (
            "priority",
            r#"confit.document.rc.env("a", "b", { priority = "high" })"#,
        ),
        ("opts", r#"confit.document.rc.alias("a", "b", 42)"#),
        ("name", r#"confit.config(42)"#),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nlocal _ = {snippet}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
        );
        let message = plan_err(&[], &profile);
        assert!(message.contains(field), "names {field}: {message}");
    }
}

#[test]
fn rc_shape_mistakes_are_plan_errors() {
    for (ctor, field, snippet) in [
        (
            "confit.document.rc.alias",
            "name",
            r#"confit.document.rc.alias(42, "bat")"#,
        ),
        (
            "confit.document.rc.alias",
            "value",
            r#"confit.document.rc.alias("cat", 42)"#,
        ),
        (
            "confit.document.rc.alias",
            "opts",
            r#"confit.document.rc.alias("cat", "bat", 42)"#,
        ),
        (
            "confit.document.rc.alias",
            "when",
            r#"confit.document.rc.alias("cat", "bat", { when = 42 })"#,
        ),
        (
            "confit.document.rc.env",
            "when",
            r#"confit.document.rc.env("A", "1", { when = { bogus = {} } })"#,
        ),
        (
            "confit.document.rc.env",
            "when",
            r#"confit.document.rc.env("A", "1", { when = { env_eq = { key = "K" } } })"#,
        ),
        (
            "confit.document.rc.profile",
            "when",
            r#"confit.document.rc.profile("P", "v", { when = { in_path = { name = 42 } } })"#,
        ),
        (
            "confit.document.rc.profile_path",
            "dir",
            r#"confit.document.rc.profile_path(42)"#,
        ),
        (
            "confit.document.rc.profile_path",
            "when",
            r#"confit.document.rc.profile_path("/bin", { when = { exists = {} } })"#,
        ),
        (
            "confit.document.rc.path_entry",
            "dir",
            r#"confit.document.rc.path_entry(42)"#,
        ),
        (
            "confit.document.rc.eval",
            "argv",
            r#"confit.document.rc.eval("nope")"#,
        ),
        (
            "confit.document.rc.eval",
            "argv",
            r#"confit.document.rc.eval({ "a", 42 })"#,
        ),
        (
            "confit.document.rc.cmd",
            "argv",
            r#"confit.document.rc.cmd({ { "b" } })"#,
        ),
        (
            "confit.document.rc.source",
            "path",
            r#"confit.document.rc.source(42)"#,
        ),
        (
            "confit.document.rc.source",
            "when",
            r#"confit.document.rc.source("x", { when = function(s) return 42 end })"#,
        ),
        (
            "confit.document.rc.eval",
            "lane",
            r#"confit.document.rc.eval({ "a" }, { lane = "sideways" })"#,
        ),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nc:add_document({snippet})\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
        );
        let message = plan_err(&[], &profile);
        assert!(message.contains(ctor), "names {ctor}: {message}");
        assert!(message.contains(field), "names {field}: {message}");
    }
    let profile = "local c = confit.config(\"web\")\nc:add_document(confit.document.rc.new({ bogus = {} }))\nreturn { shells = { \"bash\" }, configs = { c } }".to_string();
    let message = plan_err(&[], &profile);
    assert!(
        message.contains("add_document"),
        "names add_document: {message}"
    );
    assert!(message.contains("bogus"), "names section: {message}");
    let profile = "local c = confit.config(\"web\")\nc:add_document({ area = \"alias\", name = \"cat\", value = \"bat\" })\nreturn { shells = { \"bash\" }, configs = { c } }".to_string();
    let message = plan_err(&[], &profile);
    assert!(message.contains("web"), "names config: {message}");
    assert!(
        message.contains("add_document"),
        "names add_document: {message}"
    );
}

#[test]
fn unknown_kind_tables_fail_add_document() {
    let profile = "local c = confit.config(\"web\")\nlocal t = { path = \"x\" }\nsetmetatable(t, { __kind = \"bogus\" })\nc:add_document(t)\nreturn { shells = { \"bash\" }, configs = { c } }";
    let message = plan_err(&[], profile);
    assert!(
        message.contains("add_document"),
        "names add_document: {message}"
    );
    assert!(message.contains("bogus"), "names kind: {message}");
}

#[test]
fn entry_without_area_fails_add_document() {
    let profile = "local c = confit.config(\"web\")\nlocal e = confit.document.rc.alias(\"a\", \"b\")\nsetmetatable(e, { __kind = \"rc-entry\" })\nc:add_document(e)\nreturn { shells = { \"bash\" }, configs = { c } }";
    let message = plan_err(&[], profile);
    assert!(
        message.contains("add_document"),
        "names add_document: {message}"
    );
    assert!(message.contains("__area"), "names area: {message}");
}

#[test]
fn when_method_is_gone() {
    let files: &[(&str, &str)] = &[];
    let profile = "local c = confit.config(\"web\")\nlocal e = confit.document.rc.alias(\"cat\", \"bat\")\ne:when({ in_path = { name = \"bat\" } })\nreturn { shells = { \"bash\" }, configs = { c } }";
    match evaluate(files, profile) {
        Err(Error::Lua(message)) => {
            assert!(
                message.contains("when") || message.contains("attempt"),
                "names failure: {message}"
            );
        }
        Err(other) => panic!("expected Lua error, got {other:?}"),
        Ok(_) => panic!("expected Lua error, got success"),
    }
}

#[test]
fn patch_mistakes_are_plan_errors() {
    for (field, snippet) in [
        (
            "priority",
            r#"confit.patch.rc(function(_) end):priority("extreme")"#,
        ),
        (
            "priority",
            r#"confit.patch.rc(function(_) end):priority(42)"#,
        ),
        (
            "section",
            r#"confit.patch.rc(function(document)
                 document:append("aliases", confit.document.rc.alias("cat", "bat"))
               end)"#,
        ),
        (
            "section",
            r#"confit.patch.rc(function(document)
                 document:set("bogus", confit.document.rc.alias("cat", "bat"))
               end)"#,
        ),
        (
            "path",
            r#"confit.patch.structured("json", "x.json", function(data)
                 data:set(42, "x")
               end)"#,
        ),
        (
            "path",
            r#"confit.patch.structured("json", 42, function(_) end)"#,
        ),
        (
            "value",
            r#"confit.patch.structured("json", "x.json", function(data)
                 data:set("a", function() end)
               end)"#,
        ),
        (
            "value",
            r#"confit.patch.rc(function(document)
                 document:set("config", "nope")
               end)"#,
        ),
        (
            "format",
            r#"confit.patch.structured("ini", "x.ini", function(_) end)"#,
        ),
        (
            "format",
            r#"confit.document.structured("ini", {path = "a", data = {}})"#,
        ),
        ("callback", r#"confit.patch.rc(42)"#),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nlocal _ = {snippet}\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
        );
        // Constructor mistakes fail at construction, wrapper mistakes fail at live execution.
        // Both surface as plan errors from evaluate, except wrapper mistakes needing add_patch.
        let with_patch = if snippet.contains("document:") || snippet.contains("data:") {
            format!(
                "local c = confit.config(\"web\")\nlocal p = {snippet}\nc:add_patch(p)\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
            )
        } else {
            profile
        };
        let message = plan_err(&[], &with_patch);
        assert!(message.contains(field), "names {field}: {message}");
    }
}

#[test]
fn document_mistakes_are_plan_errors() {
    for (field, document) in [
        ("add_document", r#""just a string""#),
        ("add_document", r#"42"#),
        ("add_document", r#"confit.config("impostor")"#),
        ("add_document", r#"confit.patch.rc(function(_) end)"#),
        ("add_patch", r#""just a string""#),
        ("add_patch", r#"confit.document.rc.alias("a", "b")"#),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nc:{field}({document})\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
        );
        let message = plan_err(&[], &profile);
        assert!(message.contains("web"), "names config: {message}");
        assert!(message.contains(field), "names {field}: {message}");
    }
}

#[test]
fn repeated_names_are_plan_errors() {
    let message = plan_err(
        &[],
        r#"
        local a = confit.config("dup")
        local b = confit.config("dup")
        return { shells = { "bash" }, configs = { a, b } }
        "#,
    );
    assert!(message.contains("dup"), "names config: {message}");
}

#[test]
fn two_configs_fold_together() {
    let graph = ok(
        &[],
        r#"
        local base = confit.config("base")
        base:add_document(confit.document.rc.alias("ll", "eza -l"))
        local c = confit.config("web")
        c:add_document(confit.document.rc.env("EDITOR", "hx"))
        return { shells = { "bash" }, configs = { base, c } }
        "#,
    );
    assert_eq!(graph.configs.len(), 2);
    let planned = build_plan(&graph, "root", "profile", false)
        .expect("plan")
        .0;
    let rc = planned
        .documents
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc document");
    let DocumentData::Rc(data) = &rc.data else {
        panic!("rc document holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].spec.name, "ll");
    assert_eq!(data.env.len(), 1);
    assert_eq!(data.env[0].spec.name, "EDITOR");
}

#[test]
fn missing_configs_key_is_config_error() {
    match evaluate(
        &[],
        r#"
        local c = confit.config("base")
        c:add_document(confit.document.rc.alias("ll", "eza -l"))
        return { shells = { "bash" } }
        "#,
    ) {
        Err(Error::Config(message)) => {
            assert!(message.contains("configs"), "names configs: {message}");
        }
        Err(other) => panic!("expected config error, got {other:?}"),
        Ok(_) => panic!("expected config error, got success"),
    }
}
