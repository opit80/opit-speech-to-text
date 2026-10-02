# Implementation plans — Opit Speech to Text v1

The v1 brief (`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`) spans four
independent subsystems. Each gets its own plan, and each plan ends in working, testable software.
Each plan is written after the previous one lands, so it is grounded in real APIs rather than
guessed ones.

| # | Plan | Delivers | Status |
|---|---|---|---|
| 1 | [Core library + eval CLI](2026-09-30-plan-1-core-and-eval.md) | `opit-core` (audio prep, provider client, rule engine, prompt builder, history, config) and `opit-eval`, which measures WER/term accuracy against a real provider | **done** — merged to `main`, 144 tests green, final review fixed |
| 2 | [Tauri app shell](2026-09-30-plan-2-app-shell.md) | `crates/app`: platform layer (cpal mic, WH_KEYBOARD_LL hotkey, SendInput paste, Win32 overlay, keyring, sounds, autostart), dictation controller state machine, tray, window lifecycle, `commands.rs` invoke API, `tracing` logs. Dictation works end to end with a hand-edited `config.json` | **merged** to `main` (2026-10-01): 15 tasks, final whole-branch review clean after one fix wave; app 114 tests + 9 ignored smoke tests, core 134 + 3. Manual end-to-end pass (Task 15 Steps 2–7) still open |
| 3 | [Svelte UI](2026-10-01-plan-3-ui.md) | First-run wizard, Home/History/Rules/Profiles/Settings pages, en + tr i18n, "add correction rule" flow, rules preview | **merged** to `main` (2026-10-01): 12 tasks; app 135 tests + 9 ignored, core 142 + 3, UI 43 Vitest tests, `svelte-check` 0 errors / 0 warnings. Final whole-branch review: 0 Critical, 1 Important (fixed), 13 Minor (deferred). The manual pass (Task 12 Step 2) is still open |
| 4 | [Release](2026-10-01-plan-4-release.md) | NSIS currentUser installer, `tauri-plugin-updater` + GitHub Releases (minisign), release workflow, README, `docs/RELEASE-CHECKLIST.md`, SignPath application | **merged** to `main` (2026-10-01): 7 tasks plus one fix wave; app 151 tests + 9 ignored, core 143 + 3, UI 51 Vitest tests, release script 6 `node:test` tests. Final whole-branch review: 0 Critical, 3 Important (fixed), 4 Minor (deferred). Nothing published yet (no GitHub repo, tag or release); the signed local build was viewed by the user |

## Plan 1 outcome — what later plans build on

**Entry points in `opit-core`** (read the code; these are the stable seams):
- `pipeline::prepare(&Recording) -> Result<PreparedAudio, PipelineError>` runs the silence gate and resamples to 16 kHz mono. Keep the `PreparedAudio` around for "Try again".
- `pipeline::transcribe(&PreparedAudio, &PipelineContext<T>)` does one retry, then falls back once, then runs the rules. `pipeline::run` does both steps. The future is `Send`.
- `provider::OpenAiCompatible::new(Profile, Option<String>)`, `.test_connection()`, and the `Transcriber` trait. Key trimming is done inside the core.
- `rules::builtin::assemble(user_pack, &enabled_ids)` → `RuleSet::compile` gives `(RuleSet, Vec<RuleWarning>)`. `RuleSet::apply_traced` is the source for the rules preview.
- `rules::load_pack_file`, `RulePack::{from_yaml, to_yaml}` read and write `user.yaml`. `rules::prompt::build_prompt` feeds the prompt budget gauge.
- `config::AppConfig::{load, save, normalize, active_profile, fallback_for}`. The data dir name is `config::APP_DIR_NAME`.
- `history::{HistoryStore, NewDictation, AudioStore}`. The store returns audio paths and the caller deletes the files.
- `audio::gate::rms` drives the overlay level meter.

