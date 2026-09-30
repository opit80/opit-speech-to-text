//! Word error rate and term accuracy.

use std::ops::Add;

use opit_core::rules::matcher::{Pattern, PhraseMatcher};
use opit_core::text::{fold, is_word_char};

/// Folded words with punctuation (including apostrophes) treated as separators.
pub fn words(text: &str) -> Vec<String> {
    let cleaned: String = fold(text).chars().map(|c| if is_word_char(c) { c } else { ' ' }).collect();
    cleaned.split_whitespace().map(str::to_string).collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WerStats {
    pub errors: usize,
    pub reference_words: usize,
}

impl WerStats {
    pub fn rate(&self) -> f64 {
        match (self.reference_words, self.errors) {
            (0, 0) => 0.0,
            (0, _) => 1.0,
            (words, errors) => errors as f64 / words as f64,
        }
    }
}

impl Add for WerStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self { errors: self.errors + other.errors, reference_words: self.reference_words + other.reference_words }
    }
}

pub fn wer(reference: &str, hypothesis: &str) -> WerStats {
    let reference = words(reference);
    let hypothesis = words(hypothesis);
    WerStats { errors: edit_distance(&reference, &hypothesis), reference_words: reference.len() }
}

fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(x != y);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TermStats {
    pub expected: usize,
    pub hit: usize,
}

impl TermStats {
    pub fn rate(&self) -> f64 {
        if self.expected == 0 { 1.0 } else { self.hit as f64 / self.expected as f64 }
    }
}

impl Add for TermStats {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self { expected: self.expected + other.expected, hit: self.hit + other.hit }
    }
}

pub fn term_hits(reference: &str, hypothesis: &str, terms: &[String]) -> TermStats {
    let mut stats = TermStats::default();
    for term in terms {
        let expected = count(term, reference, false);
        if expected == 0 {
            continue;
        }
        stats.expected += expected;
        stats.hit += count(term, hypothesis, true).min(expected);
    }
    stats
}

fn count(term: &str, text: &str, case_sensitive: bool) -> usize {
    let pattern = Pattern { from: term.to_string(), to: term.to_string(), rule: 0, case_sensitive, turkish: true };
    PhraseMatcher::new([pattern]).find_all(text).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn words_fold_case_and_drop_punctuation() {
        assert_eq!(words("Merhaba, Dünya! Claude Code'u"), ["merhaba", "dünya", "claude", "code", "u"]);
    }

    #[test]
    fn wer_counts_substitutions_deletions_insertions() {
        assert_eq!(wer("a b c", "a b c"), WerStats { errors: 0, reference_words: 3 });
        assert_eq!(wer("a b c", "a x c").errors, 1);
        assert_eq!(wer("a b c", "a c").errors, 1);
        assert_eq!(wer("a b c", "a b c d").errors, 1);
        assert_eq!(wer("Merhaba, dünya!", "merhaba dünya").errors, 0);
    }

    #[test]
    fn wer_rate_edge_cases() {
        assert_eq!(wer("", "").rate(), 0.0);
        assert_eq!(wer("", "fazla").rate(), 1.0);
        assert!((wer("a b c d", "a x c d").rate() - 0.25).abs() < 1e-9);
        let total = wer("a b", "a") + wer("c d", "c d");
        assert_eq!(total, WerStats { errors: 1, reference_words: 4 });
    }

    #[test]
    fn term_hits_require_exact_spelling() {
        let list = terms(&["Claude Code", "GitHub", "FiveM"]);
        let stats = term_hits("Claude Code'u GitHub'a at", "cloud code'u GitHub'a at", &list);
        assert_eq!(stats, TermStats { expected: 2, hit: 1 });
        let lower = term_hits("GitHub'a at", "github'a at", &list);
        assert_eq!(lower, TermStats { expected: 1, hit: 0 });
        assert_eq!(term_hits("yok", "yok", &list).rate(), 1.0);
    }

    #[test]
    fn term_hits_cap_at_expected_count() {
        let stats = term_hits("GitHub", "GitHub GitHub", &terms(&["GitHub"]));
        assert_eq!(stats, TermStats { expected: 1, hit: 1 });
    }
}
