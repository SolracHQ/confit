use std::io::Cursor;
use std::path::{Path, PathBuf};

use confit_core::document::{Document, DocumentData};
use confit_core::error::Error;
use confit_core::fs::MemoryFs;
use confit_core::ids::{DocPath, ReadOutcome};
use confit_core::plan::{PLAN_VERSION, Plan};

/// Points at the workspace examples folder from the cli crate dir.
fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Pins HOME to a shared temp folder so engine path joins stay hermetic.
///
/// Every test pins the same value, so parallel runs cannot diverge.
fn pin_home() -> PathBuf {
    let home = std::env::temp_dir().join("confit-cli-test-home");
    match std::fs::create_dir_all(&home) {
        Ok(()) => {}
        Err(error) => panic!("test home builds: {error}"),
    }
    unsafe {
        std::env::set_var("HOME", &home);
    }
    home
}

/// Evaluates one profile file with no external plugins.
fn evaluate(profile: &Path, root: &Path) -> Result<Vec<Document>, Error> {
    pin_home();
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root: root.to_path_buf(),
            plugins: root.join("plugins"),
            re_fetch: false,
            cache_dir: None,
            fetcher: None,
            progress: None,
        },
    )
    .map(|evaluation| evaluation.documents)
}

/// Builds one memory fetcher serving the fixture mise installer.
fn fixture_fetch() -> std::sync::Arc<confit_engine::fetch::MemoryFetch> {
    let fake = std::sync::Arc::new(confit_engine::fetch::MemoryFetch::new());
    let archive = tar_gz_bytes(&[("mise/bin/mise", b"fixture-mise".as_slice(), 0o755)]);
    fake.insert(
        "https://github.com/jdx/mise/releases/download/v2026.9.10/mise-v2026.9.10-linux-x64.tar.gz",
        &archive,
    );
    fake
}

/// Builds tar.gz bytes from member triples in memory.
fn tar_gz_bytes(members: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut builder = tar::Builder::new(&mut encoder);
        for (name, bytes, mode) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            if builder.append_data(&mut header, *name, *bytes).is_err() {
                panic!("archive member writes");
            }
        }
        if builder.finish().is_err() {
            panic!("archive finishes");
        }
    }
    match encoder.finish() {
        Ok(bytes) => bytes,
        Err(error) => panic!("gzip finishes: {error}"),
    }
}

/// Evaluates one profile file against stubbed fetches in an isolated cache.
fn evaluate_fetch(
    profile: &Path,
    root: &Path,
    cache: &Path,
    fetcher: std::sync::Arc<confit_engine::fetch::MemoryFetch>,
) -> Result<Vec<Document>, Error> {
    pin_home();
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root: root.to_path_buf(),
            plugins: root.join("plugins"),
            re_fetch: false,
            cache_dir: Some(cache.to_path_buf()),
            fetcher: Some(fetcher),
            progress: None,
        },
    )
    .map(|evaluation| evaluation.documents)
}

/// Builds a plan off disk with an empty previous state.
fn build(documents: Vec<Document>) -> Result<confit_core::plan::Plan, Error> {
    confit_core::plan::Plan::build(documents, Vec::new())
}

/// Fills data hashes or panics with context.
fn fill_hashes(documents: &mut [Document]) {
    for document in documents {
        if let Err(error) = document.fill_hash() {
            panic!("hashes fill: {error}");
        }
    }
}

/// Serializes one built plan with the timestamp blanked for comparison.
fn plan_value(built: &confit_core::plan::Plan) -> serde_json::Value {
    let mut value = match serde_json::to_value(built) {
        Ok(value) => value,
        Err(error) => panic!("plan serializes: {error}"),
    };
    if let Some(created) = value.get_mut("created_at") {
        *created = serde_json::Value::String(String::new());
    }
    value
}

/// Writes one profile file into a temp root.
fn write_profile(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    match std::fs::write(&path, contents) {
        Ok(()) => {}
        Err(error) => panic!("profile writes: {error}"),
    }
    path
}

/// Reads the plan error message from a failing evaluation.
fn eval_error(profile: &str) -> String {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let path = write_profile(dir.path(), "profile.lua", profile);
    let error = match evaluate(&path, dir.path()) {
        Ok(_) => panic!("profile passes"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Plan(_)));
    error.to_string()
}

