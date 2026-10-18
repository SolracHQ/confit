//! Urls opened into resource handles.

use mlua::Lua;

use confit_binding_macros::{Face, face_impl};

use confit_driver::sha::Sha;

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

/// Fetch namespace.
pub struct Fetch;

#[face_impl(namespace = "confit.fetch")]
impl Fetch {
    /// Open one url.
    ///
    /// # Arguments
    ///
    /// - [url] The url to fetch.
    /// - [opts] The digest pin table.
    ///
    /// # Returns
    ///
    /// - The resource handle, else nil.
    /// - The failure reason, else nil.
    pub fn fetch(
        lua: &Lua,
        url: String,
        #[face(optional)] opts: FetchOpts,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_of(lua)?;
        if url.is_empty() {
            let reason =
                EngineError::mismatch("url", "a non-empty url", "empty string").to_string();
            return Ok((None, Some(reason)));
        }
        let (wanted, pin_reason) = pin_of(opts)?;
        if let Some(reason) = pin_reason {
            return Ok((None, Some(reason)));
        }
        match stores.fetch().fetch(&url, wanted, false) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

/// Resolve the digest pin from the options value.
///
/// # Returns
///
/// - Return the pin on success.
/// - Return the reason on bad digests.
fn pin_of(opts: FetchOpts) -> mlua::Result<(Option<Sha>, Option<String>)> {
    match opts.sha256 {
        None => Ok((None, None)),
        Some(digest) => open_pin(&digest),
    }
}

/// Open one digest pin.
///
/// # Returns
///
/// - Return the pin on success.
/// - Return the reason on bad digests.
fn open_pin(digest: &str) -> mlua::Result<(Option<Sha>, Option<String>)> {
    match Sha::new(digest) {
        Ok(found) => Ok((Some(found), None)),
        Err(_) => {
            let reason =
                EngineError::mismatch("sha256", "a 64-char hex digest", digest).to_string();
            Ok((None, Some(reason)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Value;
    use std::path::Path;

    use crate::api::builtin;
    use confit_driver::http::{clear, serve};
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
        let fetch = builtin(&lua, "confit.fetch").unwrap();
        lua.globals().set("fetch", fetch).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn serves_scripted_bodies_reading_text() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        serve(URL, b"1.2.3");
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
        clear();
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
        clear();
        serve(URL, b"1.2.3");
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
    fn sha_pin_mismatches_return_nil_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        clear();
        serve(URL, b"1.2.3");
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = format!(
            r#"local handle, reason = fetch("{URL}", {{ sha256 = "{}" }})
            assert(handle == nil)
            return reason"#,
            "0".repeat(64)
        );
        match run(&lua, &source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("mismatched pin returns nil with reason"),
            Err(error) => panic!("mismatched pins stay soft: {error}"),
        }
    }

    #[test]
    fn foreign_inputs_raise_showing_want() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"return fetch(1)"#;
        assert!(run(&lua, source).is_err(), "foreign url fails");
        let source = format!(r#"return fetch("{URL}", {{ nope = 1 }})"#);
        assert!(run(&lua, &source).is_err(), "unknown pin fails");
    }
}
