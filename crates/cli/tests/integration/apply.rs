use crate::common::*;

#[test]
fn apply_yes_writes_all_files() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new("yes\n");
    let runner = apply_runner(
        sample_documents(),
        Bundle::empty(),
        Some(PathBuf::from("state.json")),
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    let report = match runner.execute() {
        Ok(report) => report,
        Err(error) => panic!("apply runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let app = memory_bytes(&fs, Path::new("app.toml"));
    let app_text = match String::from_utf8(app) {
        Ok(text) => text,
        Err(error) => panic!("app parses: {error}"),
    };
    assert!(app_text.contains("confit"), "toml holds value: {app_text}");
    assert_eq!(
        memory_bytes(&fs, Path::new("bin")),
        vec![0xFF, 0x00, 0x80, 0x41]
    );
    assert_eq!(
        fs.read_link(Path::new("shortcut")),
        Some(PathBuf::from("dest"))
    );
    assert!(fs.exists(Path::new("state.json")));
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].created_at.is_empty());
}

#[test]
fn apply_non_yes_writes_nothing() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    for answer in ["no\n", "YES\n", "\n"] {
        let mut input = Cursor::new(answer);
        let runner = apply_runner(
            sample_documents(),
            Bundle::empty(),
            Some(PathBuf::from("state.json")),
            false,
            true,
            confit_cli::actions::seams::Seams::memory(&fs, &mut input),
        );
        match runner.execute() {
            Ok(_) => panic!("{answer:?} proceeds"),
            Err(error) => assert!(
                error.to_string().contains("apply aborted"),
                "abort reads clear: {error}"
            ),
        }
    }
    assert!(!fs.exists(Path::new("note")));
    assert!(!fs.exists(Path::new("app.toml")));
    assert!(!fs.exists(Path::new("shortcut")));
    assert!(!fs.exists(Path::new("bin")));
    assert!(!fs.exists(Path::new("state.json")));
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert!(entries.is_empty());
}

#[test]
fn apply_force_skips_prompt() {
    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let runner = apply_runner(
        sample_documents(),
        Bundle::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("forced apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    assert_eq!(
        memory_bytes(&fs, Path::new("bin")),
        vec![0xFF, 0x00, 0x80, 0x41]
    );
}

#[test]
fn apply_drift_reprompts() {
    pin_home();
    let fs = MemoryFs::new();
    let mut seed_input = Cursor::new(String::new());
    let seed = apply_runner(
        sample_documents(),
        Bundle::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut seed_input),
    );
    match seed.execute() {
        Ok(_) => {}
        Err(error) => panic!("seed apply runs: {error}"),
    }
    let mut recorded = sample_documents();
    fill_hashes(&mut recorded);
    let mut previous = Bundle::empty();
    previous.manifest.documents = recorded;

    let mut input = DriftInjector {
        inner: Cursor::new(b"yes\nno\n".to_vec()),
        edits: Some(vec![(PathBuf::from("note"), b"hand edit\n".to_vec())]),
        fs: &fs,
    };
    let denied = apply_runner(
        sample_documents(),
        previous.clone(),
        None,
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    match denied.execute() {
        Ok(_) => panic!("drifted apply proceeds on no"),
        Err(error) => assert!(
            error.to_string().contains("apply aborted"),
            "abort reads clear: {error}"
        ),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hand edit\n");

    let mut input = DriftInjector {
        inner: Cursor::new(b"yes\nyes\n".to_vec()),
        edits: Some(vec![(
            PathBuf::from("app.toml"),
            b"name = \"meddled\"\n".to_vec(),
        )]),
        fs: &fs,
    };
    let retry = apply_runner(
        sample_documents(),
        previous,
        None,
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    match retry.execute() {
        Ok(_) => {}
        Err(error) => panic!("confirmed drift applies: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let app = memory_bytes(&fs, Path::new("app.toml"));
    let app_text = match String::from_utf8(app) {
        Ok(text) => text,
        Err(error) => panic!("app parses: {error}"),
    };
    assert!(
        app_text.contains("confit"),
        "apply restores drift: {app_text}"
    );
}

#[test]
fn apply_rotation_drops_sixth() {
    pin_home();
    let fs = MemoryFs::new();
    for _ in 0..6 {
        let mut input = Cursor::new(String::new());
        let runner = apply_runner(
            sample_documents(),
            Bundle::empty(),
            None,
            true,
            false,
            confit_cli::actions::seams::Seams::memory(&fs, &mut input),
        );
        match runner.execute() {
            Ok(_) => {}
            Err(error) => panic!("apply runs: {error}"),
        }
    }
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 5);
}

#[test]
fn apply_history_first_restores_just_previous() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let old = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "first\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("old plan builds: {error}"),
    };
    let new = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "second\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("new plan builds: {error}"),
    };
    let dir = match confit_core::store::resolve_previous_dir() {
        Ok(dir) => dir,
        Err(error) => panic!("history dir resolves: {error}"),
    };
    match confit_core::store::write_manifest(&old, Some(&dir.join("a-old.json")), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("old entry seeds: {error}"),
    }
    match confit_core::store::write_manifest(&new, Some(&dir.join("b-new.json")), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("new entry seeds: {error}"),
    }
    match fs.write(Path::new("note"), b"hand edit\n") {
        Ok(()) => {}
        Err(error) => panic!("hand edit lands: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("%1"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: false,
    };
    let mut input = Cursor::new("yes\n");
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => {}
        Err(error) => panic!("history apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"second\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot));
}

#[test]
fn apply_history_second_restores_older() {
    pin_home();
    let fs = MemoryFs::new();
    let old = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "first\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("old plan builds: {error}"),
    };
    let new = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "second\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("new plan builds: {error}"),
    };
    let dir = match confit_core::store::resolve_previous_dir() {
        Ok(dir) => dir,
        Err(error) => panic!("history dir resolves: {error}"),
    };
    match confit_core::store::write_manifest(&old, Some(&dir.join("a-old.json")), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("old entry seeds: {error}"),
    }
    match confit_core::store::write_manifest(&new, Some(&dir.join("b-new.json")), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("new entry seeds: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("%2"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => {}
        Err(error) => panic!("older apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"first\n");
}

#[test]
fn apply_history_out_of_range_names_count() {
    pin_home();
    let fs = MemoryFs::new();
    let built = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "only\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let dir = match confit_core::store::resolve_previous_dir() {
        Ok(dir) => dir,
        Err(error) => panic!("history dir resolves: {error}"),
    };
    match confit_core::store::write_manifest(&built, Some(&dir.join("a-only.json")), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("entry seeds: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("%2"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => panic!("out-of-range applies"),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("out of range"),
                "range reads clear: {message}"
            );
            assert!(
                message.contains("holding 1"),
                "count names itself: {message}"
            );
        }
    }
}

