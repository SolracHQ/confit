//! Behavior tests: the `confit.config` primitive end to end.
//!
//! Exercises config construction through the lib API on tempfile project
//! roots: rc entries plus file artifacts into `ProfileGraph`, artifact
//! priority through methods and opts, repeated names, and malformed shapes
//! as named plan errors. Uses memory-free evaluation only; never touches
//! `$HOME`.
//!
//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use confit::binding::ProfileGraph;
use confit::error::Error;
use confit::model::state::artifact::ArtifactData;
use confit::model::state::condition::Condition;
use confit::services::plan::build_plan;
use confit::services::plan::evaluate_profile;

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

/// Evaluate `profile` inside a temp root.
fn evaluate(files: &[(&str, &str)], profile: &str) -> Result<ProfileGraph, Error> {
    let (dir, profile_path) = project(files, profile);
    evaluate_profile(dir.path(), &profile_path)
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
fn config_alias_env_when_and_file_land_in_graph() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_artifact(confit.artifact.rc.alias("ll", "eza -l"))
        c:add_artifact(confit.artifact.rc.alias("cat", "bat", {when = {in_path = {name = "bat"}}}))
        c:add_artifact(confit.artifact.rc.env("EDITOR", "hx", {when = confit.shell.env_set({key = "SSH_TTY"})}))
        c:add_artifact(confit.artifact.rc.profile("MY_VAR", "value"))
        c:add_artifact(confit.artifact.rc.profile_path("/bin", {when = {exists = {path = "/bin"}}}))
        c:add_artifact(confit.artifact.rc.init({eval = {"mise", "activate", "bash"}}))
        c:add_artifact(confit.artifact.rc.init({source = "~/.cargo/env"}, {when = function(s) return s.env_eq({key = "A", value = "b"}) end}))
        c:add_artifact(confit.artifact.file("web.txt", "hi"))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    assert_eq!(graph.configs.len(), 1);
    let config = &graph.configs[0];
    assert_eq!(config.name, "web");
    assert_eq!(config.aliases.len(), 2);
    assert_eq!(config.aliases[0].name, "ll");
    assert_eq!(config.aliases[0].value, "eza -l");
    assert!(config.aliases[0].when.is_none());
    assert_eq!(config.aliases[0].priority, 0);
    assert_eq!(config.aliases[1].name, "cat");
    assert!(matches!(
        config.aliases[1].when,
        Some(Condition::InPath { ref name }) if name == "bat"
    ));
    assert_eq!(config.envs.len(), 1);
    assert_eq!(config.envs[0].name, "EDITOR");
    assert_eq!(config.envs[0].value, "hx");
    assert!(matches!(
        config.envs[0].when,
        Some(Condition::EnvSet { ref key }) if key == "SSH_TTY"
    ));
    assert_eq!(config.profile.len(), 2);
    assert_eq!(config.profile[0].name, "MY_VAR");
    assert!(config.profile[0].when.is_none());
    assert_eq!(config.profile[1].name, "PATH");
    assert_eq!(config.profile[1].value, "/bin");
    assert!(matches!(
        config.profile[1].when,
        Some(Condition::Exists { ref path }) if path == "/bin"
    ));
    assert_eq!(config.inits.len(), 2);
    assert!(matches!(
        config.inits[1],
        confit::model::state::rc::InitEntry::Source { ref path, when: Some(_), .. } if path == "~/.cargo/env"
    ));
    assert_eq!(config.artifacts.len(), 1);
    assert_eq!(config.artifacts[0].path, "web.txt");
    assert!(matches!(
        config.artifacts[0].data,
        ArtifactData::File { ref content } if content == "hi"
    ));
}

