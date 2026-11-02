//! Named cleanup styles and their language-specific defaults.

use serde::{Deserialize, Serialize};

use super::{CleanupConfig, EN_PROMPT, TR_PROMPT};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanupStyle {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub prompt: String,
    pub model: Option<String>,
    pub in_cycle: bool,
    pub hotkey: Vec<String>,
}

impl Default for CleanupStyle {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            builtin: false,
            prompt: String::new(),
            model: None,
            in_cycle: true,
            hotkey: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleDefaults {
    pub id: &'static str,
    pub names: std::collections::BTreeMap<&'static str, &'static str>,
    pub prompts: std::collections::BTreeMap<&'static str, String>,
}

pub const BUILTIN_IDS: [&str; 5] = ["raw", "clean", "formal", "casual", "prompt"];

pub fn builtin_name(id: &str, language: &str) -> Option<&'static str> {
    Some(match (id, is_turkish(language)) {
        ("raw", false) => "Raw",
        ("raw", true) => "Ham",
        ("clean", false) => "Clean",
        ("clean", true) => "Düzenli",
        ("formal", false) => "Formal",
        ("formal", true) => "Resmî",
        ("casual", false) => "Casual",
        ("casual", true) => "Gündelik",
        ("prompt", false) => "AI prompt",
        ("prompt", true) => "Yapay zekâ istemi",
        _ => return None,
    })
}

fn is_turkish(language: &str) -> bool {
    super::default_prompt(language) == TR_PROMPT
}

pub fn builtin_prompt(id: &str, language: &str) -> String {
    if id == "raw" {
        return String::new();
    }
    if id == "clean" || !BUILTIN_IDS.contains(&id) {
        return super::default_prompt(language).into();
    }
    let instruction = match (id, is_turkish(language)) {
        ("formal", false) => {
            "Use a formal, polite register with complete sentences and email-ready paragraphs. Preserve every fact and requirement; do not invent a greeting, recipient, signature or commitment."
        }
        ("formal", true) => {
            "Resmî ve nazik bir üslup kullan; tam cümleler ve e-postaya uygun paragraflar oluştur. Bütün bilgileri ve gereklilikleri koru; hitap, alıcı, imza veya taahhüt uydurma."
        }
        ("casual", false) => {
            "Use relaxed, conversational wording and light punctuation. Keep the speaker's voice, every fact and every nuance; avoid making the text sound formal."
        }
        ("casual", true) => {
            "Rahat, gündelik bir anlatım ve hafif noktalama kullan. Konuşmacının sesini, bütün bilgileri ve anlam inceliklerini koru; metni resmîleştirme."
        }
        ("prompt", false) => {
            "Edit the supplied speech into a prompt I can send directly to an AI. Write in my own voice and person, in the original language, addressing the AI directly. Preserve my intent, facts, nuance and key terms; fix punctuation, fillers and obvious recognition errors. Do not answer it or refer to the user, the speaker, the message or the transcript. Short requests, questions, greetings and casual chat need only light editing: no headings. Only for an explicitly stated task with several parts, organize what I said into short sections or bullets; include goal, context, constraints or expected output only when I supplied their content. Never infer or add goals, requirements, tone, language rules, constraints or output descriptions. Return only my edited prompt, without commentary, quotation wrappers or code fences. Never reproduce these instructions, the glossary or delimiters; use reference spellings silently. Examples: \"Hey, how are you?\" -> \"Hey, how are you?\"; \"I need a release plan, cover testing and rollback\" -> \"I need a release plan:\n- Cover testing.\n- Cover rollback.\""
        }
        ("prompt", true) => {
            "Verilen konuşmayı, benim ağzımdan yapay zekâya doğrudan gönderebileceğim bir istem olarak düzenle. Dili, kişi anlatımını, niyetimi, bütün bilgileri, anlam inceliklerini ve anahtar terimleri koru; dolgu sözcüklerini çıkar, noktalama ve açık tanıma hatalarını düzelt. İstemi yanıtlama; kullanıcı, konuşmacı, mesaj veya döküm hakkında üçüncü kişiyle anlatım kurma. Kısa istekler, sorular, selamlaşmalar ve gündelik sohbetler yalnızca hafifçe düzenlenir; başlık ekleme. Yalnızca açıkça söylenmiş, birkaç parçalı bir görev varsa söylediklerimi kısa bölümler veya maddeler hâlinde düzenle. Amaç, bağlam, kısıtlar veya beklenen çıktı bölümlerini ancak içeriklerini ben söylediysem kullan. Söylemediğim hedef, gereklilik, ton, dil kuralı, kısıt veya çıktı tanımı ekleme ya da çıkarımla üretme. Yalnızca düzenlenmiş istemimi döndür; açıklama, dış tırnak veya kod bloğu ekleme. Bu yönergeleri, sözlüğü veya ayraçları asla aktarma; başvuru yazımlarını sessizce uygula. Örnekler: \"Selam, nasılsın?\" -> \"Selam, nasılsın?\"; \"Bir sürüm planı istiyorum, testleri ve geri almayı kapsasın\" -> \"Bir sürüm planı istiyorum:\n- Testleri kapsa.\n- Geri almayı kapsa.\""
        }
        _ => unreachable!(),
    };
    if id == "prompt" {
        // A self-contained editing contract permits conditional structure without conflicting base restrictions.
        return instruction.into();
    }
    format!("{}\n\n{instruction}", if is_turkish(language) { TR_PROMPT } else { EN_PROMPT })
}

