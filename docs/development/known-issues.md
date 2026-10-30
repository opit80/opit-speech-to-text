# Known issues and pending acceptance

Documentation extraction: 2026-10-30, source commit `3b920bba7761eedfe505df354c53e28c35d20cd0`.
This is an inventory, not evidence that old findings reproduce on current code or that desktop acceptance passed.
Retained candidates require the stated check; superseded rows identify replacement code/tests, not a new test run.
All paths in code spans are repository-root paths. See the [documentation index](../README.md),
[release checklist](../release/RELEASE-CHECKLIST.md), [active layout plan](../superpowers/plans/2026-10-30-layout-cleanup.md)
and [layout/Linux design](../superpowers/specs/2026-10-30-layout-and-linux-design.md).

## Source and disposition ledger

The source index was `docs/superpowers/plans/README.md`. Its Plan 1–4 outcome/deferred sections
are recoverable with `git show 3b920bb:docs/superpowers/plans/README.md`; the four original plan
filenames are recorded below. Row IDs follow the original leaf-bullet order, including compound
items. Each row is its retained destination. “Partly superseded” explicitly retains the unresolved portion.

| Source | Original plan / related tasks (historical paths) |
| --- | --- |
| P1 | `docs/superpowers/plans/2026-09-30-plan-1-core-and-eval.md`: Tasks 2–5 rules, 10 audio, 12–14 provider/history, 15–16 eval |
| P2 | `docs/superpowers/plans/2026-09-30-plan-2-app-shell.md`: Task 5 startup/logging, 6 controller, 8 AppCore, 11 hotkey, 12 clipboard, 13 overlay, 14 shell, 15 manual pass |
| P3 | `docs/superpowers/plans/2026-10-01-plan-3-ui.md`: Task 1 pack/YAML, 2 commands, 3 history, 4 capture/mic, 5 shell/components, 6 Home, 7 History, 8 Rules, 9 Profiles, 10 Settings, 11 wizard, 12 acceptance |
| P4 | `docs/superpowers/plans/2026-10-01-plan-4-release.md`: Task 1 uninstall, 3–4 update state/UI, 5 release helpers/workflow, 6 signing docs, 7 local build/acceptance |

### Original deferred findings

