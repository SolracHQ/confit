use crate::common::*;

#[test]
fn hooks_run_spawn_resolve_and_log() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake =
        confit_cli::hooks::FakeRunner::new(VecDeque::from([Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: b"did\n".to_vec(),
        })]));
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        Some(PathBuf::from("run.log")),
        vec![hook_for(&["tool", "--flag"], &["/fakebin"], None, vec![])],
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].argv,
        vec!["/fakebin/tool".to_string(), "--flag".to_string()]
    );
    assert_eq!(calls[0].path_dirs, vec![PathBuf::from("/fakebin")]);
    assert_eq!(calls[0].timeout_secs, 600);
    let log = memory_bytes(&fs, Path::new("run.log"));
    let log_text = match String::from_utf8(log) {
        Ok(text) => text,
        Err(error) => panic!("log parses: {error}"),
    };
    assert!(
        log_text.contains("hook 1 of 1: tool --flag"),
        "log holds header: {log_text}"
    );
    assert!(log_text.contains("did"), "log holds bytes: {log_text}");
}

#[test]
fn hooks_skip_on_passing_checks_without_spawning() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::new());
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            None,
            vec![confit_core::condition::Condition::Exists {
                path: "/fakebin/probe".to_string(),
            }],
        )],
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
}

#[test]
fn hooks_warn_on_closed_gates_without_spawning() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::new());
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            Some(confit_core::condition::Condition::InPath {
                name: "definitely-missing-confit-binary".to_string(),
            }),
            vec![],
        )],
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
}

#[test]
fn hooks_abort_on_first_failure() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::from([
        Ok(confit_cli::hooks::HookRun {
            code: 1,
            output: b"boom\n".to_vec(),
        }),
        Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        }),
    ]));
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        Some(PathBuf::from("run.log")),
        vec![
            hook_for(&["tool", "first"], &["/fakebin"], None, vec![]),
            hook_for(&["tool", "second"], &["/fakebin"], None, vec![]),
        ],
    );
    match runner.execute() {
        Ok(_) => panic!("failing hook passes"),
        Err(error) => assert!(
            error.to_string().contains("failed with code 1"),
            "failure aborts: {error}"
        ),
    }
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn hooks_timeout_aborts_as_own_error() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::from([Err(
        confit_core::error::Error::Plan("hook 'tool' timed out after 600s".to_string()),
    )]));
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        None,
        vec![hook_for(&["tool"], &["/fakebin"], None, vec![])],
    );
    match runner.execute() {
        Ok(_) => panic!("timed out hook passes"),
        Err(error) => assert!(
            error.to_string().contains("timed out"),
            "timeout reads own: {error}"
        ),
    }
}

#[test]
fn hooks_post_checks_fail_after_run() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let fake =
        confit_cli::hooks::FakeRunner::new(VecDeque::from([Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        })]));
    let probe = hook_probe();
    let runner = hook_runner(
        &fs,
        &probe,
        &mut input,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            None,
            vec![confit_core::condition::Condition::Exists {
                path: "/fakebin/absent".to_string(),
            }],
        )],
    );
    match runner.execute() {
        Ok(_) => panic!("unproven hook passes"),
        Err(error) => assert!(
            error.to_string().contains("failed checks after run"),
            "post checks verify: {error}"
        ),
    }
}

