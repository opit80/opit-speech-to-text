# Per-app rules

Microphone dictation resolves the foreground application before creating provider clients or
starting capture. The first enabled matching rule overrides a private settings snapshot. Changing
focus or saving configuration afterwards does not change that recording or queued job. Global
`active_profile_id`, `cleanup.active_style_id` and `paste` settings are never written by matching.
The recording overlay uses the existing style label for the effective style.

## UI contract

New top-level config field, with its default:

```json
{
  "app_rules": {
    "enabled": true,
    "rules": []
  }
}
```

Each rule has this wire shape (the values shown are its field defaults before normalization):

```json
{
  "id": "",
  "enabled": true,
  "exe": "",
  "title_contains": "",
  "style_id": null,
  "profile_id": null,
  "paste_method": null,
  "paste_after": null
}
```

| Field | Type | Meaning / allowed values |
| --- | --- | --- |
| `app_rules.enabled` | boolean | Master switch, default `true`. |
| `app_rules.rules` | array of rule objects | Priority is array order, default `[]`. |
| `id` | string | Stable rule identifier; unique and nonempty after normalization. |
| `enabled` | boolean | Per-rule switch, default `true`. |
| `exe` | string | Process image file name, e.g. `code.exe`; no path or wildcard. |
| `title_contains` | string | Case-insensitive substring; empty matches any title. |
| `style_id` | string or null | Existing `cleanup.styles[].id`; null keeps the global style. |
| `profile_id` | string or null | Existing `profiles[].id`; null keeps the global profile. |
| `paste_method` | string or null | `ctrl_v`, `shift_insert`, `ctrl_shift_v`, `type`; null keeps global. |
| `paste_after` | string or null | `none`, `enter`, `ctrl_enter`; null keeps global. |

Omitted fields use the defaults above, including in old configs and partial objects. A literal
`"none"` after-key explicitly disables the global after-key; JSON `null` inherits it. Style/profile
IDs are case-sensitive. A style override selects a style without enabling cleanup or changing the
profile's `apply_cleanup` flag. A profile override uses its own configured fallback, subject to the
existing retry policy (authentication failures do not trigger fallback).

Validation and normalization to mirror in the UI:

- Trim `id`, `exe`, `title_contains`, `style_id` and `profile_id`. Filename matching uses the shared
  Turkish-aware case fold (`I`, `i`, `İ`, `ı` all become `i`); normalized filenames use that fold.
  Title matching uses the same fold without modifying the stored title filter's case.
- Drop rules with empty filenames, `.` or `..`, control characters, or any of `/`, `\`, `:`, `*`,
  `?`, `"`, `<`, `>`, `|` in the filename. The UI should require a valid filename before saving.
- Keep the first occurrence of an ID. Empty or duplicate IDs become `app-rule-N` (positive integer),
  avoiding both existing IDs and IDs generated during that normalization. The UI may generate its
  own nonempty unique IDs. Use the config returned by `save_config` to pick up repairs.
- References to missing styles/profiles become `null` during config loading or saving. Runtime
  application also ignores dangling references defensively. Matching does not skip the rule:
  its remaining overrides still apply, and later rules are not considered.
- First enabled matching rule wins, including a rule with all four override fields null.
  There are no new hotkeys or hotkey conflict rules.

New Tauri command:

```ts
invoke<Array<{ exe: string; title: string }>>("list_running_apps")
```

No JS arguments. Runs enumeration via `spawn_blocking`. Returns visible, non-tool top-level
windows belonging to other processes, deduplicated by lowercase executable file name and sorted
ascending by that name. Unreadable or disappeared processes are skipped; empty titles are allowed.
When an executable has several windows, prefer the first nonempty title in enumeration order.
Titles are display suggestions, not persisted automatically. The own process is excluded by PID;
a different process running the same executable can still appear.

Enumeration failures or worker join failures reject with
`CommandError { code: "unavailable", message: string, kind: null }`. The message is localized in
English/Turkish and never contains native error details, process names or titles. An empty desktop
returns an empty array successfully.

