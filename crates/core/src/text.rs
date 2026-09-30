//! Turkish-aware text helpers shared by the rule engine, filters and eval.

/// Folds one char for case-insensitive matching.
///
/// Dotted and dotless i (`I`, `ı`, `İ`, `i`) all fold to `i`, so Turkish
/// (`HALA`/`hala`, `İstanbul`/`istanbul`) and English (`API`/`api`) spellings
/// match each other. Every other char folds to the first char of its lowercase
/// form, which keeps a 1:1 mapping between original and folded chars.
pub fn fold_char(c: char) -> char {
    match c {
        'I' | 'ı' | 'İ' | 'i' => 'i',
        _ => c.to_lowercase().next().unwrap_or(c),
    }
}

pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Uppercases the first char; with `turkish`, `i` becomes `İ` and `ı` becomes `I`.
pub fn upper_first(s: &str, turkish: bool) -> String {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let upper: String = match (turkish, first) {
        (true, 'i') => "İ".to_string(),
        (true, 'ı') => "I".to_string(),
        _ => first.to_uppercase().collect(),
    };
    upper + chars.as_str()
}

pub fn starts_lowercase(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_lowercase)
}

pub fn starts_uppercase(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_uppercase)
}

/// True when text following `before` starts a sentence.
pub fn sentence_start(before: &str) -> bool {
    for c in before.chars().rev() {
        if c == '\n' {
            return true;
        }
        if c.is_whitespace() {
            continue;
        }
        return matches!(c, '.' | '!' | '?' | '…');
    }
    true
}

/// Capitalizes `target` when it starts lowercase and the text it replaces
/// (`source`) started uppercase or sat at a sentence start.
pub fn capitalize_like(target: &str, source: &str, at_sentence_start: bool, turkish: bool) -> String {
    if starts_lowercase(target) && (at_sentence_start || starts_uppercase(source)) {
        upper_first(target, turkish)
    } else {
        target.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_matches_turkish_and_english_i_forms() {
        assert_eq!(fold("HALA"), "hala");
        assert_eq!(fold("İSTANBUL"), fold("istanbul"));
        assert_eq!(fold("ISTANBUL"), fold("ıstanbul"));
        assert_eq!(fold("API"), fold("api"));
        assert_eq!(fold("KAYIT"), fold("kayıt"));
    }

    #[test]
    fn fold_keeps_one_char_per_char() {
        let s = "İçİnde Iİıi ŞĞÜÖÇ";
        assert_eq!(fold(s).chars().count(), s.chars().count());
    }

    #[test]
    fn word_chars() {
        assert!(is_word_char('ş'));
        assert!(is_word_char('7'));
        assert!(is_word_char('_'));
        assert!(!is_word_char('\''));
        assert!(!is_word_char('’'));
        assert!(!is_word_char('.'));
    }

    #[test]
    fn upper_first_uses_turkish_i_rules() {
        assert_eq!(upper_first("iki tabağa", true), "İki tabağa");
        assert_eq!(upper_first("ılık", true), "Ilık");
        assert_eq!(upper_first("in chrome", false), "In chrome");
        assert_eq!(upper_first("hâlâ", true), "Hâlâ");
        assert_eq!(upper_first("", true), "");
    }

    #[test]
    fn sentence_start_detection() {
        assert!(sentence_start(""));
        assert!(sentence_start("   "));
        assert!(sentence_start("Geldi. "));
        assert!(sentence_start("Ne? "));
        assert!(sentence_start("satır\n"));
        assert!(sentence_start("Bekle… "));
        assert!(!sentence_start("o "));
        assert!(!sentence_start("dedi, "));
    }

    #[test]
    fn capitalize_like_rules() {
        assert_eq!(capitalize_like("hâlâ", "Hala", false, true), "Hâlâ");
        assert_eq!(capitalize_like("hâlâ", "hala", false, true), "hâlâ");
        assert_eq!(capitalize_like("hâlâ", "hala", true, true), "Hâlâ");
        assert_eq!(capitalize_like("GitHub", "github", true, false), "GitHub");
        assert_eq!(capitalize_like("", "X", true, true), "");
    }
}
