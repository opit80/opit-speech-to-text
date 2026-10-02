//! Spell standalone integers without changing identifiers, dates, times or decimal notation.

use std::sync::OnceLock;

use regex::Regex;

use super::{RuleHit, RuleKind, RuleRef};
use crate::text::{sentence_start, upper_first};

const TR_ONES: [&str; 10] = ["sıfır", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz"];
const TR_TENS: [&str; 10] = ["", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan"];
const EN_SMALL: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const EN_TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

fn group(n: u64, tr: bool) -> String {
    let mut words = Vec::new();
    if n >= 100 {
        if !tr || n / 100 > 1 {
            words.push(if tr { TR_ONES[(n / 100) as usize] } else { EN_SMALL[(n / 100) as usize] }.to_string());
        }
        words.push(if tr { "yüz" } else { "hundred" }.into());
    }
    let rest = (n % 100) as usize;
    if tr {
        if rest >= 10 {
            words.push(TR_TENS[rest / 10].into());
        }
        if !rest.is_multiple_of(10) {
            words.push(TR_ONES[rest % 10].into());
        }
    } else if rest >= 20 {
        words.push(if rest.is_multiple_of(10) {
            EN_TENS[rest / 10].into()
        } else {
            format!("{}-{}", EN_TENS[rest / 10], EN_SMALL[rest % 10])
        });
    } else if rest > 0 {
        words.push(EN_SMALL[rest].into());
    }
    words.join(" ")
}

fn integer(mut n: u64, tr: bool) -> String {
    if n == 0 {
        return if tr { TR_ONES[0] } else { EN_SMALL[0] }.into();
    }
    let scales = if tr {
        ["", "bin", "milyon", "milyar", "trilyon"]
    } else {
        ["", "thousand", "million", "billion", "trillion"]
    };
    let mut parts = Vec::new();
    let mut scale = 0;
    while n > 0 {
        let chunk = n % 1000;
        if chunk > 0 {
            let prefix = if tr && scale == 1 && chunk == 1 { String::new() } else { group(chunk, tr) };
            parts.push(format!("{} {}", prefix, scales[scale]).trim().to_string());
        }
        scale += 1;
        n /= 1000;
    }
    parts.reverse();
    parts.join(" ")
}

pub fn spell_integers(text: &str, language: &str) -> (String, Vec<RuleHit>) {
    let language = language.to_ascii_lowercase();
    let tr = language == "tr" || language.starts_with("tr-");
    if !tr && language != "en" && !language.starts_with("en-") {
        return (text.into(), Vec::new());
    }
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let re = PATTERN.get_or_init(|| Regex::new(r"-?[0-9]+(?:[.,:/-][0-9]+)*").expect("static number regex"));
    let mut out = String::with_capacity(text.len());
    let mut hits = Vec::new();
    let mut end = 0;
    for m in re.find_iter(text) {
        out.push_str(&text[end..m.start()]);
        end = m.end();
        let raw = m.as_str();
        let digits = raw.trim_start_matches('-');
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        let identifier = |c: char| c.is_alphanumeric() || matches!(c, '_' | '/' | ':' | '-' | '+' | '@' | '\'' | '’');
        let attached = before.is_some_and(|c| identifier(c) || matches!(c, '.' | '=' | '#' | '?' | '&'))
            || after.is_some_and(identifier)
            || (after == Some('.') && text[m.end() + 1..].chars().next().is_some_and(char::is_alphanumeric));
        let valid =
            !attached && digits.bytes().all(|b| b.is_ascii_digit()) && (digits == "0" || !digits.starts_with('0'));
        let n = if valid { digits.parse::<u64>().ok().filter(|n| *n < 1_000_000_000_000_000) } else { None };
        let Some(n) = n else {
            out.push_str(raw);
            continue;
        };
        let words = integer(n, tr);
        let words = if raw.starts_with('-') { format!("{} {words}", if tr { "eksi" } else { "minus" }) } else { words };
        let words = if sentence_start(&text[..m.start()]) { upper_first(&words, tr) } else { words };
        out.push_str(&words);
        hits.push(RuleHit {
            rule: RuleRef { pack_id: "numbers".into(), kind: RuleKind::Numbers, index: hits.len() },
            from: raw.into(),
            to: words,
        });
    }
    out.push_str(&text[end..]);
    (out, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_turkish_numbers_and_respects_sentence_case() {
        let (text, hits) = spell_integers("12 kişi, 100 dosya, 1001 satır ve -42 hata. 0 kayıt.", "tr");
        assert_eq!(text, "On iki kişi, yüz dosya, bin bir satır ve eksi kırk iki hata. Sıfır kayıt.");
        assert_eq!(hits.len(), 5);
        assert_eq!(spell_integers("toplam 2000001", "tr-TR").0, "toplam iki milyon bir");
    }

    #[test]
    fn preserves_structured_numbers_and_identifiers() {
        let text = "v1.2.3 GPT4 FiveM 01 007 3,5 1.000 12:30 2026-10-01 01/10/2026 +905551234567 42'ye x_12 #12 x.12 12.txt port=12 1234567890123456";
        assert_eq!(spell_integers(text, "tr").0, text);
        assert_eq!(spell_integers("12 files", "de").0, "12 files");
    }

    #[test]
    fn spells_english_and_handles_punctuation() {
        assert_eq!(
            spell_integers("21 files, 105 changes and 1000 lines.", "en-US").0,
            "Twenty-one files, one hundred five changes and one thousand lines."
        );
        assert_eq!(spell_integers("toplam 12.", "tr").0, "toplam on iki.");
    }
}
