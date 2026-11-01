//! Check runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::check::CheckRunner;
use confit_cli::cli::CheckArgs;

/// A profile that requires the shared sugar plugin and needs no fetch.
const SUGAR_MAIN: &str = r#"
local sugar = require("plugins._.sugar")
return {
    documents = { sugar.rc(sugar.home(".confit-probe")) },
    configs = {},
}
"#;

/// A profile that writes one alternative document.
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

/// Copies the example project and the shared plugins into the test folder.
fn example_project(dir: &Path) -> PathBuf {
    let site = dir.join("nest").join("site");
    common::seed_tree(&common::repo_path("../../examples/0-basic_tool"), &site);
    common::seed_tree(&common::repo_path("../../plugins"), &dir.join("plugins"));
    common::seed_file(&site.join("main.lua"), SUGAR_MAIN.as_bytes());
    site
}

/// Checks one project with the optional profile and returns the preview.
fn run_check(dir: &Path, project: &Path, profile: Option<&Path>) -> String {
    let stores = common::test_stores(dir, "nest/site");
    let args = CheckArgs {
        project: project.to_path_buf(),
        profile: profile.map(Path::to_path_buf),
    };
    CheckRunner {
        args: &args,
        stores,
        handles: common::test_handles(),
    }
    .execute()
    .unwrap()
    .text
}

#[test]
fn check_previews_the_example_project() {
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
fn check_explicit_profile_beats_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    common::seed_file(&site.join("laptop.lua"), ALT_MAIN.as_bytes());
    let text = run_check(dir.path(), &site, Some(Path::new("laptop.lua")));
    assert!(
        text.contains("home('alt-note')"),
        "the preview shows the chosen profile: {text}"
    );
}
