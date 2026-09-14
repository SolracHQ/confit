//! Basic-tool behavior test: real fixture through the lib API.
//!
//! Golden workflow: this test normalizes `created_at` to a fixed string,
//! renders via the in-memory writer, and compares byte-for-byte against
//! `tests/golden/plan_basic_tool.json`. To regenerate after an intentional
//! plan-shape change, print the rendered text from the golden test, eyeball
//! it against the fixture (`bat` alias, mise `bat = "latest"`, 2 documents,
//! `mise install` hook), then overwrite the golden file verbatim (no
//! trailing newline: `serde_json::to_string_pretty` emits none).
//!
//! Uses memory store/writer fakes only; the fixture is read from the repo
//! via `CARGO_MANIFEST_DIR`. Never touches `$HOME`.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit::binding::evaluate;
use confit::model::state::State;
use confit::model::state::StateEntry;
use confit::model::state::document::DocumentData;
use confit::model::state::document::StructuredFormat;
use confit::model::state::plan::PLAN_VERSION;
use confit::model::state::plan::Plan;
use confit::repository::MemoryFilesystem;
use confit::services::plan::{diff, load_state, summarize, write_plan};

/// Stable plan labels so the golden file is portable across checkouts.
const GOLDEN_ROOT: &str = "examples/0-basic_tool";
const GOLDEN_PROFILE: &str = "examples/0-basic_tool/profile.lua";
/// Normalized timestamp for golden comparison (plans always carry fresh time).
const GOLDEN_CREATED_AT: &str = "2026-09-09T00:00:00Z";

/// Fixture root and profile path inside the repo.
fn fixture_paths() -> (PathBuf, PathBuf) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("examples/0-basic_tool");
    let profile = root.join("profile.lua");
    (root, profile)
}

/// Evaluate the real fixture and plan with stable labels.
fn build_plan() -> Plan {
    let (root, profile) = fixture_paths();
    let graph = evaluate(&root, &profile, None).expect("fixture evaluates");
    confit::services::plan::build_plan(&graph, GOLDEN_ROOT, GOLDEN_PROFILE, false)
        .expect("plan")
        .0
}

/// Render `plan` through the memory filesystem (exercises the filesystem seam).
fn render_json(plan: &Plan) -> String {
    let text = serde_json::to_string_pretty(plan).expect("serialize");
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    write_plan(&fs, text.as_bytes(), Path::new("plan.json")).expect("memory write");
    let bytes = fs.files.borrow()["plan.json"].clone();
    String::from_utf8(bytes).expect("utf8")
}

#[test]
fn basic_tool_produces_rc_and_mise() {
    let plan = build_plan();
    assert_eq!(plan.version, PLAN_VERSION);
    assert_eq!(plan.documents.len(), 2);

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
            .find(|entry| entry.spec.name == "cat")
            .map(|entry| entry.spec.value.as_str()),
        Some("bat")
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
    let tools = table.get("tools").expect("tools table");
    assert_eq!(
        tools.get("bat").and_then(serde_json::Value::as_str),
        Some("latest")
    );

    let summary = diff(&plan, &State::empty());
    assert_eq!(summary.create, 2);
    assert_eq!(summary.update, 0);
}

#[test]
fn golden_json_is_stable() {
    let mut plan = build_plan();
    plan.created_at = GOLDEN_CREATED_AT.to_string();
    let rendered = render_json(&plan);
    let golden_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/plan_basic_tool.json");
    let golden = std::fs::read_to_string(&golden_path).expect("read golden file");
    assert_eq!(rendered, golden);
}

#[test]
fn hashes_are_stable_across_runs() {
    let first = build_plan();
    let second = build_plan();
    assert_eq!(first.documents.len(), second.documents.len());
    for (left, right) in first.documents.iter().zip(second.documents.iter()) {
        assert_eq!(left.path, right.path);
        assert!(!left.data_hash.is_empty());
        assert_eq!(left.data_hash, right.data_hash);
    }
}

#[test]
fn diff_counts_stable_when_state_matches() {
    let plan = build_plan();
    let rc = plan
        .documents
        .iter()
        .find(|document| document.path == "~/.bashrc")
        .expect("rc document");
    let mut documents = BTreeMap::new();
    documents.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: rc.data_hash.clone(),
            data: None,
        },
    );
    let seed = State { documents };
    let bytes = serde_json::to_vec(&seed).expect("state json");
    let mut files = BTreeMap::new();
    files.insert("state.json".to_string(), bytes);
    let fs = MemoryFilesystem {
        files: RefCell::new(files),
        failures: RefCell::new(BTreeMap::new()),
    };
    let previous = load_state(&fs, Some(Path::new("state.json"))).expect("memory load");
    let summary = diff(&plan, &previous);
    assert_eq!(summary.create, 1);
    assert_eq!(summary.update, 0);
}

