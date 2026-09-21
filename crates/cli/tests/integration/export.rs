use crate::common::*;

#[test]
fn export_applied_slot_writes_auto_bundle() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    seed_slot(&fs, &built);
    let args = confit_cli::cli::ExportArgs {
        picker: None,
        output: None,
        manifest: false,
    };
    let report = match run_export(&fs, &args) {
        Ok(report) => report,
        Err(error) => panic!("export runs: {error}"),
    };
    assert_eq!(report.dest, Some(PathBuf::from("applied.cb")));
    assert_eq!(report.manifest, None);
    let dest = match report.dest {
        Some(dest) => dest,
        None => panic!("file export holds a dest"),
    };
    assert!(fs.exists(&dest), "bundle lands at the auto name");
    let restored = match confit_core::store::bundle::read_bundle(&dest, &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("bundle reads: {error}"),
    };
    assert_eq!(plan_value(&restored), plan_value(&built));
}

#[test]
fn export_named_slot_writes_auto_bundle() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    seed_named(&fs, "personal", &built);
    let args = confit_cli::cli::ExportArgs {
        picker: Some("@personal".to_string()),
        output: None,
        manifest: false,
    };
    let report = match run_export(&fs, &args) {
        Ok(report) => report,
        Err(error) => panic!("export runs: {error}"),
    };
    assert_eq!(report.dest, Some(PathBuf::from("personal.cb")));
    assert_eq!(report.manifest, None);
    let dest = match report.dest {
        Some(dest) => dest,
        None => panic!("file export holds a dest"),
    };
    assert!(fs.exists(&dest), "bundle lands at the auto name");
    let restored = match confit_core::store::bundle::read_bundle(&dest, &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("bundle reads: {error}"),
    };
    assert_eq!(plan_value(&restored), plan_value(&built));
}

