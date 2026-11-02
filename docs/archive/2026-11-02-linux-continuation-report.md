# Linux continuation report (2026-11-02)

Historical snapshot. For current results, see [Linux status](../development/linux-status.md).

## Result and boundaries

Implementation continued in the existing dirty `feat/dictation-upgrades` tree on `5397be5`.
Previous edits were preserved. HEAD remains unchanged; no commit, push, tag or release occurred.
No GUI, desktop screenshot, key injection, clipboard operation, desktop entry installation,
provider request, real credential/app-data read, updater download/install, system package install
or access to `~/.tauri` occurred. Test provider traffic uses local synthetic fixtures; those
requiring a listening socket were blocked by the sandbox.

Native Linux features are **implemented but unobserved** until the owner runs the desktop checks.
Observed results here are compilation, unit tests, script tests and UI static/build checks.
Safe selection capture/voice edit is **not done**; it still fails closed on Linux. Linux hold mode
is intentionally unavailable until release events can be confirmed by the application.

## Files changed in this continuation

Paths below name this continuation's work. Other dirty files predated it and were retained.

| Files | Result |
| --- | --- |
| `Cargo.lock`, `crates/app/Cargo.toml` | Linux-only `reis`, `x11rb`, `gtk`, `gtk-layer-shell`; ashpd RemoteDesktop/shared response features. Compiled locally. Dependency reasons are in linux-status.md. |
| `crates/app/src/platform/linux/ei.rs` | Lazy session-lived RemoteDesktop/libei worker; keyboard-only permission, monotonic EI frames, revocation handling, bounded requests, session cleanup, no persisted restore token. Desktop unobserved. |
| `crates/app/src/platform/linux/input.rs` | Tested rule: fallback only before any input was sent; never duplicate uncertain delivery. |
| `crates/app/src/platform/linux/mod.rs` | Session backend dispatch; libei → ydotool → clipboard on Wayland, XTest on X11; previous clipboard/media/secrets logic retained. Selection remains fail-closed. |
| `crates/app/src/platform/linux/hotkey.rs` | Fixed Wayland cancel id/trigger, collision rejection, binding response validation, Closed/EOF/runtime error reporting. Pure tests pass; portal unobserved. |
| `crates/app/src/platform/linux/x11.rs` | XGrabKey, XKB repeat handling/remapping, actual binding release tracking, XTest preflight, EWMH foreground/list, runtime failure. Pure tests pass; X11 unobserved. |
| `crates/app/src/platform/linux/overlay.rs` | GTK layer-shell or X11 dock overlay; no focus, native input region, cancel/retry/settings, notifications when absent. Anchor tests pass; native display unobserved. |
| `crates/app/src/platform/linux/session.rs` | Tested session classification excludes XWayland foreground fallback; asynchronous tray-host probe. Host probe unobserved. |
| `crates/app/src/platform/linux/autostart.rs` | Tested persistent APPIMAGE path instead of temporary mount; no entry written during this run. |
| `crates/app/src/platform/linux/cli.rs` | Stable quoted AppImage `--cli` invocation; resource CLI executes before GTK/data/single-instance initialization. Path tests pass; packaged execution unobserved. |
| `crates/app/src/platform/mod.rs`, `crates/app/src/app_core/input.rs` | Additive default runtime-error/overlay-availability hooks; Linux Cancel action. Windows defaults preserved. |
| `crates/app/src/commands/app.rs` | Session/runtime capabilities; Linux X11 probe off main thread; persistent AppImage CLI command. Windows capability values preserved. |
| `crates/app/src/commands/session.rs`, `crates/app/src/commands/mod.rs` | Linux-only session recovery and shortcut-probe commands, with registry integration. |
| `crates/app/src/controller/session.rs` | Tested memory-only result snapshots/revisions/dismissal and expiring shortcut probe. |
| `crates/app/src/controller/handle.rs`, `crates/app/src/controller/mod.rs` | Linux session state and safe real-hotkey/CLI probe interception without recording/provider work; Cancel routing. |
| `crates/app/src/controller/delivery.rs`, `crates/app/src/controller/status.rs` | Retain all non-inserted Linux results (including failed clipboard); translated Linux overlay cancel buttons. |
| `crates/app/src/controller/tests/mod.rs` | Recovery with History off/clipboard failure; real-event probe; overlay cancellation. Pass. |
| `crates/app/src/shell/mod.rs`, `crates/app/src/shell/events.rs` | Linux CLI dispatch, real tray-host monitor, capability/hotkey refresh, session recovery change event without text. Windows synchronous startup preserved. |
| `crates/app/src/shell/startup.rs` | Avoid Windows-only `unused_mut` from Linux defaults; native Windows compilation pending. |
| `crates/app/src/shell/updates.rs` | Release AppImage check/install/restart only; native packages and debug remain disabled. Windows installer path retained. Gating tests pass; actual update unobserved. |
| `crates/app/src/shell/i18n.rs` | EN/TR cancel action, included in messages parity test. |
| `crates/app/tauri.linux.conf.json` | Debian CLI path and runtime dependencies. Packaging unobserved. |
| `ui/src/features/home/Home.svelte`, `ui/src/features/home/pending-results.ts`, `ui/src/features/home/pending-results.test.ts` | Full session recovery text with explicit copy/dismiss, native snapshot reload and revision ordering; no-tray advice. Pure tests pass; WebView interaction unobserved. |
| `ui/src/features/setup/steps/StepShortcut.svelte`, `ui/src/features/setup/shortcut-test.ts`, `ui/src/features/setup/shortcut-test.test.ts` | Linux cannot advance before an actual shortcut/CLI probe event; expires after 60 seconds, no recording, toggle-only guidance. Pure helper tests pass; actual hotkey unobserved. |
| `ui/src/features/settings/Settings.svelte` | Runtime tray/overlay/hold availability and Linux explanations. Static checks pass; visual QA unobserved. |
| `ui/src/lib/bridge/api.ts`, `ui/src/lib/bridge/events.ts`, `ui/src/lib/bridge/types.ts` | Additive typed session/capability bridge. |
| `ui/src/lib/state/app.svelte.ts`, `ui/src/preview/guide-preview.ts` | Capability refresh guards against stale/disposed responses; preview mocks updated. |
| `ui/src/lib/i18n/en.ts`, `ui/src/lib/i18n/tr.ts` | Paired Linux recovery/setup/availability/insertion explanations. Parity tests pass. |
| `scripts/release/latest-json.mjs`, `scripts/release/latest-json.test.mjs`, `scripts/release/linux-assets.test.mjs` | Signed/version-verified Linux artifacts and combined manifest/checksums; tampering tests; unchanged Windows names. Tests pass with disposable in-memory fixture keys. |
| `scripts/release/draft-release.mjs`, `scripts/release/draft-release.test.mjs` | Exact verified combined asset set uploaded to a draft only. Tests pass; no external upload ran. |
| `.github/workflows/ci.yml`, `.github/workflows/release.yml` | Full Linux app gate/build; separate signed platform artifacts and one aggregate draft job. YAML parses; remote execution unobserved. |
| `crates/app/linux/PKGBUILD`, `crates/app/linux/PACKAGING.md` | Local-checkout Arch recipe and packaging/CLI/dependency/acceptance instructions. Syntax/metadata checked; no package built. Public checksum-pinned source recipe remains pending. |
| `AGENTS.md`, `docs/development/linux-status.md`, `docs/development/2026-11-02-linux-continuation-plan.md`, this report | Current workflow map, observed evidence and remaining acceptance. |

