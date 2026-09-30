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

## Measuring accuracy

See [`docs/eval.md`](docs/eval.md) for building a personal dataset and running `opit-eval`.

## License

[MIT](LICENSE)
