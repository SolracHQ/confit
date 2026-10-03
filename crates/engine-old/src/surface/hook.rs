//! Hook
//!
//! Post-config step declarations over argv and opts.

use mlua::{Function, Lua, Table, Value};

use super::confit_table;
use super::handles::LuaRoute;
use super::runtime::{check_condition_json, condition_from_json};
use crate::error::{EngineError, FieldRef, Scope, find_engine};
use crate::lua::{TableExt, ValueExt, set_marker};
use confit_model::arg::Arg;

/// Installs the hook namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set("run", lua.create_function(run_impl)?)?;
    confit.set("hook", namespace)?;
    Ok(())
}

/// Builds one hook declaration table from argv and opts.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped argv and options.
/// - [`EngineError::OptUnknown`] for unknown option fields.
/// - [`EngineError::Duration`] for bad timeouts.
fn run_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.hook.run";
    let scope = Scope::method(CTOR);
    let (argv_value, opts_value) = args;
    let argv = read_slots(&argv_value, &scope, "argv")?;
    if argv.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("argv"),
            want: "must be a dense non-empty string-or-route array",
        }
        .into());
    }
    let opts = match opts_value {
        Value::Nil => lua.create_table()?,
        Value::Table(table) => table,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("opts"),
                want: "must be a table",
            }
            .into());
        }
    };
    check_opts_keys(&opts, &scope)?;
    let path = read_path(&opts, &scope)?;
    let requires = read_requires(lua, &opts, &scope)?;
    let when = read_when(lua, &opts, &scope)?;
    let checks = read_checks(&opts, &scope)?;
    let timeout_secs = read_timeout(&opts, &scope)?;
    let table = lua.create_table()?;
    write_slots(lua, &table, "argv", &argv)?;
    if !path.is_empty() {
        write_slots(lua, &table, "path", &path)?;
    }
    if let Some(guard) = requires {
        table.set("requires", guard)?;
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

/// Reads one dense string-or-route array value into slots.
///
/// Strings run verbatim. Route userdata carries the destination
/// route.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-dense arrays.
pub(crate) fn read_slots(value: &Value, scope: &Scope, field: &str) -> mlua::Result<Vec<Arg>> {
    let field = FieldRef::name(field);
    let table = match value.clone() {
        Value::Table(table) => table,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a dense string-or-route array",
            }
            .into());
        }
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a dense string-or-route array",
            }
            .into());
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a dense string-or-route array starting at 1",
            }
            .into());
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (_, item) in indexed {
        if let Some(text) = item.clone().opt_str() {
            out.push(Arg::Text(text));
            continue;
        }
        if let Some(data) = item.as_userdata()
            && let Ok(route) = data.borrow::<LuaRoute>()
        {
            out.push(Arg::Route(route.core().clone()));
            continue;
        }
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a dense string-or-route array",
        }
        .into());
    }
    Ok(out)
}

/// Writes one slot array into a hook declaration table.
///
/// Text slots land as strings. Route slots land as route userdata.
pub(crate) fn write_slots(
    lua: &Lua,
    table: &Table,
    field: &str,
    slots: &[Arg],
) -> mlua::Result<()> {
    let list = lua.create_table()?;
    for (position, slot) in slots.iter().enumerate() {
        match slot {
            Arg::Text(text) => list.set((position + 1) as i64, text.as_str())?,
            Arg::Route(route) => list.set(
                (position + 1) as i64,
                lua.create_userdata(LuaRoute::from(route.clone()))?,
            )?,
        }
    }
    table.set(field, list)?;
    Ok(())
}

/// Rejects unknown keys on the hook opts table.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string keys.
/// - [`EngineError::OptUnknown`] for unknown fields.
fn check_opts_keys(opts: &Table, scope: &Scope) -> mlua::Result<()> {
    const KNOWN: [&str; 5] = ["path", "requires", "when", "checks", "timeout"];
    let field = FieldRef::name("opts");
    for pair in opts.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let Some(name) = key.opt_str() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "holds a non-string key",
            }
            .into());
        };
        if !KNOWN.contains(&name.as_str()) {
            return Err(EngineError::OptUnknown {
                scope: scope.clone(),
                field,
                name,
            }
            .into());
        }
    }
    Ok(())
}

/// Reads the path extension dirs, defaulting to empty.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped path values.
fn read_path(opts: &Table, scope: &Scope) -> mlua::Result<Vec<Arg>> {
    let value: Value = opts.get("path")?;
    if value.is_nil() {
        return Ok(Vec::new());
    }
    read_slots(&value, scope, "path")
}

/// Reads the capability gate, defaulting to none.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped gates.
fn read_requires(lua: &Lua, opts: &Table, scope: &Scope) -> mlua::Result<Option<Table>> {
    let field = FieldRef::name("requires");
    let value: Value = opts.get("requires")?;
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(func) = value.clone().opt_func() {
        return call_gate_function(lua, scope, field, &func).map(Some);
    }
    let Some(table) = value.opt_table() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a condition table",
        }
        .into());
    };
    let nested = scope.slot(field);
    let json = table.to_json(&nested)?;
    check_condition_json(&json, &nested)?;
    Ok(Some(table))
}

