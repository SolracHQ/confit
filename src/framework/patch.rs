//! Patch
//!
//! Patch handle constructors for Lua. `confit.patch.rc` plus
//! `confit.patch.structured` return handles carrying target plus format
//! plus callback plus priority. Binding executes callbacks live.

use mlua::{Function, Lua, UserData, UserDataMethods, Value};

use crate::model::state::config::Patch;
use crate::model::state::document::StructuredFormat;
use crate::model::state::level::Level;

use super::confit_table;

/// Patch handle built by the `confit.patch` constructors.
///
/// Carries the target document key plus format plus callback plus merge
/// priority. Submitted through `config:add_patch`.
#[derive(Clone)]
pub struct LuaPatch {
    /// Target document key: `rc` or one document path.
    pub target: String,
    /// Structured format for patch-created documents, `None` for rc.
    pub format: Option<StructuredFormat>,
    /// Callback receiving the live wrapper.
    pub callback: Function,
    /// Merge priority for the patch.
    pub priority: Level,
}

impl LuaPatch {
    /// Converts the handle into a model patch stamped with an owner.
    ///
    /// # Arguments
    ///
    /// * `self` - handle holding target plus format plus priority.
    /// * `owner` - contributing config name.
    ///
    /// # Returns
    ///
    /// Model patch holding target plus format plus owner plus priority.
    pub fn into_model(self, owner: &str) -> Patch {
        Patch {
            document: self.target,
            format: self.format,
            owner: owner.to_string(),
            priority: self.priority,
        }
    }
}

impl UserData for LuaPatch {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("priority", |_, this, level: Value| {
            this.priority = parse_level(level)?;
            Ok(this.clone())
        });
    }
}

/// Parses a priority level from a Lua value.
///
/// # Arguments
///
/// * `value` - raw level value, one `confit.priority` string.
///
/// # Returns
///
/// Merge priority level.
///
/// # Errors
///
/// Fails with plan errors for values outside the five level names.
fn parse_level(value: Value) -> mlua::Result<Level> {
    const CTOR: &str = "confit.patch";
    let name = match value {
        Value::String(text) => text.to_string_lossy().to_ascii_uppercase(),
        _ => {
            return Err(plan_err!(
                "{CTOR}: field 'priority' must be one of 'MINOR', 'LOW', 'NORMAL', 'HIGH', or 'MAJOR'"
            ));
        }
    };
    match name.as_str() {
        "MINOR" => Ok(Level::Minor),
        "LOW" => Ok(Level::Low),
        "NORMAL" => Ok(Level::Normal),
        "HIGH" => Ok(Level::High),
        "MAJOR" => Ok(Level::Major),
        _ => Err(plan_err!(
            "{CTOR}: field 'priority' unknown level '{name}' (expected 'MINOR', 'LOW', 'NORMAL', 'HIGH', or 'MAJOR')"
        )),
    }
}

/// Installs the patch plus priority namespaces on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespaces.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set(
        "rc",
        lua.create_function(|_, callback: Value| rc_impl(callback))?,
    )?;
    namespace.set(
        "structured",
        lua.create_function(|_, args: (Value, Value, Value)| structured_impl(args))?,
    )?;
    confit.set("patch", namespace)?;

    let priority = lua.create_table()?;
    for name in ["MINOR", "LOW", "NORMAL", "HIGH", "MAJOR"] {
        priority.set(name, name)?;
    }
    confit.set("priority", priority)?;
    Ok(())
}

/// Builds an rc patch handle carrying the callback.
///
/// # Arguments
///
/// * `callback` - function receiving the live wrapper.
///
/// # Returns
///
/// Patch handle targeting the rc document.
///
/// # Errors
///
/// Fails with plan errors for callbacks holding values of other shapes.
fn rc_impl(callback: Value) -> mlua::Result<LuaPatch> {
    const CTOR: &str = "confit.patch.rc";
    let callback = match callback {
        Value::Function(callback) => callback,
        _ => {
            return Err(plan_err!("{CTOR}: field 'callback' must be a function"));
        }
    };
    Ok(LuaPatch {
        target: "rc".to_string(),
        format: None,
        callback,
        priority: Level::Normal,
    })
}

