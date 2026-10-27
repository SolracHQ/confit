//! Arg
//! Text literal or late-bound route for command arguments.

use std::borrow::Cow;

use confit_binding_macros::Face;
use serde::{Deserialize, Serialize};

use crate::routes::Route;

/// Shell-safe bytes passing through unquoted.
const SAFE_CHARS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@%_+=:,./-";

/// Text or route for command arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Face)]
#[serde(untagged)]
#[face(table, untagged)]
pub enum Arg {
    /// Plain string argument.
    Text(String),
    /// Destination route argument.
    Route(Route),
}

impl Arg {
    /// Render the portable argument display.
    pub fn display(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Route(route) => route.display(),
        }
    }

    /// Render argv args as one shell-safe display line.
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

/// Quote one word for shell use.
pub fn quote(text: &str) -> Cow<'_, str> {
    if !text.is_empty() && text.bytes().all(|byte| SAFE_CHARS.contains(&byte)) {
        Cow::Borrowed(text)
    } else if text.is_empty() {
        Cow::Borrowed("''")
    } else {
        Cow::Owned(format!("'{}'", text.replace('\'', "'\\''")))
    }
}
