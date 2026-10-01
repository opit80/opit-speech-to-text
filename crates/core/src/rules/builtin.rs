//! Rule packs shipped with the app, embedded at compile time.

use serde::Serialize;

use super::pack::RulePack;

/// Built-in packs as `(id, YAML)`, in their fixed priority order.
pub const BUILTIN_PACKS: &[(&str, &str)] = &[
    ("tr-core", include_str!("../../../../rules/tr-core.yaml")),
    ("tr-tech", include_str!("../../../../rules/tr-tech.yaml")),
    ("fivem", include_str!("../../../../rules/fivem.yaml")),
];

pub fn builtin_pack(id: &str) -> Option<RulePack> {
    BUILTIN_PACKS
        .iter()
        .find(|(pack_id, _)| *pack_id == id)
        .map(|(_, src)| RulePack::from_yaml(src).expect("built-in packs are validated by tests"))
}

/// What the Rules page lists for a built-in pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub language: String,
    pub terms: usize,
    pub corrections: usize,
    pub replacements: usize,
    pub hallucinations: usize,
}

pub fn pack_infos() -> Vec<PackInfo> {
    BUILTIN_PACKS
        .iter()
        .filter_map(|(id, _)| builtin_pack(id))
        .map(|p| PackInfo {
            terms: p.terms.len(),
            corrections: p.corrections.len(),
            replacements: p.replacements.len(),
            hallucinations: p.hallucinations.len(),
            id: p.id,
            name: p.name,
            language: p.language,
        })
        .collect()
}

/// Packs in priority order: the personal pack first, then the enabled built-in
/// packs in [`BUILTIN_PACKS`] order. Unknown ids are ignored.
pub fn assemble(user: Option<RulePack>, enabled: &[String]) -> Vec<RulePack> {
    let mut packs: Vec<RulePack> = user.into_iter().collect();
    for (id, _) in BUILTIN_PACKS {
        if enabled.iter().any(|e| e == id) {
            packs.extend(builtin_pack(id));
        }
    }
    packs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSet;
    use crate::rules::prompt::build_prompt;

    fn all_enabled() -> Vec<String> {
        BUILTIN_PACKS.iter().map(|(id, _)| id.to_string()).collect()
    }

    fn rules() -> RuleSet {
        let (rules, warnings) = RuleSet::compile(&assemble(None, &all_enabled()));
        assert!(warnings.is_empty(), "{warnings:?}");
        rules
    }

    #[test]
    fn every_builtin_pack_parses_and_its_id_matches() {
        for (id, src) in BUILTIN_PACKS {
            let pack = RulePack::from_yaml(src).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(pack.id, *id);
        }
    }

    #[test]
    fn assemble_puts_user_first_and_keeps_builtin_order() {
        let user = RulePack::from_yaml("schema: 1\nid: user\nname: Me\n").unwrap();
        let enabled = vec!["fivem".to_string(), "tr-core".to_string(), "nope".to_string()];
        let ids: Vec<String> = assemble(Some(user), &enabled).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["user", "tr-core", "fivem"]);
    }

    #[test]
    fn golden_sentences() {
        let rules = rules();
        let cases = [
            ("cloud code'u github'a pushla", "Claude Code'u GitHub'a pushla"),
            ("Hala burda mısın?", "Hâlâ burada mısın?"),
            ("sunucuda ox lib ve qb core ile fivem var", "sunucuda ox_lib ve QBCore ile FiveM var"),
            ("Bunu diployla, sonra pull request aç.", "Bunu deployla, sonra pull request aç."),
            ("şimdi polivinleri güncelle", "şimdi pluginleri güncelle"),
            ("type script ve node js", "TypeScript ve Node.js"),
            ("eve git", "eve git"),
            ("kodu git hub'a at", "kodu GitHub'a at"),
        ];
        for (input, expected) in cases {
            assert_eq!(rules.apply(input), expected, "input: {input}");
        }
    }

    #[test]
    fn core_hallucinations_are_filtered() {
        let rules = rules();
        assert!(rules.is_hallucination("Altyazı M.K."));
        assert!(rules.is_hallucination("İzlediğiniz için teşekkürler."));
        assert!(!rules.is_hallucination("Teşekkürler, görüşürüz."));
    }

    #[test]
    fn all_builtin_terms_fit_the_prompt_budget_with_a_context() {
        let context = "Türkçe yazılım geliştirme ve FiveM sunucusu üzerine konuşma.";
        let built = build_prompt(context, rules().terms(), "tr");
        assert!(built.dropped_terms.is_empty(), "dropped: {:?}", built.dropped_terms);
    }

    #[test]
    fn pack_infos_list_every_builtin_pack_with_counts() {
        let infos = pack_infos();
        let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["tr-core", "tr-tech", "fivem"]);
        for info in &infos {
            let pack = builtin_pack(&info.id).unwrap();
            assert_eq!(info.name, pack.name);
            assert_eq!(info.terms, pack.terms.len());
            assert_eq!(info.corrections, pack.corrections.len());
            assert_eq!(info.replacements, pack.replacements.len());
            assert_eq!(info.hallucinations, pack.hallucinations.len());
        }
    }
}
