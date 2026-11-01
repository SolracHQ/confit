//! Fetches urls into resource handles.

use mlua::Lua;

use lua_binding::lua_module;

use confit_core::sha::Sha;

use confit_store::handles::ResourceHandle;

use crate::api::stores_from;

pub struct Fetch;

#[lua_module("confit.fetch")]
impl Fetch {
    /// Fetches one url.
    ///
    /// # Arguments
    ///
    /// - [url] The url to fetch.
    /// - [sha256] The pinned digest.
    ///
    /// # Returns
    ///
    /// - The resource handle, else nil.
    /// - The failure reason, else nil.
    pub fn fetch(
        lua: &Lua,
        url: String,
        sha256: Option<Sha>,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        if url.is_empty() {
            return Err(mlua::Error::runtime("fetch requires a non-empty url"));
        }
        match stores.fetch().fetch(&url, sha256) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Value;
    use std::path::Path;

    use crate::api::builtin;
    use confit_core::error::FetchError;
    use confit_core::sha::Sha;
    use confit_driver::http::{clear, fail, serve};
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

    fn fresh_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let fetch = builtin(&lua, "confit.fetch").unwrap();
        lua.globals().set("fetch", fetch).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn fetch_returns_scripted_bodies_as_text() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle = assert(fetch("https://example.com/tool.bin"))
            return assert(handle:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "1.2.3"),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the fetch module opens the url: {error}"),
        }
    }

    #[test]
    fn dead_urls_return_nil_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        fail(URL, FetchError::DownloadFailed);
        let stores = test_stores(dir.path());
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle, reason = fetch("https://example.com/tool.bin")
            assert(handle == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                assert!(
                    !reason.to_string_lossy().is_empty(),
                    "the reason shows the failure: {reason:?}"
                )
            }
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the fetch module keeps a dead url soft: {error}"),
        }
    }

    #[test]
    fn sha_pins_match_the_body() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = fresh_lua(&stores);
        let pin = Sha::hash(b"1.2.3").hex();
        let source = format!(
            r#"local handle = assert(fetch("{URL}", "{pin}"))
            return assert(handle:text())"#
        );
        match run(&lua, &source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "1.2.3"),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the fetch module accepts a matching pin: {error}"),
        }
    }

    #[test]
    fn sha_pin_mismatches_return_nil_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = fresh_lua(&stores);
        let source = format!(
            r#"local handle, reason = fetch("{URL}", "{}")
            assert(handle == nil)
            return reason"#,
            "0".repeat(64)
        );
        match run(&lua, &source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the fetch module keeps a mismatched pin soft: {error}"),
        }
    }

    #[test]
    fn fetch_rejects_invalid_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = fresh_lua(&stores);
        let source = r#"return fetch(1)"#;
        assert!(
            run(&lua, source).is_err(),
            "the fetch module rejects a non-text url"
        );
        let source = format!(r#"return fetch("{URL}", 1)"#);
        assert!(
            run(&lua, &source).is_err(),
            "the fetch module rejects a non-text digest"
        );
        let source = format!(r#"return fetch("{URL}", "abc")"#);
        assert!(
            run(&lua, &source).is_err(),
            "the fetch module rejects a bad digest text"
        );
    }
}
