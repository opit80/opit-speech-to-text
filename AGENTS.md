# AGENTS.md — Opit Speech to Text

Windows voice-dictation tray app: press a hotkey, speak, the transcript is pasted at the cursor.
Transcription uses the user's own API key (Groq / OpenAI / any OpenAI-compatible server).
Rust workspace + Tauri 2 + Svelte 5. Design spec: `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`.

## Working context

- Read [documentation](docs/README.md) and [known issues / pending acceptance](docs/development/known-issues.md)
  before new work. The [publication snapshot](docs/archive/PUBLICATION-REPORT.md) is historical
  release evidence, not a current acceptance claim.
- Active layout plan: `docs/superpowers/plans/2026-10-30-layout-cleanup.md`;
  layout/Linux spec: `docs/superpowers/specs/2026-10-30-layout-and-linux-design.md`.
  Both specs and the active plan stay at these paths until Task 12. Implement only the assigned task.
- No pushes, tags or releases unless the user asks in the current conversation.
- Layout changes preserve behavior, public contracts, test discovery and installed-copy identity.
  Linux app backends, packages and desktop support require separate implementation and evidence.

## Layout

| Path | What |
|---|---|
| `crates/core` | `opit-core`: audio, providers, config, pipeline, rules, history, cleanup, file transcription and domain logic. **No Tauri or Windows crates**; builds and tests on Linux. |
| `crates/eval` | `opit-eval`: real-provider WER/term accuracy CLI; see `docs/guides/eval.md`. |
| `crates/ipc` | `opit-ipc`: protocol types/framing in `src/lib.rs`, Windows named-pipe transport in `src/windows.rs`. |
| `crates/cli` | `opit-cli`: standalone `opit` console tool; see `docs/guides/cli.md`. |
| `crates/app` | Tauri app (`opit_app`): `src/app_core/mod.rs`, `commands/`, `controller/`, `ask/`, `local/`, `platform/windows/`, `platform/factory.rs`, `platform/microphone.rs`; `src/shell/` owns startup/logging/events/i18n/tray/window/updates and the Tauri lifecycle, with root compatibility re-exports. `src/platform/windows/uninstall.rs`, `windows/hooks.nsh`, `tauri.conf.json` own installation guards. |
| `ui/` | Svelte 5 + Vite + Vitest: `src/pages/` and `pages/setup/`, `src/lib/components/`, shared `src/lib/api.ts`, `events.ts`, `types.ts`, `app.svelte.ts`, helpers and `i18n/`. Dev-only preview: `src/guide-preview.ts`, `src/BrowserPreview.svelte`, `guide-preview.html`. |
| `rules/` | Built-in rule packs: `tr-core`, `tr-tech`, `fivem`. |
| `scripts/` | CLI staging and release asset/signature/draft helpers; release tests in `scripts/release/`. |
| `.github/workflows/` | `ci.yml`: Windows full workspace, Linux portable crates and UI; `release.yml`: verified signed NSIS assets to a draft. |
| `docs/` | `README.md` index; `guides/`, `reference/`, `development/`, `release/`, dated `archive/`, `releases/`; active specs/plan remain under `superpowers/`. |

## Commands

Run from the repo root unless noted. Run builds/tests sequentially. Windows CI must pass
full-workspace clippy/tests; Linux cannot compile the app in the layout phase. Before every
commit touching the relevant files:

```sh
cargo fmt --all --check
# Windows CI / suitable Windows runner:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Linux portable gate (socket-binding tests need an unrestricted runner):
cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings
cargo test --workspace --exclude opit-speech-to-text
node --test scripts/release/*.test.mjs                # when scripts/release changes
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
  (tray, overlay) has its own strings in `crates/app/src/shell/i18n.rs`.
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
- Commits and pull requests never carry AI attribution: no `Co-Authored-By: Claude`, no
  `noreply@anthropic.com`, no "Generated with" line. The `commit-identity` CI job rejects them, and
  `.claude/settings.json` must stay in the repo because it turns the trailer off.
- Branch names and commit titles are written for a public history: name the change (`feat/…`,
  `refactor/…`), never the tool that made it. Merge titles copy the branch name.

## Do not touch / be careful

- Preserve product name, identifier `io.github.opit80.opit-speech-to-text`, executable/asset names,
  `APP_DIR_NAME`, credential service/suffix, autostart value, updater endpoint/public key and NSIS
  guards. Autostart registration requires both `ui.autostart` and `ui.setup_done`.

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
