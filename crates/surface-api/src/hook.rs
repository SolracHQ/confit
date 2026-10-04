//! Hooks
//! Plain hook tables riding configs beside documents and patches.

use mlua::{Lua, Value};
use mlua_extras::typed::Type;
use serde::Deserialize;

use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::Hook;
use confit_surface_face::module::Named;

use crate::document::decode_named_shape;
use crate::error::{ApiError, Result};

/// One hook face holding argv with gates and timeout.
pub struct HookFace;

confit_surface_face::table_face!(HookFace, {
    required argv: Type::array(Named::<Arg>::ty()),
    optional path: Type::array(Named::<Arg>::ty()),
    optional requires: Named::<Condition>::ty(),
    optional when: Named::<Condition>::ty(),
    optional checks: Type::array(Named::<Condition>::ty()),
    optional timeout: Type::string(),
});

/// The run cap in seconds for an absent timeout.
const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// One hook shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookShape {
    /// Holds the command and arguments in order.
    #[serde(default)]
    argv: Vec<Arg>,
    /// Holds the PATH extension dirs for the subprocess alone.
    #[serde(default)]
    path: Vec<Arg>,
    /// The capability gate. None runs where capable.
    #[serde(default)]
    requires: Option<Condition>,
    /// The need gate. None runs unconditionally.
    #[serde(default)]
    when: Option<Condition>,
    /// Holds the proof conditions, AND by list.
    #[serde(default)]
    checks: Vec<Condition>,
    /// The run cap text. None reads the default cap.
    #[serde(default)]
    timeout: Option<String>,
}

/// Decodes one hook table into its model hook.
///
/// - Missing optionals read defaults.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped hooks.
/// - [`ApiError::Lua`] for Lua read failures.
pub(crate) fn decode_hook(_lua: &Lua, value: &Value) -> Result<Hook> {
    let shape: HookShape = decode_named_shape(value.clone(), "hook")?;
    if shape.argv.is_empty() {
        return Err(ApiError::mismatch(
            "hook argv",
            "a non-empty string-or-route array",
            "table",
        ));
    }
    Ok(Hook {
        argv: shape.argv,
        path: shape.path,
        requires: shape.requires,
        when: shape.when,
        checks: shape.checks,
        timeout_secs: read_timeout(shape.timeout.as_deref())?,
    })
}

/// Decodes one optional timeout text into seconds.
///
/// - Missing timeouts read the default cap.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string and unparseable timeouts.
fn read_timeout(timeout: Option<&str>) -> Result<u64> {
    match timeout {
        None => Ok(DEFAULT_TIMEOUT_SECS),
        Some(text) => parse_duration(text),
    }
}

/// Parses one duration string into seconds.
///
/// - Bare digits read as seconds, suffixed parts read hours, minutes, and
///   seconds in descending order holding each unit at most once.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for empty and misshaped text.
fn parse_duration(text: &str) -> Result<u64> {
    if text.is_empty() {
        return Err(bad_duration(text));
    }
    let mut total: u64 = 0;
    let mut rank: u8 = 4;
    let mut seen: u8 = 0;
    let mut rest = text;
    let mut consumed = false;
    while !rest.is_empty() {
        let digits = rest.len() - rest.trim_start_matches(is_digit).len();
        if digits == 0 {
            return Err(bad_duration(text));
        }
        let amount: u64 = match rest[..digits].parse() {
            Ok(amount) => amount,
            Err(_) => return Err(bad_duration(text)),
        };
        rest = &rest[digits..];
        match unit_of(rest) {
            None => {
                if consumed || !rest.is_empty() {
                    return Err(bad_duration(text));
                }
                total = amount;
                consumed = true;
                rest = "";
            }
            Some((unit_rank, unit_bit, factor)) => {
                if unit_rank >= rank || seen & unit_bit != 0 {
                    return Err(bad_duration(text));
                }
                rank = unit_rank;
                seen |= unit_bit;
                let part = match amount.checked_mul(factor) {
                    Some(part) => part,
                    None => return Err(bad_duration(text)),
                };
                total = match total.checked_add(part) {
                    Some(total) => total,
                    None => return Err(bad_duration(text)),
                };
                rest = &rest[1..];
                consumed = true;
            }
        }
    }
    if consumed {
        Ok(total)
    } else {
        Err(bad_duration(text))
    }
}

/// Reports whether one char holds an ASCII digit.
fn is_digit(byte: char) -> bool {
    byte.is_ascii_digit()
}

/// Returns one duration unit rank with its bit and factor.
fn unit_of(rest: &str) -> Option<(u8, u8, u64)> {
    match rest.chars().next() {
        Some('h') => Some((3, 0b100, 3_600)),
        Some('m') => Some((2, 0b010, 60)),
        Some('s') => Some((1, 0b001, 1)),
        _ => None,
    }
}

/// Builds one duration mismatch quoting its text.
fn bad_duration(text: &str) -> ApiError {
    ApiError::mismatch(
        "hook timeout",
        "a duration like '10m'",
        ["'", text, "'"].concat(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_units_with_bare_seconds() {
        for (text, want) in [
            ("90", 90),
            ("0", 0),
            ("10s", 10),
            ("10m", 600),
            ("2h", 7_200),
            ("1h10m10s", 4_210),
            ("1h30m", 5_400),
        ] {
            match parse_duration(text) {
                Ok(found) => assert_eq!(found, want, "duration parses: {text}"),
                Err(error) => panic!("duration parses {text}: {error}"),
            }
        }
    }

    #[test]
    fn durations_fail_quoting_text() {
        for text in [
            "", "nope", "h", "10x", "10s1h", "1m1h", "1h1h", "1h30", " 10m", "10m ",
        ] {
            match parse_duration(text) {
                Ok(found) => panic!("duration passes {text} with {found}"),
                Err(error) => {
                    let message = error.to_string();
                    assert!(
                        message.contains("hook timeout") && message.contains(text),
                        "failure names the field with the text: {message}"
                    );
                }
            }
        }
    }
}
