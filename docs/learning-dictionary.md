# Learning dictionary: backend and UI contract

History edits can offer personal dictionary additions. Saving an edit never adds a
rule or a keyword. Only an explicit confirmation followed by `learning_apply`
does that. UI implementation is separate; this package does not change `ui/`.

## Configuration

New `config.json` section, also returned by `get_config` and `config-changed`:

```json
{ "learning": { "enabled": true } }
```

`learning.enabled` is a boolean, default `true`. Missing `learning` or an empty
object loads with this default through serde defaults. No config schema bump is
needed. The switch controls offering suggestions only: history edits, explicit
apply/dismiss operations and previously saved personal rules remain available.
Use the existing serialized `saveConfig` path to save the setting.

## Commands

All three new commands are asynchronous and run their blocking work through
`spawn_blocking`. JS argument names below are exact. All errors have the existing
`CommandError { code: string, message: string, kind: null }` shape. Messages never
include transcript content, terms or preference-file contents.

### `history_update_text`

JS arguments: `{ id: number, text: string }`.

Return type: `HistoryUpdate`:

```ts
interface TermChange { heard: string; corrected: string }
interface HistoryUpdate { dictation: Dictation; suggestions: TermChange[] }
```

`Dictation` JSON is **unchanged**. `text: string` is now editable and
`raw_text: string` remains the original recognized text. No new `original_text`
field or database migration is necessary. Both fields remain available through
`history_recent`, `history_search` and this command. Other existing fields retain
their types and semantics: `id`, `created_at_ms`, `profile_id`, `status`,
`audio_ms`, `latency_ms`, `audio_path`, `error_code`, `language`, `cleanup_status`,
`cleanup_model`, `cleanup_style_id`, `source_text`, `instruction`, `mode`, `agent`.
An edit changes only `text` and its FTS search data; raw text remains searchable.
Audio, recording date and processing metadata are retained.

Validation the UI must mirror:

- `id` must be positive; the row must exist and have `status: "ok"`.
- `text` must contain non-whitespace content, have no NUL character and be at most
  **100000 UTF-8 bytes**, measured before trimming. The exact supplied text is saved.
- Failed or filtered rows cannot be edited into successful rows.

Errors: `invalid_input` (validation/status), `unavailable` (missing row/history or
worker failure), `history` (SQLite failure), `learning` (preferences cannot be
read or are invalid). Preference read failures leave the edit unsaved, allowing a
safe retry. With learning disabled, preferences are not read.

Suggestions compare the last saved `text` with the new text, not `raw_text`.
Identical saves return no suggestions. There are at most eight unique pairs,
each containing one to four Unicode words on both sides. Tokenization includes
letters, numbers, underscores and combining marks. Turkish I/ı/İ/i folding matches
the rule engine. Pure punctuation/whitespace changes, pure insertions/deletions
and changes only to a sentence-initial capital are ignored. Meaningful inner
case changes such as `github` → `GitHub` remain suggestions, including at the
start of a sentence. Internal punctuation and normalized single spaces are kept
in returned replacement spans. Common unchanged prefixes/suffixes are removed
before alignment. Broad rewrites needing more than 4000000 alignment cells
produce no suggestions; the history edit still saves.

Enabled compiled rules (including replacements and term casing), remembered
pairs and `learning.enabled` filter the results. A keyword alone does not imply
that an unrelated heard spelling is a correction variant. Case-only suggestions
are considered covered only when rules produce the exact corrected spelling.

### `learning_apply`

JS arguments:

```json
{
  "items": [
    { "heard": "zeta wrong", "corrected": "ZetaSuite", "target": "rule" },
    { "heard": "omega wrong", "corrected": "OmegaSuite", "target": "term" }
  ]
}
```

`target` is exactly `"rule" | "term"`; it has no implicit default. Return type:
`RuleWarning[]`, using the existing `{ pack_id: string, message: string }` shape.
These are compilation warnings, not provider error kinds.

- `rule` reuses `RulePack::add_correction(corrected, heard)`. Case-only pairs
  become canonical terms, matching the existing manual correction flow.
- `term` saves the corrected spelling only, without adding a heard variant.
- Both choices preserve the leading comment block in `user.yaml`, compile the
  saved rules immediately and remember the confirmed pair so it is not offered
  again. Retries are idempotent.