/// Builds sample documents across text, structured, link, plus opaque kinds.
fn sample_documents() -> Vec<Document> {
    use confit_core::document::{StructuredFormat, Table};

    vec![
        Document::new(
            DocPath::new("note"),
            DocumentData::Text {
                content: "hello\n".to_string(),
                mode: None,
            },
        ),
        Document::new(
            DocPath::new("app.toml"),
            DocumentData::Structured {
                format: StructuredFormat::Toml,
                data: Table::from([("name".to_string(), serde_json::json!("confit"))]),
            },
        ),
        Document::new(
            DocPath::new("shortcut"),
            DocumentData::Link {
                target: "dest".to_string(),
            },
        ),
        Document::new(
            DocPath::new("bin"),
            DocumentData::Opaque {
                content: vec![0xFF, 0x00, 0x80, 0x41],
                mode: None,
            },
        ),
    ]
}

/// Builds an apply runner over memory fakes.
fn apply_runner<'a>(
    desired: Vec<Document>,
    previous: Plan,
    state: Option<PathBuf>,
    force: bool,
    preview: bool,
    seams: confit_cli::actions::seams::Seams<'a>,
) -> confit_cli::actions::apply::ApplyRunner<'a> {
    let plan = match Plan::build(desired, Vec::new()) {
        Ok(plan) => plan,
        Err(error) => panic!("plan builds: {error}"),
    };
    confit_cli::actions::apply::ApplyRunner {
        plan,
        previous,
        state,
        force,
        preview,
        seams,
    }
}

/// Reads memory bytes or panics with context.
fn memory_bytes(fs: &MemoryFs, path: &Path) -> Vec<u8> {
    use confit_core::fs::Filesystem;

    match fs.read(path) {
        Ok(bytes) => bytes,
        Err(error) => panic!("{} reads: {error}", path.display()),
    }
}

/// BufRead stub applying file edits on first read, simulating drift.
struct DriftInjector<'a> {
    /// Answer bytes behind the stub.
    inner: Cursor<Vec<u8>>,
    /// Edits landing on first read.
    edits: Option<Vec<(PathBuf, Vec<u8>)>>,
    /// Backend receiving the edits.
    fs: &'a MemoryFs,
}

impl std::io::Read for DriftInjector<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::BufRead for DriftInjector<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if let Some(edits) = self.edits.take() {
            use confit_core::fs::Filesystem;

            for (path, bytes) in edits {
                match self.fs.write(&path, &bytes) {
                    Ok(()) => {}
                    Err(error) => panic!("drift injects {}: {error}", path.display()),
                }
            }
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

/// Builds one hook for apply tests.
fn hook_for(
    argv: &[&str],
    path: &[&str],
    when: Option<confit_core::document::Condition>,
    checks: Vec<confit_core::document::Condition>,
) -> confit_core::hook::Hook {
    confit_core::hook::Hook {
        argv: argv.iter().map(|item| item.to_string()).collect(),
        path: path.iter().map(|item| item.to_string()).collect(),
        when,
        checks,
        timeout_secs: 600,
    }
}

/// Seeds one executable binary plus probe on a memory backend.
fn hook_fs() -> MemoryFs {
    use confit_core::fs::Filesystem;

    let fs = MemoryFs::new();
    match fs.write(Path::new("/fakebin/tool"), b"run") {
        Ok(()) => {}
        Err(error) => panic!("tool seeds: {error}"),
    }
    match fs.set_mode(Path::new("/fakebin/tool"), 0o755) {
        Ok(()) => {}
        Err(error) => panic!("tool mode seeds: {error}"),
    }
    match fs.write(Path::new("/fakebin/probe"), b"done") {
        Ok(()) => {}
        Err(error) => panic!("probe seeds: {error}"),
    }
    fs
}

/// Builds an apply runner carrying hooks plus a fake hook runner.
fn hook_runner<'a>(
    fs: &'a MemoryFs,
    input: &'a mut Cursor<Vec<u8>>,
    output: &'a mut Vec<u8>,
    fake: &'a confit_cli::actions::hooks::FakeRunner,
    log: Option<PathBuf>,
    hooks: Vec<confit_core::hook::Hook>,
) -> confit_cli::actions::apply::ApplyRunner<'a> {
    let mut seams = confit_cli::actions::seams::Seams::memory(fs, input, output);
    seams.hook_runner = Some(fake);
    seams.log_file = log;
    let mut runner = apply_runner(Vec::new(), Plan::empty(), None, true, false, seams);
    runner.plan = match Plan::build(Vec::new(), hooks) {
        Ok(plan) => plan,
        Err(error) => panic!("plan builds: {error}"),
    };
    runner
}

#[test]
fn hooks_run_spawn_resolve_and_log() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::from([Ok(
        confit_cli::actions::hooks::HookRun {
            code: 0,
            output: b"did\n".to_vec(),
        },
    )]));
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
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
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("hook 1 of 1: tool --flag"),
        "terminal line shows: {text}"
    );
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
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::new());
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            None,
            vec![confit_core::document::Condition::Exists {
                path: "/fakebin/probe".to_string(),
            }],
        )],
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert!(fake.calls().is_empty());
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("skipped: tool (checks pass)"),
        "skip line shows: {text}"
    );
}

