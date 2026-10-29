//! Apply runs over temp project roots.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::{seed_file, test_sinks, test_stores};
use confit_cli::actions::apply::ApplyRunner;
use confit_cli::cli::ApplyArgs;
use confit_core::error::TerminalResult;
use confit_driver::paths::BaseDirs;

/// Build one profile with a single text document at the relative path.
fn profile_text(relative: &str) -> String {
    format!(
        r#"
return {{
    configs = {{
        {{
            name = id("note"),
            documents = {{ {{ path = route("home", "{relative}"), text = "hello" }} }},
        }},
    }},
}}
"#
    )
}

/// Seed one project with the profile text.
fn seed_project(dir: &Path, relative: &str) {
    let site = dir.join("site");
    seed_file(&site.join("main.lua"), profile_text(relative).as_bytes());
}

/// Run one apply with the input bytes and the force flag.
fn run_apply(dir: &Path, input: &[u8], force: bool) -> TerminalResult<usize> {
    let site = dir.join("site");
    let stores = test_stores(dir, "site");
    let args = ApplyArgs {
        source: site,
        profile: None,
        force,
    };
    let mut input = std::io::BufReader::new(input);
    let hooks = dir.join("hooks");
    ApplyRunner::from_args(&args, &mut input, stores, test_sinks(), hooks)
        .and_then(ApplyRunner::execute)
        .map(|report| report.written)
}

/// Read one home destination under the shared test root.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = BaseDirs::new().home.unwrap();
    std::fs::read(home.join(relative)).unwrap()
}

/// Report whether one home destination stays absent.
fn home_absent(relative: &str) -> bool {
    let home = BaseDirs::new().home.unwrap();
    !home.join(relative).exists()
}

#[test]
fn apply_force_writes_without_prompt() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-force-note");
    match run_apply(dir.path(), b"", true) {
        Ok(written) => assert_eq!(written, 1, "one document lands"),
        Err(error) => panic!("forced apply runs: {error}"),
    }
    assert_eq!(
        home_bytes("confit-force-note"),
        b"hello",
        "the destination holds the text"
    );
}

#[test]
fn apply_yes_writes_documents() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-yes-note");
    match run_apply(dir.path(), b"yes\n", false) {
        Ok(written) => assert_eq!(written, 1, "one document lands"),
        Err(error) => panic!("confirmed apply runs: {error}"),
    }
    assert_eq!(
        home_bytes("confit-yes-note"),
        b"hello",
        "the destination holds the text"
    );
}

#[test]
fn apply_no_leaves_disk_alone() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-no-note");
    match run_apply(dir.path(), b"no\n", false) {
        Ok(written) => assert_eq!(written, 0, "refused apply writes nothing"),
        Err(error) => panic!("refused apply stays quiet: {error}"),
    }
    assert!(
        home_absent("confit-no-note"),
        "the refused destination stays absent"
    );
}
