//! Plan warnings behavior: missing plus unreadable snapshots still succeed.
//!
//! Runs `run_plan` against a memory snapshot where one artifact path is
//! absent (silent) and another is unreadable (warning). The run must stay
//! `Ok` while carrying at least one warning. Uses memory fakes only and
//! never touches `$HOME`.

//! Test target allowing `expect`: expectations assert success, and a
//! failure fails the test, which is the desired outcome.
#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use confit::actions::run_plan;
use confit::cli::PlanArgs;
use confit::repository::MemoryFilesystem;

#[test]
fn plan_with_missing_and_unreadable_still_warns() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("examples/0-basic_tool");
    let profile = root.join("profile.lua");
    let args = PlanArgs {
        profile,
        root: Some(root),
        output: None,
        state: None,
        conflicts: false,
    };
    let home = std::env::var("HOME").expect("HOME reads");
    let expanded = format!("{home}/.bashrc");
    let mut failures = BTreeMap::new();
    failures.insert(expanded, "denied".to_string());
    let fs = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(failures),
    };
    let outcome = run_plan(&args, &fs).expect("plan succeeds with warnings");
    assert_eq!(outcome.plan.artifacts.len(), 2);
    let summary = confit::presentation::render_plan_outcome(&outcome);
    assert!(
        summary.contains("Plan:"),
        "summary still renders: {summary}"
    );
    assert!(
        !outcome.warnings.is_empty(),
        "unreadable path carries warnings"
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| confit::presentation::warning_line(warning).contains("~/.bashrc")),
        "warning names the unreadable path"
    );
}
