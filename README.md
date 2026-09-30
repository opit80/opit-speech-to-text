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
| `rules/` | Built-in rule packs (`tr-core`, `tr-tech`, `fivem`) |
| `docs/` | Design brief and implementation plans |

## Development

```sh
cargo test --workspace
```

## License

[MIT](LICENSE)
