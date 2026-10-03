//! Definitions
//!
//! Live stub definitions. Every entry names the
//! registered type behind one module file.

use std::path::Path;

use mlua::Lua;
use mlua_extras::typed::generator::Definition;

use confit_store::Stores;

/// Builds the stub definitions from the live modules.
pub fn definitions(
    lua: &Lua,
    stores: &Stores,
    root: &Path,
) -> mlua::Result<Vec<(String, Definition)>> {
    let (_, path) = crate::paths::build(lua)?;
    let (_, resource) = crate::resource::build(lua, stores, root)?;
    let (_, config) = crate::config::build(lua)?;
    let (_, fetch) = crate::fetch::build(lua, stores)?;
    Ok(vec![
        ("confit.path".to_owned(), path.into()),
        ("confit.resource".to_owned(), resource.into()),
        ("confit.config".to_owned(), config.into()),
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
