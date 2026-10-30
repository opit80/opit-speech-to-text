# Cleanup styles: backend and UI contract

Cleanup stays opt-in (`cleanup.enabled: false` by default). A successful dictation also
requires its successful provider profile's `apply_cleanup` flag and the existing cleanup
eligibility checks. Selecting a style never enables cleanup or changes that profile flag.

## Config additions and migration

These fields are added to the existing config; all existing connection fields remain shared:
`base_url`, `api_key_ref`, `timeout_seconds`, `reasoning_effort`, and `model`.
Missing objects/fields load through serde defaults. There are no new config enums.

Default JSON fragment (other existing fields omitted):

```json
{
  "cleanup": {
    "active_style_id": "clean",
    "prompt_override": "",
    "styles": [
      {"id":"raw","name":"Raw","builtin":true,"prompt":"","model":null,"in_cycle":true,"hotkey":[]},
      {"id":"clean","name":"Clean","builtin":true,"prompt":"","model":null,"in_cycle":true,"hotkey":[]},
      {"id":"formal","name":"Formal","builtin":true,"prompt":"","model":null,"in_cycle":true,"hotkey":[]},
      {"id":"casual","name":"Casual","builtin":true,"prompt":"","model":null,"in_cycle":true,"hotkey":[]},
      {"id":"prompt","name":"AI prompt","builtin":true,"prompt":"","model":null,"in_cycle":true,"hotkey":[]}
    ]
  },
  "hotkey": {"style_cycle_keys":[]}
}
```

`styles` is an ordered array of objects. Each style has:

| Field | JSON type | Default / rules |
| --- | --- | --- |
| `id` | string | Required for a retained entry; fixed built-in id or `custom-` followed by at least one ASCII `[a-z0-9-]` character; at most 48 bytes/characters total. |
| `name` | string | Editable; blank/whitespace-only normalizes to the English built-in name or `Custom`. |
| `builtin` | boolean | Derived from the id during normalization; users cannot turn custom ids into built-ins. |
| `prompt` | string | `""`; whitespace-only also selects the language-specific built-in default. Empty custom prompts use the clean default. |
| `model` | string or null | `null`; blank/whitespace-only becomes null, other values are trimmed. Null/blank inherits `cleanup.model`. |
| `in_cycle` | boolean | `true`, including raw and custom entries; false excludes only cycling, not direct selection. |
| `hotkey` | string array | `[]` disables direct selection. Uses the existing key names and validation below. |

`cleanup.active_style_id` is a string defaulting to `"clean"`; unknown ids normalize to
`"clean"`. `hotkey.style_cycle_keys` is a string array defaulting to `[]` (off).

Normalization retains the first valid instance of an id, drops invalid/empty/duplicate ids,
and appends missing built-ins in `raw`, `clean`, `formal`, `casual`, `prompt` order. Built-ins
cannot be deleted; preserve them when saving UI drafts. Existing ordering and editable
fields survive normalization. Migration moves a nonblank legacy `prompt_override` into
the clean style when that style has no explicit prompt, then clears `prompt_override`.
The legacy field remains deserializable but is not consulted during prompt selection.
New UI code should edit `styles[].prompt`, leaving `prompt_override` empty.

Raw always skips cleanup, including credential lookup and the chat request, regardless of
its edited prompt/model. History reports `cleanup_status: "skipped"` and no cleanup model.
Clean uses the exact pre-existing EN/TR prompts. Formal produces polite, complete,
email-ready prose; casual keeps a relaxed voice; prompt structures the given requirements
as an AI-assistant prompt without answering them. All nonraw defaults retain the existing
transcript-as-data safety framing, prohibit translation and append the shared glossary.
Detected language, successful profile language and UI language retain their existing priority.

## Commands

All command failures use `{code: string, message: string, kind: string | null}`.

