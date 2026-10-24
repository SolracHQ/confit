//! Shared fixtures for runtime unit tests.

use std::path::Path;

use confit_store::handles::ResourceHandle;
use confit_store::{StoreRoots, Stores};

/// Build stores over one temp folder.
pub(crate) fn test_stores(dir: &Path) -> Stores {
    let (sender, _) = crossbeam_channel::unbounded();
    Stores::new(
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        },
        sender,
    )
}

/// Land one project file and return its live handle.
pub(crate) fn project_handle(
    stores: &Stores,
    dir: &Path,
    name: &str,
    body: &[u8],
) -> ResourceHandle {
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(name), body).unwrap();
    match stores.resources().resource(name) {
        Ok(found) => found,
        Err(error) => panic!("project opens: {error}"),
    }
}
