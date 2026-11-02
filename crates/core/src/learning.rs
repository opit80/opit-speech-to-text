//! Explicitly confirmed dictionary learning from history edits.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::rules::RuleSet;
use crate::text::{fold, sentence_start};

pub const MAX_SUGGESTIONS: usize = 8;
pub const MAX_DISMISSALS: usize = 4096;
pub const MAX_TERM_CHARS: usize = 256;
const MAX_DIFF_CELLS: usize = 4_000_000;
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}_][\p{L}\p{N}\p{M}_]*").unwrap());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    pub enabled: bool,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermChange {
    pub heard: String,
    pub corrected: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningTarget {
    Rule,
    Term,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningItem {
    #[serde(flatten)]
    pub change: TermChange,
    pub target: LearningTarget,
}

#[derive(Debug, thiserror::Error)]
pub enum LearningError {
    #[error("could not read or write learning preferences")]
    Io,
    #[error("learning preferences are invalid")]
    InvalidFile,
    #[error("learning preferences are full; existing choices have been retained")]
    Full,
    #[error("learning requires distinct nonempty spans of one to four words, at most 256 characters each")]
    InvalidPair,
}

impl TermChange {
    pub fn normalized(&self) -> Result<Self, LearningError> {
        let heard = collapse_ws(&self.heard);
        let corrected = collapse_ws(&self.corrected);
        if heard == corrected
            || [&heard, &corrected].iter().any(|s| {
                s.chars().count() > MAX_TERM_CHARS
                    || !(1..=4).contains(&WORD.find_iter(s).count())
                    || s.chars().any(char::is_control)
            })
        {
            return Err(LearningError::InvalidPair);
        }
        Ok(Self { heard, corrected })
    }

    fn key(&self) -> (String, String) {
        (fold(&collapse_ws(&self.heard)), fold(&collapse_ws(&self.corrected)))
    }
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Only small pairs are retained, never complete transcripts. At capacity reject new
/// choices rather than evicting old ones and offering a dismissed pair again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dismissals {
    pairs: Vec<TermChange>,
}

impl Dismissals {
    pub fn load(path: &Path) -> Result<Self, LearningError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err(LearningError::Io),
        };
        let loaded: Self = serde_json::from_slice(&bytes).map_err(|_| LearningError::InvalidFile)?;
        if loaded.pairs.len() > MAX_DISMISSALS {
            return Err(LearningError::InvalidFile);
        }
        let mut result = Self::default();
        result.dismiss(&loaded.pairs).map_err(|_| LearningError::InvalidFile)?;
        Ok(result)
    }

    pub fn save(&self, path: &Path) -> Result<(), LearningError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| LearningError::Io)?;
        }
        let bytes = serde_json::to_vec(self).map_err(|_| LearningError::InvalidFile)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|_| LearningError::Io)
    }

    pub fn contains(&self, pair: &TermChange) -> bool {
        let key = pair.key();
        self.pairs.iter().any(|p| p.key() == key)
    }

    pub fn dismiss(&mut self, pairs: &[TermChange]) -> Result<(), LearningError> {
        let mut next = self.clone();
        let mut keys: HashSet<_> = next.pairs.iter().map(TermChange::key).collect();
        for pair in pairs {
            let pair = pair.normalized()?;
            if keys.insert(pair.key()) {
                if next.pairs.len() == MAX_DISMISSALS {
                    return Err(LearningError::Full);
                }
                next.pairs.push(pair);
            }
        }
        *self = next;
        Ok(())
    }
}