| ID | Original source item | Disposition | Current code/test evidence | Actionable check / retained boundary |
| --- | --- | --- | --- | --- |
| P1-01 | Resolved by Opit Speech to Text's patient retries: HTTP errors expose only safe kinds/status codes, never provider bodies. | Superseded | `crates/core/src/provider/openai.rs: status_error and rate_limit_header_is_preserved_without_reading_the_error_body` | Keep provider-body redaction coverage; do not restore raw HTTP errors. |
| P1-02 | Resolved by Opit Speech to Text's failed-dictation history: schema v2 migrates existing rows and FTS triggers to `AUTOINCREMENT`, so deleting rows no longer reuses their audio ids. | Superseded | `crates/core/src/history/mod.rs: AUTOINCREMENT migrations and cap_removes_oldest_dates_then_ids_including_failed_audio_and_fts` | Keep delete/insert and migration coverage; old row-id reuse is historical. |
| P1-03 | Resolved by Opit Speech to Text's safer hallucination filter: single-word built-in patterns removed; whole-output stock phrases and their repeats apply only to audio of at most 6 seconds, with accent, punctuation and Turkish case folding. `apply_rules: false` still disables filtering. | Superseded | `crates/core/src/rules/hallucination.rs; crates/core/src/pipeline.rs: empty_and_hallucinated_outputs_are_flagged` | Preserve short-audio whole-output filtering and apply_rules behavior. |
| P1-04 | Case-insensitive regex replacements don't fold Turkish `İ/ı`; literal rules do. | Retained candidate | `crates/core/src/rules/engine.rs: regex_replace; crates/core/src/rules/matcher.rs` | Compare case-insensitive regex and literal replacements for I/ı/İ/i; decide whether regex needs Turkish folding. |
| P1-05 | `opit-eval` `read_wav` panics on `bits_per_sample: 0`, and a `&#124;` in a sample name breaks the report table. | Retained candidate | `crates/eval/src/dataset.rs: read_wav subtracts one from bits_per_sample; crates/eval/src/report.rs: render` | Feed a malformed zero-bit WAV and a sample name containing a pipe; require an error without panic and a valid Markdown table. Check decoder rejection before attributing a reachable panic. |
| P2-01 | Transcription task panic recovery resolved by Opit Speech to Text's dictation queue: a monitored worker reports a sanitized error and the queue advances. A hanging paste thread still cannot be escaped. | Retained / partly superseded | `crates/app/src/controller/tests.rs: provider_worker_panic_does_not_wedge_the_controller and recording_continues_while_paste_and_restore_are_blocked` | Provider panic recovery is superseded by the monitored queue. Retain the hanging-paste limitation: use a blocked fake Paster and verify whether cancellation/shutdown can finish; do not claim recording concurrency makes paste escapable. |
| P2-02 | A clipboard holding only uncopyable formats snapshots as empty, and the restore then clears it (`clipboard.rs`). | Retained candidate | `crates/app/src/platform/windows/clipboard.rs: snapshot versus selection_snapshot, restore` | On disposable Windows, copy only unsupported/GDI formats, enable restoration, dictate, and inspect original clipboard formats. Strict selection backup does not prove best-effort paste backup safety. |
| P2-03 | `mic.start` and `history.record` block a Tokio worker inside the controller actor. | Retained candidate | `crates/app/src/controller/mod.rs: microphone start and history.record call sites` | Use a slow fake microphone/history store; observe actor responsiveness and Tokio worker blocking. Reassess after the controller move; do not fix here. |
| P2-04 | The overlay's action fires on a mouse-up without a matching mouse-down, so a stray Retry could paste. | Retained candidate | `crates/app/src/platform/windows/overlay.rs: WM_LBUTTONUP and overlay_button_click` | Send a mouse-up without a preceding mouse-down over Retry on a disposable desktop; expect no unintended delivery. |
| P2-05 | Quit can lose the last log lines and a pending clipboard restore: the log guard is never dropped (`App::run` never returns on Windows), and Exit does not wait for the controller. | Retained / partly superseded | `crates/app/src/lib.rs: RunEvent::ExitRequested sends Msg::Shutdown and drains media/local; crates/app/src/logging.rs: WorkerGuard` | The old no-shutdown-dispatch description is stale. Still verify quit during pending paste/restore and final log flush: sending Shutdown and awaiting media/local is not a controller completion acknowledgement. |
| P2-06 | Two quick tray "Open" clicks race two window builds. | Retained candidate | `crates/app/src/window.rs: show_main and build` | Double-click tray Open rapidly with no WebView; require one window and no build error. |
| P2-07 | The second instance logs `starting` before it exits. | Retained candidate | `crates/app/src/lib.rs: starting log before single-instance plugin setup` | Start a second instance and inspect metadata-only logs; determine whether a redundant starting record remains. |
| P2-08 | Resolved with the dictation queue: `ClipboardOnly` logs only that text remains on the clipboard, without guessing why auto-paste was skipped. | Superseded | `crates/app/src/controller/mod.rs: PasteOutcome::ClipboardOnly; crates/app/src/controller/tests.rs: a_refused_paste_leaves_the_text_on_the_clipboard` | Retain clipboard-only metadata logging without guessing the reason. |
| P2-09 | A valid but locked `config.json` at start-up is moved aside as damaged. Its contents survive in `config.json.bad-*`. | Retained candidate | `crates/app/src/startup.rs: config recovery; crates/app/src/app_core.rs: a_config_that_could_not_be_moved_aside_is_never_overwritten` | Hold a valid scratch config with restrictive Windows sharing and start; distinguish a sharing failure from corrupt data, confirm backup protection. |
| P2-10 | back-to-back clipboard restoration resolved by the dictation queue: each paste waits for its own restore before the next paste starts; | Superseded | `crates/app/src/platform/windows/paster.rs: CLIPBOARD_OPERATION; crates/app/src/controller/tests.rs: bounded_fifo_preserves_profiles_paste_policy_and_history_order` | Preserve serialized paste/restore ordering; verify native formats in the desktop pass. |
| P2-11 | Ctrl+V sent with `wScan` 0; | Retained candidate | `crates/app/src/platform/windows/paster.rs: key_input and clipboard shortcut INPUT builders` | Check Ctrl+V behavior on multiple keyboard layouts and target controls, including scan-code-sensitive software. |
| P2-12 | the snapshot forces rendering of every delayed format. | Retained candidate | `crates/app/src/platform/windows/clipboard.rs: snapshot and copy_hglobal` | Use a delayed-rendering clipboard owner on disposable Windows; check blocking and preservation of every format. |
| P3-01 | Core: `add_correction` dedupes terms by exact match, so an existing `github` term gets a second `GitHub`. There are no tests for `header_comments` with no body or `to_yaml_with_header` with an empty header. | Retained / partly superseded | `crates/core/src/rules/pack.rs: add_correction delegates casing-only changes to add_term; term_updates_casing_in_place_with_turkish_folding` | Exact-match casing duplication is superseded. Retain the test-gap candidate: cover header_comments with a comment-only file and to_yaml_with_header with an empty header. |
| P3-02 | `config-changed` is emitted after `config_lock` is released, so overlapping tray and UI saves can deliver an older config last. | Retained candidate | `crates/app/src/commands.rs: save_config/set_active_profile/learning_apply emit after AppCore returns; crates/app/src/app_core.rs: config_lock` | Interleave tray, UI config and learning/rules saves; collect event revisions/order and ensure older replies/events cannot overwrite newer config. |
| P3-03 | The paste-failure text says "text is in History" when history is on, even if the history write itself failed. | Retained candidate | `crates/app/src/controller/mod.rs; crates/app/src/controller/tests.rs: a_failed_paste_uses_the_sessions_history_setting` | Fail a history write and paste together with history enabled; check the user message does not promise an unavailable row. |
| P3-04 | `test_connection` reads the keyring on the async runtime without `spawn_blocking`. | Retained candidate | `crates/app/src/app_core.rs: async test_connection directly calls platform.secrets.get; crates/app/src/commands.rs: test_connection` | Use a blocking fake keyring and observe runtime responsiveness; the cleanup-test path uses a blocking worker but does not supersede this path. |
| P3-05 | Resolved by Opit Speech to Text's failed-dictation history: playback and retry tests reject `audio/../` paths that escape the audio directory; deletion also stays inside that directory. | Superseded | `crates/app/src/app_core.rs: history_audio_reads_only_files_in_the_audio_folder and failed_history_wav_loads_after_reopen_and_requires_history_enabled; crates/app/src/history_service.rs` | Keep playback/retry/deletion inside the audio directory, including audio/../ escape attempts. |
| P3-06 | The capture expiry check-then-act gap and the `sync_pause` ordering race (nanosecond window). | Retained candidate | `crates/app/src/app_core.rs: set_hotkey_capture_for and sync_pause` | Stress expiry concurrent with restart/user pause; generation tests cover ordinary stale timers, not atomicity of check-then-store or pause ordering. |
| P3-07 | No tests for stale-generation timers (capture restart, mic test replace). | Superseded | `crates/app/src/app_core.rs: an_ended_hotkey_captures_timer_keeps_the_restarted_capture_paused; a_replaced_mic_tests_timer_keeps_the_new_capture_running` | Keep both named tests in Windows discovery; do not carry the old missing-tests claim. |
| P3-08 | The dictation idle check in `mic_test_start` runs before the slot lock, so a dictation starting while the device opens is not refused. | Retained candidate | `crates/app/src/app_core.rs: mic_test_start_for idle check before mic_test lock/device open` | Block fake device opening, start dictation, then release opening; check simultaneous capture refusal and cleanup. |
| P3-09 | The load-error screen doubles the prefix ("could not load: Something went wrong: …") and has no retry. | Retained candidate | `ui/src/App.svelte: app.loadError screen; ui/src/lib/app.svelte.ts: initApp catch; ui/src/lib/errors.ts` | Reject initial IPC load in both languages; inspect duplicate prefixes and recovery/retry access. |
| P3-10 | The `resumeError` banner does not clear when the hotkey is resumed from the tray. | Retained candidate | `ui/src/App.svelte: resumeError and resumeShortcut` | Fail UI Resume, then resume successfully from tray; require the stale error to disappear. |
| P3-11 | The Button reserves a 12 px spinner slot on both sides, which makes ghost and icon buttons wide. | Retained candidate | `ui/src/lib/components/Button.svelte: spinner/slot layout` | Inspect ghost and icon button width, focus target and alignment in both themes at minimum width. |
| P3-12 | Turkish wording: "tray" and "Çevriliyor" are kept for consistency with the spec and the overlay. | Historical-only | `ui/src/lib/i18n/tr.ts; crates/app/src/i18n.rs; original design brief` | This records a terminology decision, not a confirmed defect. Any future wording change requires bilingual/native consistency review. |
| P3-13 | Home: the missing-key effect depends on the whole profile object, which causes flicker and an extra keyring lookup on unrelated config changes. The module-level `disposed` flag never resets (an HMR re-run stops loading). "Show all" is shown when history is off. | Retained candidate | `ui/src/pages/Home.svelte: active/api_key_ref effect, component-local disposed and history Show all link` | The old module-level description is stale: disposed is component-local. Instrument unrelated config saves for key lookup/flicker, rerun HMR effects, and open Home with history off before deciding which candidates remain. |
| P3-14 | A delete or clear during an in-flight live refresh can drop the new row until the next event. | Retained candidate | `ui/src/pages/History.svelte: generation/listRequest, remove, refreshPending; ui/src/lib/history.test.ts` | Current mutation guards invalidate old loads and defer new events. Delay refresh, delete/clear, and complete a dictation concurrently; verify the new row remains. Pure helper tests do not establish mounted behavior. |
| P3-15 | The contenteditable transcript shows spellcheck underlines (needs `spellcheck="false"`), and IME composition can still edit it. | Retained candidate | `ui/src/pages/History.svelte: data-transcript contenteditable=plaintext-only versus edit textarea spellcheck=false` | Use spelling and IME composition on the read-only transcript; verify displayed/persisted text cannot be changed accidentally. |
| P3-16 | Playback errors show "Audio could not be encoded" and can toast twice. | Retained candidate | `ui/src/pages/History.svelte: play/stopAudio and audio error handlers; ui/src/lib/i18n/en.ts` | Play missing/corrupt audio and switch files; require an accurate single error and no duplicate toast. |
| P3-17 | A whitespace-only query still searches, and the first load waits for the 250 ms debounce. | Retained candidate | `ui/src/pages/History.svelte: query effect and setTimeout(..., 250)` | Open History and enter whitespace-only search; measure initial load and whether blank queries invoke FTS. |
| P3-18 | Settings: the microphone test stops whenever any setting is saved (the device-changed effect in `MicLevelTest` re-runs on every config object), and a hotkey error shows twice (page + app banner). | Retained candidate | `ui/src/lib/components/MicLevelTest.svelte: device-only effect with untrack; ui/src/pages/Settings.svelte and ui/src/App.svelte: error banners` | Current effect tracks device, so the old whole-config description needs reproduction. Save unrelated settings during a mic test and trigger a hotkey error; verify continuity and one clear error. |
| P3-19 | Wizard: the microphone and mode pickers save twice per change (function bindings into `Select`); the shortcut step shows the raw English hook error instead of `notice.hotkey_failed`. | Retained candidate | `ui/src/pages/setup/StepMicrophone.svelte: setMic value guard; ui/src/pages/setup/StepShortcut.svelte: mode binding/error rendering` | Count save_config calls for microphone/mode changes and force hook failure in Turkish; guard against duplicate saves and raw English errors. |
| P3-20 | `HotkeyInput`'s capture UI outlives Rust's 30 s capture auto-expiry. | Retained candidate | `ui/src/lib/components/HotkeyInput.svelte: expiry timer/session; crates/app/src/app_core.rs: HOTKEY_CAPTURE_LIMIT` | Current UI has an expiry timer. Leave capture idle beyond 30 seconds and delay validation; compare UI/hook state before classifying the old mismatch as current. |
| P3-21 | Rules: a Save click right after editing a table cell is swallowed; adding a blank row re-serializes the whole YAML. | Retained candidate | `ui/src/pages/Rules.svelte and ui/src/pages/RulesTable.svelte: draft rendering and save` | Edit a cell then click Save immediately; add a blank row in YAML containing comments, verify save and preservation of draft/comments. |
| P3-22 | Profiles: turning off "Needs an API key" leaves the old key in Credential Manager. | Retained candidate | `ui/src/pages/ProfileForm.svelte: setNeedsKey; ui/src/pages/Profiles.svelte: save/delete key lifecycle` | Use disposable unique and shared credential refs; disable Needs an API key and save, inspect cleanup without deleting another profile key. |
| P3-23 | Wizard provider step: Back during the save can orphan a stored key; Custom → Groq → Custom across Back creates a second custom profile. | Retained candidate | `ui/src/pages/setup/StepProvider.svelte: key save then config save, editingCustom; ui/src/pages/Setup.svelte` | Delay save and press Back; repeat Custom → Groq → Custom across Back; inspect orphan keys and duplicate profiles in scratch state. |
| P3-24 | A tray "Settings" click during the wizard restarts it at step 1. | Retained candidate | `ui/src/App.svelte: setup route selection; ui/src/pages/Setup.svelte: step state; ui/src/lib/app.svelte.ts: onNavigate` | Open tray Settings midway through setup; verify step progress remains or an intentional navigation is explained. |
| P3-25 | Copy: `setup.mic.body` says "Press Test" but the button reads "Test microphone" (en + tr). | Retained candidate | `ui/src/lib/i18n/en.ts and tr.ts: setup.mic.body, mic.test` | Compare actual microphone instruction and button labels in both languages; change neither during layout work. |
| P3-26 | Leftovers: `ProfileForm`'s key-lookup error is never cleared; `profiles.delete_body` promises key deletion even for keyless or shared keys; `formatNumber` and the `common.saved_at`/`close`/ `add`/`skip`/`done`/`yes`/`no` keys are unused. | Retained candidate | `ui/src/pages/ProfileForm.svelte: keyError/key effect; ui/src/pages/Profiles.svelte; ui/src/lib/format.ts and i18n dictionaries` | Fail then retry key lookup, inspect keyless/shared-key deletion wording, and search each listed symbol/key for consumers before removing unused candidates. Some explicit keyError resets already exist. |
| P4-01 | Uninstaller: `hooks.nsh` PREUNINSTALL deletes the credentials before Tauri's `CheckIfAppIsRunning`, so cancelling that prompt keeps the app and config but the keys are gone. | Superseded | `crates/app/tests/tauri_conf.rs: cancelling_uninstall_cannot_delete_credentials_and_silent_runs_keep_data; crates/app/windows/hooks.nsh` | Static order/guards are covered; actual Windows cancellation and update/silent data preservation remain manual checks below. |
| P4-02 | `uninstall.rs` finds credentials only via preset refs and refs in the current `config.json`, so keys orphaned by a config reset or a failed `deleteApiKey` stay. The docs promise "every `*.opit-speech-to-text` entry": filter with `CredEnumerateW` or soften the docs. | Retained / partly superseded | `crates/app/src/uninstall.rs: credential_refs; docs/release/RELEASE-CHECKLIST.md; docs/archive/PRE-RELEASE-REPORT.md` | Overbroad documentation promise is superseded; preset/current refs remain the boundary. Test config reset/orphaned credentials on a disposable account and manually remove only known orphaned entries. |
| P4-03 | Settings → Updates: `checkNow` writes the command's return value into `app.update`, so a late `checking` reply after the final `update-state` event leaves the page on "Checking…". | Superseded | `ui/src/lib/update.test.ts: does not overwrite an event that arrives before a stale command reply; ui/src/lib/app.svelte.ts: checkForUpdates` | Keep event revision guard and its test; do not preserve the old stuck-Checking claim. |
| P4-04 | `release.yml`: a key that does not match the pubkey is only a tauri-cli warning, so CI could publish a draft with useless signatures (add a log-grep step); re-running for the same tag fails at `gh release create` (note it in the checklist). | Superseded | `scripts/release/latest-json.test.mjs; scripts/release/draft-release.test.mjs; docs/archive/PRE-RELEASE-REPORT.md` | Keep mismatched-key/tamper/signed-version verification and rerunnable draft tests; published assets remain immutable. |

