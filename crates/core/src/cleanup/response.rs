use super::CleanupError;

pub fn strip_answer(answer: &str, input: &str) -> Result<String, CleanupError> {
    strip_transcript_answer(answer, editing_limit(input.chars().count()), &TranscriptFrame::for_input(input))
}

pub(super) fn editing_limit(input_chars: usize) -> usize {
    input_chars.saturating_mul(3).saturating_add(120)
}

pub(super) struct TranscriptFrame {
    pub(super) opening: String,
    pub(super) closing: String,
}

impl TranscriptFrame {
    pub(super) fn for_input(input: &str) -> Self {
        let mut variant = 0;
        loop {
            let tag = if variant == 0 { "transcript".into() } else { format!("transcript-{variant}") };
            let frame = Self { opening: format!("<{tag}>"), closing: format!("</{tag}>") };
            // Neither boundary may occur in the input, including a lone opening or closing tag.
            if !input.contains(&frame.opening) && !input.contains(&frame.closing) {
                return frame;
            }
            variant += 1;
        }
    }
}

pub(super) fn strip_transcript_answer(
    answer: &str,
    limit: usize,
    frame: &TranscriptFrame,
) -> Result<String, CleanupError> {
    // Strip echoed transcript wrappers only for cleanup; voice modes keep their existing parser.
    let unwrapped = strip_answer_with_limit(answer, usize::MAX)?;
    // A model may echo either boundary alone. Only remove the boundaries chosen for this request.
    let answer = unwrapped.strip_prefix(&frame.opening).unwrap_or(&unwrapped).trim();
    let answer = answer.strip_suffix(&frame.closing).unwrap_or(answer).trim();
    let answer = strip_answer_with_limit(answer, limit)?;
    if answer.contains(&frame.opening) || answer.contains(&frame.closing) {
        return Err(CleanupError::BadResponse);
    }
    Ok(answer)
}

pub(super) fn has_word_overlap(input: &str, answer: &str) -> bool {
    let input = crate::text::fold(input);
    let words: std::collections::HashSet<_> =
        input.split(|c| !crate::text::is_word_char(c)).filter(|s| s.chars().count() >= 3).collect();
    // For clean/prompt only, reject zero overlap across at least three distinct words of 3+ characters.
    // One exact word or shared 4-character prefix suffices, allowing Turkish suffix changes.
    words.len() < 3
        || crate::text::fold(answer).split(|c| !crate::text::is_word_char(c)).any(|s| {
            words.contains(s)
                || (s.chars().count() >= 4
                    && words.iter().any(|word| word.chars().zip(s.chars()).take_while(|(a, b)| a == b).count() >= 4))
        })
}

pub(super) fn is_transcript_meta_reply(input: &str, answer: &str) -> bool {
    // Missing-transcript claims and requests are independent of overlap, even for a single word.
    // If the input itself mentions a transcript, such wording may be a legitimate edit.
    !references_transcript(input) && transcript_meta_kinds(answer) != 0
}

fn references_transcript(text: &str) -> bool {
    let text = crate::text::fold(text);
    let words: Vec<_> = text.split(|c| !crate::text::is_word_char(c)).filter(|s| !s.is_empty()).collect();
    words.iter().enumerate().any(|(index, word)| {
        word.starts_with("döküm")
            || matches!(*word, "transcript" | "transcripts" | "transcription")
            || (*word == "konuşma" && words.get(index + 1).is_some_and(|s| s.starts_with("metn")))
    })
}

