//! Three-way behavior tests: disk-aware plan diffs plus disk diffs.
//!
//! Exercises `plan::diff` plus `plan::disk_warnings` through the real `0-basic_tool`
//! fixture (mirroring the wiring in `services::plan::snapshot_current`
//! over memory snapshots keyed by `"kind:path"`), plus
//! `diff::detail_disk` shapes plus `presentation::render_drift` over structured and byte kinds.
//! Uses memory fakes only; never touches `$HOME`.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use confit::model::dto::diff::ChangeLine;
use confit::model::dto::diff::Sigil;
use confit::model::dto::snapshot::Snapshot;
use confit::model::dto::warning::WarningKind;
use confit::model::state::State;
use confit::model::state::StateEntry;
use confit::model::state::plan::PLAN_VERSION;
use confit::model::state::plan::Plan;
use confit::presentation::{render_drift, render_plan, warning_line};
use confit::repository::MemoryFilesystem;
use confit::security::sha256_hex;
use confit::services::diff::detail_disk;
use confit::services::plan::{diff, disk_warnings, evaluate_profile, snapshot_current, summarize};

/// Fixture root and profile path inside the repo.
fn fixture_paths() -> (PathBuf, PathBuf) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("examples/0-basic_tool");
    let profile = root.join("profile.lua");
    (root, profile)
}

/// Evaluate the real fixture and plan with stable labels.
fn build_plan() -> (Plan, PathBuf) {
    let (root, profile) = fixture_paths();
    let graph = evaluate_profile(&root, &profile).expect("fixture evaluates");
    let plan = confit::actions::plan(
        &graph,
        "examples/0-basic_tool",
        "examples/0-basic_tool/profile.lua",
    )
    .expect("plan");
    (plan, root)
}

/// Expand a leading `~` against runtime `$HOME`; plain paths pass through unchanged.
fn expanded_key(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").expect("HOME reads");
        format!("{home}/{rest}")
    } else if path == "~" {
        std::env::var("HOME").expect("HOME reads")
    } else {
        path.to_string()
    }
}

/// Render every plan artifact to its desired bytes, keyed `"kind:path"`.
fn render_all(plan: &Plan, root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let files = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    confit::services::render::render_baseline(plan, root, &files).expect("renders")
}

#[test]
fn clean_disk_reports_unchanged_without_warnings() {
    let (plan, root) = build_plan();
    let rendered = render_all(&plan, &root);
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    for artifact in &plan.artifacts {
        let key = format!("{}:{}", artifact.kind, artifact.path);
        fs.files
            .borrow_mut()
            .insert(expanded_key(&artifact.path), rendered[&key].clone());
    }
    let snapshots = snapshot_current(&fs, &plan);
    // Seed the previous record from the plan itself: hashes match.
    let mut previous = State::empty();
    for artifact in &plan.artifacts {
        previous.artifacts.insert(
            format!("{}:{}", artifact.kind, artifact.path),
            StateEntry {
                data_hash: artifact.data_hash.clone(),
                output_hash: sha256_hex(&rendered[&format!("{}:{}", artifact.kind, artifact.path)]),
                data: None,
            },
        );
    }
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update, counts.unchanged), (0, 0, 2));
    assert_eq!(counts.delete, 0);
    let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
    assert!(warnings.is_empty());
    // No disk lines while clean.
    let disk = detail_disk(&plan, &rendered, &snapshots);
    assert_eq!(disk.len(), 2);
    assert!(disk.iter().all(|entry| entry.lines.is_empty()));
    assert_eq!(render_drift(&plan, &disk), "");
}

#[test]
fn edited_disk_reports_manual_modification() {
    let (plan, root) = build_plan();
    let rendered = render_all(&plan, &root);
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    for artifact in &plan.artifacts {
        fs.files.borrow_mut().insert(
            expanded_key(&artifact.path),
            rendered[&format!("{}:{}", artifact.kind, artifact.path)].clone(),
        );
    }
    // Hand-edit the mise file on disk.
    fs.files.borrow_mut().insert(
        expanded_key("~/.config/mise/config.toml"),
        b"[tools]\nbat = \"old\"\n".to_vec(),
    );
    let snapshots = snapshot_current(&fs, &plan);
    let mut previous = State::empty();
    for artifact in &plan.artifacts {
        previous.artifacts.insert(
            format!("{}:{}", artifact.kind, artifact.path),
            StateEntry {
                data_hash: artifact.data_hash.clone(),
                output_hash: String::new(),
                data: None,
            },
        );
    }
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update, counts.unchanged), (0, 0, 2));
    let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].path, "~/.config/mise/config.toml");
    assert_eq!(warnings[0].kind, WarningKind::ManualModification);
    assert_eq!(
        warning_line(&warnings[0]),
        "~/.config/mise/config.toml: differs from recorded: manual modification will be overwritten"
    );
    // The disk diff shows the edited key.
    let disk = detail_disk(&plan, &rendered, &snapshots);
    let mise = disk
        .iter()
        .find(|entry| entry.key == "toml:~/.config/mise/config.toml")
        .expect("mise disk detail");
    assert_eq!(
        mise.lines,
        vec![ChangeLine::keyed(
            Sigil::Update,
            "tools.bat",
            Some("old"),
            Some("latest"),
        )]
    );
    let text = render_drift(&plan, &disk);
    assert!(
        text.starts_with("Note: Objects have changed outside of ConfIt"),
        "{text}"
    );
    assert!(
        text.contains("  ~ artifact \"toml\" \"~/.config/mise/config.toml\" {"),
        "{text}"
    );
}

