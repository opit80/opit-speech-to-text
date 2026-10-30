# Voice edit and translation

Package 4 adds optional recording shortcuts for editing selected text and translating
dictation. Both shortcuts are disabled by default. No UI files are part of this package.

## Changed files

```text
crates/core/src/voice_modes.rs (new)
crates/core/src/lib.rs
crates/core/src/config.rs
crates/core/src/cleanup/mod.rs
crates/core/src/history/mod.rs
crates/app/src/app_core.rs
crates/app/src/commands.rs
crates/app/src/controller/mod.rs
crates/app/src/controller/tests.rs
crates/app/src/history_service.rs
crates/app/src/shell/i18n.rs
crates/app/src/platform/mod.rs
crates/app/src/platform/fake.rs
crates/app/src/platform/keys.rs
crates/app/src/platform/windows/clipboard.rs
crates/app/src/platform/windows/paster.rs
docs/reference/voice-modes.md (new)
```

## Config contract

The new fields in `AppConfig` serialize with these exact defaults:

```json
{
  "hotkey": {
    "edit_keys": [],
    "translate_keys": []
  },
  "translate": {
    "target_language": "en",
    "model": null
  }
}
```

This is a partial config showing additions, not a replacement for the existing config.
`hotkey.edit_keys` and `hotkey.translate_keys` are `string[]`; empty disables that mode.
Both share `hotkey.enabled` and `hotkey.mode` (`"toggle"` or `"push_to_talk"`). The same
mode shortcut stops its recording; push-to-talk stops on that shortcut's release. Releasing
another mode's shortcut does not stop it. Escape cancels and the recording pause shortcut
continues to work. Releasing push-to-talk during preparation cancels preparation.

`translate.target_language` is a string with allowed values
`en`, `tr`, `de`, `fr`, `es`, `it`, `pt`, `ru`, `ar`, `ja`, `ko`, `zh`, `nl`, `pl`, `uk`, `az`.
Normalization trims and lowercases it; unsupported or empty values become `en`.
`translate.model` is `string | null`; null uses `cleanup.model`. Normalization trims it
and converts a blank string to null. Defaults are applied to absent fields and partial
objects via `#[serde(default)]`. The config schema version remains 1.

Both modes use `cleanup.base_url`, `api_key_ref`, `timeout_seconds` and `reasoning_effort`.
Edit uses `cleanup.model`; translation uses the optional model override. `cleanup.enabled`
and every style's prompt/model are ignored. A valid connection and a stored nonblank key
when `api_key_ref` is non-null are required before opening the microphone. A keyless local
connection uses `api_key_ref: null`. Connection validation does not make a test HTTP request;
an expired or rejected key is detected when the LLM request runs.

At shortcut press, per-app profile/fallback and paste overrides are resolved and snapshotted.
Changes during recording do not affect that job. Per-app cleanup style overrides are ignored.
The normal speech pipeline still applies the successful profile's rule settings, quick retry,
fallback and patient retry ladder. Translation skips cleanup styles before translating.

### Shortcut validation

Use the existing `validate_hotkey` command with `target: "edit"` or `target: "translate"`.
Pass the complete draft config to validate unsaved shortcuts against each other:

```ts
invoke<void>("validate_hotkey", { keys: ["F18"], target: "edit", config: draftConfig });
```

The existing `keys`, `target`, and `config` JS argument names are unchanged. Return is null
on success; rejection has `CommandError { code: "invalid_input", message: string, kind: null }`.
Key parsing is case-insensitive and uses the existing supported key names. Empty keys are
allowed for auxiliary shortcuts. Unknown keys, Escape, and equal/subset/superset key
combinations with any enabled shortcut are rejected. This includes main dictation, pause,
Ask, style cycle, both new modes and individual style shortcuts. Unrelated combinations
may share modifiers. `save_config({ config })` applies the same validation before writing.
Startup normalization retains the first valid shortcut and clears later conflicts, in this
order: main, pause, Ask, style cycle, edit, translate, styles in config order.

## Commands and events

- New async command `translate_languages()`, no JS arguments, returns
  `Array<{ code: string, name_en: string, name_tr: string }>` in the code order above.
  It does no I/O and has no application error codes. The UI should use these labels directly.
- `get_config()` and `save_config({ config })` return `AppConfig` with the new fields above.
  `get_config` has no application error codes.
  Existing `save_config` errors remain unchanged (`invalid_input`, `config`, `unavailable`).
