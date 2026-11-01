//! Shared builders for cli integration tests.

#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use confit_driver::fs;
use confit_store::{StoreRoots, Stores};
use confit_ui::{FromUi, ToUi, UiHandles};

/// Build stores over one temp dir with silent progress.
pub fn test_stores(dir: &Path, project: &str) -> Stores {
    let (sender, _) = crossbeam_channel::unbounded::<ToUi>();
    Stores::new(
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join(project),
        },
        sender,
    )
}

/// Build silent handles.
pub fn test_handles() -> UiHandles {
    let (to_ui, _) = crossbeam_channel::unbounded::<ToUi>();
    let (_, from_ui) = crossbeam_channel::unbounded::<FromUi>();
    UiHandles { to_ui, from_ui }
}

/// Build handles replying with the canned answers.
pub fn replying_handles(replies: Vec<String>) -> UiHandles {
    let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
    let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
    std::thread::spawn(move || {
        let mut replies = replies.into_iter();
        for message in incoming {
            if matches!(message, ToUi::Prompt(_)) {
                let line = replies.next().unwrap_or_default();
                if outgoing.send(FromUi::Answer(line)).is_err() {
                    break;
                }
            }
        }
    });
    UiHandles { to_ui, from_ui }
}

/// Write one file with parent folders first.
pub fn seed_file(path: &Path, text: &[u8]) {
    fs::create_parent(path).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Copy one host tree under a temp folder.
pub fn seed_tree(host: &Path, dest: &Path) {
    let mut stack = vec![host.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let source = entry.path();
            let file_type = entry.file_type().unwrap();
            let target = dest.join(source.strip_prefix(host).unwrap());
            if file_type.is_dir() {
                std::fs::create_dir_all(&target).unwrap();
                stack.push(source);
            } else if file_type.is_file() {
                let bytes = std::fs::read(&source).unwrap();
                seed_file(&target, &bytes);
            }
        }
    }
}

/// Resolve one repo path beside the workspace.
pub fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}
