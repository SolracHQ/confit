//! Validate
//! Shape checks over the decode cores returning bool with reason.

use mlua::{FromLua, Lua, Value};
use mlua_extras::typed::Type;
use mlua_extras::typed::generator::DefinitionBuilder;

use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::document::{RcEntry, StructuredFormat};
use confit_model::routes::{Route, RouteBase};
use confit_surface_face::module::{Module, Named, VerbDecl, readonly};
use confit_surface_face::proxy::StrictString;
use confit_surface_face::shape::Shape;

use crate::config::decode_config;
use crate::document::SerializableValue;
use crate::document::{
    Document, LinkDocument, OpaqueDocument, RcDocument, StructuredDocument, TextDocument, Tree,
    TreeDocument, TreeSlot,
};
use crate::handles::{Archive, Handle};
use crate::hook::HookFace;
use crate::patch::{Patch, RcPatch, StructuredPatch, decode_patch};

/// Build the validate module table with its face.
pub fn build(lua: &Lua) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Shape checks over config tables.")?;
    module.register::<TextDocument>();
    module.register_schema::<StructuredFormat>();
    module.register_schema::<Route>();
    module.register_schema::<RouteBase>();
    module.register::<StructuredDocument>();
    module.register::<LinkDocument>();
    module.register::<RcDocument>();
    module.register::<OpaqueDocument>();
    module.register::<TreeDocument>();
    module.register::<Handle>();
    module.register::<Archive>();
    module.register::<Tree>();
    module.register::<TreeSlot>();
    module.register::<Document>();
    module.register::<StructuredPatch>();
    module.register::<RcPatch>();
    module.register::<Patch>();
    module.register::<HookFace>();
    module.register::<SerializableValue>();
    module.register::<StrictString>();
    module.register_schema::<RcEntry>();
    module.register_schema::<Condition>();
    module.register_schema::<Arg>();
    module.verb::<Shape<Named<Document>>, (bool, Option<String>), _>(
        lua,
        VerbDecl {
            name: "document",
            documentation: "Checks one body table.",
            params: &[("body", "The body table under checking.")],
            rets: &["True with nil, else false with the reason."],
        },
        document_impl,
    )?;
    module.verb::<Shape<ConfigFace>, (bool, Option<String>), _>(
        lua,
        VerbDecl {
            name: "config",
            documentation: "Checks one config table.",
            params: &[("config", "The config table under checking.")],
            rets: &["True with nil, else false with the reason."],
        },
        config_impl,
    )?;
    module.verb::<Shape<Patch>, (bool, Option<String>), _>(
        lua,
        VerbDecl {
            name: "patch",
            documentation: "Checks one patch table.",
            params: &[("patch", "The patch table under checking.")],
            rets: &["True with nil, else false with the reason."],
        },
        patch_impl,
    )?;
    let (table, face) = module.finish();
    Ok((readonly(lua, "confit.validate", table)?, face))
}

/// Check one body table with true and nil for good shapes and false with
/// the reason for bad shapes.
fn document_impl(lua: &Lua, body: Shape<Named<Document>>) -> mlua::Result<(bool, Option<String>)> {
    Ok(checked(
        Document::from_lua(Value::Table(body.into_inner()), lua)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    ))
}

/// Check one config table with true and nil for good shapes and false with
/// the reason for bad shapes.
fn config_impl(lua: &Lua, config: Shape<ConfigFace>) -> mlua::Result<(bool, Option<String>)> {
    Ok(checked(
        decode_config(lua, &Value::Table(config.into_inner()))
            .map(|_| ())
            .map_err(|error| error.to_string()),
    ))
}

/// Check one patch table with true and nil for good shapes and false with
/// the reason for bad shapes.
fn patch_impl(lua: &Lua, patch: Shape<Patch>) -> mlua::Result<(bool, Option<String>)> {
    Ok(checked(
        decode_patch(lua, &Value::Table(patch.into_inner()), None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    ))
}

/// One config face.
struct ConfigFace;

confit_surface_face::table_face!(ConfigFace, {
    required name: Type::string(),
    optional documents: Type::map(Named::<Route>::ty(), Named::<Document>::ty()),
    optional patches: Type::array(Patch::ty()),
    optional hooks: Type::array(Named::<HookFace>::ty()),
});

/// Return one bool with reason pair from its outcome.
fn checked(outcome: std::result::Result<(), String>) -> (bool, Option<String>) {
    match outcome {
        Ok(()) => (true, None),
        Err(reason) => (false, Some(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let validate = build(&lua).unwrap().0;
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
        let validate = build(&lua).unwrap().0;
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
            local ok, reason = validate.document({ text = "hi" })
            assert(ok and reason == nil)
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
            local ok, reason = validate.document({ nope = 1 })
            assert(not ok and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(!text.is_empty(), "reason shows the fault: {text}");
    }

    #[test]
    fn config_returns_true_with_nil_on_good_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local ok, reason = validate.config({
                name = "tools/test",
                documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
            })
            assert(ok and reason == nil)
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
            local ok, reason = validate.config({ name = "tools/test", nope = 1 })
            assert(not ok and reason ~= nil)
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
            local ok, reason = validate.config({
                documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
            })
            assert(not ok and reason ~= nil)
            return reason
        "#;
        let text = reason_of(run(&lua, source));
        assert!(
            text.contains("config name"),
            "reason shows the field: {text}"
        );
    }

    #[test]
    fn patch_returns_true_with_nil_on_good_shapes() {
        let _guard = TestGuard::install();
        let lua = test_lua();
        let source = r#"
            local ok, reason = validate.patch({
                target = { base = "home", relative = "app.json" },
                format = "json",
                apply = function(d) return d end,
            })
            assert(ok and reason == nil)
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
            local ok, reason = validate.patch({
                target = { base = "home", relative = "app.sh" },
                apply = function(d) return d end,
            })
            assert(ok and reason == nil)
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
                local ok, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "xml",
                    apply = function(d) return d end,
                })
                assert(not ok and reason ~= nil)
                return reason
                "#,
                "xml",
            ),
            (
                r#"
                local ok, reason = validate.patch({
                    target = 1,
                    format = "json",
                    apply = function(d) return d end,
                })
                assert(not ok and reason ~= nil)
                return reason
                "#,
                "a route table",
            ),
            (
                r#"
                local ok, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "json",
                    apply = 1,
                })
                assert(not ok and reason ~= nil)
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
                local ok, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(not ok and reason ~= nil)
                return reason
                "#,
                "nope",
            ),
            (
                r#"
                local ok, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "json",
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(not ok and reason ~= nil)
                return reason
                "#,
                "nope",
            ),
            (
                r#"
                local ok, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = 1,
                })
                assert(not ok and reason ~= nil)
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
                "local ok, reason = validate.document(",
                body,
                ") assert(ok, reason) return 'ok'",
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
                "text",
                "target",
            ),
            (r#"{ nope = 1 }"#, "nope", "nope"),
            (
                r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
                "frobnicate",
                "frobnicate",
            ),
        ] {
            let source = [
                "local ok, reason = validate.document(",
                body,
                ") assert(not ok and reason ~= nil) return reason",
            ]
            .concat();
            let text = reason_of(run(&lua, &source));
            assert!(text.contains(first), "reason shows the fault: {text}");
            assert!(text.contains(second), "reason shows the fault: {text}");
        }
    }
}
