//! Stubs from live shapes.

use confit_core::error::{ContextualCause, ContextualResult};
use lua_binding::stub::Stub;

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::global::Global;
use crate::api::input::Input;
use crate::api::open::Open;
use crate::api::template::Template;
use crate::api::validate::Validate;

/// Generates stubs from live shapes.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::LuaFailed`] for stub parse failures.
pub fn generate_stubs() -> ContextualResult<Vec<Stub>> {
    let stubs = vec![
        Global::stub(),
        Fetch::stub(),
        Open::stub(),
        Validate::stub(),
        Fact::stub(),
        Input::stub(),
        Template::stub(),
    ];
    for stub in &stubs {
        check_parse(&stub.module, &stub.text)?;
    }
    Ok(stubs)
}

/// Checks one stub parses as Lua.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::LuaFailed`] for stub parse failures.
fn check_parse(module: &str, text: &str) -> ContextualResult<()> {
    let lua = mlua::Lua::new();
    lua.load(text)
        .set_name(format!("@{module}"))
        .exec()
        .map_err(ContextualCause::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_stub_holds_meta_header_with_bare_return() {
        let stub = Fetch::stub();
        assert_eq!(stub.module, "confit.fetch");
        assert!(
            stub.text.starts_with("--- @meta confit.fetch\n"),
            "module stub holds the meta header"
        );
        assert!(
            stub.text.ends_with("\nreturn fetch\n"),
            "single functions return the bare name"
        );
    }

    #[test]
    fn global_stub_holds_bare_header_with_no_return() {
        let stub = Global::stub();
        assert_eq!(stub.module, "confit");
        assert!(
            stub.text.starts_with("--- @meta _\n"),
            "global stub holds the bare header"
        );
        assert!(
            !stub.text.contains("\nreturn "),
            "global stub holds no return line"
        );
    }
}