### Additional source dispositions

| ID | Source item | Disposition and destination / current evidence | Check |
| --- | --- | --- | --- |
| P2-F1 | Index Plan 2 outcome, fixed by Plan 3 Task 2: blocking commands | Superseded: `crates/app/src/commands.rs` uses async blocking wrappers for file/keyring/history work. P3-04 separately retains the direct test_connection keyring call. | Preserve off-main-thread work. |
| P2-F2 | Same source: tray pause notification | Superseded: `crates/app/src/tray.rs`, `ui/src/lib/app.svelte.ts` hotkey-state subscription. | Native tray/banner acceptance remains M3-08. |
| P2-F3 | Same source: paste text with history disabled | Superseded: `a_failed_paste_does_not_mention_history_when_it_is_disabled` in `crates/app/src/controller/tests.rs`. | P3-03 separately retains failed history-write wording. |
| P2-F4 | Same source: set_active_profile read-modify-write | Superseded: `crates/app/src/app_core.rs` uses update_config under config_lock; `update_config_applies_the_change_under_the_lock`. | P3-02 retains notification/learning ordering, not the fixed locked read. |
| P2-F5 | Same source: microphone worker panic message | Superseded: `crates/app/src/commands.rs` list_microphones maps JoinError to a safe unavailable error. | Do not expose worker panic contents. |
| P4-D1 | Index Plan 4 outcome / Task 6: SignPath workflow integration | Retained: [signing application notes](../release/signpath-application.md), `.github/workflows/release.yml`. Updater signatures do not establish Authenticode signing. | Complete application/integration separately after maintainer authorization; verify downloaded installer signing. |
| P4-D2 | Same source: release notes in app | Retained candidate: `ui/src/lib/types.ts` UpdateInfo.notes, `ui/src/pages/Settings.svelte`. | Offer a fixture release with notes; determine whether/how they should be displayed. |
| P4-D3 | Same source: tray Updates entry | Retained candidate: `crates/app/src/tray_menu.rs`. | Inspect native tray for update discovery/action; decide whether to add an entry in feature work. |
| P4-D4 | Same source: stalled update download waits for ten-minute total timeout | Retained candidate: `crates/app/src/updates.rs`. | Stall a fixture download after headers; measure timeout and recovery; evaluate an idle timeout separately. |
| H1 | Plan index implementation outcomes, historical counts and public-contract descriptions | Historical-only: source commit above; current contracts live in code and reference docs. Old task code blocks are not execution instructions. | Use current CI/discovery, never copy historical counts as current results. |
| H2 | Plan 4 Task 7 / index: local 0.1.0 signed build, absolute installer path and private outputs | Historical-only: 2026-10-01 Windows build reported 6,078,034 bytes (5.80 MB), below 15 MB; updater signature version/key checks passed. Installer execution and clean-machine acceptance were not established. Later public evidence is in [publication snapshot](../archive/PUBLICATION-REPORT.md). | Rebuild using current release instructions; the private installer/logs are not public downloads. |
| H3 | Plan 4/index first-publication GitHub/tag/secrets/draft instructions | Historical-only: [2026-10-03 publication snapshot](../archive/PUBLICATION-REPORT.md) supersedes “nothing published” and repository-creation work; [first-release runbook](../archive/FIRST-RELEASE.md) retains historical procedure. | Do not repeat publication or rotate keys for layout work. Independent key/password backup remains unconfirmed; confirm securely without printing/copying secrets into the repository. |
| H4 | `.claude/settings.json` attribution overrides | Keeps automatic commit and PR trailers off; no product execution instructions or acceptance items. | Keep the tracked file; preserve ignored `.claude/settings.local.json`. |

