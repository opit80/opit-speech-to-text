# Opit Speech to Text

Open-source voice dictation for Windows. Press a hotkey, speak, and the text is pasted where your
cursor is. Transcription runs on **your own API key** (Groq, OpenAI, or any OpenAI-compatible
server); nothing goes through a third-party server of ours.

> **Status:** early development. The design lives in
> [`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`](docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md).

## Repository layout

| Path | What |
|---|---|
| `crates/core` | `opit-core`: audio prep, provider client, rule engine, history — no UI, no OS code |
| `crates/eval` | `opit-eval`: WER / term accuracy of a provider profile on your own recordings |
| `crates/app` | `opit-speech-to-text`: the Windows tray app (Tauri 2) — hotkey, microphone, paste, overlay, tray |
| `ui/` | Svelte 5 front end of the app window |
| `rules/` | Built-in rule packs (`tr-core`, `tr-tech`, `fivem`) |
| `docs/` | Design brief and implementation plans |

## Development

Requirements: Windows 10/11, Rust 1.90+, Node 24, and the Tauri CLI
(`cargo install tauri-cli --version "^2.12" --locked`).

```sh
cargo test --workspace          # all Rust tests
cd ui && npm install && cd ..   # once
cargo tauri dev                 # run the app with the Vite dev server
cargo tauri build --no-bundle   # release exe in target/release/
```

Set `OPIT_DATA_DIR` to run against a scratch data folder instead of
`%APPDATA%\opit-speech-to-text`.

## Trying the preview

The settings UI is not built yet; the app runs from a hand-edited config.

1. Build and start it: run `npm install` in `ui/` once (see [Development](#development)), then
   `cargo tauri build --no-bundle`, then run `target\release\opit-speech-to-text.exe`. The first
   start writes `%APPDATA%\opit-speech-to-text\config.json` with defaults (Groq, Turkish, Right
   Ctrl + Right Shift, toggle mode) and puts an icon in the tray.
   `ui.autostart` defaults to `true`, so the release exe also adds itself to the Windows Run key
   (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Opit Speech to Text`) on first
   start. To turn that off, set `"ui": { "autostart": false }` in `config.json`, then quit the
   app from the tray and start it again; it removes the value on that start.
2. Store your API key in Windows Credential Manager (the command prompts for it, so it does
   not land in your shell history). `groq` is the profile's `api_key_ref`:
   ```
   cmdkey /generic:groq.opit-speech-to-text /user:groq /pass
   ```
3. Edit `config.json` if you like (for example `rules.prompt_context`, `rules.enabled_packs`,
   `hotkey.mode: "push_to_talk"`, `recording.microphone`), then quit the app from the tray and
   start it again.
4. Press **Right Ctrl + Right Shift**, speak, press it again (or tap **Ctrl** alone). The text is
   pasted where your cursor is. **Esc** cancels.

Logs are in `%APPDATA%\opit-speech-to-text\logs` (7 days). They never contain what you said.

## Measuring accuracy

See [`docs/eval.md`](docs/eval.md) for building a personal dataset and running `opit-eval`.

## License

[MIT](LICENSE)
