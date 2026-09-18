use crate::common::*;

#[test]
fn delete_prunes_orphans_keeping_shared() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let shared_bytes = b"shared-confit-blob".to_vec();
    let shared_sha = confit_core::plan::sha256_hex(&shared_bytes);
    let keep_bytes = b"keep-confit-blob".to_vec();
    let keep_sha = confit_core::plan::sha256_hex(&keep_bytes);
    let drop_bytes = b"drop-confit-blob".to_vec();
    let drop_sha = confit_core::plan::sha256_hex(&drop_bytes);
    let keep_plan = match build(vec![
        Document::new(
            DocPath::new("shared.bin"),
            DocumentData::Opaque {
                content: shared_bytes.clone(),
                mode: None,
            },
        ),
        Document::new(
            DocPath::new("keep.bin"),
            DocumentData::Opaque {
                content: keep_bytes,
                mode: None,
            },
        ),
    ]) {
        Ok(built) => built,
        Err(error) => panic!("keep plan builds: {error}"),
    };
    let drop_plan = match build(vec![
        Document::new(
            DocPath::new("shared.bin"),
            DocumentData::Opaque {
                content: shared_bytes,
                mode: None,
            },
        ),
        Document::new(
            DocPath::new("drop.bin"),
            DocumentData::Opaque {
                content: drop_bytes,
                mode: None,
            },
        ),
    ]) {
        Ok(built) => built,
        Err(error) => panic!("drop plan builds: {error}"),
    };
    let keep_dest = seed_named(&fs, "keep", &keep_plan);
    let drop_dest = seed_named(&fs, "drop", &drop_plan);
    let pool = match confit_core::store::resolve_blobs_dir() {
        Ok(pool) => pool,
        Err(error) => panic!("pool resolves: {error}"),
    };
    assert!(fs.exists(&pool.join(&shared_sha)), "shared blob pools");
    assert!(fs.exists(&pool.join(&keep_sha)), "kept blob pools");
    assert!(fs.exists(&pool.join(&drop_sha)), "dropped blob pools");
    let args = confit_cli::cli::DeleteArgs {
        name: "@drop".to_string(),
    };
    let report = match run_delete(&fs, &args) {
        Ok(report) => report,
        Err(error) => panic!("delete runs: {error}"),
    };
    assert_eq!(report.name, "drop");
    assert_eq!(report.pruned, 1);
    assert!(!fs.exists(&drop_dest), "named manifest drops");
    assert!(fs.exists(&keep_dest), "remaining manifest stays");
    assert!(
        fs.exists(&pool.join(&shared_sha)),
        "shared blob stays pooled"
    );
    assert!(fs.exists(&pool.join(&keep_sha)), "kept blob stays pooled");
    assert!(
        !fs.exists(&pool.join(&drop_sha)),
        "orphaned blob leaves the pool"
    );
}

#[test]
fn delete_non_at_names_refuse() {
    pin_home();
    let fs = MemoryFs::new();
    for raw in ["personal", "%1", "%2", "state.json", "backup.cb"] {
        let args = confit_cli::cli::DeleteArgs {
            name: raw.to_string(),
        };
        match run_delete(&fs, &args) {
            Ok(_) => panic!("{raw:?} deletes"),
            Err(error) => assert!(
                error.to_string().contains("never delete"),
                "non-@ value stays unreachable: {error}"
            ),
        }
    }
}

#[test]
fn delete_absent_slot_refuses() {
    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::DeleteArgs {
        name: "@missing".to_string(),
    };
    match run_delete(&fs, &args) {
        Ok(_) => panic!("absent slot deletes"),
        Err(error) => assert!(
            error.to_string().contains("reads absent"),
            "absent slot names itself: {error}"
        ),
    }
}
