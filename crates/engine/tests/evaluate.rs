mod common;

use common::*;
use confit_driver::fs::TestGuard;
use confit_model::document::{Data, StructuredFormat};
use confit_model::routes::{Route, RouteBase};

#[test]
fn evaluates_text_in_destination_order() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PAIR);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let displays: Vec<String> = found
                .documents
                .keys()
                .map(|route| route.display())
                .collect();
            assert_eq!(
                displays,
                vec!["home('apple')".to_owned(), "home('zebra')".to_owned()]
            );
            let apple = Route::new(RouteBase::Home, "apple").unwrap();
            let zebra = Route::new(RouteBase::Home, "zebra").unwrap();
            assert_eq!(text_of(&found.documents[&apple]), "a");
            assert_eq!(text_of(&found.documents[&zebra]), "z");
        }
        Err(error) => panic!("profile evaluates: {error}"),
    }
}

#[test]
fn evaluates_structured_with_filled_hash() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), STRUCTURED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let (route, data) = found.documents.iter().next().unwrap();
            assert_eq!(route.display(), "home('config.toml')");
            match data {
                Data::Structured { format, data } => {
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
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("duplicate destinations pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the destination: {text}"
            );
        }
    }
}

#[test]
fn root_rides_stores() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    confit_driver::fs::create_dir_all(&elsewhere).unwrap();
    let profile = elsewhere.join("profile.lua");
    confit_driver::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(found.documents.values().next().unwrap()), "hello");
        }
        Err(error) => panic!("stored root evaluates: {error}"),
    }
}

#[test]
fn project_text_flows_into_documents() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
    let profile = root.join("profile.lua");
    confit_driver::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(found.documents.values().next().unwrap()), "hello");
        }
        Err(error) => panic!("project text evaluates: {error}"),
    }
}

#[test]
fn patches_apply_over_declared_documents() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PATCHED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            match found.documents.values().next().unwrap() {
                Data::Structured { data, .. } => {
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
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE_NAMES);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("duplicate names pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("same"), "the text shows the config: {text}");
            assert!(
                text.contains("already defined"),
                "the text shows the fault: {text}"
            );
        }
    }
}

#[test]
fn two_patches_merge_both_keys() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MERGED_PATCHES);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let (route, data) = found.documents.iter().next().unwrap();
            assert_eq!(route.display(), "home('config.json')");
            match data {
                Data::Structured { format, data } => {
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
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MISSING_TARGET);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("missing target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('fresh.json')"),
                "the text shows the target: {text}"
            );
        }
    }
}

#[test]
fn text_bases_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), TEXT_BASE);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("patched text passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('note.txt')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("text"), "the text shows the base: {text}");
        }
    }
}

#[test]
fn failing_callbacks_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FAILING_CALLBACK);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("failing callback passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("patch boom"), "carries the cause: {text}");
        }
    }
}

#[test]
fn format_gaps_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FORMAT_GAP);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("format gap passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('config.json')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("json"), "the text shows the base: {text}");
            assert!(text.contains("toml"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn rc_two_patches_merge_entries() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MERGED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let (route, data) = found.documents.iter().next().unwrap();
            assert_eq!(route.display(), "home('.bashrc')");
            assert_eq!(rc_names(data), vec!["SEED".to_owned(), "FIRST".to_owned()]);
            match data {
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
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MISSING);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("missing rc target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the target: {text}"
            );
        }
    }
}

#[test]
fn rc_text_bases_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_TEXT_BASE);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("patched rc text passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('note.txt')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'text'"), "the text shows the base: {text}");
            assert!(text.contains("'rc'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn rc_failing_callbacks_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FAILING);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("failing rc callback passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("rc boom"), "carries the cause: {text}");
        }
    }
}

#[test]
fn rc_format_gaps_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FORMAT_GAP);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("rc format gap passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'rc'"), "the text shows the base: {text}");
            assert!(text.contains("'json'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn mixed_sides_merge_live_targets() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SPLIT);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let ordered: Vec<(&Route, &Data)> = found.documents.iter().collect();
            match ordered[0].1 {
                Data::Rc(_) => {
                    assert_eq!(ordered[0].0.display(), "home('.bashrc')")
                }
                _ => panic!("first document stays rc"),
            }
            match ordered[1].1 {
                Data::Structured { data, .. } => {
                    assert_eq!(ordered[1].0.display(), "home('config.json')");
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
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_OVER_STRUCTURED);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("rc over structured passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('config.json')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("json"), "the text shows the base: {text}");
            assert!(text.contains("rc"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn mixed_structured_formats_fail() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_FORMATS);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("mixed formats pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('fresh.json')"),
                "the text shows the target: {text}"
            );
            assert!(
                text.contains("json"),
                "the text shows the first format: {text}"
            );
            assert!(
                text.contains("toml"),
                "the text shows the second format: {text}"
            );
        }
    }
}

#[test]
fn mixed_kind_same_target_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SAME_TARGET);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("mixed kind target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('config.json')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("json"), "the text shows the base: {text}");
            assert!(text.contains("rc"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn foreign_patch_keys_fail_showing_keys() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FOREIGN_PATCH_KEY);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("foreign patch key passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("nope"), "the text shows the key: {text}");
        }
    }
}

#[test]
fn evaluation_carries_hooks_through() {
    let dir = tempfile::tempdir().unwrap();
    let _guard = TestGuard::install();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), HOOKS_THROUGH);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.hooks.len(), 3);
            assert_eq!(
                confit_model::arg::Arg::join(&found.hooks[0].argv),
                "mise install"
            );
            assert_eq!(found.hooks[0].timeout_secs, 600);
            assert_eq!(
                confit_model::arg::Arg::join(&found.hooks[1].argv),
                "mise install"
            );
            assert_eq!(found.hooks[1].timeout_secs, 90);
            assert_eq!(
                confit_model::arg::Arg::join(&found.hooks[2].argv),
                "fc-cache"
            );
            assert_eq!(found.hooks[2].timeout_secs, 600);
        }
        Err(error) => panic!("hooks evaluate: {error}"),
    }
}