- Casing-only term duplicates are prevented with Turkish-aware folding. An
  existing `github` is updated in place to confirmed `GitHub`, not appended.
  The same fix applies to existing `correction_draft` case-only additions.
- Batch validation and modification occur before writing either file; conflicts
  reject the entire batch. Config/rule/dismiss operations are serialized to avoid
  lost read-modify-write updates.

Errors: `invalid_input` (batch/pair validation or a heard spelling already assigned
to another canonical correction), `rules` (personal YAML parse/render/save
failure), `learning` (preferences invalid, full or unavailable), `unavailable`
(worker failure). Existing `correction_draft` now reports correction conflicts
without including either spelling in its error message; its API is unchanged.

### `learning_dismiss`

JS arguments: `{ items: TermChange[] }`. Return type: `null` (`void`). Records the
pairs without modifying personal rules or history. Errors: `invalid_input`
(batch/pair validation), `learning` (preferences invalid/full/unavailable),
`unavailable` (worker failure).

Shared apply/dismiss validation:

- A batch contains **1–8** items.
- Each side is normalized to single whitespace-separated spaces, is **1–4
  Unicode word tokens**, is at most **256 Unicode scalar characters** and has no
  remaining control characters. Normalized sides must not be exactly identical.
- Do not use JS string length as a byte/scalar count: use `TextEncoder` for UTF-8
  bytes and `[...value].length` for scalar counts, respectively.

## Events and UI integration

New events:

| Event | JSON payload | When / UI action |
|---|---|---|
| `history-updated` | row ID (`number`) | After a successful history text edit; refresh that row and any active search. No transcript is broadcast. |
| `rules-changed` | `null` | After successful `save_user_rules`, and after every completed `learning_apply` attempt (including errors, since a partial disk failure can have saved the YAML). Reload rules and prompt budget while preserving unsaved editor drafts. |

`save_user_rules` arguments and return type are unchanged. Previously there was
no Rules-page invalidation event; the UI agent should subscribe to
`rules-changed`. Existing `history-added` and `config-changed` payloads are
unchanged. `config-changed` now includes the defaulted `learning` section.

After saving a full transcript, show at most one confirmation dialog for the
returned suggestions. Let the user select each item and its rule/term target;
call `learning_apply` only after explicit confirmation. Call `learning_dismiss`
for rejected/unselected pairs and on dialog dismissal, and await persistence.
Do not re-open the same offer from a refresh event. If a persistence operation
fails, keep the choices available for retry instead of claiming they were saved.

## Persistence and operational boundary

`<data_dir>/learning.json` stores only remembered small pairs (dismissed and
confirmed), with this internal shape:

```json
{ "pairs": [{ "heard": "zeta wrong", "corrected": "ZetaSuite" }] }
```

Pairs are deduplicated by whitespace normalization and Turkish-aware folding.
The file is loaded on demand, including after restart, and replaced via a
temporary file. It is capped at **4096** distinct pairs; new choices at capacity
fail with `learning` instead of evicting an older choice and offering it again.
Existing choices can still be confirmed/dismissed idempotently. Invalid files
are retained and reported with a content-free error.

`user.yaml` and `learning.json` are separate atomic file replacements, not a
single cross-file transaction. If saving preferences fails after saving YAML,
the error explicitly says personal rules were saved and asks for a retry. The
rules event still fires. The same apply can be retried without duplicate rules
or terms. A crash in this narrow interval can leave an approved keyword pair
unremembered; it may be offered again, but never silently applied.

Automated coverage includes Unicode/Turkish diff cases, caps, enabled/disabled
filtering, rule/term/replacement coverage, old config loading, persistent FTS
edits, remembered choices after restart, header preservation, term deduplication,
batch conflict rejection, concurrent applies and partial disk failure/retry.

Manual acceptance after the UI is connected: record a real dictation, correct it
in History, verify the original recognized text remains available, reject an
offer and restart, then confirm rule and keyword choices and inspect Rules and
the prompt budget. Verify the learned rule on another microphone dictation and
verify keyword transmission only on profiles with keywords enabled. Repeat with
learning disabled and with an unsaved Rules editor draft during a rules event.