Existing `get_config` / `save_config` and `config-changed` carry the new field inside their existing
`AppConfig` payloads; arguments, return envelopes and errors are unchanged. No new events. Applying
a rule does not emit `config-changed`. No changes to history/event schemas.

## Platform and privacy boundary

`ForegroundApp::current(include_title)` reads the foreground HWND and PID, opens the process with
`PROCESS_QUERY_LIMITED_INFORMATION`, queries its full image path, closes the handle and retains
only the filename. It reads a title only when at least one enabled rule has a nonempty title
filter. Disabled/empty rule sets avoid even the foreground query. Unreadable processes and focus
changes during the snapshot apply no rule. These short calls run on the controller start path,
never inside the keyboard hook. Picker enumeration runs on a blocking worker.

Process names and titles are never logged or included in errors. The only matching log message is
`app rule applied`. The platform result intentionally does not implement `Debug`.

The start path also covers microphone Ask and CLI timed recording, which already use that path.
Text-only Ask, file jobs, History retries and the in-memory retry use their existing current-global
settings behavior and do not query the foreground application. Explicit style commands during an
active recording retain the existing ability to change that recording's style.

## Manual acceptance

- From two real applications, confirm rule order, mixed-case filenames, Turkish title filters,
  disabled rules and disabled master switch. Changing focus mid-recording must not change the job.
- Use distinct real provider profiles and a transient primary failure to verify the selected
  profile and its fallback. Confirm the active global profile stays unchanged.
- Confirm all paste methods and after-keys in real target controls, including elevated windows,
  disappearing processes, queued recordings, and the existing clipboard-only safety behavior.
- Open the app picker: confirm visible normal windows, deduplication, exclusion of tool windows
  and this process, and correct titles. Verify English/Turkish errors if enumeration fails.
- Confirm the applied style appears in the recording overlay. No application/window text should
  appear in logs. Real microphone, provider, focus/UIPI and paste acceptance remain hardware tests.

## Implementation files

Added:

- `crates/core/src/app_rules.rs`
- `crates/app/src/platform/windows/foreground.rs`
- `docs/app-rules.md`

Changed:

- `crates/core/src/config.rs`, `crates/core/src/lib.rs`
- `crates/app/src/app_core.rs`, `crates/app/src/commands.rs`, `crates/app/src/lib.rs`
- `crates/app/src/controller/mod.rs`, `crates/app/src/controller/tests.rs`
- `crates/app/src/i18n.rs`
- `crates/app/src/platform/mod.rs`, `crates/app/src/platform/fake.rs`
- `crates/app/src/platform/windows/mod.rs`

## Verification (Windows, 2026-10-28)

All Cargo commands used `CARGO_NET_OFFLINE=true` and
`CARGO_TARGET_DIR=.superpowers/cargo-target`, sequentially. No dependency changes were needed.

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed, with these counts as printed by Cargo:

| Crate | Unit tests passed / ignored | Integration tests passed / ignored |
| --- | --- | --- |
| `opit-cli` | 3 / 0 | None |
| `opit-core` | 217 / 0 | `cleanup_http`: 3 / 0; `cleanup_styles_http`: 1 / 0; `pipeline_http`: 3 / 0 |
| `opit-eval` | 9 / 0 (binary: 0 / 0) | `run_eval`: 2 / 0 |
| `opit-ipc` | 6 / 1 | None |
| `opit-speech-to-text` (`opit_app`) | 271 / 12 (binary: 0 / 0) | `logging_init`: 1 / 0; `tauri_conf`: 7 / 0; `updater_artifacts`: 0 / 1; `updater_live`: 0 / 1 |

All four doc-test suites printed 0 passed / 0 ignored. No failures.

- `cargo test --workspace desktop_picker_has_sorted_unique_file_names -- --ignored`:
  passed the one desktop smoke test, exercising real native enumeration and the foreground query
  without printing titles/names, changing focus, or touching the clipboard.
- `git diff --check`: passed.

The picker smoke verifies native API execution and returned filename/sort/dedup invariants; it
does not replace the visual picker/filter checks or end-to-end dictation acceptance above.
