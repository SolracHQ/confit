//! Near-match suggestions for unknown names.
//!
//! The distance follows Damerau-Levenshtein, so a swapped pair counts as one
//! edit.

/// The farthest edit distance that keeps a candidate.
pub const MAX_SUGGESTION_DISTANCE: usize = 3;
/// The limit on candidates for one error.
pub const MAX_SUGGESTION_COUNT: usize = 5;

/// Finds candidates near one wanted name.
///
/// Candidates come nearest first, with ties broken by name.
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
    fn nearby_keeps_transposed_names() {
        let found = nearby("documnet", &["document".to_owned(), "profile".to_owned()]);
        assert_eq!(found, vec!["document".to_owned()]);
    }

    #[test]
    fn nearby_drops_far_names() {
        let found = nearby("xyz", &["document".to_owned()]);
        assert!(found.is_empty());
    }

    #[test]
    fn nearby_orders_by_distance_then_name_and_caps() {
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
