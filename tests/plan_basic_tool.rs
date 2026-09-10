//! M1 basic-tool behavior test: real fixture through the lib API.
//!
//! Golden workflow: this test normalizes `created_at` to a fixed string,
//! renders via the in-memory writer, and compares byte-for-byte against
//! `tests/golden/plan_basic_tool.json`. To regenerate after an intentional
//! plan-shape change, print the rendered text from the golden test, eyeball
//! it against the fixture (`bat` alias, mise `bat = "latest"`, 2 artifacts,
//! `mise install` hook), then overwrite the golden file verbatim (no
//! trailing newline: `PlanFormat::Json` emits none).
//!
//! Uses memory store/writer fakes only; the fixture is read from the repo
//! via `CARGO_MANIFEST_DIR`. Never touches `$HOME`.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit::lua::evaluate;
use confit::model::{ArtifactData, Plan};
use confit::plan::{diff, orchestrate, summarize};
use confit::store::{
    MemoryState, MemoryWriter, PlanFormat, PlanWriter, State, StateEntry, StateStore,
};

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

/// Evaluate the real fixture and orchestrate with stable labels.
fn build_plan() -> Plan {
    let (root, profile) = fixture_paths();
    let graph = evaluate(&root, &profile).expect("fixture evaluates");
    orchestrate(&graph, &State::empty(), GOLDEN_ROOT, GOLDEN_PROFILE).expect("orchestrate")
}

/// Render `plan` through the memory writer (exercises the writer seam).
fn render_json(plan: &Plan) -> String {
    let writer = MemoryWriter::new();
    writer
        .write(plan, Path::new("plan.json"), PlanFormat::Json)
        .expect("memory write");
    let (text, format) = writer.take().expect("write captures");
    assert_eq!(format, PlanFormat::Json);
    text
}

#[test]
fn basic_tool_produces_rc_and_mise() {
    let plan = build_plan();
    assert_eq!(plan.version, 1);
    assert_eq!(plan.artifacts.len(), 2);
    assert_eq!(plan.hooks, vec!["mise install".to_string()]);

    let rc = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let ArtifactData::Rc(data) = &rc.data else {
        panic!("rc artifact holds rc data");
    };
    assert_eq!(data.aliases.get("cat").map(String::as_str), Some("bat"));

    let mise = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.config/mise/config.toml")
        .expect("mise artifact");
    let ArtifactData::Toml(table) = &mise.data else {
        panic!("mise artifact holds toml data");
    };
    let tools = table.get("tools").expect("tools table");
    assert_eq!(
        tools.get("bat").and_then(serde_json::Value::as_str),
        Some("latest")
    );

    let summary = diff(&plan, &State::empty());
    assert_eq!(summary.create, 2);
    assert_eq!(summary.update, 0);
    assert_eq!(summary.unchanged, 0);
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
    assert_eq!(first.artifacts.len(), second.artifacts.len());
    for (left, right) in first.artifacts.iter().zip(second.artifacts.iter()) {
        assert_eq!(left.path, right.path);
        assert!(!left.data_hash.is_empty());
        assert_eq!(left.data_hash, right.data_hash);
    }
}

#[test]
fn diff_counts_unchanged_when_state_matches() {
    let plan = build_plan();
    let rc = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: rc.data_hash.clone(),
            output_hash: String::new(),
            data: None,
        },
    );
    let store = MemoryState::with_state(State { artifacts });
    let previous = store.load().expect("memory load");
    let summary = diff(&plan, &previous);
    assert_eq!(summary.create, 1);
    assert_eq!(summary.update, 0);
    assert_eq!(summary.unchanged, 1);
}

#[test]
fn golden_summary_is_stable() {
    let plan = build_plan();
    let previous = State::empty();
    let counts = diff(&plan, &previous);
    let details = confit::diff::detail(&plan, &previous);
    let rendered = summarize(&plan, &counts, &details, false);
    let golden_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/plan_basic_tool.summary");
    let golden = std::fs::read_to_string(&golden_path).expect("read golden summary");
    assert_eq!(rendered, golden);
}

