use crate::common::*;

#[test]
fn init_scaffold_evaluates_to_one_rc() {
    use confit_core::document::RcOp;
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::InitArgs {
        dir: PathBuf::from("project"),
    };
    let init_runner = confit_cli::actions::init::InitRunner {
        args: &args,
        fs: &fs,
    };
    let report = match init_runner.execute() {
        Ok(report) => report,
        Err(error) => panic!("init runs: {error}"),
    };
    assert_eq!(report.profile, PathBuf::from("project/profile.lua"));
    assert_eq!(report.written, 15);
    let stubs = [
        "project/stubs/confit.d.lua",
        "project/stubs/namespaces/config.d.lua",
        "project/stubs/namespaces/document.d.lua",
        "project/stubs/namespaces/hook.d.lua",
        "project/stubs/namespaces/patch.d.lua",
        "project/stubs/namespaces/paths.d.lua",
        "project/stubs/namespaces/plugin.d.lua",
        "project/stubs/namespaces/resources.d.lua",
        "project/stubs/namespaces/runtime.d.lua",
        "project/stubs/namespaces/utils.d.lua",
        "project/plugins/solrachq/mise/plugin.d.lua",
        "project/plugins/solrachq/nerd_fonts/plugin.d.lua",
        "project/plugins/solrachq/merge/plugin.d.lua",
        "project/plugins/solrachq/template/plugin.d.lua",
    ];
    for stub in stubs {
        assert!(
            fs.exists(Path::new(stub)),
            "{stub} lands beside the profile"
        );
    }
    let global = memory_bytes(&fs, Path::new("project/stubs/confit.d.lua"));
    let global_text = match String::from_utf8(global) {
        Ok(text) => text,
        Err(error) => panic!("global stub parses: {error}"),
    };
    assert!(
        global_text.contains("confit = Confit"),
        "global stub ships the shared shapes"
    );

    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let profile = dir.path().join("profile.lua");
    match std::fs::write(
        &profile,
        memory_bytes(&fs, Path::new("project/profile.lua")),
    ) {
        Ok(()) => {}
        Err(error) => panic!("profile stages: {error}"),
    }
    let documents = match evaluate(&profile, dir.path()) {
        Ok(documents) => documents,
        Err(error) => panic!("scaffold evaluates: {error}"),
    };
    assert_eq!(documents.len(), 1);
    match &documents[0].data {
        ManifestData::Rc(data) => {
            assert_eq!(data.profile.len(), 1);
            match &data.profile[0].op {
                RcOp::Path { dir, .. } => assert!(dir.ends_with(".local/bin")),
                other => panic!("path expected, got {other:?}"),
            }
            assert_eq!(data.config.len(), 1);
            match &data.config[0].op {
                RcOp::Alias { name, .. } => assert_eq!(name, "ll"),
                other => panic!("alias expected, got {other:?}"),
            }
            assert!(data.final_entries.is_empty());
        }
        other => panic!("rc expected, got {other:?}"),
    }
}

#[test]
fn init_second_run_fails() {
    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::InitArgs {
        dir: PathBuf::from("project"),
    };
    let first_runner = confit_cli::actions::init::InitRunner {
        args: &args,
        fs: &fs,
    };
    match first_runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("first init runs: {error}"),
    }
    let second_runner = confit_cli::actions::init::InitRunner {
        args: &args,
        fs: &fs,
    };
    match second_runner.execute() {
        Ok(_) => panic!("second init passes"),
        Err(error) => assert!(
            error.to_string().contains("already exists"),
            "collision reads clear: {error}"
        ),
    }
}

#[test]
fn init_existing_stubs_fail_without_writes() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    match fs.write(
        Path::new("project/plugins/solrachq/mise/plugin.d.lua"),
        b"stale",
    ) {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    let args = confit_cli::cli::InitArgs {
        dir: PathBuf::from("project"),
    };
    let clash_runner = confit_cli::actions::init::InitRunner {
        args: &args,
        fs: &fs,
    };
    match clash_runner.execute() {
        Ok(_) => panic!("clashing init passes"),
        Err(error) => assert!(
            error.to_string().contains("already exists"),
            "collision reads clear: {error}"
        ),
    }
    assert!(
        !fs.exists(Path::new("project/profile.lua")),
        "clashing init writes nothing"
    );
}
