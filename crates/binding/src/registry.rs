//! Registry
//! Collector holding every bound face once in order.

use std::collections::HashSet;

/// Face collector with one entry per Lua name.
///
/// Registration order and
/// stub emission in that order.
#[derive(Debug, Default)]
pub struct Registry {
    /// The seen Lua names.
    seen: HashSet<String>,
    /// The stub blocks in registration order.
    stubs: Vec<String>,
    /// The Lua names in registration order.
    order: Vec<String>,
}

impl Registry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one face by name with its stub text.
    ///
    /// - Repeated names with the first stub text.
    /// - Empty stub text with the reserved name.
    pub fn register(&mut self, name: String, stub: String) -> bool {
        if self.seen.contains(&name) {
            return false;
        }
        self.seen.insert(name.clone());
        self.order.push(name);
        if !stub.is_empty() {
            self.stubs.push(stub);
        }
        true
    }

    /// Render collected stubs in registration order.
    pub fn stub_text(&self) -> String {
        let mut out = self.stubs.join("\n\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    /// Return the entry count.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Report whether the registry waits for entries.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Report whether one name is in the registry.
    pub fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    /// Return registration order for ordering asserts.
    pub fn names(&self) -> Vec<String> {
        self.order.clone()
    }
}