- `validate_hotkey` adds the two targets above; no signature or return-shape changes.
- `history_recent({ limit, beforeId })`, `history_search({ query, limit })`, and
  `history_retry({ id })` return the expanded `Dictation` type below. History retry rejects
  edit/translate rows with `invalid_input`, even when retained audio exists. Their safe
  retry action is to record the instruction again; the original text stays retrievable.
  Recent/search return `Dictation[]`, retry returns one `Dictation`. Recent/search reject
  with `history` or `unavailable`; retry additionally has `invalid_input` and `provider`.
  Every rejection uses `{ code: string, message: string, kind: ErrorKind | null }`.
- There are no new event names. `config-changed` carries the full expanded `AppConfig`.
  `history-added` still carries only the numeric row ID; reload History to obtain content.
  `dictation-status` keeps its existing shape:
  `{ state, can_retry, waiting, paused, retry_wait? }`, plus `kind` and `message` when
  `state === "error"`. No mode or private text fields are added to status events.
  Preparation uses `transcribing`, LLM work uses `cleaning`, followed by `pasting`/`idle`.
  LLM failures emit `error` with `kind: "rejected"`, then resume the queue. Missing connection
  emits `missing_key`; unsafe/oversized selection emits `rejected`, before any recording.
  Existing speech, microphone and paste errors retain their existing kinds.

### Dictation JSON

Every property below is always serialized, including null values. Numbers are JSON numbers;
integer milliseconds and IDs retain their existing meanings.

```ts
type Dictation = {
  id: number;
  created_at_ms: number;
  profile_id: string;
  raw_text: string;
  text: string;
  status: "ok" | "empty" | "hallucination" | "failed";
  audio_ms: number;
  latency_ms: number;
  audio_path: string | null;
  error_code: string | null;
  language: string | null;
  cleanup_status: "ok" | "failed" | "skipped" | null;
  cleanup_model: string | null;
  cleanup_style_id: string | null;
  mode: "dictation" | "ask" | "edit" | "translate";
  agent: string | null;
  source_text: string | null;
  instruction: string | null;
};
```

`source_text` and `instruction` are the new nullable columns in history schema 7.
Existing rows retain their mode and other data; both added fields load as null.
`mode` was already non-null (`dictation`/`ask`); its allowed outputs now include `edit`
and `translate`. For edit, `source_text` is the exact original selection (null for generation),
`instruction` is the rules-applied spoken instruction, and `text` is the LLM result.
For translation, `source_text` is the rules-applied source transcript, `instruction` is null,
and `text` is the translation. `raw_text` remains the provider's pre-rule output, while
`language` remains the source speech language. New modes have `agent` and `cleanup_style_id`
null, and `cleanup_model` records the effective connection model.

LLM failures save `status: "failed"`, `text: ""`, `raw_text: ""`,
`cleanup_status: "failed"`, `error_code: "rejected"`, retaining `source_text` and
`instruction`. Speech recognition failures may have no recognized instruction/source yet,
but still retain the edit's selection. No instruction or untranslated source is ever pasted
as a substitute for a failed result. Audio retention follows `history.save_audio`; these
rows do not offer the ordinary dictation audio retry. `history.enabled: false` disables
storage of selections, instructions, results and audio. Existing History search indexes
result/pre-rule text; selection and instruction remain retrievable through returned rows.

## Selection and output boundaries

Windows selection capture runs on a blocking worker before the microphone opens. It requires
a complete, restorable clipboard snapshot, notes its sequence number, sends Ctrl+C, and polls for
up to 400 ms. It reads only newly copied Unicode text and restores the snapshot before
recording. Own windows, shell windows, elevated windows and unreadable integrity are treated
as having no selection. A simultaneous unrelated user copy is preserved and capture fails
safely rather than treating that clipboard content as the selection. Selection capture and
paste/restore share a lock. Held shortcut modifiers are neutralized and restored in the
same injected input batch, supporting modifier-based push-to-talk.

Every advertised clipboard format must be supported and successfully backed up. Unsupported
formats (including enhanced metafiles and other GDI handles), failed retrieval/locking, or
enumeration errors make selection unavailable before Ctrl+C is sent. The original clipboard
is left intact, Edit shows the existing selection-unavailable hint, and recording does not start.
This safety check applies even when `paste.restore_clipboard` is false; that setting controls
only restoration after inserting a result.