**Decisions made in Plan 1 that Plan 2+ must respect:**
- Turkish matching folds `I/ı/İ/i` together. Casing never changes identifier-like targets (`iOS`, `ox_lib`, `Node.js`). Term casing ignores sentence position.
- Silence gate: frames are 30 ms. A frame counts as speech when it is above 0.008 RMS **and** either more than 2× the noise floor or above 0.02 RMS. Recordings shorter than 0.4 s are rejected, and so are recordings with less than 0.3 s of speech.
- `apply_rules: false` also turns off the hallucination filter. If the UI needs these separately, split them into two flags.
- A single malformed profile makes `AppConfig::load` return `ConfigError`. **Plan 2 must handle this at startup:** back up the bad file and start from defaults, then tell the user.
- `rustfmt.toml` sets `max_width = 120` and `use_small_heuristics = "Max"`. The workspace uses edition 2024, rust-version 1.88, and reqwest 0.13 with its default rustls TLS.

**Deferred minor findings** (candidates for a later plan):
- HTML 4xx bodies are shown raw in `ProviderError::Http` messages.
- Dictation ids are reused because the table has no `AUTOINCREMENT`. A `{id}.wav` file whose delete failed could get attached to a new row.
- `tr-core` has single-word hallucination patterns (`a`, `e`, `thank you`) that could drop a real one-word dictation.
- Case-insensitive regex replacements don't fold Turkish `İ/ı`; literal rules do.
- `opit-eval` `read_wav` panics on `bits_per_sample: 0`, and a `|` in a sample name breaks the report table.

**Not done:** the real-provider baseline (Plan 1, Task 16, Step 15). It needs a personal dataset and API keys; see `docs/eval.md`.

**Personal data (outside the repo):** the author's personal rule pack is at `%APPDATA%\opit-speech-to-text\rules\user.yaml`. The prompt context sentence noted in its header comment goes into `config.json` as `rules.prompt_context`.

## Plan 2 outcome — what Plan 3 builds on

**Status:** merged to `main` on 2026-10-01. The manual end-to-end pass (Plan 2, Task 15, Steps 2–7) has not been run yet. It needs a real microphone and a Groq key, and it measures latency and tray RAM. Its numbers and any failures still need to be recorded here.

The final-review fixes change three expectations for that pass:
- Stopping a dictation from the tray (or from our own window) never auto-pastes. The text stays on the clipboard and the overlay says "Panoda — Ctrl+V ile yapıştır".
- A microphone lost mid-recording shows the microphone error with **Try again**, which transcribes what was captured so far.
- The low-level hook cannot see keys while an elevated window has focus (UIPI). Step 4.4 therefore starts the dictation in a normal window and lets `recording.max_seconds` finish it in the admin one.

**Contract for the UI:**
- Invoke commands: `crates/app/src/commands.rs`. Events (`dictation-status`, `history-added`, `config-changed`, `navigate`): `crates/app/src/events.rs`. The full table is in the Plan 2 file, Task 14 ("Interfaces").
- Commands reject with `CommandError { code, message, kind }`.
- Plan 3 may add wizard-only commands (live microphone level test, hotkey capture) on top of `AppCore`.

**Behaviour Plan 3 must know about:**
- Debug builds use a no-op autostart (`DebugAutostart`), so the setting always reads as off there. Label or hide the toggle in debug builds.
- `save_config` refuses with a `config` error while a damaged `config.json` could not be moved aside at start-up. The UI should show that message.
- `ui.autostart` defaults to true, so the release exe registers itself in `HKCU\...\Run` on first start.

**Deferred findings (candidates for a later plan):**

Plan 3 Task 2 fixed five of the original items: the blocking commands are now `async` with
`spawn_blocking`, a tray pause emits `hotkey-state`, the paste-failure text mentions History only
when history is on, `set_active_profile` reads the config under `config_lock`, and
`list_microphones` no longer passes a worker panic message to the UI. These remain:

- Nothing recovers the state if the transcription task panics or the paste thread hangs (`controller/mod.rs`). Pasting cannot be escaped.
- A clipboard holding only uncopyable formats snapshots as empty, and the restore then clears it (`clipboard.rs`).
- `mic.start` and `history.record` block a Tokio worker inside the controller actor.
- The overlay's action fires on a mouse-up without a matching mouse-down, so a stray Retry could paste.
- Quit can lose the last log lines and a pending clipboard restore: the log guard is never dropped (`App::run` never returns on Windows), and Exit does not wait for the controller.
- Small robustness issues:
  - Two quick tray "Open" clicks race two window builds.
  - The second instance logs `starting` before it exits.
