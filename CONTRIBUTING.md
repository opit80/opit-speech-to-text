# Contributing

Opit Speech to Text is a Windows and Linux tray application with a portable Rust core and a Svelte UI.
See [docs/development/building.md](docs/development/building.md) for setup and [AGENTS.md](AGENTS.md)
for project conventions.

## Changes

- Use a feature branch and explain the problem, resulting behavior and verification in the pull request.
- Keep changes focused. Add regression tests for testable behavior.
- Keep the core free of Windows/Tauri dependencies; CI checks it on Linux and Windows.
- Put UI strings in both English and Turkish dictionaries. Never log API keys or transcripts.
- Do not include recordings, personal configuration, credentials or build output.
- Security vulnerabilities belong in a private report, as described in [SECURITY.md](SECURITY.md).

## Verification

Use Rust 1.90+, Node 24 and Tauri CLI 2.12.0. Install UI dependencies with `npm ci` in `ui/`.
Run these checks one at a time to limit memory use:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --test scripts/release/*.test.mjs
cd ui
npm run check
npm test
npm run build
```

On Linux, install the app's build dependencies (see the building guide) to run the full workspace.
Without them, or on a runner that cannot bind sockets, exclude the app from clippy/tests with
`--workspace --exclude opit-speech-to-text`. UI checks must report zero errors and zero warnings.
Use `OPIT_DATA_DIR` with a scratch directory when running the app. This isolates app files; the
credential store (Windows Credential Manager or the Linux Secret Service), autostart and the
clipboard are still system resources.

## Releases

Maintainers follow [docs/release/RELEASE-CHECKLIST.md](docs/release/RELEASE-CHECKLIST.md). Release automation
creates or refreshes a draft, verifies installer signatures and never publishes automatically.
Contributors do not need the updater private key; use `cargo tauri build --no-sign` for a local
installer. Unsigned installers cannot be used for automatic updates.
