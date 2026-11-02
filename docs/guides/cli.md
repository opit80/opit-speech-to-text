# Opit Speech to Text command-line control

Opit Speech to Text includes a new console tool, `opit.exe`, installed next to
`opit-speech-to-text.exe`. Start the tray app and finish profile setup first.
Settings → Advanced → Folders shows the CLI path with a copy button. The default install path is
`%LOCALAPPDATA%\Opit Speech to Text\opit.exe`. Call the full path, or add its folder
to your own user PATH manually. The installer does not change PATH.

PowerShell examples:

```powershell
$opit = "$env:LOCALAPPDATA\Opit Speech to Text\opit.exe"
& $opit --help
& $opit status --json
& $opit start
& $opit pause
& $opit resume
& $opit stop
& $opit toggle
& $opit cancel
& $opit retry
$text = & $opit record --seconds 5
& $opit record --seconds 3 --no-paste --json
& $opit history list --limit 20 --search "meeting" --json
& $opit history retry 42
& $opit profiles list --json
& $opit profile use groq
& $opit transcribe .\meeting.wav --txt --timestamps --cleanup -o .\meeting.txt
& $opit transcribe .\meeting.mp3 --srt -o .\meeting.srt --json
& $opit version
```

Every verb accepts `--json` before or after its arguments. `version` reports the
CLI version without connecting; the IPC `version` verb reports the running app's
version. Use `opit <verb> --help` for arguments. Exit codes are **0** success,
**1** operation/transport error, **2** usage error, **3** app not running in this
user session. In JSON mode, operation and usage errors go to stdout in the same
versioned response shape. Human-readable errors go to stderr. Help and `--version`
print their usual text.

`start` begins a dictation and `stop` queues it. `toggle` chooses between them.
`pause` and `resume` are idempotent while recording; they pause audio capture, not
the global shortcut. Invalid state changes return a structured error. Start/stop
use the app's normal clipboard-only policy. `retry` retries the last failed
in-memory dictation; `history retry` waits for recovery of the named row and uses
the existing history clipboard policy.

`record` accepts 1–300 seconds of active microphone time, waits through the queue
and returns that recording's final text. It **never pastes or changes the
clipboard**, including when stopped from the hotkey or overlay. `--no-paste`
explicitly selects the same default. Normal history and cleanup settings still
apply. Pause/resume, stop and cancel also control this recording. Windows additionally reserves Escape while recording. A busy
microphone/queue or missing credentials produces an error. Cancellation or app
shutdown terminates the waiting command.

File transcription uses the app's active profile, rules, fallback and stored
credentials through the existing file engine. Text is returned on stdout; `-o`
also writes the rendered UTF-8 text to the specified path. `--json` wraps it as
`result.text`, with duration and cleanup-failure metadata. `--timestamps` prefixes
TXT segments; SRT always requires actual provider timestamps. Missing timestamps
return an error rather than invented timings. At most one CLI file job runs at a
time; microphone dictation and the UI file worker remain independent. File jobs
do not add dictation history. Disconnecting the CLI does not cancel an accepted
job; inspect app status before retrying a mutation after any transport error.

## Local protocol and privacy

The Windows pipe is `\\.\pipe\opit-<user SID>-<session ID>-v1` and is local to the
current Windows user/session. Its protected DACL grants only that user access;
remote clients are rejected with `PIPE_REJECT_REMOTE_CLIENTS`. No TCP listener,
API key transfer or elevated privileges are required. Any program running as the
same user can control the app and read history, so treat stdout and redirected
files as private. Transcript payloads are never written to app/CLI logs.

One UTF-8 newline-delimited JSON request is accepted per connection:

```json
{"version":1,"command":{"verb":"status"}}
```

Responses contain either `{"version":1,"result":...}` or
`{"version":1,"error":{"code":"...","message":"..."}}`. Unsupported versions,
unknown verbs/fields and invalid arguments are rejected. Request frames are at
most 16 KiB, response frames at most 16 MiB, history limits 1–1000, and search
strings at most 1024 UTF-8 bytes. Request reads and response writes time out after
5 seconds. Eight simultaneous connections are serviced; long operations retain
their connection. The CLI waits up to two hours for a response and never retries
a submitted mutation automatically.

## Building

`cargo build -p opit-cli --offline` builds the standalone console tool. Use a
separate `CARGO_TARGET_DIR`, such as `target/agent-build`, if the app is running.
`cargo tauri build` runs `scripts/build-cli.mjs` before the UI build, compiling and
staging the release CLI in `target/cli-bundle/opit.exe`. Tauri's resource mapping
installs it in the same folder as the app; normal uninstall/update resource
handling includes it. A direct release app build needs
`node scripts/build-cli.mjs` first. Debug tests permit an empty staging directory.
The CLI and protocol crates also compile on Linux; Windows pipe control is only
available on Windows. Run the ignored private-pipe round trip explicitly with:

```powershell
cargo test -p opit-ipc --offline private_pipe_roundtrip -- --ignored
```


## Linux desktop command entry

Every Linux app executable accepts `--shortcut <action>` and relays a press over the existing
Unix socket before GTK or single-instance startup. Desktop bindings use the absolute app executable,
or the persistent AppImage path from `APPIMAGE`; a sibling `opit` binary is not required.
If the app is not running, this entry exits quietly with status 0 and does not start it.
A malformed action exits 2; other relay errors exit 1 with a generic message.

```sh
'/absolute/path/opit-speech-to-text' --shortcut dictation
'/absolute/path/Opit Speech to Text.AppImage' --shortcut cancel
```

Actions are `dictation`, `pause` (pause/resume), `cancel`, `ask`, `edit`, `translate`,
`style_cycle` and `style:<id>`. The internal `probe` action is harmless outside an armed setup
probe. Shortcut dictation uses normal hotkey paste semantics; `opit toggle` keeps the console
CLI's existing clipboard-only semantics. Shortcuts respect the enabled/paused state.
See [Linux shortcuts](linux-shortcuts.md) for desktop registration, defaults and acceptance limits.
The AppImage's existing `--cli <opit arguments>` entry remains available for full console CLI access.