- `controller/mod.rs` logs "target window refused input" for every `ClipboardOnly`, including the shell and own-window case.
- A valid but locked `config.json` at start-up is moved aside as damaged. Its contents survive in `config.json.bad-*`.
- Minor clipboard items:
  - back-to-back pastes within 400 ms;
  - Ctrl+V sent with `wScan` 0;
  - the snapshot forces rendering of every delayed format.

## Plan 3 outcome — what Plan 4 builds on

**Status:** merged to `main` (2026-10-01). Of the 12 tasks, Tasks 2–8 were
written by Codex (gpt-6.1-sol, reasoning high). Claude wrote Task 1 (the rescue agent wrote the
code itself) and Tasks 9–12 (Claude subagents after Codex hit its usage limit). Tasks 1–7 had a
per-task review. Tasks 8–12 had none, by the user's choice. The final whole-branch review (Claude opus) found
0 Critical and 1 Important (the wizard reset an existing Groq/OpenAI profile to the preset), fixed
in one wave by a Claude subagent; its 13 Minor findings are in the deferred list below. The branch
was merged to `main` with `--no-ff`.

**Contract changes for the UI** (the full table is in the Plan 3 file, "UI contract after this plan"):
- `app_info` now returns `AppInfo { version, data_dir, log_dir, system_locale, debug_build }`.
- The blocking commands are `async` and run on `spawn_blocking`: `save_config`, keyring,
  `get_user_rules`/`save_user_rules`, `rules_preview`, `history_*`, `list_microphones`. Join
  failures reject with code `unavailable`.
- New commands:
  - `set_active_profile(id)` runs under `config_lock` and emits `config-changed`.
  - Rules: `rule_packs`, `parse_user_rules(yaml)`, `render_user_rules(pack, previousYaml)` (keeps
    the leading `#` comment block), `correction_draft(canonical, variant)` →
    `{ yaml, outcome }` (not saved).
  - `profile_presets` returns Groq, OpenAI and a custom template; it is the one source of the
    defaults.
  - `history_audio(id)` returns the WAV as a raw IPC response. It reads only inside `audio\`.
  - `set_hotkey_capture(active)` pauses the hook while the UI captures a shortcut and expires
    after 30 s. `validate_hotkey(keys)` rejects with `invalid_input`.
  - `mic_test_start(device)` / `mic_test_stop()`: refused while a dictation runs, auto-stop after
    20 s. A superseded or stopped test rejects with `unavailable` ("the microphone test was
    stopped"), which the UI ignores.
- New events: `hotkey-state` (`HotkeyState`, from the UI and the tray) and `mic-test`
  (`{ kind: "level", value }` | `{ kind: "failed", message }`).
- CSP stays `default-src 'self'` plus `media-src 'self' blob:` for History playback.

**`setup_done`:** `UiConfig.setup_done` (default `false`) drives the first-run wizard (`#/setup`).
A `config.json` without the field loads as `false`, so existing users see the wizard once. Finish
and Skip both set it. Until then the wizard comes back whenever the window opens.

**Behaviour Plan 4 must know about:**
- Routing is hash based (`#/home`, `#/history`, `#/rules`, `#/profiles`, `#/settings`, `#/setup`).
  Rust still opens `index.html#/settings` and emits `navigate`.
- Every UI string is in `ui/src/lib/i18n/en.ts` / `tr.ts`. Config writes from the UI go through the
  serialized `saveConfig` chain in `ui/src/lib/app.svelte.ts`.
- `ui.autostart` still defaults to `true` and is applied on the first start, before the wizard's
  "Start with Windows" step. The installer (Plan 4) should decide whether that stays.
- "Delete my data on uninstall" (spec §10) is left to Plan 4.

**Automated run (Task 12 Step 1, 2026-10-01, Windows):**
- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: core 142 passed + `pipeline_http` 3; eval 9 + `run_eval` 2; app 135
  passed, 9 ignored (hardware/desktop smoke tests) + `logging_init` 1; 0 failed.
- `npm ci`: 0 vulnerabilities. `npm run check`: 163 files, 0 errors, 0 warnings.
  `npm test`: 9 files, 40 tests passed. `npm run build`: 186 modules, JS 198.83 kB
  (61.17 kB gzip), CSS 20.91 kB.

**Manual pass (Task 12 Step 2): pending — needs the user.** It needs a real microphone, a Groq
key and Task Manager. Record pass/fail per line here:
1. Fresh `OPIT_DATA_DIR` → wizard → finish in under 2 minutes, including one real dictation into
   Notepad (spec §1). Time: pending.
