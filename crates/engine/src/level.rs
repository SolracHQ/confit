//! Level
//!
//! Patch priority as data with fixed order.

/// Merge priority carried by one patch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Level {
    /// Lowest priority.
    Minor,
    /// Low priority.
    Low,
    /// Default priority.
    #[default]
    Normal,
    /// High priority.
    High,
    /// Highest priority.
    Major,
}

impl Level {
    /// Numeric rank feeding pipeline order.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Minor => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::High => 3,
            Self::Major => 4,
        }
    }

    /// Parses one of the five level names in any letter case.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "MINOR" => Some(Self::Minor),
            "LOW" => Some(Self::Low),
            "NORMAL" => Some(Self::Normal),
            "HIGH" => Some(Self::High),
            "MAJOR" => Some(Self::Major),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_order_minor_below_major() {
        assert!(Level::Minor.rank() < Level::Normal.rank());
        assert!(Level::Normal.rank() < Level::Major.rank());
        assert_eq!(Level::default(), Level::Normal);
    }

    #[test]
    fn names_parse_case_insensitively() {
        assert_eq!(Level::parse("minor"), Some(Level::Minor));
        assert_eq!(Level::parse("MAJOR"), Some(Level::Major));
        assert_eq!(Level::parse("extreme"), None);
    }
}
