//! Builds the Whisper `prompt` (an example-text hint, not an instruction).

use serde::Serialize;

/// Whisper uses at most 224 prompt tokens.
pub const PROMPT_TOKEN_BUDGET: usize = 224;

/// Conservative token estimate: one token per 3 UTF-8 bytes, rounded up.
/// Turkish letters take 2 bytes, so they are naturally counted heavier.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(3)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuiltPrompt {
    pub prompt: Option<String>,
    pub included_terms: Vec<String>,
    pub dropped_terms: Vec<String>,
    pub estimated_tokens: usize,
}

pub fn build_prompt(context: &str, terms: &[String], language: &str) -> BuiltPrompt {
    let mut base = context.split_whitespace().collect::<Vec<_>>().join(" ");
    if !base.is_empty() && !base.ends_with(['.', '!', '?', '…']) {
        base.push('.');
    }
    let base = truncate_to_budget(&base);
    let label = if language == "tr" { "Geçen terimler: " } else { "Terms: " };

    let mut included: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for term in terms {
        if dropped.is_empty() {
            included.push(term.clone());
            if estimate_tokens(&render(&base, label, &included)) <= PROMPT_TOKEN_BUDGET {
                continue;
            }
            included.pop();
        }
        dropped.push(term.clone());
    }

    let prompt = render(&base, label, &included);
    BuiltPrompt {
        estimated_tokens: estimate_tokens(&prompt),
        prompt: (!prompt.is_empty()).then_some(prompt),
        included_terms: included,
        dropped_terms: dropped,
    }
}

/// Terms that can be sent as `keywords[]` (single line, no angle brackets).
pub fn keywords(terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && !t.contains(['<', '>', '\r', '\n']))
        .map(str::to_string)
        .collect()
}

fn render(base: &str, label: &str, terms: &[String]) -> String {
    if terms.is_empty() {
        return base.to_string();
    }
    let list = terms.join(", ");
    if base.is_empty() { format!("{label}{list}.") } else { format!("{base} {label}{list}.") }
}

fn truncate_to_budget(text: &str) -> String {
    if estimate_tokens(text) <= PROMPT_TOKEN_BUDGET {
        return text.to_string();
    }
    let mut end = PROMPT_TOKEN_BUDGET * 3;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nothing_to_say_gives_no_prompt() {
        let built = build_prompt("  ", &[], "tr");
        assert_eq!(built.prompt, None);
        assert_eq!(built.estimated_tokens, 0);
    }

    #[test]
    fn context_gets_a_final_period() {
        let built = build_prompt("FiveM  üzerine konuşma", &[], "tr");
        assert_eq!(built.prompt.as_deref(), Some("FiveM üzerine konuşma."));
    }

    #[test]
    fn turkish_label_and_term_list() {
        let built = build_prompt("Yazılım konuşması.", &terms(&["Claude Code", "GitHub"]), "tr");
        assert_eq!(built.prompt.as_deref(), Some("Yazılım konuşması. Geçen terimler: Claude Code, GitHub."));
        assert_eq!(built.included_terms, ["Claude Code", "GitHub"]);
        assert!(built.dropped_terms.is_empty());
    }

    #[test]
    fn other_languages_use_english_label() {
        let built = build_prompt("", &terms(&["GitHub"]), "en");
        assert_eq!(built.prompt.as_deref(), Some("Terms: GitHub."));
    }

    #[test]
    fn budget_cuts_the_tail_and_keeps_priority() {
        let many: Vec<String> = (0..200).map(|i| format!("term{i:03}")).collect();
        let built = build_prompt("Bağlam.", &many, "tr");
        let n = built.included_terms.len();
        assert!(n > 10 && n < 200, "{n}");
        assert_eq!(built.included_terms, many[..n]);
        assert_eq!(built.dropped_terms, many[n..]);
        assert!(built.estimated_tokens <= PROMPT_TOKEN_BUDGET);
        assert_eq!(built.estimated_tokens, estimate_tokens(built.prompt.as_deref().unwrap()));
    }

    #[test]
    fn oversized_context_is_truncated_on_a_char_boundary() {
        let context = "ğ".repeat(1000);
        let built = build_prompt(&context, &terms(&["GitHub"]), "tr");
        let prompt = built.prompt.unwrap();
        assert!(estimate_tokens(&prompt) <= PROMPT_TOKEN_BUDGET);
        assert!(prompt.starts_with("ğğ"));
        assert_eq!(built.dropped_terms, ["GitHub"]);
    }

    #[test]
    fn token_estimate_is_bytes_over_three_rounded_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 2);
        assert_eq!(estimate_tokens("ğ"), 1);
    }

    #[test]
    fn keywords_drop_unsafe_terms() {
        let list = terms(&["Claude Code", "a<b", "line\nbreak", "ox_lib", " "]);
        assert_eq!(keywords(&list), ["Claude Code", "ox_lib"]);
    }
}
