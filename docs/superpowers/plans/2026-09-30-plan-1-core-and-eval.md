# Plan 1 — Core Library + Eval CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `opit-core`, the Tauri-free Rust library that turns a microphone recording into corrected text through a BYOK OpenAI-compatible provider, plus `opit-eval`, a CLI that measures WER and term accuracy with the rule layer on and off.

**Architecture:** A Cargo workspace. `crates/core` (package `opit-core`) holds the pure logic: audio prep (downmix, resample, silence gate, WAV/FLAC), provider client with retry and fallback, rule engine (corrections, replacements, term casing, hallucination filter), prompt builder, config schema, and SQLite history. It has no Tauri or OS-specific dependency and its tests run on Windows and Linux. `crates/eval` (package `opit-eval`) is a developer CLI built on the core pipeline. Built-in rule packs live as YAML in `rules/` and are embedded into the core with `include_str!`.

**Tech Stack:** Rust 2024 edition (toolchain ≥ 1.88), tokio, reqwest 0.13 (multipart, json, default rustls TLS), serde + serde_json + serde-saphyr (YAML), indexmap, regex, thiserror, hound (WAV), flacenc (FLAC), rusqlite 0.40 `bundled` (FTS5 included), clap, anyhow; tests use wiremock, tempfile, claxon.

**Spec:** `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md` (read it first; its decisions are locked).

## Global Constraints

- Names: product **Opit Speech to Text**; slug `opit-speech-to-text`; packages `opit-core` (lib `opit_core`) and `opit-eval`; data dir `%APPDATA%\opit-speech-to-text\`.
- Repo, code, comments, README: **English**. License **MIT**.
- `opit-core` must not depend on Tauri or any Windows-only crate; `cargo test -p opit-core` must pass on Linux.
- No LLM correction step. Accuracy = prompt/keywords + deterministic rules only.
- Transcript text and API keys are **never** logged or put into error messages.
- Silence gate: 30 ms frames; recordings < **0.4 s** are not sent; speech-frame total < **0.3 s** is not sent.
- Request: `POST {base_url}/audio/transcriptions`, multipart `file`, `model`, `language`, `temperature=0`, `response_format`, optional `prompt`, optional repeated `keywords[]`.
- Timeouts: connect **5 s**; total **30 s + recording length / 4**.
- verbose_json segments are dropped when `no_speech_prob > 0.6` **and** `avg_logprob < -1.0`.
- Retry: on network error / 5xx / 429, one retry after **500 ms**, then the fallback profile once, then error. 401/403 never retried.
- Prompt budget: **224 tokens**; terms in priority order (personal pack first), cut when the budget is full.
- Rule order: `corrections` → `replacements` → `terms` casing. Hallucination filter drops only when the **whole** normalized output equals a pattern.
- Rule packs are YAML with `schema: 1`. Built-ins: `tr-core`, `tr-tech`, `fivem`. The personal pack `user.yaml` never enters the repo.
- Recording limit default **180 s**, max **600 s**.
- CI must pass `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A hand-edited `user.yaml` with a typo'd field, wrong `schema`, invalid regex, empty `from`, or a regex that matches the empty string must produce a clear error or a skipped-rule warning — never a panic or a hang (tests in Tasks 3 and 5).
2. A provider (or a proxy in front of it) that answers with an HTML error page or non-JSON 200 must map to a typed error (`Server`, `Http`, `BadResponse`), not a crash or garbled text (tests in Task 12).
3. Microphone samples outside [-1, 1] or NaN/∞ must be sanitized before resampling and encoding (tests in Tasks 8 and 10).
4. A rule phrase must never replace text inside a longer word (`api` in `kapital`), and Turkish suffixes after straight **and** curly apostrophes (`cloud code'u`, `cloud code’u`) must keep working (tests in Task 2).
5. A maximum-length (600 s) recording must stay under the 25 MB provider upload limit as 16 kHz mono 16-bit WAV (test in Task 10).

---

## File Map

```
Cargo.toml                         workspace root (members, shared dependency versions)
.gitattributes                     LF line endings in the repo
.gitignore                         (exists) build outputs
LICENSE                            MIT
README.md                          project intro, layout, eval pointer
.github/workflows/ci.yml           fmt + clippy + test on windows-latest and ubuntu-latest
rules/tr-core.yaml                 Turkish spelling + hallucination patterns
rules/tr-tech.yaml                 software terms
rules/fivem.yaml                   FiveM / QBCore / ox_* terms
docs/eval.md                       how to build a personal eval dataset and run opit-eval
crates/core/Cargo.toml
crates/core/src/lib.rs             module list
crates/core/src/text.rs            Turkish-aware fold, capitalization, sentence-start detection
crates/core/src/rules/mod.rs       rules module list + re-exports
crates/core/src/rules/matcher.rs   PhraseMatcher: single-pass, longest-first, word-boundary aware
crates/core/src/rules/pack.rs      RulePack YAML model, validation, file loading
crates/core/src/rules/hallucination.rs  whole-output hallucination filter
crates/core/src/rules/engine.rs    RuleSet: compile packs, apply corrections/replacements/casing, trace hits
crates/core/src/rules/prompt.rs    Whisper prompt builder with 224-token budget; keyword list
crates/core/src/rules/builtin.rs   embedded built-in packs + pack assembly in priority order
crates/core/src/audio/mod.rs       Recording, sanitize, downmix, to_mono_16k
crates/core/src/audio/resample.rs  windowed-sinc resampler
crates/core/src/audio/gate.rs      silence gate + RMS
crates/core/src/audio/encode.rs    PCM16, WAV (hound), FLAC (flacenc)
crates/core/src/config.rs          AppConfig schema, defaults, migration, load/save
crates/core/src/provider/mod.rs    Transcriber trait, request/response types, ProviderError
crates/core/src/provider/profile.rs Profile + presets (Groq, OpenAI, custom)
crates/core/src/provider/openai.rs OpenAiCompatible HTTP client
crates/core/src/provider/retry.rs  one-retry helper
crates/core/src/pipeline.rs        prepare → transcribe (retry, fallback) → postprocess
crates/core/src/history/mod.rs     HistoryStore (rusqlite + FTS5)
crates/core/src/history/audio_store.rs  optional WAV files per dictation
crates/core/tests/pipeline_http.rs end-to-end pipeline against wiremock
crates/eval/Cargo.toml
crates/eval/src/lib.rs
crates/eval/src/metrics.rs         WER + term hit rate
crates/eval/src/dataset.rs         wav+txt pairs loader
crates/eval/src/run.rs             rules on/off evaluation loop
crates/eval/src/report.rs          Markdown table
crates/eval/src/main.rs            clap CLI
crates/eval/tests/run_eval.rs      eval loop against wiremock
```

---

### Task 1: Workspace scaffold + Turkish-aware text helpers

**Files:**
- Create: `Cargo.toml`, `.gitattributes`, `LICENSE`, `README.md`, `.github/workflows/ci.yml`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/text.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (`opit_core::text`):
  - `fn fold_char(c: char) -> char` — `I`, `ı`, `İ`, `i` all fold to `i`; other chars to the first char of their lowercase. Always 1 char in, 1 char out.
  - `fn fold(s: &str) -> String`
  - `fn is_word_char(c: char) -> bool` — alphanumeric or `_`.
  - `fn upper_first(s: &str, turkish: bool) -> String` — Turkish maps `i→İ`, `ı→I`.
  - `fn starts_lowercase(s: &str) -> bool`, `fn starts_uppercase(s: &str) -> bool`
  - `fn sentence_start(before: &str) -> bool` — true when `before` is empty/whitespace, or ends (ignoring spaces) with `.`, `!`, `?`, `…`, or a newline.
  - `fn capitalize_like(target: &str, source: &str, at_sentence_start: bool, turkish: bool) -> String` — uppercases `target`'s first letter when `target` starts lowercase and (`at_sentence_start` or `source` starts uppercase).

- [ ] **Step 1: Create the workspace files**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/core"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
license = "MIT"
repository = "https://github.com/opit80/opit-speech-to-text"

[workspace.dependencies]
anyhow = "1"
claxon = "0.4"
clap = { version = "4.6.7", features = ["derive"] }
flacenc = "0.5.1"
hound = "3.5.1"
indexmap = { version = "2.14.2", features = ["serde"] }
regex = "1.13.1"
reqwest = { version = "0.13.5", features = ["json", "multipart"] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1.0.229", features = ["derive"] }
serde-saphyr = "1.3.0"
serde_json = "1.0.151"
tempfile = "3.27.0"
thiserror = "2.0.21"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "time"] }
wiremock = "0.6.5"
```

`.gitattributes`:
```
* text=auto eol=lf
*.png binary
*.ico binary
*.wav binary
*.flac binary
```

`LICENSE`:
```
MIT License

Copyright (c) 2026 opit80

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

`README.md`:
````markdown
# Opit Speech to Text

Open-source voice dictation for Windows. Press a hotkey, speak, and the text is pasted where your
cursor is. Transcription runs on **your own API key** (Groq, OpenAI, or any OpenAI-compatible
server); nothing goes through a third-party server of ours.

> **Status:** early development. The design lives in
> [`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`](docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md).

## Repository layout

| Path | What |
|---|---|
| `crates/core` | `opit-core`: audio prep, provider client, rule engine, history — no UI, no OS code |
| `rules/` | Built-in rule packs (`tr-core`, `tr-tech`, `fivem`) |
| `docs/` | Design brief and implementation plans |

## Development

```sh
cargo test --workspace
```

## License

[MIT](LICENSE)
````

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, ubuntu-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

`crates/core/Cargo.toml`:
```toml
[package]
name = "opit-core"
description = "Core dictation logic for Opit Speech to Text: audio prep, provider client, rules, history"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]

[dev-dependencies]
```

`crates/core/src/lib.rs`:
```rust
//! Core dictation logic for Opit Speech to Text.
//!
//! This crate is deliberately free of Tauri and OS-specific code so it can be
//! tested on any platform. The app crate supplies recordings and consumes
//! transcripts through [`pipeline`].

pub mod text;
```

- [ ] **Step 2: Write the failing tests for `text`**

`crates/core/src/text.rs` (tests first; the functions come in Step 4):
```rust
//! Turkish-aware text helpers shared by the rule engine, filters and eval.

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core text::`
Expected: FAIL — compile errors `cannot find function fold` etc.

- [ ] **Step 4: Implement the helpers**

Insert above the `#[cfg(test)]` block in `crates/core/src/text.rs`:
```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core text::`
Expected: 6 passed.

- [ ] **Step 6: Run the CI checks locally**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no output from fmt, clippy finishes with no warnings. If fmt complains, run `cargo fmt --all` and re-check.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitattributes LICENSE README.md .github crates/core
git commit -m "chore: scaffold workspace and add Turkish-aware text helpers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: PhraseMatcher

**Files:**
- Create: `crates/core/src/rules/mod.rs`, `crates/core/src/rules/matcher.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod rules;`)

**Interfaces:**
- Consumes: `text::{fold_char, is_word_char, sentence_start, capitalize_like}` (Task 1).
- Produces (`opit_core::rules::matcher`):
  ```rust
  pub struct Pattern { pub from: String, pub to: String, pub rule: usize, pub case_sensitive: bool, pub turkish: bool }
  pub struct Found { pub start: usize, pub end: usize, pub pattern: usize }   // byte offsets
  pub struct MatchHit { pub rule: usize, pub from: String, pub to: String }
  pub struct PhraseMatcher;
  impl PhraseMatcher {
      pub fn new(patterns: impl IntoIterator<Item = Pattern>) -> Self;
      pub fn is_empty(&self) -> bool;
      pub fn patterns(&self) -> &[Pattern];
      pub fn find_all(&self, text: &str) -> Vec<Found>;
      pub fn replace_all(&self, text: &str) -> (String, Vec<MatchHit>);
  }
  ```
  Semantics: single left-to-right pass; at each position the longest matching pattern wins; a word boundary is required only on pattern edges that are word chars; empty/whitespace `from` is ignored; when two patterns fold to the same `from`, the first one wins; `MatchHit`s are only reported when the text actually changed; case-insensitive patterns keep capitalization via `capitalize_like`.

- [ ] **Step 1: Wire the module**

`crates/core/src/rules/mod.rs`:
```rust
//! Deterministic accuracy layer: rule packs, matching, prompt building.

pub mod matcher;
```

Append to `crates/core/src/lib.rs`:
```rust
pub mod rules;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/rules/matcher.rs`:
```rust
//! Single-pass phrase matcher used by corrections, literal replacements and
//! term casing.

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::matcher`
Expected: FAIL — `cannot find type Pattern` etc.

- [ ] **Step 4: Implement the matcher**

Insert above the tests in `crates/core/src/rules/matcher.rs`:
```rust
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
            let needle: Vec<char> = if pattern.case_sensitive {
                from.chars().collect()
            } else {
                from.chars().map(fold_char).collect()
            };
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
            let right_ok =
                !is_word_char(needle[len - 1]) || i + len == chars.len() || !is_word_char(chars[i + len].1);
            if left_ok && right_ok {
                return Some((index, len));
            }
        }
        None
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::matcher`
Expected: 13 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): add Turkish-aware phrase matcher

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Rule pack model and YAML loading

**Files:**
- Create: `crates/core/src/rules/pack.rs`
- Modify: `crates/core/src/rules/mod.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (`opit_core::rules::pack`):
  ```rust
  pub const PACK_SCHEMA: u32 = 1;
  pub struct RulePack { pub schema: u32, pub id: String, pub name: String, pub language: String,
      pub terms: Vec<String>, pub corrections: IndexMap<String, Vec<String>>,
      pub replacements: Vec<Replacement>, pub hallucinations: Vec<String> }
  pub struct Replacement { pub from: String, pub to: String, pub case_sensitive: bool, pub regex: bool }
  pub enum PackError { Yaml(String), Schema { found: u32 }, BadId(String), Io(String) }
  impl RulePack {
      pub fn from_yaml(src: &str) -> Result<Self, PackError>;
      pub fn to_yaml(&self) -> Result<String, PackError>;
      pub fn is_turkish(&self) -> bool;
  }
  pub fn load_pack_file(path: &std::path::Path) -> Result<RulePack, PackError>;
  ```
  Unknown fields are rejected (`deny_unknown_fields`) so typos surface. `language` defaults to `"tr"`. Pack ids are non-empty `[a-z0-9-]`.

- [ ] **Step 1: Add dependencies**

In `crates/core/Cargo.toml`:
```toml
[dependencies]
indexmap.workspace = true
serde.workspace = true
serde-saphyr.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Append to `crates/core/src/rules/mod.rs`:
```rust
pub mod pack;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/rules/pack.rs`:
```rust
//! Rule pack YAML model (`schema: 1`).

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
schema: 1
id: tr-tech
name: Turkish – software terms
language: tr
terms: [Claude Code, GitHub]
corrections:
  Claude Code: [cloud code, clod code]
replacements:
  - { from: hala, to: hâlâ }
  - { from: '(\d+) tl', to: '$1 TL', regex: true, case_sensitive: true }
hallucinations: [altyazı m.k]
"#;

    #[test]
    fn parses_a_full_pack() {
        let pack = RulePack::from_yaml(FULL).unwrap();
        assert_eq!(pack.id, "tr-tech");
        assert_eq!(pack.terms, vec!["Claude Code", "GitHub"]);
        assert_eq!(pack.corrections["Claude Code"], vec!["cloud code", "clod code"]);
        assert_eq!(pack.replacements[1].to, "$1 TL");
        assert!(pack.replacements[1].regex);
        assert!(pack.replacements[1].case_sensitive);
        assert!(!pack.replacements[0].regex);
        assert!(pack.is_turkish());
    }

    #[test]
    fn optional_sections_default_to_empty() {
        let pack = RulePack::from_yaml("schema: 1\nid: min\nname: Minimal\n").unwrap();
        assert_eq!(pack.language, "tr");
        assert!(pack.terms.is_empty() && pack.corrections.is_empty());
        assert!(pack.replacements.is_empty() && pack.hallucinations.is_empty());
    }

    #[test]
    fn rejects_other_schema_versions() {
        let err = RulePack::from_yaml("schema: 2\nid: x\nname: X\n").unwrap_err();
        assert!(matches!(err, PackError::Schema { found: 2 }));
    }

    #[test]
    fn rejects_unknown_fields_with_their_name() {
        let err = RulePack::from_yaml("schema: 1\nid: x\nname: X\ncorections: {}\n").unwrap_err();
        assert!(matches!(&err, PackError::Yaml(msg) if msg.contains("corections")), "{err}");
    }

    #[test]
    fn rejects_bad_ids() {
        for id in ["", "Tr Core", "tr_core", "ç"] {
            let src = format!("schema: 1\nid: '{id}'\nname: X\n");
            assert!(matches!(RulePack::from_yaml(&src), Err(PackError::BadId(_))), "{id:?}");
        }
    }

    #[test]
    fn yaml_round_trip() {
        let pack = RulePack::from_yaml(FULL).unwrap();
        let again = RulePack::from_yaml(&pack.to_yaml().unwrap()).unwrap();
        assert_eq!(pack, again);
    }

    #[test]
    fn loads_from_file_and_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("user.yaml");
        std::fs::write(&path, FULL).unwrap();
        assert_eq!(load_pack_file(&path).unwrap().id, "tr-tech");
        let missing = load_pack_file(&dir.path().join("nope.yaml")).unwrap_err();
        assert!(matches!(missing, PackError::Io(_)));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::pack`
Expected: FAIL — `cannot find type RulePack`.

- [ ] **Step 4: Implement the model**

Insert above the tests in `crates/core/src/rules/pack.rs`:
```rust
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub const PACK_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulePack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(default = "default_language")]
    pub language: String,
    /// Canonical spellings: fed to the prompt and enforced by casing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<String>,
    /// Canonical form → misrecognized variants.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub corrections: IndexMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replacements: Vec<Replacement>,
    /// Whole-output patterns that mean the model hallucinated on silence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hallucinations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub case_sensitive: bool,
    /// `from` is a regex used as-is (no implicit word boundaries); `to` may use `$1`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    #[error("invalid rule pack YAML: {0}")]
    Yaml(String),
    #[error("unsupported rule pack schema {found} (expected {PACK_SCHEMA})")]
    Schema { found: u32 },
    #[error("rule pack id must be lowercase letters, digits or '-': {0:?}")]
    BadId(String),
    #[error("cannot read rule pack: {0}")]
    Io(String),
}

impl RulePack {
    pub fn from_yaml(src: &str) -> Result<Self, PackError> {
        let pack: RulePack = serde_saphyr::from_str(src).map_err(|e| PackError::Yaml(e.to_string()))?;
        if pack.schema != PACK_SCHEMA {
            return Err(PackError::Schema { found: pack.schema });
        }
        let id_ok = !pack.id.is_empty()
            && pack.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(PackError::BadId(pack.id));
        }
        Ok(pack)
    }

    pub fn to_yaml(&self) -> Result<String, PackError> {
        serde_saphyr::to_string(self).map_err(|e| PackError::Yaml(e.to_string()))
    }

    pub fn is_turkish(&self) -> bool {
        self.language == "tr"
    }
}

pub fn load_pack_file(path: &Path) -> Result<RulePack, PackError> {
    let src = std::fs::read_to_string(path).map_err(|e| PackError::Io(format!("{}: {e}", path.display())))?;
    RulePack::from_yaml(&src)
}

fn default_language() -> String {
    "tr".to_string()
}

fn is_false(value: &bool) -> bool {
    !*value
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::pack`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add rule pack YAML model

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Hallucination filter

