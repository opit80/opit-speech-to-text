# Implementation plans — Opit Speech to Text v1

The v1 brief (`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`) spans four
independent subsystems. Each gets its own plan, and each plan ends in working, testable software.
Each plan is written after the previous one lands, so it is grounded in real APIs rather than
guessed ones.

| # | Plan | Delivers | Status |
|---|---|---|---|
| 1 | [Core library + eval CLI](2026-09-30-plan-1-core-and-eval.md) | `opit-core` (audio prep, provider client, rule engine, prompt builder, history, config) and `opit-eval`, which measures WER/term accuracy against a real provider | **done** — merged to `main`, 144 tests green, final review fixed |
| 2 | [Tauri app shell](2026-09-30-plan-2-app-shell.md) | `crates/app`: platform layer (cpal mic, WH_KEYBOARD_LL hotkey, SendInput paste, Win32 overlay, keyring, sounds, autostart), dictation controller state machine, tray, window lifecycle, `commands.rs` invoke API, `tracing` logs. Dictation works end to end with a hand-edited `config.json` | **code complete** on `feat/plan-2-app-shell` (not merged): 15 tasks, final whole-branch review clean after one fix wave; app 114 tests + 9 ignored smoke tests, core 134 + 3. Manual end-to-end pass (Task 15 Steps 2–7) still open |
| 3 | Svelte UI | First-run wizard, Home/History/Rules/Profiles/Settings pages, en + tr i18n, "add correction rule" flow, rules preview | next — being planned |
| 4 | Release | NSIS currentUser installer, `tauri-plugin-updater` + GitHub Releases (minisign), release workflow, README, `docs/RELEASE-CHECKLIST.md`, SignPath application | after plan 3 |

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

**Status:** code complete on `feat/plan-2-app-shell`, not merged to `main`. The manual end-to-end pass (Plan 2, Task 15, Steps 2–7) has not been run yet. It needs a real microphone and a Groq key, and it measures latency and tray RAM. Its numbers and any failures still need to be recorded here.

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

**Deferred findings (candidates for Plan 3):**
- Sync commands (`save_config`, keyring, `history_*`, `rules_preview` with a draft) run on the main thread. Make them `async` or `spawn_blocking` before the UI calls them often.
- Pausing the hotkey from the tray emits no event, so a UI showing `get_hotkey_state` goes stale.
- Nothing recovers the state if the transcription task panics or the paste thread hangs (`controller/mod.rs`). Pasting cannot be escaped.
- The paste-failure message says "text is in History" even when history is off (`i18n.rs`).
- A clipboard holding only uncopyable formats snapshots as empty, and the restore then clears it (`clipboard.rs`).
- `mic.start` and `history.record` block a Tokio worker inside the controller actor.
- `set_active_profile` reads the config before taking `config_lock`.
- The overlay's action fires on a mouse-up without a matching mouse-down, so a stray Retry could paste.
- Quit can lose the last log lines and a pending clipboard restore: the log guard is never dropped (`App::run` never returns on Windows), and Exit does not wait for the controller.
- Small robustness issues:
  - `list_microphones` passes a worker panic message to the UI.
  - Two quick tray "Open" clicks race two window builds.
  - The second instance logs `starting` before it exits.
- `controller/mod.rs` logs "target window refused input" for every `ClipboardOnly`, including the shell and own-window case.
- A valid but locked `config.json` at start-up is moved aside as damaged. Its contents survive in `config.json.bad-*`.
- Minor clipboard items:
  - back-to-back pastes within 400 ms;
  - Ctrl+V sent with `wScan` 0;
  - the snapshot forces rendering of every delayed format.
