# Live text while speaking

The native recording overlay can preview partial speech recognition from ElevenLabs
Scribe v2 Realtime or Deepgram Nova-3. Preview text never enters the target application.
The timer and waveform remain visible; the preview keeps up to 120 Unicode characters
and fits the newest text into two lines using native font metrics.

## UI contract

The only new recording setting is:

```json
{
  "recording": {
    "live_text": false
  }
}
```

`recording.live_text` is a boolean, defaults to `false`, and is filled by serde when
absent from older or partial configs. Save it through the existing `save_config`
command. It uses the effective profile captured at recording start, including per-app
overrides. Unsupported providers silently retain batch transcription. No hotkey or
additional validation rules are introduced.

`Profile.provider` adds the enum value `"deepgram"`. The complete allowed set is now
`"openai_compatible"`, `"elevenlabs"`, `"local_whisper"`, `"deepgram"`; an absent
provider still defaults to `"openai_compatible"`. No fields were added to `Profile`.

`profile_presets()` retains its `Profile[]` return type and now includes this preset
after ElevenLabs and before Local:

```json
{
  "provider": "deepgram",
  "id": "deepgram",
  "name": "Deepgram",
  "base_url": "https://api.deepgram.com/v1",
  "model": "nova-3",
  "api_key_ref": "deepgram",
  "language": "tr",
  "audio_format": "wav",
  "response_format": "json",
  "send_prompt": false,
  "send_keywords": false,
  "apply_rules": true,
  "apply_cleanup": true,
  "fallback_profile_id": null
}
```

Deepgram profiles are normalized to `audio_format: "wav"` on config load/save and
inside the native adapter. The UI should disable other audio formats for this provider.
`language: "auto"`, `"multi"`, or an empty string selects Deepgram's multilingual
detection; a fixed language such as `"tr"` is passed directly. ElevenLabs uses the
existing profile key and language; `"auto"` or empty requests language detection.
`send_keywords: true` enables a conservative, bounded streaming keyterm list from the
existing rules (at most 32 terms, 48 characters per term and 384 total characters).
The presets leave this optional feature off. API keys continue to use the existing
Credential Manager commands and never enter config JSON.

New Tauri command:

| Name | JS arguments | Return | Rejection |
|---|---|---|---|
| `streaming_supported` | `{ profileId: string }` | `boolean` | `CommandError { code: "invalid_input", message: "unknown profile", kind: null }` for an unknown id; `code: "unavailable"`, `kind: null` if the blocking worker stops |

The command returns true for saved ElevenLabs and Deepgram profiles and false for
other kinds. It reports capability independently of `recording.live_text`. No
credentials or network requests are needed. Unsaved profile drafts can use the same
provider-kind check locally.

Existing `test_connection(profile, apiKey)` now accepts Deepgram profiles. Its JS
arguments, return type and safe error mapping are unchanged. The native adapter
uses authenticated `GET <base_url>/projects`, without uploading audio. A key restricted
from project listing can fail this check even if it permits transcription.

No new or changed events. Live text is consumed only by the native overlay. The Home
page receives the existing dictation status events; there is no `live-text` event.
No UI files are changed by this package.

## Recording lifecycle

Capture starts without waiting for the socket. An atomic capture subscription replays
audio already captured and then queues new mono samples at the device rate. A bounded
worker queue buffers connection startup; overflow invalidates the live result. The
worker incrementally resamples to 16 kHz with the same anti-aliasing kernel as batch
preparation, emits signed 16-bit little-endian PCM in 100 ms chunks, and retains only
the filter overlap. Capture callbacks only enqueue; credential access is offloaded
to `spawn_blocking`. WebSocket TLS selects an explicit ring crypto provider and uses
the platform certificate verifier.

Paused capture contributes no samples. Deepgram receives KeepAlive every four seconds,
including during pauses. Cancel, Escape, shutdown, microphone loss or worker abort
drops the session without committing it. Session ids prevent stale previews from
overwriting another recording's overlay.

On stop the session finalizes immediately, including when the dictation is queued
behind earlier work. Finalization is limited to four seconds from stop. ElevenLabs
receives an empty final audio chunk with `commit: true`; Deepgram receives `Finalize`,
waits for `from_finalize: true`, then receives `CloseStream`. Committed segments
accumulate; interim revisions replace the current partial.

The full recording always passes through the existing speech gate before a live final
is used. Successful finals use the shared hallucination/rule/number processing, cleanup
style, Edit/Translate/Ask processing, paste policy, history and prepared-audio retention.
A connection, authentication, send, server-frame or finalization failure, timeout,
overflow, or empty final falls back to the full recorded audio through the existing
batch retry ladder and fallback profile. Only a safe error code is logged. Preview,
final text, selection, provider error bodies and keys never enter diagnostics.

## Manual acceptance

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

## Implementation files

- Dependencies: `Cargo.lock`, `crates/core/Cargo.toml`.
- Core: `crates/core/src/config.rs`, `crates/core/src/audio/resample.rs`,
  `crates/core/src/pipeline.rs`, `crates/core/src/provider/mod.rs`,
  `crates/core/src/provider/profile.rs`.
- New native providers: `crates/core/src/provider/deepgram.rs`,
  `crates/core/src/provider/streaming.rs`, `crates/core/src/provider/streaming/tests.rs`.
- App integration: `crates/app/src/commands.rs`, `crates/app/src/providers.rs`,
  `crates/app/src/file_jobs.rs`, `crates/app/src/platform/windows/uninstall.rs`,
  `crates/app/src/controller/mod.rs`, `crates/app/src/controller/recording.rs`,
  `crates/app/src/controller/transcription.rs`, `crates/app/src/controller/status.rs`,
  `crates/app/src/controller/tests/mod.rs`.
- New recording worker/tests: `crates/app/src/controller/live.rs`,
  `crates/app/src/controller/tests/live_tests.rs`.
- Platform integration: `crates/app/src/platform/mod.rs`,
  `crates/app/src/platform/fake.rs`, `crates/app/src/platform/microphone.rs`,
  `crates/app/src/platform/windows/overlay.rs`.
- Overlay constructor updates: `crates/app/src/app_core/mod.rs`, `crates/app/src/shell/mod.rs`.
- Contract and acceptance notes: `docs/reference/live-text.md`.

## Verification

Executed sequentially from the repository root on Windows with
`CARGO_NET_OFFLINE=true`, `CARGO_BUILD_JOBS=1`, and
`CARGO_TARGET_DIR=.superpowers/cargo-target`:

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed, no warnings.
- `cargo test --workspace`: passed, 582 tests passed and 15 ignored in total.

| Crate | Unit tests passed | Integration tests passed | Ignored |
|---|---:|---|---:|
| `opit-core` | 244 | `cleanup_http`: 3; `cleanup_styles_http`: 1; `pipeline_http`: 3 | 0 |
| `opit-eval` | 9 | `run_eval`: 2 | 0 |
| `opit-ipc` | 6 | none | 1 |
| `opit-speech-to-text` / `opit_app` | 303 | `logging_init`: 1; `tauri_conf`: 7 | 12 unit; 1 `updater_artifacts`; 1 `updater_live` |
| `opit-cli` | 3 | none | 0 |

Binary targets with no tests and all four documentation-test suites reported zero
tests and no failures. The ignored suites require explicit desktop/hardware or updater
acceptance. Focused core and controller tests also passed before the workspace pass.
No live provider requests, application launches, installations or Linux builds were run.