#[test]
fn untracked_disk_file_warns_overwrite() {
    let (plan, root) = build_plan();
    let rendered = render_all(&plan, &root);
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    fs.files
        .borrow_mut()
        .insert(expanded_key("~/.bashrc"), b"# my own bashrc\n".to_vec());
    let snapshots = snapshot_current(&fs, &plan);
    let previous = State::empty();
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update, counts.unchanged), (2, 0, 0));
    let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, WarningKind::OverwriteUntracked);
    assert_eq!(
        warning_line(&warnings[0]),
        "~/.bashrc: exists but no record: will be overwritten"
    );
}

#[test]
fn unreadable_disk_warns_and_stays_unchanged() {
    let (plan, root) = build_plan();
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    fs.failures
        .borrow_mut()
        .insert(expanded_key("~/.bashrc"), "permission denied".to_string());
    let snapshots = snapshot_current(&fs, &plan);
    let rc = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "~/.bashrc")
        .expect("rc artifact");
    let mut previous = State::empty();
    previous.artifacts.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: rc.data_hash.clone(),
            output_hash: String::new(),
            data: None,
        },
    );
    let rendered = render_all(&plan, &root);
    let counts = diff(&plan, &previous);
    assert_eq!((counts.create, counts.update, counts.unchanged), (1, 0, 1));
    let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
    assert_eq!(warnings.len(), 1);
    assert!(
        matches!(warnings[0].kind, WarningKind::Unreadable { .. }),
        "{:?}",
        warnings[0]
    );
    assert_eq!(
        warning_line(&warnings[0]),
        "cannot read '~/.bashrc': permission denied"
    );
}

#[test]
fn stale_record_counts_update_and_delete_flows_to_summary() {
    let (plan, _root) = build_plan();
    let mut previous = State::empty();
    previous.artifacts.insert(
        "rc:~/.bashrc".to_string(),
        StateEntry {
            data_hash: "stale".into(),
            output_hash: String::new(),
            data: None,
        },
    );
    previous.artifacts.insert(
        "toml:gone.toml".to_string(),
        StateEntry {
            data_hash: "old".into(),
            output_hash: String::new(),
            data: None,
        },
    );
    let counts = diff(&plan, &previous);
    assert_eq!(
        (
            counts.create,
            counts.update,
            counts.unchanged,
            counts.delete
        ),
        (1, 1, 0, 1)
    );
    let details = confit::services::diff::detail(&plan, &previous);
    let shaped = summarize(&counts);
    let text = render_plan(&plan, &shaped, &details, false);
    assert!(
        text.ends_with("Plan: 1 to add, 1 to change, 1 to destroy."),
        "{text}"
    );
}

#[test]
fn file_disk_diff_is_unified_and_snapshot_variants_stay_silent() {
    use confit::model::state::artifact::Artifact;
    use confit::model::state::artifact::ArtifactData;
    use confit::model::state::artifact::ArtifactKind;

    let plan = Plan {
        version: PLAN_VERSION,
        created_at: "2026-09-09T00:00:00Z".into(),
        root: "r".into(),
        profile: "p".into(),
        artifacts: vec![Artifact {
            kind: ArtifactKind::File,
            path: "dot.txt".into(),
            data: ArtifactData::File {
                content: "one\ntwo\n".into(),
            },
            contributions: Vec::new(),
            shadowed: Default::default(),
            blame: Default::default(),
            data_hash: "hash".into(),
        }],
    };
    let rendered = [("file:dot.txt".to_string(), b"one\ntwo\n".to_vec())]
        .into_iter()
        .collect();
    let snapshots = [(
        "file:dot.txt".to_string(),
        Snapshot::Present {
            bytes: b"one\nCHANGED\n".to_vec(),
            hash: sha256_hex(b"one\nCHANGED\n"),
        },
    )]
    .into_iter()
    .collect();
    let disk = detail_disk(&plan, &rendered, &snapshots);
    assert_eq!(disk.len(), 1);
    assert_eq!(
        disk[0].lines,
        vec![
            ChangeLine::header("--- on disk"),
            ChangeLine::header("+++ desired"),
            ChangeLine::header("@@ -1,2 +1,2 @@"),
            ChangeLine::text(Sigil::Context, "one"),
            ChangeLine::text(Sigil::Remove, "CHANGED"),
            ChangeLine::text(Sigil::Add, "two"),
        ]
    );
    let note = render_drift(&plan, &disk);
    assert!(
        note.starts_with("Note: Objects have changed outside of ConfIt"),
        "{note}"
    );
    assert!(note.contains("  # dot.txt has changed"), "{note}");
    assert!(
        note.contains("  ~ artifact \"file\" \"dot.txt\" {"),
        "{note}"
    );
    assert!(note.contains("+two"), "{note}");
}
