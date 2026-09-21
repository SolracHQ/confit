pub(crate) use std::io::Cursor;
pub(crate) use std::path::{Path, PathBuf};

pub(crate) use confit_core::document::{ManifestData, ManifestDocument};
pub(crate) use confit_core::drift::DriftOrder;
pub(crate) use confit_core::error::Error;
pub(crate) use confit_core::fs::memory::MemoryFs;
pub(crate) use confit_core::ids::{DocPath, ReadOutcome};
pub(crate) use confit_core::plan::{BUNDLE_VERSION, Bundle};

/// Points at the workspace examples folder from the cli crate dir.
pub(crate) fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Pins HOME to a shared temp folder so engine path joins stay hermetic.
///
/// Every test pins the same value, so parallel runs cannot diverge.
/// XDG vars pin under it too, since `dirs` honors them over HOME.
pub(crate) fn pin_home() -> PathBuf {
    let home = std::env::temp_dir().join("confit-cli-test-home");
    match std::fs::create_dir_all(&home) {
        Ok(()) => {}
        Err(error) => panic!("test home builds: {error}"),
    }
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
        std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    }
    home
}

/// Evaluates one profile file with no external plugins.
pub(crate) fn evaluate(profile: &Path, root: &Path) -> Result<Vec<ManifestDocument>, Error> {
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
pub(crate) fn fixture_fetch() -> std::sync::Arc<confit_engine::fetch::MemoryFetch> {
    let fake = std::sync::Arc::new(confit_engine::fetch::MemoryFetch::new());
    let archive = tar_gz_bytes(&[("mise/bin/mise", b"fixture-mise".as_slice(), 0o755)]);
    fake.insert(
        "https://github.com/jdx/mise/releases/download/v2026.9.10/mise-v2026.9.10-linux-x64.tar.gz",
        &archive,
    );
    fake
}

/// Builds tar.gz bytes from member triples in memory.
pub(crate) fn tar_gz_bytes(members: &[(&str, &[u8], u32)]) -> Vec<u8> {
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
pub(crate) fn evaluate_fetch(
    profile: &Path,
    root: &Path,
    cache: &Path,
    fetcher: std::sync::Arc<confit_engine::fetch::MemoryFetch>,
) -> Result<Vec<ManifestDocument>, Error> {
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

/// Builds a bundle off disk with an empty previous state.
pub(crate) fn build(documents: Vec<ManifestDocument>) -> Result<confit_core::plan::Bundle, Error> {
    let mut built = confit_core::plan::Bundle::build(documents, Vec::new())?;
    let blobs = sample_blobs();
    for (sha, bytes) in &blobs {
        if built
            .manifest
            .documents
            .iter()
            .any(|document| document.data.blob_refs().contains(&sha.as_str()))
        {
            built.blobs.insert(sha.clone(), bytes.clone());
        }
    }
    Ok(built)
}

/// Fills data hashes or panics with context.
pub(crate) fn fill_hashes(documents: &mut [ManifestDocument]) {
    for document in documents {
        if let Err(error) = document.fill_hash() {
            panic!("hashes fill: {error}");
        }
    }
}

/// Serializes one built manifest for comparison.
pub(crate) fn plan_value(built: &confit_core::plan::Bundle) -> serde_json::Value {
    match serde_json::to_value(confit_core::store::manifest::Manifest::of(built)) {
        Ok(value) => value,
        Err(error) => panic!("manifest serializes: {error}"),
    }
}

/// Writes one profile file into a temp root.
pub(crate) fn write_profile(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    match std::fs::write(&path, contents) {
        Ok(()) => {}
        Err(error) => panic!("profile writes: {error}"),
    }
    path
}

/// Reads the plan error message from a failing evaluation.
pub(crate) fn eval_error(profile: &str) -> String {
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
pub(crate) fn sample_documents() -> Vec<ManifestDocument> {
    use confit_core::document::{StructuredFormat, Table};

    vec![
        ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hello\n".to_string(),
                mode: None,
                unmanaged: false,
            },
        ),
        ManifestDocument::new(
            DocPath::new("app.toml"),
            ManifestData::Structured {
                format: StructuredFormat::Toml,
                data: Table::from([("name".to_string(), serde_json::json!("confit"))]),
            },
        ),
        ManifestDocument::new(
            DocPath::new("shortcut"),
            ManifestData::Link {
                target: "dest".to_string(),
            },
        ),
        ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: confit_core::ids::sha256_hex(&[0xFF, 0x00, 0x80, 0x41]),
                size: 4,
                mode: None,
                unmanaged: false,
            },
        ),
    ]
}

/// Builds the blob map backing the sample opaque document.
pub(crate) fn sample_blobs() -> std::collections::BTreeMap<String, Vec<u8>> {
    std::collections::BTreeMap::from([(
        confit_core::ids::sha256_hex(&[0xFF, 0x00, 0x80, 0x41]),
        vec![0xFF, 0x00, 0x80, 0x41],
    )])
}

