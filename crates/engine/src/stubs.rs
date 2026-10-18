//! Stubs
//! Stub documents from live faces.

use confit_binding::error::Result as BindingResult;
use confit_binding::registry::Registry;
use confit_binding::stub::{StubDocument, StubEmitter};

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::open::Open;
use crate::api::template::Template;
use crate::api::validate::Validate;
use crate::error::{EngineError, Result};

/// Generate stub documents from live faces.
///
/// - Documents arrive in namespace order.
/// - Every document parses as Lua.
///
/// # Errors
///
/// - [`crate::error::EngineError::Binding`] for repeated faces.
/// - [`crate::error::EngineError::Lua`] for stub parse failures.
pub fn generate_stubs() -> Result<Vec<StubDocument>> {
    let documents = vec![
        module_document(Fetch::NAMESPACE, Fetch::VERBS, Fetch::stubs)?,
        module_document(Open::NAMESPACE, Open::VERBS, Open::stubs)?,
        module_document(Validate::NAMESPACE, Validate::VERBS, Validate::stubs)?,
        module_document(Fact::NAMESPACE, Fact::VERBS, Fact::stubs)?,
        module_document(Template::NAMESPACE, Template::VERBS, Template::stubs)?,
    ];
    for document in &documents {
        check_parse(&document.namespace, &document.text)?;
    }
    Ok(documents)
}

/// Build one stub document from its module face.
///
/// # Errors
///
/// - [`crate::error::EngineError::Binding`] for repeated faces.
fn module_document(
    namespace: &'static str,
    verbs: &[&str],
    collect: fn(&mut Registry) -> BindingResult<()>,
) -> Result<StubDocument> {
    let mut registry = Registry::new();
    collect(&mut registry)?;
    Ok(emit(namespace, &registry, &return_line(verbs)))
}

/// Build the return line for one verb list.
///
/// Single verbs return the bare name and verb lists
/// return the verb table.
fn return_line(verbs: &[&str]) -> String {
    match verbs {
        [single] => format!("return {single}"),
        _ => {
            let entries: Vec<String> = verbs
                .iter()
                .map(|verb| format!("{verb} = {verb}"))
                .collect();
            format!("return {{{}}}", entries.join(", "))
        }
    }
}

/// Emit one stub document with its return line.
fn emit(namespace: &str, registry: &Registry, statement: &str) -> StubDocument {
    let emitter = StubEmitter::new(format!("--- @meta {namespace}"));
    let mut document = emitter.emit(namespace, registry);
    document.text.push('\n');
    document.text.push_str(statement);
    document.text.push('\n');
    document
}

/// Check one stub document parses as Lua.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for stub parse failures.
fn check_parse(namespace: &str, text: &str) -> Result<()> {
    let lua = mlua::Lua::new();
    lua.load(text)
        .set_name(format!("@{namespace}"))
        .exec()
        .map_err(|error| {
            EngineError::Lua(mlua::Error::RuntimeError(format!(
                "stub '{namespace}' fails to parse: {error}"
            )))
        })
}
