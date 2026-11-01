//! Command arguments as text literals or late-bound routes.

use std::borrow::Cow;

use lua_binding::Shape;
use serde::{Deserialize, Serialize};

use crate::routes::Route;

/// The bytes that the shell accepts without quotes.
const SAFE_CHARS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@%_+=:,./-";

/// One command line argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Shape)]
#[serde(untagged)]
#[shape(table, untagged)]
pub enum Arg {
    /// An argument held as literal text.
    Text(String),
    /// An argument held as a route.
    Route(Route),
}

impl Arg {
    /// Renders the argument text for output.
    pub fn display(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Route(route) => route.display(),
        }
    }

    /// Renders the argument list as one quoted line for the shell.
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

/// Wraps one word in single quotes when a byte is unsafe for the shell.
pub fn quote(text: &str) -> Cow<'_, str> {
    if !text.is_empty() && text.bytes().all(|byte| SAFE_CHARS.contains(&byte)) {
        Cow::Borrowed(text)
    } else if text.is_empty() {
        Cow::Borrowed("''")
    } else {
        Cow::Owned(format!("'{}'", text.replace('\'', "'\\''")))
    }
}
