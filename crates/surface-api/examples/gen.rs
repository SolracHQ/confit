use std::path::{Path, PathBuf};

use mlua::Lua;
use mlua_extras::typed::generator::Definition;

use confit_store::Stores;
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

fn definitions(lua: &Lua, stores: &Stores, root: &Path) -> mlua::Result<Vec<(String, Definition)>> {
    let (_, resource) = confit_surface_api::resource::build(lua, stores, root)?;
    let (_, validate) = confit_surface_api::validate::build(lua)?;
    let (_, fetch) = confit_surface_api::fetch::build(lua, stores)?;
    Ok(vec![
        ("confit.resource".to_owned(), resource.into()),
        ("confit.validate".to_owned(), validate.into()),
        ("confit.fetch".to_owned(), fetch.into()),
        ("confit.version".to_owned(), version_definition()),
    ])
}

fn version_definition() -> Definition {
    Definition::start()
        .document("Api level of the Lua surface, one integer.")
        .proxy::<i64>("version")
        .finish()
}
