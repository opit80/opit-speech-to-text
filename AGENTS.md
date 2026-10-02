# AGENTS.md — Opit Speech to Text

Windows voice-dictation tray app: press a hotkey, speak, the transcript is pasted at the cursor.
Transcription uses the user's own API key (Groq / OpenAI / any OpenAI-compatible server).
Rust workspace + Tauri 2 + Svelte 5. Design spec: `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`.

## Status (2026-10-03)

- Plans 1–4 are all implemented and merged to `main` (core, app shell, UI, release). Plan index,
  outcomes, open manual passes and **deferred findings** (candidates for the next work):
  `docs/superpowers/plans/README.md` — read it before starting new work.
- The first public release **v0.1.3** is published as stable **Latest** at
  `https://github.com/opit80/opit-speech-to-text/releases/tag/v0.1.3`; `origin` is configured.
  Windows/Linux/UI CI, release packaging and production updater downloads passed. See
  `docs/PUBLICATION-REPORT.md` for evidence and the explicitly untested installation/hardware boundary.
- Do **not** push additional changes, tag or create releases unless the user asks in that conversation.

## Layout

| Path | What |
|---|---|
| `crates/core` | `opit-core`: audio prep, provider client, rule engine, prompt builder, history, config. **No Tauri or Windows crates**; must build and test on Linux. |
| `crates/eval` | `opit-eval` CLI: WER / term accuracy against a real provider (`docs/eval.md`). |
| `crates/app` | `opit-speech-to-text` (lib `opit_app`): Tauri 2 tray app — `commands.rs` (invoke API), `app_core.rs`, `controller/` (dictation state machine), `platform/` (cpal mic, WH_KEYBOARD_LL hook, SendInput paste, Win32 overlay, keyring, autostart), `updates.rs`, `uninstall.rs`, `windows/hooks.nsh` (NSIS hooks), `tauri.conf.json`. |
| `ui/` | Svelte 5 + Vite + Vitest. `src/lib/` = typed `api.ts`/`events.ts`/`types.ts`, live state `app.svelte.ts`, `i18n/en.ts` + `tr.ts`, `components/`; `src/pages/` (+ `pages/setup/` wizard). |
| `rules/` | Built-in rule packs (`tr-core`, `tr-tech`, `fivem`). |
| `scripts/release/` | `latest-json.mjs` (+ `node:test` tests): release asset renaming and updater manifest. |
| `.github/workflows/` | `ci.yml` (fmt/clippy/test on Windows + Linux, UI check/test/build), `release.yml` (tag → signed NSIS + `latest.json` → draft release). |
| `docs/` | Spec, plans, `RELEASE-CHECKLIST.md`, `signpath-application.md`, `eval.md`. |

## Commands

Run from the repo root unless noted. All of these must pass before every commit that touches them:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --test scripts/release/latest-json.test.mjs      # when scripts/release changes
cd ui && npm run check && npm test && npm run build   # svelte-check must give 0 errors AND 0 warnings
```

- First time in `ui/`: `npm ci`.
- Run the app: `cargo tauri dev` (Tauri CLI 2.12.0: `cargo install tauri-cli --version 2.12.0 --locked`).
- Release exe without signing: `cargo tauri build --no-bundle`. A bundled build needs
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (or use `--no-sign`).
- Use `OPIT_DATA_DIR=<scratch folder>` to run against a throwaway data folder instead of the
  user's real `%APPDATA%\opit-speech-to-text\`.
- The machine is often low on memory: run builds/tests one at a time, not in parallel.

## Rules

- Code, comments, docs and commit messages are in **English**. Every user-facing UI string exists
  in both `ui/src/lib/i18n/en.ts` and `tr.ts` (parity is tested) — no literal user text in
  `.svelte` files. Turkish strings use correct orthography (ç, ğ, ı, İ, ö, ş, ü). The Rust side
  (tray, overlay) has its own strings in `crates/app/src/i18n.rs`.
- **Never log or put in error messages** transcript text or API keys. API keys live only in
  Windows Credential Manager (`<api_key_ref>.opit-speech-to-text`); the UI only passes them to
  `set_api_key` and clears the input afterwards.
- Commands reject with `CommandError { code, message, kind }`; keep `ui/src/lib/types.ts` /
  `api.ts` in sync with Rust command names and argument names (JS uses camelCase args).
- Blocking work (file I/O, keyring, SQLite, rule compilation) never runs on the Tauri main thread:
  such commands are `async` + `spawn_blocking`.
- UI config writes go through the serialized `saveConfig(mutate)` chain in
  `ui/src/lib/app.svelte.ts`; components never keep their own copy of `config` except a form draft.
- UI: only components from `ui/src/lib/components/` and tokens from `ui/src/app.css` (no raw
  colours), native `<dialog>` + `showModal()`, every input labelled, keyboard reachable, WCAG AA in
  light and dark. No new UI dependencies without a reason. CSP stays `default-src 'self'`
  (+ `media-src 'self' blob:`); no network requests from the UI.
- Pages must load their data on mount (closing the window destroys the WebView) and tolerate a
  dictation already running.
- TDD where there is testable logic: Rust unit tests, Vitest for pure UI modules.
- Commit style: Conventional-ish prefixes as in `git log` (`feat(ui): …`, `fix(updater): …`,
  `docs: …`, `ci: …`). Work on a feature branch; merge to `main` locally with `--no-ff` only when
  the user agrees.

## Do not touch / be careful

- **Updater signing key** `%USERPROFILE%\.tauri\opit-speech-to-text.key` (+ `.pub`, `.password`):
  never print, copy into the repo, commit or regenerate it — a new key strands every installed
  copy. Only the public key belongs in `crates/app/tauri.conf.json`.
- The user's own build may be running and lock `target\release\opit-speech-to-text.exe` (and the
  app's single-instance lock). Don't kill it without asking; build into a separate
  `CARGO_TARGET_DIR` (e.g. `target\agent-build`) instead.
- `hooks.nsh` / `uninstall.rs` "delete my data" must never run during an update or a silent
  reinstall — keep the `$UpdateMode` guard and its tests.
- `.superpowers/` (git-ignored) holds past agent workspaces and ledgers; `release-assets/`,
  `target/`, `ui/dist/`, `crates/app/gen/` are build output.
