//! Validate
//! Shape checks over the decode cores returning bool with reason.

use confit_binding_macros::face_impl;

use crate::api::config::Config;
use crate::api::patch::Patch;
use confit_model::condition::Condition;
use confit_model::document::Data;
use confit_model::document::RcEntry;
use confit_model::hook::Hook;
use confit_model::routes::Route;

/// Declare shape-check statics over face decodes.
macro_rules! check {
    (
        $( {
            $(#[doc = $verb_documentation:literal])+
            fn $verb:ident (
                $(#[doc = $parameter_documentation:literal])+
                $parameter:ident : Value
                $(,)?
            ) -> $checked:ty
        } ),* $(,)?
    ) => {
        #[face_impl(namespace = "confit.validate")]
        impl Validate {
            $(
                $(#[doc = $verb_documentation])+
                ///
                /// # Arguments
                ///
                $(#[doc = $parameter_documentation])+
                ///
                /// # Returns
                ///
                /// - The decoded value, else nil.
                /// - The failure reason, else nil.
                pub fn $verb(
                    lua: &::mlua::Lua,
                    $parameter: ::mlua::Value,
                ) -> ::mlua::Result<(
                    ::std::option::Option<$checked>,
                    ::std::option::Option<::std::string::String>,
                )> {
                    match <$checked as ::confit_binding::value::Decode>::decode(&$parameter, lua) {
                        ::std::result::Result::Ok(found) => ::std::result::Result::Ok((
                            ::std::option::Option::Some(found),
                            ::std::option::Option::None,
                        )),
                        ::std::result::Result::Err(error) => ::std::result::Result::Ok((
                            ::std::option::Option::None,
                            ::std::option::Option::Some(error.to_string()),
                        )),
                    }
                }
            )*
        }
    };
}

/// Validate namespace.
pub struct Validate;

check! {
    {
        /// Check one body table.
        fn document(
            /// - [body] The body table under checking.
            body: Value,
        ) -> Data
    },
    {
        /// Check one config table.
        fn config(
            /// - [config] The config table under checking.
            config: Value,
        ) -> Config
    },
    {
        /// Check one patch table.
        fn patch(
            /// - [patch] The patch table under checking.
            patch: Value,
        ) -> Patch
    },
    {
        /// Check one hook table.
        fn hook(
            /// - [value] The hook table under checking.
            value: Value,
        ) -> Hook
    },
    {
        /// Check one condition table.
        fn condition(
            /// - [value] The condition table under checking.
            value: Value,
        ) -> Condition
    },
    {
        /// Check one rc entry table.
        fn entry(
            /// - [value] The entry table under checking.
            value: Value,
        ) -> RcEntry
    },
    {
        /// Check one route table.
        fn route(
            /// - [value] The route table under checking.
            value: Value,
        ) -> Route
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Value};

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let validate = Validate::register(&lua).unwrap();
        lua.globals().set("validate", validate).unwrap();
        lua
    }

    fn opened_lua() -> (tempfile::TempDir, Lua) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.txt"), b"hello").unwrap();
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
        let open = crate::api::builtin(&lua, "confit.open").unwrap();
        lua.globals().set("open", open).unwrap();
        let validate = Validate::register(&lua).unwrap();
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
    fn patch_rejects_foreign_shapes_showing_keys() {
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
        let (_held, lua) = opened_lua();
        let cases = [
            r#"{ text = "hi", mode = "644", unmanaged = true }"#,
            r#"{ format = "json", data = { alpha = "one" } }"#,
            #[cfg(unix)]
            r#"{ target = { base = "home", relative = "linked" } }"#,
            r#"{ profile = { { env = { name = "EDITOR", value = "hx" } } } }"#,
            r#"{ handle = assert(open("note.txt")), mode = "644" }"#,
            r#"{ members = { ["top.txt"] = assert(open("note.txt")) } }"#,
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
    fn document_checks_blob_bodies_without_writes() {
        let (held, lua) = opened_lua();
        for body in [
            r#"{ handle = assert(open("note.txt")), mode = "644" }"#,
            r#"{ members = { ["top.txt"] = assert(open("note.txt")) } }"#,
        ] {
            let source = [
                "local found, reason = validate.document(",
                body,
                ") assert(found ~= nil, reason) return 'ok'",
            ]
            .concat();
            match run(&lua, &source) {
                Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
                _ => panic!("good body passes without writes: {body}"),
            }
        }
        let cache = held.path().join("temp").join("blobs");
        assert!(
            !cache.exists()
                || confit_driver::fs::read_dir(&cache)
                    .unwrap_or_default()
                    .is_empty(),
            "checks write zero blob files"
        );
    }

    #[test]
    fn document_names_mixed_markers() {
        let lua = test_lua();
        let mut cases = vec![
            (r#"{ nope = 1 }"#, "unknown field", "nope"),
            (
                r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
                "unknown tag",
                "profile",
            ),
        ];
        #[cfg(unix)]
        cases.push((
            r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
            "Document_Text",
            "Document_Link",
        ));
        #[cfg(not(unix))]
        cases.push((r#"{ text = "hi", nope = 1 }"#, "Document_Text", "nope"));
        for (body, first, second) in cases {
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

    #[test]
    fn every_check_verb_rejects_numbers_with_reasons() {
        let lua = test_lua();
        for verb in [
            "document",
            "config",
            "patch",
            "hook",
            "condition",
            "entry",
            "route",
        ] {
            let source = [
                "local found, reason = validate.",
                verb,
                "(42) assert(found == nil and reason ~= nil) return reason",
            ]
            .concat();
            let text = reason_of(run(&lua, &source));
            assert!(!text.is_empty(), "{verb} shows the fault");
        }
    }

    #[test]
    fn new_check_verbs_pass_good_shapes() {
        let lua = test_lua();
        for (verb, body) in [
            ("hook", r#"{ argv = { "true" } }"#),
            ("condition", r#"{ all = {} }"#),
            ("entry", r#"{ alias = { name = "x", expansion = "y" } }"#),
            ("route", r#"{ base = "home", relative = ".bashrc" }"#),
        ] {
            let source = [
                "local found, reason = validate.",
                verb,
                "(",
                body,
                ") assert(found ~= nil and reason == nil) return 'ok'",
            ]
            .concat();
            match run(&lua, &source) {
                Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
                _ => panic!("{verb} passes good shapes"),
            }
        }
    }
}
