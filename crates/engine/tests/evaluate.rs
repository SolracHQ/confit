mod common;

use common::*;
use confit_model::arg::Arg;
use confit_model::document::{Data, StructuredDocument, StructuredFormat};
use confit_model::routes::{Route, RouteBase};

#[test]
fn evaluates_text_in_destination_order() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PAIR);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let displays: Vec<String> = found
                .documents
                .iter()
                .map(|document| document.path.display())
                .collect();
            assert_eq!(
                displays,
                vec!["home('apple')".to_owned(), "home('zebra')".to_owned()]
            );
            let apple = Route::new(RouteBase::Home, "apple").unwrap();
            let zebra = Route::new(RouteBase::Home, "zebra").unwrap();
            let apple_data = &found
                .documents
                .iter()
                .find(|document| document.path == apple)
                .unwrap()
                .data;
            let zebra_data = &found
                .documents
                .iter()
                .find(|document| document.path == zebra)
                .unwrap()
                .data;
            assert_eq!(text_of(apple_data), "a");
            assert_eq!(text_of(zebra_data), "z");
        }
        Err(error) => panic!("profile evaluates: {error}"),
    }
}

#[test]
fn evaluates_structured_with_filled_hash() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), STRUCTURED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('config.toml')");
            match &document.data {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Toml);
                    match data.get("profile").and_then(|item| item.get("shell")) {
                        Some(shell) => assert_eq!(shell.as_str(), Some("bash")),
                        None => panic!("structured data carries the shell"),
                    }
                }
                _ => panic!("structured document evaluates"),
            }
        }
        Err(error) => panic!("structured profile evaluates: {error}"),
    }
}

#[test]
fn duplicate_destinations_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "duplicate destinations fail"
    );
}

#[test]
fn root_rides_stores() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let profile = elsewhere.join("profile.lua");
    std::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(&found.documents.first().unwrap().data), "hello");
        }
        Err(error) => panic!("stored root evaluates: {error}"),
    }
}

#[test]
fn project_text_flows_into_documents() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    let profile = root.join("profile.lua");
    std::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(&found.documents.first().unwrap().data), "hello");
        }
        Err(error) => panic!("project text evaluates: {error}"),
    }
}

#[test]
fn patches_apply_over_declared_documents() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PATCHED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            match &found.documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
                    assert_eq!(
                        data.get("count").and_then(|item| item.as_u64()),
                        Some(1),
                        "base keys keep their values against later writes",
                    );
                    assert_eq!(
                        data.get("extra").and_then(|item| item.as_str()),
                        Some("yes"),
                        "fresh keys take the writing patch",
                    );
                }
                _ => panic!("patched document stays structured"),
            }
        }
        Err(error) => panic!("patched profile evaluates: {error}"),
    }
}

#[test]
fn duplicate_names_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE_NAMES);
    assert!(engine.evaluate(&profile).is_err(), "duplicate names fail");
}

#[test]
fn two_patches_merge_both_keys() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MERGED_PATCHES);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('config.json')");
            match &document.data {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(
                        data.get("alpha").and_then(|item| item.as_str()),
                        Some("one")
                    );
                    assert_eq!(data.get("beta").and_then(|item| item.as_str()), Some("two"));
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("merged document stays structured"),
            }
        }
        Err(error) => panic!("merged patches evaluate: {error}"),
    }
}

#[test]
fn missing_targets_fail_showing_targets() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MISSING_TARGET);
    assert!(engine.evaluate(&profile).is_err(), "missing target fails");
}

#[test]
fn text_bases_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), TEXT_BASE);
    assert!(engine.evaluate(&profile).is_err(), "patched text fails");
}

#[test]
fn failing_callbacks_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FAILING_CALLBACK);
    assert!(engine.evaluate(&profile).is_err(), "failing callback fails");
}

#[test]
fn format_gaps_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FORMAT_GAP);
    assert!(engine.evaluate(&profile).is_err(), "format gap fails");
}

#[test]
fn rc_two_patches_merge_entries() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MERGED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('.bashrc')");
            assert_eq!(
                rc_names(&document.data),
                vec!["SEED".to_owned(), "FIRST".to_owned()]
            );
            match &document.data {
                Data::Rc(rc) => {
                    assert_eq!(rc.config.len(), 1);
                }
                _ => panic!("merged rc stays rc"),
            }
        }
        Err(error) => panic!("merged rc evaluates: {error}"),
    }
}

#[test]
fn rc_missing_targets_fail_showing_targets() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MISSING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "missing rc target fails"
    );
}

#[test]
fn rc_text_bases_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_TEXT_BASE);
    assert!(engine.evaluate(&profile).is_err(), "patched rc text fails");
}

#[test]
fn rc_failing_callbacks_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FAILING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "failing rc callback fails"
    );
}

#[test]
fn rc_format_gaps_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FORMAT_GAP);
    assert!(engine.evaluate(&profile).is_err(), "rc format gap fails");
}

#[test]
fn mixed_sides_merge_live_targets() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SPLIT);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let ordered: &[_] = &found.documents;
            match &ordered[0].data {
                Data::Rc(_) => {
                    assert_eq!(ordered[0].path.display(), "home('.bashrc')")
                }
                _ => panic!("first document stays rc"),
            }
            match &ordered[1].data {
                Data::Structured(StructuredDocument { data, .. }) => {
                    assert_eq!(ordered[1].path.display(), "home('config.json')");
                    assert_eq!(
                        data.get("alpha").and_then(|item| item.as_str()),
                        Some("one")
                    );
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("second document stays structured"),
            }
        }
        Err(error) => panic!("split profile evaluates: {error}"),
    }
}

#[test]
fn rc_patches_over_structured_bases_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_OVER_STRUCTURED);
    assert!(
        engine.evaluate(&profile).is_err(),
        "rc over structured fails"
    );
}

#[test]
fn mixed_shapes_fail() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_FORMATS);
    assert!(engine.evaluate(&profile).is_err(), "mixed formats fail");
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SAME_TARGET);
    assert!(
        engine.evaluate(&profile).is_err(),
        "mixed kind target fails"
    );
}

#[test]
fn foreign_patch_keys_fail_showing_keys() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FOREIGN_PATCH_KEY);
    assert!(
        engine.evaluate(&profile).is_err(),
        "foreign patch key fails"
    );
}

#[test]
fn evaluation_carries_hooks_through() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), HOOKS_THROUGH);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.hooks.len(), 3);
            assert_eq!(Arg::join(&found.hooks[0].argv), "mise install");
            assert_eq!(found.hooks[0].timeout_secs, 600);
            assert_eq!(Arg::join(&found.hooks[1].argv), "mise install");
            assert_eq!(found.hooks[1].timeout_secs, 90);
            assert_eq!(Arg::join(&found.hooks[2].argv), "fc-cache");
            assert_eq!(found.hooks[2].timeout_secs, 600);
        }
        Err(error) => panic!("hooks evaluate: {error}"),
    }
}

#[test]
fn bad_base_renders_face_line() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let source = r#"
        return {
            documents = {
                { path = { base = "void", relative = "note.txt" }, text = "hi" },
            },
        }
    "#;
    let (_, profile) = profile_with(dir.path(), source);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("bad base passes"),
        Err(error) => assert_eq!(
            error.to_string(),
            "Cannot evaluate the profile, documents[1].path.base: \"void\" is not a valid variant, choose one of home, config, data, cache, literal"
        ),
    }
}
