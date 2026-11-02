//! Rescue slot capture over unmanaged destinations.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::{apply, restore};
use confit_cli::cli::{ApplyArgs, RestoreArgs};
use confit_driver::paths::BaseDirs;
use confit_store::state::Picker;

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

/// Builds a profile that writes one tree member from a project file.
fn profile_tree(folder: &str, member: &str) -> String {
    format!(
        r#"
local open = require("confit.open")
local seed = assert(open("seed.txt"):ref())
return {{
    configs = {{
        {{
            name = id("tree"),
            documents = {{
                {{
                    path = route("home", "{folder}"),
                    members = {{ ["{member}"] = seed }},
                }},
            }},
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

/// Seeds the site profile with a tree document and its project file.
fn seed_tree_project(dir: &Path, folder: &str, member: &str, text: &str) {
    let site = dir.join("site");
    common::seed_file(&site.join("seed.txt"), text.as_bytes());
    common::seed_file(
        &site.join("main.lua"),
        profile_tree(folder, member).as_bytes(),
    );
}

/// Applies the site project with the prompt skipped and keeps the report.
fn apply_project(dir: &Path) -> confit_cli::actions::commit::WriteReport {
    let site = dir.join("site");
    let stores = common::test_stores(dir, "site");
    let args = ApplyArgs {
        project: site,
        profile: None,
        force: true,
    };
    let hooks = dir.join("hooks");
    apply::run(&args, stores, common::test_handles(), hooks).unwrap()
}

/// Restores one source into the home folder with the prompt skipped.
fn restore_source(dir: &Path, source: PathBuf) {
    let stores = common::test_stores(dir, "site");
    let args = RestoreArgs {
        source,
        force: true,
    };
    let hooks = dir.join("hooks");
    restore::run(&args, stores, common::test_handles(), hooks).unwrap();
}

/// Reads one home destination under the shared test root.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = BaseDirs::new().home.unwrap();
    std::fs::read(home.join(relative)).unwrap()
}

/// Writes one home destination directly and bypasses the runners.
fn write_home(relative: &str, text: &[u8]) {
    let home = BaseDirs::new().home.unwrap();
    common::seed_file(&home.join(relative), text);
}

#[test]
fn rescue_capture_saves_the_unmanaged_file() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-rescue-note", "managed");
    write_home("confit-rescue-note", b"original");
    let report = apply_project(dir.path());
    assert_eq!(report.saved, 1, "the apply saves the unmanaged file");
    assert_eq!(
        home_bytes("confit-rescue-note"),
        b"managed",
        "the apply writes the managed bytes"
    );
    restore_source(dir.path(), PathBuf::from("@!"));
    assert_eq!(
        home_bytes("confit-rescue-note"),
        b"original",
        "the rescue restore returns the original bytes"
    );
}

#[test]
fn rescue_second_apply_keeps_the_first_capture() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-rescue-keep-note", "managed");
    write_home("confit-rescue-keep-note", b"original");
    apply_project(dir.path());
    let report = apply_project(dir.path());
    assert_eq!(
        report.saved, 0,
        "the managed route stores no second capture"
    );
    restore_source(dir.path(), PathBuf::from("@!"));
    assert_eq!(
        home_bytes("confit-rescue-keep-note"),
        b"original",
        "the rescue restore returns the first capture"
    );
}

#[test]
fn rescue_empty_capture_writes_no_slot() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-rescue-absent-note", "managed");
    let report = apply_project(dir.path());
    assert_eq!(report.saved, 0, "the absent file stores no capture");
    let stores = common::test_stores(dir.path(), "site");
    let picker = Picker::Named("!".to_owned());
    assert!(
        stores.states().resolve(&picker).is_err(),
        "no capture leaves the rescue slot absent"
    );
}

#[test]
fn rescue_later_capture_replaces_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-rescue-first-note", "managed");
    write_home("confit-rescue-first-note", b"first");
    apply_project(dir.path());
    seed_project(dir.path(), "confit-rescue-second-note", "managed");
    write_home("confit-rescue-second-note", b"second");
    apply_project(dir.path());
    restore_source(dir.path(), PathBuf::from("@!"));
    assert_eq!(
        home_bytes("confit-rescue-second-note"),
        b"second",
        "the rescue slot holds the latest capture"
    );
}

#[test]
fn rescue_capture_includes_tree_members() {
    let dir = tempfile::tempdir().unwrap();
    seed_tree_project(dir.path(), "confit-rescue-tree", "member.txt", "managed");
    let home = BaseDirs::new().home.unwrap();
    let folder = home.join("confit-rescue-tree");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("member.txt"), b"original").unwrap();
    let report = apply_project(dir.path());
    assert_eq!(report.saved, 1, "the apply saves the tree member");
    restore_source(dir.path(), PathBuf::from("@!"));
    assert_eq!(
        std::fs::read(folder.join("member.txt")).unwrap(),
        b"original",
        "the rescue restore returns the member bytes"
    );
}

#[test]
fn rescue_missing_slot_fails() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-rescue-missing-note", "managed");
    let stores = common::test_stores(dir.path(), "site");
    let args = RestoreArgs {
        source: PathBuf::from("@!"),
        force: true,
    };
    let hooks = dir.path().join("hooks");
    assert!(
        restore::run(&args, stores, common::test_handles(), hooks).is_err(),
        "an absent rescue slot fails the restore"
    );
}