/// Seeded previous: matching mise collapses, altered alias changes, extra alias removes.
fn seeded_previous(plan: &Plan) -> State {
    let mise = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.config/mise/config.toml")
        .expect("mise artifact");
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "toml:~/.config/mise/config.toml".to_string(),
        StateEntry {
            data_hash: mise.data_hash.clone(),
            output_hash: String::new(),
            data: Some(serde_json::to_value(&mise.data).expect("mise snapshot")),
        },
    );
    artifacts.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: "previous".to_string(),
            output_hash: String::new(),
            data: Some(serde_json::json!({
                "rc": {
                    "profile": [],
                    "env": [],
                    "aliases": {"cat": "oldbat", "oldkey": "oldval"},
                    "init": []
                }
            })),
        },
    );
    State { artifacts }
}

#[test]
fn seeded_state_renders_change_and_remove() {
    let plan = build_plan();
    let previous = seeded_previous(&plan);
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update, counts.unchanged), (0, 1, 1));
    let details = confit::diff::detail(&plan, &previous);
    let mise = details
        .iter()
        .find(|detail| detail.key == "toml:~/.config/mise/config.toml")
        .expect("mise detail");
    assert_eq!(mise.status, confit::diff::ArtifactStatus::Unchanged);
    let rc = details
        .iter()
        .find(|detail| detail.key == "rc:~/.bashrc")
        .expect("rc detail");
    assert_eq!(rc.status, confit::diff::ArtifactStatus::Update);
    let text = summarize(&plan, &counts, &details, false);
    assert!(
        text.contains("~/.bashrc: rc ~ update ← bat"),
        "rc header marks update: {text}"
    );
    assert!(
        text.contains("~ alias cat = oldbat → bat (bat)"),
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
        lines[mise_index], "~/.config/mise/config.toml: toml ← bat",
        "matching artifact collapses to header alone"
    );
    assert_eq!(
        lines[mise_index + 1],
        "plan: 0 create, 1 update, 1 unchanged",
        "unchanged artifact emits no entry lines"
    );
}

#[test]
fn conflicts_flag_toggles_loser_display() {
    use confit::diff::{ArtifactDetail, ArtifactStatus, ChangeKind, EntryChange};
    use confit::model::{Artifact, ArtifactData, ArtifactKind, BlameSet, Contribution};
    use std::collections::BTreeMap;

    let mut aliases = BTreeMap::new();
    aliases.insert("cat".to_string(), "bat".to_string());
    let mut blame_aliases = BTreeMap::new();
    blame_aliases.insert("cat".to_string(), "bat".to_string());
    let plan = Plan {
        version: 1,
        created_at: "2026-09-09T00:00:00Z".into(),
        root: GOLDEN_ROOT.into(),
        profile: GOLDEN_PROFILE.into(),
        artifacts: vec![Artifact {
            kind: ArtifactKind::Rc,
            path: "~/.bashrc".into(),
            data: ArtifactData::Rc(confit::model::RcData {
                profile: Vec::new(),
                env: Vec::new(),
                aliases,
                init: Vec::new(),
            }),
            contributions: vec![Contribution {
                tool: "bat".into(),
                order: 1,
            }],
            shadowed: Default::default(),
            blame: BlameSet {
                aliases: blame_aliases,
                ..BlameSet::default()
            },
            data_hash: "new".into(),
        }],
        hooks: Vec::new(),
    };
    let counts = confit::plan::DiffSummary {
        create: 0,
        update: 1,
        unchanged: 0,
    };
    let details = vec![ArtifactDetail {
        key: "rc:~/.bashrc".to_string(),
        status: ArtifactStatus::Update,
        entries: vec![EntryChange {
            label: "alias cat".to_string(),
            change: ChangeKind::Changed {
                from: "oldbat".to_string(),
                to: "bat".to_string(),
            },
            tool: Some("bat".to_string()),
            over: Some("eza".to_string()),
        }],
    }];
    let plain = summarize(&plan, &counts, &details, false);
    assert!(
        plain.contains("~ alias cat = oldbat → bat (bat)"),
        "default shows winner only: {plain}"
    );
    assert!(!plain.contains("wins over"), "{plain}");
    let conflicts = summarize(&plan, &counts, &details, true);
    assert!(
        conflicts.contains("~ alias cat = oldbat → bat (bat wins over eza)"),
        "flag shows winner over loser: {conflicts}"
    );
}