## Pending manual acceptance

Every checkbox below is retained and pending. Use a disposable Windows account/VM and scratch
`OPIT_DATA_DIR`, a real microphone, working provider keys and Task Manager as applicable.
Scratch data does not isolate credentials, registry, clipboard or installer state; use disposable
credentials and never run destructive checks against the owner's installation. Record build SHA,
OS/device, date, pass/fail and measured values without transcripts or API keys.

Current expectations override obsolete wording in the historical plans: clipboard restore is
opt-in and only restores an unchanged clipboard; autostart needs both `ui.autostart` and
`ui.setup_done`; microphone-loss salvage and patient retries exist. Tray/own-window stops are
clipboard-only. With an elevated target, begin recording in a normal window and allow the
recording time limit to stop it: the global hook cannot see elevated-window keys (UIPI).

### Plan 1 — Task 16 Step 15

- [ ] **M1-15 — real-provider baseline.** Prerequisites: personal WAV/TXT dataset and provider keys; follow [eval guide](../guides/eval.md). Compare Groq default, turbo and OpenAI gpt-transcribe with rules on/off, tr-core/tr-tech/fivem and personal rules. Record WER, term accuracy and median latency in metrics-only output for the future baseline (historical requested destination: `docs/eval-baseline.md`, not yet produced). Never commit personal recordings/transcripts. Default-model selection remains evidence-dependent.

