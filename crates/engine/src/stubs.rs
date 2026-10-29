//! Stub documents from live faces.

use confit_binding::stub::StubDocument;
use confit_core::error::{ContextualCause, ContextualResult};

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::global::Global;
use crate::api::open::Open;
use crate::api::template::Template;
use crate::api::validate::Validate;

/// Generate stub documents from live faces.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::LuaFailed`] for stub parse failures.
pub fn generate_stubs() -> ContextualResult<Vec<StubDocument>> {
    let documents = vec![
        Global::stub_document(),
        Fetch::stub_document(),
        Open::stub_document(),
        Validate::stub_document(),
        Fact::stub_document(),
        Template::stub_document(),
    ];
    for document in &documents {
        check_parse(&document.namespace, &document.text)?;
    }
    Ok(documents)
}

/// Check one stub document parses as Lua.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::LuaFailed`] for stub parse failures.
fn check_parse(namespace: &str, text: &str) -> ContextualResult<()> {
    let lua = mlua::Lua::new();
    lua.load(text)
        .set_name(format!("@{namespace}"))
        .exec()
        .map_err(ContextualCause::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_document_holds_meta_header_with_bare_return() {
        let document = Fetch::stub_document();
        assert_eq!(document.namespace, "confit.fetch");
        assert!(
            document.text.starts_with("--- @meta confit.fetch\n"),
            "namespace document holds the meta header"
        );
        assert!(
            document.text.ends_with("\nreturn fetch\n"),
            "single verbs return the bare name"
        );
    }

    #[test]
    fn globals_document_holds_bare_header_with_no_return() {
        let document = Global::stub_document();
        assert_eq!(document.namespace, "confit");
        assert!(
            document.text.starts_with("--- @meta _\n"),
            "globals document holds the bare header"
        );
        assert!(
            !document.text.contains("\nreturn "),
            "globals document holds no return line"
        );
    }
}