The product/identifier, app directory, credential names, updater endpoint/public key, Windows
installer asset names, NSIS files and `$UpdateMode` guards were not changed. No Windows platform
file or `tauri.conf.json` differs from HEAD. Windows cross-platform compilation remains unverified.

## Verification

Build/test commands were run sequentially using `CARGO_TARGET_DIR=target/agent-build`; final Rust
runs use `CARGO_BUILD_JOBS=1`. No application process was started. Test fixtures use temporary data;
the final all-target run also explicitly used
`OPIT_DATA_DIR=/tmp/opit-linux-port-5397be5-scratch`.

| Command | Observed result |
| --- | --- |
| `cargo fmt --all --check` | Pass. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, Linux full workspace. |
| `cargo test --workspace` | Fails at `opit-core --lib`: CLI 3 pass; core 217 pass/31 fail because socket binding is denied. Cargo stops at this target. |
| `cargo test --workspace --no-fail-fast` | **509 pass, 91 fail, 3 ignored** across all targets; six failing targets below. 90 failures explicitly report socket bind `PermissionDenied`/`Operation not permitted`; one local-server port test reports `LocalError::Port`, mapped from the same `TcpListener::bind` path. No new Linux behavior test failed in the final run. |
| `node --test scripts/release/*.test.mjs` | Pass: 4 test-file subprocesses. |
| `node --test --test-isolation=none scripts/release/*.test.mjs` | Pass: 19 individual tests, 0 fail. |
| `cd ui && npm run check` | 0 errors, 0 warnings. |
| `cd ui && npm test` | 26 files, 152 tests pass. |
| `cd ui && npm run build` | Pass, 236 modules. |
| `cargo build -p opit-speech-to-text` | Pass; Linux debug executable compiled/linked, not launched. |
| PKGBUILD syntax/metadata, workflow YAML and Linux config JSON | Pass (`bash -n`, `makepkg --printsrcinfo`, YAML/JSON parsing). |
| `git diff --check` | Pass. |