pub(super) fn prompt_leak_markers(text: &str) -> u8 {
    let mut markers = u8::from(references_transcript(text));
    let text = crate::text::fold(text);
    let words: Vec<_> = text.split(|c| !crate::text::is_word_char(c)).filter(|s| !s.is_empty()).collect();
    if words.iter().any(|word| word.starts_with("sözlük") || matches!(*word, "glossary" | "glossaries")) {
        markers |= 2;
    }
    let english_person = |word: &str| matches!(word, "user" | "users" | "speaker" | "speakers");
    if words.windows(2).any(|pair| {
        ((pair[0].starts_with("kullanici") || pair[0].starts_with("konuşmaci")) && pair[1].starts_with("mesaj"))
            || (english_person(pair[0]) && pair[1] == "message")
    }) || words.windows(3).any(|triple| english_person(triple[0]) && triple[1] == "s" && triple[2] == "message")
    {
        markers |= 4;
    }
    markers
}

fn transcript_meta_kinds(text: &str) -> u8 {
    let text = crate::text::fold(text).replace("n't", " not").replace("n’t", " not");
    let mut kinds = 0;
    for clause in text.split(['.', '!', '?', ';', '\n']) {
        let words: Vec<_> = clause.split(|c| !crate::text::is_word_char(c)).filter(|s| !s.is_empty()).collect();
        for (index, word) in words.iter().enumerate() {
            let turkish = word.starts_with("döküm")
                || (*word == "konuşma" && words.get(index + 1).is_some_and(|s| s.starts_with("metn")));
            let english = matches!(*word, "transcript" | "transcripts" | "transcription");
            if !turkish && !english {
                continue;
            }
            // Require a transcript reference and a specific missing/request phrase within six words.
            let nearby = &words[index.saturating_sub(6)..words.len().min(index + 7)];
            let has = |needle: &str| nearby.contains(&needle);
            if turkish {
                if nearby.iter().any(|s| {
                    [
                        "paylaşılmad",
                        "paylaşılmam",
                        "sağlanmad",
                        "sağlanmam",
                        "verilmed",
                        "verilmem",
                        "gönderilmed",
                        "gönderilmem",
                    ]
                    .iter()
                    .any(|stem| s.starts_with(&crate::text::fold(stem)))
                }) {
                    kinds |= 1;
                }
                if [
                    "gönder",
                    "gönderin",
                    "gönderiniz",
                    "paylaş",
                    "paylaşın",
                    "paylaşınız",
                    "sağla",
                    "sağlayın",
                    "sağlayınız",
                    "gönderirseniz",
                    "paylaşırsanız",
                ]
                .iter()
                .any(|request| has(&crate::text::fold(request)))
                {
                    kinds |= 2;
                }
            }
            if english {
                let supplied =
                    |word: &str| matches!(word, "provided" | "supplied" | "given" | "shared" | "sent" | "received");
                let not_supplied = nearby.windows(2).any(|pair| pair[0] == "not" && supplied(pair[1]))
                    || nearby.windows(3).any(|triple| triple[0] == "not" && triple[1] == "been" && supplied(triple[2]));
                if has("missing") || has("absent") || (index > 0 && words[index - 1] == "no") || not_supplied {
                    kinds |= 1;
                }
                if has("please") && nearby.iter().any(|s| matches!(*s, "provide" | "supply" | "share" | "send")) {
                    kinds |= 2;
                }
            }
        }
    }
    kinds
}

pub fn strip_answer_with_limit(answer: &str, limit: usize) -> Result<String, CleanupError> {
    let mut answer = answer.trim();
    if answer.starts_with("```") && answer.ends_with("```") && answer.len() >= 6 {
        let inner = &answer[3..answer.len() - 3];
        answer = match inner.split_once('\n') {
            Some((label, body))
                if label.trim().chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) =>
            {
                body.trim()
            }
            _ => inner.trim(),
        };
    }
    for (left, right) in [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’')] {
        if answer.starts_with(left) && answer.ends_with(right) && answer.len() >= left.len_utf8() + right.len_utf8() {
            answer = answer[left.len_utf8()..answer.len() - right.len_utf8()].trim();
            break;
        }
    }
    if answer.is_empty() || answer.chars().count() > limit {
        return Err(CleanupError::BadResponse);
    }
    Ok(answer.into())
}