**Files:**
- Create: `crates/core/src/rules/hallucination.rs`
- Modify: `crates/core/src/rules/mod.rs`

**Interfaces:**
- Consumes: `text::fold` (Task 1).
- Produces (`opit_core::rules::hallucination`):
  ```rust
  pub fn normalize(text: &str) -> String;   // fold, trim surrounding punctuation/space, collapse inner space
  pub struct HallucinationFilter;
  impl HallucinationFilter {
      pub fn new(patterns: impl IntoIterator<Item = String>) -> Self;
      pub fn matches(&self, text: &str) -> bool;   // whole normalized text equals a pattern
  }
  ```

- [ ] **Step 1: Wire the module**

Append to `crates/core/src/rules/mod.rs`:
```rust
pub mod hallucination;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/rules/hallucination.rs`:
```rust
//! Drops outputs that are nothing but a known silence hallucination.

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
        assert_eq!(normalize("  “Altyazı  M.K.”  "), "altyazi m.k");
        assert_eq!(normalize("¿?"), "¿");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::hallucination`
Expected: FAIL — `cannot find function normalize`.

- [ ] **Step 4: Implement**

Insert above the tests:
```rust
use std::collections::HashSet;

use crate::text::fold;

const EDGE_PUNCTUATION: &[char] = &['.', ',', '!', '?', ';', ':', '…', '"', '\'', '“', '”', '‘', '’', '-'];

/// Folds case, trims surrounding punctuation and whitespace, collapses inner whitespace.
pub fn normalize(text: &str) -> String {
    let folded = fold(text);
    let trimmed = folded.trim_matches(|c: char| c.is_whitespace() || EDGE_PUNCTUATION.contains(&c));
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Default, Clone)]
pub struct HallucinationFilter {
    patterns: HashSet<String>,
}

impl HallucinationFilter {
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        let patterns = patterns.into_iter().map(|p| normalize(&p)).filter(|p| !p.is_empty()).collect();
        Self { patterns }
    }

    /// True only when the entire output is a known hallucination.
    pub fn matches(&self, text: &str) -> bool {
        let normalized = normalize(text);
        !normalized.is_empty() && self.patterns.contains(&normalized)
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::hallucination`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): add whole-output hallucination filter

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 5: RuleSet engine

**Files:**
- Create: `crates/core/src/rules/engine.rs`
- Modify: `crates/core/src/rules/mod.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `PhraseMatcher`, `Pattern`, `MatchHit` (Task 2); `RulePack`, `Replacement` (Task 3); `HallucinationFilter` (Task 4); `text::{fold, sentence_start, capitalize_like}` (Task 1).
- Produces (`opit_core::rules::engine`, re-exported from `opit_core::rules`):
  ```rust
  pub enum RuleKind { Correction, Replacement, Casing }            // serde: snake_case
  pub struct RuleRef { pub pack_id: String, pub kind: RuleKind, pub index: usize }
  pub struct RuleHit { pub rule: RuleRef, pub from: String, pub to: String }
  pub struct RuleWarning { pub pack_id: String, pub message: String }
  pub struct RuleSet;
  impl RuleSet {
      pub fn empty() -> Self;
      /// `packs` in priority order: personal pack first.
      pub fn compile(packs: &[RulePack]) -> (Self, Vec<RuleWarning>);
      pub fn apply(&self, text: &str) -> String;
      pub fn apply_traced(&self, text: &str) -> (String, Vec<RuleHit>);
      pub fn is_hallucination(&self, text: &str) -> bool;
      pub fn terms(&self) -> &[String];                              // deduped, priority order
  }
  ```
  `RuleRef::index` is the position of the canonical key in `corrections`, of the item in `replacements`, or of the term in `terms`, inside its pack. Invalid regexes and empty `from` values are skipped with a warning; compile never fails.

- [ ] **Step 1: Add the dependency and wire the module**

In `crates/core/Cargo.toml` `[dependencies]` add:
```toml
regex.workspace = true
```

Replace `crates/core/src/rules/mod.rs` with:
```rust
//! Deterministic accuracy layer: rule packs, matching, prompt building.

pub mod engine;
pub mod hallucination;
pub mod matcher;
pub mod pack;

pub use engine::{RuleHit, RuleKind, RuleRef, RuleSet, RuleWarning};
pub use pack::{PackError, Replacement, RulePack, load_pack_file};
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/rules/engine.rs`:
```rust
//! Compiles rule packs and applies corrections → replacements → term casing.

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
        let rules = compile(&[pack("a", "hallucinations: [altyazı m.k]\n"), pack("b", "hallucinations: [thank you]\n")]);
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
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::engine`
Expected: FAIL — `cannot find type RuleSet`.

- [ ] **Step 4: Implement the engine**

Insert above the tests in `crates/core/src/rules/engine.rs`:
```rust
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
            casing: PhraseMatcher::new(casing),
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::`
Expected: every `rules::` test passes (13 matcher + 7 pack + 4 hallucination + 11 engine).

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add rule engine with corrections, replacements and term casing

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Prompt builder

**Files:**
- Create: `crates/core/src/rules/prompt.rs`
- Modify: `crates/core/src/rules/mod.rs`

**Interfaces:**
- Consumes: nothing beyond std/serde.
- Produces (`opit_core::rules::prompt`):
  ```rust
  pub const PROMPT_TOKEN_BUDGET: usize = 224;
  pub fn estimate_tokens(text: &str) -> usize;     // ceil(utf8 bytes / 3), deliberately conservative
  pub struct BuiltPrompt { pub prompt: Option<String>, pub included_terms: Vec<String>,
                           pub dropped_terms: Vec<String>, pub estimated_tokens: usize }
  pub fn build_prompt(context: &str, terms: &[String], language: &str) -> BuiltPrompt;
  pub fn keywords(terms: &[String]) -> Vec<String>; // terms safe for `keywords[]` (no <, >, CR, LF)
  ```
  The prompt reads `"{context}. Geçen terimler: A, B, C."` when `language == "tr"` and uses `"Terms: …"` otherwise. Terms are added in order until the next one would go over the budget. That term and every term after it are dropped, so the priority order is never changed. A context longer than the budget is cut at a char boundary.

- [ ] **Step 1: Wire the module**

Add to `crates/core/src/rules/mod.rs` (keep the module list alphabetical):
```rust
pub mod prompt;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/rules/prompt.rs`:
```rust
//! Builds the Whisper `prompt` (an example-text hint, not an instruction).

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::prompt`
Expected: FAIL — `cannot find function build_prompt`.

- [ ] **Step 4: Implement**

Insert above the tests:
```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::prompt`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): add Whisper prompt builder with 224-token budget

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Built-in rule packs

**Files:**
- Create: `rules/tr-core.yaml`, `rules/tr-tech.yaml`, `rules/fivem.yaml`
- Create: `crates/core/src/rules/builtin.rs`
- Modify: `crates/core/src/rules/mod.rs`

**Interfaces:**
- Consumes: `RulePack` (Task 3), `RuleSet` (Task 5), `build_prompt` (Task 6).
- Produces (`opit_core::rules::builtin`):
  ```rust
  pub const BUILTIN_PACKS: &[(&str, &str)];               // (id, YAML source), fixed priority order
  pub fn builtin_pack(id: &str) -> Option<RulePack>;
  /// Personal pack first, then enabled built-ins in BUILTIN_PACKS order; unknown ids ignored.
  pub fn assemble(user: Option<RulePack>, enabled: &[String]) -> Vec<RulePack>;
  ```

These packs hold the generic part of the author's old rule set. Personal rules (names, slang, one-off phrases) already live in the author's own `%APPDATA%\opit-speech-to-text\rules\user.yaml` and never enter the repo. Rules that match real words were dropped: `ettik→Et`, `MC→MCP`, `kendindeki→backend'deki`, `terleyip→derleyip`, `Big Sur→bilgisayarım`. `Git` is deliberately not a term, because casing would turn the Turkish word "git" (go) into "Git".

- [ ] **Step 1: Write the packs**

`rules/tr-core.yaml`:
```yaml
schema: 1
id: tr-core
name: Turkish – core spelling
language: tr

corrections:
  hâlâ: [hala]
  pekâlâ: [pekala]
  sağ ol: [sağol]
  sağ olun: [sağolun]
  burada: [burda]
  kaydet: [kayıt et]

# Dropped only when they are the entire output (typical on silence).
hallucinations:
  - altyazı m.k
  - altyazı ekleyen
  - altyazılar m.k
  - abone ol
  - abone olun
  - abone olmayı unutmayın
  - izlediğiniz için teşekkürler
  - m.k
  - mk
  - a
  - e
  - thank you
  - thanks for watching
  - please subscribe
```

`rules/tr-tech.yaml`:
```yaml
schema: 1
id: tr-tech
name: Turkish – software terms
language: tr

# Priority order: earlier terms win the prompt budget.
# `Git` is deliberately absent: casing would turn Turkish "git" (go) into "Git".
terms:
  - Claude Code
  - Claude
  - CLAUDE.md
  - GitHub
  - TypeScript
  - JavaScript
  - React
  - Node.js
  - npm
  - pnpm
  - Vite
  - API
  - JSON
  - SQL
  - WebSocket
  - endpoint
  - frontend
  - backend
  - middleware
  - worktree
  - pull request
  - PR
  - merge
  - deploy
  - plugin
  - subagent
  - LLM
  - MCP
  - MariaDB
  - tmux
  - Ollama
  - read only

corrections:
  Claude Code: [cloud code, clod code, claude kod, klad kod]
  Claude Code'da: [cloud kodda]
  Claude in Chrome: [cloud in chrome]
  CLAUDE.md: [cloud.md, claudemd, claude md, cloud md]
  LLM: [el elem, elelem, el el em]
  MCP: [em si pi, emsipi]
  plugin: [pull win]
  mergele: [mörşle, mörsle, mörge, mergle, merch ile]
  deployla: [diployla, diploğla, diploy ile]
  deploy: [diploy]
  TypeScript: [type script]
  JavaScript: [java script]
  GitHub: [git hub]
  worktree: [work tree]
  pull request: [pul request]
  frontend: [front end]
  backend: [back end]
  endpoint: [end point, en point]
  WebSocket: [web soket, web socket]
  Node.js: [node js, nodejs]
  MariaDB: [maria db]
  memory: [memori]
  tmux: [tmbox]
  Ollama: [olulama]

replacements:
  - { from: worktrillerini, to: "worktree'lerini" }
  - { from: polivin, to: plugin, regex: true }
  - { from: sabecit, to: subagent, regex: true }
  - { from: subagenbt, to: subagent, regex: true }
```

`rules/fivem.yaml`:
```yaml
schema: 1
id: fivem
name: FiveM – server development
language: tr

terms:
  - FiveM
  - Cfx.re
  - Qbox
  - QBCore
  - ox_lib
  - ox_target
  - ox_inventory
  - oxmysql
  - Lua
  - NUI
  - noclip
  - aduty

corrections:
  FiveM: [five em, fife m]
  Cfx.re: [cfx re, c f x re]
  Qbox: [q box, cubox]
  QBCore: [qb core]
  ox_lib: [ox lib, oxlib]
  ox_target: [ox target, oxtarget]
  ox_inventory: [ox inventory]
  oxmysql: [ox mysql]
  NUI: [en yu ay]
  noclip: [no glip]
  noclip'teki: [no glipteki]
  aduty: [aduti]
```

- [ ] **Step 2: Wire the module**

Add to `crates/core/src/rules/mod.rs`:
```rust
pub mod builtin;
```

- [ ] **Step 3: Write the failing tests**

`crates/core/src/rules/builtin.rs`:
```rust
//! Rule packs shipped with the app, embedded at compile time.

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
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p opit-core rules::builtin`
Expected: FAIL — `cannot find value BUILTIN_PACKS`.

- [ ] **Step 5: Implement**

Insert above the tests:
```rust
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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p opit-core rules::builtin`
Expected: 5 passed. If a golden sentence fails, fix the pack YAML rather than the engine, unless the engine contradicts brief §5.3.

- [ ] **Step 7: Commit**

```bash
git add rules crates/core/src
git commit -m "feat(rules): add tr-core, tr-tech and fivem built-in packs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Recording, downmix and resampling

**Files:**
- Create: `crates/core/src/audio/mod.rs`, `crates/core/src/audio/resample.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod audio;`)

**Interfaces:**
- Consumes: nothing.
- Produces (`opit_core::audio`):
  ```rust
  pub const TARGET_RATE: u32 = 16_000;
  pub struct Recording { pub samples: Vec<f32>, pub sample_rate: u32, pub channels: u16 } // interleaved
  impl Recording { pub fn frames(&self) -> usize; pub fn duration_ms(&self) -> u64; }
  pub fn sanitize(sample: f32) -> f32;            // NaN/∞ → 0, clamp to [-1, 1]
  pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32>;   // sanitized mono
  pub fn to_mono_16k(recording: &Recording) -> Vec<f32>;
  // opit_core::audio::resample
  pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32>;
  ```
  The resampler is a windowed-sinc (Blackman) interpolator with a precomputed 256-phase table. Its kernel spans 16 output samples on each side, and its cutoff is 0.95 × the lower Nyquist, so downsampling does not alias.

- [ ] **Step 1: Wire the modules**

Append to `crates/core/src/lib.rs`:
```rust
pub mod audio;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/audio/resample.rs`:
```rust
//! Band-limited sample-rate conversion.

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    /// RMS of the middle half, away from edge effects.
    fn mid_rms(x: &[f32]) -> f32 {
        let mid = &x[x.len() / 4..x.len() * 3 / 4];
        (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt()
    }

    #[test]
    fn same_rate_is_identity() {
        let x = vec![0.1, -0.2, 0.3];
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn output_length_follows_the_ratio() {
        assert_eq!(resample(&vec![0.0; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 44_100], 44_100, 16_000).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 8_000], 8_000, 16_000).len(), 16_000);
    }

    #[test]
    fn passband_tone_keeps_its_level() {
        let expected = 0.5 / 2f32.sqrt();
        for from in [48_000, 44_100, 8_000] {
            let out = resample(&sine(1_000.0, from, 1.0, 0.5), from, 16_000);
            let got = mid_rms(&out);
            assert!((got - expected).abs() / expected < 0.03, "{from}: {got}");
        }
    }

    #[test]
    fn tone_above_new_nyquist_is_removed() {
        let out = resample(&sine(10_000.0, 48_000, 1.0, 0.5), 48_000, 16_000);
        assert!(mid_rms(&out) < 0.01, "{}", mid_rms(&out));
    }
}
```

`crates/core/src/audio/mod.rs`:
```rust
//! Audio preparation: everything between the microphone buffer and the upload.

pub mod resample;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_and_zeroes_non_finite() {
        assert_eq!(sanitize(1.5), 1.0);
        assert_eq!(sanitize(-3.0), -1.0);
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(f32::INFINITY), 0.0);
        assert_eq!(sanitize(0.25), 0.25);
    }

    #[test]
    fn downmix_averages_channels_and_sanitizes() {
        assert_eq!(downmix(&[0.25, 0.75, 1.0, f32::NAN], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[2.0, 0.5], 1), vec![1.0, 0.5]);
    }

    #[test]
    fn duration_uses_frames_not_samples() {
        let rec = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(rec.frames(), 48_000);
        assert_eq!(rec.duration_ms(), 1_000);
        let broken = Recording { samples: vec![0.0; 10], sample_rate: 0, channels: 0 };
        assert_eq!(broken.duration_ms(), 0);
    }

    #[test]
    fn to_mono_16k_converts_stereo_48k() {
        let rec = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(to_mono_16k(&rec).len(), 16_000);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core audio::`
Expected: FAIL — `cannot find function resample` / `sanitize`.

- [ ] **Step 4: Implement the resampler**

Insert above the tests in `crates/core/src/audio/resample.rs`:
```rust
use std::f64::consts::PI;

/// Kernel half-width measured in output samples.
const HALF_WIDTH_OUT: f64 = 16.0;
/// Fractional positions precomputed per kernel.
const PHASES: usize = 256;

/// Converts `input` from `from` Hz to `to` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let scale = (f64::from(to) / f64::from(from)).min(1.0);
    let half = (HALF_WIDTH_OUT / scale).ceil() as usize;
    let taps = 2 * half;
    let table = kernel_table(half, 0.95 * scale);
    let step = f64::from(from) / f64::from(to);
    let out_len = (input.len() as f64 * f64::from(to) / f64::from(from)).floor() as usize;

    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let t = n as f64 * step;
        let base = t.floor();
        let phase = ((t - base) * PHASES as f64).round() as usize;
        let row = &table[phase * taps..(phase + 1) * taps];
        let first = base as isize - half as isize + 1;
        let mut acc = 0.0f32;
        for (k, weight) in row.iter().enumerate() {
            let index = first + k as isize;
            if index >= 0 && (index as usize) < input.len() {
                acc += input[index as usize] * weight;
            }
        }
        out.push(acc);
    }
    out
}

/// Blackman-windowed sinc weights for `PHASES + 1` fractional offsets,
/// each row normalized to unity DC gain.
fn kernel_table(half: usize, cutoff: f64) -> Vec<f32> {
    let taps = 2 * half;
    let width = half as f64;
    let mut table = vec![0.0f32; (PHASES + 1) * taps];
    let mut weights = vec![0.0f64; taps];
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        for (k, weight) in weights.iter_mut().enumerate() {
            let x = k as f64 - width + 1.0 - frac;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { (PI * cutoff * x).sin() / (PI * cutoff * x) };
            let window = if x.abs() >= width {
                0.0
            } else {
                0.42 + 0.5 * (PI * x / width).cos() + 0.08 * (2.0 * PI * x / width).cos()
            };
            *weight = sinc * window;
        }
        let sum: f64 = weights.iter().sum();
        let row = &mut table[phase * taps..(phase + 1) * taps];
        for (dst, weight) in row.iter_mut().zip(&weights) {
            *dst = (weight / sum) as f32;
        }
    }
    table
}
```

- [ ] **Step 5: Implement `Recording` and helpers**

Insert between `pub mod resample;` and the tests in `crates/core/src/audio/mod.rs`:
```rust
/// Sample rate sent to providers.
pub const TARGET_RATE: u32 = 16_000;

