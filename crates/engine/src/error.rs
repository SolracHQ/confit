//! Error
//!
//! Plan domain errors plus nested plan lookup.

/// Builds a core plan domain error from a message.
pub(crate) fn plan(message: impl Into<String>) -> confit_core::error::Error {
    confit_core::error::Error::Plan(message.into())
}

/// Builds a plan domain error from a message.
pub(crate) fn plan_error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::external(plan(message))
}

/// Finds one plan message walking nested error sources.
pub(crate) fn find_plan(error: &mlua::Error) -> Option<String> {
    use std::error::Error as StdError;
    let mut current: Option<&dyn StdError> = Some(error);
    while let Some(node) = current {
        if let Some(domain) = node.downcast_ref::<confit_core::error::Error>()
            && let confit_core::error::Error::Plan(message) = domain
        {
            return Some(message.clone());
        }
        current = node.source();
    }
    None
}
