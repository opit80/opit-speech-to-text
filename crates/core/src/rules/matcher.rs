//! Single-pass phrase matcher used by corrections, literal replacements and
//! term casing.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::text::{capitalize_like, fold_char, is_word_char, sentence_start};

/// One phrase to look for and the text that replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub from: String,
    pub to: String,
    /// Caller-defined id reported back in [`MatchHit::rule`].
    pub rule: usize,
    /// Compare exact chars instead of folded chars; also disables capitalization.
    pub case_sensitive: bool,
    /// Use Turkish rules (`i` → `İ`) when capitalizing the target.
    pub turkish: bool,
}

/// A match in the input, as byte offsets, and the index of the pattern that matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub pattern: usize,
}

/// A replacement that changed the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchHit {
    pub rule: usize,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default)]
pub struct PhraseMatcher {
    patterns: Vec<Pattern>,
    needles: Vec<Vec<char>>,
    /// Folded first char → pattern indices, longest needle first.
    by_first: HashMap<char, Vec<usize>>,
}

impl PhraseMatcher {
    /// Builds a matcher. Empty or whitespace-only `from` values are ignored, and
    /// when two patterns compare equal the earlier one wins.
    pub fn new(patterns: impl IntoIterator<Item = Pattern>) -> Self {
        let mut matcher = PhraseMatcher::default();
        let mut seen = HashSet::new();
        for pattern in patterns {
            let from = pattern.from.trim().to_string();
            if from.is_empty() {
                continue;
            }
            let needle: Vec<char> =
                if pattern.case_sensitive { from.chars().collect() } else { from.chars().map(fold_char).collect() };
            if !seen.insert((pattern.case_sensitive, needle.clone())) {
                continue;
            }
            let index = matcher.patterns.len();
            matcher.by_first.entry(fold_char(needle[0])).or_default().push(index);
            matcher.patterns.push(Pattern { from, ..pattern });
            matcher.needles.push(needle);
        }
        let needles = &matcher.needles;
        for list in matcher.by_first.values_mut() {
            list.sort_by_key(|&i| Reverse(needles[i].len()));
        }
        matcher
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    pub fn find_all(&self, text: &str) -> Vec<Found> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let folded: Vec<char> = chars.iter().map(|&(_, c)| fold_char(c)).collect();
        let mut found = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match self.match_at(&chars, &folded, i) {
                Some((pattern, len)) => {
                    let start = chars[i].0;
                    let end = chars.get(i + len).map_or(text.len(), |&(byte, _)| byte);
                    found.push(Found { start, end, pattern });
                    i += len;
                }
                None => i += 1,
            }
        }
        found
    }

    pub fn replace_all(&self, text: &str) -> (String, Vec<MatchHit>) {
        let mut out = String::with_capacity(text.len());
        let mut hits = Vec::new();
        let mut last = 0;
        for found in self.find_all(text) {
            let pattern = &self.patterns[found.pattern];
            let source = &text[found.start..found.end];
            let to = if pattern.case_sensitive {
                pattern.to.clone()
            } else {
                capitalize_like(&pattern.to, source, sentence_start(&text[..found.start]), pattern.turkish)
            };
            out.push_str(&text[last..found.start]);
            out.push_str(&to);
            if to != source {
                hits.push(MatchHit { rule: pattern.rule, from: source.to_string(), to });
            }
            last = found.end;
        }
        out.push_str(&text[last..]);
        (out, hits)
    }

    fn match_at(&self, chars: &[(usize, char)], folded: &[char], i: usize) -> Option<(usize, usize)> {
        let candidates = self.by_first.get(&folded[i])?;
        for &index in candidates {
            let needle = &self.needles[index];
            let len = needle.len();
            if i + len > chars.len() {
                continue;
            }
            let equal = if self.patterns[index].case_sensitive {
                chars[i..i + len].iter().map(|&(_, c)| c).eq(needle.iter().copied())
            } else {
                folded[i..i + len] == needle[..]
            };
            if !equal {
                continue;
            }
            let left_ok = !is_word_char(needle[0]) || i == 0 || !is_word_char(chars[i - 1].1);
            let right_ok = !is_word_char(needle[len - 1]) || i + len == chars.len() || !is_word_char(chars[i + len].1);
            if left_ok && right_ok {
                return Some((index, len));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(from: &str, to: &str) -> Pattern {
        Pattern { from: from.into(), to: to.into(), rule: 0, case_sensitive: false, turkish: true }
    }

    fn replace(patterns: Vec<Pattern>, text: &str) -> String {
        PhraseMatcher::new(patterns).replace_all(text).0
    }

    #[test]
    fn replaces_whole_words_only() {
        assert_eq!(replace(vec![p("api", "API")], "api kapital apiler"), "API kapital apiler");
    }

    #[test]
    fn longest_pattern_wins() {
        let patterns = vec![p("cap", "X"), p("cap x", "Y")];
        assert_eq!(replace(patterns, "a cap x b cap"), "a Y b X");
    }

    #[test]
    fn matches_across_turkish_case_forms() {
        assert_eq!(replace(vec![p("kayıt et", "kaydet")], "o KAYIT ET dedi"), "o Kaydet dedi");
        assert_eq!(replace(vec![p("kayıt et", "kaydet")], "o kayit et dedi"), "o kaydet dedi");
    }

    #[test]
    fn keeps_capitalization_at_sentence_start_and_uppercase_source() {
        let pats = || vec![p("hala", "hâlâ")];
        assert_eq!(replace(pats(), "Hala geldi"), "Hâlâ geldi");
        assert_eq!(replace(pats(), "o hala burada"), "o hâlâ burada");
        assert_eq!(replace(pats(), "Geldi. hala burada"), "Geldi. Hâlâ burada");
    }

    #[test]
    fn capitalizes_with_turkish_dotted_i() {
        assert_eq!(replace(vec![p("2 tabağa", "iki tabağa")], "2 tabağa koy"), "İki tabağa koy");
    }

    #[test]
    fn suffix_after_straight_and_curly_apostrophe() {
        let pats = || vec![p("cloud code", "Claude Code")];
        assert_eq!(replace(pats(), "cloud code'u aç"), "Claude Code'u aç");
        assert_eq!(replace(pats(), "cloud code’u aç"), "Claude Code’u aç");
        assert_eq!(replace(pats(), "cloud coder"), "cloud coder");
    }

    #[test]
    fn non_word_edges_do_not_need_boundaries() {
        assert_eq!(replace(vec![p("cloud.md", "CLAUDE.md")], "cloud.md dosyası"), "CLAUDE.md dosyası");
        assert_eq!(replace(vec![p("c++", "C++")], "c++'ı sev"), "C++'ı sev");
    }

    #[test]
    fn empty_patterns_are_ignored() {
        let m = PhraseMatcher::new(vec![p("  ", "X"), p("", "Y")]);
        assert!(m.is_empty());
        assert_eq!(m.replace_all("abc").0, "abc");
    }

    #[test]
    fn first_duplicate_wins() {
        let mut second = p("HALA", "B");
        second.rule = 1;
        let (out, hits) = PhraseMatcher::new(vec![p("hala", "A"), second]).replace_all("hala");
        assert_eq!(out, "A");
        assert_eq!(hits[0].rule, 0);
    }

    #[test]
    fn case_sensitive_patterns_match_exact_case_only() {
        let mut pat = p("Pekala", "Pekâlâ");
        pat.case_sensitive = true;
        let m = PhraseMatcher::new(vec![pat]);
        assert_eq!(m.replace_all("pekala Pekala").0, "pekala Pekâlâ");
    }

    #[test]
    fn unchanged_text_reports_no_hit() {
        let m = PhraseMatcher::new(vec![p("GitHub", "GitHub")]);
        assert!(m.replace_all("GitHub").1.is_empty());
        let (out, hits) = m.replace_all("github");
        assert_eq!(out, "GitHub");
        assert_eq!(hits, vec![MatchHit { rule: 0, from: "github".into(), to: "GitHub".into() }]);
    }

    #[test]
    fn multibyte_text_before_match() {
        assert_eq!(replace(vec![p("hala", "hâlâ")], "çok güzel 👍 hala"), "çok güzel 👍 hâlâ");
    }

    #[test]
    fn find_all_reports_byte_offsets() {
        let m = PhraseMatcher::new(vec![p("şey", "X")]);
        let found = m.find_all("bir şey");
        // "bir " is 4 bytes; "şey" is 4 bytes (ş takes 2).
        assert_eq!(found, vec![Found { start: 4, end: 8, pattern: 0 }]);
    }
}
