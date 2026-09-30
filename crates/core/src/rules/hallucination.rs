//! Drops outputs that are nothing but a known silence hallucination.

use std::collections::HashSet;

use crate::text::fold;

const EDGE_PUNCTUATION: &[char] = &['.', ',', '!', '?', ';', ':', '…', '"', '\'', '“', '”', '‘', '’', '-'];

/// Folds case, trims surrounding punctuation and whitespace, collapses inner whitespace.
pub fn normalize(text: &str) -> String {
    let folded = fold(text);
    let trimmed = folded.trim_matches(|c: char| c.is_whitespace() || EDGE_PUNCTUATION.contains(&c));
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Default, Clone)]
pub struct HallucinationFilter {
    patterns: HashSet<String>,
}

impl HallucinationFilter {
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        let patterns = patterns.into_iter().map(|p| normalize(&p)).filter(|p| !p.is_empty()).collect();
        Self { patterns }
    }

    /// True only when the entire output is a known hallucination.
    pub fn matches(&self, text: &str) -> bool {
        let normalized = normalize(text);
        !normalized.is_empty() && self.patterns.contains(&normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter() -> HallucinationFilter {
        HallucinationFilter::new(["altyazı m.k".to_string(), "thank you".to_string(), "  ".to_string()])
    }

    #[test]
    fn matches_whole_output_ignoring_case_and_edge_punctuation() {
        let f = filter();
        assert!(f.matches("Altyazı M.K."));
        assert!(f.matches("altyazi m.k"));
        assert!(f.matches("  Thank you!  "));
        assert!(f.matches("thank   you"));
    }

    #[test]
    fn never_matches_inside_a_sentence() {
        let f = filter();
        assert!(!f.matches("Bu altyazı m.k değil"));
        assert!(!f.matches("I said thank you to him"));
    }

    #[test]
    fn empty_input_and_empty_patterns_never_match() {
        let f = filter();
        assert!(!f.matches(""));
        assert!(!f.matches("..."));
    }

    #[test]
    fn normalize_examples() {
        assert_eq!(normalize("  “Altyazı  M.K.”  "), "altyazi m.k");
        assert_eq!(normalize("¿?"), "¿");
    }
}
