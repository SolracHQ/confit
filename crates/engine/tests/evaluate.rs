mod common;

use common::*;
use confit_model::arg::Arg;
use confit_model::document::{Data, StructuredDocument, StructuredFormat};
use confit_model::routes::{Route, RouteBase};

#[test]
fn evaluate_keeps_text_in_destination_order() {
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
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_builds_structured_data() {
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
                        None => panic!("expected the shell key"),
                    }
                }
                _ => panic!("expected a structured body"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_duplicate_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects duplicate destinations"
    );
}

#[test]
fn evaluate_uses_the_project_root_from_stores() {
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
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn project_text_reaches_the_documents() {
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
        Err(error) => panic!("the profile fails to evaluate: {error}"),
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
                        "the base keys keep their values against later writes",
                    );
                    assert_eq!(
                        data.get("extra").and_then(|item| item.as_str()),
                        Some("yes"),
                        "the fresh keys take the writing patch",
                    );
                }
                _ => panic!("the patched document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_duplicate_names() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE_NAMES);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects duplicate config names"
    );
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
                _ => panic!("the merged document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_absent_target() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MISSING_TARGET);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a missing patch target"
    );
}

#[test]
fn evaluate_rejects_a_text_base() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), TEXT_BASE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a patch over text"
    );
}

#[test]
fn evaluate_rejects_a_callback_error() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FAILING_CALLBACK);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a failing callback"
    );
}

#[test]
fn evaluate_rejects_a_format_gap() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FORMAT_GAP);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a format gap"
    );
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
                _ => panic!("the merged document is rc"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_absent_rc_target() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MISSING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a missing rc target"
    );
}

#[test]
fn evaluate_rejects_an_rc_text_base() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_TEXT_BASE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc patch over text"
    );
}

#[test]
fn evaluate_rejects_an_rc_callback_error() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FAILING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a failing rc callback"
    );
}

#[test]
fn evaluate_rejects_an_rc_format_gap() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FORMAT_GAP);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc format gap"
    );
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
                _ => panic!("the first document is rc"),
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
                _ => panic!("the second document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_rc_patch_over_structured() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_OVER_STRUCTURED);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc patch over a structured body"
    );
}

#[test]
fn evaluate_rejects_mixed_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_FORMATS);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects mixed formats"
    );
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SAME_TARGET);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a mixed kind target"
    );
}

#[test]
fn evaluate_rejects_an_unknown_patch_key() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FOREIGN_PATCH_KEY);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an unknown patch key"
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
            assert_eq!(found.hooks[0].timeout_secs.secs(), 600);
            assert_eq!(Arg::join(&found.hooks[1].argv), "mise install");
            assert_eq!(found.hooks[1].timeout_secs.secs(), 90);
            assert_eq!(Arg::join(&found.hooks[2].argv), "fc-cache");
            assert_eq!(found.hooks[2].timeout_secs.secs(), 600);
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_reports_an_invalid_route_base() {
    use confit_core::error::{ErrorCause, EvalError};

    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let source = r#"
        return {
            documents = {
                { path = route("void", "note.txt"), text = "hi" },
            },
        }
    "#;
    let (_, profile) = profile_with(dir.path(), source);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("the engine accepts a bad route base"),
        Err(error) => {
            assert!(
                matches!(error.cause(), ErrorCause::EvalError(EvalError::LuaFailed)),
                "the cause shows the failure: {error}"
            );
            assert!(
                error.to_string().contains("is not a valid variant"),
                "the line shows the shape failure: {error}"
            );
        }
    }
}
