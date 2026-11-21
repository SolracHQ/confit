//! Apply runs over temporary project roots.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::{apply, commit::WriteReport};
use confit_cli::cli::ApplyArgs;
use confit_core::error::TerminalResult;
use confit_driver::paths::BaseDirs;
use confit_events::{
    EventHandles,
    protocol::{FromUi, ToUi},
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

/// Builds UI handles whose answers come from a canned list.
fn replying_handles(replies: Vec<String>) -> EventHandles {
    let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
    let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
    std::thread::spawn(move || {
        let mut replies = replies.into_iter();
        for message in incoming {
            match message {
                ToUi::Prompt(_) => {
                    let line = replies.next().unwrap_or_default();
                    if outgoing.send(FromUi::Answer(line)).is_err() {
                        break;
                    }
                }
                ToUi::ShowAndConfirm { force, .. } => {
                    if force {
                        if outgoing.send(FromUi::Approved).is_err() {
                            break;
                        }
                        continue;
                    }
                    let line = replies.next().unwrap_or_default();
                    let reply = if line.trim() == "yes" {
                        FromUi::Approved
                    } else {
                        FromUi::Close
                    };
                    if outgoing.send(reply).is_err() {
                        break;
                    }
                }
                ToUi::Done { .. } => {}
                _ => {}
            }
        }
    });
    EventHandles::new(to_ui, from_ui)
}

/// Builds a profile that writes one text document at the relative path.
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

/// Seeds the site profile with the document text.
fn seed_project(dir: &Path, relative: &str) {
    let site = dir.join("site");
    common::seed_file(&site.join("main.lua"), profile_text(relative).as_bytes());
}

/// Applies the project with the canned replies and returns the report.
fn run_apply(
    dir: &Path,
    replies: &[&str],
    force: bool,
    profile: Option<PathBuf>,
) -> TerminalResult<WriteReport> {
    let site = dir.join("site");
    let stores = test_stores(dir, "site");
    let args = ApplyArgs {
        project: site,
        profile,
        force,
    };
    let replies = replies.iter().map(ToString::to_string).collect();
    let hooks = dir.join("hooks");
    apply::run(&args, stores, replying_handles(replies), hooks)
}

/// Reads one home destination under the shared test root.
fn home_bytes(relative: &str) -> Vec<u8> {
    let home = BaseDirs::new().home.unwrap();
    std::fs::read(home.join(relative)).unwrap()
}

/// Returns whether one home destination is absent.
fn home_absent(relative: &str) -> bool {
    let home = BaseDirs::new().home.unwrap();
    !home.join(relative).exists()
}

#[test]
fn apply_force_skips_the_prompt() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-force-note");
    match run_apply(dir.path(), &[], true, None) {
        Ok(report) => {
            assert_eq!(report.applied.added, 1, "the apply adds one document");
            assert_eq!(report.applied.changed, 0, "the apply reports zero changes");
        }
        Err(error) => panic!("the forced apply fails to run: {error}"),
    }
    assert_eq!(
        home_bytes("confit-force-note"),
        b"hello",
        "the destination holds the text"
    );
}

#[test]
fn yes_reply_writes_the_documents() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-yes-note");
    match run_apply(dir.path(), &["yes"], false, None) {
        Ok(report) => {
            assert_eq!(report.applied.added, 1, "the apply adds one document");
            assert_eq!(report.applied.changed, 0, "the apply reports zero changes");
        }
        Err(error) => panic!("the confirmed apply fails to run: {error}"),
    }
    assert_eq!(
        home_bytes("confit-yes-note"),
        b"hello",
        "the destination holds the text"
    );
}

#[test]
fn no_reply_keeps_the_disk_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-no-note");
    match run_apply(dir.path(), &["no"], false, None) {
        Ok(report) => {
            assert_eq!(
                report.applied.added, 0,
                "the refused apply reports zero additions"
            );
            assert_eq!(
                report.applied.changed, 0,
                "the refused apply reports zero changes"
            );
        }
        Err(error) => panic!("the refused apply fails to run: {error}"),
    }
    assert!(
        home_absent("confit-no-note"),
        "the refused destination is absent"
    );
}

#[test]
fn apply_explicit_profile_overrides_the_default() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path(), "confit-main-note");
    let site = dir.path().join("site");
    common::seed_file(
        &site.join("laptop.lua"),
        profile_text("confit-alt-note").as_bytes(),
    );
    match run_apply(dir.path(), &[], true, Some(PathBuf::from("laptop.lua"))) {
        Ok(report) => {
            assert_eq!(report.applied.added, 1, "the apply adds one document");
            assert_eq!(report.applied.changed, 0, "the apply reports zero changes");
        }
        Err(error) => panic!("the explicit profile apply fails to run: {error}"),
    }
    assert_eq!(
        home_bytes("confit-alt-note"),
        b"hello",
        "the chosen profile writes the document"
    );
    assert!(
        home_absent("confit-main-note"),
        "only the chosen profile writes"
    );
}