### Plan 2 — Task 15 Steps 2–8

- [ ] **M2-02 — build/start/recovery.** Build a release exe in a separate target directory; store a disposable key securely. Start with valid scratch config and personal rules: tray/window appear, config is preserved, metadata logs reach ready without unexpected warnings; prompt context and enabled packs are picked up after restart.
- [ ] **M2-03a — toggle.** In Notepad, Right Ctrl + Right Shift starts sound/level overlay; speaking and pressing it again inserts rule-corrected text and configured trailing space, with completion sound/timing.
- [ ] **M2-03b — latency.** Record five five-second dictations using a release build and real Groq; report the five timings and median. Original acceptance target: median < 1.5 s, not a measured result.
- [ ] **M2-03c — lone Ctrl/Escape.** After releasing the combo, a lone Ctrl tap stops; Escape during recording/transcription cancels without later paste and is swallowed where supported. Idle Ctrl+C/Ctrl+V never triggers dictation.
- [ ] **M2-04a — silence/limit/PTT.** Immediate stop (< 0.4 s) and three seconds of silence send no provider request; max_seconds=10 stops at ten seconds; push-to-talk starts on press and sends on release; restore settings afterwards.
- [ ] **M2-04b — clipboard.** With restoration explicitly enabled, copy a marker, dictate and wait: original formats return and dictated text is excluded from clipboard history where supported. A new copy during transcription/restore is preserved. With restoration off, the transcript stays available. Exercise formats/delayed rendering from P2-02/P2-12.
- [ ] **M2-04c — stop-time safety/UIPI.** Check normal, own, tray, shell and elevated targets; unsafe stops stay clipboard-only with hint and no synthesized input. Start in a normal window and auto-stop in an elevated one; manual Ctrl+V must recover the text.
- [ ] **M2-04d — microphone.** Missing configured device falls back with warning; USB unplug mid-recording keeps app alive and exposes Try again for captured audio (salvage), without claiming the original lost-device wording is the whole current contract.
- [ ] **M2-04e — keys/network/retry.** Wrong key yields safe invalid-key error and Settings link; corrected key plus Try again resends retained audio. Missing key refuses before recording. Network loss enters cancellable patient waits (10/30/60/120 s after quick retry/fallback; Retry-After can lengthen waits); reconnect/retry recovers. Verify failed History/audio behavior with History enabled/disabled.
- [ ] **M2-04f — fallback.** Configure two real profiles, fail the primary transiently, verify fallback output and history profile; restore configuration. Do not send real requests during layout automation.
- [ ] **M2-05a — tray.** Left click toggles, profile selection persists/checkmarks follow, Pause disables/resumes shortcuts; Quit exits.
- [ ] **M2-05b — window/RAM/single instance.** Close WebView, wait about ten seconds, require no WebView2 children and record private working set (original target < 40 MB). Open restores window; a second instance focuses the existing window without another tray icon. Double-Open race remains P2-06.
- [ ] **M2-06a — history/privacy.** History writes rows; Save audio creates WAVs; History disabled writes no new dictation/audio. Inspect logs privately for a distinctive dictated word and key fragment: neither appears. Do not paste search results into reports.
- [ ] **M2-06b — damaged data.** Corrupt scratch config: backup .bad-* and defaults plus notice. Broken personal rules: notice and built-in dictation still works; repair files and confirm recovery. Locked valid config is P2-09.
- [ ] **M2-07 — autostart.** Release build only: Run value exists only after Finish/Skip with autostart enabled, points to quoted installed exe plus --autostart, sign-in starts tray-only; disabling removes it. Debug builds must not register it.
- [ ] **M2-08 — ignored probes.** Discover the current ignored tests and run applicable native credential/sound/mic/hotkey/clipboard/overlay probes serially on a suitable Windows desktop. Record actual names/counts and prerequisites; the old plan's “8 passed” is not a current discovery count.

