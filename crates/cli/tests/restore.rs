//! Restore runs over bundles, slots, and history.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::{apply, bundle::BundleRunner, restore};
use confit_cli::cli::{ApplyArgs, BundleArgs, RestoreArgs};
use confit_driver::paths::BaseDirs;

/// Builds a profile that writes one text document with the given text.
fn profile_text(relative: &str, text: &str) -> String {
    format!(
        r#"
return {{
    configs = {{
        {{
            name = id("note"),
            documents = {{ {{ path = route("home", "{relative}"), text = "{text}" }} }},
        }},
    }},
}}
"#
    )
}

/// Seeds the site profile with the document text.
fn seed_project(dir: &Path, relative: &str, text: &str) {
    let site = dir.join("site");
    common::seed_file(
        &site.join("main.lua"),
        profile_text(relative, text).as_bytes(),
    );
}

/// Bundles the site project to the output path with the test stores.
fn bundle_project(dir: &Path, output: PathBuf) {
    let site = dir.join("site");
    let stores = common::test_stores(dir, "site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output,
    };
    BundleRunner {
        args: &args,
        stores,
        handles: common::test_handles(),
    }
    .execute()
    .unwrap();
}

/// Restores one package into the home folder with the prompt skipped.
fn restore_source(dir: &Path, source: PathBuf) {
    let stores = common::test_stores(dir, "site");
    let args = RestoreArgs {
        source,
        force: true,
    };
    let hooks = dir.join("hooks");
    restore::run(&args, stores, common::test_handles(), hooks).unwrap();
}

/// Applies the site project with the prompt skipped and drops the report.
fn apply_project(dir: &Path) {
    let site = dir.join("site");
    let stores = common::test_stores(dir, "site");
    let args = ApplyArgs {
        project: site,
        profile: None,
        force: true,
    };
    let hooks = dir.join("hooks");
    apply::run(&args, stores, common::test_handles(), hooks).unwrap();
}

/// Reads one home destination under the shared test root.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = BaseDirs::new().home.unwrap();
    std::fs::read(home.join(relative)).unwrap()
}

/// Writes one home destination directly and bypasses the runners.
fn write_home(relative: &str, text: &[u8]) {
    let home = BaseDirs::new().home.unwrap();
    std::fs::write(home.join(relative), text).unwrap();
}

#[test]
fn restore_bundle_file_writes_the_home_destination() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-cb-note", "hello");
    let bundle = dir.path().join("state.cb");
    bundle_project(dir.path(), bundle.clone());
    restore_source(dir.path(), bundle);
    assert_eq!(
        home_bytes("confit-restore-cb-note"),
        b"hello",
        "the restore writes the bundle content to home"
    );
}

#[test]
fn restore_named_state_writes_the_home_destination() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-named-note", "hello");
    bundle_project(dir.path(), PathBuf::from("@restore-probe"));
    restore_source(dir.path(), PathBuf::from("@restore-probe"));
    assert_eq!(
        home_bytes("confit-restore-named-note"),
        b"hello",
        "the restore writes the named content to home"
    );
}

#[test]
fn restore_history_returns_the_original() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-rollback-note", "original");
    apply_project(dir.path());
    seed_project(dir.path(), "confit-restore-rollback-note", "second");
    apply_project(dir.path());
    assert_eq!(
        home_bytes("confit-restore-rollback-note"),
        b"second",
        "the second apply writes the second text"
    );
    write_home("confit-restore-rollback-note", b"changed");
    restore_source(dir.path(), PathBuf::from("%1"));
    assert_eq!(
        home_bytes("confit-restore-rollback-note"),
        b"original",
        "the history restore writes the original bytes"
    );
}

#[test]
fn restore_previous_history_returns_the_earlier_run() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-prev-note", "first");
    apply_project(dir.path());
    seed_project(dir.path(), "confit-restore-prev-note", "second");
    apply_project(dir.path());
    assert_eq!(
        home_bytes("confit-restore-prev-note"),
        b"second",
        "the second apply writes the second text"
    );
    restore_source(dir.path(), PathBuf::from("%1"));
    assert_eq!(
        home_bytes("confit-restore-prev-note"),
        b"first",
        "the previous history entry writes the first bytes"
    );
}

#[test]
fn restore_history_rejects_an_out_of_range_pick() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-range-note", "only");
    apply_project(dir.path());
    let stores = common::test_stores(dir.path(), "site");
    let args = RestoreArgs {
        source: PathBuf::from("%1"),
        force: true,
    };
    let hooks = dir.path().join("hooks");
    assert!(
        restore::run(&args, stores, common::test_handles(), hooks).is_err(),
        "one entry leaves %1 out of range"
    );
}

#[test]
fn restore_history_rejects_slot_zero() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-zero-note", "only");
    apply_project(dir.path());
    let stores = common::test_stores(dir.path(), "site");
    let args = RestoreArgs {
        source: PathBuf::from("%0"),
        force: true,
    };
    let hooks = dir.path().join("hooks");
    assert!(
        restore::run(&args, stores, common::test_handles(), hooks).is_err(),
        "slot zero is internal and never parses"
    );
}