#[test]
fn hooks_warn_on_closed_gates_without_spawning() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::new());
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            Some(confit_core::document::Condition::InPath {
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
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("warn: tool cannot run (in_path(definitely-missing-confit-binary))"),
        "warn line shows: {text}"
    );
}

#[test]
fn hooks_abort_on_first_failure() {
    use std::collections::VecDeque;

    pin_home();
    let fs = hook_fs();
    let mut input = Cursor::new(Vec::new());
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::from([
        Ok(confit_cli::actions::hooks::HookRun {
            code: 1,
            output: b"boom\n".to_vec(),
        }),
        Ok(confit_cli::actions::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        }),
    ]));
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
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
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::from([Err(
        confit_core::error::Error::Plan("hook 'tool' timed out after 600s".to_string()),
    )]));
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
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
    let mut output = Vec::new();
    let fake = confit_cli::actions::hooks::FakeRunner::new(VecDeque::from([Ok(
        confit_cli::actions::hooks::HookRun {
            code: 0,
            output: Vec::new(),
        },
    )]));
    let runner = hook_runner(
        &fs,
        &mut input,
        &mut output,
        &fake,
        None,
        vec![hook_for(
            &["tool"],
            &["/fakebin"],
            None,
            vec![confit_core::document::Condition::Exists {
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
fn apply_yes_writes_all_files() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new("yes\n");
    let mut output = Vec::new();
    let runner = apply_runner(
        sample_documents(),
        Plan::empty(),
        Some(PathBuf::from("state.json")),
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    let report = match runner.execute() {
        Ok(report) => report,
        Err(error) => panic!("apply runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let app = memory_bytes(&fs, Path::new("app.toml"));
    let app_text = match String::from_utf8(app) {
        Ok(text) => text,
        Err(error) => panic!("app parses: {error}"),
    };
    assert!(app_text.contains("confit"), "toml holds value: {app_text}");
    assert_eq!(
        memory_bytes(&fs, Path::new("bin")),
        vec![0xFF, 0x00, 0x80, 0x41]
    );
    assert_eq!(
        fs.read_link(Path::new("shortcut")),
        Some(PathBuf::from("dest"))
    );
    assert!(fs.exists(Path::new("state.json")));
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].created_at.is_empty());
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("Plan:"), "preview renders: {text}");
}

#[test]
fn apply_non_yes_writes_nothing() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    for answer in ["no\n", "YES\n", "\n"] {
        let mut input = Cursor::new(answer);
        let mut output = Vec::new();
        let runner = apply_runner(
            sample_documents(),
            Plan::empty(),
            Some(PathBuf::from("state.json")),
            false,
            true,
            confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
        );
        match runner.execute() {
            Ok(_) => panic!("{answer:?} proceeds"),
            Err(error) => assert!(
                error.to_string().contains("apply aborted"),
                "abort reads clear: {error}"
            ),
        }
    }
    assert!(!fs.exists(Path::new("note")));
    assert!(!fs.exists(Path::new("app.toml")));
    assert!(!fs.exists(Path::new("shortcut")));
    assert!(!fs.exists(Path::new("bin")));
    assert!(!fs.exists(Path::new("state.json")));
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert!(entries.is_empty());
}

#[test]
fn apply_force_skips_prompt() {
    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let runner = apply_runner(
        sample_documents(),
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("forced apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    assert_eq!(
        memory_bytes(&fs, Path::new("bin")),
        vec![0xFF, 0x00, 0x80, 0x41]
    );
}

#[test]
fn apply_plan_file_skips_preview() {
    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    match confit_core::store::write_plan(&built, Some(Path::new("plan.json")), &fs) {
        Ok(()) => {}
        Err(error) => panic!("plan writes: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        profile: Some(PathBuf::from("profile.lua")),
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        plan: Some(PathBuf::from("plan.json")),
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => {}
        Err(error) => panic!("plan-file apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    assert!(output.is_empty(), "preview skips reading");
}

#[test]
fn apply_drift_reprompts() {
    pin_home();
    let fs = MemoryFs::new();
    let mut seed_input = Cursor::new(String::new());
    let mut seed_output = Vec::new();
    let seed = apply_runner(
        sample_documents(),
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut seed_input, &mut seed_output),
    );
    match seed.execute() {
        Ok(_) => {}
        Err(error) => panic!("seed apply runs: {error}"),
    }
    let mut recorded = sample_documents();
    fill_hashes(&mut recorded);
    let mut previous = Plan::empty();
    previous.documents = recorded;

    let mut input = DriftInjector {
        inner: Cursor::new(b"yes\nno\n".to_vec()),
        edits: Some(vec![(PathBuf::from("note"), b"hand edit\n".to_vec())]),
        fs: &fs,
    };
    let mut output = Vec::new();
    let denied = apply_runner(
        sample_documents(),
        previous.clone(),
        None,
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match denied.execute() {
        Ok(_) => panic!("drifted apply proceeds on no"),
        Err(error) => assert!(
            error.to_string().contains("apply aborted"),
            "abort reads clear: {error}"
        ),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hand edit\n");
    let text = String::from_utf8_lossy(&output);
    assert_eq!(
        text.matches("Type 'yes' to continue").count(),
        2,
        "drift prompts again: {text}"
    );

    let mut input = DriftInjector {
        inner: Cursor::new(b"yes\nyes\n".to_vec()),
        edits: Some(vec![(
            PathBuf::from("app.toml"),
            b"name = \"meddled\"\n".to_vec(),
        )]),
        fs: &fs,
    };
    let mut output = Vec::new();
    let retry = apply_runner(
        sample_documents(),
        previous,
        None,
        false,
        true,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match retry.execute() {
        Ok(_) => {}
        Err(error) => panic!("confirmed drift applies: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let app = memory_bytes(&fs, Path::new("app.toml"));
    let app_text = match String::from_utf8(app) {
        Ok(text) => text,
        Err(error) => panic!("app parses: {error}"),
    };
    assert!(
        app_text.contains("confit"),
        "apply restores drift: {app_text}"
    );
}

#[test]
fn apply_rotation_drops_sixth() {
    pin_home();
    let fs = MemoryFs::new();
    for _ in 0..6 {
        let mut input = Cursor::new(String::new());
        let mut output = Vec::new();
        let runner = apply_runner(
            sample_documents(),
            Plan::empty(),
            None,
            true,
            false,
            confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
        );
        match runner.execute() {
            Ok(_) => {}
            Err(error) => panic!("apply runs: {error}"),
        }
    }
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 5);
}

#[test]
fn recover_roundtrips() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let mut seed_input = Cursor::new(String::new());
    let mut seed_output = Vec::new();
    let seed = apply_runner(
        sample_documents(),
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut seed_input, &mut seed_output),
    );
    match seed.execute() {
        Ok(_) => {}
        Err(error) => panic!("seed apply runs: {error}"),
    }
    match fs.write(Path::new("note"), b"hand edit\n") {
        Ok(()) => {}
        Err(error) => panic!("hand edit lands: {error}"),
    }
    let listing = confit_cli::cli::RecoverArgs {
        index: None,
        force: false,
    };
    let mut output = Vec::new();
    let mut listing_input = Cursor::new(String::new());
    let listing_runner = confit_cli::actions::recover::RecoverRunner {
        args: &listing,
        seams: confit_cli::actions::seams::Seams::memory(&fs, &mut listing_input, &mut output),
    };
    match listing_runner.execute() {
        Ok(None) => {}
        Ok(Some(_)) => panic!("listing applies"),
        Err(error) => panic!("listing runs: {error}"),
    }
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("0 @ "),
        "listing shows index plus timestamp: {text}"
    );
    assert!(
        !text.contains("profile.lua"),
        "listing carries no labels: {text}"
    );

    let pick = confit_cli::cli::RecoverArgs {
        index: Some(0),
        force: false,
    };
    let mut input = Cursor::new("yes\n");
    let mut output = Vec::new();
    let pick_runner = confit_cli::actions::recover::RecoverRunner {
        args: &pick,
        seams: confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    };
    match pick_runner.execute() {
        Ok(Some(_)) => {}
        Ok(None) => panic!("pick lists only"),
        Err(error) => panic!("recover applies: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot));
}

#[test]
fn recover_unknown_index_fails() {
    pin_home();
    let fs = MemoryFs::new();
    let pick = confit_cli::cli::RecoverArgs {
        index: Some(3),
        force: true,
    };
    let mut unknown_input = Cursor::new(String::new());
    let mut unknown_output = Vec::new();
    let unknown_runner = confit_cli::actions::recover::RecoverRunner {
        args: &pick,
        seams: confit_cli::actions::seams::Seams::memory(
            &fs,
            &mut unknown_input,
            &mut unknown_output,
        ),
    };
    match unknown_runner.execute() {
        Ok(_) => panic!("unknown index applies"),
        Err(error) => assert!(
            error.to_string().contains("out of range"),
            "range reads clear: {error}"
        ),
    }
}

#[test]
fn apply_opaque_bytes_land_identical() {
    pin_home();
    let fs = MemoryFs::new();
    let raw = vec![0xFF, 0x00, 0x80, 0x41, 0xFE];
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let runner = apply_runner(
        vec![Document::new(
            DocPath::new("bin"),
            DocumentData::Opaque {
                content: raw.clone(),
                mode: None,
            },
        )],
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert_eq!(memory_bytes(&fs, Path::new("bin")), raw);
}

#[test]
fn apply_link_lands_as_symlink() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let runner = apply_runner(
        vec![Document::new(
            DocPath::new("shortcut"),
            DocumentData::Link {
                target: "dest".to_string(),
            },
        )],
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    assert_eq!(
        fs.read_link(Path::new("shortcut")),
        Some(PathBuf::from("dest"))
    );
    assert!(fs.exists(Path::new("shortcut")));
}

#[test]
fn apply_then_drift_stays_quiet() {
    use confit_core::fs::{snapshot, snapshot_tree};

    pin_home();
    let fs = MemoryFs::new();
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let runner = apply_runner(
        sample_documents(),
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let mut recorded = sample_documents();
    fill_hashes(&mut recorded);
    let mut previous = Plan::empty();
    previous.documents = recorded;
    let drifts = previous.drift(&|path| snapshot(path, &fs), &|path| {
        snapshot_tree(&path.expand(), &fs)
    });
    assert!(drifts.is_empty(), "fresh apply shows no drift: {drifts:?}");
}

#[test]
fn fixture_plans_stay_deterministic() {
    for fixture in [
        "0-basic_tool",
        "1-structured_resource",
        "2-templated_resource",
    ] {
        let cache = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("cache builds: {error}"),
        };
        let root = examples_root().join(fixture);
        let profile = root.join("profile.lua");
        let first = match evaluate_fetch(&profile, &root, cache.path(), fixture_fetch()) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} first plan builds: {error}"),
            },
            Err(error) => panic!("{fixture} first evaluation runs: {error}"),
        };
        let second = match evaluate_fetch(&profile, &root, cache.path(), fixture_fetch()) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} second plan builds: {error}"),
            },
            Err(error) => panic!("{fixture} second evaluation runs: {error}"),
        };
        assert!(!first.documents.is_empty(), "{fixture} holds documents");
        assert_eq!(
            plan_value(&first),
            plan_value(&second),
            "{fixture} repeats exactly"
        );
    }
}

#[test]
fn registration_order_swap_yields_identical_bytes() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local first = confit.config("aaa")
first:add_document(confit.document.text("note-a", "alpha"))
first:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "aaa")
end))
local second = confit.config("zzz")
second:add_document(confit.document.text("note-z", "zeta"))
second:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "zzz")
end):priority(confit.priority.HIGH))
return { shells = { "bash" }, configs = { %s } }
"#;
    let forward = write_profile(
        dir.path(),
        "forward.lua",
        &body.replace("%s", "first, second"),
    );
    let swapped = write_profile(
        dir.path(),
        "swapped.lua",
        &body.replace("%s", "second, first"),
    );
    let run = |path: &Path| {
        let documents = match evaluate(path, dir.path()) {
            Ok(documents) => documents,
            Err(error) => panic!("profile evaluates: {error}"),
        };
        match build(documents) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        }
    };
    let first = run(&forward);
    let second = run(&swapped);
    assert_eq!(plan_value(&first), plan_value(&second));
    let shared = match serde_json::to_value(&first.documents) {
        Ok(value) => value,
        Err(error) => panic!("documents serialize: {error}"),
    };
    let slot = shared
        .as_array()
        .and_then(|items| items.iter().find(|item| item["path"] == "shared.json"))
        .and_then(|item| item["data"]["structured"]["data"]["slot"].as_str());
    assert!(matches!(slot, Some("zzz")));
}

