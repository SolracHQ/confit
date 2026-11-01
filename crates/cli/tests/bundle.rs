//! Bundle runs over example project layouts.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::bundle::BundleRunner;
use confit_cli::cli::BundleArgs;
use confit_runtime::manifest::stored;
use confit_store::state::Picker;

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
    common::seed_tree(&common::repo_path("../../examples/0-basic_tool"), &site);
    common::seed_tree(&common::repo_path("../../plugins"), &dir.join("plugins"));
    common::seed_file(&site.join("main.lua"), SUGAR_MAIN.as_bytes());
    site
}

#[test]
fn bundle_file_holds_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let output = dir.path().join("bundle.cb");
    let stores = common::test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: output.clone(),
    };
    let report = BundleRunner {
        args: &args,
        stores,
        handles: common::test_handles(),
    }
    .execute()
    .unwrap();
    assert!(
        report.text.contains("bundle.cb"),
        "the report shows the bundle: {}",
        report.text
    );
    let bytes = std::fs::read(&output).unwrap();
    assert!(!bytes.is_empty(), "the bundle holds bytes");
}

#[test]
fn named_bundle_state_holds_documents() {
    let dir = tempfile::tempdir().unwrap();
    let site = example_project(dir.path());
    let stores = common::test_stores(dir.path(), "nest/site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output: PathBuf::from("@probe"),
    };
    let report = BundleRunner {
        args: &args,
        stores: stores.clone(),
        handles: common::test_handles(),
    }
    .execute()
    .unwrap();
    assert!(
        report.text.contains("@probe"),
        "the report shows the state: {}",
        report.text
    );
    let picker: Picker = "@probe".parse().unwrap();
    let manifest = stored::resolve(&stores.states(), &picker).unwrap();
    assert!(
        !manifest.documents.is_empty(),
        "the named state holds documents"
    );
}