#[test]
fn apply_named_slot_restores() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "named\n".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    seed_named(&fs, "personal", &built);
    match fs.write(Path::new("note"), b"hand edit\n") {
        Ok(()) => {}
        Err(error) => panic!("hand edit lands: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("@personal"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: false,
    };
    let mut input = Cursor::new("yes\n");
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => {}
        Err(error) => panic!("named apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"named\n");
}

#[test]
fn apply_named_slot_absent_fails() {
    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("@missing"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => panic!("absent slot applies"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains("absent"), "absent reads clear: {message}");
            assert!(message.contains("@missing"), "name shows: {message}");
        }
    }
}

#[test]
fn apply_lua_positional_evaluates_profile() {
    use confit_core::fs::Filesystem;

    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("apply-lua-note", "probe\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let fs = MemoryFs::new();
    match fs.write(&profile, b"seed") {
        Ok(()) => {}
        Err(error) => panic!("profile seeds memory: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => {}
        Err(error) => panic!("lua apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("apply-lua-note")), b"probe\n");
}

#[test]
fn apply_cb_positional_loads_bundle() {
    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    match confit_core::store::write_bundle(&built, Path::new("backup.cb"), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("bundle writes: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("backup.cb"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input).with_print(print_tx);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("bundle positional runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    assert!(lines.is_empty(), "bundle skips preview");
}

#[test]
fn apply_then_drift_stays_quiet() {
    use confit_core::fs::{snapshot, snapshot_tree};

    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let runner = apply_runner(
        sample_documents(),
        Bundle::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let mut recorded = sample_documents();
    fill_hashes(&mut recorded);
    let mut previous = Bundle::empty();
    previous.manifest.documents = recorded;
    let drifts = previous.drift(
        &|path| snapshot(path, &fs),
        &|path| snapshot_tree(&path.expand(), &fs),
        DriftOrder::RecordedFirst,
    );
    assert!(drifts.is_empty(), "fresh apply shows no drift: {drifts:?}");
}

#[test]
fn apply_second_profile_removes_recorded_orphans() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    if let Err(error) = fs.write(Path::new("stray"), b"mine") {
        panic!("stray writes: {error}");
    }
    let mut input = Cursor::new(String::new());
    let first = apply_runner(
        sample_documents(),
        Bundle::empty(),
        Some(PathBuf::from("state.json")),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    match first.execute() {
        Ok(_) => {}
        Err(error) => panic!("first apply runs: {error}"),
    }
    if let Err(error) = fs.remove(Path::new("bin")) {
        panic!("bin pre-deletes: {error}");
    }
    let previous = match confit_core::store::load_state(Some(Path::new("state.json")), &fs) {
        Ok(previous) => previous,
        Err(error) => panic!("state loads: {error}"),
    };
    let desired = vec![
        ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hello again\n".to_string(),
                mode: None,
            },
        ),
        ManifestDocument::new(
            DocPath::new("app.toml"),
            ManifestData::Structured {
                format: confit_core::document::StructuredFormat::Toml,
                data: confit_core::document::Table::from([(
                    "name".to_string(),
                    serde_json::json!("confit"),
                )]),
            },
        ),
    ];
    let mut input = Cursor::new(String::new());
    let second = apply_runner(
        desired,
        previous,
        Some(PathBuf::from("state.json")),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input),
    );
    let report = match second.execute() {
        Ok(report) => report,
        Err(error) => panic!("second apply runs: {error}"),
    };
    assert_eq!(report.removed, 1);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello again\n");
    assert!(!fs.exists(Path::new("shortcut")), "link orphan removes");
    assert!(!fs.exists(Path::new("bin")), "absent orphan stays quiet");
    assert_eq!(memory_bytes(&fs, Path::new("stray")), b"mine");
}

#[test]
fn apply_emits_writing_plan_fact() {
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let (sender, receiver) = crossbeam_channel::unbounded();
    let mut input = Cursor::new(String::new());
    let runner = apply_runner(
        sample_documents(),
        Bundle::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input).with_progress(sender),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let mut seen = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        seen.push(event);
    }
    assert!(
        seen.iter().any(|event| matches!(
            event,
            confit_core::progress::Event::WritingPlan { documents: 4 }
        )),
        "writing plan fires with document count"
    );
    assert!(
        !seen
            .iter()
            .any(|event| matches!(event, confit_core::progress::Event::ReadingPlan { .. })),
        "memory apply reads no plan files"
    );
}

#[test]
fn apply_plan_file_without_profile_runs_on_file_alone() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    match confit_core::store::write_bundle(&built, Path::new("backup.cb"), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("plan writes: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("backup.cb"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,

            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("plan-file apply runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(report.removed, 0);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot), "plan file apply writes fixed slot only");
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].created_at.is_empty());
}

#[test]
fn two_profiles_share_one_slot_last_applied_wins() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    let mut first_input = Cursor::new(String::new());
    let first = apply_runner(
        vec![ManifestDocument::new(
            DocPath::new("first"),
            ManifestData::Text {
                content: "one\n".to_string(),
                mode: None,
            },
        )],
        Bundle::empty(),
        Some(slot.clone()),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut first_input),
    );
    match first.execute() {
        Ok(_) => {}
        Err(error) => panic!("first apply runs: {error}"),
    }
    let previous = match confit_core::store::load_state(Some(&slot), &fs) {
        Ok(previous) => previous,
        Err(error) => panic!("slot loads: {error}"),
    };
    assert_eq!(previous.manifest.documents.len(), 1);
    let mut second_input = Cursor::new(String::new());
    let second = apply_runner(
        vec![ManifestDocument::new(
            DocPath::new("second"),
            ManifestData::Text {
                content: "two\n".to_string(),
                mode: None,
            },
        )],
        previous,
        Some(slot.clone()),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut second_input),
    );
    let report = match second.execute() {
        Ok(report) => report,
        Err(error) => panic!("second apply runs: {error}"),
    };
    assert_eq!(report.removed, 1);
    assert!(
        !fs.exists(Path::new("first")),
        "orphan from first run removes"
    );
    assert_eq!(memory_bytes(&fs, Path::new("second")), b"two\n");
    let slot_plan = match confit_core::store::load_state(Some(&slot), &fs) {
        Ok(slot_plan) => slot_plan,
        Err(error) => panic!("slot reloads: {error}"),
    };
    assert_eq!(slot_plan.manifest.documents.len(), 1);
    assert_eq!(slot_plan.manifest.documents[0].path, DocPath::new("second"));
}

