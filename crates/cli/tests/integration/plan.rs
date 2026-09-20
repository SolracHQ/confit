use crate::common::*;

#[test]
fn plan_file_feeds_state_roundtrip() {
    use confit_core::fs::{Filesystem, MemoryFs};

    let built = match build(vec![ManifestDocument::new(
        DocPath::new("note"),
        ManifestData::Text {
            content: "hi".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let text = match confit_core::store::manifest::manifest_json(&built) {
        Ok(text) => text,
        Err(error) => panic!("plan serializes: {error}"),
    };
    let fs = MemoryFs::new();
    match fs.write(Path::new("plan.json"), text.as_bytes()) {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match fs.write(Path::new("state.json"), text.as_bytes()) {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    let state = match confit_core::store::slots::load_state(Some(Path::new("state.json")), &fs) {
        Ok(state) => state,
        Err(error) => panic!("plan feeds state: {error}"),
    };
    assert_eq!(state.manifest.documents.len(), 1);
    assert_eq!(state.manifest.version, BUNDLE_VERSION);
    assert!(!state.manifest.documents[0].data_hash.is_empty());
    let rebuilt = match confit_core::plan::Bundle::build(
        vec![ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hi".to_string(),
                mode: None,
            },
        )],
        Vec::new(),
    ) {
        Ok(built) => built,
        Err(error) => panic!("plan rebuilds: {error}"),
    };
    let summary = rebuilt.summary(&state);
    assert_eq!(summary.update, 0);
    assert_eq!(summary.create, 0);
}

#[test]
fn stale_state_version_fails_as_unsupported() {
    use confit_core::fs::{Filesystem, MemoryFs};

    let fs = MemoryFs::new();
    match fs.write(Path::new("state.json"), b"{\"version\":1,\"documents\":[]}") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match confit_core::store::slots::load_state(Some(Path::new("state.json")), &fs) {
        Ok(_) => panic!("stale version passes"),
        Err(error) => assert_eq!(
            error.to_string(),
            format!("state version 1 reads unsupported, want {BUNDLE_VERSION}")
        ),
    }
}

#[test]
fn missing_state_version_fails_as_plan_error() {
    use confit_core::fs::{Filesystem, MemoryFs};

    let fs = MemoryFs::new();
    match fs.write(Path::new("state.json"), b"{\"documents\":[]}") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match confit_core::store::slots::load_state(Some(Path::new("state.json")), &fs) {
        Ok(_) => panic!("missing version passes"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains("version"), "version named: {message}");
            assert!(message.contains("state.json"), "names file: {message}");
        }
    }
}

#[test]
fn plan_without_output_stores_tmp_payload() {
    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("tmp-probe-note", "probe\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: None,
    };
    let fs = confit_cli::fs::OsFs;
    let mut input = Cursor::new(String::new());
    let plan_runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: true,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input),
    };
    let outcome = match plan_runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs: {error}"),
    };
    let stored = match outcome.stored {
        Some(stored) => stored,
        None => panic!("tmp plan stores"),
    };
    assert!(
        stored
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("confit-plan-"))
    );
    assert!(
        stored
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb")),
        "tmp payload reads as a bundle: {}",
        stored.display()
    );
    let plan = match confit_core::store::bundle::read_bundle(&stored, &confit_cli::fs::OsFs) {
        Ok(plan) => plan,
        Err(error) => panic!("tmp bundle reads: {error}"),
    };
    assert_eq!(plan.manifest.documents.len(), 1);
    assert_eq!(plan.manifest.version, BUNDLE_VERSION);
    assert_eq!(plan_value(&plan), plan_value(&outcome.built));
    match std::fs::remove_file(&stored) {
        Ok(()) => {}
        Err(error) => panic!("tmp plan cleans: {error}"),
    }
}

#[test]
fn plan_without_output_nor_tmp_stores_nothing() {
    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("tmp-probe-note", "probe\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: None,
    };
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let plan_runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input),
    };
    let outcome = match plan_runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs: {error}"),
    };
    assert!(outcome.stored.is_none(), "bare plan stores nothing");
    assert_eq!(outcome.built.manifest.documents.len(), 1);
}