/// Raw microphone capture: interleaved `f32` samples at the device rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Recording {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.frames() as u64 * 1000 / u64::from(self.sample_rate)
    }
}

/// Replaces NaN/∞ with silence and clamps to [-1, 1].
pub fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 }
}

/// Averages interleaved channels into sanitized mono.
pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return samples.iter().copied().map(sanitize).collect();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().map(sanitize).sum::<f32>() / channels as f32)
        .collect()
}

pub fn to_mono_16k(recording: &Recording) -> Vec<f32> {
    let mono = downmix(&recording.samples, recording.channels);
    resample::resample(&mono, recording.sample_rate, TARGET_RATE)
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p opit-core audio::`
Expected: 8 passed.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): add recording model, downmix and band-limited resampler

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Silence gate

**Files:**
- Create: `crates/core/src/audio/gate.rs`
- Modify: `crates/core/src/audio/mod.rs` (add `pub mod gate;`)

**Interfaces:**
- Consumes: `audio::TARGET_RATE` (Task 8).
- Produces (`opit_core::audio::gate`):
  ```rust
  pub const FRAME_MS: u64 = 30;
  pub const MIN_RECORDING_MS: u64 = 400;
  pub const MIN_SPEECH_MS: u64 = 300;
  pub enum GateVerdict { Speech { speech_ms: u64 }, TooShort, NoSpeech }
  pub fn rms(samples: &[f32]) -> f32;          // also used by the app's level meter
  pub fn check(samples_16k: &[f32]) -> GateVerdict;
  ```
  A frame counts as speech when its RMS exceeds `clamp(3 × 10th-percentile frame RMS, 0.008, 0.03)`. The noise floor adapts to the room. The 0.03 cap stops a recording that is speech from start to finish from raising its own threshold above itself.

- [ ] **Step 1: Wire the module**

Add to `crates/core/src/audio/mod.rs` next to `pub mod resample;`:
```rust
pub mod gate;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/audio/gate.rs`:
```rust
//! Decides whether a recording contains enough speech to be worth sending.
//! This is the main defence against hallucinations on silence.

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, amp: f32) -> Vec<f32> {
        let n = (16_000.0 * seconds) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin()).collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (16_000.0 * seconds) as usize]
    }

    /// Deterministic uniform noise in [-amp, amp].
    fn noise(seconds: f32, amp: f32) -> Vec<f32> {
        let mut state: u32 = 12_345;
        (0..(16_000.0 * seconds) as usize)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    #[test]
    fn rms_values() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn short_recordings_are_too_short() {
        assert_eq!(check(&tone(0.3, 0.3)), GateVerdict::TooShort);
        assert_eq!(check(&[]), GateVerdict::TooShort);
    }

    #[test]
    fn silence_and_quiet_noise_have_no_speech() {
        assert_eq!(check(&silence(1.0)), GateVerdict::NoSpeech);
        assert_eq!(check(&noise(2.0, 0.004)), GateVerdict::NoSpeech);
    }

    #[test]
    fn steady_fan_noise_has_no_speech() {
        assert_eq!(check(&noise(2.0, 0.026)), GateVerdict::NoSpeech);
    }

    #[test]
    fn continuous_speech_passes() {
        assert!(matches!(check(&tone(1.0, 0.3)), GateVerdict::Speech { speech_ms } if speech_ms >= 900));
    }

    #[test]
    fn quiet_speech_after_silence_passes() {
        let mut samples = silence(1.5);
        samples.extend(tone(0.5, 0.02));
        assert!(matches!(check(&samples), GateVerdict::Speech { .. }));
    }

    #[test]
    fn a_short_burst_is_not_enough() {
        let mut samples = silence(1.8);
        samples.extend(tone(0.2, 0.3));
        assert_eq!(check(&samples), GateVerdict::NoSpeech);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core audio::gate`
Expected: FAIL — `cannot find function check`.

- [ ] **Step 4: Implement**

Insert above the tests:
```rust
use super::TARGET_RATE;

pub const FRAME_MS: u64 = 30;
pub const MIN_RECORDING_MS: u64 = 400;
pub const MIN_SPEECH_MS: u64 = 300;

const MIN_THRESHOLD: f32 = 0.008;
const MAX_THRESHOLD: f32 = 0.03;
const NOISE_FLOOR_FACTOR: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Speech { speech_ms: u64 },
    TooShort,
    NoSpeech,
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Classifies 16 kHz mono samples.
pub fn check(samples_16k: &[f32]) -> GateVerdict {
    let duration_ms = samples_16k.len() as u64 * 1000 / u64::from(TARGET_RATE);
    if duration_ms < MIN_RECORDING_MS {
        return GateVerdict::TooShort;
    }
    let frame_len = (u64::from(TARGET_RATE) * FRAME_MS / 1000) as usize;
    let levels: Vec<f32> = samples_16k.chunks(frame_len).map(rms).collect();
    let threshold = speech_threshold(&levels);
    let speech_ms = levels.iter().filter(|&&level| level > threshold).count() as u64 * FRAME_MS;
    if speech_ms < MIN_SPEECH_MS { GateVerdict::NoSpeech } else { GateVerdict::Speech { speech_ms } }
}

fn speech_threshold(levels: &[f32]) -> f32 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let noise_floor = sorted[sorted.len() / 10];
    (noise_floor * NOISE_FLOOR_FACTOR).clamp(MIN_THRESHOLD, MAX_THRESHOLD)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core audio::gate`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src
git commit -m "feat(core): add adaptive silence gate

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: WAV and FLAC encoding

**Files:**
- Create: `crates/core/src/audio/encode.rs`
- Modify: `crates/core/src/audio/mod.rs` (add `pub mod encode;`), `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `audio::{sanitize, TARGET_RATE}` (Task 8).
- Produces (`opit_core::audio::encode`):
  ```rust
  pub enum AudioFormat { Flac, Wav }                  // serde: "flac" | "wav"
  pub struct EncodedAudio { pub bytes: Vec<u8>, pub format: AudioFormat }
  impl EncodedAudio { pub fn mime(&self) -> &'static str; pub fn file_name(&self) -> &'static str; }
  pub struct EncodeError(pub String);
  pub fn to_pcm16(samples: &[f32]) -> Vec<i16>;
  pub fn encode_wav(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError>;
  pub fn encode_flac(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError>;
  pub fn encode(samples_16k: &[f32], format: AudioFormat) -> Result<EncodedAudio, EncodeError>;
  ```

- [ ] **Step 1: Add dependencies and wire the module**

In `crates/core/Cargo.toml`:
```toml
[dependencies]
flacenc.workspace = true
hound.workspace = true

[dev-dependencies]
claxon.workspace = true
```
(add these lines to the existing sections, keeping them alphabetical)

Add to `crates/core/src/audio/mod.rs`:
```rust
pub mod encode;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/audio/encode.rs`:
```rust
//! 16-bit mono encoders for provider uploads.

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn tone_pcm() -> Vec<i16> {
        let samples: Vec<f32> =
            (0..16_000).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin()).collect();
        to_pcm16(&samples)
    }

    #[test]
    fn pcm16_clamps_and_silences_bad_samples() {
        assert_eq!(to_pcm16(&[2.0, -2.0, f32::NAN, 0.5, 0.0]), vec![32767, -32767, 0, 16384, 0]);
    }

    #[test]
    fn wav_round_trip() {
        let pcm = tone_pcm();
        let wav = encode_wav(&pcm, 16_000).unwrap();
        let mut reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        let decoded: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn flac_round_trip_and_smaller_than_wav() {
        let pcm = tone_pcm();
        let flac = encode_flac(&pcm, 16_000).unwrap();
        assert_eq!(&flac[..4], b"fLaC");
        let mut reader = claxon::FlacReader::new(Cursor::new(&flac)).unwrap();
        let decoded: Vec<i32> = reader.samples().map(Result::unwrap).collect();
        assert_eq!(decoded, pcm.iter().map(|&s| i32::from(s)).collect::<Vec<_>>());
        assert!(flac.len() < encode_wav(&pcm, 16_000).unwrap().len());
    }

    #[test]
    fn encode_sets_format_mime_and_file_name() {
        let wav = encode(&[0.0; 1600], AudioFormat::Wav).unwrap();
        assert_eq!((wav.mime(), wav.file_name()), ("audio/wav", "audio.wav"));
        let flac = encode(&[0.0; 1600], AudioFormat::Flac).unwrap();
        assert_eq!((flac.mime(), flac.file_name()), ("audio/flac", "audio.flac"));
        assert_eq!(flac.format, AudioFormat::Flac);
    }

    #[test]
    fn max_length_recording_fits_the_upload_limit_as_wav() {
        let ten_minutes = vec![0.0f32; 600 * 16_000];
        let wav = encode(&ten_minutes, AudioFormat::Wav).unwrap();
        assert!(wav.bytes.len() < 25 * 1024 * 1024, "{}", wav.bytes.len());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core audio::encode`
Expected: FAIL — `cannot find function to_pcm16`.

- [ ] **Step 4: Implement**

Insert above the tests:
```rust
use std::io::Cursor;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use serde::{Deserialize, Serialize};

use super::{TARGET_RATE, sanitize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    Wav,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAudio {
    pub bytes: Vec<u8>,
    pub format: AudioFormat,
}

impl EncodedAudio {
    pub fn mime(&self) -> &'static str {
        match self.format {
            AudioFormat::Flac => "audio/flac",
            AudioFormat::Wav => "audio/wav",
        }
    }

    pub fn file_name(&self) -> &'static str {
        match self.format {
            AudioFormat::Flac => "audio.flac",
            AudioFormat::Wav => "audio.wav",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("audio encoding failed: {0}")]
pub struct EncodeError(pub String);

pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|&s| (sanitize(s) * f32::from(i16::MAX)).round() as i16).collect()
}

pub fn encode_wav(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError> {
    let spec = hound::WavSpec { channels: 1, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut bytes = Vec::with_capacity(44 + pcm.len() * 2);
    let err = |e: hound::Error| EncodeError(e.to_string());
    let mut writer = hound::WavWriter::new(Cursor::new(&mut bytes), spec).map_err(err)?;
    for &sample in pcm {
        writer.write_sample(sample).map_err(err)?;
    }
    writer.finalize().map_err(err)?;
    Ok(bytes)
}

pub fn encode_flac(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError> {
    let config = flacenc::config::Encoder::default().into_verified().map_err(|e| EncodeError(format!("{e:?}")))?;
    let samples: Vec<i32> = pcm.iter().map(|&s| i32::from(s)).collect();
    let source = flacenc::source::MemSource::from_samples(&samples, 1, 16, sample_rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| EncodeError(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| EncodeError(format!("{e:?}")))?;
    Ok(sink.into_inner())
}

/// Encodes 16 kHz mono samples in `format`.
pub fn encode(samples_16k: &[f32], format: AudioFormat) -> Result<EncodedAudio, EncodeError> {
    let pcm = to_pcm16(samples_16k);
    let bytes = match format {
        AudioFormat::Wav => encode_wav(&pcm, TARGET_RATE)?,
        AudioFormat::Flac => encode_flac(&pcm, TARGET_RATE)?,
    };
    Ok(EncodedAudio { bytes, format })
}
```

Note: the `flacenc` calls were verified against flacenc 0.5.1 on this machine. If `from_samples` rejects `usize` for the rate, pass `sample_rate` without the cast; the compiler error names the expected type.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core audio::encode`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add WAV and FLAC encoders

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Provider profiles and app config

**Files:**
- Create: `crates/core/src/provider/mod.rs`, `crates/core/src/provider/profile.rs`, `crates/core/src/config.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `AudioFormat` (Task 10).
- Produces (`opit_core::provider::profile`, re-exported as `opit_core::provider::{Profile, ResponseFormat, presets}`):
  ```rust
  pub enum ResponseFormat { VerboseJson, Json }                   // serde snake_case; as_str()
  pub struct Profile { pub id: String, pub name: String, pub base_url: String, pub model: String,
      pub api_key_ref: Option<String>, pub language: String, pub audio_format: AudioFormat,
      pub response_format: ResponseFormat, pub send_prompt: bool, pub send_keywords: bool,
      pub apply_rules: bool, pub fallback_profile_id: Option<String> }
  impl Profile { pub fn is_insecure(&self) -> bool; }             // http:// base URL
  pub mod presets { pub const GROQ_ID: &str; pub const OPENAI_ID: &str;
      pub fn groq() -> Profile; pub fn openai() -> Profile;
      pub fn custom(id: &str, name: &str, base_url: &str, model: &str) -> Profile; }
  ```
- Produces (`opit_core::config`):
  ```rust
  pub const APP_DIR_NAME: &str = "opit-speech-to-text";
  pub const CONFIG_SCHEMA_VERSION: u32 = 1;
  pub const MIN_RECORDING_SECONDS: u32 = 10;
  pub const MAX_RECORDING_SECONDS: u32 = 600;
  pub struct AppConfig { pub schema_version: u32, pub ui_language: Option<String>,
      pub active_profile_id: String, pub profiles: Vec<Profile>, pub hotkey: HotkeyConfig,
      pub recording: RecordingConfig, pub paste: PasteConfig, pub history: HistoryConfig,
      pub rules: RulesConfig, pub ui: UiConfig }
  pub struct HotkeyConfig { pub keys: Vec<String>, pub mode: HotkeyMode, pub enabled: bool }
  pub enum HotkeyMode { Toggle, PushToTalk }
  pub struct RecordingConfig { pub microphone: Option<String>, pub max_seconds: u32 }
  pub struct PasteConfig { pub restore_clipboard: bool, pub trailing_space: bool }
  pub struct HistoryConfig { pub enabled: bool, pub save_audio: bool, pub audio_retention_days: u32 }
  pub struct RulesConfig { pub enabled_packs: Vec<String>, pub prompt_context: String }
  pub struct UiConfig { pub start_in_tray: bool, pub autostart: bool,
      pub overlay_position: OverlayPosition, pub sound_feedback: bool }
  pub enum OverlayPosition { RightCenter, TopCenter, BottomCenter, LeftCenter }
  pub enum ConfigError { Json(String), TooNew { found: u32 }, Io(String) }
  impl AppConfig {
      pub fn from_json(src: &str) -> Result<Self, ConfigError>;   // migrate → deserialize → normalize
      pub fn to_json(&self) -> String;
      pub fn load(path: &Path) -> Result<Self, ConfigError>;      // missing file → default
      pub fn save(&self, path: &Path) -> Result<(), ConfigError>; // atomic (tmp + rename)
      pub fn normalize(&mut self);
      pub fn profile(&self, id: &str) -> Option<&Profile>;
      pub fn active_profile(&self) -> Option<&Profile>;
      pub fn fallback_for(&self, profile: &Profile) -> Option<&Profile>;
  }
  ```
  Defaults: profiles `[groq, openai]`, active `groq`, hotkey `["RightCtrl", "RightShift"]` in toggle mode, max 180 s, restore clipboard on, trailing space on, history on, audio off, 30-day retention, packs `["tr-core", "tr-tech"]`, empty prompt context, start in tray off, autostart on, overlay right-center, sound feedback on. The transcription language lives on each profile (default `tr`).

- [ ] **Step 1: Add dependencies and wire the modules**

In `crates/core/Cargo.toml` `[dependencies]` add:
```toml
serde_json.workspace = true
```

`crates/core/src/provider/mod.rs`:
```rust
//! Speech-to-text providers. v1 has one adapter: OpenAI-compatible HTTP.

pub mod profile;

pub use profile::{Profile, ResponseFormat, presets};
```

Append to `crates/core/src/lib.rs`:
```rust
pub mod config;
pub mod provider;
```

- [ ] **Step 2: Write the failing profile tests**

`crates/core/src/provider/profile.rs`:
```rust
//! Provider profiles and built-in presets.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groq_preset_matches_the_brief() {
        let p = presets::groq();
        assert_eq!(p.id, "groq");
        assert_eq!(p.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(p.model, "whisper-large-v3");
        assert_eq!(p.audio_format, AudioFormat::Flac);
        assert_eq!(p.response_format, ResponseFormat::VerboseJson);
        assert!(p.send_prompt && !p.send_keywords && p.apply_rules);
        assert_eq!(p.api_key_ref.as_deref(), Some("groq"));
        assert_eq!(p.language, "tr");
    }

    #[test]
    fn openai_preset_sends_keywords_and_plain_json() {
        let p = presets::openai();
        assert_eq!(p.base_url, "https://api.openai.com/v1");
        assert_eq!(p.model, "gpt-transcribe");
        assert_eq!(p.audio_format, AudioFormat::Wav);
        assert_eq!(p.response_format, ResponseFormat::Json);
        assert!(p.send_prompt && p.send_keywords);
    }

    #[test]
    fn custom_preset_is_conservative() {
        let p = presets::custom("gpu", "GPU box", "http://10.0.0.2:8888/v1", "large-v3");
        assert_eq!(p.audio_format, AudioFormat::Wav);
        assert_eq!(p.response_format, ResponseFormat::Json);
        assert!(p.send_prompt && !p.send_keywords && p.apply_rules);
        assert!(p.is_insecure());
        assert!(!presets::groq().is_insecure());
    }

    #[test]
    fn serializes_with_snake_case_enums() {
        let json = serde_json::to_value(presets::groq()).unwrap();
        assert_eq!(json["audio_format"], "flac");
        assert_eq!(json["response_format"], "verbose_json");
        let back: Profile = serde_json::from_value(json).unwrap();
        assert_eq!(back, presets::groq());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core provider::profile`
Expected: FAIL — `cannot find module presets` / `cannot find type Profile` (and `config` not found, since `config.rs` doesn't exist yet: create it as an empty file with only `//! Application settings.` so the crate compiles).

- [ ] **Step 4: Implement profiles**

Insert above the tests in `crates/core/src/provider/profile.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::audio::encode::AudioFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    VerboseJson,
    Json,
}

impl ResponseFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseFormat::VerboseJson => "verbose_json",
            ResponseFormat::Json => "json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// Credential Manager entry that holds the API key; `None` means no key (local servers).
    #[serde(default)]
    pub api_key_ref: Option<String>,
    /// ISO-639-1 transcription language, or `auto`.
    #[serde(default = "default_language")]
    pub language: String,
    pub audio_format: AudioFormat,
    pub response_format: ResponseFormat,
    pub send_prompt: bool,
    pub send_keywords: bool,
    /// Apply the client-side rule layer; off when the server applies its own rules.
    pub apply_rules: bool,
    #[serde(default)]
    pub fallback_profile_id: Option<String>,
}

impl Profile {
    pub fn is_insecure(&self) -> bool {
        self.base_url.trim_start().to_ascii_lowercase().starts_with("http://")
    }
}

fn default_language() -> String {
    "tr".to_string()
}

pub mod presets {
    use super::{AudioFormat, Profile, ResponseFormat};

    pub const GROQ_ID: &str = "groq";
    pub const OPENAI_ID: &str = "openai";

    pub fn groq() -> Profile {
        Profile {
            id: GROQ_ID.into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            model: "whisper-large-v3".into(),
            api_key_ref: Some(GROQ_ID.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Flac,
            response_format: ResponseFormat::VerboseJson,
            send_prompt: true,
            send_keywords: false,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }

    pub fn openai() -> Profile {
        Profile {
            id: OPENAI_ID.into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-transcribe".into(),
            api_key_ref: Some(OPENAI_ID.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Wav,
            response_format: ResponseFormat::Json,
            send_prompt: true,
            send_keywords: true,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }

    /// A user-defined OpenAI-compatible server. Plain JSON is the safe default;
    /// the user can switch to verbose_json if the server supports it.
    pub fn custom(id: &str, name: &str, base_url: &str, model: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key_ref: Some(id.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Wav,
            response_format: ResponseFormat::Json,
            send_prompt: true,
            send_keywords: false,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }
}
```

- [ ] **Step 5: Run the profile tests to verify they pass**

Run: `cargo test -p opit-core provider::profile`
Expected: 4 passed.

- [ ] **Step 6: Write the failing config tests**

Replace `crates/core/src/config.rs` with:
```rust
//! Application settings (`config.json`), with defaults, validation and schema migration.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::presets;

    #[test]
    fn defaults_match_the_brief() {
        let c = AppConfig::default();
        assert_eq!(c.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(c.active_profile_id, "groq");
        assert_eq!(c.profiles, vec![presets::groq(), presets::openai()]);
        assert_eq!(c.hotkey.keys, ["RightCtrl", "RightShift"]);
        assert_eq!(c.hotkey.mode, HotkeyMode::Toggle);
        assert_eq!(c.recording.max_seconds, 180);
        assert!(c.paste.restore_clipboard && c.paste.trailing_space);
        assert!(c.history.enabled && !c.history.save_audio);
        assert_eq!(c.history.audio_retention_days, 30);
        assert_eq!(c.rules.enabled_packs, ["tr-core", "tr-tech"]);
        assert!(!c.ui.start_in_tray && c.ui.autostart && c.ui.sound_feedback);
        assert_eq!(c.ui.overlay_position, OverlayPosition::RightCenter);
    }

    #[test]
    fn json_round_trip() {
        let c = AppConfig::default();
        assert_eq!(AppConfig::from_json(&c.to_json()).unwrap(), c);
    }

    #[test]
    fn partial_json_fills_defaults_and_ignores_unknown_fields() {
        let c = AppConfig::from_json(r#"{"schema_version":1,"paste":{"trailing_space":false},"future":true}"#).unwrap();
        assert!(!c.paste.trailing_space);
        assert!(c.paste.restore_clipboard);
        assert_eq!(c.profiles.len(), 2);
    }

    #[test]
    fn unversioned_files_are_migrated() {
        let c = AppConfig::from_json(r#"{"recording":{"max_seconds":60}}"#).unwrap();
        assert_eq!(c.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(c.recording.max_seconds, 60);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let err = AppConfig::from_json(r#"{"schema_version":99}"#).unwrap_err();
        assert!(matches!(err, ConfigError::TooNew { found: 99 }));
    }

    #[test]
    fn garbage_is_a_json_error() {
        assert!(matches!(AppConfig::from_json("{nope"), Err(ConfigError::Json(_))));
        assert!(matches!(AppConfig::from_json("[1,2]"), Err(ConfigError::Json(_))));
    }

    #[test]
    fn normalize_repairs_invalid_values() {
        let mut c = AppConfig::default();
        c.recording.max_seconds = 5_000;
        c.active_profile_id = "missing".into();
        c.profiles[0].fallback_profile_id = Some("groq".into());
        c.profiles[1].fallback_profile_id = Some("ghost".into());
        c.profiles.push(presets::groq());
        c.normalize();
        assert_eq!(c.recording.max_seconds, MAX_RECORDING_SECONDS);
        assert_eq!(c.active_profile_id, "groq");
        assert_eq!(c.profiles.len(), 2, "duplicate id removed");
        assert!(c.profiles.iter().all(|p| p.fallback_profile_id.is_none()));

        c.recording.max_seconds = 0;
        c.profiles.clear();
        c.normalize();
        assert_eq!(c.recording.max_seconds, MIN_RECORDING_SECONDS);
        assert_eq!(c.profiles, vec![presets::groq()]);
    }

    #[test]
    fn active_and_fallback_lookup() {
        let mut c = AppConfig::default();
        c.profiles[0].fallback_profile_id = Some("openai".into());
        let active = c.active_profile().unwrap();
        assert_eq!(active.id, "groq");
        assert_eq!(c.fallback_for(active).unwrap().id, "openai");
        assert!(c.fallback_for(&c.profiles[1]).is_none());
    }

    #[test]
    fn load_missing_file_gives_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        assert_eq!(AppConfig::load(&path).unwrap(), AppConfig::default());
        let mut c = AppConfig::default();
        c.rules.prompt_context = "FiveM üzerine konuşma.".into();
        c.save(&path).unwrap();
        c.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), c);
    }
}
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cargo test -p opit-core config::`
Expected: FAIL — `cannot find type AppConfig`.

- [ ] **Step 8: Implement the config**

Insert above the tests in `crates/core/src/config.rs`:
```rust
use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::{Profile, presets};

pub const APP_DIR_NAME: &str = "opit-speech-to-text";
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const MIN_RECORDING_SECONDS: u32 = 10;
pub const MAX_RECORDING_SECONDS: u32 = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    /// `en`, `tr`, or `None` to follow the system language.
    pub ui_language: Option<String>,
    pub active_profile_id: String,
    pub profiles: Vec<Profile>,
    pub hotkey: HotkeyConfig,
    pub recording: RecordingConfig,
    pub paste: PasteConfig,
    pub history: HistoryConfig,
    pub rules: RulesConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            ui_language: None,
            active_profile_id: presets::GROQ_ID.into(),
            profiles: vec![presets::groq(), presets::openai()],
            hotkey: HotkeyConfig::default(),
            recording: RecordingConfig::default(),
            paste: PasteConfig::default(),
            history: HistoryConfig::default(),
            rules: RulesConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    Toggle,
    PushToTalk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// Key names understood by the app's hotkey hook, e.g. `RightCtrl`.
    pub keys: Vec<String>,
    pub mode: HotkeyMode,
    pub enabled: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { keys: vec!["RightCtrl".into(), "RightShift".into()], mode: HotkeyMode::Toggle, enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Device name; `None` = system default.
    pub microphone: Option<String>,
    pub max_seconds: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self { microphone: None, max_seconds: 180 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PasteConfig {
    pub restore_clipboard: bool,
    pub trailing_space: bool,
}

impl Default for PasteConfig {
    fn default() -> Self {
        Self { restore_clipboard: true, trailing_space: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub save_audio: bool,
    pub audio_retention_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { enabled: true, save_audio: false, audio_retention_days: 30 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub enabled_packs: Vec<String>,
    /// Short context sentence placed at the start of the Whisper prompt.
    pub prompt_context: String,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self { enabled_packs: vec!["tr-core".into(), "tr-tech".into()], prompt_context: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPosition {
    RightCenter,
    TopCenter,
    BottomCenter,
    LeftCenter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub start_in_tray: bool,
    pub autostart: bool,
    pub overlay_position: OverlayPosition,
    pub sound_feedback: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            start_in_tray: false,
            autostart: true,
            overlay_position: OverlayPosition::RightCenter,
            sound_feedback: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("config is not valid: {0}")]
    Json(String),
    #[error("config was written by a newer version (schema {found})")]
    TooNew { found: u32 },
    #[error("config file I/O failed: {0}")]
    Io(String),
}

impl AppConfig {
    pub fn from_json(src: &str) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(src).map_err(|e| ConfigError::Json(e.to_string()))?;
        let value = migrate(value)?;
        let mut config: AppConfig = serde_json::from_value(value).map_err(|e| ConfigError::Json(e.to_string()))?;
        config.normalize();
        Ok(config)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("config always serializes")
    }

    /// Loads `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(src) => Self::from_json(&src),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Io(e.to_string())),
        }
    }

    /// Writes atomically: a temp file next to `path`, then a rename over it.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let io = |e: std::io::Error| ConfigError::Io(e.to_string());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// Repairs values a hand edit or an old version could leave inconsistent.
    pub fn normalize(&mut self) {
        self.schema_version = CONFIG_SCHEMA_VERSION;
        self.recording.max_seconds = self.recording.max_seconds.clamp(MIN_RECORDING_SECONDS, MAX_RECORDING_SECONDS);

        let mut seen = HashSet::new();
        self.profiles.retain(|p| seen.insert(p.id.clone()));
        if self.profiles.is_empty() {
            self.profiles.push(presets::groq());
        }
        if self.profile(&self.active_profile_id).is_none() {
            self.active_profile_id = self.profiles[0].id.clone();
        }
        for profile in &mut self.profiles {
            let valid = profile.fallback_profile_id.as_ref().is_some_and(|f| f != &profile.id && seen.contains(f));
            if !valid {
                profile.fallback_profile_id = None;
            }
        }
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profile(&self.active_profile_id)
    }

    pub fn fallback_for(&self, profile: &Profile) -> Option<&Profile> {
        profile.fallback_profile_id.as_deref().and_then(|id| self.profile(id))
    }
}

