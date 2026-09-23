use crate::common::*;

#[test]
fn memory_snapshot_covers_present_absent_unreadable() {
    use confit_core::fs::{Filesystem, memory::MemoryFs, snapshot::snapshot};

    let mut fs = MemoryFs::new();
    match fs.write(Path::new("present"), b"bytes") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    fs.mark_unreadable(Path::new("denied"));
    assert!(matches!(
        snapshot(&DocPath::new("present"), &fs),
        ReadOutcome::Present { .. }
    ));
    assert!(matches!(
        snapshot(&DocPath::new("missing"), &fs),
        ReadOutcome::Absent
    ));
    assert!(matches!(
        snapshot(&DocPath::new("denied"), &fs),
        ReadOutcome::Unreadable { .. }
    ));
}

#[test]
fn drift_reports_manual_edits_on_memory_fs() {
    use confit_core::document::{StructuredFormat, Table};
    use confit_core::fs::{
        Filesystem,
        memory::MemoryFs,
        snapshot::{snapshot_document, snapshot_tree},
    };

    pin_home();
    let mut recorded_docs = vec![
        ManifestDocument::new(
            DocPath::new("app.toml"),
            ManifestData::Structured {
                format: StructuredFormat::Toml,
                data: Table::from([
                    ("name".to_string(), serde_json::json!("old")),
                    ("gone".to_string(), serde_json::json!("yes")),
                ]),
            },
        ),
        ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hello\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("vanished"),
            ManifestData::Text {
                content: "bye".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
    ];
    fill_hashes(&mut recorded_docs);
    let mut previous = Bundle::empty();
    previous.manifest.documents = recorded_docs;
    let fs = MemoryFs::new();
    match fs.write(
        Path::new("app.toml"),
        b"name = \"new\"\nadded = \"fresh\"\n",
    ) {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match fs.write(Path::new("note"), b"hello world\n") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    let drifts = previous.drift(
        &|document| snapshot_document(document, &fs),
        &|path| snapshot_tree(&path.expand(), &fs),
        DriftOrder::RecordedFirst,
        &fs,
    );
    let built = Bundle::empty();
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &previous,
        drift: &drifts,
        first_run: false,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    let text = report.render();
    assert!(
        text.contains("~ app.toml: name = old -> new"),
        "key diff points at changed key: {text}"
    );
    assert!(
        text.contains("vanished: manually deleted."),
        "missing path reads as deleted: {text}"
    );
    assert!(
        text.contains("changed outside config: add to config or the next apply loses them"),
        "drift framing survives: {text}"
    );
}

#[test]
fn plan_shows_old_to_new_on_updates() {
    use confit_core::document::{StructuredFormat, Table};

    let mut old_docs = vec![ManifestDocument::new(
        DocPath::new("app.toml"),
        ManifestData::Structured {
            format: StructuredFormat::Toml,
            data: Table::from([("name".to_string(), serde_json::json!("old"))]),
        },
    )];
    fill_hashes(&mut old_docs);
    let mut previous = Bundle::empty();
    previous.manifest.documents = old_docs;
    let desired = vec![ManifestDocument::new(
        DocPath::new("app.toml"),
        ManifestData::Structured {
            format: StructuredFormat::Toml,
            data: Table::from([("name".to_string(), serde_json::json!("new"))]),
        },
    )];
    let built = match Bundle::build(desired, Vec::new()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    assert_eq!(built.summary(&previous).update, 1);
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &previous,
        drift: &[],
        first_run: false,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    let text = report.render();
    assert!(
        text.contains("~ app.toml: toml"),
        "update header carries its sigil: {text}"
    );
    assert!(
        text.contains("~ name = old -> new"),
        "update shows old to new: {text}"
    );
}

#[test]
fn first_run_preview_shows_impact_plus_in_place() {
    use confit_core::fs::{
        Filesystem,
        snapshot::{snapshot_document, snapshot_tree},
    };

    pin_home();
    let fs = MemoryFs::new();
    match fs.write(Path::new("same"), b"kept\n") {
        Ok(()) => {}
        Err(error) => panic!("same seeds: {error}"),
    }
    match fs.write(Path::new("clash"), b"disk\n") {
        Ok(()) => {}
        Err(error) => panic!("clash seeds: {error}"),
    }
    let desired = vec![
        ManifestDocument::new(
            DocPath::new("same"),
            ManifestData::Text {
                content: "kept\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("clash"),
            ManifestData::Text {
                content: "desired\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("gone"),
            ManifestData::Text {
                content: "fresh\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
    ];
    let built = match Bundle::build(desired.clone(), Vec::new()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    let drift = built.drift(
        &|document| snapshot_document(document, &fs),
        &|path| snapshot_tree(&path.expand(), &fs),
        DriftOrder::DiskFirst,
        &fs,
    );
    let empty = Bundle::empty();
    let steady = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &empty,
        drift: &[],
        first_run: false,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    let first = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &empty,
        drift: &drift,
        first_run: true,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    assert!(steady.render().contains("to change"));
    assert_eq!(
        first.summary_lines(),
        vec!["Documents: 1 to add, 1 to change, 0 to destroy.".to_string()]
    );
    let text = first.render();
    assert!(
        text.contains("1 to add, 1 to change, 0 to destroy"),
        "{text}"
    );
    assert!(text.contains("+ gone: text"), "create header shows: {text}");
    assert!(
        !text.contains("changed outside config"),
        "outside wording stays out: {text}"
    );

    let slot = PathBuf::from("state.json");
    assert!(!fs.exists(&slot), "slot reads absent for first run");
    let mut input = Cursor::new("yes\n");
    let runner = apply_runner(
        desired,
        Bundle::empty(),
        Some(slot),
        false,
        confit_cli::seams::Seams::memory(&fs, &*EMPTY_PROBE, &mut input),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("first apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("clash")), b"desired\n");
}

#[test]
fn steady_plan_flow_pins_recorded_headers_through_drift_and_preview() {
    use confit_core::drift::Drift;
    use confit_core::fs::{
        Filesystem,
        snapshot::{snapshot_document, snapshot_tree},
    };

    pin_home();
    let fs = MemoryFs::new();
    match fs.write(Path::new("order-pin-note"), b"disk\n") {
        Ok(()) => {}
        Err(error) => panic!("disk seeds: {error}"),
    }
    let mut recorded_docs = vec![ManifestDocument::new(
        DocPath::new("order-pin-note"),
        ManifestData::Text {
            content: "recorded\n".to_string(),
            mode: None,
            unmanaged: false,
        },
    )];
    fill_hashes(&mut recorded_docs);
    let mut previous = Bundle::empty();
    previous.manifest.documents = recorded_docs;
    let drifts = previous.drift(
        &|document| snapshot_document(document, &fs),
        &|path| snapshot_tree(&path.expand(), &fs),
        DriftOrder::RecordedFirst,
        &fs,
    );
    assert_eq!(drifts.len(), 1);
    match &drifts[0] {
        Drift::Hunk { hunks, .. } => {
            assert!(
                hunks.contains("-recorded"),
                "steady drift removes recorded: {hunks}"
            );
            assert!(hunks.contains("+disk"), "steady drift adds disk: {hunks}");
            assert!(
                !hunks.contains("+recorded"),
                "steady drift never adds recorded: {hunks}"
            );
            assert!(
                !hunks.contains("-disk"),
                "steady drift never removes disk: {hunks}"
            );
        }
        _ => panic!("steady flow pins hunk drift"),
    }
    let built = match Bundle::build(
        vec![ManifestDocument::new(
            DocPath::new("order-pin-note"),
            ManifestData::Text {
                content: "recorded\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        )],
        Vec::new(),
    ) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &previous,
        drift: &drifts,
        first_run: false,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    let text = report.render();
    assert!(
        text.contains("Changes outside Confit will be overwritten on next apply"),
        "drift title leads: {text}"
    );
    assert!(
        text.contains("~ order-pin-note: text"),
        "drift header carries its sigil: {text}"
    );
    assert!(
        !text.contains("---"),
        "steady summary renders no file markers: {text}"
    );
    assert!(
        !text.contains("+++"),
        "steady summary renders no new markers: {text}"
    );
    assert!(
        !text.contains("@@"),
        "steady summary renders no range markers: {text}"
    );
    assert!(
        text.contains("-recorded"),
        "steady summary shows removed content: {text}"
    );
    assert!(
        text.contains("+disk"),
        "steady summary shows added content: {text}"
    );
    match fs.write(Path::new("steady-state.json"), b"slot") {
        Ok(()) => {}
        Err(error) => panic!("slot seeds: {error}"),
    }
    let mut input = Cursor::new(String::new());
    let runner = apply_runner(
        vec![ManifestDocument::new(
            DocPath::new("order-pin-note"),
            ManifestData::Text {
                content: "recorded\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        )],
        previous,
        Some(PathBuf::from("steady-state.json")),
        true,
        confit_cli::seams::Seams::memory(&fs, &*EMPTY_PROBE, &mut input),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("steady preview runs: {error}"),
    }
}

#[test]
fn first_run_flow_pins_desired_headers_through_drift_and_preview() {
    use confit_core::drift::Drift;
    use confit_core::fs::{
        Filesystem,
        snapshot::{snapshot_document, snapshot_tree},
    };

    pin_home();
    let fs = MemoryFs::new();
    match fs.write(Path::new("order-pin-note"), b"disk\n") {
        Ok(()) => {}
        Err(error) => panic!("disk seeds: {error}"),
    }
    let desired = vec![ManifestDocument::new(
        DocPath::new("order-pin-note"),
        ManifestData::Text {
            content: "desired\n".to_string(),
            mode: None,
            unmanaged: false,
        },
    )];
    let built = match Bundle::build(desired.clone(), Vec::new()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    let drifts = built.drift(
        &|document| snapshot_document(document, &fs),
        &|path| snapshot_tree(&path.expand(), &fs),
        DriftOrder::DiskFirst,
        &fs,
    );
    assert_eq!(drifts.len(), 1);
    match &drifts[0] {
        Drift::Hunk { hunks, .. } => {
            assert!(
                hunks.contains("-disk"),
                "first-run drift removes disk: {hunks}"
            );
            assert!(
                hunks.contains("+desired"),
                "first-run drift adds desired: {hunks}"
            );
            assert!(
                !hunks.contains("+disk"),
                "first-run drift never adds disk: {hunks}"
            );
            assert!(
                !hunks.contains("-desired"),
                "first-run drift never removes desired: {hunks}"
            );
        }
        _ => panic!("first-run flow pins hunk drift"),
    }
    let empty = Bundle::empty();
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &empty,
        drift: &drifts,
        first_run: true,
        hooks: confit_cli::presentation::summary::Hooks {
            lifecycle: &[],
            evaluated: &[],
        },
    };
    let text = report.render();
    assert!(
        !text.contains("---"),
        "first-run summary renders no file markers: {text}"
    );
    assert!(
        !text.contains("+++"),
        "first-run summary renders no new markers: {text}"
    );
    assert!(
        !text.contains("@@"),
        "first-run summary renders no range markers: {text}"
    );
    assert!(
        text.contains("-disk"),
        "first-run summary shows removed content: {text}"
    );
    assert!(
        text.contains("+desired"),
        "first-run summary shows added content: {text}"
    );
    let mut input = Cursor::new(String::new());
    let runner = apply_runner(
        desired,
        Bundle::empty(),
        Some(PathBuf::from("first-run-state.json")),
        true,
        confit_cli::seams::Seams::memory(&fs, &*EMPTY_PROBE, &mut input),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("first-run preview runs: {error}"),
    }
}