#[test]
fn plan_reads_fixed_slot() {
    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("tmp-slot-note", "probe\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let slot = match confit_core::store::slots::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    let home = match dirs::home_dir() {
        Some(home) => home,
        None => panic!("home resolves"),
    };
    assert!(slot.starts_with(&home), "slot lives under home");
    assert!(
        slot.ends_with("confit/state.json"),
        "slot holds one fixed name"
    );
    let seeded = serde_json::json!({
        "version": BUNDLE_VERSION,
        "documents": [
            {"path": "tmp-slot-note", "data": {"text": {"content": "old\n"}}, "data_hash": ""}
        ],
    });
    if let Some(parent) = slot.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        panic!("slot dir builds: {error}");
    }
    if let Err(error) = std::fs::write(&slot, serde_json::to_string(&seeded).unwrap_or_default()) {
        panic!("slot seeds: {error}");
    }
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: None,
    };
    let fs = confit_cli::fs::OsFs;
    let mut input = Cursor::new(String::new());
    let slot_runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input),
    };
    let outcome = match slot_runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs: {error}"),
    };
    assert_eq!(outcome.previous.manifest.documents.len(), 1);
    assert_eq!(outcome.built.summary(&outcome.previous).update, 1);
    match std::fs::remove_file(&slot) {
        Ok(()) => {}
        Err(error) => panic!("slot cleans: {error}"),
    }
}

#[test]
fn named_plan_rejects_bare_separator_and_parent() {
    pin_home();
    for raw in ["@", "@a/b", "@.."] {
        match confit_cli::cli::resolve_plan_file(Path::new(raw)) {
            Ok(_) => panic!("{raw:?} passes"),
            Err(error) => assert!(!error.to_string().is_empty()),
        }
    }
}

#[test]
fn plan_file_output_roundtrips_as_bundle() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let dest = Path::new("proof.cb");
    match confit_core::store::bundle::write_bundle(&built, dest, &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("bundle writes: {error}"),
    }
    let restored = match confit_core::store::bundle::read_bundle(dest, &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("bundle reads: {error}"),
    };
    assert_eq!(restored.manifest.documents.len(), 4);
    assert_eq!(plan_value(&restored), plan_value(&built));
    let pool = match confit_core::store::blobs::resolve_blobs_dir() {
        Ok(pool) => pool,
        Err(error) => panic!("pool resolves: {error}"),
    };
    let entries = match fs.list_dir(&pool) {
        Ok(entries) => entries,
        Err(error) => panic!("pool lists: {error}"),
    };
    assert!(entries.is_empty(), "file outputs populate no pool entries");
}

#[test]
fn plan_manifest_json_roundtrips() {
    pin_home();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let text = match confit_core::store::manifest::manifest_json(&built) {
        Ok(text) => text,
        Err(error) => panic!("manifest renders: {error}"),
    };
    let manifest: confit_core::store::manifest::Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => panic!("manifest parses: {error}"),
    };
    assert_eq!(manifest.version, BUNDLE_VERSION);
    assert_eq!(manifest.documents.len(), 4);
    assert!(
        text.contains("\"blob\""),
        "opaque payloads persist as blob refs: {text}"
    );
}

#[test]
fn plan_named_output_lands_slot_manifest_plus_pool() {
    use confit_core::fs::Filesystem;

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
    match confit_core::store::slots::write_manifest(&built, Some(&dest), &fs, None) {
        Ok(()) => {}
        Err(error) => panic!("named plan writes: {error}"),
    }
    let reloaded = match confit_core::store::slots::load_state(Some(&dest), &fs) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("named plan loads: {error}"),
    };
    assert_eq!(plan_value(&reloaded), plan_value(&built));
    let pool = match confit_core::store::blobs::resolve_blobs_dir() {
        Ok(pool) => pool,
        Err(error) => panic!("pool resolves: {error}"),
    };
    let sha = confit_core::plan::sha256_hex(&[0xFF, 0x00, 0x80, 0x41]);
    assert!(fs.exists(&pool.join(&sha)), "slot writes store their blobs");
}