fn migrate(mut value: Value) -> Result<Value, ConfigError> {
    let object = value.as_object_mut().ok_or_else(|| ConfigError::Json("config root must be an object".into()))?;
    let version = object.get("schema_version").and_then(Value::as_u64).unwrap_or(0);
    if version > u64::from(CONFIG_SCHEMA_VERSION) {
        return Err(ConfigError::TooNew { found: version as u32 });
    }
    // v0 (unversioned pre-release files) has the same shape as v1.
    object.insert("schema_version".into(), Value::from(CONFIG_SCHEMA_VERSION));
    Ok(value)
}
```

Note on `normalize`: `seen` still holds the ids of every retained profile when the fallback loop runs, so it doubles as the set of valid fallback targets. The `profiles.clear()` case adds `groq` after `seen` was built. That is harmless because a lone profile can't have a fallback.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test -p opit-core config:: provider::profile`
Expected: 9 config tests + 4 profile tests pass.

- [ ] **Step 10: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add provider profiles and versioned app config

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: OpenAI-compatible provider client

**Files:**
- Create: `crates/core/src/provider/openai.rs`
- Modify: `crates/core/src/provider/mod.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `Profile`, `ResponseFormat` (Task 11); `EncodedAudio`, `encode`, `AudioFormat` (Task 10).
- Produces (`opit_core::provider`):
  ```rust
  pub struct TranscribeRequest<'a> { pub audio: &'a EncodedAudio, pub audio_ms: u64,
                                     pub prompt: Option<&'a str>, pub keywords: &'a [String] }
  pub struct RawTranscript { pub text: String, pub dropped_segments: usize }
  pub enum ProviderError { Unauthorized(u16), PayloadTooLarge, RateLimited, Server(u16),
                           Http { status: u16, message: String }, Network(String), Timeout, BadResponse(String) }
  impl ProviderError { pub fn is_retryable(&self) -> bool; }   // RateLimited | Server | Network | Timeout
  pub trait Transcriber {
      fn profile(&self) -> &Profile;
      fn transcribe(&self, request: &TranscribeRequest<'_>)
          -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send;
  }
  // opit_core::provider::openai
  pub const CONNECT_TIMEOUT: Duration;          // 5 s
  pub const BASE_REQUEST_TIMEOUT: Duration;     // 30 s
  pub fn request_timeout(base: Duration, audio_ms: u64) -> Duration;   // base + audio_ms / 4
  pub struct OpenAiCompatible;
  impl OpenAiCompatible {
      pub fn new(profile: Profile, api_key: Option<String>) -> Result<Self, ProviderError>;
      pub fn with_base_timeout(self, base: Duration) -> Self;
      pub fn endpoint(&self) -> String;
      /// "Test connection" (brief §6): GET {base_url}/models; if that answers neither
      /// 2xx nor 401/403, a 1 s silent transcription instead.
      pub async fn test_connection(&self) -> Result<(), ProviderError>;
  }
  impl Transcriber for OpenAiCompatible;
  ```
  Multipart fields: `file` (with file name and MIME), `model`, `temperature=0`, `response_format`, `language` (skipped when empty or `auto`), `prompt` (only when `send_prompt` and non-empty), and one `keywords[]` per keyword (only when `send_keywords`). Error messages never include the API key or transcript text.

- [ ] **Step 1: Add dependencies and wire the module**

In `crates/core/Cargo.toml`:
```toml
[dependencies]
reqwest.workspace = true
tokio.workspace = true

[dev-dependencies]
wiremock.workspace = true
```
(add to the existing sections)

Replace `crates/core/src/provider/mod.rs` with:
```rust
//! Speech-to-text providers. v1 has one adapter: OpenAI-compatible HTTP.

use crate::audio::encode::EncodedAudio;

pub mod openai;
pub mod profile;

pub use openai::OpenAiCompatible;
pub use profile::{Profile, ResponseFormat, presets};

pub struct TranscribeRequest<'a> {
    pub audio: &'a EncodedAudio,
    /// Length of the audio; stretches the request timeout.
    pub audio_ms: u64,
    pub prompt: Option<&'a str>,
    pub keywords: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTranscript {
    pub text: String,
    /// verbose_json segments dropped as probable silence.
    pub dropped_segments: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("the API key was rejected (HTTP {0})")]
    Unauthorized(u16),
    #[error("the recording exceeds the provider's upload limit")]
    PayloadTooLarge,
    #[error("the provider is rate limiting requests")]
    RateLimited,
    #[error("the provider had a server error (HTTP {0})")]
    Server(u16),
    #[error("the provider refused the request (HTTP {status}): {message}")]
    Http { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("the request timed out")]
    Timeout,
    #[error("the provider sent an unreadable response: {0}")]
    BadResponse(String),
}

impl ProviderError {
    /// Worth one retry and, after that, the fallback profile.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited | ProviderError::Server(_) | ProviderError::Network(_) | ProviderError::Timeout
        )
    }
}

