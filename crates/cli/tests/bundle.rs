//! Bundle runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::bundle::BundleRunner;
use confit_cli::cli::{BundleArgs, UiView};
use confit_events::EventHandles;
use confit_store::{StoreRoots, Stores, state::Picker};

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

/// Copies the example project and the shared plugins into the test folder.
fn example_project(dir: &Path) -> PathBuf {
    let site = dir.join("nest").join("site");
    seed_tree(&repo_path("../../examples/0-basic_tool"), &site);
    seed_tree(&repo_path("../../plugins"), &dir.join("plugins"));
    common::seed_file(&site.join("main.lua"), SUGAR_MAIN.as_bytes());
    site
}

#[test]
fn bundle_file_holds_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let output = dir.path().join("bundle.cb");
    let stores = test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: output.clone(),
        view: UiView::Desired,
        force: true,
    };
    let report = BundleRunner {
        args: &args,
        stores,
        handles: test_handles(),
    }
    .execute()
    .unwrap();
    assert!(
        report.output.contains("bundle.cb"),
        "the report shows the bundle: {}",
        report.output
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.is_empty(), "the bundle holds bytes");
}

#[test]
fn named_bundle_state_holds_documents() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let stores = test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: PathBuf::from("@probe"),
        view: UiView::Desired,
        force: true,
    };
    let report = BundleRunner {
        args: &args,
        stores: stores.clone(),
        handles: test_handles(),
    }
    .execute()
    .unwrap();
    assert!(
        report.output.contains("@probe"),
        "the report shows the state: {}",
        report.output
    );
    let picker: Picker = "@probe".parse().unwrap();
    let manifest = stores.states().load_state(&picker).unwrap();
    assert!(
        !manifest.documents.is_empty(),
        "the named state holds documents"
    );
}