#[test]
fn plan_runner_memory_seams_reach_sink_and_bundle() {
    use confit_core::fs::Filesystem;

    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("seam-note", "seam\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let fs = MemoryFs::new();
    let (sender, receiver) = crossbeam_channel::unbounded();
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: Some(PathBuf::from("seam-out-order-pin")),
    };
    let mut input = Cursor::new(String::new());
    let runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input).with_progress(sender),
    };
    let outcome = match runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs on memory seams: {error}"),
    };
    assert_eq!(outcome.built.manifest.documents.len(), 1);
    assert!(outcome.first_run, "memory slot reads absent for first run");
    let mut seen = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        seen.push(event);
    }
    assert!(
        seen.iter()
            .any(|event| matches!(event, confit_core::progress::Event::ReadingPlan { .. })),
        "reading plan reaches the injected sink"
    );
    assert!(
        seen.iter()
            .any(|event| matches!(event, confit_core::progress::Event::Hashing)),
        "hashing reaches the injected sink"
    );
    assert!(
        seen.iter().any(|event| matches!(
            event,
            confit_core::progress::Event::WritingPlan { documents: 1 }
        )),
        "writing plan reaches the injected sink"
    );
    assert!(
        fs.exists(Path::new("seam-out-order-pin.cb")),
        "bare output gains the bundle suffix in memory"
    );
    assert!(
        !fs.exists(Path::new("seam-out-order-pin")),
        "bare output writes no suffixless file"
    );
    let restored =
        match confit_core::store::bundle::read_bundle(Path::new("seam-out-order-pin.cb"), &fs) {
            Ok(restored) => restored,
            Err(error) => panic!("memory bundle reads: {error}"),
        };
    assert_eq!(restored.manifest.documents.len(), 1);
    assert!(
        !Path::new("seam-out-order-pin.cb").exists(),
        "memory run writes no host file"
    );
}

#[test]
fn plan_runner_named_output_exempt_keeps_slot() {
    use confit_core::fs::Filesystem;

    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_document(confit.document.text("seam-slot-note", "seam\n"))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let fs = MemoryFs::new();
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: Some(PathBuf::from("@seam-slot-order-pin")),
    };
    let mut input = Cursor::new(String::new());
    let runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input),
    };
    let outcome = match runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("named plan runs on memory seams: {error}"),
    };
    assert_eq!(outcome.built.manifest.documents.len(), 1);
    let slot = match confit_cli::cli::resolve_plan_file(Path::new("@seam-slot-order-pin")) {
        Ok(slot) => slot,
        Err(error) => panic!("named output resolves: {error}"),
    };
    assert!(fs.exists(&slot), "named output lands the slot manifest");
    let reloaded = match confit_core::store::slots::load_state(Some(&slot), &fs) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("slot manifest loads: {error}"),
    };
    assert_eq!(reloaded.manifest.documents.len(), 1);
    let sibling = match slot.with_file_name("seam-slot-order-pin.cb").to_str() {
        Some(_) => slot.with_file_name("seam-slot-order-pin.cb"),
        None => panic!("slot sibling resolves"),
    };
    assert!(
        !fs.exists(&sibling),
        "named output writes no bundle sibling"
    );
}

#[test]
fn live_headless_finish_joins_twice() {
    let live = confit_cli::presentation::spinner::Live::new();
    if let Some(sender) = live.sink() {
        let _ = sender.send(confit_core::progress::Event::Hashing);
        let _ = sender.send(confit_core::progress::Event::WritingPlan { documents: 1 });
    }
    live.finish();
    live.finish();
}

#[test]
fn plan_hook_lines_carry_lifecycle_without_evaluation() {
    pin_home();
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local tool = confit.config("tool")
tool:add_hook(confit.hook.run({ "tool", "--flag" }, { when = confit.runtime.in_path("mise") }))
return { shells = { "bash" }, configs = { tool } }
"#;
    let profile = write_profile(dir.path(), "profile.lua", body);
    let fs = MemoryFs::new();
    let args = confit_cli::cli::PlanArgs {
        profile,
        shared: confit_cli::cli::SharedArgs {
            root: Some(dir.path().to_path_buf()),
            plugins: None,
            re_fetch: false,
        },
        output: None,
    };
    let mut input = Cursor::new(String::new());
    let runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        seams: confit_cli::seams::Seams::memory(&fs, &mut input),
    };
    let outcome = match runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs: {error}"),
    };
    assert_eq!(
        outcome.hook_lines,
        vec![
            "+ tool --flag".to_string(),
            "  + when (in_path(mise))".to_string(),
        ]
    );
    for line in &outcome.hook_lines {
        assert!(!line.contains("! run:"), "no evaluated run: {line}");
        assert!(
            !line.contains("warn: cannot run"),
            "no evaluated warn: {line}"
        );
        assert!(!line.contains("skipped:"), "no evaluated skip: {line}");
    }
}