#[test]
fn config_plan_renders_rc_and_file() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_artifact(confit.artifact.rc.alias("ll", "eza -l"))
        c:add_artifact(confit.artifact.rc.env("EDITOR", "hx", {when = confit.shell.env_set({key = "SSH_TTY"})}))
        c:add_artifact(confit.artifact.file("web.txt", "hi"))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    let planned = build_plan(&graph, "root", "profile").expect("plan");
    assert_eq!(planned.artifacts.len(), 2);
    let rc = planned
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let ArtifactData::Rc(data) = &rc.data else {
        panic!("rc artifact holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].name, "ll");
    assert_eq!(data.aliases[0].value, "eza -l");
    assert_eq!(data.env.len(), 1);
    assert_eq!(data.env[0].name, "EDITOR");
    assert!(matches!(data.env[0].when, Some(Condition::EnvSet { .. })));
    let file = planned
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "web.txt")
        .expect("file artifact");
    assert!(matches!(
        file.data,
        ArtifactData::File { ref content } if content == "hi"
    ));
    assert!(!file.data_hash.is_empty());
}

#[test]
fn priority_rides_artifacts_through_methods_and_opts() {
    let graph = ok(
        &[],
        r#"
        local c = confit.config("web")
        c:add_artifact(confit.artifact.rc.alias("plain", "a"))
        c:add_artifact(confit.artifact.rc.alias("method", "b"):with_priority(5))
        c:add_artifact(confit.artifact.rc.alias("opted", "c", { priority = 7 }))
        c:add_artifact(confit.artifact.rc.env("E", "1"):when(confit.shell.env_set({key = "X"})):with_priority(3))
        c:add_artifact(confit.artifact.file("web.txt", "hi"):with_priority(9))
        return { shells = { "bash" }, configs = { c } }
        "#,
    );
    let config = &graph.configs[0];
    assert_eq!(config.aliases[0].priority, 0);
    assert_eq!(config.aliases[1].priority, 5);
    assert_eq!(config.aliases[2].priority, 7);
    assert_eq!(config.envs[0].priority, 3);
    assert!(matches!(
        config.envs[0].when,
        Some(Condition::EnvSet { .. })
    ));
    assert_eq!(config.artifacts[0].priority, 9);
}

