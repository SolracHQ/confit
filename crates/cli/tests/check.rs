//! Check runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::check::CheckRunner;
use confit_cli::cli::CheckArgs;
use confit_differ::diff::Diff;
use confit_events::{
    EventHandles,
    protocol::{Phase, ToUi},
};
use confit_store::{StoreRoots, Stores};

/// Builds stores under one temporary folder with a silent progress sink.
fn test_stores(dir: &Path, project: &str) -> Stores {
    Stores::new(
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join(project),
        },
        EventHandles::silent(),
    )
}

/// Builds UI handles with no reader and no reply.
fn test_handles() -> EventHandles {
    EventHandles::silent()
}

/// Copies every file of a host tree under a temporary folder.
fn seed_tree(host: &Path, dest: &Path) {
    let mut stack = vec![host.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let source = entry.path();
            let file_type = entry.file_type().unwrap();
            let target = dest.join(source.strip_prefix(host).unwrap());
            if file_type.is_dir() {
                std::fs::create_dir_all(&target).unwrap();
                stack.push(source);
            } else if file_type.is_file() {
                let bytes = std::fs::read(&source).unwrap();
                common::seed_file(&target, &bytes);
            }
        }
    }
}

/// Resolves one repository path from the crate folder.
fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

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
    seed_tree(&repo_path("../../examples/0-basic_tool"), &site);
    seed_tree(&repo_path("../../plugins"), &dir.join("plugins"));
    common::seed_file(&site.join("main.lua"), SUGAR_MAIN.as_bytes());
    site
}

/// Checks one project with the optional profile and returns the diff.
fn run_check(dir: &Path, project: &Path, profile: Option<&Path>) -> Diff {
    let stores = test_stores(dir, "nest/site");
    let args = CheckArgs {
        project: project.to_path_buf(),
        profile: profile.map(Path::to_path_buf),
    };
    CheckRunner {
        args: &args,
        stores,
        handles: test_handles(),
    }
    .execute()
    .unwrap()
    .diff
}

#[test]
fn check_previews_the_example_project() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let diff = run_check(dir.path(), &site, None);
    assert_eq!(
        diff.desired.len(),
        1,
        "the preview holds one row: {}",
        diff.desired.len()
    );
    assert!(
        diff.desired
            .iter()
            .any(|change| change.path.display() == "home('.confit-probe')"),
        "the preview shows the destination: {:?}",
        diff.desired
            .iter()
            .map(|change| change.path.display())
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_explicit_profile_beats_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    common::seed_file(&site.join("laptop.lua"), ALT_MAIN.as_bytes());
    let diff = run_check(dir.path(), &site, Some(Path::new("laptop.lua")));
    assert_eq!(
        diff.desired.len(),
        1,
        "the preview holds one row: {}",
        diff.desired.len()
    );
    assert!(
        diff.desired
            .iter()
            .any(|change| change.path.display() == "home('alt-note')"),
        "the preview shows the chosen profile: {:?}",
        diff.desired
            .iter()
            .map(|change| change.path.display())
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_shows_the_desired_preview_only() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
    let (_, from_ui) = crossbeam_channel::unbounded::<confit_events::protocol::FromUi>();
    let handles = EventHandles::new(to_ui, from_ui);
    let stores = test_stores(dir.path(), "nest/site");
    let args = CheckArgs {
        project: site,
        profile: None,
    };
    let diff = CheckRunner {
        args: &args,
        stores,
        handles,
    }
    .execute()
    .unwrap()
    .diff;
    let mut previews = Vec::new();
    let mut starts = Vec::new();
    let mut rest = Vec::new();
    while let Ok(message) = incoming.try_recv() {
        match message {
            ToUi::DesiredPreview(found) => previews.push(found),
            ToUi::PhaseStarted(phase) => starts.push(phase),
            _ => rest.push(message),
        }
    }
    assert_eq!(previews, vec![diff.clone()], "the preview carries the diff");
    assert_eq!(
        starts,
        vec![Phase::Evaluation],
        "the execution announces evaluation before the preview"
    );
    assert!(rest.is_empty(), "the execution sends no other event");
    assert_eq!(
        diff.desired.len(),
        1,
        "the preview holds one row: {}",
        diff.desired.len()
    );
}