2. Home: the status follows the hotkey and the button; the recent list updates live. Pending.
3. History: search with Turkish letters, audio playback (save audio on), add a correction from a
   real misrecognition and confirm it fixes the next dictation of that word. Pending.
4. Rules: a pack toggle changes the prompt gauge; a table edit keeps the `user.yaml` header; broken
   YAML is refused without losing the text. Pending.
5. Profiles: a second Groq profile with its own key; fallback; delete removes the Credential
   Manager entry. Pending.
6. Settings: language switch (UI + tray + overlay), shortcut capture (no accidental dictation; the
   hook resumes after closing mid-capture), microphone test, clipboard options. Pending.
7. Close the window during a dictation and reopen it from the tray: Home shows the running state.
   Pending.
8. Tray "Pause shortcut" → the paused banner appears in the open window without a reload. Pending.
9. Task Manager: tray-only RAM after closing the window (target < 40 MB). Value: pending.
10. Keyboard only: every page works with Tab/Shift+Tab/Enter/Space/Escape; dark mode is readable.
    Pending.

The visual and click-through checks listed in the Task 8–11 reports (dropdown and badge contrast
in both themes, radio-card states, layout at the minimum width, reverting a failed settings save)
belong to the same pass.

**Deferred findings (candidates for a later plan):**
- Core: `add_correction` dedupes terms by exact match, so an existing `github` term gets a second
  `GitHub`. There are no tests for `header_comments` with no body or `to_yaml_with_header` with an
  empty header.
- App commands:
  - `config-changed` is emitted after `config_lock` is released, so overlapping tray and UI saves
    can deliver an older config last.
  - The paste-failure text says "text is in History" when history is on, even if the history write
    itself failed.
  - `test_connection` reads the keyring on the async runtime without `spawn_blocking`.
  - The `history_audio` containment test has no `..` path case.
- Hotkey and microphone test:
  - The capture expiry check-then-act gap and the `sync_pause` ordering race (nanosecond window).
  - No tests for stale-generation timers (capture restart, mic test replace).
  - The dictation idle check in `mic_test_start` runs before the slot lock, so a dictation starting
    while the device opens is not refused.
- UI shell:
  - The load-error screen doubles the prefix ("could not load: Something went wrong: …") and has no
    retry.
  - The `resumeError` banner does not clear when the hotkey is resumed from the tray.
  - The Button reserves a 12 px spinner slot on both sides, which makes ghost and icon buttons wide.
  - Turkish wording: "tray" and "Çevriliyor" are kept for consistency with the spec and the overlay.
- Home: the missing-key effect depends on the whole profile object, which causes flicker and an
  extra keyring lookup on unrelated config changes. The module-level `disposed` flag never resets
  (an HMR re-run stops loading). "Show all" is shown when history is off.
- History:
  - A delete or clear during an in-flight live refresh can drop the new row until the next event.
  - The contenteditable transcript shows spellcheck underlines (needs `spellcheck="false"`), and
    IME composition can still edit it.
  - Playback errors show "Audio could not be encoded" and can toast twice.
  - A whitespace-only query still searches, and the first load waits for the 250 ms debounce.
- Final whole-branch review (Tasks 8–12 had no per-task review):
  - Settings: the microphone test stops whenever any setting is saved (the device-changed effect in
    `MicLevelTest` re-runs on every config object), and a hotkey error shows twice (page + app
    banner).
  - Wizard: the microphone and mode pickers save twice per change (function bindings into
    `Select`); the shortcut step shows the raw English hook error instead of
    `notice.hotkey_failed`.
  - `HotkeyInput`'s capture UI outlives Rust's 30 s capture auto-expiry.
  - Rules: a Save click right after editing a table cell is swallowed; adding a blank row
    re-serializes the whole YAML.
  - Profiles: turning off "Needs an API key" leaves the old key in Credential Manager.
  - Wizard provider step: Back during the save can orphan a stored key; Custom → Groq → Custom
    across Back creates a second custom profile.
  - A tray "Settings" click during the wizard restarts it at step 1.
  - Copy: `setup.mic.body` says "Press Test" but the button reads "Test microphone" (en + tr).
  - Leftovers: `ProfileForm`'s key-lookup error is never cleared; `profiles.delete_body` promises
    key deletion even for keyless or shared keys; `formatNumber` and the `common.saved_at`/`close`/
    `add`/`skip`/`done`/`yes`/`no` keys are unused.

