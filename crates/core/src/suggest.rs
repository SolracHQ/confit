//! Near-match suggestions for unknown names.
//!
//! Damerau-Levenshtein keeps transpositions cheap, covering the typo class.

/// Farthest edit distance keeping one candidate.
pub const MAX_SUGGESTION_DISTANCE: usize = 3;
/// Most candidates kept per error.
pub const MAX_SUGGESTION_COUNT: usize = 5;

/// Collect known names near one wanted name.
///
/// Order runs nearest first, breaking ties by name.
pub fn nearby(wanted: &str, candidates: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .map(|candidate| {
            (
                strsim::damerau_levenshtein(wanted, candidate),
                candidate.as_str(),
            )
        })
        .filter(|(distance, _)| *distance <= MAX_SUGGESTION_DISTANCE)
        .collect();
    scored.sort();
    scored
        .into_iter()
        .map(|(_, name)| name.to_owned())
        .take(MAX_SUGGESTION_COUNT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_transposed_names() {
        let found = nearby("documnet", &["document".to_owned(), "profile".to_owned()]);
        assert_eq!(found, vec!["document".to_owned()]);
    }

    #[test]
    fn drops_far_names() {
        let found = nearby("xyz", &["document".to_owned()]);
        assert!(found.is_empty());
    }

    #[test]
    fn orders_by_distance_then_name_and_caps() {
        let candidates = ["abd", "abe", "abf", "abg", "abh", "abi", "zzzq"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        let found = nearby("abc", &candidates);
        assert_eq!(
            found,
            ["abd", "abe", "abf", "abg", "abh"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        );
    }
}
