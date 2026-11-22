//! Checks model shapes.

use lua_binding::lua_module;
use lua_binding::value::Decode;
use mlua::{Lua, Value};

use crate::api::config::Config;
use crate::api::patch::Patch;
use confit_model::condition::Condition;
use confit_model::document::Document;
use confit_model::document::RcEntry;
use confit_model::hook::Hook;
use confit_model::routes::Route;

pub struct Validate;

#[lua_module("confit.validate")]
impl Validate {
    /// Checks one document table.
    ///
    /// # Arguments
    ///
    /// - [body] The document table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn document(lua: &Lua, body: Value) -> mlua::Result<(Option<Document>, Option<String>)> {
        checked(lua, &body)
    }

    /// Checks one config table.
    ///
    /// # Arguments
    ///
    /// - [config] The config table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn config(lua: &Lua, config: Value) -> mlua::Result<(Option<Config>, Option<String>)> {
        checked(lua, &config)
    }

    /// Checks one patch table.
    ///
    /// # Arguments
    ///
    /// - [patch] The patch table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn patch(lua: &Lua, patch: Value) -> mlua::Result<(Option<Patch>, Option<String>)> {
        checked(lua, &patch)
    }

    /// Checks one hook table.
    ///
    /// # Arguments
    ///
    /// - [value] The hook table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn hook(lua: &Lua, value: Value) -> mlua::Result<(Option<Hook>, Option<String>)> {
        checked(lua, &value)
    }

    /// Checks one condition table.
    ///
    /// # Arguments
    ///
    /// - [value] The condition table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn condition(lua: &Lua, value: Value) -> mlua::Result<(Option<Condition>, Option<String>)> {
        checked(lua, &value)
    }

    /// Checks one rc entry table.
    ///
    /// # Arguments
    ///
    /// - [value] The rc entry table.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn entry(lua: &Lua, value: Value) -> mlua::Result<(Option<RcEntry>, Option<String>)> {
        checked(lua, &value)
    }

    /// Checks one route handle.
    ///
    /// # Arguments
    ///
    /// - [value] The route handle.
    ///
    /// # Returns
    ///
    /// - The decoded value.
    /// - The failure reason.
    pub fn route(lua: &Lua, value: Value) -> mlua::Result<(Option<Route>, Option<String>)> {
        checked(lua, &value)
    }
}

/// Decodes one value with its reason.
fn checked<T: Decode>(lua: &Lua, value: &Value) -> mlua::Result<(Option<T>, Option<String>)> {
    match T::decode(value, lua) {
        Ok(found) => Ok((Some(found), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::global::Global;
    use mlua::{Lua, Value};

    fn fresh_lua() -> Lua {
        let lua = Lua::new();
        let validate = Validate::add(&lua).unwrap();
        lua.globals().set("validate", validate).unwrap();
        Global::install(&lua, &lua.globals()).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    fn reason_of(value: Value) {
        match value {
            Value::String(_) => {}
            _ => panic!("expected a string reason"),
        }
    }

    #[test]
    fn document_check_accepts_a_good_shape() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.document({ path = route("home", "subject"), text = "hi" })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("expected a string marker"),
        }
    }

    #[test]
    fn document_check_rejects_invalid_shapes() {
        let lua = fresh_lua();
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
    fn config_check_accepts_a_good_shape() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.config({
                name = id("tools/test"),
                documents = { { path = route("home", ".bashrc"), text = "hi" } },
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("expected a string marker"),
        }
    }

    #[test]
    fn config_check_rejects_invalid_shapes() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.config({ name = id("tools/test"), nope = 1 })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
    }

    #[test]
    fn config_check_rejects_an_absent_name() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.config({
                documents = { { path = route("home", ".bashrc"), text = "hi" } },
            })
            assert(found == nil and reason ~= nil)
            return reason
        "#;
        reason_of(run(&lua, source));
    }

    #[test]
    fn patch_check_accepts_a_good_shape() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.patch({
                target = route("home", "app.json"),
                format = "json",
                apply = function(d) return d end,
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("expected a string marker"),
        }
    }

    #[test]
    fn patch_accepts_rc_shapes() {
        let lua = fresh_lua();
        let source = r#"
            local found, reason = validate.patch({
                target = route("home", "app.sh"),
                apply = function(d) return d end,
            })
            assert(found ~= nil and reason == nil)
            return "ok"
        "#;
        match run(&lua, source) {
            Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
            _ => panic!("expected a string marker"),
        }
    }

    #[test]
    fn patch_check_rejects_invalid_shapes() {
        let lua = fresh_lua();
        let cases = [
            r#"
                local found, reason = validate.patch({
                    target = route("home", "a"),
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
                    target = route("home", "a"),
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
    fn patch_check_rejects_unknown_keys() {
        let lua = fresh_lua();
        let cases = [
            r#"
                local found, reason = validate.patch({
                    target = route("home", "a"),
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
            r#"
                local found, reason = validate.patch({
                    target = route("home", "a"),
                    format = "json",
                    apply = function(d) return d end,
                    nope = 1,
                })
                assert(found == nil and reason ~= nil)
                return reason
                "#,
            r#"
                local found, reason = validate.patch({
                    target = route("home", "a"),
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
        let lua = fresh_lua();
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
    fn validate_verbs_accept_good_shapes() {
        let lua = fresh_lua();
        for (verb, body) in [
            ("hook", r#"{ argv = { "true" }, id = id("tools/test") }"#),
            ("condition", r#"{ all = {} }"#),
            ("entry", r#"{ alias = { name = "x", expansion = "y" } }"#),
            ("route", r#"route("home", ".bashrc")"#),
        ] {
            let source = format!(
                "local found, reason = validate.{verb}({body}) assert(found ~= nil and reason == nil) return 'ok'"
            );
            match run(&lua, &source) {
                Value::String(found) => assert_eq!(found.to_string_lossy(), "ok"),
                _ => panic!("the {verb} check accepts a good shape"),
            }
        }
    }
}
