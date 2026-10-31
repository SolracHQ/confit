//! Restore runs over bundles, slots, and history.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{seed_file, test_sinks, test_stores};
use confit_cli::actions::{apply, bundle::BundleRunner, restore};
use confit_cli::cli::{ApplyArgs, BundleArgs, RestoreArgs};
use confit_driver::paths::BaseDirs;

/// Build one profile with a single text document holding the text.
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

/// Seed one project with the profile text.
fn seed_project(dir: &Path, relative: &str, text: &str) {
    let site = dir.join("site");
    seed_file(
        &site.join("main.lua"),
        profile_text(relative, text).as_bytes(),
    );
}

/// Bundle one project to the output path with shared stores.
fn bundle_project(dir: &Path, output: PathBuf) {
    let site = dir.join("site");
    let stores = test_stores(dir, "site");
    let args = BundleArgs {
        project: site,
        profile: None,
        output,
    };
    BundleRunner {
        args: &args,
        stores,
    }
    .execute()
    .unwrap();
}

/// Restore one package with the prompt skipped.
fn restore_source(dir: &Path, source: PathBuf) {
    let stores = test_stores(dir, "site");
    let args = RestoreArgs {
        source,
        force: true,
    };
    let mut input = std::io::BufReader::new(b"" as &[u8]);
    let hooks = dir.join("hooks");
    restore::run(&args, &mut input, stores, test_sinks(), hooks).unwrap();
}

/// Apply one project with the prompt skipped.
fn apply_project(dir: &Path) {
    let site = dir.join("site");
    let stores = test_stores(dir, "site");
    let args = ApplyArgs {
        project: site,
        profile: None,
        force: true,
    };
    let mut input = std::io::BufReader::new(b"" as &[u8]);
    let hooks = dir.join("hooks");
    apply::run(&args, &mut input, stores, test_sinks(), hooks).unwrap();
}

/// Read one home destination under the shared test root.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = BaseDirs::new().home.unwrap();
    std::fs::read(home.join(relative)).unwrap()
}

/// Write one home destination directly, bypassing the runners.
fn write_home(relative: &str, text: &[u8]) {
    let home = BaseDirs::new().home.unwrap();
    std::fs::write(home.join(relative), text).unwrap();
}

#[test]
fn restore_bundle_file_writes_home() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-cb-note", "hello");
    let bundle = dir.path().join("state.cb");
    bundle_project(dir.path(), bundle.clone());
    restore_source(dir.path(), bundle);
    assert_eq!(
        home_bytes("confit-restore-cb-note"),
        b"hello",
        "the bundle content lands home"
    );
}

#[test]
fn restore_named_state_writes_home() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-named-note", "hello");
    bundle_project(dir.path(), PathBuf::from("@restore-probe"));
    restore_source(dir.path(), PathBuf::from("@restore-probe"));
    assert_eq!(
        home_bytes("confit-restore-named-note"),
        b"hello",
        "the named content lands home"
    );
}

#[test]
fn restore_history_brings_original_back() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-restore-rollback-note", "original");
    apply_project(dir.path());
    assert_eq!(
        home_bytes("confit-restore-rollback-note"),
        b"original",
        "the apply lands first"
    );
    write_home("confit-restore-rollback-note", b"changed");
    restore_source(dir.path(), PathBuf::from("%1"));
    assert_eq!(
        home_bytes("confit-restore-rollback-note"),
        b"original",
        "the rollback returns the bytes"
    );
}