### Plan 3 — Task 12 Step 2 and page-level passes

- [ ] **M3-01 — wizard.** Fresh data → language/provider/key test/mic/shortcut/rules/real Notepad dictation → Finish in < 2 minutes (measure); wrong key blocks until Continue anyway, Back permits correction; close/reopen midway resumes safe persisted configuration; Finish and Skip prevent wizard on restart. Sources: Task 11 Step 4, Task 12 Step 2.1.
- [ ] **M3-02 — Home.** Hotkey/button status and recent list update live, tray profile changes follow, missing key warning appears after reopen, History off is clear. Source: Task 6 Step 4 / Task 12 Step 2.2.
- [ ] **M3-03 — History.** Turkish search finds İstanbul, load-more has no duplicates and new dictations update live; delete/clear confirmations support Escape; saved audio plays and switches cleanly. Select a partial misrecognition (expands to words), add correction, preserve user.yaml header and confirm next real dictation uses it; identical correct/wrong is rejected. Source: Task 7 Step 5 / Task 12 Step 2.3.
- [ ] **M3-04 — Rules.** Pack toggle changes budget; broken YAML remains editable and cannot enter Table view; table/YAML round trip keeps header; invalid regex saves with skipped-rule warning, text preview shows corrected output/hit, dirty banner tracks changes. Source: Task 8 Step 5 / Task 12 Step 2.4.
- [ ] **M3-05 — Profiles.** Wrong/correct key Test reports safe error/Connected; save stores key and clears warning/input. Keyless HTTP custom profile warns; unreachable endpoint reports safe network error. Real fallback works, deleting fallback clears references; tray active-profile change does not spuriously mark another draft changed; active profile cannot be deleted. Verify credential deletion only for the appropriate refs. Source: Task 9 Step 4 / Task 12 Step 2.5.
- [ ] **M3-06 — Settings.** Language changes UI and next native tray/overlay status. Capturing current combo never dictates; typing-only A is rejected; a valid alternate combo works in Notepad; close mid-capture restores hook within 30 seconds. Mic test shows moving level/silence verdict, rejects during dictation; numeric max 5 normalizes to 10; History off disables audio switches; debug autostart is disabled. Test clipboard options. Source: Task 10 Step 4 / Task 12 Step 2.6.
- [ ] **M3-07 — reopen.** Close during active dictation then reopen from tray: current state/queue appear, listeners neither disappear nor duplicate. Source: Task 12 Step 2.7.
- [ ] **M3-08 — pause.** Tray Pause shortcut updates the open-window banner without reload; resume follows. Source: Task 12 Step 2.8.
- [ ] **M3-09 — memory.** Measure tray-only private working set after WebView closes (target < 40 MB), do not reuse old test counts or size measurements. Source: Task 12 Step 2.9.
- [ ] **M3-10 — keyboard/contrast/layout.** Every page works using Tab/Shift+Tab/Enter/Space/Escape; inspect dropdowns, badges and radio-card states in both themes and languages, minimum-width layout, and failed settings-save rollback. Source: Task 12 Step 2.10 and index's Tasks 8–11 visual follow-ups.