#[test]
fn golden_summary_is_stable() {
    let plan = build_plan();
    let previous = State::empty();
    let counts = diff(&plan, &previous);
    let shaped = summarize(&counts);
    let details = confit::services::diff::detail(&plan, &previous);
    let rendered = confit::presentation::render_plan(&plan, &shaped, &details);
    let golden_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/plan_basic_tool.summary");
    let golden = std::fs::read_to_string(&golden_path).expect("read golden summary");
    assert_eq!(rendered, golden);
}

/// Seeded previous: matching mise collapses, altered alias changes, extra alias removes.
fn seeded_previous(plan: &Plan) -> State {
    let mise = plan
        .documents
        .iter()
        .find(|document| document.path == "~/.config/mise/config.toml")
        .expect("mise document");
    let mut documents = BTreeMap::new();
    documents.insert(
        "structured:~/.config/mise/config.toml".to_string(),
        StateEntry {
            data_hash: mise.data_hash.clone(),
            data: Some(serde_json::to_value(&mise.data).expect("mise snapshot")),
        },
    );
    documents.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: "previous".to_string(),
            data: Some(serde_json::json!({
                "rc": {
                    "profile": [],
                    "env": [],
                    "aliases": [
                        {"name": "cat", "value": "oldbat", "when": {"in_path": {"name": "bat"}}},
                        {"name": "oldkey", "value": "oldval", "when": null}
                    ],
                    "init": []
                }
            })),
        },
    );
    State { documents }
}

#[test]
fn seeded_state_renders_change_and_remove() {
    let plan = build_plan();
    let seed = seeded_previous(&plan);
    let bytes = serde_json::to_vec(&seed).expect("state json");
    let mut files = BTreeMap::new();
    files.insert("state.json".to_string(), bytes);
    let fs = MemoryFilesystem {
        files: RefCell::new(files),
        failures: RefCell::new(BTreeMap::new()),
    };
    let previous = load_state(&fs, Some(Path::new("state.json"))).expect("memory load");
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update), (0, 1));
    let details = confit::services::diff::detail(&plan, &previous);
    let mise = details
        .iter()
        .find(|detail| detail.key == "structured:~/.config/mise/config.toml")
        .expect("mise detail");
    assert_eq!(
        mise.status,
        confit::model::dto::diff::DocumentStatus::Unchanged
    );
    let rc = details
        .iter()
        .find(|detail| detail.key == "rc:~/.bashrc")
        .expect("rc detail");
    assert_eq!(rc.status, confit::model::dto::diff::DocumentStatus::Update);
    let shaped = summarize(&counts);
    let text = confit::presentation::render_plan(&plan, &shaped, &details);
    assert!(
        text.contains("~/.bashrc: rc ~ update"),
        "rc header marks update: {text}"
    );
    assert!(!text.contains("←"), "headers carry no attribution: {text}");
    assert!(
        text.contains("~ alias cat = oldbat → bat"),
        "altered alias renders changed: {text}"
    );
    assert!(
        text.contains("- alias oldkey = oldval"),
        "extra previous alias renders removed: {text}"
    );
    let lines: Vec<&str> = text.lines().collect();
    let mise_index = lines
        .iter()
        .position(|line| line.starts_with("~/.config/mise/config.toml"))
        .expect("mise header");
    assert_eq!(
        lines[mise_index], "~/.config/mise/config.toml: structured",
        "matching document collapses to header alone"
    );
    assert_eq!(
        lines[mise_index + 1],
        "Plan: 0 to add, 1 to change, 0 to destroy.",
        "unchanged document emits no entry lines"
    );
}

#[test]
fn colliding_alias_emits_winner_note() {
    let dir = tempfile::tempdir().expect("tempdir");
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(
        &profile_path,
        r#"
        local aaa = confit.config("aaa")
        aaa:add_document(confit.document.rc.alias("cat", "bat"))
        local zzz = confit.config("zzz")
        zzz:add_document(confit.document.rc.alias("cat", "eza"))
        return { shells = { "bash" }, configs = { aaa, zzz } }
        "#,
    )
    .expect("write profile");
    let graph = evaluate(dir.path(), &profile_path, None).expect("evaluate");
    let planned = confit::services::plan::build_plan(&graph, "root", "profile", false)
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
    assert_eq!(
        data.aliases
            .iter()
            .find(|entry| entry.spec.name == "cat")
            .map(|entry| entry.spec.value.as_str()),
        Some("bat"),
        "winner value lands in the plan"
    );
}
