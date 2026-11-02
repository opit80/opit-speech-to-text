# Media during microphone recording

Dictation can pause media playback or lower other applications' audio while the
microphone records. Native media operations run on one dedicated MTA worker thread,
with an ordered command queue; recording never waits for that worker.

## UI contract

The complete default object added at the top level of `config.json` is:

```json
{
  "media": {
    "enabled": true,
    "mode": "duck",
    "duck_volume_percent": 50
  }
}
```

| Field | JSON type | Default | Accepted values |
| --- | --- | --- | --- |
| `media.enabled` | boolean | `true` | `true`, `false` |
| `media.mode` | string | `"duck"` | `"pause"`, `"duck"` |
| `media.duck_volume_percent` | integer (Rust `u8`) | `50` | Decode: 0–255; normalize/save: clamp to 0–100 |

Both `AppConfig` and `MediaConfig` use serde defaults: missing objects and missing
individual fields load with the values above. The schema version remains 1.
Unknown enum strings, negative numbers, fractions, values above 255, and incorrect
JSON types are rejected during deserialization. The UI should constrain the
percentage input to integers 0–100 and label it as the percentage of the original
volume retained, not the reduction. Zero means silence; 100 leaves volume unchanged.
There are no new shortcut conflicts or validation dependencies.

There are no new commands, command arguments, error codes, or event names:

- `get_config()`: no JS arguments; returns `AppConfig`, now including `media`.
- `save_config({ config: AppConfig })`: JS argument name `config`; returns the
  normalized `AppConfig`, including `media`. Existing `CommandError { code,
  message, kind }` handling remains: `config` for configuration writes refused or
  failed, `invalid_input` for existing Ask/hotkey validation, `unavailable` for a
  stopped blocking task. Deserialization failures use
  Tauri's existing invalid-argument rejection before the command runs.
- `config-changed`: payload is the complete saved `AppConfig`, now including
  `media`; no media-specific event is emitted.

Expose the enable switch and mode selector; enable the volume control for duck
mode. Preserve the saved percentage when switching modes or disabling the feature.
Settings changes apply to the next recording. An active recording retains its
start-time choice, including the obligation to restore after the switch is disabled.

## Recording lifecycle

Coverage includes normal dictation (hotkey, tray/Home and controller/IPC start),
microphone Ask, and timed CLI microphone recording. Text-only Ask, file
transcription, retries of existing audio, and the Settings microphone test do not
engage media control.

Engagement is queued only after microphone capture successfully starts. User pause
keeps media control engaged; resume does not engage it again. Stop, Escape/cancel,
maximum active recording length, microphone failure, and controller shutdown drop
the recording guard and queue release. Stop/error release is queued before audio
preparation, transcription, cleanup, Ask processing, or paste. A recording or media
worker unwind also restores through a guard. The app's explicit quit path drains
the media worker from `spawn_blocking` before process exit. Repeated engage while
engaged and repeated release are harmless.

Engage/release are asynchronous, so media can be audible briefly at capture start.
A release pending while engagement is running stops further session mutations.

## Native behavior

- Pause uses WinRT `GlobalSystemMediaTransportControlsSessionManager`. It selects
  sessions currently Playing, calls `TryPauseAsync`, and retains the corresponding
  session objects only when the request succeeds (or a failed/timed-out request
  has already left that session Paused). Release calls `TryPlayAsync` only on
  retained sessions still Paused. Sessions already paused/stopped at start are
  untouched. No media keystroke is sent. An unavailable manager produces a static,
  non-sensitive warning and otherwise has no effect.
- Duck enumerates WASAPI sessions on all active render endpoints. It excludes the
  current process, system-sounds sessions, and sessions whose identity cannot be
  determined safely. Each eligible session is set to original volume × percentage
  / 100. Release restores the exact stored original only if the current volume
  still matches the applied value (absolute float round-off tolerance 0.000001).
  Changed levels are left alone. Muting and endpoint master volume are untouched.
  Sessions/endpoints created after engagement are ignored.
- Each WinRT async operation has a 300 ms timeout and receives cancellation on
  timeout. An unresponsive session is skipped; other sessions continue. Session
  references, COM calls, and their disposal stay on the worker thread, including
  restoration during unwind. Logs contain static diagnostics and session counts,
  never app names, media titles, selected text, transcripts, clipboard data, or keys.

## Desktop acceptance

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
