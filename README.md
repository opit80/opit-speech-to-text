# Opit Speech to Text

Open-source voice dictation for Windows. Press a hotkey, speak, and the text is pasted where your
cursor is. Transcription runs on **your own API key** (Groq, OpenAI, or any OpenAI-compatible
server); nothing goes through a third-party server of ours.

> **Status:** v1 is in pre-release. Installers will be on the
> [Releases page](https://github.com/opit80/opit-speech-to-text/releases). The design lives in
> [`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`](docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md).

## Install

Requirements: Windows 10 or 11, x64.

Download `opit-speech-to-text_<version>_x64-setup.exe` from the
[Releases page](https://github.com/opit80/opit-speech-to-text/releases) and run it. It installs
for your user only, in `%LOCALAPPDATA%\Opit Speech to Text`, without an admin prompt. If the
WebView2 runtime is missing, the installer downloads and adds it (this needs internet).

**SmartScreen.** v1 installers are not code-signed yet, so Windows may show "Windows protected
your PC". Click **More info → Run anyway**. Code-signing certificates are paid, so the project is
applying for free open-source signing through SignPath Foundation. Updates are signed regardless:
the app installs only an update whose signature matches the key built into it.

The last installer page can start the app (and create a desktop shortcut). The setup wizard
follows on the first start.

## Using the app

Install it (see [Install](#install)), or, for development, build it as described in
[Development](#development). The app puts an icon in the tray and opens its window.

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
| Settings | Interface language, start with Windows, start in the tray, sound feedback, the shortcut (capture a new one, mode, pause), microphone with a level test, maximum recording length, paste options, overlay position, history, audio saving and retention. **Updates** checks for and installs new versions. **About** shows the version and the data and log folders. |

Settings are saved as soon as you change them. Profiles and the rules editor have a **Save**
button. Closing the window keeps the app running in the tray; open it again from the tray icon.

**Where settings live.** Everything is under `%APPDATA%\opit-speech-to-text\`: `config.json`
(settings and profiles), `rules\user.yaml` (your personal rules), `history.db`, `audio\` (only
when audio saving is on) and `logs\` (7 days). API keys are kept only in Windows Credential
Manager. The logs never contain what you said or your keys.

**Start with Windows** (on by default) is applied when the setup wizard is finished or skipped,
never before. It writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
`Opit Speech to Text`, and turning the switch off removes it.

## Updates

The installed app asks GitHub for a newer version 20 seconds after it starts and then once a day
while it runs. Settings → Updates → **Check for updates automatically** controls this; turn it
off and the app never contacts GitHub on its own (**Check now** still works).

When a newer version exists, a banner and Settings → Updates offer **Install and restart**. The
app downloads the update, checks its signature, closes, runs the installer with a small progress
window and starts again. Your settings, history, rules, API keys and the "Start with Windows"
entry are kept. The button is disabled while a dictation is recording, transcribing or pasting:
finish the dictation first.

You can also update by hand: download the newer installer and run it over the old one.

## Uninstall

Windows Settings → Apps → Installed apps → *Opit Speech to Text* → Uninstall.

The uninstaller always removes the program, its shortcuts and the "Start with Windows" entry. Tick
**Delete the application data** to also remove:

- `%APPDATA%\opit-speech-to-text\` (settings, `rules\user.yaml`, history, audio, logs),
- the WebView data in `%LOCALAPPDATA%\io.github.opit80.opit-speech-to-text`,
- the API keys this app stored in Windows Credential Manager.

**Back up `rules\user.yaml` first if you want to keep your rules.**

If you uninstalled without ticking the box, the data stays for a reinstall. To remove it by hand,
delete the two folders above and the Credential Manager entries whose names end in
`.opit-speech-to-text`: list them with `cmdkey /list`, then delete each one, for example
`cmdkey /delete:groq.opit-speech-to-text`.

## Privacy

Your recorded audio goes only to the provider of the active profile (or to its fallback profile,
if you set one). The only other network request the app makes is the update check to GitHub,
which you can turn off (see [Updates](#updates)). There is no telemetry. The logs never contain
what you said or your API keys.

## Repository layout

| Path | What |
|---|---|
| `crates/core` | `opit-core`: audio prep, provider client, rule engine, history — no UI, no OS code |
| `crates/eval` | `opit-eval`: WER / term accuracy of a provider profile on your own recordings |
| `crates/app` | `opit-speech-to-text`: the Windows tray app (Tauri 2) — hotkey, microphone, paste, overlay, tray |
| `ui/` | Svelte 5 front end of the app window |
| `rules/` | Built-in rule packs (`tr-core`, `tr-tech`, `fivem`) |
| `scripts/release/` | `latest-json.mjs`: release assets and the updater manifest |
| `docs/` | Design brief, implementation plans and the [release checklist](docs/RELEASE-CHECKLIST.md) |

## Development

Requirements: Windows 10/11, Rust 1.90+, Node 24, and the Tauri CLI
(`cargo install tauri-cli --version 2.12.0 --locked`).

```sh
cargo test --workspace          # all Rust tests
cd ui && npm install && cd ..   # once
cargo tauri dev                 # run the app with the Vite dev server
cargo tauri build --no-bundle   # release exe in target/release/
```

`cargo tauri build` (with bundling) builds the installer and signs the updater artifacts, so it
needs `TAURI_SIGNING_PRIVATE_KEY` (the private key, or the path to its file) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Without the key, use `cargo tauri build --no-bundle` (exe
only) or `cargo tauri build --no-sign` (an unsigned installer, which cannot serve as an update).

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
- Debug builds never check for updates and cannot install one; Settings → Updates says
  "Updates work only in the installed app".

## Releasing

For maintainers:

1. Bump `version` under `[workspace.package]` in `Cargo.toml` and commit it (with the updated
   `Cargo.lock`) on `main`.
2. `git tag vX.Y.Z` (exactly `v` plus that version) and push the tag. The *Release* workflow
   (`.github/workflows/release.yml`) checks the tag against `Cargo.toml`, builds the signed
   installer and `latest.json`, and attaches them to a **draft** release.
3. Run [`docs/RELEASE-CHECKLIST.md`](docs/RELEASE-CHECKLIST.md) on a clean Windows with the
   draft's installer, then publish the draft. Publishing is what makes
   `https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json` visible
   to the installed apps; drafts are ignored.

Repository secrets: `TAURI_SIGNING_PRIVATE_KEY` (the content of the private key file) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never regenerate the key: installed apps accept only updates
signed with it, so a new key strands every installed copy.

## Measuring accuracy

See [`docs/eval.md`](docs/eval.md) for building a personal dataset and running `opit-eval`.

## License

[MIT](LICENSE)