/// Reads the run gate, defaulting to none.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped gates.
fn read_when(lua: &Lua, opts: &Table, scope: &Scope) -> mlua::Result<Option<Table>> {
    let field = FieldRef::name("when");
    let value: Value = opts.get("when")?;
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(func) = value.clone().opt_func() {
        return call_gate_function(lua, scope, field, &func).map(Some);
    }
    let Some(table) = value.opt_table() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a condition table",
        }
        .into());
    };
    let nested = scope.slot(field);
    let json = table.to_json(&nested)?;
    check_condition_json(&json, &nested)?;
    Ok(Some(table))
}

/// Calls one gate builder function with the runtime namespace.
///
/// # Errors
///
/// - [`EngineError::Field`] for missing runtime tables.
/// - [`EngineError::GateFailed`] for failing gate calls.
fn call_gate_function(
    lua: &Lua,
    scope: &Scope,
    field: FieldRef,
    func: &Function,
) -> mlua::Result<Table> {
    let missing = || EngineError::Field {
        scope: scope.clone(),
        field: field.clone(),
        want: "needs the confit.runtime table",
    };
    let confit: Value = lua.globals().get("confit")?;
    let confit = confit
        .req_table(scope, &field.to_string())
        .map_err(|_| missing())?;
    let runtime: Value = confit.get("runtime")?;
    let runtime = runtime
        .req_table(scope, &field.to_string())
        .map_err(|_| missing())?;
    let table = match func.call::<Table>(runtime) {
        Ok(table) => table,
        Err(error) => {
            if find_engine(&error).is_some() {
                return Err(error);
            }
            return Err(EngineError::GateFailed {
                scope: scope.clone(),
                field,
                reason: error.to_string(),
            }
            .into());
        }
    };
    let nested = scope.slot(field);
    let json = table.to_json(&nested)?;
    check_condition_json(&json, &nested)?;
    Ok(table)
}

/// Reads the proof conditions, defaulting to empty.
///
/// # Errors
///
/// - [`EngineError::Field`] for sparse lists and
///   misshaped members.
fn read_checks(opts: &Table, scope: &Scope) -> mlua::Result<Vec<Table>> {
    let value: Value = opts.get("checks")?;
    if value.is_nil() {
        return Ok(Vec::new());
    }
    let field = FieldRef::name("checks");
    let Some(table) = value.opt_table() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field,
            want: "must be a dense condition array",
        }
        .into());
    };
    let dense = || EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("checks"),
        want: "must be a dense condition array starting at 1",
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(dense().into());
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(dense().into());
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (index, item) in indexed {
        let field = FieldRef::index("checks", index as usize);
        let Some(cond) = item.opt_table() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field,
                want: "must be a condition table",
            }
            .into());
        };
        let nested = scope.slot(field);
        let json = cond.to_json(&nested)?;
        check_condition_json(&json, &nested)?;
        out.push(cond);
    }
    Ok(out)
}

/// Parses one Lua-shaped duration into seconds.
///
/// Bare digits read as seconds. Each unit holds at most once.
///
/// # Errors
///
/// - [`EngineError::Duration`] for empty, garbage, and
///   wrong-order text quoting the text.
fn parse_duration(text: &str, scope: &Scope) -> Result<u64, EngineError> {
    let invalid = || EngineError::Duration {
        scope: scope.clone(),
        text: text.to_owned(),
    };
    if text.is_empty() {
        return Err(invalid());
    }
    let mut total: u64 = 0;
    let mut rank: u8 = 4;
    let mut seen: u8 = 0;
    let mut rest = text;
    let mut consumed_any = false;
    while !rest.is_empty() {
        let digits = rest.len()
            - rest
                .trim_start_matches(|byte: char| byte.is_ascii_digit())
                .len();
        if digits == 0 {
            return Err(invalid());
        }
        let amount: u64 = match rest[..digits].parse() {
            Ok(amount) => amount,
            Err(_) => return Err(invalid()),
        };
        rest = &rest[digits..];
        let (unit_rank, unit_bit, factor) = match rest.chars().next() {
            Some('h') => (3, 0b100, 3_600),
            Some('m') => (2, 0b010, 60),
            Some('s') => (1, 0b001, 1),
            _ => (0, 0b000, 1),
        };
        if unit_rank == 0 {
            if consumed_any || !rest.is_empty() {
                return Err(invalid());
            }
            total = amount;
            consumed_any = true;
            rest = "";
            continue;
        }
        if unit_rank >= rank || seen & unit_bit != 0 {
            return Err(invalid());
        }
        rank = unit_rank;
        seen |= unit_bit;
        let part = match amount.checked_mul(factor) {
            Some(part) => part,
            None => return Err(invalid()),
        };
        total = match total.checked_add(part) {
            Some(total) => total,
            None => return Err(invalid()),
        };
        rest = &rest[1..];
        consumed_any = true;
    }
    if consumed_any {
        Ok(total)
    } else {
        Err(invalid())
    }
}

