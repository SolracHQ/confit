//! Plan runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{repo_path, seed_file, seed_tree, test_sinks, test_stores};
use confit_cli::actions::plan::PlanRunner;
use confit_cli::cli::PlanArgs;
use confit_cli::error::CliError;

/// Fetch-free profile over the shared sugar plugin.
const SUGAR_MAIN: &str = r#"
local sugar = require("plugins._.sugar")
return {
    documents = { sugar.rc(sugar.home(".confit-probe")) },
    configs = {},
}
"#;

/// Second profile for the explicit profile choice run.
const ALT_MAIN: &str = r#"
return {
    configs = {
        {
            name = "alt",
            documents = { { path = { base = "home", relative = "alt-note" }, text = "alt" } },
        },
    },
}
"#;

/// Seed the real example tree with the shared plugins beside it.
fn example_project(dir: &Path) -> PathBuf {
    let site = dir.join("nest").join("site");
    seed_tree(&repo_path("../../examples/0-basic_tool"), &site);
    seed_tree(&repo_path("../../plugins"), &dir.join("plugins"));
    seed_file(&site.join("main.lua"), SUGAR_MAIN.as_bytes());
    site
}

/// Run one plan over the project with the optional profile and output.
fn run_plan(dir: &Path, project: &Path, profile: Option<&Path>, output: Option<PathBuf>) -> String {
    let stores = test_stores(dir, "nest/site");
    let args = PlanArgs {
        project: project.to_path_buf(),
        profile: profile.map(Path::to_path_buf),
        output,
    };
    PlanRunner {
        args: &args,
        stores,
        sinks: test_sinks(),
    }
    .execute()
    .unwrap()
    .text
}

#[test]
fn plan_shows_preview_for_example_project() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let text = run_plan(dir.path(), &site, None, None);
    assert!(
        text.contains("Summary"),
        "the preview closes with counts: {text}"
    );
    assert!(
        text.contains("home('.confit-probe')"),
        "the preview shows the destination: {text}"
    );
}

#[test]
fn plan_bundle_lands_on_temp_path() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let output = dir.path().join("plan.cb");
    let _ = run_plan(dir.path(), &site, None, Some(output.clone()));
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.is_empty(), "the bundle holds bytes");
}

#[test]
fn plan_without_main_lua_fails() {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("nest").join("site");
    std::fs::create_dir_all(&site).unwrap();
    let stores = test_stores(dir.path(), "nest/site");
    let args = PlanArgs {
        project: site,
        profile: None,
        output: None,
    };
    match (PlanRunner {
        args: &args,
        stores,
        sinks: test_sinks(),
    })
    .execute()
    {
        Err(CliError::NoDefaultProfile { .. }) => {}
        Err(other) => panic!("absent default flattens: {other}"),
        Ok(_) => panic!("absent default passes"),
    }
}

#[test]
fn plan_explicit_profile_beats_default() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    seed_file(&site.join("laptop.lua"), ALT_MAIN.as_bytes());
    let text = run_plan(dir.path(), &site, Some(Path::new("laptop.lua")), None);
    assert!(
        text.contains("home('alt-note')"),
        "the preview shows the chosen profile: {text}"
    );
}