## Plan 4 outcome

**Status:** merged to `main` (2026-10-01) after the user viewed the local build; nothing published (no
GitHub repo, push, tag or release). Tasks 1–7 and the fix wave were implemented by Claude
subagents, because Codex was at its usage limit. Instead of per-task reviews there was one
whole-branch review (Claude opus) after Task 6, before the installer was built, so the installer
below includes its fixes. It found 0 Critical, 3 Important and 4 Minor issues. The 3 Important
findings were fixed in one wave (`e6fb3a5`, `e8605ba`): a failed install now shows its error, update
checks and downloads time out, and dictations are refused while an update installs. The 4 Minor
findings are listed in "Plan 4 — deferred findings" below.

**Installer for review** (built by Task 7 with the real signing key; not run):
- Setup exe:
  `C:\projelerim\whisper\speechtotextexe\target\plan4-release\release\bundle\nsis\Opit Speech to Text_0.1.0_x64-setup.exe`
  (+ `.sig`). It was built with `CARGO_TARGET_DIR=target\plan4-release`, because the older app
  running from `target\release` locks that exe.
- Size: 6,078,034 bytes = **5.80 MB** (spec §1: < 15 MB).
- SHA-256: `E62DD69EDA95E498AD79B85EBEF82B22A1A3C6112E44B80D389366E0463DBCE8`.
- Trusted comment of the updater signature:
  `trusted comment: timestamp:1790830397	file:Opit Speech to Text_0.1.0_x64-setup.exe	version:0.1.0`
  (contains `version:0.1.0`, as `requireSignedVersion` needs). The build log has no "does not match
  the public key" line.
- `release-assets\` (repo root, ignored) holds the renamed copy
  `opit-speech-to-text_0.1.0_x64-setup.exe`, its `.sig`, and `latest.json` (version `0.1.0`, URL
  `https://github.com/opit80/opit-speech-to-text/releases/download/v0.1.0/opit-speech-to-text_0.1.0_x64-setup.exe`).

**Rulings applied:**
- Ruling: `ui.autostart` keeps its default `true`, but the app writes the Run value only when `ui.autostart && ui.setup_done` (`app_core::autostart_wanted`). Start-up applies that rule, so a fresh install removes any stale value until the wizard is finished or skipped, and finishing or skipping applies the user's choice at once. The installer never writes the Run key. Tauri's uninstaller already deletes the value named after `productName`, which is our `VALUE_NAME`. Cost if wrong: a user who closes the wizard without finishing or skipping it gets no autostart until they do. That is one click, and reverting is a one-line change in `autostart_wanted`. This also meets SignPath's rule that the software does not change system configuration without a warning.
- Ruling: "delete my data" is Tauri's own uninstaller checkbox, "Delete the application data" / "Uygulama verilerini sil". It is unchecked by default and ignored in update mode. Out of the box it removes `%APPDATA%\<identifier>` and `%LOCALAPPDATA%\<identifier>` (WebView2 data). Our `hooks.nsh` extends it to `%APPDATA%\opit-speech-to-text\` and to this app's Credential Manager entries: the uninstaller runs `opit-speech-to-text.exe --delete-credentials` while the exe still exists. When the box is left unticked, the uninstaller removes only the program, the shortcuts and the Run value, and the data stays for a reinstall. Cost if wrong: an unticked box leaves data behind (the README documents manual removal), and a mis-guarded hook would wipe data during an update. Task 1's guard test and the checklist's update line exist to catch that.

**View the app (user, before any release):**
1. Close the old app that runs from `target\release`: tray icon → Quit (Çıkış). The installed app is a
   single instance, so it will not start while the old one runs.