#[test]
fn changed_gate_skips_quiet_apply_runs_touching_apply() {
    use std::collections::VecDeque;

    use confit_core::fs::Filesystem;

    pin_home();
    let gate = || {
        Some(confit_core::condition::Condition::Changed {
            path: "touched".to_string(),
        })
    };
    let desired = || {
        vec![ManifestDocument::new(
            DocPath::new("touched"),
            ManifestData::Text {
                content: "hi\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        )]
    };
    let hook = || hook_for(&["tool"], &["/fakebin"], gate(), vec![]);

    let fs = hook_fs();
    match fs.write(Path::new("touched"), b"hi\n") {
        Ok(()) => {}
        Err(error) => panic!("touched seeds: {error}"),
    }
    let mut recorded = desired();
    fill_hashes(&mut recorded);
    let mut previous = Bundle::empty();
    previous.manifest.documents = recorded;
    let manifest = match Bundle::build(desired(), vec![hook()]) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    let mut input = Cursor::new(String::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::new());
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let probe = hook_probe();
    let mut seams = confit_cli::seams::Seams::memory(&fs, &probe, &mut input).with_print(print_tx);
    seams.hook_runner = Some(&fake);
    let runner = confit_cli::actions::apply::ApplyRunner {
        manifest,
        previous,
        state: None,
        force: true,
        seams,
    };
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("quiet apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    assert!(
        lines
            .iter()
            .any(|line| line == "skipped: tool (no need: changed(touched))"),
        "quiet apply skips the hook: {lines:?}"
    );

    let fs = hook_fs();
    let manifest = match Bundle::build(desired(), vec![hook()]) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    let mut input = Cursor::new(String::new());
    let fake =
        confit_cli::hooks::FakeRunner::new(VecDeque::from([Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        })]));
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let probe = hook_probe();
    let mut seams = confit_cli::seams::Seams::memory(&fs, &probe, &mut input).with_print(print_tx);
    seams.hook_runner = Some(&fake);
    let runner = confit_cli::actions::apply::ApplyRunner {
        manifest,
        previous: Bundle::empty(),
        state: None,
        force: true,
        seams,
    };
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("touching apply runs: {error}"),
    }
    assert_eq!(fake.calls().len(), 1);
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    assert!(
        lines.iter().any(|line| line.contains("hook 1 of 1: tool")),
        "touching apply runs the hook: {lines:?}"
    );
}

#[test]
fn requires_closed_apply_warns_without_spawning() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut hook = hook_for(&["tool"], &["/fakebin"], None, vec![]);
    hook.requires = Some(confit_core::condition::Condition::InPath {
        name: "definitely-missing-confit-binary".to_string(),
    });
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::new());
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let probe = hook_probe();
    let mut seams = confit_cli::seams::Seams::memory(&fs, &probe, &mut input).with_print(print_tx);
    seams.hook_runner = Some(&fake);
    let mut runner = apply_runner(Vec::new(), Bundle::empty(), None, true, seams);
    runner.manifest = match Bundle::build(Vec::new(), vec![hook]) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    assert!(
        lines.iter().any(|line| line
            == "warn: tool cannot run (in_path(definitely-missing-confit-binary))"),
        "closed requires warns: {lines:?}"
    );
}

#[test]
fn when_closed_apply_skips_without_spawning() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let hook = hook_for(
        &["tool"],
        &["/fakebin"],
        Some(confit_core::condition::Condition::InPath {
            name: "definitely-missing-confit-binary".to_string(),
        }),
        vec![],
    );
    let mut input = Cursor::new(Vec::new());
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::new());
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let probe = hook_probe();
    let mut seams = confit_cli::seams::Seams::memory(&fs, &probe, &mut input).with_print(print_tx);
    seams.hook_runner = Some(&fake);
    let mut runner = apply_runner(Vec::new(), Bundle::empty(), None, true, seams);
    runner.manifest = match Bundle::build(Vec::new(), vec![hook]) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    assert!(
        lines.iter().any(
            |line| line == "skipped: tool (no need: in_path(definitely-missing-confit-binary))"
        ),
        "closed when skips: {lines:?}"
    );
}

#[test]
fn print_lines_keep_hook_order_as_data() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let (print_tx, print_rx) = crossbeam_channel::unbounded();
    let fake = confit_cli::hooks::FakeRunner::new(VecDeque::from([
        Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        }),
        Ok(confit_cli::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        }),
    ]));
    let probe = hook_probe();
    let mut seams = confit_cli::seams::Seams::memory(&fs, &probe, &mut input).with_print(print_tx);
    seams.hook_runner = Some(&fake);
    let mut runner = apply_runner(Vec::new(), Bundle::empty(), None, true, seams);
    runner.manifest = match Bundle::build(
        Vec::new(),
        vec![
            hook_for(&["tool", "first"], &["/fakebin"], None, vec![]),
            hook_for(&["tool", "second"], &["/fakebin"], None, vec![]),
        ],
    ) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let mut lines = Vec::new();
    while let Ok(line) = print_rx.try_recv() {
        lines.push(line);
    }
    let first = match lines.iter().position(|line| line.contains("hook 1 of 2")) {
        Some(index) => index,
        None => panic!("first hook prints"),
    };
    let second = match lines.iter().position(|line| line.contains("hook 2 of 2")) {
        Some(index) => index,
        None => panic!("second hook prints"),
    };
    assert!(first < second, "hook lines keep order");
}