| Command | JS arguments | Return | Error codes |
| --- | --- | --- | --- |
| `set_active_style` | `{id: string}` | Saved, normalized `AppConfig` | `invalid_input` (unknown id or invalid current settings), `config` (persistence), `unavailable` (blocking worker failure). `kind` is null. |
| `cleanup_style_defaults` | none | `StyleDefaults[]`, shape below | Infallible. |
| `validate_hotkey` (extended) | `{keys: string[], target?: string, config?: AppConfig}` | null | `invalid_input` (invalid keys/target, missing style, collision or reserved Escape). `kind` is null. |

`cleanup_style_defaults` returns five entries in fixed built-in order:

```ts
type StyleDefaults = {
  id: 'raw' | 'clean' | 'formal' | 'casual' | 'prompt';
  names: {en: string; tr: string};
  prompts: {en: string; tr: string};
};
```

Raw prompts are empty strings. Other entries contain the full default texts. Show those
texts as placeholders/previews and reset by saving `prompt: ""`, rather than copying a
translated prompt into the config. Names in stored defaults are English. Native tray and
overlay labels localize an unchanged English built-in name; any other name is used verbatim.

`validate_hotkey` without `target` retains its original syntax-only, nonempty behavior.
For full validation pass `target` and optionally the complete unsaved config draft:

- `dictation` edits `hotkey.keys` (nonempty required).
- `pause` edits `hotkey.pause_keys` (empty disables).
- `ask` edits `hotkey.ask_keys` (empty disables).
- `style_cycle` edits `hotkey.style_cycle_keys` (empty disables).
- `style:<id>` edits that entry's `hotkey` (empty disables).

With no `config`, validation uses current saved settings. It replaces only the addressed
shortcut in a temporary copy, checks all shortcuts and performs no writes. Key names are
trimmed and case-insensitive; comparison uses deduplicated virtual-key sets, disregarding
order. Equal sets and subset/superset pairs are conflicts. Optional shortcuts cannot contain
Escape. A save rejects conflicts before normalization and leaves disk/live settings intact.
On load/explicit normalization, an invalid main shortcut becomes the existing default;
invalid/conflicting optional shortcuts are cleared in priority order: dictation, pause,
ask, style cycle, then direct style shortcuts in array order. The UI must mirror these rules.

Existing `save_config` persists style edits and rebuilds shortcut registration only when
the action-to-key mappings change. Name, prompt, model, cycle membership and array-order
edits keep active trackers intact; unchanged key combinations preserve their pending release
even when another shortcut changes. Changing the active id through `save_config` also updates a live recording.
`test_cleanup({config})` retains its `{before: string, after: string}` result and existing
error contract; it tests the selected style/model. Raw returns the unchanged sample with
no key lookup or HTTP call. `cleanup_presets` returns the additional style fields in every
preset config. The existing `cleanup_prompts` command still returns the clean EN/TR prompts.

## Events and history

No new event names. `config-changed` retains its full `AppConfig` payload, now including
the fields above, and fires after a successful command/tray/hotkey style switch. Existing
`history-added` and dictation status payloads are unchanged. The tray menu refreshes after
config changes, including style addition/removal/rename/hotkey/cycle edits.

History schema **6** adds nullable `cleanup_style_id TEXT`. Every serialized `Dictation`
returned by `history_recent` / `history_search` now also has:

```ts
cleanup_style_id: string | null;
```

Existing rows migrate to null. New dictation rows store the selected style id even when
cleanup is disabled/skipped/failed; Ask rows use null. History retry uses the style chosen
at retry start and replaces the column along with the result. Ids may refer to a style
deleted since recording, so the UI must tolerate unknown ids. Migration preserves search,
audio paths, ids and recording dates.

## Recording, cycling and native feedback

The complete style object (including prompt/model) is snapshotted at dictation start and
carried with the job independently of the paste snapshot. Unrelated config edits do not
change an existing job. Explicit style selection while recording replaces only that
recording's style; queued/transcribing jobs retain their own snapshots. Shared connection
settings and profile eligibility retain their start-of-job snapshots.