pub trait Transcriber {
    fn profile(&self) -> &Profile;

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send;
}
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/provider/openai.rs`:
```rust
//! Client for any server that implements OpenAI's `/audio/transcriptions`.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::audio::encode::{AudioFormat, encode};
    use crate::provider::presets;

    const PATH: &str = "/v1/audio/transcriptions";

    async fn server(status: u16, body: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        server
    }

    fn profile_for(server: &MockServer) -> Profile {
        let mut profile = presets::groq();
        profile.base_url = format!("{}/v1/", server.uri());
        profile
    }

    fn wav() -> EncodedAudio {
        encode(&[0.0; 16_000], AudioFormat::Wav).unwrap()
    }

    async fn send(profile: Profile, key: Option<&str>, prompt: Option<&str>, keywords: &[String]) -> Result<RawTranscript, ProviderError> {
        let audio = wav();
        let client = OpenAiCompatible::new(profile, key.map(str::to_string)).unwrap();
        client.transcribe(&TranscribeRequest { audio: &audio, audio_ms: 1_000, prompt, keywords }).await
    }

    /// Values of every multipart field called `name`.
    fn fields(body: &str, name: &str) -> Vec<String> {
        let marker = format!("name=\"{name}\"");
        body.match_indices(&marker)
            .map(|(at, _)| {
                let rest = &body[at..];
                let start = rest.find("\r\n\r\n").unwrap() + 4;
                let end = rest[start..].find("\r\n").unwrap();
                rest[start..start + end].to_string()
            })
            .collect()
    }

    async fn only_request(server: &MockServer) -> wiremock::Request {
        let mut requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        requests.remove(0)
    }

    #[tokio::test]
    async fn sends_the_expected_multipart_form() {
        let server = server(200, r#"{"text":" Merhaba dünya "}"#).await;
        let keywords = vec!["Claude Code".to_string(), "ox_lib".to_string()];
        let mut profile = profile_for(&server);
        profile.send_keywords = true;
        let raw = send(profile, Some("sk-test"), Some("Bağlam."), &keywords).await.unwrap();
        assert_eq!(raw, RawTranscript { text: "Merhaba dünya".into(), dropped_segments: 0 });

        let request = only_request(&server).await;
        assert_eq!(request.headers.get("authorization").unwrap(), "Bearer sk-test");
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert_eq!(fields(&body, "model"), ["whisper-large-v3"]);
        assert_eq!(fields(&body, "temperature"), ["0"]);
        assert_eq!(fields(&body, "response_format"), ["verbose_json"]);
        assert_eq!(fields(&body, "language"), ["tr"]);
        assert_eq!(fields(&body, "prompt"), ["Bağlam."]);
        assert_eq!(fields(&body, "keywords[]"), ["Claude Code", "ox_lib"]);
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.to_ascii_lowercase().contains("content-type: audio/wav"));
    }

    #[tokio::test]
    async fn optional_fields_are_left_out() {
        let server = server(200, r#"{"text":"ok"}"#).await;
        let mut profile = profile_for(&server);
        profile.language = "auto".into();
        profile.send_prompt = false;
        let keywords = vec!["GitHub".to_string()];
        send(profile, None, Some("Bağlam."), &keywords).await.unwrap();

        let request = only_request(&server).await;
        assert!(request.headers.get("authorization").is_none());
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert!(fields(&body, "language").is_empty());
        assert!(fields(&body, "prompt").is_empty());
        assert!(fields(&body, "keywords[]").is_empty(), "groq preset has send_keywords off");
    }

    #[tokio::test]
    async fn verbose_json_drops_silent_segments() {
        let body = r#"{"text":"Merhaba altyazı m.k","segments":[
            {"text":" Merhaba","no_speech_prob":0.1,"avg_logprob":-0.2},
            {"text":" altyazı m.k","no_speech_prob":0.9,"avg_logprob":-1.5}]}"#;
        let server = server(200, body).await;
        let raw = send(profile_for(&server), None, None, &[]).await.unwrap();
        assert_eq!(raw, RawTranscript { text: "Merhaba".into(), dropped_segments: 1 });
    }

    #[tokio::test]
    async fn uncertain_but_speechy_segments_are_kept() {
        let body = r#"{"text":"a b","segments":[
            {"text":"a","no_speech_prob":0.9,"avg_logprob":-0.5},
            {"text":"b","no_speech_prob":0.2,"avg_logprob":-1.5}]}"#;
        let server = server(200, body).await;
        let raw = send(profile_for(&server), None, None, &[]).await.unwrap();
        assert_eq!(raw.text, "a b");
    }

    #[tokio::test]
    async fn http_statuses_map_to_typed_errors() {
        let cases = [
            (401, "{}", ProviderError::Unauthorized(401)),
            (403, "{}", ProviderError::Unauthorized(403)),
            (413, "{}", ProviderError::PayloadTooLarge),
            (429, "{}", ProviderError::RateLimited),
            (503, "<html>Service Unavailable</html>", ProviderError::Server(503)),
            (
                400,
                r#"{"error":{"message":"model not found"}}"#,
                ProviderError::Http { status: 400, message: "model not found".into() },
            ),
        ];
        for (status, body, expected) in cases {
            let server = server(status, body).await;
            assert_eq!(send(profile_for(&server), None, None, &[]).await.unwrap_err(), expected, "HTTP {status}");
        }
    }

    #[tokio::test]
    async fn non_json_success_is_a_bad_response() {
        let server = server(200, "<html>captive portal</html>").await;
        let err = send(profile_for(&server), None, None, &[]).await.unwrap_err();
        assert!(matches!(err, ProviderError::BadResponse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn slow_server_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}").set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;
        let audio = wav();
        let client =
            OpenAiCompatible::new(profile_for(&server), None).unwrap().with_base_timeout(Duration::from_millis(200));
        let request = TranscribeRequest { audio: &audio, audio_ms: 0, prompt: None, keywords: &[] };
        assert_eq!(client.transcribe(&request).await.unwrap_err(), ProviderError::Timeout);
    }

    #[tokio::test]
    async fn closed_port_is_a_network_error() {
        // Bind and immediately release a port so nothing listens on it.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut profile = presets::groq();
        profile.base_url = format!("http://127.0.0.1:{port}/v1");
        let err = send(profile, None, None, &[]).await.unwrap_err();
        assert!(matches!(err, ProviderError::Network(_)), "{err:?}");
    }

    #[test]
    fn timeout_grows_with_audio_length() {
        assert_eq!(request_timeout(BASE_REQUEST_TIMEOUT, 8_000), Duration::from_secs(32));
    }

    #[test]
    fn endpoint_joins_without_double_slash() {
        let mut profile = presets::groq();
        profile.base_url = "https://example.com/v1/".into();
        assert_eq!(OpenAiCompatible::new(profile, None).unwrap().endpoint(), "https://example.com/v1/audio/transcriptions");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core provider::openai`
Expected: FAIL — `cannot find type OpenAiCompatible`.

- [ ] **Step 4: Implement the client**

Insert above the tests:
```rust
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use super::{Profile, ProviderError, RawTranscript, ResponseFormat, TranscribeRequest, Transcriber};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const BASE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const NO_SPEECH_PROB_LIMIT: f64 = 0.6;
const AVG_LOGPROB_LIMIT: f64 = -1.0;

pub fn request_timeout(base: Duration, audio_ms: u64) -> Duration {
    base + Duration::from_millis(audio_ms / 4)
}

pub struct OpenAiCompatible {
    client: reqwest::Client,
    profile: Profile,
    api_key: Option<String>,
    base_timeout: Duration,
}

impl OpenAiCompatible {
    pub fn new(profile: Profile, api_key: Option<String>) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let api_key = api_key.filter(|k| !k.trim().is_empty());
        Ok(Self { client, profile, api_key, base_timeout: BASE_REQUEST_TIMEOUT })
    }

    /// Overrides the 30 s base timeout (tests, slow self-hosted servers).
    pub fn with_base_timeout(mut self, base: Duration) -> Self {
        self.base_timeout = base;
        self
    }

    pub fn endpoint(&self) -> String {
        format!("{}/audio/transcriptions", self.profile.base_url.trim().trim_end_matches('/'))
    }

    fn form(&self, request: &TranscribeRequest<'_>) -> Form {
        let audio = request.audio;
        let file = Part::bytes(audio.bytes.clone())
            .file_name(audio.file_name())
            .mime_str(audio.mime())
            .expect("static MIME types are valid");
        let mut form = Form::new()
            .part("file", file)
            .text("model", self.profile.model.clone())
            .text("temperature", "0")
            .text("response_format", self.profile.response_format.as_str());
        let language = self.profile.language.trim();
        if !language.is_empty() && language != "auto" {
            form = form.text("language", language.to_string());
        }
        if self.profile.send_prompt
            && let Some(prompt) = request.prompt.filter(|p| !p.trim().is_empty())
        {
            form = form.text("prompt", prompt.to_string());
        }
        if self.profile.send_keywords {
            for keyword in request.keywords {
                form = form.text("keywords[]", keyword.clone());
            }
        }
        form
    }
}

impl Transcriber for OpenAiCompatible {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        let mut builder = self
            .client
            .post(self.endpoint())
            .timeout(request_timeout(self.base_timeout, request.audio_ms))
            .multipart(self.form(request));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        let verbose = self.profile.response_format == ResponseFormat::VerboseJson;
        async move {
            let response = builder.send().await.map_err(send_error)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(send_error)?;
            if !(200..300).contains(&status) {
                return Err(status_error(status, &body));
            }
            parse_body(&body, verbose)
        }
    }
}

fn send_error(err: reqwest::Error) -> ProviderError {
    if err.is_timeout() { ProviderError::Timeout } else { ProviderError::Network(err.to_string()) }
}

fn status_error(status: u16, body: &str) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Unauthorized(status),
        413 => ProviderError::PayloadTooLarge,
        429 => ProviderError::RateLimited,
        500..=599 => ProviderError::Server(status),
        _ => ProviderError::Http { status, message: error_message(body) },
    }
}

/// OpenAI-style `{"error":{"message":…}}`, else the first 200 chars of the body.
fn error_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct Envelope {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        message: String,
    }
    serde_json::from_str::<Envelope>(body)
        .map(|e| e.error.message)
        .unwrap_or_else(|_| body.chars().take(200).collect())
}

#[derive(Deserialize)]
struct ApiResponse {
    text: String,
    #[serde(default)]
    segments: Option<Vec<Segment>>,
}

#[derive(Deserialize)]
struct Segment {
    #[serde(default)]
    text: String,
    #[serde(default)]
    no_speech_prob: f64,
    #[serde(default)]
    avg_logprob: f64,
}

fn parse_body(body: &str, verbose: bool) -> Result<RawTranscript, ProviderError> {
    let response: ApiResponse =
        serde_json::from_str(body).map_err(|e| ProviderError::BadResponse(e.to_string()))?;
    let segments = if verbose { response.segments.unwrap_or_default() } else { Vec::new() };
    let is_silence = |s: &Segment| s.no_speech_prob > NO_SPEECH_PROB_LIMIT && s.avg_logprob < AVG_LOGPROB_LIMIT;
    let dropped = segments.iter().filter(|s| is_silence(s)).count();
    if dropped == 0 {
        return Ok(RawTranscript { text: response.text.trim().to_string(), dropped_segments: 0 });
    }
    let text = segments
        .iter()
        .filter(|s| !is_silence(s))
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(RawTranscript { text, dropped_segments: dropped })
}
```

The body is read as text before JSON parsing, so HTML error pages from proxies map by status code, and a non-JSON 200 becomes `BadResponse`. `reqwest::Error`'s display includes the URL but never headers, so the key cannot leak through `Network(..)`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-core provider::`
Expected: 10 openai tests + 4 profile tests pass. `closed_port_is_a_network_error` can take about 2 s on Windows, which retries a refused localhost connection.

- [ ] **Step 6: Write the failing connection-test tests**

Add to the `tests` module in `crates/core/src/provider/openai.rs`:
```rust
    async fn connection_server(models_status: u16, transcription_status: u16) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(models_status).set_body_string(r#"{"data":[]}"#))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(PATH))
            .respond_with(ResponseTemplate::new(transcription_status).set_body_string(r#"{"text":""}"#))
            .mount(&server)
            .await;
        server
    }

    async fn test_connection_with(server: &MockServer) -> Result<(), ProviderError> {
        OpenAiCompatible::new(profile_for(server), Some("k".into())).unwrap().test_connection().await
    }

    #[tokio::test]
    async fn connection_ok_via_models_without_transcribing() {
        let server = connection_server(200, 500).await;
        assert_eq!(test_connection_with(&server).await, Ok(()));
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| r.method.as_str() == "GET"));
    }

    #[tokio::test]
    async fn connection_reports_a_rejected_key_from_models() {
        let server = connection_server(401, 200).await;
        assert_eq!(test_connection_with(&server).await, Err(ProviderError::Unauthorized(401)));
    }

    #[tokio::test]
    async fn connection_falls_back_to_a_silent_transcription() {
        let server = connection_server(404, 200).await;
        assert_eq!(test_connection_with(&server).await, Ok(()));
        let failing = connection_server(404, 500).await;
        assert_eq!(test_connection_with(&failing).await, Err(ProviderError::Server(500)));
    }
```

Run: `cargo test -p opit-core provider::openai::tests::connection`
Expected: FAIL — `no method named test_connection`.

- [ ] **Step 7: Implement `test_connection`**

Add to `impl OpenAiCompatible` in `crates/core/src/provider/openai.rs`:
```rust
    /// Checks the base URL and key with `GET {base_url}/models`. Servers without
    /// that endpoint get a 1 s silent transcription instead.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let url = format!("{}/models", self.profile.base_url.trim().trim_end_matches('/'));
        let mut request = self.client.get(url).timeout(Duration::from_secs(10));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        if let Ok(response) = request.send().await {
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(());
            }
            if matches!(status, 401 | 403) {
                return Err(ProviderError::Unauthorized(status));
            }
        }
        let silence = encode(&[0.0; 16_000], self.profile.audio_format)
            .map_err(|e| ProviderError::BadResponse(e.to_string()))?;
        let request = TranscribeRequest { audio: &silence, audio_ms: 1_000, prompt: None, keywords: &[] };
        self.transcribe(&request).await.map(|_| ())
    }
```
and add this import at the top of the file:
```rust
use crate::audio::encode::encode;
```

- [ ] **Step 8: Run the provider tests**

Run: `cargo test -p opit-core provider::`
Expected: 14 openai tests + 4 profile tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add OpenAI-compatible transcription client

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Pipeline with retry and fallback

**Files:**
- Create: `crates/core/src/provider/retry.rs`, `crates/core/src/pipeline.rs`, `crates/core/tests/pipeline_http.rs`
- Modify: `crates/core/src/provider/mod.rs` (add `pub mod retry;`), `crates/core/src/lib.rs` (add `pub mod pipeline;`)

**Interfaces:**
- Consumes: `Recording`, `to_mono_16k`, `TARGET_RATE` (Task 8); `gate::check` (Task 9); `encode`, `EncodeError`, `EncodedAudio` (Task 10); `Profile` (Task 11); `Transcriber`, `TranscribeRequest`, `RawTranscript`, `ProviderError`, `OpenAiCompatible` (Task 12); `RuleSet`, `RuleHit` (Task 5); `build_prompt`, `keywords` (Task 6).
- Produces (`opit_core::provider::retry`):
  ```rust
  pub const RETRY_DELAY: Duration;   // 500 ms
  pub async fn with_retry<T, F, Fut>(delay: Duration, attempt: F) -> Result<T, ProviderError>
  where F: FnMut() -> Fut, Fut: Future<Output = Result<T, ProviderError>>;
  ```
- Produces (`opit_core::pipeline`) — the single entry point the app uses:
  ```rust
  pub enum TranscriptStatus { Ok, Empty, Hallucination }     // as_str(): "ok" | "empty" | "hallucination"; parse(&str)
  pub struct Transcript { pub raw_text: String, pub text: String, pub status: TranscriptStatus,
      pub profile_id: String, pub used_fallback: bool, pub audio_ms: u64, pub request_ms: u64, pub hits: Vec<RuleHit> }
  pub struct PreparedAudio { pub samples: Vec<f32>, pub audio_ms: u64, pub speech_ms: u64 }
  pub enum PipelineError { TooShort, NoSpeech, Encode(EncodeError), Provider(ProviderError) }
  pub struct PipelineContext<'a, T> { pub primary: &'a T, pub fallback: Option<&'a T>,
      pub rules: &'a RuleSet, pub prompt_context: &'a str, pub retry_delay: Duration }
  pub fn prepare(recording: &Recording) -> Result<PreparedAudio, PipelineError>;
  pub async fn transcribe<T: Transcriber>(audio: &PreparedAudio, ctx: &PipelineContext<'_, T>) -> Result<Transcript, PipelineError>;
  pub fn postprocess(raw: &str, rules: &RuleSet, apply_rules: bool) -> (String, TranscriptStatus, Vec<RuleHit>);
  pub async fn run<T: Transcriber>(recording: &Recording, ctx: &PipelineContext<'_, T>) -> Result<Transcript, PipelineError>;
  ```
  `prepare` and `transcribe` are separate so the app can keep a `PreparedAudio` in memory for "Try again". The primary profile gets one retry. The fallback gets a single attempt, and only after a retryable failure. The prompt and keywords are built per profile (`send_prompt` / `send_keywords`). Rules and the hallucination filter run only when the profile that answered has `apply_rules`. The `run` future must be `Send` (the app spawns it on tokio).

- [ ] **Step 1: Wire the modules**

Add to `crates/core/src/provider/mod.rs` below `pub mod profile;`:
```rust
pub mod retry;
```

Append to `crates/core/src/lib.rs`:
```rust
pub mod pipeline;
```

- [ ] **Step 2: Write the failing retry tests**

`crates/core/src/provider/retry.rs`:
```rust
//! One retry for transient provider failures.

#[cfg(test)]
mod tests {
    use super::*;

    async fn run_script(mut script: Vec<Result<i32, ProviderError>>) -> (Result<i32, ProviderError>, usize) {
        script.reverse();
        let mut calls = 0;
        let result = with_retry(Duration::ZERO, || {
            calls += 1;
            let next = script.pop().expect("no more scripted results");
            async move { next }
        })
        .await;
        (result, calls)
    }

    #[tokio::test]
    async fn retries_once_on_transient_errors() {
        assert_eq!(run_script(vec![Err(ProviderError::Server(502)), Ok(7)]).await, (Ok(7), 2));
    }

    #[tokio::test]
    async fn gives_up_after_the_second_transient_error() {
        let (result, calls) = run_script(vec![Err(ProviderError::Timeout), Err(ProviderError::RateLimited)]).await;
        assert_eq!((result, calls), (Err(ProviderError::RateLimited), 2));
    }

    #[tokio::test]
    async fn does_not_retry_permanent_errors() {
        assert_eq!(run_script(vec![Err(ProviderError::Unauthorized(401))]).await, (Err(ProviderError::Unauthorized(401)), 1));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core provider::retry`
Expected: FAIL — `cannot find function with_retry` (and `pipeline` module missing: create `crates/core/src/pipeline.rs` containing only `//! Recording → transcript.` for now).

- [ ] **Step 4: Implement retry**

Insert above the tests in `crates/core/src/provider/retry.rs`:
```rust
use std::time::Duration;

use super::ProviderError;

pub const RETRY_DELAY: Duration = Duration::from_millis(500);

/// Runs `attempt`; on a retryable error waits `delay` and runs it once more.
pub async fn with_retry<T, F, Fut>(delay: Duration, mut attempt: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    match attempt().await {
        Err(err) if err.is_retryable() => {
            tokio::time::sleep(delay).await;
            attempt().await
        }
        result => result,
    }
}
```

- [ ] **Step 5: Run the retry tests to verify they pass**

Run: `cargo test -p opit-core provider::retry`
Expected: 3 passed.

- [ ] **Step 6: Write the failing pipeline unit tests**

Replace `crates/core/src/pipeline.rs` with:
```rust
//! Recording → transcript: prepare audio, call the provider (retry, fallback),
//! then post-process with the rule layer.

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::provider::{Profile, presets};
    use crate::rules::builtin::assemble;

    struct Scripted {
        profile: Profile,
        script: Mutex<VecDeque<Result<RawTranscript, ProviderError>>>,
        calls: AtomicUsize,
        prompts: Mutex<Vec<Option<String>>>,
    }

    impl Scripted {
        fn new(id: &str, script: Vec<Result<&str, ProviderError>>) -> Self {
            let script = script
                .into_iter()
                .map(|r| r.map(|text| RawTranscript { text: text.into(), dropped_segments: 0 }))
                .collect();
            Self {
                profile: presets::custom(id, id, "http://unused.invalid/v1", "m"),
                script: Mutex::new(script),
                calls: AtomicUsize::new(0),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Transcriber for Scripted {
        fn profile(&self) -> &Profile {
            &self.profile
        }

        fn transcribe(
            &self,
            request: &TranscribeRequest<'_>,
        ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.prompts.lock().unwrap().push(request.prompt.map(str::to_string));
            let next = self.script.lock().unwrap().pop_front().expect("unexpected extra call");
            async move { next }
        }
    }

    fn tone_recording(seconds: f32) -> Recording {
        let frames = (48_000.0 * seconds) as usize;
        let samples = (0..frames)
            .flat_map(|i| {
                let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
                [s, s]
            })
            .collect();
        Recording { samples, sample_rate: 48_000, channels: 2 }
    }

    fn speech() -> PreparedAudio {
        prepare(&tone_recording(1.0)).unwrap()
    }

    fn ctx<'a>(primary: &'a Scripted, fallback: Option<&'a Scripted>, rules: &'a RuleSet) -> PipelineContext<'a, Scripted> {
        PipelineContext { primary, fallback, rules, prompt_context: "", retry_delay: Duration::ZERO }
    }

    fn tech_rules() -> RuleSet {
        RuleSet::compile(&assemble(None, &["tr-core".to_string(), "tr-tech".to_string()])).0
    }

    #[test]
    fn prepare_rejects_short_and_silent_recordings() {
        assert_eq!(prepare(&tone_recording(0.2)).unwrap_err(), PipelineError::TooShort);
        let silent = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(prepare(&silent).unwrap_err(), PipelineError::NoSpeech);
        let ok = speech();
        assert_eq!((ok.audio_ms, ok.samples.len()), (1_000, 16_000));
    }

    #[test]
    fn status_strings_round_trip() {
        for status in [TranscriptStatus::Ok, TranscriptStatus::Empty, TranscriptStatus::Hallucination] {
            assert_eq!(TranscriptStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(TranscriptStatus::parse("nope"), None);
    }

    #[tokio::test]
    async fn retries_the_primary_once() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Server(503)), Ok("merhaba")]);
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.used_fallback, primary.calls()), ("merhaba", false, 2));
        assert_eq!(t.profile_id, "a");
    }

    #[tokio::test]
    async fn falls_back_after_the_retry_fails() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Server(502)), Err(ProviderError::Timeout)]);
        let fallback = Scripted::new("b", vec![Ok("yedek")]);
        let t = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.used_fallback, t.profile_id.as_str()), ("yedek", true, "b"));
        assert_eq!((primary.calls(), fallback.calls()), (2, 1));
    }

    #[tokio::test]
    async fn fallback_gets_a_single_attempt() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::RateLimited), Err(ProviderError::RateLimited)]);
        let fallback = Scripted::new("b", vec![Err(ProviderError::Server(500))]);
        let err = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Server(500)));
        assert_eq!(fallback.calls(), 1);
    }

    #[tokio::test]
    async fn auth_errors_skip_retry_and_fallback() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Unauthorized(401))]);
        let fallback = Scripted::new("b", vec![Ok("x")]);
        let err = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Unauthorized(401)));
        assert_eq!((primary.calls(), fallback.calls()), (1, 0));
    }

    #[tokio::test]
    async fn rules_run_on_the_provider_text() {
        let rules = tech_rules();
        let primary = Scripted::new("a", vec![Ok("cloud code'u aç")]);
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!(t.raw_text, "cloud code'u aç");
        assert_eq!(t.text, "Claude Code'u aç");
        assert_eq!(t.status, TranscriptStatus::Ok);
        assert!(!t.hits.is_empty());
    }

    #[tokio::test]
    async fn rules_are_skipped_when_the_profile_says_so() {
        let rules = tech_rules();
        let mut primary = Scripted::new("a", vec![Ok(" cloud code'u aç ")]);
        primary.profile.apply_rules = false;
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!(t.text, "cloud code'u aç");
        assert!(t.hits.is_empty());
    }

    #[tokio::test]
    async fn empty_and_hallucinated_outputs_are_flagged() {
        let rules = tech_rules();
        let blank = Scripted::new("a", vec![Ok("   ")]);
        let t = transcribe(&speech(), &ctx(&blank, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.status), ("", TranscriptStatus::Empty));

        let ghost = Scripted::new("a", vec![Ok("Altyazı M.K.")]);
        let t = transcribe(&speech(), &ctx(&ghost, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.status), ("", TranscriptStatus::Hallucination));
        assert_eq!(t.raw_text, "Altyazı M.K.");
    }

    #[tokio::test]
    async fn prompt_follows_the_profile() {
        let rules = tech_rules();
        let primary = Scripted::new("a", vec![Ok("x")]);
        let mut with_context = ctx(&primary, None, &rules);
        with_context.prompt_context = "Yazılım konuşması";
        transcribe(&speech(), &with_context).await.unwrap();
        let prompt = primary.prompts.lock().unwrap()[0].clone().unwrap();
        assert!(prompt.starts_with("Yazılım konuşması. Geçen terimler: Claude Code, Claude,"), "{prompt}");

        let mut quiet = Scripted::new("b", vec![Ok("x")]);
        quiet.profile.send_prompt = false;
        transcribe(&speech(), &ctx(&quiet, None, &rules)).await.unwrap();
        assert_eq!(quiet.prompts.lock().unwrap()[0], None);
    }
}
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cargo test -p opit-core pipeline::`
Expected: FAIL — `cannot find function prepare`.

