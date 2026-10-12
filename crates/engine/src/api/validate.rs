//! Validate
//! Shape checks over the decode cores returning bool with reason.

use mlua::{Lua, Value};

use crate::api::config::Config;
use crate::api::patch::Patch;
use confit_binding::value::Decode;
use confit_model::document::Data;

confit_binding::verb! {
    /// Checks one body table.
    fn document(
        lua: &Lua,
        /// The body table under checking.
        body: Value,
    ) -> (
        /// The decoded body, else nil.
        Option<Data>,
        /// The failure reason, else nil.
        Option<String>,
    ) [call = document_impl]
}

confit_binding::verb! {
    /// Checks one config table.
    fn config(
        lua: &Lua,
        /// The config table under checking.
        config: Value,
    ) -> (
        /// The decoded config, else nil.
        Option<Config>,
        /// The failure reason, else nil.
        Option<String>,
    ) [call = config_impl]
}

confit_binding::verb! {
    /// Checks one patch table.
    fn patch(
        lua: &Lua,
        /// The patch table under checking.
        patch: Value,
    ) -> (
        /// The decoded patch, else nil.
        Option<Patch>,
        /// The failure reason, else nil.
        Option<String>,
    ) [call = patch_impl]
}

confit_binding::module! {
    namespace = "confit.validate",
    verbs = [document, config, patch],
}

/// Check one body table with the decoded body and nil for good shapes and nil
/// with the reason for bad shapes.
fn document_impl(lua: &Lua, body: Value) -> mlua::Result<(Option<Data>, Option<String>)> {
    match Data::decode(&body, lua) {
        Ok(found) => Ok((Some(found), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

/// Check one config table with the decoded config and nil for good shapes and
/// nil with the reason for bad shapes.
fn config_impl(lua: &Lua, config: Value) -> mlua::Result<(Option<Config>, Option<String>)> {
    match Config::decode(&config, lua) {
        Ok(found) => Ok((Some(found), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

/// Check one patch table with the decoded patch and nil for good shapes and nil
/// with the reason for bad shapes.
fn patch_impl(lua: &Lua, patch: Value) -> mlua::Result<(Option<Patch>, Option<String>)> {
    match Patch::decode(&patch, lua) {
        Ok(found) => Ok((Some(found), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let validate = Module::register(&lua).unwrap();
        lua.globals().set("validate", validate).unwrap();
        lua
    }

    fn opened_lua() -> (tempfile::TempDir, Lua) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        confit_driver::fs::create_dir_all(&root).unwrap();
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = confit_store::Stores::new(
            confit_store::StoreRoots {
                config_base: directory.path().join("config"),
                temp_base: directory.path().join("temp"),
                project_root: root,
            },
            sender,
        );
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let resource = crate::api::builtin(&lua, "confit.resource").unwrap();
        lua.globals().set("resource", resource).unwrap();
        let validate = Module::register(&lua).unwrap();
        lua.globals().set("validate", validate).unwrap();
        (directory, lua)
    }

    fn run(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    fn reason_of(value: Value) -> String {
        match value {
            Value::String(found) => found.to_string_lossy(),
            _ => panic!("validate reason returns a string"),
        }
    }

    #[test]
    fn document_returns_true_with_nil_on_good_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.document({ text = "hi" })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("good document passes"),
        }
    }

    #[test]
    fn document_returns_false_with_reason_on_bad_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.document({ nope = 1 })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(!text.is_empty(), "reason shows the fault: {text}");
        let source = r#"
            local found, reason = validate.document(1)
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(
            text.contains("Document_Text"),
            "reason shows the want: {text}"
        );
    }

    #[test]
    fn config_returns_true_with_nil_on_good_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({
                name = "tools/test",
                documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("good config passes"),
        }
    }

    #[test]
    fn config_returns_false_with_reason_on_bad_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({ name = "tools/test", nope = 1 })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(text.contains("nope"), "reason shows the key: {text}");
    }

    #[test]
    fn config_returns_false_with_reason_on_missing_names() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({
                documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
            })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(
            text.contains("Config.name"),
            "reason shows the field: {text}"
        );
    }

    #[test]
    fn patch_returns_true_with_nil_on_good_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.patch({
                target = { base = "home", relative = "app.json" },
                format = "json",
                apply = function(d) return d end,
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("good patch passes"),
        }
    }

    #[test]
    fn patch_accepts_rc_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.patch({
                target = { base = "home", relative = "app.sh" },
                apply = function(d) return d end,
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("good rc patch passes"),
        }
    }

    #[test]
    fn patch_returns_false_with_reason_on_bad_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let cases = [
            (
                r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "xml",
                    apply = function(d) return d end,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "xml",
            ),
            (
                r#"
                local found, reason = validate.patch({
                    target = 1,
                    format = "json",
                    apply = function(d) return d end,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "a table",
            ),
            (
                r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "json",
                    apply = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "a function",
            ),
        ];
        for (source, want) in cases {
            let text = reason_of(run(&lua, source));
            assert!(text.contains(want), "reason shows the fault: {text}");
        }
    }

    #[test]
    fn patch_rejects_foreign_shapes_naming_keys() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let cases = [
            (
                r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "nope",
            ),
            (
                r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "json",
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "nope",
            ),
            (
                r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
                "a function",
            ),
        ];
        for (source, want) in cases {
            let text = reason_of(run(&lua, source));
            assert!(text.contains(want), "reason shows the fault: {text}");
        }
    }

    #[test]
    fn document_checks_every_body_kind() {
        let _guard = TestGuard::install();
        let (_held, lua) = opened_lua();
        let cases = [
            r#"{ text = "hi", mode = "644", unmanaged = true }"#,
            r#"{ format = "json", data = { alpha = "one" } }"#,
            r#"{ target = { base = "home", relative = "linked" } }"#,
            r#"{ profile = { { env = { name = "EDITOR", value = "hx" } } } }"#,
            r#"{ handle = assert(resource("note.txt")), mode = "644" }"#,
            r#"{ members = { ["top.txt"] = assert(resource("note.txt")) } }"#,
        ];
        for body in cases {
            let source = [
                "local found, reason = validate.document(",
                body,
                ") assert(found ~= nil, reason) return 'ok'",
            ]
            .concat();
            match run(&lua, &source) {
                Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
                _ => panic!("good body passes: {body}"),
            }
        }
    }

    #[test]
    fn document_names_mixed_markers() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        for (body, first, second) in [
            (
                r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
                "Document_Text",
                "Document_Link",
            ),
            (r#"{ nope = 1 }"#, "unknown field", "nope"),
            (
                r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
                "unknown tag",
                "profile",
            ),
        ] {
            let source = [
                "local found, reason = validate.document(",
                body,
                ") assert(found == nil and reason ~= nil) return reason",
            ]
            .concat();
            let text = reason_of(run(&lua, &source));
            assert!(text.contains(first), "reason shows the fault: {text}");
            assert!(text.contains(second), "reason shows the fault: {text}");
        }
    }
}