Earlier red development tests and the initial overlay-test expectation failure were corrected;
the table describes final outcomes. The socket failures were not skipped or marked ignored.

| Failing target | Passed | Failed |
| --- | ---: | ---: |
| `opit-core --lib` | 217 | 31 |
| `opit-core --test cleanup_http` | 0 | 30 |
| `opit-core --test cleanup_styles_http` | 0 | 1 |
| `opit-core --test pipeline_http` | 0 | 3 |
| `opit-eval --test run_eval` | 0 | 2 |
| `opit-speech-to-text --lib` | 276 | 24 |

The final app library also has 1 ignored test. Two integration tests are ignored, for 3 total.
All other test targets passed. A network-capable/unrestricted runner is required to establish a
green full workspace; do not interpret these results as a green suite or as an application defect.

## Not done / owner acceptance

- Safe voice-edit selection capture is not implemented; it fails closed on both Linux sessions.
- Session release confirmation/hold-mode enabling remains follow-up work.
- No KDE/GNOME/X11 native interaction or browser/visual QA of the changed UI was performed.
- No AppImage/deb/Arch package, autostart entry, stable packaged CLI, signed replacement/restart,
  clean-distribution helper/dependency check, or remote workflow was executed.
- No Windows build or manual acceptance ran. A public checksum-pinned Arch source recipe awaits
  final published source. Wayland permission behavior across launches and fixed cancel-key desktop
  conflicts need owner checks. See linux-status.md for the detailed acceptance list.

## Files outside the repository

Only scratch artifacts were explicitly written outside the repository: `/tmp/opit-*.log` files
listed below and `/tmp/opit-linux-port-5397be5-scratch/`. Node release fixtures use transient
`/tmp/opit-linux-release-test-*` directories and remove them in `finally`; Rust fixtures use
`tempfile` directories. No persistent desktop/system/user preference was changed.

- `/tmp/opit-autostart-red.log`
- `/tmp/opit-cancel-red.log`
- `/tmp/opit-clippy.log`
- `/tmp/opit-fmt.log`
- `/tmp/opit-linux-build.log`
- `/tmp/opit-linux-check.log`
- `/tmp/opit-linux-green.log`
- `/tmp/opit-linux-lib.log`
- `/tmp/opit-linux-red.log`
- `/tmp/opit-release-detailed.log`
- `/tmp/opit-release-tests.log`
- `/tmp/opit-ui-build.log`
- `/tmp/opit-ui-check.log`
- `/tmp/opit-ui-test.log`
- `/tmp/opit-updater-red.log`
- `/tmp/opit-workspace-all-tests.log`
- `/tmp/opit-workspace-test.log`

## Individual sandbox-blocked failures

