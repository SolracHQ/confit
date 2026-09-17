//! Hook
//!
//! Post-config step declarations over argv plus opts.

use mlua::{Function, Lua, Table, Value};
use serde_json::Value as Json;

use super::confit_table;
use super::runtime::{check_condition_json, condition_from_json};
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt, set_marker};

/// Installs the hook namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set("run", lua.create_function(run_impl)?)?;
    confit.set("hook", namespace)?;
    Ok(())
}

/// Builds one hook declaration table from argv plus opts.
fn run_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.hook.run";
    let (argv_value, opts_value) = args;
    let argv = argv_value.req_string_array(CTOR, "argv").map_err(|_| {
        plan_error(format!(
            "{CTOR}: field 'argv' must be a dense non-empty string array"
        ))
    })?;
    if argv.is_empty() {
        return Err(plan_error(format!(
            "{CTOR}: field 'argv' must be a dense non-empty string array"
        )));
    }
    let opts = match opts_value {
        Value::Nil => lua.create_table()?,
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(format!("{CTOR}: field 'opts' must be a table")));
        }
    };
    check_opts_keys(&opts, CTOR)?;
    let path = read_path(&opts, CTOR)?;
    let when = read_when(lua, &opts, CTOR)?;
    let checks = read_checks(&opts, CTOR)?;
    let timeout_secs = read_timeout(&opts, CTOR)?;
    let table = lua.create_table()?;
    let argv_table = lua.create_table()?;
    for (position, item) in argv.iter().enumerate() {
        argv_table.set((position + 1) as i64, item.as_str())?;
    }
    table.set("argv", argv_table)?;
    if !path.is_empty() {
        let path_table = lua.create_table()?;
        for (position, item) in path.iter().enumerate() {
            path_table.set((position + 1) as i64, item.as_str())?;
        }
        table.set("path", path_table)?;
    }
    if let Some(guard) = when {
        table.set("when", guard)?;
    }
    if !checks.is_empty() {
        let checks_table = lua.create_table()?;
        for (position, item) in checks.into_iter().enumerate() {
            checks_table.set((position + 1) as i64, item)?;
        }
        table.set("checks", checks_table)?;
    }
    table.set("timeout_secs", timeout_secs)?;
    set_marker(lua, &table, "hook", None)?;
    Ok(table)
}

/// Rejects unknown keys on the hook opts table.
fn check_opts_keys(opts: &Table, ctor: &str) -> mlua::Result<()> {
    const KNOWN: [&str; 4] = ["path", "when", "checks", "timeout"];
    for pair in opts.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let Some(name) = key.opt_str() else {
            return Err(plan_error(format!(
                "{ctor}: field 'opts' holds a non-string key"
            )));
        };
        if !KNOWN.contains(&name.as_str()) {
            return Err(plan_error(format!(
                "{ctor}: field 'opts' unknown field '{name}'"
            )));
        }
    }
    Ok(())
}

/// Reads the path extension dirs, defaulting to empty.
fn read_path(opts: &Table, ctor: &str) -> mlua::Result<Vec<String>> {
    let value: Value = opts.get("path")?;
    if value.is_nil() {
        return Ok(Vec::new());
    }
    value
        .req_string_array(ctor, "path")
        .map_err(|_| plan_error(format!("{ctor}: field 'path' must be a dense string array")))
}

/// Reads the run gate, defaulting to none.
fn read_when(lua: &Lua, opts: &Table, ctor: &str) -> mlua::Result<Option<Table>> {
    let value: Value = opts.get("when")?;
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(func) = value.clone().opt_func() {
        return call_when_function(lua, ctor, &func).map(Some);
    }
    let Some(table) = value.opt_table() else {
        return Err(plan_error(format!(
            "{ctor}: field 'when' must be a condition table"
        )));
    };
    let json = table
        .to_json(&format!("{ctor}: field 'when'"))
        .map_err(|error| plan_error(format!("{ctor}: field 'when' {error}")))?;
    check_condition_json(&json, &format!("{ctor}: field 'when'"))
        .map_err(|detail| plan_error(format!("{ctor}: field 'when' {detail}")))?;
    Ok(Some(table))
}

/// Calls one `when` builder function with the runtime namespace.
fn call_when_function(lua: &Lua, ctor: &str, func: &Function) -> mlua::Result<Table> {
    let missing = || {
        plan_error(format!(
            "{ctor}: field 'when' needs the confit.runtime table"
        ))
    };
    let confit: Value = lua.globals().get("confit")?;
    let confit = confit.req_table(ctor, "when").map_err(|_| missing())?;
    let runtime: Value = confit.get("runtime")?;
    let runtime = runtime.req_table(ctor, "when").map_err(|_| missing())?;
    let table = match func.call::<Table>(runtime) {
        Ok(table) => table,
        Err(error) => {
            if crate::error::find_plan(&error).is_some() {
                return Err(error);
            }
            return Err(plan_error(format!("{ctor}: field 'when' failed: {error}")));
        }
    };
    let json = table
        .to_json(&format!("{ctor}: field 'when'"))
        .map_err(|error| plan_error(format!("{ctor}: field 'when' {error}")))?;
    check_condition_json(&json, &format!("{ctor}: field 'when'"))
        .map_err(|detail| plan_error(format!("{ctor}: field 'when' {detail}")))?;
    Ok(table)
}