/// Reads the timeout in seconds, defaulting to ten minutes.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-string timeouts.
/// - [`EngineError::Duration`] for bad duration text.
fn read_timeout(opts: &Table, scope: &Scope) -> mlua::Result<u64> {
    const DEFAULT: &str = "10m";
    let value: Value = opts.get("timeout")?;
    if value.is_nil() {
        return Ok(parse_duration(DEFAULT, scope)?);
    }
    let Some(text) = value.opt_str() else {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("timeout"),
            want: "must be a duration string",
        }
        .into());
    };
    Ok(parse_duration(&text, scope)?)
}

/// Converts one hook declaration table into core data.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped hook fields.
pub(crate) fn convert_hook(table: &Table, scope: &Scope) -> mlua::Result<confit_model::hook::Hook> {
    use confit_model::hook::Hook;

    let argv_value: Value = table.get("argv")?;
    let argv = match argv_value.is_nil() {
        true => Vec::new(),
        false => read_slots(&argv_value, scope, "argv")?,
    };
    if argv.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("argv"),
            want: "must be a dense non-empty string-or-route array",
        }
        .into());
    }
    let path_value: Value = table.get("path")?;
    let path = match path_value.is_nil() {
        true => Vec::new(),
        false => read_slots(&path_value, scope, "path")?,
    };
    let requires = match table.get::<Value>("requires")? {
        Value::Nil => None,
        Value::Table(guard) => {
            let nested = scope.slot(FieldRef::name("requires"));
            let json = guard.to_json(&nested)?;
            Some(condition_from_json(&json, &nested)?)
        }
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("requires"),
                want: "must be a condition table",
            }
            .into());
        }
    };
    let when = match table.get::<Value>("when")? {
        Value::Nil => None,
        Value::Table(guard) => {
            let nested = scope.slot(FieldRef::name("when"));
            let json = guard.to_json(&nested)?;
            Some(condition_from_json(&json, &nested)?)
        }
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("when"),
                want: "must be a condition table",
            }
            .into());
        }
    };
    let mut checks = Vec::new();
    match table.get::<Value>("checks")? {
        Value::Nil => {}
        Value::Table(list) => {
            let mut indexed: Vec<(i64, Value)> = Vec::new();
            for pair in list.pairs::<Value, Value>() {
                let (key, item) = pair?;
                let Some(index) = key.as_integer() else {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::name("checks"),
                        want: "must be a dense condition array",
                    }
                    .into());
                };
                indexed.push((index, item));
            }
            indexed.sort_by_key(|(index, _)| *index);
            for (position, (index, _)) in indexed.iter().enumerate() {
                if *index != position as i64 + 1 {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::name("checks"),
                        want: "must be a dense condition array",
                    }
                    .into());
                }
            }
            for (index, item) in indexed {
                let field = FieldRef::index("checks", index as usize);
                let Some(cond) = item.opt_table() else {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field,
                        want: "must be a condition table",
                    }
                    .into());
                };
                let nested = scope.slot(field);
                let json = cond.to_json(&nested)?;
                checks.push(condition_from_json(&json, &nested)?);
            }
        }
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("checks"),
                want: "must be a dense condition array",
            }
            .into());
        }
    }
    let timeout_secs = match table.get::<Value>("timeout_secs")? {
        Value::Integer(secs) if secs >= 0 => secs as u64,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("timeout_secs"),
                want: "must be an integer",
            }
            .into());
        }
    };
    Ok(Hook {
        argv,
        path,
        requires,
        when,
        checks,
        timeout_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parser_cases() {
        let scope = Scope::method("test");
        let cases = vec![
            ("90", 90),
            ("0", 0),
            ("10s", 10),
            ("10m", 600),
            ("2h", 7_200),
            ("1h10m10s", 4_210),
            ("1h30m", 5_400),
        ];
        for (text, want) in cases {
            match parse_duration(text, &scope) {
                Ok(got) => assert_eq!(got, want, "duration {text:?}"),
                Err(error) => panic!("duration {text:?} parses: {error}"),
            }
        }
    }

    #[test]
    fn duration_parser_failures_quote_text() {
        let scope = Scope::method("test");
        for text in [
            "", "nope", "h", "10x", "10s1h", "1m1h", "1h1h", "1h30", " 10m", "10m ",
        ] {
            match parse_duration(text, &scope) {
                Ok(got) => panic!("duration {text:?} passes with {got}"),
                Err(error) => {
                    let message = error.to_string();
                    assert!(
                        message.contains(text) && message.contains('\''),
                        "failure quotes text: {message}"
                    );
                }
            }
        }
    }
}
