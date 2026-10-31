//! Bundle runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{repo_path, seed_file, seed_tree, test_stores};
use confit_cli::actions::bundle::BundleRunner;
use confit_cli::cli::BundleArgs;
use confit_runtime::manifest::stored;
use confit_store::state::Picker;

/// Fetch-free profile over the shared sugar plugin.
const SUGAR_MAIN: &str = r#"
local sugar = require("plugins._.sugar")
return {
    documents = { sugar.rc(sugar.home(".confit-probe")) },
    configs = {},
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

#[test]
fn bundle_file_lands_non_empty_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let output = dir.path().join("bundle.cb");
    let stores = test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: output.clone(),
    };
    let report = BundleRunner {
        args: &args,
        stores,
    }
    .execute()
    .unwrap();
    assert!(
        report.text.contains("bundle.cb"),
        "the report names the bundle: {}",
        report.text
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.is_empty(), "the bundle holds bytes");
}

#[test]
fn bundle_named_state_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let stores = test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: PathBuf::from("@probe"),
    };
    let report = BundleRunner {
        args: &args,
        stores: stores.clone(),
    }
    .execute()
    .unwrap();
    assert!(
        report.text.contains("@probe"),
        "the report names the state: {}",
        report.text
    );
    let picker: Picker = "@probe".parse().unwrap();
    let manifest = stored::resolve(&stores.states(), &picker).unwrap();
    assert!(
        !manifest.documents.is_empty(),
        "the named state holds documents"
    );
}
