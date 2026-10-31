//! Check runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{repo_path, seed_file, seed_tree, test_sinks, test_stores};
use confit_cli::actions::check::CheckRunner;
use confit_cli::cli::CheckArgs;

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
            name = id("alt"),
            documents = { { path = route("home", "alt-note"), text = "alt" } },
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

/// Run one check over the project with the optional profile.
fn run_check(dir: &Path, project: &Path, profile: Option<&Path>) -> String {
    let stores = test_stores(dir, "nest/site");
    let args = CheckArgs {
        project: project.to_path_buf(),
        profile: profile.map(Path::to_path_buf),
    };
    CheckRunner {
        args: &args,
        stores,
        sinks: test_sinks(),
    }
    .execute()
    .unwrap()
    .text
}

#[test]
fn check_shows_preview_for_example_project() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let text = run_check(dir.path(), &site, None);
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
fn check_explicit_profile_beats_default() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    seed_file(&site.join("laptop.lua"), ALT_MAIN.as_bytes());
    let text = run_check(dir.path(), &site, Some(Path::new("laptop.lua")));
    assert!(
        text.contains("home('alt-note')"),
        "the preview shows the chosen profile: {text}"
    );
}