- [ ] **Step 8: Implement the pipeline**

Insert above the tests in `crates/core/src/pipeline.rs`:
```rust
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::audio::encode::{EncodeError, EncodedAudio, encode};
use crate::audio::gate::{self, GateVerdict};
use crate::audio::{self, Recording, TARGET_RATE};
use crate::provider::retry::with_retry;
use crate::provider::{ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use crate::rules::prompt::{build_prompt, keywords};
use crate::rules::{RuleHit, RuleSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptStatus {
    Ok,
    /// The provider returned nothing.
    Empty,
    /// The whole output matched a known silence hallucination.
    Hallucination,
}

impl TranscriptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::Hallucination => "hallucination",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "empty" => Some(Self::Empty),
            "hallucination" => Some(Self::Hallucination),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    /// Provider output before rules.
    pub raw_text: String,
    /// Text to paste.
    pub text: String,
    pub status: TranscriptStatus,
    /// Profile that produced the text (differs from the primary after a fallback).
    pub profile_id: String,
    pub used_fallback: bool,
    pub audio_ms: u64,
    /// Wall time from the first request to the final answer, retries included.
    pub request_ms: u64,
    pub hits: Vec<RuleHit>,
}

/// 16 kHz mono audio that passed the silence gate; kept for "Try again".
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAudio {
    pub samples: Vec<f32>,
    pub audio_ms: u64,
    pub speech_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    #[error("the recording is too short")]
    TooShort,
    #[error("no speech was detected")]
    NoSpeech,
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

pub struct PipelineContext<'a, T> {
    pub primary: &'a T,
    pub fallback: Option<&'a T>,
    pub rules: &'a RuleSet,
    pub prompt_context: &'a str,
    pub retry_delay: Duration,
}

pub fn prepare(recording: &Recording) -> Result<PreparedAudio, PipelineError> {
    let samples = audio::to_mono_16k(recording);
    match gate::check(&samples) {
        GateVerdict::TooShort => Err(PipelineError::TooShort),
        GateVerdict::NoSpeech => Err(PipelineError::NoSpeech),
        GateVerdict::Speech { speech_ms } => {
            let audio_ms = samples.len() as u64 * 1000 / u64::from(TARGET_RATE);
            Ok(PreparedAudio { samples, audio_ms, speech_ms })
        }
    }
}

pub async fn transcribe<T: Transcriber>(
    audio: &PreparedAudio,
    ctx: &PipelineContext<'_, T>,
) -> Result<Transcript, PipelineError> {
    let started = Instant::now();
    let primary_audio = encode(&audio.samples, ctx.primary.profile().audio_format)?;
    let (raw, used, used_fallback) = match call(ctx.primary, &primary_audio, audio, ctx, true).await {
        Ok(raw) => (raw, ctx.primary, false),
        Err(err) => match ctx.fallback {
            Some(fallback) if err.is_retryable() => {
                let format = fallback.profile().audio_format;
                let fallback_audio;
                let encoded = if format == primary_audio.format {
                    &primary_audio
                } else {
                    fallback_audio = encode(&audio.samples, format)?;
                    &fallback_audio
                };
                (call(fallback, encoded, audio, ctx, false).await?, fallback, true)
            }
            _ => return Err(err.into()),
        },
    };

    let profile = used.profile();
    let (text, status, hits) = postprocess(&raw.text, ctx.rules, profile.apply_rules);
    Ok(Transcript {
        raw_text: raw.text,
        text,
        status,
        profile_id: profile.id.clone(),
        used_fallback,
        audio_ms: audio.audio_ms,
        request_ms: started.elapsed().as_millis() as u64,
        hits,
    })
}

pub fn postprocess(raw: &str, rules: &RuleSet, apply_rules: bool) -> (String, TranscriptStatus, Vec<RuleHit>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), TranscriptStatus::Empty, Vec::new());
    }
    if !apply_rules {
        return (trimmed.to_string(), TranscriptStatus::Ok, Vec::new());
    }
    if rules.is_hallucination(trimmed) {
        return (String::new(), TranscriptStatus::Hallucination, Vec::new());
    }
    let (text, hits) = rules.apply_traced(trimmed);
    (text, TranscriptStatus::Ok, hits)
}

pub async fn run<T: Transcriber>(recording: &Recording, ctx: &PipelineContext<'_, T>) -> Result<Transcript, PipelineError> {
    let audio = prepare(recording)?;
    transcribe(&audio, ctx).await
}

async fn call<T: Transcriber>(
    transcriber: &T,
    encoded: &EncodedAudio,
    audio: &PreparedAudio,
    ctx: &PipelineContext<'_, T>,
    retry: bool,
) -> Result<RawTranscript, ProviderError> {
    let profile = transcriber.profile();
    let prompt = if profile.send_prompt {
        build_prompt(ctx.prompt_context, ctx.rules.terms(), &profile.language).prompt
    } else {
        None
    };
    let keyword_list = if profile.send_keywords { keywords(ctx.rules.terms()) } else { Vec::new() };
    let request =
        TranscribeRequest { audio: encoded, audio_ms: audio.audio_ms, prompt: prompt.as_deref(), keywords: &keyword_list };
    if retry {
        with_retry(ctx.retry_delay, || transcriber.transcribe(&request)).await
    } else {
        transcriber.transcribe(&request).await
    }
}
```

- [ ] **Step 9: Run the unit tests to verify they pass**

Run: `cargo test -p opit-core pipeline::`
Expected: 10 passed.

- [ ] **Step 10: Write the HTTP end-to-end test**

`crates/core/tests/pipeline_http.rs`:
```rust
//! Full pipeline against a fake OpenAI-compatible server.

use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::pipeline::{self, PipelineContext, TranscriptStatus};
use opit_core::provider::{OpenAiCompatible, presets};
use opit_core::rules::RuleSet;
use opit_core::rules::builtin::assemble;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn speech() -> Recording {
    let samples = (0..48_000)
        .flat_map(|i| {
            let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
            [s, s]
        })
        .collect();
    Recording { samples, sample_rate: 48_000, channels: 2 }
}

/// Compile-time check: the app spawns this future on tokio.
fn assert_send<F: Send>(future: F) -> F {
    future
}

#[tokio::test]
async fn groq_style_round_trip_with_rules() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "cloud code'u aç",
            "segments": [{"text": "cloud code'u aç", "no_speech_prob": 0.01, "avg_logprob": -0.1}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut profile = presets::groq();
    profile.base_url = format!("{}/openai/v1", server.uri());
    let client = OpenAiCompatible::new(profile, Some("gsk-test".into())).unwrap();
    let (rules, _) = RuleSet::compile(&assemble(None, &["tr-core".to_string(), "tr-tech".to_string()]));
    let ctx = PipelineContext {
        primary: &client,
        fallback: None,
        rules: &rules,
        prompt_context: "Yazılım konuşması",
        retry_delay: Duration::ZERO,
    };

    let transcript = assert_send(pipeline::run(&speech(), &ctx)).await.unwrap();
    assert_eq!(transcript.text, "Claude Code'u aç");
    assert_eq!(transcript.status, TranscriptStatus::Ok);
    assert_eq!(transcript.profile_id, "groq");

    let body = server.received_requests().await.unwrap().remove(0).body;
    assert!(body.windows(4).any(|w| w == b"fLaC"), "the Groq profile uploads FLAC");
}

#[tokio::test]
async fn http_fallback_after_two_server_errors() {
    let primary_server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(503)).expect(2).mount(&primary_server).await;
    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "yedekten geldi"})))
        .expect(1)
        .mount(&fallback_server)
        .await;

    let mut primary_profile = presets::groq();
    primary_profile.base_url = format!("{}/v1", primary_server.uri());
    let fallback_profile = presets::custom("gpu", "GPU", &format!("{}/v1", fallback_server.uri()), "large-v3");
    let primary = OpenAiCompatible::new(primary_profile, None).unwrap();
    let fallback = OpenAiCompatible::new(fallback_profile, None).unwrap();
    let rules = RuleSet::empty();
    let ctx = PipelineContext {
        primary: &primary,
        fallback: Some(&fallback),
        rules: &rules,
        prompt_context: "",
        retry_delay: Duration::ZERO,
    };

    let transcript = pipeline::run(&speech(), &ctx).await.unwrap();
    assert!(transcript.used_fallback);
    assert_eq!(transcript.profile_id, "gpu");
    assert_eq!(transcript.text, "yedekten geldi");
}
```

- [ ] **Step 11: Run all core tests**

Run: `cargo test -p opit-core`
Expected: all unit tests and the 2 `pipeline_http` tests pass. If `assert_send` fails to compile, a non-`Send` value is held across an `.await` in `pipeline.rs`; fix it there rather than dropping the check.

- [ ] **Step 12: Commit**

```bash
git add crates/core
git commit -m "feat(core): add dictation pipeline with retry and fallback

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: History store (SQLite + FTS5)

**Files:**
- Create: `crates/core/src/history/mod.rs`, `crates/core/src/history/audio_store.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod history;`), `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `TranscriptStatus` (Task 13); `text::is_word_char` (Task 1).
- Produces (`opit_core::history`):
  ```rust
  pub struct Dictation { pub id: i64, pub created_at_ms: i64, pub profile_id: String, pub raw_text: String,
      pub text: String, pub status: TranscriptStatus, pub audio_ms: u64, pub latency_ms: u64, pub audio_path: Option<String> }
  pub struct NewDictation<'a> { pub created_at_ms: i64, pub profile_id: &'a str, pub raw_text: &'a str,
      pub text: &'a str, pub status: TranscriptStatus, pub audio_ms: u64, pub latency_ms: u64 }
  pub enum HistoryError { Sqlite(rusqlite::Error), Io(std::io::Error) }
  pub struct HistoryStore;
  impl HistoryStore {
      pub fn open(path: &Path) -> Result<Self, HistoryError>;         // creates parent dir, WAL
      pub fn open_in_memory() -> Result<Self, HistoryError>;
      pub fn insert(&self, d: &NewDictation<'_>) -> Result<i64, HistoryError>;
      pub fn set_audio_path(&self, id: i64, path: &str) -> Result<(), HistoryError>;
      pub fn get(&self, id: i64) -> Result<Option<Dictation>, HistoryError>;
      pub fn recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, HistoryError>; // newest first
      pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, HistoryError>;
      pub fn delete(&self, id: i64) -> Result<Option<String>, HistoryError>;   // returns audio path to remove
      pub fn clear(&self) -> Result<Vec<String>, HistoryError>;                 // returns audio paths to remove
      pub fn take_expired_audio(&self, older_than_ms: i64) -> Result<Vec<String>, HistoryError>;
  }
  pub fn fts_query(input: &str) -> Option<String>;   // user text → safe FTS5 prefix query
  // opit_core::history::audio_store
  pub struct AudioStore;
  impl AudioStore { pub fn new(dir: impl Into<PathBuf>) -> Self;
      pub fn save(&self, id: i64, wav: &[u8]) -> io::Result<PathBuf>; pub fn remove(path: &Path) -> io::Result<()>; }
  ```
  The store never deletes audio files itself: it returns paths, and the caller removes them with `AudioStore::remove`. This keeps SQLite and file I/O separately testable. Search indexes both `text` and `raw_text` with `unicode61 remove_diacritics 2`, so `hala` finds `hâlâ`.

- [ ] **Step 1: Add the dependency and wire the module**

In `crates/core/Cargo.toml` `[dependencies]` add:
```toml
rusqlite.workspace = true
```

Append to `crates/core/src/lib.rs`:
```rust
pub mod history;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/history/audio_store.rs`:
```rust
//! Optional per-dictation WAV files (only when "save audio" is on).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_into_a_created_dir_and_removes_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path().join("audio"));
        let path = store.save(42, b"RIFF").unwrap();
        assert_eq!(path.file_name().unwrap(), "42.wav");
        assert_eq!(std::fs::read(&path).unwrap(), b"RIFF");
        AudioStore::remove(&path).unwrap();
        assert!(!path.exists());
        AudioStore::remove(&path).unwrap();
    }
}
```

`crates/core/src/history/mod.rs`:
```rust
//! Local dictation history: SQLite with an FTS5 index.

pub mod audio_store;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(text: &'a str, raw_text: &'a str, created_at_ms: i64) -> NewDictation<'a> {
        NewDictation {
            created_at_ms,
            profile_id: "groq",
            raw_text,
            text,
            status: TranscriptStatus::Ok,
            audio_ms: 1_000,
            latency_ms: 800,
        }
    }

    fn ids(list: &[Dictation]) -> Vec<i64> {
        list.iter().map(|d| d.id).collect()
    }

    #[test]
    fn insert_and_get_round_trip() {
        let store = HistoryStore::open_in_memory().unwrap();
        let id = store.insert(&entry("Claude Code'u aç", "cloud code'u aç", 1_000)).unwrap();
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.text, "Claude Code'u aç");
        assert_eq!(d.raw_text, "cloud code'u aç");
        assert_eq!((d.status, d.audio_ms, d.latency_ms, d.audio_path), (TranscriptStatus::Ok, 1_000, 800, None));
        assert!(store.get(id + 1).unwrap().is_none());
    }

    #[test]
    fn recent_is_newest_first_and_pages() {
        let store = HistoryStore::open_in_memory().unwrap();
        for i in 0..5 {
            store.insert(&entry("x", "x", i)).unwrap();
        }
        assert_eq!(ids(&store.recent(2, None).unwrap()), [5, 4]);
        assert_eq!(ids(&store.recent(2, Some(4)).unwrap()), [3, 2]);
    }

    #[test]
    fn search_covers_text_raw_text_prefixes_and_diacritics() {
        let store = HistoryStore::open_in_memory().unwrap();
        let a = store.insert(&entry("Claude Code'u aç", "cloud code'u aç", 1)).unwrap();
        let b = store.insert(&entry("o hâlâ burada", "o hala burada", 2)).unwrap();
        assert_eq!(ids(&store.search("claude", 10).unwrap()), [a]);
        assert_eq!(ids(&store.search("cloud", 10).unwrap()), [a]);
        assert_eq!(ids(&store.search("burad", 10).unwrap()), [b]);
        assert_eq!(ids(&store.search("hâlâ", 10).unwrap()), [b]);
        assert_eq!(ids(&store.search("code'u", 10).unwrap()), [a]);
        let c = store.insert(&entry("kalıcı ayar", "kalıcı ayar", 3)).unwrap();
        assert_eq!(ids(&store.search("KALICI", 10).unwrap()), [c]);
        assert_eq!(ids(&store.search("kalici", 10).unwrap()), [c]);
    }

    #[test]
    fn search_tolerates_fts_syntax_and_empty_input() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.insert(&entry("merhaba", "merhaba", 1)).unwrap();
        for query in ["", "   ", "\"(*", "AND OR NOT", "merhaba\" OR \"x"] {
            store.search(query, 10).unwrap_or_else(|e| panic!("{query:?}: {e}"));
        }
        assert!(store.search("\"(*", 10).unwrap().is_empty());
    }

    #[test]
    fn delete_and_clear_return_audio_paths() {
        let store = HistoryStore::open_in_memory().unwrap();
        let a = store.insert(&entry("bir", "bir", 1)).unwrap();
        let b = store.insert(&entry("iki", "iki", 2)).unwrap();
        store.insert(&entry("üç", "üç", 3)).unwrap();
        store.set_audio_path(a, "a.wav").unwrap();
        store.set_audio_path(b, "b.wav").unwrap();

        assert_eq!(store.delete(a).unwrap().as_deref(), Some("a.wav"));
        assert!(store.search("bir", 10).unwrap().is_empty());
        assert_eq!(store.clear().unwrap(), ["b.wav"]);
        assert!(store.recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn expired_audio_is_detached_once() {
        let store = HistoryStore::open_in_memory().unwrap();
        let old = store.insert(&entry("eski", "eski", 100)).unwrap();
        let new = store.insert(&entry("yeni", "yeni", 900)).unwrap();
        store.set_audio_path(old, "old.wav").unwrap();
        store.set_audio_path(new, "new.wav").unwrap();
        assert_eq!(store.take_expired_audio(500).unwrap(), ["old.wav"]);
        assert_eq!(store.get(old).unwrap().unwrap().audio_path, None);
        assert_eq!(store.get(new).unwrap().unwrap().audio_path.as_deref(), Some("new.wav"));
        assert!(store.take_expired_audio(500).unwrap().is_empty());
    }

    #[test]
    fn file_database_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("history.db");
        {
            let store = HistoryStore::open(&path).unwrap();
            store.insert(&entry("kalıcı", "kalıcı", 1)).unwrap();
        }
        let store = HistoryStore::open(&path).unwrap();
        assert_eq!(store.recent(10, None).unwrap().len(), 1);
        assert_eq!(store.search("kalıcı", 10).unwrap().len(), 1);
    }

    #[test]
    fn fts_query_quotes_word_tokens() {
        assert_eq!(fts_query("Claude code'u").as_deref(), Some("\"claude\"* \"code\"* \"u\"*"));
        assert_eq!(fts_query(" \"(* "), None);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-core history::`