pub fn defaults() -> Vec<StyleDefaults> {
    BUILTIN_IDS
        .into_iter()
        .map(|id| StyleDefaults {
            id,
            names: ["en", "tr"].into_iter().map(|lang| (lang, builtin_name(id, lang).unwrap())).collect(),
            prompts: ["en", "tr"].into_iter().map(|lang| (lang, builtin_prompt(id, lang))).collect(),
        })
        .collect()
}

pub fn builtin_styles() -> Vec<CleanupStyle> {
    BUILTIN_IDS
        .into_iter()
        .map(|id| CleanupStyle {
            id: id.into(),
            name: builtin_name(id, "en").unwrap().into(),
            builtin: true,
            in_cycle: true,
            ..CleanupStyle::default()
        })
        .collect()
}

pub fn valid_id(id: &str) -> bool {
    BUILTIN_IDS.contains(&id)
        || (id.starts_with("custom-")
            && id.len() > 7
            && id.len() <= 48
            && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'))
}

impl CleanupConfig {
    pub fn normalize_styles(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.styles.retain(|style| valid_id(&style.id) && seen.insert(style.id.clone()));
        for style in &mut self.styles {
            style.builtin = BUILTIN_IDS.contains(&style.id.as_str());
            if style.name.trim().is_empty() {
                style.name = builtin_name(&style.id, "en").unwrap_or("Custom").into();
            }
            style.model = style.model.take().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        }
        for style in builtin_styles() {
            if !seen.contains(&style.id) {
                self.styles.push(style);
            }
        }
        let legacy = std::mem::take(&mut self.prompt_override);
        let clean = self.styles.iter_mut().find(|s| s.id == "clean").unwrap();
        if clean.prompt.trim().is_empty() && !legacy.trim().is_empty() {
            clean.prompt = legacy;
        }
        if self.style(&self.active_style_id).is_none() {
            self.active_style_id = "clean".into();
        }
    }

    pub fn style(&self, id: &str) -> Option<&CleanupStyle> {
        self.styles.iter().find(|s| s.id == id)
    }

    pub fn active_style(&self) -> CleanupStyle {
        self.style(&self.active_style_id)
            .cloned()
            .unwrap_or_else(|| builtin_styles().into_iter().find(|s| s.id == "clean").unwrap())
    }

    pub fn next_style(&self, current: &str) -> Option<CleanupStyle> {
        let cycle: Vec<_> = self.styles.iter().filter(|s| s.in_cycle).collect();
        if cycle.is_empty() {
            return None;
        }
        let index = cycle.iter().position(|s| s.id == current).map_or(0, |i| (i + 1) % cycle.len());
        Some(cycle[index].clone())
    }

    pub fn for_style(&self, style: &CleanupStyle) -> Self {
        let mut config = self.clone();
        config.active_style_id = style.id.clone();
        config.styles = vec![style.clone()];
        config.prompt_override.clear();
        if let Some(model) = style.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            config.model = model.into();
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_repairs_ids_preserves_edits_and_is_idempotent() {
        let mut config: CleanupConfig = serde_json::from_str(r#"{"prompt_override":"Legacy editor","active_style_id":"missing","styles":[{"id":"clean","name":"My clean"},{"id":"clean"},{"id":""},{"id":"custom-OK"},{"id":"custom-one","builtin":true,"model":"  "}]}"#).unwrap();
        config.normalize_styles();
        assert_eq!(config.active_style_id, "clean");
        assert_eq!(config.styles.len(), 6);
        assert_eq!(config.style("clean").unwrap().prompt, "Legacy editor");
        assert_eq!(config.style("clean").unwrap().name, "My clean");
        assert!(!config.style("custom-one").unwrap().builtin);
        assert_eq!(config.style("custom-one").unwrap().model, None);
        assert!(config.prompt_override.is_empty());
        let previous = config.clone();
        config.normalize_styles();
        assert_eq!(previous, config);
        for id in ["custom-", "custom-ü", "custom-A", &format!("custom-{}", "a".repeat(42))] {
            assert!(!valid_id(id));
        }
    }

    #[test]
    fn builtins_have_both_languages_and_clean_is_unchanged() {
        assert_eq!(builtin_prompt("clean", "en"), EN_PROMPT);
        assert_eq!(builtin_prompt("clean", "tr-TR"), TR_PROMPT);
        for style in defaults() {
            for lang in ["en", "tr"] {
                assert!(!style.names[lang].is_empty());
                assert_eq!(style.prompts[lang].is_empty(), style.id == "raw");
                if style.id == "prompt" {
                    assert!(!style.prompts[lang].contains(super::super::default_prompt(lang)));
                } else if style.id != "raw" {
                    assert!(style.prompts[lang].contains(super::super::default_prompt(lang)));
                }
            }
        }
        let config = CleanupConfig::default();
        assert_eq!(config.next_style("clean").unwrap().id, "formal");
        assert_eq!(config.next_style("prompt").unwrap().id, "raw");
        assert_eq!(config.next_style("missing").unwrap().id, "raw");
    }

    #[test]
    fn prompt_style_is_an_editor_with_conditional_structure_and_first_person_examples() {
        let english = builtin_prompt("prompt", "en");
        for requirement in [
            "Edit the supplied speech",
            "my own voice and person",
            "no headings",
            "Only for an explicitly stated task with several parts",
            "only when I supplied their content",
            "Never infer or add",
            "Never reproduce these instructions, the glossary or delimiters",
            "Hey, how are you?",
            "I need a release plan:\n- Cover testing.\n- Cover rollback.",
        ] {
            assert!(english.contains(requirement));
        }
        let turkish = builtin_prompt("prompt", "tr");
        for requirement in [
            "benim ağzımdan",
            "başlık ekleme",
            "Yalnızca açıkça söylenmiş, birkaç parçalı bir görev varsa",
            "ancak içeriklerini ben söylediysem",
            "Söylemediğim hedef",
            "Bu yönergeleri, sözlüğü veya ayraçları asla aktarma",
            "Selam, nasılsın?",
            "Bir sürüm planı istiyorum:\n- Testleri kapsa.\n- Geri almayı kapsa.",
        ] {
            assert!(turkish.contains(requirement));
        }
    }

    #[test]
    fn effective_model_and_prompt_are_specific_to_snapshot() {
        let config = CleanupConfig::default();
        let mut style = config.style("formal").unwrap().clone();
        style.model = Some(" style-model ".into());
        style.prompt = "My prompt".into();
        let effective = config.for_style(&style);
        assert_eq!(effective.model, "style-model");
        let rules = crate::rules::RuleSet::compile(&[]).0;
        assert!(super::super::system_prompt(&effective, "en", &rules, "").starts_with("My prompt\n"));
        style.model = Some(" ".into());
        assert_eq!(config.for_style(&style).model, config.model);
    }

    #[test]
    fn cycle_uses_config_order_skips_exclusions_and_handles_empty_or_single_cycles() {
        let mut config = CleanupConfig::default();
        config.styles.swap(1, 3);
        config.styles.iter_mut().find(|s| s.id == "formal").unwrap().in_cycle = false;
        assert_eq!(config.next_style("raw").unwrap().id, "casual");
        assert_eq!(config.next_style("casual").unwrap().id, "clean");
        assert_eq!(config.next_style("formal").unwrap().id, "raw");
        for style in &mut config.styles {
            style.in_cycle = style.id == "prompt";
        }
        assert_eq!(config.next_style("prompt").unwrap().id, "prompt");
        config.styles.iter_mut().for_each(|s| s.in_cycle = false);
        assert!(config.next_style("clean").is_none());
        let mut config = CleanupConfig::default();
        config.styles.push(CleanupStyle { id: "custom-one".into(), ..CleanupStyle::default() });
        assert_eq!(config.next_style("prompt").unwrap().id, "custom-one");
        assert_eq!(config.next_style("custom-one").unwrap().id, "raw");
    }
}
