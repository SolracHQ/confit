use crate::common::*;

#[test]
fn delete_prunes_orphans_keeping_shared() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let shared_bytes = b"shared-confit-blob".to_vec();
    let shared_sha = confit_core::ids::sha256_hex(&shared_bytes);
    let keep_bytes = b"keep-confit-blob".to_vec();
    let keep_sha = confit_core::ids::sha256_hex(&keep_bytes);
    let drop_bytes = b"drop-confit-blob".to_vec();
    let drop_sha = confit_core::ids::sha256_hex(&drop_bytes);
    let keep_plan = match build(vec![
        ManifestDocument::new(
            DocPath::new("shared.bin"),
            ManifestData::Opaque {
                blob: shared_sha.clone(),
                size: shared_bytes.len() as u64,
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("keep.bin"),
            ManifestData::Opaque {
                blob: keep_sha.clone(),
                size: keep_bytes.len() as u64,
                mode: None,
                unmanaged: false,
            },
        ),
    ]) {
        Ok(mut built) => {
            built.blobs.insert(shared_sha.clone(), shared_bytes.clone());
            built.blobs.insert(keep_sha.clone(), keep_bytes.clone());
            built
        }
        Err(error) => panic!("keep bundle builds: {error}"),
    };
    let drop_plan = match build(vec![
        ManifestDocument::new(
            DocPath::new("shared.bin"),
            ManifestData::Opaque {
                blob: shared_sha.clone(),
                size: shared_bytes.len() as u64,
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("drop.bin"),
            ManifestData::Opaque {
                blob: drop_sha.clone(),
                size: drop_bytes.len() as u64,
                mode: None,
                unmanaged: false,
            },
        ),
    ]) {
        Ok(mut built) => {
            built.blobs.insert(shared_sha.clone(), shared_bytes.clone());
            built.blobs.insert(drop_sha.clone(), drop_bytes.clone());
            built
        }
        Err(error) => panic!("drop bundle builds: {error}"),
    };
    let keep_dest = seed_named(&fs, "keep", &keep_plan);
    let drop_dest = seed_named(&fs, "drop", &drop_plan);
    let pool = match confit_core::store::blobs::resolve_blobs_dir() {
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