Expected: FAIL — `cannot find type HistoryStore` / `AudioStore`.

- [ ] **Step 4: Implement `AudioStore`**

Insert above the tests in `crates/core/src/history/audio_store.rs`:
```rust
use std::io;
use std::path::{Path, PathBuf};

pub struct AudioStore {
    dir: PathBuf,
}

impl AudioStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn save(&self, id: i64, wav: &[u8]) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{id}.wav"));
        std::fs::write(&path, wav)?;
        Ok(path)
    }

    /// Removes a stored file; a file that is already gone is not an error.
    pub fn remove(path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}
```

- [ ] **Step 5: Implement `HistoryStore`**

Insert between `pub mod audio_store;` and the tests in `crates/core/src/history/mod.rs`:
```rust
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

pub use audio_store::AudioStore;

use crate::pipeline::TranscriptStatus;
use crate::text::{fold, is_word_char};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE dictations (
    id            INTEGER PRIMARY KEY,
    created_at_ms INTEGER NOT NULL,
    profile_id    TEXT    NOT NULL,
    raw_text      TEXT    NOT NULL,
    text          TEXT    NOT NULL,
    status        TEXT    NOT NULL,
    audio_ms      INTEGER NOT NULL,
    latency_ms    INTEGER NOT NULL,
    audio_path    TEXT,
    -- Turkish-folded copies (see text::fold) that the FTS index is built from.
    search_text   TEXT    NOT NULL,
    search_raw    TEXT    NOT NULL
);
CREATE INDEX dictations_created ON dictations(created_at_ms);
CREATE VIRTUAL TABLE dictations_fts USING fts5(
    search_text, search_raw,
    content = 'dictations', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER dictations_ai AFTER INSERT ON dictations BEGIN
    INSERT INTO dictations_fts(rowid, search_text, search_raw) VALUES (new.id, new.search_text, new.search_raw);
END;
CREATE TRIGGER dictations_ad AFTER DELETE ON dictations BEGIN
    INSERT INTO dictations_fts(dictations_fts, rowid, search_text, search_raw)
    VALUES ('delete', old.id, old.search_text, old.search_raw);
END;
CREATE TRIGGER dictations_au AFTER UPDATE OF search_text, search_raw ON dictations BEGIN
    INSERT INTO dictations_fts(dictations_fts, rowid, search_text, search_raw)
    VALUES ('delete', old.id, old.search_text, old.search_raw);
    INSERT INTO dictations_fts(rowid, search_text, search_raw) VALUES (new.id, new.search_text, new.search_raw);
END;
";

const COLUMNS: &str = "id, created_at_ms, profile_id, raw_text, text, status, audio_ms, latency_ms, audio_path";

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("history file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dictation {
    pub id: i64,
    pub created_at_ms: i64,
    pub profile_id: String,
    pub raw_text: String,
    pub text: String,
    pub status: TranscriptStatus,
    pub audio_ms: u64,
    pub latency_ms: u64,
    pub audio_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewDictation<'a> {
    pub created_at_ms: i64,
    pub profile_id: &'a str,
    pub raw_text: &'a str,
    pub text: &'a str,
    pub status: TranscriptStatus,
    pub audio_ms: u64,
    pub latency_ms: u64,
}

pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, HistoryError> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn })
    }

    pub fn insert(&self, d: &NewDictation<'_>) -> Result<i64, HistoryError> {
        self.conn.execute(
            "INSERT INTO dictations
               (created_at_ms, profile_id, raw_text, text, status, audio_ms, latency_ms, search_text, search_raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                d.created_at_ms,
                d.profile_id,
                d.raw_text,
                d.text,
                d.status.as_str(),
                d.audio_ms as i64,
                d.latency_ms as i64,
                fold(d.text),
                fold(d.raw_text)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_audio_path(&self, id: i64, path: &str) -> Result<(), HistoryError> {
        self.conn.execute("UPDATE dictations SET audio_path = ?1 WHERE id = ?2", params![path, id])?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<Dictation>, HistoryError> {
        let sql = format!("SELECT {COLUMNS} FROM dictations WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], from_row).optional()?)
    }

    pub fn recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, HistoryError> {
        let sql = format!("SELECT {COLUMNS} FROM dictations WHERE id < ?1 ORDER BY id DESC LIMIT ?2");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![before_id.unwrap_or(i64::MAX), limit as i64], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, HistoryError> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {COLUMNS} FROM dictations
             WHERE id IN (SELECT rowid FROM dictations_fts WHERE dictations_fts MATCH ?1)
             ORDER BY id DESC LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![fts, limit as i64], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete(&self, id: i64) -> Result<Option<String>, HistoryError> {
        let path = self
            .conn
            .query_row("SELECT audio_path FROM dictations WHERE id = ?1", [id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        self.conn.execute("DELETE FROM dictations WHERE id = ?1", [id])?;
        Ok(path)
    }

    pub fn clear(&self) -> Result<Vec<String>, HistoryError> {
        let paths = self.audio_paths_before(i64::MAX)?;
        self.conn.execute("DELETE FROM dictations", [])?;
        Ok(paths)
    }

    /// Detaches audio files of dictations created before `older_than_ms` and
    /// returns their paths for deletion.
    pub fn take_expired_audio(&self, older_than_ms: i64) -> Result<Vec<String>, HistoryError> {
        let paths = self.audio_paths_before(older_than_ms)?;
        self.conn.execute(
            "UPDATE dictations SET audio_path = NULL WHERE audio_path IS NOT NULL AND created_at_ms < ?1",
            [older_than_ms],
        )?;
        Ok(paths)
    }

    fn audio_paths_before(&self, before_ms: i64) -> Result<Vec<String>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT audio_path FROM dictations WHERE audio_path IS NOT NULL AND created_at_ms < ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([before_ms], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Dictation> {
    let status: String = row.get(5)?;
    Ok(Dictation {
        id: row.get(0)?,
        created_at_ms: row.get(1)?,
        profile_id: row.get(2)?,
        raw_text: row.get(3)?,
        text: row.get(4)?,
        status: TranscriptStatus::parse(&status).unwrap_or(TranscriptStatus::Ok),
        audio_ms: row.get::<_, i64>(6)? as u64,
        latency_ms: row.get::<_, i64>(7)? as u64,
        audio_path: row.get(8)?,
    })
}

/// Turns free text into an FTS5 query of quoted prefix terms, so user input can
/// never be parsed as FTS5 syntax. Returns `None` when nothing searchable is left.
pub fn fts_query(input: &str) -> Option<String> {
    let cleaned: String = fold(input).chars().map(|c| if is_word_char(c) { c } else { ' ' }).collect();
    let terms: Vec<String> = cleaned.split_whitespace().map(|t| format!("\"{t}\"*")).collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}
```

The FTS index is built from Turkish-folded copies of the text (`search_text`, `search_raw`), and `fts_query` folds the query the same way, so dotted and dotless i match (`kalici` finds `kalıcı`). The tokenizer's `remove_diacritics 2` handles â/ş/ğ (`hala` finds `hâlâ`). `clear` passes `i64::MAX` as the cutoff, so `audio_paths_before` returns every stored path.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p opit-core history::`
Expected: 9 passed (8 store + 1 audio store).

- [ ] **Step 7: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "feat(core): add SQLite dictation history with FTS5 search

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Eval crate — WER and term metrics

**Files:**
- Create: `crates/eval/Cargo.toml`, `crates/eval/src/lib.rs`, `crates/eval/src/metrics.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Consumes: `opit_core::text::{fold, is_word_char}` (Task 1); `opit_core::rules::matcher::{Pattern, PhraseMatcher}` (Task 2).
- Produces (`opit_eval::metrics`):
  ```rust
  pub fn words(text: &str) -> Vec<String>;                 // folded, punctuation → separators
  pub struct WerStats { pub errors: usize, pub reference_words: usize }   // rate(), add()
  pub fn wer(reference: &str, hypothesis: &str) -> WerStats;
  pub struct TermStats { pub expected: usize, pub hit: usize }            // rate(), add()
  /// Occurrences of each term in the reference (case-insensitive), and how many
  /// appear in the hypothesis with exact canonical spelling.
  pub fn term_hits(reference: &str, hypothesis: &str, terms: &[String]) -> TermStats;
  ```

- [ ] **Step 1: Create the crate**

In the root `Cargo.toml`, change `members`:
```toml
members = ["crates/core", "crates/eval"]
```

`crates/eval/Cargo.toml`:
```toml
[package]
name = "opit-eval"
description = "Measures transcription accuracy of Opit Speech to Text provider profiles"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
anyhow.workspace = true
clap.workspace = true
hound.workspace = true
opit-core = { path = "../core" }
tokio.workspace = true

[dev-dependencies]
serde_json.workspace = true
tempfile.workspace = true
wiremock.workspace = true
```

`crates/eval/src/lib.rs`:
```rust
//! Accuracy evaluation for provider profiles and rule packs.

pub mod metrics;
```

- [ ] **Step 2: Write the failing tests**

`crates/eval/src/metrics.rs`:
```rust
//! Word error rate and term accuracy.

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn words_fold_case_and_drop_punctuation() {
        assert_eq!(words("Merhaba, Dünya! Claude Code'u"), ["merhaba", "dünya", "claude", "code", "u"]);
    }

    #[test]
    fn wer_counts_substitutions_deletions_insertions() {
        assert_eq!(wer("a b c", "a b c"), WerStats { errors: 0, reference_words: 3 });
        assert_eq!(wer("a b c", "a x c").errors, 1);
        assert_eq!(wer("a b c", "a c").errors, 1);
        assert_eq!(wer("a b c", "a b c d").errors, 1);
        assert_eq!(wer("Merhaba, dünya!", "merhaba dünya").errors, 0);
    }

    #[test]
    fn wer_rate_edge_cases() {
        assert_eq!(wer("", "").rate(), 0.0);
        assert_eq!(wer("", "fazla").rate(), 1.0);
        assert!((wer("a b c d", "a x c d").rate() - 0.25).abs() < 1e-9);
        let total = wer("a b", "a").add(wer("c d", "c d"));
        assert_eq!(total, WerStats { errors: 1, reference_words: 4 });
    }

    #[test]
    fn term_hits_require_exact_spelling() {
        let list = terms(&["Claude Code", "GitHub", "FiveM"]);
        let stats = term_hits("Claude Code'u GitHub'a at", "cloud code'u GitHub'a at", &list);
        assert_eq!(stats, TermStats { expected: 2, hit: 1 });
        let lower = term_hits("GitHub'a at", "github'a at", &list);
        assert_eq!(lower, TermStats { expected: 1, hit: 0 });
        assert_eq!(term_hits("yok", "yok", &list).rate(), 1.0);
    }

    #[test]
    fn term_hits_cap_at_expected_count() {
        let stats = term_hits("GitHub", "GitHub GitHub", &terms(&["GitHub"]));
        assert_eq!(stats, TermStats { expected: 1, hit: 1 });
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-eval metrics::`
Expected: FAIL — `cannot find function words`.

- [ ] **Step 4: Implement**

Insert above the tests:
```rust
use opit_core::rules::matcher::{Pattern, PhraseMatcher};
use opit_core::text::{fold, is_word_char};

/// Folded words with punctuation (including apostrophes) treated as separators.
pub fn words(text: &str) -> Vec<String> {
    let cleaned: String = fold(text).chars().map(|c| if is_word_char(c) { c } else { ' ' }).collect();
    cleaned.split_whitespace().map(str::to_string).collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WerStats {
    pub errors: usize,
    pub reference_words: usize,
}

impl WerStats {
    pub fn rate(&self) -> f64 {
        match (self.reference_words, self.errors) {
            (0, 0) => 0.0,
            (0, _) => 1.0,
            (words, errors) => errors as f64 / words as f64,
        }
    }

    pub fn add(self, other: Self) -> Self {
        Self { errors: self.errors + other.errors, reference_words: self.reference_words + other.reference_words }
    }
}

pub fn wer(reference: &str, hypothesis: &str) -> WerStats {
    let reference = words(reference);
    let hypothesis = words(hypothesis);
    WerStats { errors: edit_distance(&reference, &hypothesis), reference_words: reference.len() }
}

fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(x != y);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TermStats {
    pub expected: usize,
    pub hit: usize,
}

impl TermStats {
    pub fn rate(&self) -> f64 {
        if self.expected == 0 { 1.0 } else { self.hit as f64 / self.expected as f64 }
    }

    pub fn add(self, other: Self) -> Self {
        Self { expected: self.expected + other.expected, hit: self.hit + other.hit }
    }
}

pub fn term_hits(reference: &str, hypothesis: &str, terms: &[String]) -> TermStats {
    let mut stats = TermStats::default();
    for term in terms {
        let expected = count(term, reference, false);
        if expected == 0 {
            continue;
        }
        stats.expected += expected;
        stats.hit += count(term, hypothesis, true).min(expected);
    }
    stats
}

fn count(term: &str, text: &str, case_sensitive: bool) -> usize {
    let pattern = Pattern { from: term.to_string(), to: term.to_string(), rule: 0, case_sensitive, turkish: true };
    PhraseMatcher::new([pattern]).find_all(text).len()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-eval metrics::`
Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/eval
git commit -m "feat(eval): add WER and term accuracy metrics

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: Eval runner, report, CLI and docs

**Files:**
- Create: `crates/eval/src/dataset.rs`, `crates/eval/src/run.rs`, `crates/eval/src/report.rs`, `crates/eval/src/main.rs`, `crates/eval/tests/run_eval.rs`, `docs/eval.md`
- Modify: `crates/eval/src/lib.rs`, `README.md`, `.gitignore`

**Interfaces:**
- Consumes: `pipeline::{prepare, transcribe, PipelineContext, PreparedAudio}` (Task 13); `OpenAiCompatible`, `Profile`, `presets`, `ProviderError`, `retry::RETRY_DELAY` (Tasks 11–13); `RuleSet`, `load_pack_file`, `builtin::assemble` (Tasks 3, 5, 7); `Recording` (Task 8); metrics (Task 15).
- Produces:
  ```rust
  // opit_eval::dataset
  pub struct Sample { pub name: String, pub recording: Recording, pub reference: String }
  pub fn load_dir(dir: &Path) -> anyhow::Result<(Vec<Sample>, Vec<String>)>;   // samples, warnings
  pub fn read_wav(path: &Path) -> anyhow::Result<Recording>;
  // opit_eval::run
  pub enum RulesMode { On, Off, Both }                                          // clap ValueEnum
  pub struct EvalConfig { pub profile: Profile, pub api_key: Option<String>, pub rules: RuleSet,
      pub prompt_context: String, pub mode: RulesMode, pub retry_delay: Duration }
  pub struct Outcome { pub hypothesis: String, pub error: Option<String>, pub wer: WerStats,
      pub terms: TermStats, pub latency_ms: u64 }
  pub struct SampleResult { pub name: String, pub off: Option<Outcome>, pub on: Option<Outcome> }
  pub async fn run_eval(samples: &[Sample], config: &EvalConfig) -> Result<Vec<SampleResult>, ProviderError>;
  // opit_eval::report
  pub fn render(results: &[SampleResult]) -> String;                           // Markdown
  ```
  "Off" means no prompt, no keywords, no rules, and no hallucination filter: the raw provider output. "On" means the profile as configured with the given rules. Both modes score against the same term list (the active packs' terms). The report prints metrics only, never transcript text.

- [ ] **Step 1: Wire the modules**

Replace `crates/eval/src/lib.rs` with:
```rust
//! Accuracy evaluation for provider profiles and rule packs.

pub mod dataset;
pub mod metrics;
pub mod report;
pub mod run;
```

Append to `.gitignore`:
```
/eval-data/
```

- [ ] **Step 2: Write the failing dataset and report tests**

`crates/eval/src/dataset.rs`:
```rust
//! Loads `name.wav` + `name.txt` pairs.

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, samples: &[i16]) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn loads_pairs_and_warns_about_missing_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("b.wav"), &[0; 160]);
        write_wav(&dir.path().join("a.wav"), &[16_384; 160]);
        std::fs::write(dir.path().join("a.txt"), "  Merhaba dünya.\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "ignored").unwrap();

        let (samples, warnings) = load_dir(dir.path()).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "a");
        assert_eq!(samples[0].reference, "Merhaba dünya.");
        assert_eq!(samples[0].recording.sample_rate, 16_000);
        assert!((samples[0].recording.samples[0] - 0.5).abs() < 1e-6);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("b:"), "{}", warnings[0]);
    }

    #[test]
    fn missing_dir_is_an_error() {
        assert!(load_dir(Path::new("definitely/not/here")).is_err());
    }
}
```

`crates/eval/src/report.rs`:
```rust
//! Markdown summary of an eval run.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{TermStats, WerStats};
    use crate::run::{Outcome, SampleResult};

    fn outcome(errors: usize, words: usize, hit: usize, expected: usize, latency_ms: u64) -> Outcome {
        Outcome {
            hypothesis: String::new(),
            error: None,
            wer: WerStats { errors, reference_words: words },
            terms: TermStats { expected, hit },
            latency_ms,
        }
    }

    #[test]
    fn renders_rows_totals_and_errors() {
        let mut failed = outcome(3, 3, 0, 0, 0);
        failed.error = Some("no speech was detected".into());
        let results = vec![
            SampleResult { name: "a".into(), off: Some(outcome(1, 4, 0, 1, 700)), on: Some(outcome(0, 4, 1, 1, 900)) },
            SampleResult { name: "b".into(), off: None, on: Some(failed) },
        ];
        let md = render(&results);
        assert!(md.starts_with("| sample | WER off | WER on | terms off | terms on | latency on (ms) |\n"), "{md}");
        assert!(md.contains("| a | 25.0% | 0.0% | 0/1 | 1/1 | 900 |\n"), "{md}");
        assert!(md.contains("| b | – | 100.0% | – | 0/0 | – |\n"), "{md}");
        assert!(md.contains("| **total** | 25.0% | 42.9% | 0/1 | 1/1 | 900 |\n"), "{md}");
        assert!(md.contains("- b (on): no speech was detected"), "{md}");
    }

    #[test]
    fn median_of_latencies() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5]), Some(5));
        assert_eq!(median(&mut [9, 1, 5]), Some(5));
        assert_eq!(median(&mut [4, 1, 3, 2]), Some(3));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-eval`
Expected: FAIL — unresolved `load_dir`, `render`, and missing `run` module (create `crates/eval/src/run.rs` with just `//! Rules on/off evaluation loop.` so the crate compiles).