2. Back up `%APPDATA%\opit-speech-to-text\rules\user.yaml` (the installed app uses the real data
   folder). Backing up the whole `%APPDATA%\opit-speech-to-text\` folder is safer still.
3. Run the setup exe above. A locally built file has no internet mark, so SmartScreen does not
   appear here; it will for downloaded releases. Expect no UAC prompt, English or Turkish pages,
   install to `%LOCALAPPDATA%\Opit Speech to Text`, and "Run" on the last page.
4. If `setup_done` is still false in your config, the wizard opens. (Your current `config.json` has
   no `setup_done` field, so it will.) The Run value must not exist before Finish/Skip
   (`reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v "Opit Speech to Text"`) and
   must exist after it, if left checked, now pointing at the installed exe instead of
   `target\release`.
5. Settings → Updates: the switch is on, and **Check now** shows "Could not check for updates:
   Could not fetch a valid release JSON from the remote". This is expected until the GitHub repo
   and a release exist. No banner appears.
6. One real dictation with the shortcut, the tray menu, and tray RAM in Task Manager (< 40 MB).
7. Optional: uninstall without ticking "Delete the application data" and confirm the data stays.
   Tick it only on a machine whose data you have backed up.

**Automated run (Task 7 Step 1, 2026-10-01, Windows):**
- `cargo fmt --all --check`: clean. `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: core 143 passed + `pipeline_http` 3; eval 9 + `run_eval` 2; app 151
  passed, 9 ignored (hardware/desktop smoke tests) + `logging_init` 1 + `tauri_conf` 5; 0 failed.
- `node --test scripts/release/latest-json.test.mjs`: 6 passed.
- `npm ci`: 0 vulnerabilities. `npm run check`: 167 files, 0 errors, 0 warnings. `npm test`: 10
  files, 51 tests passed. `npm run build`: 191 modules, JS 205.63 kB (63.12 kB gzip), CSS 21.11 kB.
- `cargo tauri build --ci` (signed): release build in 1 m 47 s, one NSIS bundle and one updater
  signature.

**What the user must do before the first release:**
- Back up `%USERPROFILE%\.tauri\opit-speech-to-text.key`, its `.pub` and the password (password
  manager). They cannot be recreated, and losing them strands every installed copy.
- Create the GitHub repo `opit80/opit-speech-to-text` and push `main`.
- Add the secrets `TAURI_SIGNING_PRIVATE_KEY` (content of
  `%USERPROFILE%\.tauri\opit-speech-to-text.key`) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (content
  of the `.password` file), then delete the `.password` file.
- Tag `v0.1.0`, run `docs/RELEASE-CHECKLIST.md` on a clean Windows and publish the draft.
- Apply to SignPath Foundation with `docs/signpath-application.md` after the first release, with
  MFA on.
- The manual passes still open from Plans 2 and 3.

**Deferred (not in this plan):** SignPath integration in the workflow; release notes shown inside
the app (the `notes` field is not displayed); a tray menu entry for updates. Found while executing
the plan: a stalled update download fails only after the 10-minute total timeout (an idle timeout
would need `configure_client` and a direct reqwest dependency). The final review's Minor findings
are in the next section.

## Plan 4 — deferred findings

**Follow-up (2026-10-03):** local pre-release preparation addresses the early credential deletion,
the stale update-check UI reply, mismatched signing keys and same-tag draft reruns. It also corrects
the cleanup documentation to cover preset/current profile keys, rather than promising enumeration
of every orphaned credential. See [../../PRE-RELEASE-REPORT.md](../../PRE-RELEASE-REPORT.md) and
[../../FIRST-RELEASE.md](../../FIRST-RELEASE.md) for current evidence and remaining acceptance work.
The original review findings below are retained as historical context. Nothing has been published.

**Deferred findings (candidates for a later plan)** from the final whole-branch review; its three
Important findings were fixed in one wave:
- Uninstaller: `hooks.nsh` PREUNINSTALL deletes the credentials before Tauri's
  `CheckIfAppIsRunning`, so cancelling that prompt keeps the app and config but the keys are gone.
- `uninstall.rs` finds credentials only via preset refs and refs in the current `config.json`, so
  keys orphaned by a config reset or a failed `deleteApiKey` stay. The docs promise "every
  `*.opit-speech-to-text` entry": filter with `CredEnumerateW` or soften the docs.
- Settings → Updates: `checkNow` writes the command's return value into `app.update`, so a late
  `checking` reply after the final `update-state` event leaves the page on "Checking…".
- `release.yml`: a key that does not match the pubkey is only a tauri-cli warning, so CI could
  publish a draft with useless signatures (add a log-grep step); re-running for the same tag fails
  at `gh release create` (note it in the checklist).
