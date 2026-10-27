//! Drops outputs that are nothing but a known silence hallucination.

use std::collections::HashSet;

use crate::text::fold;

// Canonical Latin accent groups; decomposed combining marks are discarded below.
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' | 'ǟ' | 'ǡ' | 'ǻ' | 'ȁ' | 'ȃ' | 'ȧ' | 'ḁ' | 'ạ'
        | 'ả' | 'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'ḉ' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ȅ' | 'ȇ' | 'ȩ' | 'ḕ' | 'ḗ' | 'ḙ' | 'ḛ' | 'ḝ' | 'ẹ'
        | 'ẻ' | 'ẽ' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ǐ' | 'ȉ' | 'ȋ' | 'ḭ' | 'ḯ' | 'ỉ' | 'ị' => {
            'i'
        }
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ǹ' | 'ṅ' | 'ṇ' | 'ṉ' | 'ṋ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ŏ' | 'ő' | 'ơ' | 'ǒ' | 'ǫ' | 'ǭ' | 'ȍ' | 'ȏ' | 'ȫ' | 'ȭ' | 'ȯ' | 'ȱ'
        | 'ṍ' | 'ṏ' | 'ṑ' | 'ṓ' | 'ọ' | 'ỏ' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' => {
            'o'
        }
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ư' | 'ǔ' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' | 'ȕ' | 'ȗ'
        | 'ṳ' | 'ṵ' | 'ṷ' | 'ṹ' | 'ṻ' | 'ụ' | 'ủ' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => 'u',
        'ý' | 'ÿ' | 'ŷ' | 'ȳ' | 'ẏ' | 'ẙ' | 'ỳ' | 'ỵ' | 'ỷ' | 'ỹ' => 'y',
        'ď' | 'ḋ' | 'ḍ' | 'ḏ' | 'ḑ' | 'ḓ' => 'd',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'ǧ' | 'ǵ' | 'ḡ' => 'g',
        'ĥ' | 'ȟ' | 'ḣ' | 'ḥ' | 'ḧ' | 'ḩ' | 'ḫ' | 'ẖ' => 'h',
        'ĵ' | 'ǰ' => 'j',
        'ķ' | 'ǩ' | 'ḱ' | 'ḳ' | 'ḵ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ḷ' | 'ḹ' | 'ḻ' | 'ḽ' => 'l',
        'ŕ' | 'ŗ' | 'ř' | 'ȑ' | 'ȓ' | 'ṙ' | 'ṛ' | 'ṝ' | 'ṟ' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' | 'ṡ' | 'ṣ' | 'ṥ' | 'ṧ' | 'ṩ' => 's',
        'ţ' | 'ť' | 'ț' | 'ṫ' | 'ṭ' | 'ṯ' | 'ṱ' | 'ẗ' => 't',
        'ŵ' | 'ẁ' | 'ẃ' | 'ẅ' | 'ẇ' | 'ẉ' | 'ẘ' => 'w',
        'ź' | 'ż' | 'ž' | 'ẑ' | 'ẓ' | 'ẕ' => 'z',
        'ḃ' | 'ḅ' | 'ḇ' => 'b',
        'ḟ' => 'f',
        'ḿ' | 'ṁ' | 'ṃ' => 'm',
        'ṕ' | 'ṗ' => 'p',
        'ṽ' | 'ṿ' => 'v',
        'ẋ' | 'ẍ' => 'x',
        _ => c,
    }
}

/// Folds Turkish case and accents, ignores punctuation and collapses whitespace.
pub fn normalize(text: &str) -> String {
    let folded: String =
        fold(text).chars().map(fold_accent).filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Default, Clone)]
pub struct HallucinationFilter {
    patterns: HashSet<String>,
}

impl HallucinationFilter {
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        let patterns = patterns.into_iter().map(|p| normalize(&p).replace(' ', "")).filter(|p| !p.is_empty()).collect();
        Self { patterns }
    }

    /// True only when the entire output is a known hallucination.
    pub fn matches(&self, text: &str) -> bool {
        let normalized = normalize(text).replace(' ', "");
        !normalized.is_empty()
            && self.patterns.iter().any(|pattern| {
                // Only complete phrase repetitions may cover the entire output.
                normalized.len().is_multiple_of(pattern.len())
                    && normalized.as_bytes().chunks(pattern.len()).all(|chunk| chunk == pattern.as_bytes())
            })
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
        assert!(f.matches("Thank,you!Thank you..."));
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
        assert_eq!(normalize("  “Altyazı  M.K.”  "), "altyazi mk");
        assert_eq!(normalize("¿?"), "");
        assert!(filter().matches("THANK, YOU! Thank you. THANK YOU."));
        assert!(
            HallucinationFilter::new(["izlediğiniz için teşekkürler".into()]).matches("IZLEDIGINIZ ICIN TESEKKURLER!")
        );
        assert!(!filter().matches("thank you thank you for your help"));
    }
}
