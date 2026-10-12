//! Urls opened into resource handles.

use mlua::Lua;

use confit_binding_macros::Face;

use confit_driver::sha::Sha;
use confit_store::fetch::error::FetchError;
use confit_store::handles::ResourceHandle;

use crate::api::stores_of;
use crate::error::EngineError;

/// Fetch options.
#[derive(Debug, Clone, Default, Face)]
#[face(table)]
pub struct FetchOpts {
    /// The pinned sha256 hex digest.
    pub sha256: Option<String>,
}

confit_binding::verb! {
    /// Opens one url into its handle.
    fn fetch(
        lua: &Lua,
        /// The url to fetch.
        url: String,
        /// The digest pin table.
        #[face(optional)] opts: Option<FetchOpts>,
    ) -> (
        /// The resource handle, else nil.
        Option<ResourceHandle>,
        /// The failure reason, else nil.
        Option<String>,
    ) [call = fetch_impl]
}

confit_binding::module! {
    namespace = "confit.fetch",
    verb = fetch,
}

/// Open one url into its resource handle.
///
/// Dead urls take nil with reason and pin mismatches raise.
///
/// # Errors
///
/// - [`EngineError::Mismatch`] for non-string urls and misshaped pins.
/// - [`EngineError::Fetch`] for pin mismatches.
fn fetch_impl(
    lua: &Lua,
    raw: String,
    opts: Option<FetchOpts>,
) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
    let stores = stores_of(lua)?;
    if raw.is_empty() {
        return Err(EngineError::mismatch("url", "a non-empty url", "empty string").into());
    }
    let wanted = pin_of(opts)?;
    match stores.fetch().fetch(&raw, wanted, false) {
        Ok(found) => Ok((Some(found), None)),
        Err(error @ FetchError::DigestMismatch { .. }) => Err(EngineError::Fetch(error).into()),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

/// Resolve the digest pin from the options value.
///
/// # Errors
///
/// - [`EngineError::Mismatch`] for bad digests.
fn pin_of(opts: Option<FetchOpts>) -> mlua::Result<Option<Sha>> {
    match opts.and_then(|found| found.sha256) {
        None => Ok(None),
        Some(digest) => open_pin(&digest),
    }
}

/// Open one digest pin.
///
/// # Errors
///
/// - [`EngineError::Mismatch`] for non-hex digests.
fn open_pin(digest: &str) -> mlua::Result<Option<Sha>> {
    match Sha::new(digest) {
        Ok(found) => Ok(Some(found)),
        Err(_) => Err(EngineError::mismatch("sha256", "a 64-char hex digest", digest).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Value;
    use std::path::Path;

    use confit_driver::fs::TestGuard;
    use confit_driver::sha::Sha;
    use confit_store::{StoreRoots, Stores};

    const URL: &str = "https://example.com/tool.bin";

    fn test_stores(dir: &Path) -> Stores {
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
                    "reason shows the url: {reason:?}"
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
                assert!(
                    text.contains("sha256 mismatch"),
                    "the text shows the pin: {text}"
                );
                assert!(text.contains(URL), "the text shows the url: {text}");
            }
        }
    }

    #[test]
    fn foreign_inputs_raise_naming_want() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"return fetch(1)"#;
        match run(&lua, source) {
            Ok(_) => panic!("foreign url passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("a string"), "the text shows the want: {text}");
            }
        }
        match run(&lua, "return fetch(\"\")") {
            Ok(_) => panic!("empty url passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("a non-empty url"),
                    "the text shows the want: {text}"
                );
            }
        }
        let source = format!(r#"return fetch("{URL}", {{ nope = 1 }})"#);
        match run(&lua, &source) {
            Ok(_) => panic!("unknown pin passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("nope"), "the text shows the field: {text}");
            }
        }
        let source = format!(r#"return fetch("{URL}", {{ sha256 = "zzz" }})"#);
        match run(&lua, &source) {
            Ok(_) => panic!("bad digest passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("64-char hex digest"),
                    "the text shows the want: {text}"
                );
                assert!(text.contains("zzz"), "the text shows the received: {text}");
            }
        }
    }
}
