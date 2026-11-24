//! Lua call stacks attached to constructed values.
//!
//! The `#[shape(traced)]` marker records the Lua frames above a constructor
//! call in a `LuaTrace` field.

use std::fmt::{Display, Formatter};

use mlua::Lua;

/// One Lua source site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSite {
    /// The chunk path.
    pub file: String,
    /// The line in the chunk.
    pub line: String,
}

/// The Lua call stack above a constructed value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LuaTrace(pub Vec<SourceSite>);

impl LuaTrace {
    /// Captures the Lua frames above the current call.
    ///
    /// Returns `None` when no Lua source frame is present.
    pub fn capture(lua: &Lua) -> Option<Self> {
        let mut sites = Vec::new();
        let mut level: usize = 1;
        loop {
            let found = lua.inspect_stack(level, |debug| {
                let source = debug.source();
                let what = source.what.to_owned();
                let file = source.short_src.map(|text| {
                    let owned = text.into_owned();
                    match owned.strip_prefix('@') {
                        Some(trimmed) => trimmed.to_owned(),
                        None => owned,
                    }
                });
                let line = debug.current_line();
                (what, file, line)
            });
            let Some((what, file, line)) = found else {
                break;
            };
            if what == "Lua" || what == "main" {
                if let Some(file) = file {
                    sites.push(SourceSite {
                        file,
                        line: line.unwrap_or(0).to_string(),
                    });
                }
                level += 1;
            } else {
                break;
            }
        }
        if sites.is_empty() {
            None
        } else {
            Some(Self(sites))
        }
    }
}

impl Display for SourceSite {
    /// Renders the site as `file:line`.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.file, self.line)
    }
}
