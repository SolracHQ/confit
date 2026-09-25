//! Hook
//!
//! Post-config step declarations over argv and opts.

use mlua::{Function, Lua, Table, Value};

use super::confit_table;
use super::handles::LuaRoute;
use super::runtime::{check_condition_json, condition_from_json};
use crate::error::plan_error;
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
fn run_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.hook.run";
    let (argv_value, opts_value) = args;
    let argv = read_slots(&argv_value, CTOR, "argv")?;
    if argv.is_empty() {
        return Err(plan_error(format!(
            "{CTOR}: field 'argv' must be a dense non-empty string-or-route array"
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
    let requires = read_requires(lua, &opts, CTOR)?;
    let when = read_when(lua, &opts, CTOR)?;
    let checks = read_checks(&opts, CTOR)?;
    let timeout_secs = read_timeout(&opts, CTOR)?;
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
/// route. Anything else fails as a plan error naming the field.
pub(crate) fn read_slots(value: &Value, ctor: &str, field: &str) -> mlua::Result<Vec<Arg>> {
    const DENSE: &str = "must be a dense string-or-route array";
    let table = match value.clone() {
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(format!("{ctor}: field '{field}' {DENSE}")));
        }
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        let Some(index) = key.as_integer() else {
            return Err(plan_error(format!("{ctor}: field '{field}' {DENSE}")));
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(plan_error(format!(
                "{ctor}: field '{field}' must be a dense string-or-route array starting at 1"
            )));
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
        return Err(plan_error(format!("{ctor}: field '{field}' {DENSE}")));
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
fn check_opts_keys(opts: &Table, ctor: &str) -> mlua::Result<()> {
    const KNOWN: [&str; 5] = ["path", "requires", "when", "checks", "timeout"];
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
fn read_path(opts: &Table, ctor: &str) -> mlua::Result<Vec<Arg>> {
    let value: Value = opts.get("path")?;
    if value.is_nil() {
        return Ok(Vec::new());
    }
    read_slots(&value, ctor, "path")
}

/// Reads the capability gate, defaulting to none.
fn read_requires(lua: &Lua, opts: &Table, ctor: &str) -> mlua::Result<Option<Table>> {
    let value: Value = opts.get("requires")?;
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(func) = value.clone().opt_func() {
        return call_gate_function(lua, ctor, "requires", &func).map(Some);
    }
    let Some(table) = value.opt_table() else {
        return Err(plan_error(format!(
            "{ctor}: field 'requires' must be a condition table"
        )));
    };
    let json = table
        .to_json(&format!("{ctor}: field 'requires'"))
        .map_err(|error| plan_error(format!("{ctor}: field 'requires' {error}")))?;
    check_condition_json(&json, &format!("{ctor}: field 'requires'"))
        .map_err(|detail| plan_error(format!("{ctor}: field 'requires' {detail}")))?;
    Ok(Some(table))
}

/// Reads the run gate, defaulting to none.
fn read_when(lua: &Lua, opts: &Table, ctor: &str) -> mlua::Result<Option<Table>> {
    let value: Value = opts.get("when")?;
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(func) = value.clone().opt_func() {
        return call_gate_function(lua, ctor, "when", &func).map(Some);
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

/// Calls one gate builder function with the runtime namespace.
fn call_gate_function(lua: &Lua, ctor: &str, field: &str, func: &Function) -> mlua::Result<Table> {
    let missing = || {
        plan_error(format!(
            "{ctor}: field '{field}' needs the confit.runtime table"
        ))
    };
    let confit: Value = lua.globals().get("confit")?;
    let confit = confit.req_table(ctor, field).map_err(|_| missing())?;
    let runtime: Value = confit.get("runtime")?;
    let runtime = runtime.req_table(ctor, field).map_err(|_| missing())?;
    let table = match func.call::<Table>(runtime) {
        Ok(table) => table,
        Err(error) => {
            if crate::error::find_plan(&error).is_some() {
                return Err(error);
            }
            return Err(plan_error(format!(
                "{ctor}: field '{field}' failed: {error}"
            )));
        }
    };
    let json = table
        .to_json(&format!("{ctor}: field '{field}'"))
        .map_err(|error| plan_error(format!("{ctor}: field '{field}' {error}")))?;
    check_condition_json(&json, &format!("{ctor}: field '{field}'"))
        .map_err(|detail| plan_error(format!("{ctor}: field '{field}' {detail}")))?;
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

/// Parses one Lua-shaped duration into seconds.
///
/// Bare digits read as seconds. Each unit holds at most once.
///
/// # Errors
///
/// Empty, garbage, and wrong order fail with the text quoted.
fn parse_duration(text: &str) -> Result<u64, String> {
    if text.is_empty() {
        return Err(format!("invalid duration '{text}'"));
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
            return Err(format!("invalid duration '{text}'"));
        }
        let amount: u64 = match rest[..digits].parse() {
            Ok(amount) => amount,
            Err(_) => return Err(format!("invalid duration '{text}'")),
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
                return Err(format!("invalid duration '{text}'"));
            }
            total = amount;
            consumed_any = true;
            rest = "";
            continue;
        }
        if unit_rank >= rank || seen & unit_bit != 0 {
            return Err(format!("invalid duration '{text}'"));
        }
        rank = unit_rank;
        seen |= unit_bit;
        let part = match amount.checked_mul(factor) {
            Some(part) => part,
            None => return Err(format!("invalid duration '{text}'")),
        };
        total = match total.checked_add(part) {
            Some(total) => total,
            None => return Err(format!("invalid duration '{text}'")),
        };
        rest = &rest[1..];
        consumed_any = true;
    }
    if consumed_any {
        Ok(total)
    } else {
        Err(format!("invalid duration '{text}'"))
    }
}

/// Reads the timeout in seconds, defaulting to ten minutes.
fn read_timeout(opts: &Table, ctor: &str) -> mlua::Result<u64> {
    const DEFAULT: &str = "10m";
    let value: Value = opts.get("timeout")?;
    if value.is_nil() {
        return match parse_duration(DEFAULT) {
            Ok(secs) => Ok(secs),
            Err(error) => Err(plan_error(format!("{ctor}: {error}"))),
        };
    }
    let Some(text) = value.opt_str() else {
        return Err(plan_error(format!(
            "{ctor}: field 'timeout' must be a duration string"
        )));
    };
    parse_duration(&text).map_err(|error| plan_error(format!("{ctor}: field 'timeout' {error}")))
}

/// Converts one hook declaration table into core data.
pub(crate) fn convert_hook(table: &Table, ctx: &str) -> mlua::Result<confit_model::hook::Hook> {
    use confit_model::hook::Hook;

    let argv_value: Value = table.get("argv")?;
    let argv = match argv_value.is_nil() {
        true => Vec::new(),
        false => read_slots(&argv_value, ctx, "argv")?,
    };
    if argv.is_empty() {
        return Err(plan_error(format!(
            "{ctx}: field 'argv' must be a dense non-empty string-or-route array"
        )));
    }
    let path_value: Value = table.get("path")?;
    let path = match path_value.is_nil() {
        true => Vec::new(),
        false => read_slots(&path_value, ctx, "path")?,
    };
    let requires = match table.get::<Value>("requires")? {
        Value::Nil => None,
        Value::Table(guard) => {
            let json = guard
                .to_json(&format!("{ctx}: field 'requires'"))
                .map_err(|error| plan_error(format!("{ctx}: field 'requires' {error}")))?;
            Some(condition_from_json(
                &json,
                &format!("{ctx}: field 'requires'"),
            )?)
        }
        _ => {
            return Err(plan_error(format!(
                "{ctx}: field 'requires' must be a condition table"
            )));
        }
    };
    let when = match table.get::<Value>("when")? {
        Value::Nil => None,
        Value::Table(guard) => {
            let json = guard
                .to_json(&format!("{ctx}: field 'when'"))
                .map_err(|error| plan_error(format!("{ctx}: field 'when' {error}")))?;
            Some(condition_from_json(&json, &format!("{ctx}: field 'when'"))?)
        }
        _ => {
            return Err(plan_error(format!(
                "{ctx}: field 'when' must be a condition table"
            )));
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
                    return Err(plan_error(format!(
                        "{ctx}: field 'checks' must be a dense condition array"
                    )));
                };
                indexed.push((index, item));
            }
            indexed.sort_by_key(|(index, _)| *index);
            for (position, (index, _)) in indexed.iter().enumerate() {
                if *index != position as i64 + 1 {
                    return Err(plan_error(format!(
                        "{ctx}: field 'checks' must be a dense condition array"
                    )));
                }
            }
            for (index, item) in indexed {
                let Some(cond) = item.opt_table() else {
                    return Err(plan_error(format!(
                        "{ctx}: field 'checks[{index}]' must be a condition table"
                    )));
                };
                let json = cond
                    .to_json(&format!("{ctx}: field 'checks[{index}]'"))
                    .map_err(|error| {
                        plan_error(format!("{ctx}: field 'checks[{index}]' {error}"))
                    })?;
                checks.push(condition_from_json(
                    &json,
                    &format!("{ctx}: field 'checks[{index}]'"),
                )?);
            }
        }
        _ => {
            return Err(plan_error(format!(
                "{ctx}: field 'checks' must be a dense condition array"
            )));
        }
    }
    let timeout_secs = match table.get::<Value>("timeout_secs")? {
        Value::Integer(secs) if secs >= 0 => secs as u64,
        _ => {
            return Err(plan_error(format!(
                "{ctx}: field 'timeout_secs' must be an integer"
            )));
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
            match parse_duration(text) {
                Ok(got) => assert_eq!(got, want, "duration {text:?}"),
                Err(error) => panic!("duration {text:?} parses: {error}"),
            }
        }
    }

    #[test]
    fn duration_parser_failures_quote_text() {
        for text in [
            "", "nope", "h", "10x", "10s1h", "1m1h", "1h1h", "1h30", " 10m", "10m ",
        ] {
            match parse_duration(text) {
                Ok(got) => panic!("duration {text:?} passes with {got}"),
                Err(error) => assert!(
                    error.contains(text) && error.contains('\''),
                    "failure quotes text: {error}"
                ),
            }
        }
    }
}
