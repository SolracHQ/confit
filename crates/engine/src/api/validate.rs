//! Shape checks over the decode cores.

use confit_binding::value::Decode;
use confit_binding_macros::face_impl;
use mlua::{Lua, Value};

use crate::api::config::Config;
use crate::api::patch::Patch;
use confit_model::condition::Condition;
use confit_model::document::Document;
use confit_model::document::RcEntry;
use confit_model::hook::Hook;
use confit_model::routes::Route;

/// Validate namespace.
pub struct Validate;

/// Decode one value with its failure reason beside it.
fn checked<T: Decode>(lua: &Lua, value: &Value) -> mlua::Result<(Option<T>, Option<String>)> {
    match T::decode(value, lua) {
        Ok(found) => Ok((Some(found), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

#[face_impl(namespace = "confit.validate")]
impl Validate {
    /// Check one document table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [body] The document table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn document(lua: &Lua, body: Value) -> mlua::Result<(Option<Document>, Option<String>)> {
        checked(lua, &body)
    }

    /// Check one config table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [config] The config table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn config(lua: &Lua, config: Value) -> mlua::Result<(Option<Config>, Option<String>)> {
        checked(lua, &config)
    }

    /// Check one patch table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [patch] The patch table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn patch(lua: &Lua, patch: Value) -> mlua::Result<(Option<Patch>, Option<String>)> {
        checked(lua, &patch)
    }

    /// Check one hook table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [value] The hook table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn hook(lua: &Lua, value: Value) -> mlua::Result<(Option<Hook>, Option<String>)> {
        checked(lua, &value)
    }

    /// Check one condition table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [value] The condition table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn condition(lua: &Lua, value: Value) -> mlua::Result<(Option<Condition>, Option<String>)> {
        checked(lua, &value)
    }

    /// Check one rc entry table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [value] The rc entry table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn entry(lua: &Lua, value: Value) -> mlua::Result<(Option<RcEntry>, Option<String>)> {
        checked(lua, &value)
    }

    /// Check one route table, else nil with reason.
    ///
    /// # Arguments
    ///
    /// - [value] The route table under checking.
    ///
    /// # Returns
    ///
    /// - The decoded value, else nil.
    /// - The failure reason, else nil.
    pub fn route(lua: &Lua, value: Value) -> mlua::Result<(Option<Route>, Option<String>)> {
        checked(lua, &value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Value};

    fn test_lua() -> Lua {
        let lua = Lua::new();
        let validate = Validate::register(&lua).unwrap();
        lua.globals().set("validate", validate).unwrap();
        crate::api::global::Global::install(&lua, &lua.globals()).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    fn reason_of(value: Value) {
        match value {
            Value::String(_) => {}
            _ => panic!("validate reason returns a string"),
        }
    }

    #[test]
    fn document_gave_true_with_nil_on_good_shapes() {
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.document({ path = { base = "home", relative = "subject" }, text = "hi" })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("good document passes"),
        }
    }

    #[test]
    fn document_gave_false_with_reason_on_bad_shapes() {
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.document({ nope = 1 })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
        let source = r#"
            local found, reason = validate.document(1)
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
    }

    #[test]
    fn config_gave_true_with_nil_on_good_shapes() {
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({
                name = id("tools/test"),
                documents = { { path = { base = "home", relative = ".bashrc" }, text = "hi" } },
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
    fn config_gave_false_with_reason_on_bad_shapes() {
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({ name = id("tools/test"), nope = 1 })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
    }

    #[test]
    fn config_gave_false_with_reason_on_missing_names() {
        let lua = test_lua();
        let source = r#"
            local found, reason = validate.config({
                documents = { { path = { base = "home", relative = ".bashrc" }, text = "hi" } },
            })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
    }

    #[test]
    fn patch_gave_true_with_nil_on_good_shapes() {
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
    fn patch_gave_false_with_reason_on_bad_shapes() {
        let lua = test_lua();
        let cases = [
            r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "xml",
                    apply = function(d) return d end,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
            r#"
                local found, reason = validate.patch({
                    target = 1,
                    format = "json",
                    apply = function(d) return d end,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
            r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    format = "json",
                    apply = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
        ];
        for source in cases {
            reason_of(run(&lua, source));
        }
    }

    #[test]
    fn patch_rejects_foreign_shapes_showing_keys() {
        let lua = test_lua();
        let cases = [
            r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
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
            r#"
                local found, reason = validate.patch({
                    target = { base = "home", relative = "a" },
                    apply = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
        ];
        for source in cases {
            reason_of(run(&lua, source));
        }
    }

    // Body shapes pin in model. Validate pins true/false alone.
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
            let source = format!(
                "local found, reason = validate.{verb}(42) assert(found == nil and reason ~= nil) return reason"
            );
            reason_of(run(&lua, &source));
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
            let source = format!(
                "local found, reason = validate.{verb}({body}) assert(found ~= nil and reason == nil) return 'ok'"
            );
            match run(&lua, &source) {
                Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
                _ => panic!("{verb} passes good shapes"),
            }
        }
    }
}
