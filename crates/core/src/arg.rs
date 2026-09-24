//! Arg
//!
//! Text literal or late-bound route for command arguments.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::handles::Route;

/// Shell-safe bytes passing through unquoted.
const SAFE_CHARS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@%_+=:,./-";

/// One command argument holding text or a late-bound route.
///
/// Text runs verbatim. Routes resolve through the workspace
/// at apply time and render as portable `base:relative`
/// display in previews and manifests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    /// Holds a plain string under running verbatim.
    Text(String),
    /// Holds a destination route under apply-time resolving.
    Route(Route),
}

impl Arg {
    /// Renders the portable argument display.
    pub fn display(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Route(route) => route.display(),
        }
    }

    /// Renders argv slots as one shell-safe display line.
    ///
    /// Slots holding shell-special text quote single, so the
    /// line pastes back into the shown command.
    pub fn join(argv: &[Arg]) -> String {
        argv.iter()
            .map(Arg::display)
            .map(|text| quote(&text).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl From<String> for Arg {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<Route> for Arg {
    fn from(route: Route) -> Self {
        Self::Route(route)
    }
}

/// Shell quoting for one word.
///
/// Safe text holds alphanumerics plus `@%_+=:,./-` only.
pub(crate) fn quote(text: &str) -> Cow<'_, str> {
    if !text.is_empty() && text.bytes().all(|byte| SAFE_CHARS.contains(&byte)) {
        Cow::Borrowed(text)
    } else if text.is_empty() {
        Cow::Borrowed("''")
    } else {
        Cow::Owned(format!("'{}'", text.replace('\'', "'\\''")))
    }
}
