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

Developer notes:

- Set `OPIT_DATA_DIR` to run against a scratch data folder instead of
  `%APPDATA%\opit-speech-to-text`. An empty folder starts like a fresh install, wizard included.
- API keys live in Windows Credential Manager. The Profiles page and the wizard store them there,
  but you can also store one by hand. The command prompts for the key, so it does not land in
  your shell history. `groq` is the profile's `api_key_ref`:
  ```
  cmdkey /generic:groq.opit-speech-to-text /user:groq /pass
  ```
- Debug builds never register autostart, so the "Start with Windows" switch is disabled there.

## Using the app

Build and start it: run `npm install` in `ui/` once (see [Development](#development)), then
`cargo tauri build --no-bundle`, then run `target\release\opit-speech-to-text.exe`. The app puts
an icon in the tray and opens its window.

**First run.** A setup wizard opens on the first start:

1. **Language**: the language of the app's menus and messages (English or Turkish). The language
   you dictate in is set per profile.
2. **Provider**: Groq (recommended), OpenAI or your own OpenAI-compatible server. Paste your API
   key and test the connection.
3. **Microphone**: pick a device and check that the level meter moves.
4. **Shortcut**: the default is **Right Ctrl + Right Shift**; you can change it and the mode here.
5. **Rules and startup**: turn on the built-in rule packs you want, and choose whether the app
   starts with Windows.
6. **Try it**: dictate one sentence into Notepad, or start a test from the wizard itself.

**Skip setup** keeps the defaults. Until the wizard is finished or skipped, it opens again
whenever the window opens.

**Dictating.** Press the shortcut, speak, then press it again (or tap **Ctrl** alone). The text is
pasted where your cursor is. **Esc** cancels. In push-to-talk mode you hold the shortcut while you
speak. A dictation started from the app's own window goes to the clipboard instead of being
pasted.

**Pages** (left sidebar):

| Page | What it does |
|---|---|
| Home | Dictation status, a start/stop button, a quick profile switch and the latest dictations. Warns when the active profile has no API key. |
| History | Search every dictation (Turkish letters match loosely: `ı`/`i`, `ş`/`s` …), copy, delete, clear, and play the audio when audio saving is on. Select a misrecognised word and use **Add correction rule** to fix it in future dictations, with a before/after preview. |
| Rules | Turn the built-in rule packs (`tr-core`, `tr-tech`, `fivem`) on or off, set the context sentence, and watch the prompt budget gauge. Edit your personal rules (`user.yaml`) as a table or as raw YAML, and try them on any text before saving. |
| Profiles | Transcription providers from presets (Groq, OpenAI, custom server): base URL, model, language, prompt options and a fallback profile. Store or remove the API key and test the connection. |
| Settings | Interface language, start with Windows, start in the tray, sound feedback, the shortcut (capture a new one, mode, pause), microphone with a level test, maximum recording length, paste options, overlay position, history, audio saving and retention. **About** shows the version and the data and log folders. |

Settings are saved as soon as you change them. Profiles and the rules editor have a **Save**
button. Closing the window keeps the app running in the tray; open it again from the tray icon.

**Where settings live.** Everything is under `%APPDATA%\opit-speech-to-text\`: `config.json`
(settings and profiles), `rules\user.yaml` (your personal rules), `history.db`, `audio\` (only
when audio saving is on) and `logs\` (7 days). API keys are kept only in Windows Credential
Manager. The logs never contain what you said or your keys.

`ui.autostart` defaults to `true`, so the release exe adds itself to the Windows Run key
(`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Opit Speech to Text`) on its first
start, before the wizard runs. Turning off **Start with Windows** (in the wizard or in Settings)
removes the value.

## Measuring accuracy

See [`docs/eval.md`](docs/eval.md) for building a personal dataset and running `opit-eval`.

## License

[MIT](LICENSE)