#[test]
fn entry_priority_beats_config_order_in_plan() {
    let graph = ok(
        &[],
        r#"
        local low = confit.config("aaa")
        low:add_artifact(confit.artifact.rc.alias("cat", "low"))
        local high = confit.config("zzz")
        high:add_artifact(confit.artifact.rc.alias("cat", "high"):with_priority(1))
        return { shells = { "bash" }, configs = { low, high } }
        "#,
    );
    let planned = build_plan(&graph, "root", "profile").expect("plan");
    let rc = planned
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let ArtifactData::Rc(data) = &rc.data else {
        panic!("rc artifact holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].value, "high");
    assert_eq!(data.aliases[0].priority, 1);
}

#[test]
fn priority_mistakes_are_plan_errors() {
    for (field, snippet) in [
        (
            "priority",
            r#"confit.artifact.rc.alias("a", "b", { priority = "high" })"#,
        ),
        (
            "priority",
            r#"confit.artifact.rc.alias("a", "b"):with_priority(-1)"#,
        ),
        (
            "priority",
            r#"confit.artifact.rc.alias("a", "b"):with_priority(1.5)"#,
        ),
        (
            "priority",
            r#"confit.artifact.file("f", "x"):with_priority(true)"#,
        ),
        (
            "priority",
            r#"confit.artifact.rc.alias("a", "b", { priority = 4294967296 })"#,
        ),
        ("opts", r#"confit.artifact.rc.alias("a", "b", 42)"#),
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
            "confit.artifact.rc.alias",
            "name",
            r#"confit.artifact.rc.alias(42, "bat")"#,
        ),
        (
            "confit.artifact.rc.alias",
            "value",
            r#"confit.artifact.rc.alias("cat", 42)"#,
        ),
        (
            "confit.artifact.rc.alias",
            "opts",
            r#"confit.artifact.rc.alias("cat", "bat", 42)"#,
        ),
        (
            "confit.artifact.rc.alias",
            "when",
            r#"confit.artifact.rc.alias("cat", "bat", { when = 42 })"#,
        ),
        (
            "confit.artifact.rc.env",
            "when",
            r#"confit.artifact.rc.env("A", "1", { when = { bogus = {} } })"#,
        ),
        (
            "confit.artifact.rc.env",
            "when",
            r#"confit.artifact.rc.env("A", "1", { when = { env_eq = { key = "K" } } })"#,
        ),
        (
            "confit.artifact.rc.profile",
            "when",
            r#"confit.artifact.rc.profile("P", "v", { when = { in_path = { name = 42 } } })"#,
        ),
        (
            "confit.artifact.rc.profile_path",
            "dir",
            r#"confit.artifact.rc.profile_path(42)"#,
        ),
        (
            "confit.artifact.rc.profile_path",
            "when",
            r#"confit.artifact.rc.profile_path("/bin", { when = { exists = {} } })"#,
        ),
        (
            "confit.artifact.rc.init",
            "spec",
            r#"confit.artifact.rc.init({ eval = { "a" }, cmd = { "b" } })"#,
        ),
        (
            "confit.artifact.rc.init",
            "spec",
            r#"confit.artifact.rc.init(42)"#,
        ),
        (
            "confit.artifact.rc.init",
            "spec.eval",
            r#"confit.artifact.rc.init({ eval = { "a", 42 } })"#,
        ),
        (
            "confit.artifact.rc.init",
            "when",
            r#"confit.artifact.rc.init({ cmd = { "a" } }):when(42)"#,
        ),
        (
            "confit.artifact.rc.init",
            "when",
            r#"confit.artifact.rc.init({ cmd = { "a" } }, { when = function(s) return 42 end })"#,
        ),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nc:add_artifact({snippet})\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
        );
        let message = plan_err(&[], &profile);
        assert!(message.contains(ctor), "names {ctor}: {message}");
        assert!(message.contains(field), "names {field}: {message}");
    }
    let profile = "local c = confit.config(\"web\")\nc:add_artifact({ area = \"alias\", name = \"cat\", value = \"bat\" })\nreturn { shells = { \"bash\" }, configs = { c } }".to_string();
    let message = plan_err(&[], &profile);
    assert!(message.contains("web"), "names config: {message}");
    assert!(
        message.contains("add_artifact"),
        "names add_artifact: {message}"
    );
}

#[test]
fn artifact_mistakes_are_plan_errors() {
    for (field, artifact) in [
        ("add_artifact", r#""just a string""#),
        ("add_artifact", r#"42"#),
        ("add_artifact", r#"confit.config("impostor")"#),
    ] {
        let profile = format!(
            "local c = confit.config(\"web\")\nc:add_artifact({artifact})\nreturn {{ shells = {{ \"bash\" }}, configs = {{ c }} }}"
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
        base:add_artifact(confit.artifact.rc.alias("ll", "eza -l"))
        local c = confit.config("web")
        c:add_artifact(confit.artifact.rc.env("EDITOR", "hx"))
        return { shells = { "bash" }, configs = { base, c } }
        "#,
    );
    assert_eq!(graph.configs.len(), 2);
    let planned = build_plan(&graph, "root", "profile").expect("plan");
    let rc = planned
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let ArtifactData::Rc(data) = &rc.data else {
        panic!("rc artifact holds rc data");
    };
    assert_eq!(data.aliases.len(), 1);
    assert_eq!(data.aliases[0].name, "ll");
    assert_eq!(data.env.len(), 1);
    assert_eq!(data.env[0].name, "EDITOR");
}

#[test]
fn missing_configs_key_is_config_error() {
    match evaluate(
        &[],
        r#"
        local c = confit.config("base")
        c:add_artifact(confit.artifact.rc.alias("ll", "eza -l"))
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
