//! Apply runs over temp project roots.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::{seed_file, test_sinks, test_stores};
use confit_cli::actions::apply::ApplyRunner;
use confit_cli::cli::ApplyArgs;
use confit_cli::error::CliError;
use confit_driver::fs::TestGuard;

/// Build one profile with a single text document at the relative path.
fn profile_text(relative: &str) -> String {
    format!(
        r#"
return {{
    configs = {{
        {{
            name = "note",
            documents = {{ [{{ base = "home", relative = "{relative}" }}] = {{ text = "hello" }} }},
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

/// Run one apply with the input answer and the force flag.
fn run_apply(dir: &Path, answer: &[u8], force: bool) -> Result<usize, CliError> {
    let site = dir.join("site");
    let stores = test_stores(dir, "site");
    let args = ApplyArgs {
        source: site,
        profile: None,
        force,
    };
    let mut input = std::io::BufReader::new(answer);
    let hooks = dir.join("hooks");
    ApplyRunner::from_args(&args, &mut input, stores, test_sinks(), hooks)
        .and_then(ApplyRunner::execute)
        .map(|report| report.written)
}

/// Read one home destination through the memory backend.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = dirs::home_dir().unwrap();
    confit_driver::fs::read(&home.join(relative)).unwrap()
}

#[test]
fn apply_force_writes_without_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
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
    let _guard = TestGuard::install();
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
    let _guard = TestGuard::install();
    seed_project(dir.path(), "confit-no-note");
    match run_apply(dir.path(), b"no\n", false) {
        Err(CliError::Aborted) => {}
        Err(other) => panic!("refused apply flattens: {other}"),
        Ok(_) => panic!("refused apply passes"),
    }
    let home = dirs::home_dir().unwrap();
    assert!(
        !confit_driver::fs::exists(&home.join("confit-no-note")),
        "the refused destination stays absent"
    );
}
