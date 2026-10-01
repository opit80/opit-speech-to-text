# SignPath Foundation application — notes for the maintainer

SignPath Foundation signs open-source Windows software for free
(<https://signpath.org>, terms: <https://signpath.org/terms>, apply: <https://signpath.org/apply>).
Only the maintainer can apply; this file collects what the form and the terms ask for. Check the
current terms before applying: they can change.

## When to apply
After the first public release (v0.1.0) is published on GitHub. SignPath expects an actively
maintained project with releases and documented functionality.

## Project facts for the form
| Field | Value |
|---|---|
| Project name | Opit Speech to Text |
| Repository | https://github.com/opit80/opit-speech-to-text |
| Homepage / download page | https://github.com/opit80/opit-speech-to-text (README) and its Releases page |
| License | MIT (OSI-approved), no commercial dual licensing |
| Description | Windows voice dictation: press a shortcut, speak, the text is pasted at the cursor. Transcription uses the user's own API key (Groq, OpenAI or any OpenAI-compatible server). |
| Platform / language | Windows 10/11 x64; Rust (Tauri 2) + Svelte; built by GitHub Actions on `windows-latest` (`.github/workflows/release.yml`) |
| Artifacts to sign | the app exe `opit-speech-to-text.exe` and the NSIS installer `opit-speech-to-text_<version>_x64-setup.exe` |
| Third-party binaries | none shipped except what the Tauri NSIS template includes (NSIS plugins, WebView2 bootstrapper download) — list them if asked |
| Binary metadata | product name, version, publisher and copyright are set in `crates/app/tauri.conf.json` and `Cargo.toml` |

## Conditions to meet (from the terms) and where we stand
- Team roles must be named: **Authors**, **Reviewers**, **Approvers**. Today all three are
  `opit80`. List them on the code signing policy page.
- **MFA** for every team member on GitHub and on SignPath: turn it on before applying.
- Only software built from this repository's own source is signed.
- No system changes without a warning: "Start with Windows" is a visible, pre-checked wizard
  step applied only after the wizard; the uninstaller removes it (and, on request, all data).
- Uninstall instructions: README → Uninstall.
- Privacy statement (required). Proposed text for the policy page:
  "Opit Speech to Text sends your recorded audio only to the transcription provider you configure,
  and asks GitHub for the latest version (Settings → Updates; can be turned off). It sends nothing
  else and has no telemetry."

## Code signing policy page (publish only once accepted)
Add a "Code signing policy" section to the README (or `docs/CODE-SIGNING-POLICY.md` linked from
it) containing, verbatim: "Free code signing provided by SignPath.io, certificate by SignPath
Foundation", the team roles above, and the privacy statement.

## After acceptance (separate work, not in Plan 4)
Wire the SignPath GitHub Action into `.github/workflows/release.yml` so both the exe (before NSIS
packs it, via `bundle > windows > signCommand`) and the installer are signed, then drop the
SmartScreen note from the README.