### Plan 4 — release and signing

- [ ] **M4-01 — install.** Follow the [release checklist](../release/RELEASE-CHECKLIST.md) using the real CI-produced installer in clean Windows 10/11 x64 as a standard user. Confirm internet-mark SmartScreen, <15 MB installer, no UAC, localized pages, install directory/shortcuts, setup timing and no autostart before Finish/Skip. Sources: Task 6 checklist, Task 7 acceptance and archived reports.
- [ ] **M4-02 — update/restart.** Use a real lower installed version; verify offer, busy guard, install/restart/new version, settings/history/rules/keys/Run value preservation. Updater fixture/live download tests never prove execution/restart. Do not create fake public versions merely to test.
- [ ] **M4-03 — uninstall/reinstall.** Verify keep-data uninstall and reinstall, opt-in delete-data removal including configured local folders, cancel-while-running preservation, and update/silent-reinstall guards. Preset/current refs are covered; orphaned credentials may need manual removal. Back up disposable data first.
- [ ] **M4-04 — signing/security.** Confirm independent updater key/password backup securely, then pursue SignPath application/integration separately; preserve updater trust and immutable published assets. Neither snapshot proves Authenticode signing or backup completion.

## Modern feature acceptance carried forward

Each destination below retains the complete source checklist; none is marked passed here.
Use the source's device/provider/session prerequisites and expected results, and record outcomes separately.