#[test]
fn named_plan_output_roundtrips_through_apply() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let dest = match confit_cli::cli::resolve_plan_file(Path::new("@work")) {
        Ok(dest) => dest,
        Err(error) => panic!("named output resolves: {error}"),
    };
    assert!(dest.ends_with("confit/plans/work.json"));
    match confit_core::store::write_manifest(&built, Some(&dest), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("named plan writes: {error}"),
    }
    let reloaded = match confit_core::store::load_state(Some(&dest), &fs) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("named plan loads: {error}"),
    };
    assert_eq!(reloaded.manifest.documents.len(), 4);
    let args = confit_cli::cli::ApplyArgs {
        source: PathBuf::from("@work"),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("named plan applies: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot), "named apply writes fixed slot");
}

#[test]
fn apply_bundle_populates_pool() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let bundle = PathBuf::from("proof.cb");
    match confit_core::store::write_bundle(&built, &bundle, &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("bundle writes: {error}"),
    }
    let pool = match confit_core::store::resolve_blobs_dir() {
        Ok(pool) => pool,
        Err(error) => panic!("pool resolves: {error}"),
    };
    let before = match fs.list_dir(&pool) {
        Ok(before) => before,
        Err(error) => panic!("pool lists: {error}"),
    };
    assert!(before.is_empty(), "bundle plans start with no pool entries");
    let args = confit_cli::cli::ApplyArgs {
        source: bundle,
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("bundle apply runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    assert_eq!(
        memory_bytes(&fs, Path::new("bin")),
        vec![0xFF, 0x00, 0x80, 0x41]
    );
    let sha = confit_core::plan::sha256_hex(&[0xFF, 0x00, 0x80, 0x41]);
    assert!(
        fs.exists(&pool.join(&sha)),
        "applying a bundle populates the pool"
    );
}

#[test]
fn apply_rotation_prunes_exclusive_blobs() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    let shared_bytes = b"shared-confit-blob".to_vec();
    let shared_sha = confit_core::plan::sha256_hex(&shared_bytes);
    let mut exclusive_shas = Vec::new();
    for generation in 0..6 {
        let unique_bytes = format!("unique-confit-blob-{generation}").into_bytes();
        let unique_sha = confit_core::plan::sha256_hex(&unique_bytes);
        exclusive_shas.push(unique_sha.clone());
        let desired = vec![
            ManifestDocument::new(
                DocPath::new("shared.bin"),
                ManifestData::Opaque {
                    blob: shared_sha.clone(),
                    mode: None,
                },
            ),
            ManifestDocument::new(
                DocPath::new("unique.bin"),
                ManifestData::Opaque {
                    blob: unique_sha.clone(),
                    mode: None,
                },
            ),
        ];
        let mut plan = match build(desired) {
            Ok(plan) => plan,
            Err(error) => panic!("plan builds: {error}"),
        };
        plan.blobs.insert(shared_sha.clone(), shared_bytes.clone());
        plan.blobs.insert(unique_sha, unique_bytes);
        let mut input = Cursor::new(String::new());
        let runner = confit_cli::actions::apply::ApplyRunner {
            plan,
            previous: Bundle::empty(),
            state: Some(slot.clone()),
            force: true,
            preview: false,
            seams: confit_cli::actions::seams::Seams::memory(&fs, &mut input),
        };
        match runner.execute() {
            Ok(_) => {}
            Err(error) => panic!("apply {generation} runs: {error}"),
        }
    }
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 5);
    let pool = match confit_core::store::resolve_blobs_dir() {
        Ok(pool) => pool,
        Err(error) => panic!("pool resolves: {error}"),
    };
    let dropped = match exclusive_shas.first() {
        Some(dropped) => dropped,
        None => panic!("exclusive shas record"),
    };
    assert!(
        !fs.exists(&pool.join(dropped)),
        "rotated-out bytes leave the pool"
    );
    assert!(
        fs.exists(&pool.join(&shared_sha)),
        "shared bytes stay pooled"
    );
    for (generation, sha) in exclusive_shas.iter().enumerate().skip(1) {
        assert!(
            fs.exists(&pool.join(sha)),
            "kept generation {generation} stays pooled"
        );
    }
    assert_eq!(
        memory_bytes(&fs, Path::new("unique.bin")),
        b"unique-confit-blob-5"
    );
}

#[test]
fn confirm_accepts_only_literal_yes() {
    pin_home();
    let fs = MemoryFs::new();
    for (answer, want) in [("yes\n", true), ("no\n", false), ("\n", false)] {
        let mut input = Cursor::new(answer);
        let mut seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input);
        let got = match seams.confirm() {
            Ok(got) => got,
            Err(error) => panic!("confirm reads: {error}"),
        };
        assert_eq!(got, want, "answer decides: {answer:?}");
    }
}
