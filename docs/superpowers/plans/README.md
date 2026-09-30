# Implementation plans — Opit Speech to Text v1

The v1 brief (`docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`) spans four
independent subsystems. Each gets its own plan, and each plan ends in working, testable software.
Later plans are written after the previous one lands, so they are grounded in real APIs rather
than guessed ones.

| # | Plan | Delivers | Status |
|---|---|---|---|
| 1 | [Core library + eval CLI](2026-09-30-plan-1-core-and-eval.md) | `opit-core` (audio prep, provider client, rule engine, prompt builder, history, config) and `opit-eval`, which measures WER/term accuracy against a real provider | written |
| 2 | Tauri app shell | `crates/app`: platform layer (cpal mic, WH_KEYBOARD_LL hotkey, SendInput paste, Win32 overlay, keyring, sounds, autostart), dictation controller state machine, tray, window lifecycle, `commands.rs` invoke API. Dictation works end to end with a hand-edited `config.json` | after plan 1 |
| 3 | Svelte UI | First-run wizard, Home/History/Rules/Profiles/Settings pages, en + tr i18n, "add correction rule" flow, rules preview | after plan 2 |
| 4 | Release | NSIS currentUser installer, `tauri-plugin-updater` + GitHub Releases (minisign), release workflow, README, `docs/RELEASE-CHECKLIST.md`, SignPath application | after plan 3 |
