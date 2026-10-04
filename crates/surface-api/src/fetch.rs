//! Urls opened into resource handles.

use mlua::{FromLua, Lua, LuaSerdeExt, Value};
use mlua_extras::typed::generator::DefinitionBuilder;
use schemars::JsonSchema;
use serde::Deserialize;

use confit_model::sha::Sha;
use confit_store::Stores;
use confit_store::fetch::error::FetchError;
use confit_surface_face::module::{Module, VerbDecl};

use crate::error::ApiError;
use crate::handles::Handle;

/// Fetch options holding the digest pin.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchOpts {
    /// The pinned sha256 hex digest.
    #[serde(default)]
    pub sha256: Option<String>,
}

confit_surface_face::typed!(FetchOpts);

impl FromLua for FetchOpts {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

/// Builds the fetch function with its face, opening urls into handles.
pub fn build(lua: &Lua, stores: &Stores) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Opened urls behind resource handles.")?;
    module.register::<Handle>();
    module.register::<FetchOpts>();
    let stores = stores.clone();
    module.verb::<(String, Option<FetchOpts>), (Option<Handle>, Option<String>), _>(
        lua,
        VerbDecl {
            name: "fetch",
            documentation: "Opens one url into its handle.",
            params: &[
                ("url", "The url to fetch."),
                ("opts", "The digest pin table."),
            ],
            rets: &[
                "The resource handle, else nil.",
                "The failure reason, else nil.",
            ],
        },
        move |lua, (url, opts)| fetch_impl(lua, &stores, url, opts),
    )?;
    let (table, face) = module.finish();
    Ok((Value::Function(table.get("fetch")?), face))
}

/// Opens one url into its resource handle.
///
/// Dead urls return nil with reason and pin mismatches raise.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string urls and misshaped pins.
/// - [`ApiError::Fetch`] for pin mismatches.
/// - [`ApiError::Open`] for cache containment failures.
fn fetch_impl(
    _lua: &Lua,
    stores: &Stores,
    raw: String,
    opts: Option<FetchOpts>,
) -> mlua::Result<(Option<Handle>, Option<String>)> {
    if raw.is_empty() {
        return Err(ApiError::mismatch("url", "a non-empty url", "empty string").into());
    }
    let wanted = pin_of(opts)?;
    match stores.fetch().fetch(&raw, wanted, false) {
        Ok(found) => {
            let opened = stores.resources().fetched(&found).map_err(ApiError::Open)?;
            Ok((Some(Handle::new(opened, stores.clone())), None))
        }
        Err(error @ FetchError::DigestMismatch { .. }) => Err(ApiError::Fetch(error).into()),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

/// Resolves the digest pin from the options value.
///
/// - Missing options read none.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for bad digests.
fn pin_of(opts: Option<FetchOpts>) -> mlua::Result<Option<Sha>> {
    match opts.and_then(|found| found.sha256) {
        None => Ok(None),
        Some(digest) => open_pin(&digest),
    }
}

/// Opens one digest pin.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-hex digests.
fn open_pin(digest: &str) -> mlua::Result<Option<Sha>> {
    match Sha::new(digest) {
        Ok(found) => Ok(Some(found)),
        Err(_) => Err(ApiError::mismatch("sha256", "a 64-char hex digest", digest).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use confit_driver::fs::TestGuard;
    use confit_model::sha::Sha;
    use confit_store::{StoreRoots, Stores};

    const URL: &str = "https://example.com/tool.bin";

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                cache_base: dir.join("cache"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let fetch = crate::api::builtin(&lua, "confit.fetch").unwrap();
        lua.globals().set("fetch", fetch).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn serves_scripted_bodies_reading_text() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        confit_driver::http::clear();
        confit_driver::http::serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(fetch("https://example.com/tool.bin"))
            return assert(handle:text()) .. "|" .. handle:sha()
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(
                found.to_string_lossy(),
                format!("1.2.3|{}", Sha::hash(b"1.2.3").hex())
            ),
            Ok(_) => panic!("fetch text returns a string"),
            Err(error) => panic!("fetch opens its url: {error}"),
        }
    }

    #[test]
    fn dead_urls_return_nil_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        confit_driver::http::clear();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local handle, reason = fetch("https://example.com/tool.bin")
            assert(handle == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                assert!(
                    reason.to_string_lossy().contains(URL),
                    "reason names the url: {reason:?}"
                )
            }
            Ok(_) => panic!("dead url returns nil with reason"),
            Err(error) => panic!("dead urls stay soft: {error}"),
        }
    }

    #[test]
    fn sha_pins_match_passing() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        confit_driver::http::clear();
        confit_driver::http::serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let pin = Sha::hash(b"1.2.3").hex();
        let source = format!(
            r#"local handle = assert(fetch("{URL}", {{ sha256 = "{pin}" }}))
            return assert(handle:text())"#
        );
        match run(&lua, &source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "1.2.3"),
            Ok(_) => panic!("pinned fetch returns a string"),
            Err(error) => panic!("matching pins pass: {error}"),
        }
    }

    #[test]
    fn sha_pin_mismatches_raise() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        confit_driver::http::clear();
        confit_driver::http::serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = format!(
            r#"return fetch("{URL}", {{ sha256 = "{}" }})"#,
            "0".repeat(64)
        );
        match run(&lua, &source) {
            Ok(_) => panic!("mismatched pin passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("sha256 mismatch"), "names the pin: {text}");
                assert!(text.contains(URL), "names the url: {text}");
            }
        }
    }

    #[test]
    fn foreign_inputs_raise_naming_want() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local handle, reason = fetch(1)
            assert(handle == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                assert!(
                    reason.to_string_lossy().contains('1'),
                    "reason names the url: {reason:?}"
                )
            }
            Ok(_) => panic!("coerced url returns nil with reason"),
            Err(error) => panic!("coerced urls stay soft: {error}"),
        }
        match run(&lua, "return fetch(\"\")") {
            Ok(_) => panic!("empty url passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("a non-empty url"), "names the want: {text}");
            }
        }
        let source = format!(r#"return fetch("{URL}", {{ nope = 1 }})"#);
        match run(&lua, &source) {
            Ok(_) => panic!("unknown pin passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("nope"), "names the field: {text}");
            }
        }
        let source = format!(r#"return fetch("{URL}", {{ sha256 = "zzz" }})"#);
        match run(&lua, &source) {
            Ok(_) => panic!("bad digest passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("64-char hex digest"),
                    "names the want: {text}"
                );
                assert!(text.contains("zzz"), "names the received: {text}");
            }
        }
    }
}