/// Word-level LCS alignment uses the same Turkish fold as rules. Punctuation is
/// not an alignment token. Limit the matrix to 8 MB; unusually large edits simply
/// produce no automatic suggestions and can still be saved normally.
pub fn diff_terms(before: &str, after: &str) -> Vec<TermChange> {
    if before == after {
        return Vec::new();
    }
    let a: Vec<_> = WORD.find_iter(before).collect();
    let b: Vec<_> = WORD.find_iter(after).collect();
    let unchanged = |a: &regex::Match<'_>, b: &regex::Match<'_>| {
        a.as_str() == b.as_str() || (fold(a.as_str()) == fold(b.as_str()) && initial_case_only(before, a, after, b))
    };
    // A small correction in a long transcript should not need a large matrix.
    let prefix = a.iter().zip(&b).take_while(|(a, b)| unchanged(a, b)).count();
    let (a, b) = (&a[prefix..], &b[prefix..]);
    let suffix = a.iter().rev().zip(b.iter().rev()).take_while(|(a, b)| unchanged(a, b)).count();
    let (a, b) = (&a[..a.len() - suffix], &b[..b.len() - suffix]);
    let width = b.len() + 1;
    let cells = (a.len() + 1).saturating_mul(width);
    if cells > MAX_DIFF_CELLS || a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let af: Vec<_> = a.iter().map(|w| fold(w.as_str())).collect();
    let bf: Vec<_> = b.iter().map(|w| fold(w.as_str())).collect();
    let mut lcs = vec![0u16; cells];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if af[i] == bf[j] {
                1 + lcs[(i + 1) * width + j + 1]
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut result = Vec::new();
    let (mut i, mut j, mut start_i, mut start_j) = (0, 0, 0, 0);
    let mut add = |ai: usize, ae: usize, bi: usize, be: usize| {
        if !(1..=4).contains(&(ae - ai)) || !(1..=4).contains(&(be - bi)) {
            return;
        }
        let pair = TermChange {
            heard: collapse_ws(&before[a[ai].start()..a[ae - 1].end()]),
            corrected: collapse_ws(&after[b[bi].start()..b[be - 1].end()]),
        };
        if let Ok(pair) = pair.normalized()
            && result.len() < MAX_SUGGESTIONS
            && !result.iter().any(|p: &TermChange| p.key() == pair.key())
        {
            result.push(pair);
        }
    };
    while i < a.len() && j < b.len() {
        if af[i] == bf[j] {
            add(start_i, i, start_j, j);
            if a[i].as_str() != b[j].as_str() && !initial_case_only(before, &a[i], after, &b[j]) {
                add(i, i + 1, j, j + 1);
            }
            i += 1;
            j += 1;
            start_i = i;
            start_j = j;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    add(start_i, a.len(), start_j, b.len());
    result
}

fn initial_case_only(before: &str, a: &regex::Match<'_>, after: &str, b: &regex::Match<'_>) -> bool {
    let at_start = |text: &str, start| sentence_start(text[..start].trim_end_matches(['"', '\'', '“', '‘', '(', '[']));
    let mut ac = a.as_str().chars();
    let mut bc = b.as_str().chars();
    let (Some(first_a), Some(first_b)) = (ac.next(), bc.next()) else { return false };
    at_start(before, a.start())
        && at_start(after, b.start())
        && ac.as_str() == bc.as_str()
        && first_a.is_uppercase() != first_b.is_uppercase()
}

pub fn filter_suggestions(
    items: Vec<TermChange>,
    enabled: bool,
    rules: &RuleSet,
    dismissed: &Dismissals,
) -> Vec<TermChange> {
    if !enabled {
        return Vec::new();
    }
    items
        .into_iter()
        .filter(|item| {
            if dismissed.contains(item) {
                return false;
            }
            let applied = rules.apply(&item.heard);
            // Casing suggestions require the exact canonical spelling to be covered.
            if fold(&item.heard) == fold(&item.corrected) {
                applied != item.corrected
            } else {
                fold(&applied) != fold(&item.corrected)
            }
        })
        .take(MAX_SUGGESTIONS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(heard: &str, corrected: &str) -> TermChange {
        TermChange { heard: heard.into(), corrected: corrected.into() }
    }

    #[test]
    fn old_configs_default_to_enabled_and_disabled_round_trips() {
        let old: crate::config::AppConfig = serde_json::from_str("{}").unwrap();
        assert!(old.learning.enabled);
        let empty: crate::config::AppConfig = serde_json::from_str(r#"{"learning":{}}"#).unwrap();
        assert!(empty.learning.enabled);
        let mut config = old;
        config.learning.enabled = false;
        let loaded: crate::config::AppConfig = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert!(!loaded.learning.enabled);
    }

    #[test]
    fn replacements_allow_one_to_four_words_on_each_side() {
        assert_eq!(diff_terms("use git hap today", "use GitHub today"), vec![pair("git hap", "GitHub")]);
        assert_eq!(
            diff_terms("bir iki üç dört", "beş altı yedi sekiz"),
            vec![pair("bir iki üç dört", "beş altı yedi sekiz")]
        );
        assert!(diff_terms("bir iki üç dört beş", "altı yedi sekiz dokuz on").is_empty());
        assert!(diff_terms("word", "one two three four five").is_empty());
    }

    #[test]
    fn ignores_punctuation_whitespace_insertions_and_deletions() {
        for (before, after) in [
            ("merhaba dünya", "merhaba,  dünya!"),
            ("bir iki", "bir yeni iki"),
            ("bir eski iki", "bir iki"),
            ("", "ekle"),
            ("sil", ""),
            ("node.js", "node js"),
        ] {
            assert!(diff_terms(before, after).is_empty(), "{before:?} / {after:?}");
        }
    }

    #[test]
    fn turkish_initial_capitalization_is_ignored_but_inner_case_is_kept() {
        for (before, after) in [
            ("istanbul güzel", "İstanbul güzel"),
            ("ılık su", "Ilık su"),
            ("geldi. ışık açık", "geldi. Işık açık"),
            ("\"istanbul\"", "\"İstanbul\""),
        ] {
            assert!(diff_terms(before, after).is_empty());
        }
        assert_eq!(diff_terms("bugün github aç", "bugün GitHub aç"), vec![pair("github", "GitHub")]);
        assert_eq!(diff_terms("github aç", "GitHub aç"), vec![pair("github", "GitHub")]);
        assert_eq!(diff_terms("burası istanbul", "burası İstanbul"), vec![pair("istanbul", "İstanbul")]);
        assert_eq!(diff_terms("şimdi yanlız değil", "şimdi yalnız değil"), vec![pair("yanlız", "yalnız")]);
    }

    #[test]
    fn unicode_combining_marks_and_repeated_anchors_are_preserved() {
        assert_eq!(diff_terms("use cafe today", "use cafe\u{301} today"), vec![pair("cafe", "cafe\u{301}")]);
        assert_eq!(diff_terms("bir hata bir hata", "bir doğru bir doğru"), vec![pair("hata", "doğru")]);
        assert_eq!(diff_terms("今天 错误", "今天 正确"), vec![pair("错误", "正确")]);
    }

    #[test]
    fn caps_unique_suggestions() {
        let before = (0..20).map(|n| format!("bad{n} anchor{n}")).collect::<Vec<_>>().join(" ");
        let after = (0..20).map(|n| format!("good{n} anchor{n}")).collect::<Vec<_>>().join(" ");
        assert_eq!(diff_terms(&before, &after).len(), MAX_SUGGESTIONS);
    }

    #[test]
    fn small_edits_in_long_transcripts_are_compared_without_large_matrices() {
        let prefix = "unchanged words ".repeat(5000);
        let suffix = " more unchanged words".repeat(5000);
        assert_eq!(
            diff_terms(&format!("{prefix}wrong{suffix}"), &format!("{prefix}Correct{suffix}")),
            vec![pair("wrong", "Correct")]
        );
        let huge_a = "wrong ".repeat(2100);
        let huge_b = "correct ".repeat(2100);
        assert!(diff_terms(&huge_a, &huge_b).is_empty());
    }

    #[test]
    fn filters_rules_terms_dismissals_and_disabled_setting() {
        let pack = crate::rules::RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [GitHub]\ncorrections:\n  yalnız: [yanlız]\nreplacements:\n  - {from: burda, to: burada}\n").unwrap();
        let rules = crate::rules::RuleSet::compile(&[pack]).0;
        let mut dismissed = Dismissals::default();
        dismissed.dismiss(&[pair("İSİM", "Name")]).unwrap();
        let items = vec![
            pair("github", "GitHub"),
            pair("yanlız", "yalnız"),
            pair("burda", "burada"),
            pair("ısım", "name"),
            pair("git hap", "GitHub"),
        ];
        assert_eq!(filter_suggestions(items.clone(), true, &rules, &dismissed), vec![pair("git hap", "GitHub")]);
        assert!(filter_suggestions(items, false, &rules, &dismissed).is_empty());
    }

    #[test]
    fn dismissal_persistence_is_atomic_deduplicated_and_capped_without_eviction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("learning.json");
        let mut dismissed = Dismissals::load(&path).unwrap();
        dismissed.dismiss(&[pair("wrong", "Right"), pair("WRONG", "right")]).unwrap();
        dismissed.save(&path).unwrap();
        let mut loaded = Dismissals::load(&path).unwrap();
        assert!(loaded.contains(&pair("wrong", "Right")));
        assert_eq!(loaded.pairs.len(), 1);
        loaded.pairs = (0..MAX_DISMISSALS).map(|n| pair(&format!("wrong{n}"), "right")).collect();
        assert!(matches!(loaded.dismiss(&[pair("extra", "right")]), Err(LearningError::Full)));
        assert_eq!(loaded.pairs.len(), MAX_DISMISSALS);
        std::fs::write(&path, "private invalid content").unwrap();
        let error = Dismissals::load(&path).unwrap_err().to_string();
        assert!(!error.contains("private"));
    }
}