#[test]
fn export_history_slots_write_auto_bundles() {
    pin_home();
    let fs = MemoryFs::new();
    let old = match build(vec![ManifestDocument::new(
        DocPath::new("history-old"),
        ManifestData::Text {
            content: "old\n".to_string(),
            mode: None,
            unmanaged: false,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("old bundle builds: {error}"),
    };
    let new = match build(vec![ManifestDocument::new(
        DocPath::new("history-new"),
        ManifestData::Text {
            content: "new\n".to_string(),
            mode: None,
            unmanaged: false,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("new bundle builds: {error}"),
    };
    let dir = match confit_core::store::slots::resolve_previous_dir() {
        Ok(dir) => dir,
        Err(error) => panic!("history dir resolves: {error}"),
    };
    match confit_core::store::slots::write_manifest(&old, Some(&dir.join("a-old.json")), &fs, None)
    {
        Ok(()) => {}
        Err(error) => panic!("old entry seeds: {error}"),
    }
    match confit_core::store::slots::write_manifest(&new, Some(&dir.join("b-new.json")), &fs, None)
    {
        Ok(()) => {}
        Err(error) => panic!("new entry seeds: {error}"),
    }
    let first_args = confit_cli::cli::ExportArgs {
        picker: Some("%1".to_string()),
        output: None,
        manifest: false,
    };
    let first = match run_export(&fs, &first_args) {
        Ok(report) => report,
        Err(error) => panic!("newest export runs: {error}"),
    };
    assert_eq!(first.dest, Some(PathBuf::from("prev-1.cb")));
    let first_dest = match first.dest {
        Some(dest) => dest,
        None => panic!("file export holds a dest"),
    };
    let first_restored = match confit_core::store::bundle::read_bundle(&first_dest, &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("newest bundle reads: {error}"),
    };
    assert_eq!(plan_value(&first_restored), plan_value(&new));
    let second_args = confit_cli::cli::ExportArgs {
        picker: Some("%2".to_string()),
        output: None,
        manifest: false,
    };
    let second = match run_export(&fs, &second_args) {
        Ok(report) => report,
        Err(error) => panic!("older export runs: {error}"),
    };
    assert_eq!(second.dest, Some(PathBuf::from("prev-2.cb")));
    let second_dest = match second.dest {
        Some(dest) => dest,
        None => panic!("file export holds a dest"),
    };
    let second_restored = match confit_core::store::bundle::read_bundle(&second_dest, &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("older bundle reads: {error}"),
    };
    assert_eq!(plan_value(&second_restored), plan_value(&old));
}

#[test]
fn export_missing_applied_slot_refuses() {
    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::ExportArgs {
        picker: None,
        output: None,
        manifest: false,
    };
    match run_export(&fs, &args) {
        Ok(_) => panic!("absent slot exports"),
        Err(error) => assert!(
            error.to_string().contains("apply first"),
            "absent slot names the fix: {error}"
        ),
    }
}

#[test]
fn export_output_imposes_cb_suffix() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    seed_named(&fs, "personal", &built);
    let bare_args = confit_cli::cli::ExportArgs {
        picker: Some("@personal".to_string()),
        output: Some(PathBuf::from("backup")),
        manifest: false,
    };
    let bare = match run_export(&fs, &bare_args) {
        Ok(report) => report,
        Err(error) => panic!("bare output exports: {error}"),
    };
    assert_eq!(bare.dest, Some(PathBuf::from("backup.cb")));
    assert!(
        fs.exists(Path::new("backup.cb")),
        "bare output gains the bundle suffix"
    );
    assert!(
        !fs.exists(Path::new("backup")),
        "bare output writes no suffixless file"
    );
    let restored = match confit_core::store::bundle::read_bundle(Path::new("backup.cb"), &fs) {
        Ok(restored) => restored,
        Err(error) => panic!("bundle reads: {error}"),
    };
    assert_eq!(plan_value(&restored), plan_value(&built));
    let kept_args = confit_cli::cli::ExportArgs {
        picker: Some("@personal".to_string()),
        output: Some(PathBuf::from("kept.cb")),
        manifest: false,
    };
    let kept = match run_export(&fs, &kept_args) {
        Ok(report) => report,
        Err(error) => panic!("suffixed output exports: {error}"),
    };
    assert_eq!(kept.dest, Some(PathBuf::from("kept.cb")));
}

#[test]
fn export_bare_path_picker_refuses() {
    pin_home();
    let fs = MemoryFs::new();
    for raw in ["backup.cb", "backup", "slot.json"] {
        let args = confit_cli::cli::ExportArgs {
            picker: Some(raw.to_string()),
            output: None,
            manifest: false,
        };
        match run_export(&fs, &args) {
            Ok(_) => panic!("{raw:?} parses as a slot"),
            Err(error) => assert!(
                error.to_string().contains("unsupported"),
                "bare value names the picker shapes: {error}"
            ),
        }
    }
}

#[test]
fn export_manifest_prints_pretty_json_without_base64() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let raw = vec![0xFF, 0x00, 0x80, 0x41];
    let built = match build(vec![ManifestDocument::new(
        DocPath::new("bin"),
        ManifestData::Opaque {
            blob: confit_core::ids::sha256_hex(&raw),
            size: raw.len() as u64,
            mode: None,
            unmanaged: false,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("bundle builds: {error}"),
    };
    seed_named(&fs, "personal", &built);
    let args = confit_cli::cli::ExportArgs {
        picker: Some("@personal".to_string()),
        output: None,
        manifest: true,
    };
    let report = match run_export(&fs, &args) {
        Ok(report) => report,
        Err(error) => panic!("manifest export runs: {error}"),
    };
    assert_eq!(report.dest, None);
    let text = match report.manifest {
        Some(text) => text,
        None => panic!("manifest export holds text"),
    };
    assert!(text.contains('\n'), "manifest prints pretty JSON: {text}");
    assert!(
        text.contains("\"blob\""),
        "opaque payloads persist as blob refs: {text}"
    );
    assert!(
        !text.contains("content"),
        "manifest holds refs, no inline bytes: {text}"
    );
    let manifest: confit_core::store::manifest::Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => panic!("manifest parses: {error}"),
    };
    assert_eq!(manifest.version, BUNDLE_VERSION);
    assert_eq!(manifest.documents.len(), 1);
    match &manifest.documents[0].data {
        confit_core::document::ManifestData::Opaque { blob, .. } => assert_eq!(
            blob,
            &confit_core::ids::sha256_hex(&raw),
            "blob ref names the payload hash"
        ),
        other => panic!("opaque ref expected, got {other:?}"),
    }
    assert!(
        !fs.exists(Path::new("personal.cb")),
        "manifest print writes no bundle file"
    );
}

#[test]
fn export_output_plus_manifest_refuses() {
    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::ExportArgs {
        picker: Some("@personal".to_string()),
        output: Some(PathBuf::from("backup.cb")),
        manifest: true,
    };
    match run_export(&fs, &args) {
        Ok(_) => panic!("output plus manifest passes"),
        Err(error) => assert!(
            error.to_string().contains("refuse"),
            "joint flags refuse together: {error}"
        ),
    }
}