/// Reads the proof conditions, defaulting to empty.
fn read_checks(opts: &Table, ctor: &str) -> mlua::Result<Vec<Table>> {
    let value: Value = opts.get("checks")?;
    if value.is_nil() {
        return Ok(Vec::new());
    }
    let Some(table) = value.opt_table() else {
        return Err(plan_error(format!(
            "{ctor}: field 'checks' must be a dense condition array"
        )));
    };
    let dense = || {
        plan_error(format!(
            "{ctor}: field 'checks' must be a dense condition array starting at 1"
        ))
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(dense());
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(dense());
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (index, item) in indexed {
        let field = format!("checks[{index}]");
        let Some(cond) = item.opt_table() else {
            return Err(plan_error(format!(
                "{ctor}: field '{field}' must be a condition table"
            )));
        };
        let json = cond
            .to_json(&format!("{ctor}: field '{field}'"))
            .map_err(|error| plan_error(format!("{ctor}: field '{field}' {error}")))?;
        check_condition_json(&json, &format!("{ctor}: field '{field}'"))
            .map_err(|detail| plan_error(format!("{ctor}: field '{field}' {detail}")))?;
        out.push(cond);
    }
    Ok(out)
}

/// Reads the timeout in seconds, defaulting to ten minutes.
fn read_timeout(opts: &Table, ctor: &str) -> mlua::Result<u64> {
    const DEFAULT: &str = "10m";
    let value: Value = opts.get("timeout")?;
    if value.is_nil() {
        return match confit_core::runtime::parse_duration(DEFAULT) {
            Ok(secs) => Ok(secs),
            Err(error) => Err(plan_error(format!("{ctor}: {error}"))),
        };
    }
    let Some(text) = value.opt_str() else {
        return Err(plan_error(format!(
            "{ctor}: field 'timeout' must be a duration string"
        )));
    };
    confit_core::runtime::parse_duration(&text)
        .map_err(|error| plan_error(format!("{ctor}: field 'timeout' {error}")))
}

/// Converts one hook declaration table into core data.
pub(crate) fn convert_hook(table: &Table, ctx: &str) -> mlua::Result<confit_core::hook::Hook> {
    use confit_core::hook::Hook;

    let json = table
        .to_json(&format!("{ctx}: convert"))
        .map_err(|error| plan_error(format!("{ctx}: convert {error}")))?;
    let map = match json.as_object() {
        Some(map) => map,
        None => return Err(plan_error(format!("{ctx} must be a confit.hook value"))),
    };
    let strings = |field: &str| -> mlua::Result<Vec<String>> {
        let mut out = Vec::new();
        match map.get(field) {
            None | Some(Json::Null) => return Ok(out),
            Some(Json::Array(items)) => {
                for item in items {
                    match item.as_str() {
                        Some(text) => out.push(text.to_string()),
                        None => {
                            return Err(plan_error(format!(
                                "{ctx}: field '{field}' must be a string array"
                            )));
                        }
                    }
                }
            }
            Some(_) => {
                return Err(plan_error(format!(
                    "{ctx}: field '{field}' must be a string array"
                )));
            }
        }
        Ok(out)
    };
    let argv = strings("argv")?;
    if argv.is_empty() {
        return Err(plan_error(format!(
            "{ctx}: field 'argv' must be a dense non-empty string array"
        )));
    }
    let path = strings("path")?;
    let when = match map.get("when") {
        None | Some(Json::Null) => None,
        Some(cond) => Some(condition_from_json(cond, &format!("{ctx}: field 'when'"))?),
    };
    let mut checks = Vec::new();
    match map.get("checks") {
        None | Some(Json::Null) => {}
        Some(Json::Array(items)) => {
            for (position, item) in items.iter().enumerate() {
                checks.push(condition_from_json(
                    item,
                    &format!("{ctx}: field 'checks[{}]'", position + 1),
                )?);
            }
        }
        Some(_) => {
            return Err(plan_error(format!(
                "{ctx}: field 'checks' must be a dense condition array"
            )));
        }
    }
    let timeout_secs = match map.get("timeout_secs").and_then(Json::as_u64) {
        Some(secs) => secs,
        None => {
            return Err(plan_error(format!(
                "{ctx}: field 'timeout_secs' must be an integer"
            )));
        }
    };
    Ok(Hook {
        argv,
        path,
        when,
        checks,
        timeout_secs,
    })
}
