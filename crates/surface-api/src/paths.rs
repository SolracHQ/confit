//! Paths
//! Route decode helpers over host bases.

use mlua::{Lua, Value};

use confit_model::routes::Route;

pub use confit_model::routes::{SegmentFault, split_checked};

use crate::error::{ApiError, Result};

/// Decodes one route table into its route.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for foreign tables, bases and segments.
pub(crate) fn decode_route(value: &Value, field: &str) -> Result<Route> {
    serde_path_to_error::deserialize(mlua::serde::Deserializer::new(value.clone())).map_err(
        |error: serde_path_to_error::Error<mlua::Error>| {
            let detail = error.inner().to_string();
            ApiError::mismatch(field, detail.clone(), detail)
        },
    )
}

/// Builds one live route table from its route.
///
/// # Errors
///
/// - [`mlua::Error`] for table build failures.
pub(crate) fn route_value(lua: &Lua, route: &Route) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    table.set("base", route.base().name())?;
    table.set("relative", route.relative().display().to_string())?;
    Ok(Value::Table(table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forged_tables_fail_naming_field() {
        let lua = Lua::new();
        let value: Value = lua
            .load("return { base = \"home\", relative = \"../out\" }")
            .call(())
            .unwrap();
        match decode_route(&value, "document destination") {
            Ok(_) => panic!("forged parent passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("document destination"), "names field: {text}");
                assert!(
                    text.contains("valid relative segments"),
                    "names want: {text}"
                );
            }
        }
        let value: Value = lua
            .load("return { base = \"nope\", relative = \"a\" }")
            .call(())
            .unwrap();
        match decode_route(&value, "document destination") {
            Ok(_) => panic!("foreign base passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("known base"), "names want: {text}");
                assert!(text.contains("nope"), "names received: {text}");
            }
        }
    }
}