/// Builds an apply runner over memory fakes.
pub(crate) fn apply_runner<'a>(
    desired: Vec<ManifestDocument>,
    previous: Bundle,
    state: Option<PathBuf>,
    force: bool,
    preview: bool,
    seams: confit_cli::seams::Seams<'a>,
) -> confit_cli::actions::apply::ApplyRunner<'a> {
    let manifest = match build(desired) {
        Ok(manifest) => manifest,
        Err(error) => panic!("bundle builds: {error}"),
    };
    confit_cli::actions::apply::ApplyRunner {
        manifest,
        previous,
        state,
        force,
        preview,
        seams,
    }
}

/// Reads memory bytes or panics with context.
pub(crate) fn memory_bytes(fs: &MemoryFs, path: &Path) -> Vec<u8> {
    use confit_core::fs::Filesystem;

    match fs.read(path) {
        Ok(bytes) => bytes,
        Err(error) => panic!("{} reads: {error}", path.display()),
    }
}

/// BufRead stub applying file edits on first read, simulating drift.
pub(crate) struct DriftInjector<'a> {
    /// Answer bytes behind the stub.
    pub(crate) inner: Cursor<Vec<u8>>,
    /// Edits landing on first read.
    pub(crate) edits: Option<Vec<(PathBuf, Vec<u8>)>>,
    /// Backend receiving the edits.
    pub(crate) fs: &'a MemoryFs,
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
pub(crate) fn hook_for(
    argv: &[&str],
    path: &[&str],
    when: Option<confit_core::condition::Condition>,
    checks: Vec<confit_core::condition::Condition>,
) -> confit_core::hook::Hook {
    confit_core::hook::Hook {
        argv: argv.iter().map(|item| item.to_string()).collect(),
        path: path.iter().map(|item| item.to_string()).collect(),
        requires: None,
        when,
        checks,
        timeout_secs: 600,
    }
}

/// Seeds one executable binary plus probe on a memory backend.
pub(crate) fn hook_fs() -> MemoryFs {
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
pub(crate) fn hook_runner<'a>(
    fs: &'a MemoryFs,
    input: &'a mut Cursor<Vec<u8>>,
    fake: &'a confit_cli::hooks::FakeRunner,
    log: Option<PathBuf>,
    hooks: Vec<confit_core::hook::Hook>,
) -> confit_cli::actions::apply::ApplyRunner<'a> {
    let mut seams = confit_cli::seams::Seams::memory(fs, input);
    seams.hook_runner = Some(fake);
    seams.log_file = log;
    let mut runner = apply_runner(Vec::new(), Bundle::empty(), None, true, false, seams);
    runner.manifest = match Bundle::build(Vec::new(), hooks) {
        Ok(bundle) => bundle,
        Err(error) => panic!("bundle builds: {error}"),
    };
    runner
}

/// Seeds the applied slot manifest plus pool on a memory backend.
pub(crate) fn seed_slot(fs: &MemoryFs, manifest: &Bundle) -> PathBuf {
    let slot = match confit_core::store::slots::default_state_path() {
        Ok(slot) => slot,
        Err(error) => panic!("slot resolves: {error}"),
    };
    match confit_core::store::slots::write_manifest(manifest, Some(&slot), fs, None) {
        Ok(()) => {}
        Err(error) => panic!("slot seeds: {error}"),
    }
    slot
}

/// Seeds one named slot manifest plus pool on a memory backend.
pub(crate) fn seed_named(fs: &MemoryFs, name: &str, manifest: &Bundle) -> PathBuf {
    let dest = match confit_core::store::slots::resolve_named_slot(name) {
        Ok(dest) => dest,
        Err(error) => panic!("named slot resolves: {error}"),
    };
    match confit_core::store::slots::write_manifest(manifest, Some(&dest), fs, None) {
        Ok(()) => {}
        Err(error) => panic!("named slot seeds: {error}"),
    }
    dest
}

/// Runs one export on memory seams.
pub(crate) fn run_export(
    fs: &MemoryFs,
    args: &confit_cli::cli::ExportArgs,
) -> Result<confit_cli::actions::export::ExportReport, Error> {
    let mut input = Cursor::new(String::new());
    confit_cli::actions::export::ExportRunner::run(
        args,
        confit_cli::seams::Seams::memory(fs, &mut input),
    )
}

/// Runs one delete on memory seams.
pub(crate) fn run_delete(
    fs: &MemoryFs,
    args: &confit_cli::cli::DeleteArgs,
) -> Result<confit_cli::actions::delete::DeleteReport, Error> {
    let mut input = Cursor::new(String::new());
    confit_cli::actions::delete::run(args, confit_cli::seams::Seams::memory(fs, &mut input))
}