Cycle starts from the recording's style when recording, otherwise the global active style.
It follows config array order, wraps, and ignores `in_cycle: false`. If the current style
is excluded, the first included style is selected; an empty cycle does nothing. All five
built-ins are included by default. Direct hotkeys select regardless of `in_cycle`.
Style shortcuts use the existing global hotkey enabled/pause/capture state. Saving a style
selection applies immediately to the recording and runs through a separate FIFO persistence
worker. Stop, Escape, shutdown and the recording limit never await a style write or the config
lock. Rapid cycle/direct presses retain their logical order even while a save is blocked;
new recordings use the latest pending choice, before foreground-app overrides. Completion
messages cannot replace a newer choice or another recording's snapshot. A failed latest save
restores the last confirmed style only for the same recording; queued/transcribing jobs keep
the choice they captured. If a tray/UI selection overtakes queued writes, its choice is
appended to the writer so it remains the final persisted selection. The writer drains queued
requests after controller shutdown while the runtime remains available. The hook uses one list of registered auxiliary actions
for pause, Ask, cycle and direct style selection; autorepeat does not select twice and a
style Ctrl release does not produce a lone-Ctrl dictation stop.

The tray has a localized Style/Stil submenu with the active style checked. While recording,
the selected name stays inline in the recording overlay, preserving its timing, level and
pause state. Without a recording, switching displays the name for 1.5 seconds. Style choice
does not change the separate Ask request-cleanup behavior, which uses clean. File import
retains its subtitle formatting rules; raw skips its optional cleanup and model overrides
are honored, while named prose prompts apply to live dictation and the cleanup test.

## Manual acceptance

Use a throwaway `OPIT_DATA_DIR` and an existing key in Credential Manager. Verify actual
WH_KEYBOARD_LL input while idle, recording and paused (toggle and push-to-talk), direct
selection, cycle order/exclusions, and Escape/capture behavior. Check tray checkmarks,
English/Turkish labels and renamed/custom styles, including overlay timing and truncation
at different DPI settings. Speak EN/TR samples into a text editor for each style, verify
raw makes no cleanup request, verify provider model overrides, then restart and inspect
the persisted selection and History. Exercise a failed config save and a version-5 history
database. Real microphone/desktop/provider behavior is outside the fake and local-HTTP
automated test boundary.

## Changed files and automated verification

Historical Windows verification snapshot, 2026-10-28.

Added:

- `crates/core/src/cleanup/styles.rs`
- `crates/core/src/hotkey.rs`
- `crates/core/tests/cleanup_styles_http.rs`
- `docs/reference/cleanup-styles.md`

Modified:

- Core: `crates/core/src/cleanup/mod.rs`, `config.rs`, `history/mod.rs`, `lib.rs`.
- App: `crates/app/src/app_core.rs`, `commands.rs`, `controller/mod.rs`, `controller/tests.rs`,
  `events.rs`, `file_jobs.rs`, `history_service.rs`, `i18n.rs`, `lib.rs`, `platform/fake.rs`,
  `platform/keys.rs`, `platform/mod.rs`, `platform/windows/hotkey.rs`, `tray.rs`, `tray_menu.rs`.

All commands ran sequentially from the repo root with
`CARGO_TARGET_DIR=.superpowers/cargo-target` and `CARGO_NET_OFFLINE=true`:

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, with these Cargo test batches:

| Crate | Passed batches | Ignored |
| --- | --- | --- |
| `opit-cli` | main: 3 | 0 |
| `opit-core` | lib: 212; cleanup_http: 3; cleanup_styles_http: 1; pipeline_http: 3 | 0 |
| `opit-eval` | lib: 9; main: 0; run_eval: 2 | 0 |
| `opit-ipc` | lib: 6 | 1 |
| `opit-speech-to-text` | lib: 267; main: 0; logging_init: 1; tauri_conf: 7; updater_artifacts: 0; updater_live: 0 | lib: 11; updater_artifacts: 1; updater_live: 1 |

All four library doc-test batches have zero tests. Total: **514 passed, 14 ignored, 0 failed**.
This 2026-10-28 Windows verification summary is retained from private local output; the log
is not a public artifact. It does not establish the real microphone/desktop/provider checks above.
`git diff --check` passed and `ui/` has no diff. No new dependencies, git writes, commits,
branch switches, pushes, tags or releases were performed.
