//! Marks and paint helpers.

use std::sync::OnceLock;

use confit_differ::change::{Change, ChangeKind};
use extend::ext;

/// Paint clear code.
const RESET: &str = "\x1b[0m";

/// Plan symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// Added rows.
    Add,
    /// Changed rows.
    Update,
    /// Removed rows.
    Remove,
    /// Incomparable rows.
    Query,
}

/// Write state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Row on disk.
    Applied,
    /// Failed row.
    Failed,
    /// Unattempted row.
    Unattempted,
}

/// Line marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    /// Write state.
    pub outcome: Option<Outcome>,
    /// Plan symbol.
    pub symbol: Option<Symbol>,
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Symbol::Add => write!(f, "{}", "+".add()),
            Symbol::Update => write!(f, "{}", "~".change()),
            Symbol::Remove => write!(f, "{}", "-".delete()),
            Symbol::Query => write!(f, "{}", "?".delete()),
        }
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Applied => write!(f, "{}", "✓".add()),
            Outcome::Failed => write!(f, "{}", "✗".fail()),
            Outcome::Unattempted => write!(f, "{}", "·".dim()),
        }
    }
}

impl From<ChangeKind> for Symbol {
    fn from(kind: ChangeKind) -> Self {
        match kind {
            ChangeKind::Addition => Symbol::Add,
            ChangeKind::Change => Symbol::Update,
            ChangeKind::Removal => Symbol::Remove,
        }
    }
}

impl Symbol {
    /// Returns the symbol for grouped members.
    pub fn from_members(members: &[&Change]) -> Self {
        debug_assert!(!members.is_empty());
        if members
            .iter()
            .all(|member| matches!(member.kind, ChangeKind::Addition))
        {
            Symbol::Add
        } else if members
            .iter()
            .all(|member| matches!(member.kind, ChangeKind::Removal))
        {
            Symbol::Remove
        } else {
            Symbol::Update
        }
    }
}

impl Marks {
    /// Returns the plan marks.
    pub fn plan(symbol: Symbol) -> Self {
        Self {
            outcome: None,
            symbol: Some(symbol),
        }
    }
}

#[ext]
pub impl str {
    /// Paints the header.
    fn header(&self) -> String {
        paint(self, "1")
    }

    /// Paints the line.
    fn dim(&self) -> String {
        paint(self, "2")
    }

    /// Paints the failure.
    fn fail(&self) -> String {
        paint(self, "1;31")
    }

    /// Paints the addition.
    fn add(&self) -> String {
        paint(self, "32")
    }

    /// Paints the change.
    fn change(&self) -> String {
        paint(self, "33")
    }

    /// Paints the removal.
    fn delete(&self) -> String {
        paint(self, "31")
    }

    /// Paints the title.
    fn title(&self) -> String {
        paint(self, "1;4;34")
    }

    /// Paints the binary name.
    fn binary(&self) -> String {
        paint(self, "1;32")
    }
}

/// Paints the text.
fn paint(text: &str, code: &str) -> String {
    if colors_enabled() {
        format!("\x1b[{code}m{text}{RESET}")
    } else {
        text.to_owned()
    }
}

/// Returns whether stderr supports color.
fn colors_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        anstream::AutoStream::choice(&std::io::stderr()) != anstream::ColorChoice::Never
    })
}
