# Building and developing

This page covers building Opit Speech to Text from source on Windows and Linux. Project conventions
are in [AGENTS.md](../../AGENTS.md); contribution rules are in [CONTRIBUTING.md](../../CONTRIBUTING.md).

## Requirements

- Rust 1.90 or newer (`rust-version` in the root `Cargo.toml`), Node 24, and the Tauri CLI 2.12.0:
  `cargo install tauri-cli --version 2.12.0 --locked`.
- **Windows:** Windows 10 or 11, x64.
- **Linux:** the WebKitGTK, GTK and audio development packages. On Debian and Ubuntu, CI installs:

  ```sh
  sudo apt-get update
  sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
    libayatana-appindicator3-dev librsvg2-dev libasound2-dev libgtk-layer-shell-dev patchelf
  ```

  At runtime the app also uses desktop helpers (`wl-clipboard`, `xclip`, `secret-tool` from
  libsecret, `pactl`, `playerctl`, and optionally `ydotool` and `notify-send`). See
  [Linux packaging](../../crates/app/linux/PACKAGING.md) for the full list and the local Arch
  `PKGBUILD`.

Tauri CLI 2.12.0 panics during its build step when it runs from a non-ASCII path. Build from a
checkout whose path contains only ASCII characters.

## Build and run

```sh
cd ui && npm ci && cd ..        # once, and after ui/package-lock.json changes
cargo tauri dev                 # run the app with the Vite dev server
cargo tauri build --no-bundle   # release executable in target/release/
```

`cargo tauri build` (with bundling) builds the Windows installer, or the Linux AppImage and deb, and
signs the updater artifacts, so it needs `TAURI_SIGNING_PRIVATE_KEY` (the private key, or the path to
its file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Without the key, use `cargo tauri build
--no-bundle` (executable only) or `cargo tauri build --no-sign` (unsigned packages, which cannot
serve as an update). For local Linux packages, see [Linux packaging](../../crates/app/linux/PACKAGING.md).

If your own build of the app is running, it locks its executable and the single-instance lock. Build
into a separate target folder instead, for example `CARGO_TARGET_DIR=target/agent-build`.

## Checks

Run these one at a time; the build is memory-hungry.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --test scripts/release/*.test.mjs   # when scripts/release changes
cd ui && npm run check && npm test && npm run build
```

`npm run check` must report 0 errors and 0 warnings. CI runs the full workspace on Windows and on
Linux with the packages above installed.

**Linux portable gate.** If the app's Linux dependencies are missing, or the runner cannot bind
sockets, check the portable crates without the app:

```sh
cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings
cargo test --workspace --exclude opit-speech-to-text
```

Socket-binding tests need an unrestricted runner. This gate does not replace the full Windows run.

## Developer notes

- **Browser preview.** To preview the app without a microphone, credentials or a Tauri process, run
  `cd ui` then `npm run dev -- --port 5199 --strictPort` and open
  `http://127.0.0.1:5199/app-preview.html#/home` (entry: `ui/app-preview.html`). This dev-only entry
  uses synthetic history and configuration, always reports the Windows platform, and is excluded from
  the application build. Its switches and shortcut editor affect only in-memory demo state, reset on
  reload, and never access real devices or credentials. Query parameters (before the hash) include
  `lang=en`, `mode=ptt`, `paused=true`, `recording=true` and `empty=true`. Other pages: `#/history`,
  `#/files`, `#/rules`, `#/profiles`, `#/settings`, `#/usage`.
- **Scratch data.** Set `OPIT_DATA_DIR` to run against a scratch data folder instead of the real
  one (`%APPDATA%\opit-speech-to-text` on Windows, `~/.config/opit-speech-to-text` on Linux). An
  empty folder starts like a fresh install, wizard included. This isolates app files only: the
  credential store, autostart and the clipboard are still system resources.
- **Storing a key by hand.** The Profiles page and the wizard store API keys, but you can also store
  one yourself. Both commands prompt for the key, so it does not land in your shell history. `groq`
  is the profile's `api_key_ref`.
  - Windows (Credential Manager):
    ```
    cmdkey /generic:groq.opit-speech-to-text /user:groq /pass
    ```
  - Linux (Secret Service):
    ```sh
    secret-tool store --label='Opit Speech to Text' service opit-speech-to-text key_ref groq
    ```
- Debug builds never register autostart, so the start-at-login switch is disabled there.
- Debug builds never check for updates and cannot install one; Settings → Updates and about says so.
  On Linux, only a release AppImage updates itself.
