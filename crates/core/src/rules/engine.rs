//! Compiles rule packs and applies corrections → replacements → term casing.

use std::collections::HashSet;

use regex::{Regex, RegexBuilder};
use serde::Serialize;

use super::hallucination::HallucinationFilter;
use super::matcher::{MatchHit, Pattern, PhraseMatcher};
use super::pack::RulePack;
use crate::text::{capitalize_like, fold, sentence_start};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Correction,
    Replacement,
    Casing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleRef {
    pub pack_id: String,
    pub kind: RuleKind,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleHit {
    pub rule: RuleRef,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleWarning {
    pub pack_id: String,
    pub message: String,
}

enum Step {
    Literal(PhraseMatcher),
    Regex { re: Regex, to: String, rule: usize, case_sensitive: bool, turkish: bool },
}

pub struct RuleSet {
    refs: Vec<RuleRef>,
    corrections: PhraseMatcher,
    replacements: Vec<Step>,
    casing: PhraseMatcher,
    hallucinations: HallucinationFilter,
    terms: Vec<String>,
}

impl RuleSet {
    pub fn empty() -> Self {
        Self::compile(&[]).0
    }

    /// Compiles `packs` in priority order (personal pack first). Broken rules are
    /// skipped and reported as warnings; compilation itself never fails.
    pub fn compile(packs: &[RulePack]) -> (Self, Vec<RuleWarning>) {
        let mut refs = Vec::new();
        let mut warnings = Vec::new();
        let mut corrections = Vec::new();
        let mut replacements = Vec::new();
        let mut casing = Vec::new();
        let mut hallucinations = Vec::new();
        let mut terms = Vec::new();
        let mut term_keys = HashSet::new();

        for pack in packs {
            let turkish = pack.is_turkish();
            let mut new_ref = |kind, index| {
                refs.push(RuleRef { pack_id: pack.id.clone(), kind, index });
                refs.len() - 1
            };

            for (index, (canonical, variants)) in pack.corrections.iter().enumerate() {
                if canonical.trim().is_empty() {
                    warnings.push(RuleWarning {
                        pack_id: pack.id.clone(),
                        message: format!("correction #{} has an empty target; skipped", index + 1),
                    });
                    continue;
                }
                let rule = new_ref(RuleKind::Correction, index);
                for variant in variants {
                    corrections.push(Pattern {
                        from: variant.clone(),
                        to: canonical.clone(),
                        rule,
                        case_sensitive: false,
                        turkish,
                    });
                }
            }

            for (index, item) in pack.replacements.iter().enumerate() {
                let number = index + 1;
                if item.from.trim().is_empty() {
                    warnings.push(RuleWarning {
                        pack_id: pack.id.clone(),
                        message: format!("replacement #{number} has an empty `from`; skipped"),
                    });
                    continue;
                }
                let rule = new_ref(RuleKind::Replacement, index);
                if item.regex {
                    match RegexBuilder::new(&item.from).case_insensitive(!item.case_sensitive).build() {
                        Ok(re) => replacements.push(Step::Regex {
                            re,
                            to: item.to.clone(),
                            rule,
                            case_sensitive: item.case_sensitive,
                            turkish,
                        }),
                        Err(err) => warnings.push(RuleWarning {
                            pack_id: pack.id.clone(),
                            message: format!("replacement #{number} has an invalid regex: {err}"),
                        }),
                    }
                } else {
                    replacements.push(Step::Literal(PhraseMatcher::new([Pattern {
                        from: item.from.clone(),
                        to: item.to.clone(),
                        rule,
                        case_sensitive: item.case_sensitive,
                        turkish,
                    }])));
                }
            }

            for (index, term) in pack.terms.iter().enumerate() {
                let term = term.trim();
                if term.is_empty() || !term_keys.insert(fold(term)) {
                    continue;
                }
                let rule = new_ref(RuleKind::Casing, index);
                terms.push(term.to_string());
                casing.push(Pattern { from: term.into(), to: term.into(), rule, case_sensitive: false, turkish });
            }

            hallucinations.extend(pack.hallucinations.iter().cloned());
        }

        let rules = RuleSet {
            refs,
            corrections: PhraseMatcher::new(corrections),
            replacements,
            casing: PhraseMatcher::new(casing).without_sentence_case(),
            hallucinations: HallucinationFilter::new(hallucinations),
            terms,
        };
        (rules, warnings)
    }

    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    pub fn is_hallucination(&self, text: &str) -> bool {
        self.hallucinations.matches(text)
    }

    pub fn apply(&self, text: &str) -> String {
        self.apply_traced(text).0
    }

    pub fn apply_traced(&self, text: &str) -> (String, Vec<RuleHit>) {
        let mut hits = Vec::new();
        let (mut text, found) = self.corrections.replace_all(text);
        self.collect(&mut hits, found);
        for step in &self.replacements {
            let (next, found) = match step {
                Step::Literal(matcher) => matcher.replace_all(&text),
                Step::Regex { re, to, rule, case_sensitive, turkish } => {
                    regex_replace(re, to, *rule, *case_sensitive, *turkish, &text)
                }
            };
            self.collect(&mut hits, found);
            text = next;
        }
        let (text, found) = self.casing.replace_all(&text);
        self.collect(&mut hits, found);
        (text, hits)
    }

    fn collect(&self, out: &mut Vec<RuleHit>, found: Vec<MatchHit>) {
        out.extend(found.into_iter().map(|hit| RuleHit {
            rule: self.refs[hit.rule].clone(),
            from: hit.from,
            to: hit.to,
        }));
    }
}

fn regex_replace(
    re: &Regex,
    to: &str,
    rule: usize,
    case_sensitive: bool,
    turkish: bool,
    text: &str,
) -> (String, Vec<MatchHit>) {
    let mut out = String::with_capacity(text.len());
    let mut hits = Vec::new();
    let mut last = 0;
    for caps in re.captures_iter(text) {
        let whole = caps.get(0).expect("capture group 0 always exists");
        if whole.as_str().is_empty() {
            continue;
        }
        let mut replacement = String::new();
        caps.expand(to, &mut replacement);
        if !case_sensitive {
            replacement =
                capitalize_like(&replacement, whole.as_str(), sentence_start(&text[..whole.start()]), turkish);
        }
        out.push_str(&text[last..whole.start()]);
        out.push_str(&replacement);
        if replacement != whole.as_str() {
            hits.push(MatchHit { rule, from: whole.as_str().to_string(), to: replacement });
        }
        last = whole.end();
    }
    out.push_str(&text[last..]);
    (out, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(id: &str, body: &str) -> RulePack {
        RulePack::from_yaml(&format!("schema: 1\nid: {id}\nname: Test\n{body}")).unwrap()
    }

    fn compile(packs: &[RulePack]) -> RuleSet {
        let (rules, warnings) = RuleSet::compile(packs);
        assert!(warnings.is_empty(), "{warnings:?}");
        rules
    }

    #[test]
    fn applies_corrections_then_replacements_then_casing() {
        let rules = compile(&[pack(
            "p",
            "terms: [GitHub]\ncorrections:\n  kod: [kot]\nreplacements:\n  - { from: kod yaz, to: kodla }\n",
        )]);
        assert_eq!(rules.apply("şimdi kot yaz ve github"), "şimdi kodla ve GitHub");
    }

    #[test]
    fn personal_pack_wins_on_the_same_variant() {
        let user = pack("user", "corrections:\n  X: [foo]\n");
        let builtin = pack("b", "corrections:\n  Y: [foo]\n");
        assert_eq!(compile(&[user, builtin]).apply("bir foo"), "bir X");
    }

    #[test]
    fn regex_replacement_supports_captures() {
        let rules = compile(&[pack("p", "replacements:\n  - { from: '(\\d+) tl', to: '$1 TL', regex: true }\n")]);
        assert_eq!(rules.apply("fiyat 50 tl oldu"), "fiyat 50 TL oldu");
    }

    #[test]
    fn regex_replacement_keeps_capitalization() {
        let rules = compile(&[pack("p", "replacements:\n  - { from: polivin, to: plugin, regex: true }\n")]);
        assert_eq!(rules.apply("Polivinler hazır"), "Pluginler hazır");
    }

    #[test]
    fn invalid_regex_is_skipped_with_a_warning() {
        let p = pack("p", "replacements:\n  - { from: '(', to: x, regex: true }\n  - { from: a, to: b }\n");
        let (rules, warnings) = RuleSet::compile(&[p]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].pack_id, "p");
        assert!(warnings[0].message.contains("#1"), "{}", warnings[0].message);
        assert_eq!(rules.apply("x a"), "x b");
    }

    #[test]
    fn empty_from_is_skipped_with_a_warning() {
        let (_, warnings) = RuleSet::compile(&[pack("p", "replacements:\n  - { from: '  ', to: x }\n")]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn regex_matching_empty_string_changes_nothing() {
        let rules = compile(&[pack("p", "replacements:\n  - { from: 'x*', to: Y, regex: true }\n")]);
        let (out, hits) = rules.apply_traced("abc");
        assert_eq!(out, "abc");
        assert!(hits.is_empty());
    }

    #[test]
    fn traced_hits_name_the_rule() {
        let rules = compile(&[pack("user", "corrections:\n  X: [foo]\n")]);
        let (_, hits) = rules.apply_traced("bir foo");
        assert_eq!(
            hits,
            vec![RuleHit {
                rule: RuleRef { pack_id: "user".into(), kind: RuleKind::Correction, index: 0 },
                from: "foo".into(),
                to: "X".into(),
            }]
        );
    }

    #[test]
    fn terms_are_deduped_in_priority_order() {
        let user = pack("user", "terms: [GitHub, Sigrid]\n");
        let builtin = pack("b", "terms: [github, FiveM, '  ']\n");
        assert_eq!(compile(&[user, builtin]).terms(), ["GitHub", "Sigrid", "FiveM"]);
    }

    #[test]
    fn hallucinations_come_from_all_packs() {
        let rules =
            compile(&[pack("a", "hallucinations: [altyazı m.k]\n"), pack("b", "hallucinations: [thank you]\n")]);
        assert!(rules.is_hallucination("Altyazı M.K."));
        assert!(rules.is_hallucination("Thank you."));
        assert!(!rules.is_hallucination("Merhaba"));
    }

    #[test]
    fn empty_rule_set_is_identity() {
        let rules = RuleSet::empty();
        assert_eq!(rules.apply("cloud code"), "cloud code");
        assert!(rules.terms().is_empty());
        assert!(!rules.is_hallucination("thank you"));
    }

    #[test]
    fn casing_never_invents_capitals() {
        let rules = compile(&[pack("user", "terms: [iOS, iPhone, npm, ox_lib, worktree, GitHub]\n")]);
        let cases = [
            ("iOS güncellemesi geldi.", "iOS güncellemesi geldi."),
            ("Telefon aldım. iPhone'u sevdim", "Telefon aldım. iPhone'u sevdim"),
            ("npm install çalıştır", "npm install çalıştır"),
            ("ox_lib yükle", "ox_lib yükle"),
            ("Worktree aç", "Worktree aç"),
            ("IOS ve WORKTREE", "iOS ve Worktree"),
            ("github", "GitHub"),
        ];
        for (input, expected) in cases {
            assert_eq!(rules.apply(input), expected, "input: {input}");
        }
    }

    #[test]
    fn empty_correction_target_is_skipped_with_a_warning() {
        let (rules, warnings) = RuleSet::compile(&[pack("p", "corrections:\n  '': [şey]\n")]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(rules.apply("bu şey güzel"), "bu şey güzel");
    }
}