- [ ] **Step 4: Implement the dataset loader**

Insert above the tests in `crates/eval/src/dataset.rs`:
```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use opit_core::audio::Recording;

pub struct Sample {
    pub name: String,
    pub recording: Recording,
    pub reference: String,
}

/// Loads `name.wav` + `name.txt` pairs from `dir`, sorted by name. A WAV file
/// without a transcript is skipped and reported in the returned warnings.
pub fn load_dir(dir: &Path) -> Result<(Vec<Sample>, Vec<String>)> {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();

    let mut samples = Vec::new();
    let mut warnings = Vec::new();
    for wav in wavs {
        let name = wav.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(reference) = std::fs::read_to_string(wav.with_extension("txt")) else {
            warnings.push(format!("{name}: no {name}.txt next to the WAV file, skipped"));
            continue;
        };
        let recording = read_wav(&wav).with_context(|| format!("cannot decode {}", wav.display()))?;
        samples.push(Sample { name, recording, reference: reference.trim().to_string() });
    }
    Ok((samples, warnings))
}

pub fn read_wav(path: &Path) -> Result<Recording> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(Recording { samples, sample_rate: spec.sample_rate, channels: spec.channels })
}
```

- [ ] **Step 5: Implement the runner**

Replace `crates/eval/src/run.rs` with:
```rust
//! Rules on/off evaluation loop.

use std::time::Duration;

use clap::ValueEnum;
use opit_core::pipeline::{self, PipelineContext, PreparedAudio};
use opit_core::provider::{OpenAiCompatible, Profile, ProviderError};
use opit_core::rules::RuleSet;

use crate::dataset::Sample;
use crate::metrics::{TermStats, WerStats, term_hits, wer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RulesMode {
    On,
    Off,
    Both,
}

pub struct EvalConfig {
    pub profile: Profile,
    pub api_key: Option<String>,
    pub rules: RuleSet,
    pub prompt_context: String,
    pub mode: RulesMode,
    pub retry_delay: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub hypothesis: String,
    pub error: Option<String>,
    pub wer: WerStats,
    pub terms: TermStats,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleResult {
    pub name: String,
    pub off: Option<Outcome>,
    pub on: Option<Outcome>,
}

pub async fn run_eval(samples: &[Sample], config: &EvalConfig) -> Result<Vec<SampleResult>, ProviderError> {
    let on_client = OpenAiCompatible::new(config.profile.clone(), config.api_key.clone())?;
    let mut off_profile = config.profile.clone();
    off_profile.send_prompt = false;
    off_profile.send_keywords = false;
    off_profile.apply_rules = false;
    let off_client = OpenAiCompatible::new(off_profile, config.api_key.clone())?;
    let no_rules = RuleSet::empty();
    let terms = config.rules.terms();
    let wants_off = config.mode != RulesMode::On;
    let wants_on = config.mode != RulesMode::Off;

    let mut results = Vec::with_capacity(samples.len());
    for sample in samples {
        let (off, on) = match pipeline::prepare(&sample.recording) {
            Ok(audio) => {
                let off = if wants_off {
                    Some(evaluate(&off_client, &no_rules, "", &audio, sample, terms, config.retry_delay).await)
                } else {
                    None
                };
                let on = if wants_on {
                    Some(
                        evaluate(&on_client, &config.rules, &config.prompt_context, &audio, sample, terms, config.retry_delay)
                            .await,
                    )
                } else {
                    None
                };
                (off, on)
            }
            Err(err) => {
                let failed = failed(sample, terms, err.to_string());
                (wants_off.then(|| failed.clone()), wants_on.then_some(failed))
            }
        };
        results.push(SampleResult { name: sample.name.clone(), off, on });
    }
    Ok(results)
}

async fn evaluate(
    client: &OpenAiCompatible,
    rules: &RuleSet,
    prompt_context: &str,
    audio: &PreparedAudio,
    sample: &Sample,
    terms: &[String],
    retry_delay: Duration,
) -> Outcome {
    let ctx = PipelineContext { primary: client, fallback: None, rules, prompt_context, retry_delay };
    match pipeline::transcribe(audio, &ctx).await {
        Ok(transcript) => Outcome {
            wer: wer(&sample.reference, &transcript.text),
            terms: term_hits(&sample.reference, &transcript.text, terms),
            latency_ms: transcript.request_ms,
            hypothesis: transcript.text,
            error: None,
        },
        Err(err) => failed(sample, terms, err.to_string()),
    }
}

fn failed(sample: &Sample, terms: &[String], error: String) -> Outcome {
    Outcome {
        hypothesis: String::new(),
        error: Some(error),
        wer: wer(&sample.reference, ""),
        terms: term_hits(&sample.reference, "", terms),
        latency_ms: 0,
    }
}
```

- [ ] **Step 6: Implement the report**

Insert above the tests in `crates/eval/src/report.rs`:
```rust
use crate::metrics::{TermStats, WerStats};
use crate::run::{Outcome, SampleResult};

const NONE: &str = "–";

pub fn render(results: &[SampleResult]) -> String {
    let mut out = String::from(
        "| sample | WER off | WER on | terms off | terms on | latency on (ms) |\n|---|---:|---:|---:|---:|---:|\n",
    );
    for r in results {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            r.name,
            wer_cell(r.off.as_ref().map(|o| o.wer)),
            wer_cell(r.on.as_ref().map(|o| o.wer)),
            terms_cell(r.off.as_ref().map(|o| o.terms)),
            terms_cell(r.on.as_ref().map(|o| o.terms)),
            r.on.as_ref().filter(|o| o.error.is_none()).map_or(NONE.to_string(), |o| o.latency_ms.to_string()),
        ));
    }

    let off: Vec<&Outcome> = results.iter().filter_map(|r| r.off.as_ref()).collect();
    let on: Vec<&Outcome> = results.iter().filter_map(|r| r.on.as_ref()).collect();
    let mut latencies: Vec<u64> = on.iter().filter(|o| o.error.is_none()).map(|o| o.latency_ms).collect();
    out.push_str(&format!(
        "| **total** | {} | {} | {} | {} | {} |\n",
        wer_cell(total_wer(&off)),
        wer_cell(total_wer(&on)),
        terms_cell(total_terms(&off)),
        terms_cell(total_terms(&on)),
        median(&mut latencies).map_or(NONE.to_string(), |m| m.to_string()),
    ));

    let errors: Vec<String> = results
        .iter()
        .flat_map(|r| {
            [("off", &r.off), ("on", &r.on)].into_iter().filter_map(move |(mode, outcome)| {
                outcome.as_ref().and_then(|o| o.error.as_ref()).map(|e| format!("- {} ({mode}): {e}", r.name))
            })
        })
        .collect();
    if !errors.is_empty() {
        out.push_str("\n**Errors**\n\n");
        out.push_str(&errors.join("\n"));
        out.push('\n');
    }
    out
}

/// Upper median; `None` for an empty list.
pub fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

fn total_wer(outcomes: &[&Outcome]) -> Option<WerStats> {
    (!outcomes.is_empty()).then(|| outcomes.iter().fold(WerStats::default(), |acc, o| acc.add(o.wer)))
}

fn total_terms(outcomes: &[&Outcome]) -> Option<TermStats> {
    (!outcomes.is_empty()).then(|| outcomes.iter().fold(TermStats::default(), |acc, o| acc.add(o.terms)))
}

fn wer_cell(stats: Option<WerStats>) -> String {
    stats.map_or(NONE.to_string(), |s| format!("{:.1}%", s.rate() * 100.0))
}

fn terms_cell(stats: Option<TermStats>) -> String {
    stats.map_or(NONE.to_string(), |s| format!("{}/{}", s.hit, s.expected))
}
```

- [ ] **Step 7: Run the unit tests to verify they pass**

Run: `cargo test -p opit-eval --lib`
Expected: metrics 5 + dataset 2 + report 2 = 9 passed.

- [ ] **Step 8: Write the failing runner integration test**

`crates/eval/tests/run_eval.rs`:
```rust
//! Eval loop against a fake provider.

use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::provider::presets;
use opit_core::rules::{RulePack, RuleSet};
use opit_eval::dataset::Sample;
use opit_eval::report;
use opit_eval::run::{EvalConfig, RulesMode, run_eval};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn tone() -> Recording {
    let samples = (0..16_000).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin()).collect();
    Recording { samples, sample_rate: 16_000, channels: 1 }
}

fn config(server: &MockServer, mode: RulesMode) -> EvalConfig {
    let mut profile = presets::groq();
    profile.base_url = format!("{}/v1", server.uri());
    let pack = RulePack::from_yaml(
        "schema: 1\nid: t\nname: T\nterms: [Claude Code]\ncorrections:\n  Claude Code: [cloud code]\n",
    )
    .unwrap();
    let (rules, _) = RuleSet::compile(&[pack]);
    EvalConfig { profile, api_key: None, rules, prompt_context: String::new(), mode, retry_delay: Duration::ZERO }
}

#[tokio::test]
async fn rules_on_beats_rules_off_on_the_same_audio() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "cloud code'u aç"})))
        .expect(2)
        .mount(&server)
        .await;
    let samples = vec![Sample { name: "a".into(), recording: tone(), reference: "Claude Code'u aç".into() }];

    let results = run_eval(&samples, &config(&server, RulesMode::Both)).await.unwrap();
    let off = results[0].off.as_ref().unwrap();
    let on = results[0].on.as_ref().unwrap();
    assert_eq!((off.wer.errors, on.wer.errors), (1, 0));
    assert_eq!((off.terms.hit, off.terms.expected), (0, 1));
    assert_eq!((on.terms.hit, on.terms.expected), (1, 1));
    assert!(report::render(&results).contains("| a | 25.0% | 0.0% | 0/1 | 1/1 |"));
}

#[tokio::test]
async fn silent_samples_are_reported_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
    let silent = Recording { samples: vec![0.0; 16_000], sample_rate: 16_000, channels: 1 };
    let samples = vec![Sample { name: "s".into(), recording: silent, reference: "bir şey".into() }];

    let results = run_eval(&samples, &config(&server, RulesMode::On)).await.unwrap();
    assert!(results[0].off.is_none());
    assert_eq!(results[0].on.as_ref().unwrap().error.as_deref(), Some("no speech was detected"));
}
```

- [ ] **Step 9: Run it**

Run: `cargo test -p opit-eval --test run_eval`
Expected: 2 passed. The runner already exists (Step 5), so this test must pass the first time it runs. If it fails, the bug is in `run.rs` or in the pipeline wiring, not in the test.

- [ ] **Step 10: Write the CLI**

`crates/eval/src/main.rs`:
```rust
//! `opit-eval`: measure WER and term accuracy of a provider profile on a local dataset.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use opit_core::provider::retry::RETRY_DELAY;
use opit_core::provider::{Profile, presets};
use opit_core::rules::builtin::assemble;
use opit_core::rules::{RuleSet, load_pack_file};
use opit_eval::run::{EvalConfig, RulesMode, run_eval};
use opit_eval::{dataset, report};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Preset {
    Groq,
    Openai,
    Custom,
}

/// Measure WER and term accuracy of a provider profile on a folder of wav+txt pairs.
#[derive(Debug, Parser)]
#[command(name = "opit-eval", version)]
struct Args {
    /// Folder with `name.wav` + `name.txt` pairs.
    #[arg(long)]
    dir: PathBuf,
    #[arg(long, value_enum, default_value_t = Preset::Groq)]
    preset: Preset,
    /// Override the preset's base URL (required for `custom`).
    #[arg(long)]
    base_url: Option<String>,
    /// Override the preset's model (required for `custom`).
    #[arg(long)]
    model: Option<String>,
    /// Transcription language (ISO-639-1 or `auto`).
    #[arg(long, default_value = "tr")]
    language: String,
    #[arg(long, value_enum, default_value_t = RulesMode::Both)]
    rules: RulesMode,
    /// Built-in packs to enable, comma-separated.
    #[arg(long, value_delimiter = ',', default_value = "tr-core,tr-tech")]
    packs: Vec<String>,
    /// Personal rule pack (user.yaml), applied with the highest priority.
    #[arg(long)]
    user_rules: Option<PathBuf>,
    /// Prompt context sentence.
    #[arg(long, default_value = "")]
    context: String,
    /// Environment variable holding the API key (default: GROQ_API_KEY / OPENAI_API_KEY / OPIT_API_KEY).
    #[arg(long)]
    api_key_env: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let profile = build_profile(&args)?;
    let key_var = args.api_key_env.clone().unwrap_or_else(|| default_key_var(args.preset).to_string());
    let api_key = std::env::var(&key_var).ok().filter(|k| !k.trim().is_empty());
    if api_key.is_none() && !matches!(args.preset, Preset::Custom) {
        bail!("set {key_var} to your API key");
    }

    let user = args.user_rules.as_deref().map(load_pack_file).transpose().context("cannot load --user-rules")?;
    let (rules, warnings) = RuleSet::compile(&assemble(user, &args.packs));
    for warning in &warnings {
        eprintln!("warning [{}]: {}", warning.pack_id, warning.message);
    }

    let (samples, skipped) = dataset::load_dir(&args.dir)?;
    for message in &skipped {
        eprintln!("warning: {message}");
    }
    if samples.is_empty() {
        bail!("no wav+txt pairs found in {}", args.dir.display());
    }

    let config = EvalConfig {
        profile,
        api_key,
        rules,
        prompt_context: args.context.clone(),
        mode: args.rules,
        retry_delay: RETRY_DELAY,
    };
    let results = run_eval(&samples, &config).await?;
    print!("{}", report::render(&results));
    Ok(())
}

fn build_profile(args: &Args) -> Result<Profile> {
    let mut profile = match args.preset {
        Preset::Groq => presets::groq(),
        Preset::Openai => presets::openai(),
        Preset::Custom => {
            let (Some(url), Some(model)) = (&args.base_url, &args.model) else {
                bail!("--preset custom needs --base-url and --model");
            };
            presets::custom("custom", "Custom", url, model)
        }
    };
    if let Some(url) = &args.base_url {
        profile.base_url = url.clone();
    }
    if let Some(model) = &args.model {
        profile.model = model.clone();
    }
    profile.language = args.language.clone();
    Ok(profile)
}

fn default_key_var(preset: Preset) -> &'static str {
    match preset {
        Preset::Groq => "GROQ_API_KEY",
        Preset::Openai => "OPENAI_API_KEY",
        Preset::Custom => "OPIT_API_KEY",
    }
}
```

- [ ] **Step 11: Smoke-test the CLI without a network**

Run: `cargo run -q -p opit-eval -- --help`
Expected: usage text listing `--dir`, `--preset`, `--rules`, `--packs`, `--user-rules`.

Run: `cargo run -q -p opit-eval -- --dir . --preset custom`
Expected: exits non-zero with `--preset custom needs --base-url and --model`.

- [ ] **Step 12: Write the docs**

`docs/eval.md`:
````markdown
# Measuring accuracy with `opit-eval`

`opit-eval` sends a folder of recordings to a provider and reports word error rate (WER) and
term accuracy, with the rule layer **off** (raw provider output) and **on** (prompt + rules).
Use it to pick a default model and to check that a rule actually helps before keeping it.

## Build a dataset

1. Create a folder outside the repository (or `eval-data/`, which is git-ignored).
2. Record short clips the way you really dictate: 5–30 s each, your usual microphone,
   mixed Turkish and technical terms. Any WAV works (the tool resamples to 16 kHz mono).
3. Next to each `clip.wav`, write `clip.txt` with exactly what you said, spelled the way you
   want it pasted (`Claude Code'u GitHub'a pushla`).
4. 20–50 clips give a useful signal; include the terms your rule packs care about.

Recordings of your voice are personal data. Never commit them.

## Run

```sh
# Groq (default preset), rules on and off, built-in packs tr-core + tr-tech
GROQ_API_KEY=gsk_... cargo run --release -p opit-eval -- --dir eval-data

# Add FiveM terms, your personal pack and a prompt context
cargo run --release -p opit-eval -- --dir eval-data \
  --packs tr-core,tr-tech,fivem \
  --user-rules "$APPDATA/opit-speech-to-text/rules/user.yaml" \
  --context "Türkçe yazılım geliştirme ve FiveM sunucusu üzerine konuşma."

# Compare models
cargo run --release -p opit-eval -- --dir eval-data --model whisper-large-v3-turbo
OPENAI_API_KEY=sk-... cargo run --release -p opit-eval -- --dir eval-data --preset openai

# A self-hosted OpenAI-compatible server
cargo run --release -p opit-eval -- --dir eval-data --preset custom \
  --base-url http://192.168.1.10:8888/v1 --model large-v3
```

## Read the table

| Column | Meaning |
|---|---|
| WER off / on | Word error rate after case folding and punctuation removal; lower is better |
| terms off / on | Term occurrences spelled exactly as in the packs / occurrences in the reference |
| latency on | Request time in ms (median in the total row) |

A rule earns its place when "on" beats "off" on WER or terms without making another clip worse.
If a rule never fires on your dataset under the current model, delete it.
````

In `README.md`, add a row to the layout table:
```markdown
| `crates/eval` | `opit-eval`: WER / term accuracy of a provider profile on your own recordings |
```
and a section before `## License`:
```markdown
## Measuring accuracy

See [`docs/eval.md`](docs/eval.md) for building a personal dataset and running `opit-eval`.
```

- [ ] **Step 13: Full verification**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: fmt prints nothing, clippy has no warnings, every test in both crates passes.

- [ ] **Step 14: Commit**

```bash
git add .gitignore README.md docs/eval.md crates/eval Cargo.lock
git commit -m "feat(eval): add opit-eval CLI with rules on/off report

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 15 (manual, needs the user's API key): real-provider baseline**

This step is not required for the plan to be complete, but its result feeds the default-model decision in brief §6. With a dataset in `eval-data/`, run the Groq default, the turbo model and OpenAI `gpt-transcribe` as shown in `docs/eval.md`, with `--packs tr-core,tr-tech,fivem` and `--user-rules` pointing at the personal pack. Save the three Markdown tables to `docs/eval-baseline.md`. That file holds metrics only, never transcripts.