Selections are limited to 32,000 Unicode characters. A read/restore failure or oversized
selection stops the operation and shows a localized hint; it does not silently generate text.
An absent Unicode text format means no text selection. If Unicode text is advertised but
retrieval fails, capture instead restores the backup and reports selection unavailable.
An unchanged clipboard after the copy timeout means no selection, so Edit generates new text.
The selection and instruction are separate JSON values in the LLM user message; the system
prompt explicitly makes selection material non-instructional. Edit/generation prompts use
the instruction's detected language, then successful profile language, then UI language,
with Turkish and English prompt variants. Translation prompts use a whitelisted English
language name and preserve names, technical terms and code.

Generation answers are capped at 8,000 Unicode characters. Edit and translation answers are
capped at `clamp(source_characters * 4, 4000, 128000)`. Empty and truncated/invalid API answers
are refused. These limits are separate from cleanup's existing `3 * input + 120` limit.
The chat request uses the shared timeout/reasoning settings and a 16,384-token output budget.
Successful results use the ordinary trailing-space, paste method, post-insert key,
clipboard restoration, media guard and foreground auto-paste policy.

## Manual acceptance

Use a scratch `OPIT_DATA_DIR` and the user's configured cleanup connection for these checks:

1. Select text in Notepad, an editor and a browser; run Edit, speak an instruction, and verify
   the selected text is replaced, original clipboard formats restored, and both original and
   instruction available in History. Repeat without a selection for generation.
2. Verify toggle and push-to-talk with function-key and modifier shortcuts; check pause,
   Escape during preparation/LLM work, and the localized Edit/Translate overlay labels.
3. Translate into several scripts (including Turkish, German, Arabic and Japanese). Verify
   model override, terminology, code, per-app profile/paste overrides, and media pause/duck.
4. Reject/misconfigure the cleanup key, disconnect the network and force a timeout. Verify
   no source/instruction is pasted, the failed History row retains original material, and
   the next queued dictation proceeds.
5. Test elevated and own windows, focus changes, concurrent user copies, busy clipboard,
   long selections, and rich/image clipboard payloads. Selection capture requires a complete
   HGLOBAL-backed backup; verify that GDI/uncopyable formats abort capture before Ctrl+C and
   leave the clipboard intact, including with paste restoration disabled. An advertised
   Unicode format that cannot be retrieved must restore the backup and show the existing
   selection-unavailable hint instead of starting generation.
   Applications using clipboard brokers may fail the owner-process check safely. Clipboard
   owners that block delayed rendering can exceed the nominal polling time.

No real microphone, interactive clipboard, installed app, or live provider is exercised by
the automated suite. No new dependencies, commits, branch changes, pushes or releases are
required for this package.

## Automated verification

Historical Windows verification snapshot, 2026-10-28.

The final checks ran sequentially on Windows with `CARGO_NET_OFFLINE=true` and
`CARGO_TARGET_DIR=.superpowers/cargo-target`:

```text
cargo fmt --all --check                              PASS
cargo clippy --workspace --all-targets --offline -- -D warnings  PASS
cargo test --workspace --offline                     PASS
```

| Crate / suite | Passed | Ignored |
| --- | ---: | ---: |
| opit-cli | 3 | 0 |
| opit-core unit | 222 | 0 |
| opit-core cleanup_http / cleanup_styles_http / pipeline_http | 3 / 1 / 3 | 0 |
| opit-eval unit / run_eval | 9 / 2 | 0 |
| opit-ipc unit | 6 | 1 |
| opit-speech-to-text unit | 283 | 12 |
| app logging_init / tauri_conf | 1 / 7 | 0 |
| app updater_artifacts / updater_live | 0 / 0 | 1 / 1 |

All main-target and doc-test suites with zero tests also passed. Total: 540 passed,
15 ignored, zero failed. These are the 2026-10-28 Windows results from private local output,
not downloadable logs or proof of the native microphone/clipboard/provider acceptance above.
The initial new core prompt/limit tests failed before implementation, then passed. Coverage
includes old config defaults and normalization, shortcut conflicts and release events,
schema migration/reopening, edit with/without selection, LLM failure without paste,
translation model/rules, per-app fallback/paste, media release, pause, preparation cancellation,
early microphone failure and shutdown during preparation. The native i18n inventory checks
all added strings in both languages.
