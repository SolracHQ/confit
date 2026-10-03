use std::path::PathBuf;

use confit_surface_api::definitions;
use confit_surface_face::stubs;

fn main() {
    if let Err(error) = run() {
        eprintln!("stubs gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/stubs"));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let (sender, _) = crossbeam_channel::unbounded();
    let stores = confit_store::Stores::new(
        confit_store::StoreRoots {
            config_base: scratch.path().join("config"),
            cache_base: scratch.path().join("cache"),
            temp_base: scratch.path().join("temp"),
            project_root: scratch.path().to_path_buf(),
        },
        sender,
    );
    let lua = mlua::Lua::new();
    let live = definitions(&lua, &stores, scratch.path()).map_err(|error| error.to_string())?;
    let written = stubs::write(&dir, &live).map_err(|error| error.to_string())?;
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}