- [ ] **F-01 — app-rules.** Complete [Manual acceptance](../reference/app-rules.md#manual-acceptance).

<!-- Retained source checklist; pending, not a new observed result. -->

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

- [ ] **F-02 — cleanup-styles.** Complete [Manual acceptance](../reference/cleanup-styles.md#manual-acceptance).

<!-- Retained source checklist; pending, not a new observed result. -->

Use a throwaway `OPIT_DATA_DIR` and an existing key in Credential Manager. Verify actual
WH_KEYBOARD_LL input while idle, recording and paused (toggle and push-to-talk), direct
selection, cycle order/exclusions, and Escape/capture behavior. Check tray checkmarks,
English/Turkish labels and renamed/custom styles, including overlay timing and truncation
at different DPI settings. Speak EN/TR samples into a text editor for each style, verify
raw makes no cleanup request, verify provider model overrides, then restart and inspect
the persisted selection and History. Exercise a failed config save and a version-5 history
database. Real microphone/desktop/provider behavior is outside the fake and local-HTTP
automated test boundary.

- [ ] **F-03 — live-text.** Complete [Manual acceptance](../reference/live-text.md#manual-acceptance).

<!-- Retained source checklist; pending, not a new observed result. -->

- Verify real microphone rates (especially 44.1/48 kHz), Bluetooth/USB devices, very
  short utterances, long pauses, device loss, cancel and rapid consecutive dictations.
- Verify both providers' real credentials, partial latency, finalization, rate limits,
  network loss and batch recovery; loopback tests do not establish provider acceptance.
- Deepgram Nova-3 Turkish streaming is documented as supported. ElevenLabs realtime
  does not document Turkish support: `language_code=tr` is passed through and requires
  a real provider test. An unsupported language must recover through batch.
- Inspect the overlay at multiple DPIs and in light/dark/high contrast modes: newest
  words, long tokens, ellipsis, timer and waveform. Verify elevated targets, clipboard
  policy, history audio and Edit/Translate with real applications.
- Confirm the UI agent's settings integration preserves the new boolean on saves and
  presents Deepgram/capability accurately. This package does not build that UI.

- [ ] **F-04 — voice-modes.** Complete [Manual acceptance](../reference/voice-modes.md#manual-acceptance).

<!-- Retained source checklist; pending, not a new observed result. -->

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

- [ ] **F-05 — paste-methods.** Complete [Desktop acceptance checks](../reference/paste-methods.md#desktop-acceptance-checks).

<!-- Retained source checklist; pending, not a new observed result. -->

Automated tests use fake platform effects and pure Windows INPUT builders. Complete
these checks on a real desktop without sharing transcript or clipboard contents:

1. Try all clipboard shortcuts in supported editors and terminals, with and without
   trailing space and clipboard restoration. Verify an intervening user copy is
   preserved and mixed queued jobs insert in recording order.
2. Type Turkish letters, emoji, mixed CR/LF/CRLF lines, and a long document. Check
   complete insertion, newline counts, no stuck modifiers, and an unchanged
   clipboard (including non-text formats).
3. For each method, test own-window/tray stops, shell focus, and an elevated target.
   Confirm clipboard fallback and no synthesized insertion or after-key.
4. Test each after choice in an editor and a disposable terminal session. Confirm
   exactly one after-key follows insertion, including before clipboard restoration.
5. Change method/after while recording or waiting in the queue; verify each job uses
   its starting choices. Test Ask Paste, Ask Copy, and History retries with `type`
   and an after-key selected.
6. During a long typed insertion, switch to the app's own window or an elevated
   target; verify interruption without an after-key or a duplicate clipboard paste.

- [ ] **F-06 — media-control.** Complete [Desktop acceptance](../reference/media-control.md#desktop-acceptance).

<!-- Retained source checklist; pending, not a new observed result. -->

Automated tests cover policy, worker ordering/unwind/shutdown, config compatibility,
WinRT timeout behavior, and controller lifecycle using fakes. Real audio and media
applications still require a desktop pass:

1. Start two playing media sessions plus one already paused. In pause mode, begin
   dictation and confirm only the playing sessions pause. On stop, verify that only
   those sessions resume, before transcription completes.
2. During recording, manually resume one session and stop another. Release must
   leave their new states alone. Repeat with Escape, max length, device unplug,
   microphone Ask, timed CLI recording, and app quit.
3. In duck mode, use different per-app volumes on multiple active render endpoints.
   Check scaling at 0%, 20%, and 100%, exclusion of this process/system sounds,
   exact original restoration, and preservation of a manually changed session level.
4. Pause/resume a recording: media remains paused/ducked until stop/cancel. Disable
   media control during that recording: release still restores it; the next clip
   leaves media alone. Settings microphone tests and text/file requests must not
   affect playback.
5. Check a machine with no render endpoint, a closed media app, an app that refuses
   transport control, and an unresponsive media session. Recording must still start;
   failures must expose no application name or media title in logs.

Media applications control their own transport semantics; cancellation of an async
request cannot force a non-cooperating application to undo a delayed effect.
WASAPI/WinRT synchronous interface calls have no per-call cancellation API here;
async operation waits are bounded. Forced process termination/power loss cannot
run restoration guards. Normal quit and Rust unwinding are covered.

- [ ] **F-07 — learning-dictionary.** Source: [feature contract](../reference/learning-dictionary.md). Manual acceptance after the UI is connected: record a real dictation, correct it
in History, verify the original recognized text remains available, reject an
offer and restart, then confirm rule and keyword choices and inspect Rules and
the prompt budget. Verify the learned rule on another microphone dictation and
verify keyword transmission only on profiles with keywords enabled. Repeat with
learning disabled and with an unsaved Rules editor draft during a rules event.

- [ ] **F-08 — dictation-upgrades.** Source: [feature contract](../reference/dictation-upgrades.md). The automated tests use fake devices and local provider responses. A real microphone,
tray menu, focused target application, and clipboard restoration still require a
desktop acceptance pass; no live provider request is needed for automated checks.

- [ ] **F-09 — local-transcription.** Source: [feature contract](../guides/local-transcription.md). Automated checks use synthetic HTTP responses, archives and clocks. Real artifact
downloads, GPU startup, microphone dictation and transcription speed require a
separate manual pass on the target PC.

- [ ] **F-10 — private IPC.** Source: [CLI guide](../guides/cli.md#building). On Windows, run the ignored private_pipe_roundtrip test with a suitable named-pipe runner; verify the user-only transport and privacy boundary. Linux cannot establish Windows named-pipe acceptance.

## Document-retention and link exceptions

All twenty document moves have a destination in the [index](../README.md). The three release
documents are dated historical snapshots, including their original limitations and public CI/download
evidence. Private logs/build paths have been replaced by dated summaries; no downloadable logs are invented.
Both specs and the active layout plan stay in `docs/superpowers/` until Task 12. Their historical
move tables/old-path descriptions are intentional. Both specs' `docs/RELEASE-CHECKLIST.md` prose
references are an intentional pending repair owned by Task 12; their live destination is now
`docs/release/RELEASE-CHECKLIST.md`. The proposed `docs/CODE-SIGNING-POLICY.md` in the signing
notes is a future document, not a broken existing target. No Linux support evidence is inferred
from portable-crate checks; the [Linux status skeleton](linux-status.md) intentionally has no claims.