#[test]
fn bad_format_fails_as_plan_error() {
    let message = eval_error(
        r#"
local c = confit.config("c")
c:add_patch(confit.patch.structured("ini", "app.ini", function(_) end))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(
        message.contains("must be one of"),
        "names the format rule: {message}"
    );
}

#[test]
fn repeat_config_fails_as_plan_error() {
    let message = eval_error(
        r#"
local first = confit.config("dup")
local second = confit.config("dup")
return { shells = { "bash" }, configs = { first, second } }
"#,
    );
    assert!(
        message.contains("already defined"),
        "names the repeat: {message}"
    );
}

#[test]
fn append_on_non_list_fails_as_plan_error() {
    let message = eval_error(
        r#"
local c = confit.config("c")
c:add_document(confit.document.structured("json", { path = "app.json", data = { name = "x" } }))
c:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("name", "y")
end))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(
        message.contains("non-list"),
        "names the shape fault: {message}"
    );
}

#[test]
fn memory_snapshot_covers_present_absent_unreadable() {
    use confit_core::fs::{Filesystem, MemoryFs, snapshot};

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
    use confit_core::fs::{Filesystem, MemoryFs, snapshot, snapshot_tree};

    pin_home();
    let mut recorded_docs = vec![
        Document::new(
            DocPath::new("app.toml"),
            DocumentData::Structured {
                format: StructuredFormat::Toml,
                data: Table::from([
                    ("name".to_string(), serde_json::json!("old")),
                    ("gone".to_string(), serde_json::json!("yes")),
                ]),
            },
        ),
        Document::new(
            DocPath::new("note"),
            DocumentData::Text {
                content: "hello\n".to_string(),
                mode: None,
            },
        ),
        Document::new(
            DocPath::new("vanished"),
            DocumentData::Text {
                content: "bye".to_string(),
                mode: None,
            },
        ),
    ];
    fill_hashes(&mut recorded_docs);
    let previous = Plan {
        version: PLAN_VERSION,
        documents: recorded_docs,
        created_at: String::new(),
        hooks: Vec::new(),
    };
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
    let drifts = previous.drift(&|path| snapshot(path, &fs), &|path| {
        snapshot_tree(&path.expand(), &fs)
    });
    let built = confit_core::plan::Plan {
        version: confit_core::plan::PLAN_VERSION,
        documents: Vec::new(),
        created_at: String::new(),
        hooks: Vec::new(),
    };
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &previous,
        drift: &drifts,
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

    let mut old_docs = vec![Document::new(
        DocPath::new("app.toml"),
        DocumentData::Structured {
            format: StructuredFormat::Toml,
            data: Table::from([("name".to_string(), serde_json::json!("old"))]),
        },
    )];
    fill_hashes(&mut old_docs);
    let previous = Plan {
        version: PLAN_VERSION,
        documents: old_docs,
        created_at: String::new(),
        hooks: Vec::new(),
    };
    let desired = vec![Document::new(
        DocPath::new("app.toml"),
        DocumentData::Structured {
            format: StructuredFormat::Toml,
            data: Table::from([("name".to_string(), serde_json::json!("new"))]),
        },
    )];
    let built = match confit_core::plan::Plan::build(desired, Vec::new()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    assert_eq!(built.summary(&previous).update, 1);
    let report = confit_cli::presentation::summary::Summary {
        built: &built,
        previous: &previous,
        drift: &[],
    };
    let text = report.render();
    assert!(
        text.contains("~ name = old -> new"),
        "update shows old to new: {text}"
    );
}

#[test]
fn plan_file_feeds_state_roundtrip() {
    use confit_core::fs::{Filesystem, MemoryFs};

    let built = match build(vec![Document::new(
        DocPath::new("note"),
        DocumentData::Text {
            content: "hi".to_string(),
            mode: None,
        },
    )]) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    let text = match serde_json::to_string_pretty(&built) {
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
    let state = match confit_core::store::load_state(Some(Path::new("state.json")), &fs) {
        Ok(state) => state,
        Err(error) => panic!("plan feeds state: {error}"),
    };
    assert_eq!(state.documents.len(), 1);
    assert_eq!(state.version, PLAN_VERSION);
    assert!(!state.documents[0].data_hash.is_empty());
    let rebuilt = match confit_core::plan::Plan::build(
        vec![Document::new(
            DocPath::new("note"),
            DocumentData::Text {
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
    match fs.write(
        Path::new("state.json"),
        b"{\"version\":1,\"documents\":[],\"created_at\":\"\"}",
    ) {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match confit_core::store::load_state(Some(Path::new("state.json")), &fs) {
        Ok(_) => panic!("stale version passes"),
        Err(error) => assert_eq!(
            error.to_string(),
            format!("state version 1 reads unsupported, want {PLAN_VERSION}")
        ),
    }
}

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
    assert_eq!(report.written, 14);
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
        DocumentData::Rc(data) => {
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
            assert_eq!(data.final_entries.len(), 1);
            match &data.final_entries[0].op {
                RcOp::Eval { argv } => assert_eq!(argv[0], "starship"),
                other => panic!("eval expected, got {other:?}"),
            }
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

#[test]
fn missing_state_version_fails_as_plan_error() {
    use confit_core::fs::{Filesystem, MemoryFs};

    let fs = MemoryFs::new();
    match fs.write(Path::new("state.json"), b"{\"documents\":[]}") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    match confit_core::store::load_state(Some(Path::new("state.json")), &fs) {
        Ok(_) => panic!("missing version passes"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains("version"), "version named: {message}");
            assert!(message.contains("state.json"), "names file: {message}");
        }
    }
}

#[test]
fn apply_second_profile_removes_recorded_orphans() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    if let Err(error) = fs.write(Path::new("stray"), b"mine") {
        panic!("stray writes: {error}");
    }
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let first = apply_runner(
        sample_documents(),
        Plan::empty(),
        Some(PathBuf::from("state.json")),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    match first.execute() {
        Ok(_) => {}
        Err(error) => panic!("first apply runs: {error}"),
    }
    if let Err(error) = fs.remove(Path::new("bin")) {
        panic!("bin pre-deletes: {error}");
    }
    let previous = match confit_core::store::load_state(Some(Path::new("state.json")), &fs) {
        Ok(previous) => previous,
        Err(error) => panic!("state loads: {error}"),
    };
    let desired = vec![
        Document::new(
            DocPath::new("note"),
            DocumentData::Text {
                content: "hello again\n".to_string(),
                mode: None,
            },
        ),
        Document::new(
            DocPath::new("app.toml"),
            DocumentData::Structured {
                format: confit_core::document::StructuredFormat::Toml,
                data: confit_core::document::Table::from([(
                    "name".to_string(),
                    serde_json::json!("confit"),
                )]),
            },
        ),
    ];
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let second = apply_runner(
        desired,
        previous,
        Some(PathBuf::from("state.json")),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output),
    );
    let report = match second.execute() {
        Ok(report) => report,
        Err(error) => panic!("second apply runs: {error}"),
    };
    assert_eq!(report.removed, 1);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello again\n");
    assert!(!fs.exists(Path::new("shortcut")), "link orphan removes");
    assert!(!fs.exists(Path::new("bin")), "absent orphan stays quiet");
    assert_eq!(memory_bytes(&fs, Path::new("stray")), b"mine");
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
    let plan_runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: true,
        progress: None,
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
    let text = match std::fs::read_to_string(&stored) {
        Ok(text) => text,
        Err(error) => panic!("tmp plan reads: {error}"),
    };
    let plan: Plan = match serde_json::from_str(&text) {
        Ok(plan) => plan,
        Err(error) => panic!("tmp plan parses: {error}"),
    };
    assert_eq!(plan.documents.len(), 1);
    match std::fs::remove_file(&stored) {
        Ok(()) => {}
        Err(error) => panic!("tmp plan cleans: {error}"),
    }
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
    let slot = match confit_core::store::default_state_path() {
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
        "version": PLAN_VERSION,
        "documents": [
            {"path": "tmp-slot-note", "data": {"text": {"content": "old\n"}}, "data_hash": ""}
        ],
        "created_at": "",
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
    let slot_runner = confit_cli::actions::plan::PlanRunner {
        args: &args,
        store_tmp: false,
        progress: None,
    };
    let outcome = match slot_runner.execute() {
        Ok(outcome) => outcome,
        Err(error) => panic!("plan runs: {error}"),
    };
    assert_eq!(outcome.previous.documents.len(), 1);
    assert_eq!(outcome.built.summary(&outcome.previous).update, 1);
    match std::fs::remove_file(&slot) {
        Ok(()) => {}
        Err(error) => panic!("slot cleans: {error}"),
    }
}

#[test]
fn apply_emits_writing_plan_fact() {
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let inner = seen.clone();
    let sink: confit_engine::ProgressCallback = std::sync::Arc::new(move |event| {
        if let Ok(mut guard) = inner.lock() {
            guard.push(event);
        }
    });
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let runner = apply_runner(
        sample_documents(),
        Plan::empty(),
        None,
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output).with_progress(sink),
    );
    match runner.execute() {
        Ok(_) => {}
        Err(error) => panic!("apply runs: {error}"),
    }
    let guard = match seen.lock() {
        Ok(guard) => guard,
        Err(error) => panic!("events read: {error}"),
    };
    assert!(
        guard.iter().any(|event| matches!(
            event,
            confit_engine::ProgressEvent::WritingPlan { documents: 4 }
        )),
        "writing plan fires with document count"
    );
    assert!(
        !guard
            .iter()
            .any(|event| matches!(event, confit_engine::ProgressEvent::ReadingPlan { .. })),
        "memory apply reads no plan files"
    );
}

#[test]
fn apply_plan_file_without_profile_runs_on_file_alone() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let built = match build(sample_documents()) {
        Ok(built) => built,
        Err(error) => panic!("plan builds: {error}"),
    };
    match confit_core::store::write_plan(&built, Some(Path::new("plan.json")), &fs) {
        Ok(()) => {}
        Err(error) => panic!("plan writes: {error}"),
    }
    let args = confit_cli::cli::ApplyArgs {
        profile: None,
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,

            re_fetch: false,
        },
        plan: Some(PathBuf::from("plan.json")),
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("plan-file apply runs: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(report.removed, 0);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot), "plan file apply writes fixed slot only");
    let entries = match confit_core::store::list_previous(&fs) {
        Ok(entries) => entries,
        Err(error) => panic!("previous lists: {error}"),
    };
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].created_at.is_empty());
}

#[test]
fn apply_without_plan_nor_profile_fails() {
    use std::io::Cursor;

    pin_home();
    let fs = MemoryFs::new();
    let args = confit_cli::cli::ApplyArgs {
        profile: None,
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,

            re_fetch: false,
        },
        plan: None,
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output);
    match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(_) => panic!("planless apply passes"),
        Err(error) => assert!(
            error.to_string().contains("--profile"),
            "names the missing flag: {error}"
        ),
    }
}

#[test]
fn two_profiles_share_one_slot_last_applied_wins() {
    use confit_core::fs::Filesystem;

    pin_home();
    let fs = MemoryFs::new();
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    let mut first_input = Cursor::new(String::new());
    let mut first_output = Vec::new();
    let first = apply_runner(
        vec![Document::new(
            DocPath::new("first"),
            DocumentData::Text {
                content: "one\n".to_string(),
                mode: None,
            },
        )],
        Plan::empty(),
        Some(slot.clone()),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut first_input, &mut first_output),
    );
    match first.execute() {
        Ok(_) => {}
        Err(error) => panic!("first apply runs: {error}"),
    }
    let previous = match confit_core::store::load_state(Some(&slot), &fs) {
        Ok(previous) => previous,
        Err(error) => panic!("slot loads: {error}"),
    };
    assert_eq!(previous.documents.len(), 1);
    let mut second_input = Cursor::new(String::new());
    let mut second_output = Vec::new();
    let second = apply_runner(
        vec![Document::new(
            DocPath::new("second"),
            DocumentData::Text {
                content: "two\n".to_string(),
                mode: None,
            },
        )],
        previous,
        Some(slot.clone()),
        true,
        false,
        confit_cli::actions::seams::Seams::memory(&fs, &mut second_input, &mut second_output),
    );
    let report = match second.execute() {
        Ok(report) => report,
        Err(error) => panic!("second apply runs: {error}"),
    };
    assert_eq!(report.removed, 1);
    assert!(
        !fs.exists(Path::new("first")),
        "orphan from first run removes"
    );
    assert_eq!(memory_bytes(&fs, Path::new("second")), b"two\n");
    let slot_plan = match confit_core::store::load_state(Some(&slot), &fs) {
        Ok(slot_plan) => slot_plan,
        Err(error) => panic!("slot reloads: {error}"),
    };
    assert_eq!(slot_plan.documents.len(), 1);
    assert_eq!(slot_plan.documents[0].path, DocPath::new("second"));
}

#[test]
fn named_plan_output_roundtrips_through_apply() {
    use confit_core::fs::Filesystem;
    use std::io::Cursor;

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
    assert!(dest.ends_with("confit/plans/work.json"));
    match confit_core::store::write_plan(&built, Some(&dest), &fs) {
        Ok(()) => {}
        Err(error) => panic!("named plan writes: {error}"),
    }
    let reloaded = match confit_core::store::load_state(Some(&dest), &fs) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("named plan loads: {error}"),
    };
    assert_eq!(reloaded.documents.len(), 4);
    let args = confit_cli::cli::ApplyArgs {
        profile: None,
        shared: confit_cli::cli::SharedArgs {
            root: None,
            plugins: None,
            re_fetch: false,
        },
        plan: Some(PathBuf::from("@work")),
        force: true,
    };
    let mut input = Cursor::new(String::new());
    let mut output = Vec::new();
    let seams = confit_cli::actions::seams::Seams::memory(&fs, &mut input, &mut output);
    let report = match confit_cli::actions::apply::ApplyRunner::run(&args, seams) {
        Ok(report) => report,
        Err(error) => panic!("named plan applies: {error}"),
    };
    assert_eq!(report.written, 4);
    assert_eq!(memory_bytes(&fs, Path::new("note")), b"hello\n");
    let slot = match confit_core::store::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    assert!(fs.exists(&slot), "named apply writes fixed slot");
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
fn explicit_plan_path_passes_through() {
    pin_home();
    let path = match confit_cli::cli::resolve_plan_file(Path::new("plan.json")) {
        Ok(path) => path,
        Err(error) => panic!("explicit path resolves: {error}"),
    };
    assert_eq!(path, PathBuf::from("plan.json"));
}
