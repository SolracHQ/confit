//! Level
//!
//! Merge priority as data with fixed order.

use serde::{Deserialize, Serialize};

/// Defines the merge priority level.
///
/// The engine sorts by level and never interprets beyond order. Declaration
/// order sets the rank, so derived `Ord` sorts from `Minor` to `Major`.
/// Serializes lowercase (`"minor"`, `"normal"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Lowest priority.
    Minor,
    /// Low priority.
    Low,
    /// Default priority for entries, documents, and patches.
    Normal,
    /// High priority.
    High,
    /// Highest priority.
    Major,
}

impl Default for Level {
    /// Yields the default priority.
    ///
    /// # Returns
    ///
    /// `Normal`, the priority carried without explicit tuning.
    fn default() -> Self {
        Self::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_runs_minor_to_major() {
        assert!(Level::Minor < Level::Low);
        assert!(Level::Low < Level::Normal);
        assert!(Level::Normal < Level::High);
        assert!(Level::High < Level::Major);
        let mut levels = vec![
            Level::Major,
            Level::Minor,
            Level::Normal,
            Level::Low,
            Level::High,
        ];
        levels.sort();
        assert_eq!(
            levels,
            vec![
                Level::Minor,
                Level::Low,
                Level::Normal,
                Level::High,
                Level::Major
            ]
        );
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(Level::default(), Level::Normal);
    }

    #[test]
    fn serde_shape_is_lowercase_and_round_trips() {
        assert_eq!(
            serde_json::to_value(Level::Major).unwrap(),
            serde_json::json!("major")
        );
        assert_eq!(
            serde_json::to_value(Level::Normal).unwrap(),
            serde_json::json!("normal")
        );
        for level in [
            Level::Minor,
            Level::Low,
            Level::Normal,
            Level::High,
            Level::Major,
        ] {
            let round_tripped: Level =
                serde_json::from_value(serde_json::to_value(level).unwrap()).unwrap();
            assert_eq!(round_tripped, level);
        }
    }
}
