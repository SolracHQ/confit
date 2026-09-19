use std::path::PathBuf;
use std::sync::Arc;

use confit_core::error::Result;
use confit_core::progress::Event;
use confit_engine::{EvalOpts, evaluate};

/// Writes files plus profile into a temp root.
fn project(files: &[(&str, &[u8])], profile: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    for (name, contents) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            panic!("mkdirs build: {error}");
        }
        if let Err(error) = std::fs::write(&path, contents) {
            panic!("file writes: {error}");
        }
    }
    let profile_path = dir.path().join("profile.lua");
    if let Err(error) = std::fs::write(&profile_path, profile) {
        panic!("profile writes: {error}");
    }
    (dir, profile_path)
}

/// Drains one receiver into a vector in receive order.
fn drain(receiver: &crossbeam_channel::Receiver<Event>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
}

/// Builds one memory fetcher holding a single stub.
fn stubbed(url: &str, body: &[u8]) -> Arc<confit_engine::fetch::MemoryFetch> {
    let fake = Arc::new(confit_engine::fetch::MemoryFetch::new());
    fake.insert(url, body);
    fake
}

#[test]
fn fetch_download_reports_started_plus_downloaded_plus_patch() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/version";
    let profile = r#"
local v = confit.resources.fetch_text("https://example.com/version")
local alpha = confit.config("alpha")
alpha:add_document(confit.document.structured("json", { path = "app.json", data = { v = v } }))
alpha:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:set("touched", true)
end))
return { shells = { "bash" }, configs = { alpha } }
"#;
    let (dir, profile_path) = project(&[], profile);
    let (sender, receiver) = crossbeam_channel::unbounded();
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: Some(cache.path().to_path_buf()),
            fetcher: Some(stubbed(url, b"1.2.3")),
            progress: Some(sender),
        },
    );
    match outcome {
        Ok(_) => {}
        Err(error) => panic!("profile evaluates: {error}"),
    }
    std::mem::drop(dir);
    let events = drain(&receiver);
    let started = events
        .iter()
        .position(|event| matches!(event, Event::FetchStarted { url: seen } if seen == url));
    let downloaded = events.iter().position(
        |event| matches!(event, Event::FetchDownloaded { url: seen, bytes: 5 } if seen == url),
    );
    let patched = events.iter().position(|event| {
        matches!(event, Event::PatchApplied { owner, target, done: 1, total: 1 }
                if owner == "alpha" && target == "app.json")
    });
    match (started, downloaded, patched) {
        (Some(first), Some(second), Some(third)) => {
            assert!(first < second);
            assert!(second < third);
        }
        _ => panic!("fetch plus patch events missing: {events:?}"),
    }
}

#[test]
fn fetch_cache_hit_reports_cached_without_download() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/version";
    let profile = r#"
local v = confit.resources.fetch_text("https://example.com/version")
return {
  shells = { "bash" },
  documents = { confit.document.text("v", v) },
  configs = { confit.config("tool") },
}
"#;
    let (first_dir, first_profile) = project(&[], profile);
    let first = evaluate(
        &first_profile,
        EvalOpts {
            root: first_dir.path().to_path_buf(),
            plugins: first_dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: Some(cache.path().to_path_buf()),
            fetcher: Some(stubbed(url, b"1.2.3")),
            progress: None,
        },
    );
    match first {
        Ok(_) => {}
        Err(error) => panic!("first fetch runs: {error}"),
    }
    std::mem::drop(first_dir);
    let (second_dir, second_profile) = project(&[], profile);
    let (sender, receiver) = crossbeam_channel::unbounded();
    let empty = Arc::new(confit_engine::fetch::MemoryFetch::new());
    let second: Result<confit_engine::Evaluation> = evaluate(
        &second_profile,
        EvalOpts {
            root: second_dir.path().to_path_buf(),
            plugins: second_dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: Some(cache.path().to_path_buf()),
            fetcher: Some(empty),
            progress: Some(sender),
        },
    );
    match second {
        Ok(_) => {}
        Err(error) => panic!("cached fetch runs: {error}"),
    }
    std::mem::drop(second_dir);
    let events = drain(&receiver);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::FetchCached { url: seen, bytes: 5 } if seen == url
        )),
        "cached event missing: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::FetchDownloaded { .. })),
        "download fires on hit: {events:?}"
    );
}

#[test]
fn unpack_reports_kept_over_total() {
    let members: &[(&str, &[u8], u32)] = &[
        ("fonts/Regular.ttf", b"ttfdata".as_slice(), 0o644),
        ("fonts/readme.txt", b"readme".as_slice(), 0o644),
    ];
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
    let archive = match encoder.finish() {
        Ok(bytes) => bytes,
        Err(error) => panic!("gzip finishes: {error}"),
    };
    let profile = r#"
local kept = confit.document.compressed("fonts.tar.gz", function(path, info, content)
  if path:find("%.ttf$") then
    return confit.document.text("/fonts/" .. path, content)
  end
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let (dir, profile_path) = project(&[("fonts.tar.gz", archive.as_slice())], profile);
    let (sender, receiver) = crossbeam_channel::unbounded();
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: None,
            fetcher: None,
            progress: Some(sender),
        },
    );
    match outcome {
        Ok(_) => {}
        Err(error) => panic!("profile evaluates: {error}"),
    }
    std::mem::drop(dir);
    let events = drain(&receiver);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::Unpacked { archive, kept: 1, total: 2 } if archive == "fonts.tar.gz"
        )),
        "unpack event missing: {events:?}"
    );
}
