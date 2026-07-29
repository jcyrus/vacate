//! A ~40 line subsequence matcher.
//!
//! The candidate list is "ports open on this machine", so it is tens of items,
//! not tens of thousands. A real fuzzy-matching crate would be more dependency
//! than the problem deserves.

/// Score `haystack` against `query`, or `None` if the query isn't a
/// subsequence of it. Higher is better.
pub fn score(haystack: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let mut score = 0;
    let mut chars = haystack.char_indices().peekable();
    let mut previous: Option<char> = None;
    let mut last_match_index: Option<usize> = None;

    for needle in query.chars().flat_map(char::to_lowercase) {
        loop {
            let (index, candidate) = chars.next()?;
            let preceding = previous.replace(candidate);

            if candidate.to_lowercase().next() != Some(needle) {
                // Skipping characters is allowed, but not free.
                score -= 1;
                continue;
            }

            score += if last_match_index.is_some_and(|last| last + 1 == index) {
                // Adjacent to the previous match: the user is typing a real
                // run of characters, not scattered initials.
                10
            } else if index == 0 {
                8
            } else if preceding.is_some_and(|p| !p.is_alphanumeric()) {
                // Start of a new word.
                6
            } else if preceding.is_some_and(|p| p.is_alphabetic() != candidate.is_alphabetic()) {
                // A digit right after a letter (or vice versa) reads as a
                // boundary too, e.g. the "8080" in "node8080".
                4
            } else {
                1
            };
            last_match_index = Some(index);
            break;
        }
    }

    // Prefer tighter matches: the fewer leftover characters, the better.
    let leftover = chars.count() as i32;
    Some(score - leftover / 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("anything", ""), Some(0));
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert!(score("node", "xyz").is_none());
        assert!(score("node", "nodejs").is_none());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(score("Docker", "docker").is_some());
        assert!(score("docker", "DOCK").is_some());
    }

    #[test]
    fn subsequences_match_with_gaps() {
        assert!(score("3000 node cyrus TCP", "ndc").is_some());
    }

    #[test]
    fn prefix_beats_scattered_match() {
        let prefix = score("node server", "node").unwrap();
        let scattered = score("no docker element", "node").unwrap();
        assert!(prefix > scattered, "{prefix} should beat {scattered}");
    }

    #[test]
    fn exact_port_beats_incidental_digits() {
        let exact = score("3000 node cyrus TCP", "3000").unwrap();
        let incidental = score("53 systemd-resolve root UDP", "3000");
        assert!(incidental.is_none() || exact > incidental.unwrap());
    }

    #[test]
    fn word_start_beats_mid_word() {
        let word_start = score("8080 python root TCP", "p").unwrap();
        let mid_word = score("8080 openvpn root TCP", "p").unwrap();
        assert!(word_start > mid_word, "{word_start} should beat {mid_word}");
    }
}