- `unittests src/lib.rs`: `files::tests::cleanup_never_repeats_replacements_with_or_without_timestamps`
- `unittests src/lib.rs`: `files::tests::cancellation_drops_an_inflight_request_and_never_starts_another_chunk`
- `unittests src/lib.rs`: `files::tests::file_mode_falls_back_to_plain_json_and_disables_all_subtitles`
- `unittests src/lib.rs`: `files::tests::subtitle_cleanup_http_preserves_markers_and_falls_back_on_shortening`
- `unittests src/lib.rs`: `files::tests::two_chunk_http_job_offsets_segments_and_reuses_retry_ladder`
- `unittests src/lib.rs`: `provider::deepgram::tests::connection_check_is_read_only_and_http_errors_are_safe`
- `unittests src/lib.rs`: `provider::deepgram::tests::native_wav_request_auth_language_and_detection`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::auto_language_and_explicit_keyterms_follow_native_contract`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::connection_and_warmup_are_authenticated_reads_even_on_failure`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::dispatches_scribe_with_native_fields_and_no_paid_addons`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::errors_never_expose_response_values`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::file_options_request_word_timing_and_diarization_without_changing_dictation_defaults`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::redirects_never_forward_credentials_or_audio`
- `unittests src/lib.rs`: `provider::elevenlabs::tests::transcription_preserves_the_rate_limit_header`
- `unittests src/lib.rs`: `provider::local::tests::multipart_is_keyless_and_detected_language_is_normalized`
- `unittests src/lib.rs`: `provider::openai::tests::closed_port_is_a_network_error`
- `unittests src/lib.rs`: `provider::openai::tests::connection_falls_back_to_a_silent_transcription`
- `unittests src/lib.rs`: `provider::openai::tests::connection_ok_via_models_without_transcribing`
- `unittests src/lib.rs`: `provider::openai::tests::connection_reports_a_rejected_key_from_models`
- `unittests src/lib.rs`: `provider::openai::tests::http_statuses_map_to_typed_errors`
- `unittests src/lib.rs`: `provider::openai::tests::non_json_success_is_a_bad_response`
- `unittests src/lib.rs`: `provider::openai::tests::optional_fields_are_left_out`
- `unittests src/lib.rs`: `provider::openai::tests::pasted_key_with_newline_is_trimmed`
- `unittests src/lib.rs`: `provider::openai::tests::rate_limit_header_is_preserved_without_reading_the_error_body`
- `unittests src/lib.rs`: `provider::openai::tests::sends_the_expected_multipart_form`
- `unittests src/lib.rs`: `provider::openai::tests::slow_server_times_out`
- `unittests src/lib.rs`: `provider::openai::tests::uncertain_but_speechy_segments_are_kept`
- `unittests src/lib.rs`: `provider::openai::tests::verbose_json_drops_silent_segments`
- `unittests src/lib.rs`: `provider::openai::tests::warm_up_calls_models_with_the_key_and_ignores_errors`
- `unittests src/lib.rs`: `provider::streaming::tests::local_error_frames_abrupt_disconnects_and_handshake_statuses_are_safe`
- `unittests src/lib.rs`: `provider::streaming::tests::local_servers_verify_partial_audio_and_finish_handshakes_for_both_protocols`
- `tests/cleanup_http.rs`: `connection_errors_retry_once_without_resetting_the_budget`
- `tests/cleanup_http.rs`: `client_errors_are_never_retried`
- `tests/cleanup_http.rs`: `overlap_guard_allows_edits_prompt_restructuring_and_short_or_custom_inputs`
- `tests/cleanup_http.rs`: `both_stalled_attempts_share_one_total_deadline`
- `tests/cleanup_http.rs`: `first_success_wins_in_either_order`
- `tests/cleanup_http.rs`: `expansion_keeps_the_absolute_response_guard_and_other_modes_keep_their_messages`
- `tests/cleanup_http.rs`: `failures_keep_rules_applied_text_and_never_expose_server_body`
- `tests/cleanup_http.rs`: `null_content_is_empty_and_length_is_distinct_even_with_null_content`
- `tests/cleanup_http.rs`: `builtins_reject_unrelated_meta_replies_and_keep_rules_applied_text`
- `tests/cleanup_http.rs`: `equally_slow_valid_responses_succeed_for_cleanup_voice_modes_and_ask`
- `tests/cleanup_http.rs`: `http_errors_have_safe_specific_kinds_without_retrying`
- `tests/cleanup_http.rs`: `cleanup_and_voice_mode_token_budgets_have_reasoning_headroom_and_a_cap`
- `tests/cleanup_http.rs`: `hedge_race_keeps_the_pending_success_when_either_attempt_fails`
- `tests/cleanup_http.rs`: `expansion_limits_and_token_budgets_follow_the_style`
- `tests/cleanup_http.rs`: `invalid_key_marker_is_detected_only_within_the_bounded_prefix`
- `tests/cleanup_http.rs`: `review_meta_replies_are_rejected_even_when_they_quote_short_inputs`
- `tests/cleanup_http.rs`: `request_shape_glossary_and_final_casing`
- `tests/cleanup_http.rs`: `prompt_rejects_new_instruction_markers_without_rejecting_dictated_markers`
- `tests/cleanup_http.rs`: `review_overlap_is_only_required_for_clean_and_prompt_styles`
- `tests/cleanup_http.rs`: `stalled_400_body_is_rejected_before_the_hedge_starts`
- `tests/cleanup_http.rs`: `review_frame_avoids_literal_tags_and_strips_one_sided_echoes`
- `tests/cleanup_http.rs`: `stalled_attempt_counts_and_deadlines_at_timeout_boundaries`
- `tests/cleanup_http.rs`: `stalled_first_attempt_is_recovered_by_one_hedge_within_the_total_budget`
- `tests/cleanup_http.rs`: `timeout_and_omitted_reasoning_and_disabled_guards`
- `tests/cleanup_http.rs`: `review_dictated_transcript_requests_and_missing_transcript_statements_are_preserved`
- `tests/cleanup_http.rs`: `review_turkish_suffix_rewrites_keep_a_shared_stem`
- `tests/cleanup_http.rs`: `transcript_framing_and_echo_stripping_also_cover_custom_instructions`
- `tests/cleanup_http.rs`: `terminal_errors_do_not_hedge_for_any_shared_transport_caller`
- `tests/cleanup_http.rs`: `prompt_accepts_lightly_edited_chat_and_first_person_multi_part_tasks_in_both_languages`
- `tests/cleanup_http.rs`: `prompt_instruction_leak_keeps_rules_applied_text_for_the_reported_greeting`
- `tests/cleanup_styles_http.rs`: `named_style_resolves_model_and_language_prompt_with_shared_glossary`
- `tests/pipeline_http.rs`: `groq_style_round_trip_with_rules`
- `tests/pipeline_http.rs`: `a_shared_client_works_inside_a_spawned_task`
- `tests/pipeline_http.rs`: `http_fallback_after_two_server_errors`
- `tests/run_eval.rs`: `rules_on_beats_rules_off_on_the_same_audio`
- `tests/run_eval.rs`: `silent_samples_are_reported_without_a_request`
- `unittests src/lib.rs`: `app_core::tests::cli_file_dispatch_uses_the_shared_engine_and_requires_real_timestamps`
- `unittests src/lib.rs`: `ask::tests::chat_cancel_and_timeout_do_not_commit_partial_turns`
- `unittests src/lib.rs`: `ask::tests::chat_http_failures_keep_their_kind_without_saving_private_turns`
- `unittests src/lib.rs`: `ask::tests::chat_request_shape_continuation_and_reset_use_only_the_ask_key`
- `unittests src/lib.rs`: `controller::tests::ask_request_cleanup_is_separate_from_dictation_cleanup_and_keeps_original_history`
- `unittests src/lib.rs`: `controller::tests::ask_test_uses_fixed_request_without_paste_history_or_conversation_changes`
- `unittests src/lib.rs`: `controller::tests::ask_text_and_voice_share_the_queue_and_save_request_answer_and_agent`
- `unittests src/lib.rs`: `controller::tests::cleanup_success_is_saved_and_escape_during_cleanup_pastes_rules_applied_input`
- `unittests src/lib.rs`: `controller::tests::escape_after_cleanup_completion_overrides_success_and_failure_before_delivery`
- `unittests src/lib.rs`: `controller::tests::escape_during_ask_cancels_without_paste_or_dictation_retry`
- `unittests src/lib.rs`: `controller::tests::escape_during_voice_modes_or_ask_cleanup_still_cancels_fully`
- `unittests src/lib.rs`: `controller::tests::live_tests::streaming_finals_continue_through_edit_translate_and_cleanup`
- `unittests src/lib.rs`: `controller::tests::saving_style_metadata_while_holding_each_recording_shortcut_keeps_the_stop_release`
- `unittests src/lib.rs`: `controller::tests::second_escape_after_skipping_cleanup_cancels_the_pending_dictation`
- `unittests src/lib.rs`: `controller::tests::voice_edit_with_or_without_selection_uses_connection_and_saves_originals`
- `unittests src/lib.rs`: `controller::tests::voice_llm_failure_never_pastes_and_retains_failed_edit_or_translation`
- `unittests src/lib.rs`: `controller::tests::voice_modes_keep_per_app_profile_fallback_and_paste_but_ignore_style`
- `unittests src/lib.rs`: `controller::tests::voice_push_to_talk_releases_only_its_own_mode_and_can_pause`
- `unittests src/lib.rs`: `controller::tests::voice_translation_uses_rules_target_and_model_without_style_or_selection`
- `unittests src/lib.rs`: `local::runtime::server::tests::port_is_ephemeral_and_idle_zero_never_stops`
- `unittests src/lib.rs`: `local::tests::existing_retry_ladder_supports_cloud_and_local_fallback_in_both_directions`
- `unittests src/lib.rs`: `local::tests::shared_local_server_preserves_language_without_keys_and_updates_idle_time`
- `unittests src/lib.rs`: `providers::tests::cleanup_clients_reuse_connections_and_refresh_when_key_or_settings_change`
- `unittests src/lib.rs`: `providers::tests::warm_up_touches_the_models_endpoint`