/// Builds a structured patch handle carrying the callback.
///
/// # Arguments
///
/// * `args` - format value plus path value plus callback value.
///
/// # Returns
///
/// Patch handle targeting the document at the path.
///
/// # Errors
///
/// Fails with plan errors for bad formats plus bad paths plus bad callbacks.
fn structured_impl(args: (Value, Value, Value)) -> mlua::Result<LuaPatch> {
    const CTOR: &str = "confit.patch.structured";
    let (format_value, path_value, callback_value) = args;
    let format = match format_value {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_err!(
                "{CTOR}: field 'format' must be one of 'json', 'toml', or 'yaml'"
            ));
        }
    };
    let structured = match format.as_str() {
        "json" => StructuredFormat::Json,
        "toml" => StructuredFormat::Toml,
        "yaml" => StructuredFormat::Yaml,
        _ => {
            return Err(plan_err!(
                "{CTOR}: field 'format' must be one of 'json', 'toml', or 'yaml'"
            ));
        }
    };
    let path = match path_value {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_err!("{CTOR}: field 'path' must be a string"));
        }
    };
    let callback = match callback_value {
        Value::Function(callback) => callback,
        _ => {
            return Err(plan_err!("{CTOR}: field 'callback' must be a function"));
        }
    };
    Ok(LuaPatch {
        target: path,
        format: Some(structured),
        callback,
        priority: Level::Normal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Error;

    /// Builds a Lua state holding document plus patch namespaces.
    ///
    /// # Returns
    ///
    /// Lua state holding both namespaces.
    fn setup() -> Lua {
        let lua = Lua::new();
        crate::framework::document::install(&lua).expect("install document");
        install(&lua).expect("install patch");
        lua
    }

    /// Evaluates a Lua chunk returning one patch handle.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning one patch handle.
    ///
    /// # Returns
    ///
    /// Patch handle holding chunk data.
    fn eval_patch(lua: &Lua, expr: &str) -> LuaPatch {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::UserData(handle) => handle.borrow::<LuaPatch>().expect("patch").clone(),
            other => panic!("expected patch userdata, got {other:?}"),
        }
    }

    /// Reports plan domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for plan domain failures, false for other failures.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(domain) = err.downcast_ref::<Error>() {
            return matches!(domain, Error::Plan(_));
        }
        format!("{err}").contains("plan error:")
    }

    #[test]
    fn handles_carry_target_format_callback_priority() {
        let lua = setup();
        let patch = eval_patch(
            &lua,
            r#"return confit.patch.rc(function(document) document:set("config", confit.document.rc.alias("cat", "bat")) end)"#,
        );
        assert_eq!(patch.target, "rc");
        assert_eq!(patch.format, None);
        assert_eq!(patch.priority, Level::Normal);
        let structured = eval_patch(
            &lua,
            r#"return confit.patch.structured("json", "x.json", function(data) data:set("a", 1) end)"#,
        );
        assert_eq!(structured.target, "x.json");
        assert_eq!(structured.format, Some(StructuredFormat::Json));
        assert_eq!(structured.priority, Level::Normal);
    }

    #[test]
    fn priority_levels_parse_with_normal_default() {
        let lua = setup();
        let patch = eval_patch(&lua, r#"return confit.patch.rc(function(_) end)"#);
        assert_eq!(patch.priority, Level::Normal);
        for (name, level) in [
            ("MINOR", Level::Minor),
            ("LOW", Level::Low),
            ("NORMAL", Level::Normal),
            ("HIGH", Level::High),
            ("MAJOR", Level::Major),
        ] {
            let patch = eval_patch(
                &lua,
                &format!(
                    r#"return confit.patch.rc(function(_) end):priority(confit.priority.{name})"#
                ),
            );
            assert_eq!(patch.priority, level, "{name}");
        }
    }

    #[test]
    fn unknown_levels_fail_as_plan_errors() {
        let lua = setup();
        for expr in [
            r#"return confit.patch.rc(function(_) end):priority("extreme")"#,
            r#"return confit.patch.rc(function(_) end):priority(42)"#,
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            assert!(
                format!("{err}").contains("priority"),
                "names priority: {expr}: {err}"
            );
        }
    }

    #[test]
    fn constructor_mistakes_fail_as_plan_errors() {
        let lua = setup();
        for (field, expr) in [
            (
                "format",
                r#"return confit.patch.structured("ini", "x.ini", function(_) end)"#,
            ),
            (
                "path",
                r#"return confit.patch.structured("json", 42, function(_) end)"#,
            ),
            ("callback", r#"return confit.patch.rc(42)"#),
            (
                "callback",
                r#"return confit.patch.structured("json", "x.json", 42)"#,
            ),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            assert!(
                format!("{err}").contains(field),
                "names {field}: {expr}: {err}"
            );
        }
    }
}
