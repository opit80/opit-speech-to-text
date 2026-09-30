# Plan 2 — Tauri App Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/app` (package `opit-speech-to-text`), the Windows tray app that turns a hotkey press into pasted text: microphone capture, global hotkey, clipboard paste, overlay, credential store, sounds, autostart, the dictation state machine, tray, window lifecycle, the invoke API for the future UI, and file logs. At the end, dictation works end to end with a hand-edited `config.json`.

**Architecture:** One Tauri 2 process. All logic that can be tested without Windows lives behind small traits in `platform/` (microphone, hotkey, paster, overlay, secrets, sounds, autostart) with in-memory fakes; `platform/windows/` implements them with cpal, windows-rs, keyring-core and windows-registry. `controller` is a single tokio actor that owns every piece of dictation state and receives messages from the hotkey hook, tray, overlay and UI, so two dictations can never overlap. `AppCore` holds everything the invoke commands and the tray need, without Tauri types, so it is tested with the fakes; `commands.rs`, `tray.rs`, `window.rs` and `events.rs` are thin Tauri wrappers. The Svelte side is only a skeleton that proves the invoke/event wiring.

**Tech Stack:** Rust 2024 (rust-version **1.90**), Tauri 2.12 (`tray-icon`), tauri-plugin-single-instance 2.5, tokio 1.53, cpal 0.18, windows 0.62, keyring-core 1.0 + windows-native-keyring-store 1.1, windows-registry 0.6, tracing 0.1 + tracing-subscriber 0.3 + tracing-appender 0.2, dirs 7, sys-locale 0.3; UI: Svelte 5.57, Vite 8.3, TypeScript 6.0, svelte-check 4.7, @tauri-apps/api 2.12.

**Spec:** `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md` (§3, §4, §7 tray/overlay, §8, §10). Read it and `docs/superpowers/plans/README.md` (Plan 1 outcome and the decisions this plan must respect) first. The spec's decisions are locked.

## Global Constraints

- Names: product **Opit Speech to Text**; package/binary `opit-speech-to-text`; lib `opit_app`; identifier `io.github.opit80.opit-speech-to-text`; data dir `%APPDATA%\opit-speech-to-text\` (`config.json`, `rules\user.yaml`, `history.db`, `audio\`, `logs\`).
- Code, comments, README: **English**. User-facing Rust strings (overlay, tray) exist in **en + tr** and live only in `i18n.rs`.
- `opit-core` stays free of Tauri and Windows crates; `cargo test -p opit-core` must still pass on Linux. The app crate is Windows-only; Linux CI excludes it.
- Workspace `rust-version = "1.90"` (tauri 2.12, tauri-build 2.7 and the single-instance plugin require 1.90; with 1.88 the resolver silently downgrades them).
- **Transcript text and API keys are never logged** and never appear in error messages. `controller::Msg` is deliberately not `Debug`. Keyring errors are mapped with `Display` only (their `Debug` can contain secret bytes). The panic hook logs the location, never the payload.
- Hotkey default **Right Ctrl + Right Shift**, mode **toggle**; alternative **push-to-talk**. In toggle mode the same combo or a **lone Ctrl** (pressed and released with nothing else, after the combo was released) stops; **Esc** cancels while recording or transcribing. Injected key events (our own Ctrl+V) never count.
- Paste: back up the clipboard → put the text → `SendInput` Ctrl+V → restore the old clipboard **400 ms** later, only if the clipboard still holds our text. Elevated (higher-integrity) target → no Ctrl+V, text stays on the clipboard, overlay "Panoda — Ctrl+V ile yapıştır". Trailing space setting appends one space.
- Recording limit default **180 s**, max **600 s** (clamped by `AppConfig::normalize`); at the limit the recording is sent automatically.
- One dictation at a time. `Cancelled` and `Error` are transient states, each followed by `Idle`. The prepared audio of a failed request is kept in memory for **Try again**; starting a new recording drops it.
- Logs: `tracing` → `%APPDATA%\opit-speech-to-text\logs\opit.YYYY-MM-DD.log` (UTC date), daily rotation, **7 files** kept.
- Closing the main window destroys it (and its WebView); the app lives on in the tray. Only tray **Quit** exits.
- `OPIT_DATA_DIR` overrides the data folder (development, manual tests). Debug builds never write the autostart Run key.
- CI must pass on Windows: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; on Linux the same with `--exclude opit-speech-to-text`; UI: `npm ci`, `npm run check`, `npm run build` in `ui/`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A hand-edited `config.json` that is broken (bad JSON, a malformed profile, a newer `schema_version`) must never stop the app: it is moved to `config.json.bad-<ms>`, defaults are written, and the overlay says so. A file that could not be moved aside is never overwritten (tests in Task 5, overlay notice in Task 8).
2. A second trigger (tray click, hotkey, UI button, Try again) while a dictation is transcribing or pasting must not start a second dictation or paste twice (test in Task 6).
3. Our own `SendInput` Ctrl+V must never be read as a hotkey press, and a key released on the secure desktop (UAC, lock screen) must not jam the combo forever (tests in Tasks 3 and 11).
4. If the user copies something else within the 400 ms restore window, the restore must not overwrite it; an empty original clipboard stays empty; Win+V history must not collect dictations (smoke test in Task 12, run in Task 15).
5. Transcript text must not reach the log files on any path, including provider errors and the fallback profile (test in Task 6).

## Design decisions made in this plan

These fill gaps the spec leaves open; reviewers should check them, not re-derive them.

- **Mono in memory.** The capture callback downmixes to mono, so a 600 s 48 kHz recording costs 115 MB of RAM instead of 230 MB. The device's native rate is kept (the core resamples).
- **Connection warm-up.** When recording starts, `GET {base_url}/models` opens and pools the TLS connection, and one HTTP client per profile is cached across dictations, so the upload after the user stops does not pay DNS + TLS. This needs two small core additions (Task 1).
- **No API key → no recording.** If the active profile needs a key and none is stored, the overlay says so with a Settings button instead of recording into the void.
- **Overlay clicks.** The overlay is click-through except while it shows a button (Try again / Settings); then a click anywhere on it fires the action.
- **Esc is swallowed** (not passed to the focused app) only while recording or transcribing.
- **Clipboard privacy.** Dictated text is marked `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0`, `CanUploadToCloudClipboard = 0`.
- **`--autostart` starts in the tray** (no window), like `ui.start_in_tray`.
- **History `latency_ms`** is the time from stopping (or pressing Try again) to the paste, i.e. what the user feels.
- **API keys in Plan 2** are stored with `cmdkey` (the UI arrives in Plan 3): `cmdkey /generic:<key_ref>.opit-speech-to-text /user:<key_ref> /pass` — verified to be readable by the keyring store.
- **Events:** `dictation-status` (`DictationStatus`), `history-added` (row id), `config-changed` (`AppConfig`), `navigate` (route string).

---

## File Map

```
Cargo.toml                                  workspace: + crates/app, new shared deps, rust-version 1.90
.gitignore                                  + /crates/app/gen/
.github/workflows/ci.yml                    Windows: whole workspace; Linux: without the app; UI job
README.md                                   development + running the app
crates/core/src/provider/mod.rs             + impl Transcriber for Arc<T>
crates/core/src/provider/openai.rs          + OpenAiCompatible::warm_up
crates/core/tests/pipeline_http.rs          + shared client in a spawned task
crates/app/Cargo.toml
crates/app/build.rs                         tauri-build
crates/app/tauri.conf.json                  no windows from config; ui/dist; NSIS bundle config
crates/app/capabilities/default.json        core:default for the main window
crates/app/icons/                           icon.svg (source) + generated icon.ico and PNGs
crates/app/src/main.rs                      windows_subsystem + opit_app::run()
crates/app/src/lib.rs                       run(): logging, Tauri builder, setup wiring, exit handling
crates/app/src/platform/mod.rs              platform traits + stand-ins (NoOverlay, UnavailableSecrets, NoAutostart)
crates/app/src/platform/keys.rs             key names → VK codes, KeyTracker (pure)
crates/app/src/platform/fake.rs             in-memory doubles (tests only)
crates/app/src/platform/windows/mod.rs      Win32 module list + call_guarded
crates/app/src/platform/windows/secrets.rs  KeyringStore (Credential Manager)
crates/app/src/platform/windows/autostart.rs RegistryAutostart (HKCU Run)
crates/app/src/platform/windows/sounds.rs   WinSounds (synthesized WAV + PlaySoundW)
crates/app/src/platform/windows/microphone.rs CpalMicrophone
crates/app/src/platform/windows/hotkey.rs   WinHotkey (WH_KEYBOARD_LL thread)
crates/app/src/platform/windows/clipboard.rs raw clipboard snapshot/restore
crates/app/src/platform/windows/paster.rs   WinPaster (backup, SendInput, UIPI check, delayed restore)
crates/app/src/platform/windows/overlay.rs  WinOverlay (layered, click-through popup on its own thread)
crates/app/src/controller/mod.rs            dictation actor
crates/app/src/controller/status.rs         DictationState / DictationStatus / ErrorKind
crates/app/src/controller/tests.rs          controller tests with fakes
crates/app/src/i18n.rs                      en/tr strings for overlay + tray
crates/app/src/settings.rs                  Settings snapshot + SettingsHandle
crates/app/src/startup.rs                   Paths, config recovery, user.yaml loading
crates/app/src/logging.rs                   tracing init, 7-day rotation, panic hook
crates/app/src/providers.rs                 HttpProviders (keys + cached clients + warm-up)
crates/app/src/history_service.rs           HistoryService (SQLite + WAV files) + DisabledHistory
crates/app/src/app_core.rs                  AppCore: logic behind commands and tray
crates/app/src/tray_menu.rs                 tray menu as data (pure)
crates/app/src/tray.rs                      Tauri tray icon + menu
crates/app/src/window.rs                    main window create/focus
crates/app/src/events.rs                    event names + TauriEvents (UiEvents impl)
crates/app/src/commands.rs                  invoke API
crates/app/tests/logging_init.rs            global logger test (own process)
ui/package.json, ui/package-lock.json, ui/vite.config.ts, ui/svelte.config.js, ui/tsconfig.json,
ui/index.html, ui/src/main.ts, ui/src/App.svelte   Svelte 5 skeleton
docs/superpowers/plans/README.md            roadmap status + Plan 2 outcome
```

---

### Task 1: Core — shared provider clients and connection warm-up

**Files:**
- Modify: `crates/core/src/provider/mod.rs` (append)
- Modify: `crates/core/src/provider/openai.rs` (new `models_url` + `warm_up`, one test)
- Test: `crates/core/tests/pipeline_http.rs` (one test)

**Interfaces:**
- Consumes: `opit_core::provider::{Transcriber, OpenAiCompatible}` from Plan 1.
- Produces:
  - `impl<T: Transcriber + Sync> Transcriber for std::sync::Arc<T>` — the app keeps one `Arc<OpenAiCompatible>` per profile and passes `&Arc<…>` to `PipelineContext`.
  - `pub async fn OpenAiCompatible::warm_up(&self)` — best-effort `GET {base_url}/models` with the bearer key; every error ignored; reads the body so the connection returns to the pool.

- [ ] **Step 1: Write the failing tests**

Append inside the `tests` module at the end of `crates/core/src/provider/openai.rs`:
```rust
    #[tokio::test]
    async fn warm_up_calls_models_with_the_key_and_ignores_errors() {
        let server = connection_server(500, 500).await;
        let client = OpenAiCompatible::new(profile_for(&server), Some("sk-warm".into())).unwrap();
        client.warm_up().await;
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "no transcription is attempted");
        assert_eq!(requests[0].url.path(), "/v1/models");
        assert_eq!(requests[0].headers.get("authorization").unwrap(), "Bearer sk-warm");

        let closed = OpenAiCompatible::new(presets::custom("x", "x", "http://127.0.0.1:9/v1", "m"), None).unwrap();
        closed.warm_up().await;
    }
```

In `crates/core/tests/pipeline_http.rs`, change the first import line `use std::time::Duration;` to:
```rust
use std::sync::Arc;
use std::time::Duration;
```
and append:
```rust
#[tokio::test]
async fn a_shared_client_works_inside_a_spawned_task() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "paylaşılan istemci"})))
        .mount(&server)
        .await;
    let profile = presets::custom("local", "Local", &format!("{}/v1", server.uri()), "m");
    let client = Arc::new(OpenAiCompatible::new(profile, None).unwrap());
    let rules = Arc::new(RuleSet::empty());
    let audio = Arc::new(pipeline::prepare(&speech()).unwrap());

    let task = tokio::spawn(async move {
        let ctx = PipelineContext {
            primary: &client,
            fallback: None,
            rules: &rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
        };
        pipeline::transcribe(&audio, &ctx).await
    });
    assert_eq!(task.await.unwrap().unwrap().text, "paylaşılan istemci");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p opit-core -- warm_up shared_client`
Expected: FAIL to compile — `no method named warm_up found for struct OpenAiCompatible` and `the trait Transcriber is not implemented for Arc<OpenAiCompatible>`.

- [ ] **Step 3: Implement**

In `crates/core/src/provider/openai.rs`, replace the head of `test_connection` (its doc comment, signature and the first two lines of its body):
```rust
    /// Checks the base URL and key with `GET {base_url}/models`. Servers without
    /// that endpoint get a 1 s silent transcription instead.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let url = format!("{}/models", self.profile.base_url.trim().trim_end_matches('/'));
        let mut request = self.client.get(url).timeout(Duration::from_secs(10));
```
with:
```rust
    fn models_url(&self) -> String {
        format!("{}/models", self.profile.base_url.trim().trim_end_matches('/'))
    }

    /// Opens (and pools) the connection while the user is still speaking, so the upload
    /// does not pay for DNS + TLS. Best effort: every error is ignored.
    pub async fn warm_up(&self) {
        let mut request = self.client.get(self.models_url()).timeout(Duration::from_secs(5));
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        if let Ok(response) = request.send().await {
            // Reading the body hands the connection back to the pool.
            let _ = response.bytes().await;
        }
    }

    /// Checks the base URL and key with `GET {base_url}/models`. Servers without
    /// that endpoint get a 1 s silent transcription instead.
    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        let mut request = self.client.get(self.models_url()).timeout(Duration::from_secs(10));
```

Append to `crates/core/src/provider/mod.rs`:
```rust

/// Lets the app keep one client (and its pooled connection) per profile and share it
/// between dictations.
impl<T: Transcriber + Sync> Transcriber for std::sync::Arc<T> {
    fn profile(&self) -> &Profile {
        (**self).profile()
    }

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        (**self).transcribe(request)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p opit-core`
Expected: 132 unit tests and 3 `pipeline_http` tests pass (was 131 + 2).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all --check && cargo clippy -p opit-core --all-targets -- -D warnings`
Expected: no output from fmt, no clippy warnings.

```bash
git add crates/core
git commit -m "feat(core): share provider clients and warm up connections

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: App crate scaffold, UI skeleton and CI

**Files:**
- Modify: `Cargo.toml` (full replacement below), `.gitignore`, `.github/workflows/ci.yml`, `README.md`
- Create: `crates/app/Cargo.toml`, `crates/app/build.rs`, `crates/app/tauri.conf.json`, `crates/app/capabilities/default.json`, `crates/app/icons/*`, `crates/app/src/main.rs`, `crates/app/src/lib.rs`
- Create: `ui/package.json`, `ui/package-lock.json` (generated), `ui/vite.config.ts`, `ui/svelte.config.js`, `ui/tsconfig.json`, `ui/index.html`, `ui/src/main.ts`, `ui/src/App.svelte`

**Interfaces:**
- Consumes: nothing new.
- Produces: `opit_app::run()` (a window that shows the UI skeleton; replaced in Task 14); the crate's full dependency set, so later tasks only add modules.

Notes for the implementer:
- Tauri needs the **tauri-cli 2.12+** for `cargo tauri icon/dev/build`: `cargo install tauri-cli --version "^2.12" --locked` (2.10 works but warns about `STATIC_VCRUNTIME`). Run `cargo tauri …` from the repo root.
- `cargo build/clippy/test` never read `ui/dist`; only `cargo tauri build` (feature `custom-protocol`) embeds it and fails if it is missing. CI therefore needs no npm step for the Rust job.
- `tauri.conf.json` creates **no** windows: `run()` builds the main window in code, which lets `--autostart` start hidden (Task 14).

- [ ] **Step 1: Workspace manifest**

Replace `Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/core", "crates/eval", "crates/app"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.90"
license = "MIT"
repository = "https://github.com/opit80/opit-speech-to-text"

[workspace.dependencies]
anyhow = "1"
claxon = "0.4"
clap = { version = "4.6.7", features = ["derive"] }
cpal = "0.18.2"
dirs = "7.0.0"
flacenc = "0.5.1"
hound = "3.5.1"
indexmap = { version = "2.14.2", features = ["serde"] }
keyring-core = "1.0.0"
regex = "1.13.1"
reqwest = { version = "0.13.5", features = ["json", "multipart"] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1.0.229", features = ["derive"] }
serde-saphyr = "1.3.0"
serde_json = "1.0.151"
sys-locale = "0.3.2"
tauri = "2.12.0"
tauri-build = "2.7.0"
tauri-plugin-single-instance = "2.5.1"
tempfile = "3.27.0"
thiserror = "2.0.21"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "time"] }
tracing = "0.1.44"
tracing-appender = "0.2.5"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
windows = "0.62.2"
windows-native-keyring-store = { version = "1.1.0", default-features = false }
windows-registry = "0.6.1"
wiremock = "0.6.5"
```

Append to `.gitignore`:
```
/crates/app/gen/
```

- [ ] **Step 2: The app crate**

`crates/app/Cargo.toml`:
```toml
[package]
name = "opit-speech-to-text"
description = "Opit Speech to Text: Windows dictation app (Tauri shell, platform layer, dictation controller)"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[lib]
name = "opit_app"

[build-dependencies]
tauri-build.workspace = true

[dependencies]
dirs.workspace = true
opit-core = { path = "../core" }
serde.workspace = true
serde_json.workspace = true
sys-locale.workspace = true
tauri = { workspace = true, features = ["tray-icon"] }
tauri-plugin-single-instance.workspace = true
thiserror.workspace = true
tokio = { workspace = true, features = ["sync"] }
tracing.workspace = true
tracing-appender.workspace = true
tracing-subscriber.workspace = true

[target.'cfg(windows)'.dependencies]
cpal.workspace = true
keyring-core.workspace = true
windows = { workspace = true, features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_Media_Audio",
    "Win32_Security",
    "Win32_System_DataExchange",
    "Win32_System_LibraryLoader",
    "Win32_System_Memory",
    "Win32_System_Threading",
    "Win32_UI_HiDpi",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
] }
windows-native-keyring-store.workspace = true
windows-registry.workspace = true

[dev-dependencies]
hound.workspace = true
tempfile.workspace = true
tokio = { workspace = true, features = ["test-util"] }
wiremock.workspace = true
```

`crates/app/build.rs`:
```rust
fn main() {
    tauri_build::build();
}
```

`crates/app/tauri.conf.json`:
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Opit Speech to Text",
  "identifier": "io.github.opit80.opit-speech-to-text",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../../ui/dist",
    "beforeDevCommand": { "script": "npm run dev", "cwd": "../../ui", "wait": false },
    "beforeBuildCommand": { "script": "npm run build", "cwd": "../../ui" }
  },
  "app": {
    "windows": [],
    "withGlobalTauri": false,
    "security": {
      "csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.ico"]
  }
}
```

`crates/app/capabilities/default.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Main window: core defaults (includes event listen/unlisten). App commands need no entry without an app ACL manifest.",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

`crates/app/src/main.rs`:
```rust
// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    opit_app::run();
}
```

`crates/app/src/lib.rs` (temporary; Task 14 replaces it):
```rust
//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

use tauri::{WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Opit Speech to Text")
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the Tauri application could not be built");
}
```

- [ ] **Step 3: Icons**

`crates/app/icons/icon.svg`:
```xml
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <rect x="32" y="32" width="960" height="960" rx="208" fill="#2563eb"/>
  <rect x="392" y="176" width="240" height="420" rx="120" fill="#ffffff"/>
  <path d="M288 480v24c0 124 100 224 224 224s224-100 224-224v-24" fill="none" stroke="#ffffff" stroke-width="56" stroke-linecap="round"/>
  <path d="M512 728v104M392 840h240" fill="none" stroke="#ffffff" stroke-width="56" stroke-linecap="round"/>
</svg>
```

Generate the raster icons, then keep only what Windows and the bundle config need (`icon.ico` becomes the exe resource and `default_window_icon()`, which the tray also uses; `bundle.icon` needs the PNGs):
```powershell
cargo tauri icon crates/app/icons/icon.svg -o crates/app/icons
Get-ChildItem crates/app/icons -Exclude icon.svg,icon.ico,32x32.png,128x128.png,128x128@2x.png | Remove-Item -Recurse -Force
```
Expected: `crates/app/icons` holds exactly `icon.svg`, `icon.ico`, `32x32.png`, `128x128.png`, `128x128@2x.png`.

- [ ] **Step 4: UI skeleton**

`ui/package.json`:
```json
{
  "name": "opit-speech-to-text-ui",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "check": "svelte-check --tsconfig ./tsconfig.json"
  },
  "dependencies": {
    "@tauri-apps/api": "2.12.0"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "7.3.1",
    "@tsconfig/svelte": "5.0.8",
    "svelte": "5.57.1",
    "svelte-check": "4.7.6",
    "typescript": "6.0.3",
    "vite": "8.3.1"
  }
}
```

`ui/vite.config.ts`:
```ts
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte()],
  // Keep Rust/Tauri CLI output visible.
  clearScreen: false,
  server: {
    // Must match build.devUrl in crates/app/tauri.conf.json.
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // WebView2 (Chromium) on Windows.
    target: "chrome105",
  },
});
```

`ui/svelte.config.js`:
```js
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import("@sveltejs/vite-plugin-svelte").SvelteConfig} */
export default {
  preprocess: vitePreprocess(),
};
```

`ui/tsconfig.json` (TypeScript 6 defaults `types` to empty, so `svelte` and `vite/client` are listed; svelte-check 4.7 does not accept TypeScript 7 yet, hence the 6.0.3 pin):
```json
{
  "extends": "@tsconfig/svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "types": ["svelte", "vite/client"],

    "resolveJsonModule": true,
    "allowJs": true,
    "checkJs": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true
  },
  "include": ["src/**/*.ts", "src/**/*.js", "src/**/*.svelte"]
}
```

`ui/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Opit Speech to Text</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`ui/src/main.ts`:
```ts
import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) {
  throw new Error("#app element not found");
}

const app = mount(App, { target });

export default app;
```

`ui/src/App.svelte` (temporary; Task 14 replaces it):
```svelte
<main>
  <h1>Opit Speech to Text</h1>
  <p>Starting…</p>
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    padding: 1.5rem;
  }
</style>
```

Run:
```powershell
cd ui; npm install; npm run check; npm run build; cd ..
```
Expected: `npm install` creates `ui/package-lock.json` without peer warnings; `svelte-check` reports `0 ERRORS 0 WARNINGS`; `vite build` writes `ui/dist/`.

- [ ] **Step 5: CI**

Replace `.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, ubuntu-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      # The app crate is Windows-only (Tauri + Win32); Linux checks the portable crates.
      - name: Clippy (Windows)
        if: runner.os == 'Windows'
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Clippy (Linux)
        if: runner.os != 'Windows'
        run: cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings
      - name: Test (Windows)
        if: runner.os == 'Windows'
        run: cargo test --workspace
      - name: Test (Linux)
        if: runner.os != 'Windows'
        run: cargo test --workspace --exclude opit-speech-to-text

  ui:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: ui
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: ui/package-lock.json
      - run: npm ci
      - run: npm run check
      - run: npm run build
```

- [ ] **Step 6: README**

In `README.md`, add this row to the layout table after the `crates/eval` row:
```markdown
| `crates/app` | `opit-speech-to-text`: the Windows tray app (Tauri 2) — hotkey, microphone, paste, overlay, tray |
| `ui/` | Svelte 5 front end of the app window |
```
and replace the `## Development` section with:
````markdown
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
````

- [ ] **Step 7: Build and run**

Run: `cargo build -p opit-speech-to-text; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace`
Expected: builds (first build downloads Tauri, several minutes); no clippy warnings; all Plan 1 tests still pass.

Run: `cargo tauri dev` (from the repo root)
Expected: a window titled "Opit Speech to Text" shows "Starting…". Close it; the process exits (tray behaviour comes in Task 14).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore .github/workflows/ci.yml README.md crates/app ui/package.json ui/package-lock.json ui/vite.config.ts ui/svelte.config.js ui/tsconfig.json ui/index.html ui/src
git commit -m "feat(app): scaffold the Tauri app crate and Svelte skeleton

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Platform seams — traits, key tracker, fakes

**Files:**
- Create: `crates/app/src/platform/mod.rs`, `crates/app/src/platform/keys.rs`, `crates/app/src/platform/fake.rs`
- Modify: `crates/app/src/lib.rs` (module list)

**Interfaces:**
- Consumes: `opit_core::audio::Recording`, `opit_core::config::OverlayPosition`.
- Produces (`opit_app::platform`), used by every later task:
  - Traits `Microphone { devices(); start(device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError> }`, `Capture { finish(self: Box<Self>) -> Recording }` (drop = cancel), `Hotkey { register(keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError>; set_paused(bool); set_capture_escape(bool) }`, `Paster { paste(text, restore) -> PasteOutcome }`, `Overlay { show(OverlayView); hide(); set_position(OverlayPosition) }`, `SecretStore { get/set/delete }`, `Sounds { play(Sound) }`, `Autostart { is_enabled(); set_enabled(bool) }`.
  - Data: `CaptureEvent::{Level(f32), Failed(String)}`, `Started { capture, device, fell_back }`, `HotkeyEvent::{ComboDown, ComboUp, LoneCtrl, Escape}`, `PasteOutcome::{Pasted, ClipboardOnly, Failed(String)}`, `CLIPBOARD_RESTORE_DELAY` (400 ms), `OverlayView { tone, text, level, button, hide_after }`, `Tone`, `OverlayAction::{Retry, OpenSettings}`, `OverlayButton`, `Sound::{Start, Done, Cancel, Error}`, errors `MicError`, `HotkeyError`, `SecretError`.
  - Sink types: `CaptureSink`, `HotkeySink`, `OverlaySink` = `Arc<dyn Fn(…) + Send + Sync>`; they must never block (they run on the audio thread, inside the keyboard hook, and on the overlay window thread).
  - Stand-ins: `NoOverlay`, `UnavailableSecrets(String)`, `NoAutostart(String)`.
  - `platform::keys::{vk_for(name) -> Option<u32>, parse_combo(&[String]) -> Result<Vec<u32>, HotkeyError>, KeyTracker::{new, retarget, on_key(vk, down) -> Option<HotkeyEvent>, prune(still_down) -> Option<HotkeyEvent>, held, combo}, VK_ESCAPE, VK_CONTROL, VK_LCONTROL, VK_RCONTROL}`.
  - `platform::fake` (tests only): `FakeMic` (+ `speech_recording()`, `silent_recording()`), `FakeHotkey`, `FakePaster`, `FakeOverlay`, `FakeSounds`, `FakeSecrets`, `FakeAutostart`.

`KeyTracker` rules (the hook in Task 11 only feeds it `(vk, down)`): auto-repeat downs are ignored; `ComboDown` fires once when exactly the combo keys are held; `ComboUp` fires once when any combo key is released after that; `LoneCtrl` fires on the release of a Ctrl that went down alone with nothing else pressed before its release and never when the combo is exactly that Ctrl; `Escape` fires on Esc down; `prune` drops keys whose key-up was missed.

- [ ] **Step 1: Traits, stand-ins and module wiring**

`crates/app/src/platform/mod.rs` (the `windows` submodule is added in Task 9):
```rust
//! Platform seams. Windows implementations live in `platform::windows`; tests use `platform::fake`.

#[cfg(test)]
pub mod fake;
pub mod keys;

use std::sync::Arc;
use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::config::OverlayPosition;

// ---------- Microphone ----------
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MicError {
    #[error("no microphone was found")]
    NoDevice,
    #[error("the microphone could not be opened: {0}")]
    Open(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    /// RMS of the last ~50 ms, 0..1. Sent at most every 50 ms.
    Level(f32),
    /// The stream died (device unplugged, driver error). Sent once.
    Failed(String),
}

pub type CaptureSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

pub struct Started {
    pub capture: Box<dyn Capture>,
    /// Name of the device actually opened.
    pub device: String,
    /// The requested device was missing or failed, so the system default was opened instead.
    pub fell_back: bool,
}

pub trait Microphone: Send + Sync {
    /// Input device names, default device first.
    fn devices(&self) -> Vec<String>;
    /// Opens `device` (None = system default) and starts capturing into memory at the
    /// device's native rate/channels. Falls back to the default device when the named one
    /// is missing or fails to open.
    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError>;
}

pub trait Capture: Send {
    /// Stops the stream and returns everything captured. Dropping without calling this cancels.
    fn finish(self: Box<Self>) -> Recording;
}

// ---------- Hotkey ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// All combo keys are now held (fires once per press, not on auto-repeat).
    ComboDown,
    /// A combo key was released after ComboDown.
    ComboUp,
    /// A Ctrl key was pressed and released with no other key pressed in between,
    /// and it was not part of forming the combo.
    LoneCtrl,
    /// Esc went down.
    Escape,
}

pub type HotkeySink = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("unknown key name: {0}")]
    UnknownKey(String),
    #[error("the shortcut has no keys")]
    Empty,
    #[error("the keyboard hook could not be installed: {0}")]
    Install(String),
}

pub trait Hotkey: Send + Sync {
    /// Installs (or re-targets) the global hook for `keys` (names like "RightCtrl").
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError>;
    /// Paused: no events are delivered.
    fn set_paused(&self, paused: bool);
    /// While on, Esc key-downs are swallowed (not passed to the focused app).
    fn set_capture_escape(&self, on: bool);
}

// ---------- Paster ----------
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteOutcome {
    /// Ctrl+V was sent.
    Pasted,
    /// The text is on the clipboard but Ctrl+V could not be sent (elevated target / UIPI).
    ClipboardOnly,
    /// The clipboard could not be written.
    Failed(String),
}

/// Old clipboard is restored this long after Ctrl+V.
pub const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(400);

pub trait Paster: Send + Sync {
    /// Backs up the clipboard, puts `text` on it, sends Ctrl+V, and (when `restore`)
    /// restores the old clipboard CLIPBOARD_RESTORE_DELAY later on a background thread.
    /// Blocking; call from a blocking thread.
    fn paste(&self, text: &str, restore: bool) -> PasteOutcome;
}

// ---------- Overlay ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Busy,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Retry,
    OpenSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayButton {
    pub action: OverlayAction,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayView {
    pub tone: Tone,
    pub text: String,
    /// Some(level 0..1) draws a level bar under the text.
    pub level: Option<f32>,
    /// Some → the overlay becomes clickable (not click-through) and clicking it fires the action.
    pub button: Option<OverlayButton>,
    /// Auto-hide after this long; None = stays until the next show/hide.
    pub hide_after: Option<Duration>,
}

pub type OverlaySink = Arc<dyn Fn(OverlayAction) + Send + Sync>;

pub trait Overlay: Send + Sync {
    fn show(&self, view: OverlayView);
    fn hide(&self);
    fn set_position(&self, position: OverlayPosition);
}

// ---------- Secrets ----------
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("credential store error: {0}")]
pub struct SecretError(pub String);

pub trait SecretStore: Send + Sync {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, key_ref: &str) -> Result<(), SecretError>;
}

// ---------- Sounds ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Start,
    Done,
    Cancel,
    Error,
}

pub trait Sounds: Send + Sync {
    /// Non-blocking.
    fn play(&self, sound: Sound);
}

// ---------- Autostart ----------
pub trait Autostart: Send + Sync {
    fn is_enabled(&self) -> Result<bool, String>;
    /// Idempotent; enabling rewrites the command so a moved exe keeps working.
    fn set_enabled(&self, enabled: bool) -> Result<(), String>;
}

// ---------- Stand-ins when a platform piece cannot start ----------

/// Used when the overlay window cannot be created; dictation keeps working with sounds and the tray.
pub struct NoOverlay;

impl Overlay for NoOverlay {
    fn show(&self, _view: OverlayView) {}
    fn hide(&self) {}
    fn set_position(&self, _position: OverlayPosition) {}
}

/// Used when the credential store cannot be opened; every call reports why.
pub struct UnavailableSecrets(pub String);

impl SecretStore for UnavailableSecrets {
    fn get(&self, _key_ref: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError(self.0.clone()))
    }

    fn set(&self, _key_ref: &str, _secret: &str) -> Result<(), SecretError> {
        Err(SecretError(self.0.clone()))
    }

    fn delete(&self, _key_ref: &str) -> Result<(), SecretError> {
        Err(SecretError(self.0.clone()))
    }
}

/// Used when the executable path is unknown, so no Run entry can be written.
pub struct NoAutostart(pub String);

impl Autostart for NoAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Err(self.0.clone())
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), String> {
        Err(self.0.clone())
    }
}
```

`crates/app/src/platform/fake.rs`:
```rust
//! In-memory platform doubles for controller and app-state tests.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use opit_core::audio::Recording;
use opit_core::config::OverlayPosition;

use super::*;

/// A 1 s 220 Hz tone at 48 kHz stereo: passes the silence gate.
pub fn speech_recording() -> Recording {
    let samples = (0..48_000)
        .flat_map(|i| {
            let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
            [s, s]
        })
        .collect();
    Recording { samples, sample_rate: 48_000, channels: 2 }
}

pub fn silent_recording() -> Recording {
    Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 }
}

#[derive(Default)]
pub struct FakeMic {
    pub recording: Mutex<Option<Recording>>,
    pub fail_start: Mutex<Option<MicError>>,
    pub fell_back: Mutex<bool>,
    pub requested: Mutex<Vec<Option<String>>>,
    pub sink: Mutex<Option<CaptureSink>>,
    pub finished: Arc<AtomicUsize>,
    pub dropped: Arc<AtomicUsize>,
}

impl FakeMic {
    pub fn starts(&self) -> usize {
        self.requested.lock().unwrap().len()
    }

    /// Sends a capture event as the running stream would.
    pub fn emit(&self, event: CaptureEvent) {
        let sink = self.sink.lock().unwrap().clone().expect("no capture running");
        sink(event);
    }
}

impl Microphone for FakeMic {
    fn devices(&self) -> Vec<String> {
        vec!["Fake Mic".into()]
    }

    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError> {
        self.requested.lock().unwrap().push(device.map(str::to_string));
        if let Some(err) = self.fail_start.lock().unwrap().take() {
            return Err(err);
        }
        *self.sink.lock().unwrap() = Some(sink);
        let recording = self.recording.lock().unwrap().clone().unwrap_or_else(speech_recording);
        Ok(Started {
            capture: Box::new(FakeCapture {
                recording: Some(recording),
                finished: self.finished.clone(),
                dropped: self.dropped.clone(),
            }),
            device: "Fake Mic".into(),
            fell_back: *self.fell_back.lock().unwrap(),
        })
    }
}

struct FakeCapture {
    recording: Option<Recording>,
    finished: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl Capture for FakeCapture {
    fn finish(mut self: Box<Self>) -> Recording {
        self.finished.fetch_add(1, Ordering::SeqCst);
        self.recording.take().unwrap()
    }
}

impl Drop for FakeCapture {
    fn drop(&mut self) {
        if self.recording.is_some() {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[derive(Default)]
pub struct FakeHotkey {
    pub registered: Mutex<Vec<Vec<String>>>,
    pub sink: Mutex<Option<HotkeySink>>,
    pub fail_register: Mutex<Option<HotkeyError>>,
    pub paused: Mutex<bool>,
    pub capture_escape: Mutex<Vec<bool>>,
}

impl FakeHotkey {
    pub fn escape_captured(&self) -> bool {
        self.capture_escape.lock().unwrap().last().copied().unwrap_or(false)
    }
}

impl Hotkey for FakeHotkey {
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError> {
        if let Some(err) = self.fail_register.lock().unwrap().take() {
            return Err(err);
        }
        self.registered.lock().unwrap().push(keys.to_vec());
        *self.sink.lock().unwrap() = Some(sink);
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
    }

    fn set_capture_escape(&self, on: bool) {
        self.capture_escape.lock().unwrap().push(on);
    }
}

pub struct FakePaster {
    pub outcome: Mutex<PasteOutcome>,
    pub pasted: Mutex<Vec<(String, bool)>>,
}

impl Default for FakePaster {
    fn default() -> Self {
        Self { outcome: Mutex::new(PasteOutcome::Pasted), pasted: Mutex::default() }
    }
}

impl FakePaster {
    pub fn texts(&self) -> Vec<String> {
        self.pasted.lock().unwrap().iter().map(|(text, _)| text.clone()).collect()
    }
}

impl Paster for FakePaster {
    fn paste(&self, text: &str, restore: bool) -> PasteOutcome {
        self.pasted.lock().unwrap().push((text.to_string(), restore));
        self.outcome.lock().unwrap().clone()
    }
}

#[derive(Default)]
pub struct FakeOverlay {
    pub views: Mutex<Vec<OverlayView>>,
    pub hides: AtomicUsize,
    pub positions: Mutex<Vec<OverlayPosition>>,
}

impl FakeOverlay {
    pub fn last(&self) -> OverlayView {
        self.views.lock().unwrap().last().cloned().expect("overlay never shown")
    }
}

impl Overlay for FakeOverlay {
    fn show(&self, view: OverlayView) {
        self.views.lock().unwrap().push(view);
    }

    fn hide(&self) {
        self.hides.fetch_add(1, Ordering::SeqCst);
    }

    fn set_position(&self, position: OverlayPosition) {
        self.positions.lock().unwrap().push(position);
    }
}

#[derive(Default)]
pub struct FakeSounds {
    pub played: Mutex<Vec<Sound>>,
}

impl Sounds for FakeSounds {
    fn play(&self, sound: Sound) {
        self.played.lock().unwrap().push(sound);
    }
}

#[derive(Default)]
pub struct FakeSecrets {
    pub map: Mutex<HashMap<String, String>>,
    pub broken: Mutex<bool>,
}

impl FakeSecrets {
    pub fn with(key_ref: &str, secret: &str) -> Self {
        let secrets = Self::default();
        secrets.map.lock().unwrap().insert(key_ref.into(), secret.into());
        secrets
    }
}

impl SecretStore for FakeSecrets {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        if *self.broken.lock().unwrap() {
            return Err(SecretError("locked".into()));
        }
        Ok(self.map.lock().unwrap().get(key_ref).cloned())
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError> {
        self.map.lock().unwrap().insert(key_ref.into(), secret.into());
        Ok(())
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        self.map.lock().unwrap().remove(key_ref);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeAutostart {
    pub enabled: Mutex<bool>,
}

impl Autostart for FakeAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(*self.enabled.lock().unwrap())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        *self.enabled.lock().unwrap() = enabled;
        Ok(())
    }
}
```

At the top of `crates/app/src/lib.rs`, below the `//!` line, add:
```rust

pub mod platform;
```

- [ ] **Step 2: Write the failing key-tracker tests**

`crates/app/src/platform/keys.rs`:
```rust
//! Key names, virtual-key codes and the hotkey state machine. Pure and cross-platform so it
//! can be unit-tested anywhere; the Windows keyboard hook only feeds it (vk, down) pairs.

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::*;

    const LCTRL: u32 = 0xA2;
    const RCTRL: u32 = 0xA3;
    const LSHIFT: u32 = 0xA0;
    const RSHIFT: u32 = 0xA1;
    const RALT: u32 = 0xA5;
    const KEY_A: u32 = 0x41;
    const KEY_C: u32 = 0x43;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    /// Feeds `(vk, down)` pairs and collects every emitted event.
    fn run(t: &mut KeyTracker, seq: &[(u32, bool)]) -> Vec<HotkeyEvent> {
        seq.iter().filter_map(|&(vk, down)| t.on_key(vk, down)).collect()
    }

    fn rctrl_rshift() -> KeyTracker {
        KeyTracker::new(vec![RCTRL, RSHIFT])
    }

    #[test]
    fn vk_for_modifiers_are_side_specific() {
        assert_eq!(vk_for("LeftCtrl"), Some(0xA2));
        assert_eq!(vk_for("RightCtrl"), Some(0xA3));
        assert_eq!(vk_for("LeftShift"), Some(0xA0));
        assert_eq!(vk_for("RightShift"), Some(0xA1));
        assert_eq!(vk_for("LeftAlt"), Some(0xA4));
        assert_eq!(vk_for("RightAlt"), Some(0xA5));
        assert_eq!(vk_for("LeftWin"), Some(0x5B));
        assert_eq!(vk_for("RightWin"), Some(0x5C));
    }

    #[test]
    fn vk_for_is_case_insensitive_and_trims() {
        assert_eq!(vk_for("rightctrl"), Some(0xA3));
        assert_eq!(vk_for("RIGHTCTRL"), Some(0xA3));
        assert_eq!(vk_for(" Space "), Some(0x20));
        assert_eq!(vk_for("f5"), Some(0x74));
        assert_eq!(vk_for("q"), Some(0x51));
    }

    #[test]
    fn vk_for_function_keys() {
        assert_eq!(vk_for("F1"), Some(0x70));
        assert_eq!(vk_for("F12"), Some(0x7B));
        assert_eq!(vk_for("F13"), Some(0x7C));
        assert_eq!(vk_for("F24"), Some(0x87));
        assert_eq!(vk_for("F"), Some(0x46), "a lone F is the letter");
        for bad in ["F0", "F25", "F01", "F+1", "F-1", "F1a", "Fx"] {
            assert_eq!(vk_for(bad), None, "{bad}");
        }
    }

    #[test]
    fn vk_for_letters_and_digits() {
        assert_eq!(vk_for("A"), Some(0x41));
        assert_eq!(vk_for("Z"), Some(0x5A));
        assert_eq!(vk_for("0"), Some(0x30));
        assert_eq!(vk_for("9"), Some(0x39));
        assert_eq!(vk_for("AB"), None);
        assert_eq!(vk_for("!"), None);
        assert_eq!(vk_for(""), None);
    }

    #[test]
    fn vk_for_navigation_and_locks() {
        assert_eq!(vk_for("Space"), Some(0x20));
        assert_eq!(vk_for("CapsLock"), Some(0x14));
        assert_eq!(vk_for("ScrollLock"), Some(0x91));
        assert_eq!(vk_for("Pause"), Some(0x13));
        assert_eq!(vk_for("Insert"), Some(0x2D));
        assert_eq!(vk_for("Home"), Some(0x24));
        assert_eq!(vk_for("End"), Some(0x23));
        assert_eq!(vk_for("PageUp"), Some(0x21));
        assert_eq!(vk_for("PageDown"), Some(0x22));
        assert_eq!(vk_for("Ctrl"), None, "generic modifiers are not accepted");
        assert_eq!(vk_for("Escape"), None);
    }

    #[test]
    fn parse_combo_errors_and_dedupe() {
        assert_eq!(parse_combo(&[]), Err(HotkeyError::Empty));
        assert_eq!(parse_combo(&names(&["RightCtrl", "Hyper"])), Err(HotkeyError::UnknownKey("Hyper".into())));
        assert_eq!(parse_combo(&names(&["RightCtrl", "rightctrl", "RightShift"])), Ok(vec![RCTRL, RSHIFT]));
        assert_eq!(parse_combo(&names(&["F13"])), Ok(vec![0x7C]));
    }

    #[test]
    fn combo_either_order_fires_once() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![ComboUp]);

        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RSHIFT, true), (RCTRL, true)]), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RCTRL, false), (RSHIFT, false)]), vec![ComboUp]);
    }

    #[test]
    fn auto_repeat_does_not_refire() {
        let mut t = rctrl_rshift();
        let seq = [(RCTRL, true), (RCTRL, true), (RSHIFT, true), (RSHIFT, true), (RCTRL, true), (RSHIFT, true)];
        assert_eq!(run(&mut t, &seq), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![ComboUp]);
    }

    #[test]
    fn combo_up_fires_once_whatever_release_order() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true)]);
        assert_eq!(t.on_key(RCTRL, false), Some(ComboUp));
        assert_eq!(t.on_key(RSHIFT, false), None);
        // Pressing again works.
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
    }

    #[test]
    fn re_pressing_released_combo_key_fires_again() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true), (RSHIFT, false)]);
        // RCtrl still held; pressing RShift again re-forms the combo.
        assert_eq!(t.on_key(RSHIFT, true), Some(ComboDown));
    }

    #[test]
    fn combo_release_does_not_produce_lone_ctrl() {
        let mut t = rctrl_rshift();
        let ev = run(&mut t, &[(RCTRL, true), (RSHIFT, true), (RSHIFT, false), (RCTRL, false)]);
        assert_eq!(ev, vec![ComboDown, ComboUp]);
        // A later lone left Ctrl tap is a LoneCtrl.
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn lone_ctrl_both_sides_and_generic() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RCTRL, false)]), vec![LoneCtrl]);
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
        assert_eq!(run(&mut t, &[(VK_CONTROL, true), (VK_CONTROL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn ctrl_shortcut_is_not_lone_ctrl() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (KEY_C, true), (KEY_C, false), (LCTRL, false)]), vec![]);
        // Other key released first does not matter either.
        assert_eq!(run(&mut t, &[(LCTRL, true), (KEY_C, true), (LCTRL, false), (KEY_C, false)]), vec![]);
    }

    #[test]
    fn ctrl_pressed_while_other_key_held_is_not_lone() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(KEY_A, true), (LCTRL, true), (KEY_A, false), (LCTRL, false)]), vec![]);
    }

    #[test]
    fn both_ctrls_is_not_lone() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (RCTRL, true), (RCTRL, false), (LCTRL, false)]), vec![]);
    }

    #[test]
    fn single_ctrl_combo_never_lone() {
        let mut t = KeyTracker::new(vec![RCTRL]);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RCTRL, true), (RCTRL, false)]), vec![ComboDown, ComboUp]);
        // The other Ctrl is still a LoneCtrl.
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn single_ctrl_combo_blocked_by_extra_key_is_still_not_lone() {
        let mut t = KeyTracker::new(vec![RCTRL]);
        assert_eq!(run(&mut t, &[(KEY_A, true), (RCTRL, true), (KEY_A, false), (RCTRL, false)]), vec![]);
    }

    #[test]
    fn extra_key_held_blocks_combo() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(KEY_A, true), (RCTRL, true), (RSHIFT, true)]), vec![]);
        // Releasing the extra key does not retro-fire; only a combo key-down can form it.
        assert_eq!(t.on_key(KEY_A, false), None);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![]);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
    }

    #[test]
    fn extra_modifier_blocks_combo() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LSHIFT, true), (RCTRL, true), (RSHIFT, true)]), vec![]);
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RALT, true), (RSHIFT, true)]), vec![]);
    }

    #[test]
    fn extra_key_during_active_combo_keeps_it_active() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true)]);
        assert_eq!(run(&mut t, &[(KEY_A, true), (KEY_A, false)]), vec![]);
        assert_eq!(t.on_key(RCTRL, false), Some(ComboUp));
    }

    #[test]
    fn escape_fires_on_down_not_repeat_or_up() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(VK_ESCAPE, true), (VK_ESCAPE, true), (VK_ESCAPE, true)]), vec![Escape]);
        assert_eq!(t.on_key(VK_ESCAPE, false), None);
        assert_eq!(t.on_key(VK_ESCAPE, true), Some(Escape));
    }

    #[test]
    fn escape_during_combo_fires_and_cancels_lone_ctrl() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true), (VK_ESCAPE, true)]), vec![ComboDown, Escape]);
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (VK_ESCAPE, true), (VK_ESCAPE, false), (LCTRL, false)]), vec![Escape]);
    }

    #[test]
    fn unmatched_key_up_is_ignored() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, false), (KEY_A, false), (LCTRL, false)]), vec![]);
        assert!(t.held().is_empty());
    }

    #[test]
    fn prune_drops_stale_keys_and_ends_combo() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(KEY_A, true)]); // A's key-up gets lost
        assert_eq!(t.prune(|vk| vk != KEY_A), None);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
        assert_eq!(t.prune(|vk| vk != RSHIFT), Some(ComboUp));
        assert_eq!(t.held(), &[RCTRL]);
        assert_eq!(t.prune(|_| true), None);
    }

    #[test]
    fn retarget_keeps_held_keys() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(KEY_A, true)]);
        t.retarget(vec![0x7C]);
        assert_eq!(t.combo(), &[0x7C]);
        assert_eq!(t.on_key(0x7C, true), None, "A is still held");
        run(&mut t, &[(KEY_A, false), (0x7C, false)]);
        assert_eq!(t.on_key(0x7C, true), Some(ComboDown));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib platform::keys`
Expected: FAIL to compile — `cannot find type KeyTracker`, `cannot find function vk_for`.

- [ ] **Step 4: Implement the key tracker**

Insert above the tests in `crates/app/src/platform/keys.rs`:
```rust
use super::{HotkeyError, HotkeyEvent};

/// VK_ESCAPE.
pub const VK_ESCAPE: u32 = 0x1B;
/// Generic VK_CONTROL. Low-level hooks normally deliver the left/right codes instead.
pub const VK_CONTROL: u32 = 0x11;
/// VK_LCONTROL.
pub const VK_LCONTROL: u32 = 0xA2;
/// VK_RCONTROL.
pub const VK_RCONTROL: u32 = 0xA3;

/// Maps a key name (case-insensitive, e.g. "RightCtrl", "f13", "a", "0") to the virtual-key
/// code a `WH_KEYBOARD_LL` hook delivers for it. Modifiers are side-specific.
pub fn vk_for(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    let named = match lower.as_str() {
        "leftctrl" => 0xA2,
        "rightctrl" => 0xA3,
        "leftshift" => 0xA0,
        "rightshift" => 0xA1,
        "leftalt" => 0xA4,
        "rightalt" => 0xA5,
        "leftwin" => 0x5B,
        "rightwin" => 0x5C,
        "space" => 0x20,
        "capslock" => 0x14,
        "scrolllock" => 0x91,
        "pause" => 0x13,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    let bytes = lower.as_bytes();
    if bytes.len() == 1 {
        return match bytes[0] {
            c @ b'a'..=b'z' => Some(u32::from(c - b'a') + 0x41),
            c @ b'0'..=b'9' => Some(u32::from(c - b'0') + 0x30),
            _ => None,
        };
    }
    // F1..F24 → 0x70..0x87. Reject leading zeros ("F01") and signs so names stay canonical.
    let digits = lower.strip_prefix('f')?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match digits.parse::<u32>() {
        Ok(n @ 1..=24) => Some(0x6F + n),
        _ => None,
    }
}

/// Resolves key names into a deduplicated list of virtual-key codes (first occurrence wins).
pub fn parse_combo(names: &[String]) -> Result<Vec<u32>, HotkeyError> {
    if names.is_empty() {
        return Err(HotkeyError::Empty);
    }
    let mut combo = Vec::with_capacity(names.len());
    for name in names {
        let vk = vk_for(name).ok_or_else(|| HotkeyError::UnknownKey(name.clone()))?;
        if !combo.contains(&vk) {
            combo.push(vk);
        }
    }
    Ok(combo)
}

fn is_ctrl(vk: u32) -> bool {
    matches!(vk, VK_CONTROL | VK_LCONTROL | VK_RCONTROL)
}

/// Turns a stream of physical key transitions into [`HotkeyEvent`]s.
#[derive(Debug, Clone)]
pub struct KeyTracker {
    combo: Vec<u32>,
    /// Every key currently held (combo or not), in press order.
    held: Vec<u32>,
    /// ComboDown was emitted and ComboUp has not been yet.
    combo_active: bool,
    /// A Ctrl key that went down alone and may still become a LoneCtrl on release.
    lone_candidate: Option<u32>,
}

impl KeyTracker {
    /// `combo` must be non-empty and deduplicated (see [`parse_combo`]).
    pub fn new(combo: Vec<u32>) -> Self {
        Self { combo, held: Vec::new(), combo_active: false, lone_candidate: None }
    }

    /// Switches to a new combo but keeps the held-key set, so keys already down while
    /// re-registering are still accounted for. Pending ComboDown/LoneCtrl state is dropped.
    pub fn retarget(&mut self, combo: Vec<u32>) {
        self.combo = combo;
        self.combo_active = false;
        self.lone_candidate = None;
    }

    /// The combo this tracker watches.
    pub fn combo(&self) -> &[u32] {
        &self.combo
    }

    /// Keys currently believed to be held.
    pub fn held(&self) -> &[u32] {
        &self.held
    }

    /// Feeds one physical transition. Auto-repeat downs of held keys are ignored.
    pub fn on_key(&mut self, vk: u32, down: bool) -> Option<HotkeyEvent> {
        if down { self.on_down(vk) } else { self.on_up(vk) }
    }

    fn on_down(&mut self, vk: u32) -> Option<HotkeyEvent> {
        if self.held.contains(&vk) {
            return None; // auto-repeat
        }
        let was_empty = self.held.is_empty();
        self.held.push(vk);

        if self.lone_candidate.is_some_and(|c| c != vk) {
            self.lone_candidate = None;
        }
        if is_ctrl(vk) && was_empty && self.combo != [vk] {
            self.lone_candidate = Some(vk);
        }

        if !self.combo_active && self.combo_is_exactly_held() {
            self.combo_active = true;
            self.lone_candidate = None;
            return Some(HotkeyEvent::ComboDown);
        }
        (vk == VK_ESCAPE).then_some(HotkeyEvent::Escape)
    }

    fn on_up(&mut self, vk: u32) -> Option<HotkeyEvent> {
        let was_held = match self.held.iter().position(|&k| k == vk) {
            Some(i) => {
                self.held.remove(i);
                true
            }
            None => false,
        };
        if self.combo_active && self.combo.contains(&vk) {
            self.combo_active = false;
            self.lone_candidate = None;
            return Some(HotkeyEvent::ComboUp);
        }
        if self.lone_candidate == Some(vk) {
            self.lone_candidate = None;
            if was_held {
                return Some(HotkeyEvent::LoneCtrl);
            }
        }
        None
    }

    /// Drops held keys for which `still_down(vk)` is false (their key-up was missed, e.g.
    /// released on the secure desktop). Returns ComboUp if that ends an active combo.
    pub fn prune(&mut self, mut still_down: impl FnMut(u32) -> bool) -> Option<HotkeyEvent> {
        let before = self.held.len();
        self.held.retain(|&vk| still_down(vk));
        if self.held.len() == before {
            return None;
        }
        if self.lone_candidate.is_some_and(|c| !self.held.contains(&c)) {
            self.lone_candidate = None;
        }
        if self.combo_active && !self.combo.iter().all(|k| self.held.contains(k)) {
            self.combo_active = false;
            return Some(HotkeyEvent::ComboUp);
        }
        None
    }

    fn combo_is_exactly_held(&self) -> bool {
        self.held.len() == self.combo.len() && self.combo.iter().all(|k| self.held.contains(k))
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib platform::keys`
Expected: 25 passed.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): add platform traits, key tracker and test fakes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Dictation status, overlay/tray strings and the settings snapshot

**Files:**
- Create: `crates/app/src/controller/mod.rs` (stub; Task 6 replaces it), `crates/app/src/controller/status.rs`, `crates/app/src/i18n.rs`, `crates/app/src/settings.rs`
- Modify: `crates/app/src/lib.rs` (module list)

**Interfaces:**
- Consumes: `opit_core::{pipeline::PipelineError, provider::ProviderError, config::AppConfig, rules::{RulePack, RuleSet, RuleWarning, builtin::assemble}}`.
- Produces:
  - `controller::ErrorKind` (`MissingKey, InvalidKey, TooLarge, RateLimited, Server, Network, Timeout, BadResponse, Rejected, Audio, Microphone, Credentials, Paste`; `ALL`; `needs_settings()`; `From<&ProviderError>`, `From<&PipelineError>`), serialized snake_case.
  - `controller::DictationState::{Idle, Recording, Transcribing, Pasting, Cancelled, Error { kind, message }}` (serde tag `state`), `controller::DictationStatus { state (flattened), can_retry }` with `Default` = idle.
  - `i18n::Lang::{En, Tr}`, `i18n::resolve(ui_language: Option<&str>, system_locale: Option<&str>) -> Lang`, `i18n::system_locale()`, and one method per string (`listening`, `listening_default_mic`, `transcribing`, `pasted(seconds)`, `clipboard_only`, `no_speech`, `too_short`, `empty`, `cancelled`, `retry`, `open_settings`, `config_reset`, `user_rules_broken`, `hotkey_failed`, `error(kind)`, `tray_open`, `tray_profile`, `tray_pause_hotkey`, `tray_quit`, `tray_tooltip(recording)`).
  - `settings::Settings { config: AppConfig, rules: Arc<RuleSet>, lang: Lang }`, `Settings::build(config, user_pack: Option<RulePack>, system_locale: Option<&str>) -> (Settings, Vec<RuleWarning>)`; `settings::SettingsHandle` (`Clone`; `new`, `current() -> Arc<Settings>`, `replace(Settings)`). A dictation holds one `Arc<Settings>` from start to end.

- [ ] **Step 1: Wire the modules**

`crates/app/src/controller/mod.rs`:
```rust
//! Dictation state machine (filled in by Task 6). For now: the status types.

mod status;

pub use status::{DictationState, DictationStatus, ErrorKind};
```

Add to the module list in `crates/app/src/lib.rs` (keep it alphabetical):
```rust
pub mod controller;
pub mod i18n;
pub mod settings;
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/controller/status.rs`:
```rust
//! What the controller reports to the tray and the UI.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_errors_map_to_kinds() {
        assert_eq!(ErrorKind::from(&ProviderError::Unauthorized(401)), ErrorKind::InvalidKey);
        assert_eq!(ErrorKind::from(&ProviderError::Http { status: 400, message: "x".into() }), ErrorKind::Rejected);
        let pipeline = PipelineError::Provider(ProviderError::Timeout);
        assert_eq!(ErrorKind::from(&pipeline), ErrorKind::Timeout);
        assert!(ErrorKind::InvalidKey.needs_settings() && !ErrorKind::Server.needs_settings());
    }

    #[test]
    fn status_serializes_flat_for_the_ui() {
        let status = DictationStatus {
            state: DictationState::Error { kind: ErrorKind::Network, message: "Ağ hatası".into() },
            can_retry: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json, serde_json::json!({"state":"error","kind":"network","message":"Ağ hatası","can_retry":true}));
        let idle = serde_json::to_value(DictationStatus::default()).unwrap();
        assert_eq!(idle, serde_json::json!({"state":"idle","can_retry":false}));
    }
}
```

`crates/app/src/i18n.rs`:
```rust
//! Strings the Rust side shows itself (overlay, tray). The Svelte UI has its own i18n.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_language_wins_over_the_system() {
        assert_eq!(resolve(Some("en"), Some("tr-TR")), Lang::En);
        assert_eq!(resolve(Some("tr"), Some("en-US")), Lang::Tr);
        assert_eq!(resolve(None, Some("tr-TR")), Lang::Tr);
        assert_eq!(resolve(None, Some("de-DE")), Lang::En);
        assert_eq!(resolve(None, None), Lang::En);
    }

    #[test]
    fn pasted_uses_the_local_decimal_separator() {
        assert_eq!(Lang::En.pasted(0.94), "Pasted (0.9 s)");
        assert_eq!(Lang::Tr.pasted(0.94), "Yapıştırıldı (0,9 sn)");
    }

    #[test]
    fn every_error_kind_has_both_languages() {
        for kind in ErrorKind::ALL {
            assert!(!Lang::En.error(kind).is_empty());
            assert_ne!(Lang::En.error(kind), Lang::Tr.error(kind), "{kind:?}");
        }
    }
}
```

`crates/app/src/settings.rs`:
```rust
//! The live settings snapshot (config + compiled rules + UI language). A dictation takes
//! one `Arc<Settings>` when it starts, so a config change mid-dictation cannot tear it.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_terms_come_first_and_language_is_resolved() {
        let user = RulePack::from_yaml("schema: 1\nid: user\nname: Me\nterms: [Opit]\n").unwrap();
        let (settings, warnings) = Settings::build(AppConfig::default(), Some(user), Some("tr-TR"));
        assert!(warnings.is_empty());
        assert_eq!(settings.rules.terms()[0], "Opit");
        assert_eq!(settings.lang, Lang::Tr);
    }

    #[test]
    fn broken_user_rules_become_warnings() {
        let user = RulePack::from_yaml(
            "schema: 1\nid: user\nname: Me\nreplacements:\n  - { from: '(', to: x, regex: true }\n",
        )
        .unwrap();
        let (_, warnings) = Settings::build(AppConfig::default(), Some(user), None);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn replace_is_seen_by_every_clone_but_not_by_held_snapshots() {
        let handle = SettingsHandle::new(Settings::build(AppConfig::default(), None, None).0);
        let clone = handle.clone();
        let held = handle.current();
        let mut config = AppConfig::default();
        config.paste.trailing_space = false;
        handle.replace(Settings::build(config, None, None).0);
        assert!(!clone.current().config.paste.trailing_space);
        assert!(held.config.paste.trailing_space);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib -- controller::status i18n settings`
Expected: FAIL to compile — `cannot find type ErrorKind`, `cannot find type Lang`, `cannot find type Settings`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/controller/status.rs`:
```rust
use opit_core::pipeline::PipelineError;
use opit_core::provider::ProviderError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    MissingKey,
    InvalidKey,
    TooLarge,
    RateLimited,
    Server,
    Network,
    Timeout,
    BadResponse,
    Rejected,
    Audio,
    Microphone,
    Credentials,
    Paste,
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 13] = [
        ErrorKind::MissingKey,
        ErrorKind::InvalidKey,
        ErrorKind::TooLarge,
        ErrorKind::RateLimited,
        ErrorKind::Server,
        ErrorKind::Network,
        ErrorKind::Timeout,
        ErrorKind::BadResponse,
        ErrorKind::Rejected,
        ErrorKind::Audio,
        ErrorKind::Microphone,
        ErrorKind::Credentials,
        ErrorKind::Paste,
    ];

    /// Errors the user fixes in Settings rather than by trying again.
    pub fn needs_settings(self) -> bool {
        matches!(self, ErrorKind::MissingKey | ErrorKind::InvalidKey | ErrorKind::Credentials)
    }
}

impl From<&ProviderError> for ErrorKind {
    fn from(err: &ProviderError) -> Self {
        match err {
            ProviderError::Unauthorized(_) => ErrorKind::InvalidKey,
            ProviderError::PayloadTooLarge => ErrorKind::TooLarge,
            ProviderError::RateLimited => ErrorKind::RateLimited,
            ProviderError::Server(_) => ErrorKind::Server,
            ProviderError::Http { .. } => ErrorKind::Rejected,
            ProviderError::Network(_) => ErrorKind::Network,
            ProviderError::Timeout => ErrorKind::Timeout,
            ProviderError::BadResponse(_) => ErrorKind::BadResponse,
        }
    }
}

impl From<&PipelineError> for ErrorKind {
    fn from(err: &PipelineError) -> Self {
        match err {
            PipelineError::Provider(e) => e.into(),
            PipelineError::Encode(_) | PipelineError::TooShort | PipelineError::NoSpeech => ErrorKind::Audio,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DictationState {
    Idle,
    Recording,
    Transcribing,
    Pasting,
    /// Transient: emitted once, followed by `Idle`.
    Cancelled,
    /// Transient: emitted once, followed by `Idle`.
    Error {
        kind: ErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationStatus {
    #[serde(flatten)]
    pub state: DictationState,
    /// Audio of the last failed dictation is kept; "Try again" resends it.
    pub can_retry: bool,
}

impl Default for DictationStatus {
    fn default() -> Self {
        Self { state: DictationState::Idle, can_retry: false }
    }
}
```

Insert above the tests in `crates/app/src/i18n.rs`:
```rust
use serde::Serialize;

use crate::controller::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Tr,
}

/// `ui_language` from the config wins; `None` follows the system locale (e.g. `tr-TR`).
pub fn resolve(ui_language: Option<&str>, system_locale: Option<&str>) -> Lang {
    let pick = ui_language.or(system_locale).unwrap_or("en");
    if pick.to_ascii_lowercase().starts_with("tr") { Lang::Tr } else { Lang::En }
}

pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

impl Lang {
    fn pick(self, en: &'static str, tr: &'static str) -> &'static str {
        match self {
            Lang::En => en,
            Lang::Tr => tr,
        }
    }

    pub fn listening(self) -> &'static str {
        self.pick("Listening", "Dinleniyor")
    }

    pub fn listening_default_mic(self) -> &'static str {
        self.pick("Listening (default microphone)", "Dinleniyor (varsayılan mikrofon)")
    }

    pub fn transcribing(self) -> &'static str {
        self.pick("Transcribing…", "Çevriliyor…")
    }

    /// `Pasted (0.9 s)` / `Yapıştırıldı (0,9 sn)`.
    pub fn pasted(self, seconds: f32) -> String {
        match self {
            Lang::En => format!("Pasted ({seconds:.1} s)"),
            Lang::Tr => format!("Yapıştırıldı ({} sn)", format!("{seconds:.1}").replace('.', ",")),
        }
    }

    pub fn clipboard_only(self) -> &'static str {
        self.pick("In clipboard — press Ctrl+V to paste", "Panoda — Ctrl+V ile yapıştır")
    }

    pub fn no_speech(self) -> &'static str {
        self.pick("No speech detected", "Konuşma algılanmadı")
    }

    pub fn too_short(self) -> &'static str {
        self.pick("Recording too short", "Kayıt çok kısa")
    }

    pub fn empty(self) -> &'static str {
        self.pick("No text came back", "Metin çıkmadı")
    }

    pub fn cancelled(self) -> &'static str {
        self.pick("Cancelled", "İptal edildi")
    }

    pub fn retry(self) -> &'static str {
        self.pick("Try again", "Tekrar dene")
    }

    pub fn open_settings(self) -> &'static str {
        self.pick("Settings", "Ayarlar")
    }

    pub fn config_reset(self) -> &'static str {
        self.pick("Settings file was damaged; defaults loaded", "Ayar dosyası bozuktu; varsayılanlar yüklendi")
    }

    pub fn user_rules_broken(self) -> &'static str {
        self.pick("Personal rules could not be read", "Kişisel kurallar okunamadı")
    }

    pub fn hotkey_failed(self) -> &'static str {
        self.pick("Shortcut unavailable — use the tray icon", "Kısayol kurulamadı — tray simgesini kullan")
    }

    pub fn error(self, kind: ErrorKind) -> &'static str {
        use ErrorKind::*;
        match kind {
            MissingKey => self.pick("No API key for this profile", "Bu profil için API anahtarı yok"),
            InvalidKey => self.pick("API key is invalid", "API anahtarı geçersiz"),
            TooLarge => self.pick("Recording exceeds the provider limit", "Kayıt sağlayıcı sınırını aşıyor"),
            RateLimited => self.pick("Provider is rate limiting", "Sağlayıcı istek sınırına takıldı"),
            Server => self.pick("Provider server error", "Sağlayıcı sunucu hatası"),
            Network => self.pick("Network error", "Ağ hatası"),
            Timeout => self.pick("Request timed out", "İstek zaman aşımına uğradı"),
            BadResponse => self.pick("Provider sent an unreadable answer", "Sağlayıcı okunamayan yanıt verdi"),
            Rejected => self.pick("Provider rejected the request", "Sağlayıcı isteği reddetti"),
            Audio => self.pick("Audio could not be encoded", "Ses kodlanamadı"),
            Microphone => self.pick("Microphone unavailable", "Mikrofon kullanılamıyor"),
            Credentials => self.pick("Credential Manager error", "Kimlik bilgisi deposu hatası"),
            Paste => self.pick("Could not paste — text is in History", "Yapıştırılamadı — metin Geçmiş'te"),
        }
    }

    pub fn tray_open(self) -> &'static str {
        self.pick("Open", "Aç")
    }

    pub fn tray_profile(self) -> &'static str {
        self.pick("Profile", "Profil")
    }

    pub fn tray_pause_hotkey(self) -> &'static str {
        self.pick("Pause shortcut", "Kısayolu duraklat")
    }

    pub fn tray_quit(self) -> &'static str {
        self.pick("Quit", "Çıkış")
    }

    pub fn tray_tooltip(self, recording: bool) -> &'static str {
        if recording {
            self.pick("Opit Speech to Text — recording", "Opit Speech to Text — kayıtta")
        } else {
            "Opit Speech to Text"
        }
    }
}
```

Insert above the tests in `crates/app/src/settings.rs`:
```rust
use std::sync::{Arc, PoisonError, RwLock};

use opit_core::config::AppConfig;
use opit_core::rules::builtin::assemble;
use opit_core::rules::{RulePack, RuleSet, RuleWarning};

use crate::i18n::{self, Lang};

pub struct Settings {
    pub config: AppConfig,
    pub rules: Arc<RuleSet>,
    pub lang: Lang,
}

impl Settings {
    /// Compiles the personal pack (if any) and the enabled built-in packs. Broken rules
    /// are skipped and returned as warnings.
    pub fn build(
        config: AppConfig,
        user_pack: Option<RulePack>,
        system_locale: Option<&str>,
    ) -> (Self, Vec<RuleWarning>) {
        let (rules, warnings) = RuleSet::compile(&assemble(user_pack, &config.rules.enabled_packs));
        let lang = i18n::resolve(config.ui_language.as_deref(), system_locale);
        (Self { config, rules: Arc::new(rules), lang }, warnings)
    }
}

#[derive(Clone)]
pub struct SettingsHandle(Arc<RwLock<Arc<Settings>>>);

impl SettingsHandle {
    pub fn new(settings: Settings) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(settings))))
    }

    pub fn current(&self) -> Arc<Settings> {
        self.0.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn replace(&self, settings: Settings) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(settings);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib -- controller::status i18n::tests settings::tests`
Expected: 8 passed (2 + 3 + 3).

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): add dictation status, en/tr strings and settings snapshot

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Data folder, start-up recovery and file logging

**Files:**
- Create: `crates/app/src/startup.rs`, `crates/app/src/logging.rs`
- Test: `crates/app/tests/logging_init.rs`
- Modify: `crates/app/src/lib.rs` (module list)

**Interfaces:**
- Consumes: `opit_core::config::{AppConfig, APP_DIR_NAME}`, `opit_core::rules::{RulePack, load_pack_file}`.
- Produces:
  - `startup::DATA_DIR_ENV` (`"OPIT_DATA_DIR"`), `startup::Paths { root, config, user_rules, history_db, audio, logs }` with `Paths::under(root)` and `Paths::from_system() -> Option<Paths>` (env override, else `dirs::config_dir()/opit-speech-to-text`).
  - `startup::StartupNotice::{ConfigReset { backup: Option<PathBuf>, reason }, UserRulesBroken { reason }}` (serde tag `kind`).
  - `startup::load_config(path, now_ms) -> LoadedConfig { config, created, notice }`: missing → defaults written, `created = true`; unusable (bad JSON, bad profile, newer schema, I/O) → renamed to `config.json.bad-<now_ms>`, defaults written only if the rename worked.
  - `startup::load_user_pack(path) -> (Option<RulePack>, Option<StartupNotice>)`: missing → `(None, None)`; broken → `(None, Some(UserRulesBroken))`.
  - `logging::init(log_dir) -> Result<WorkerGuard, LogInitError>` (daily `opit.YYYY-MM-DD.log`, UTC date, 7 files, no ANSI, `RUST_LOG` overrides the `info` default; second call fails before touching files), `logging::install_panic_hook()` (location only, never the payload). Keep the guard alive until exit.

- [ ] **Step 1: Wire the modules**

Add to the module list in `crates/app/src/lib.rs`:
```rust
pub mod logging;
pub mod startup;
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/startup.rs`:
```rust
//! Data locations and the start-up loading that must never stop the app from opening:
//! a broken `config.json` is backed up and replaced by defaults, a broken `user.yaml`
//! is skipped.

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join(APP_DIR_NAME));
        (dir, paths)
    }

    #[test]
    fn layout_matches_the_brief() {
        let paths = Paths::under(PathBuf::from("R"));
        assert_eq!(paths.config, PathBuf::from("R").join("config.json"));
        assert_eq!(paths.user_rules, PathBuf::from("R").join("rules").join("user.yaml"));
        assert_eq!(paths.history_db, PathBuf::from("R").join("history.db"));
        assert_eq!(paths.audio, PathBuf::from("R").join("audio"));
        assert_eq!(paths.logs, PathBuf::from("R").join("logs"));
    }

    #[test]
    fn first_run_writes_the_defaults() {
        let (_dir, paths) = paths();
        let loaded = load_config(&paths.config, 1);
        assert!(loaded.created && loaded.notice.is_none());
        assert_eq!(AppConfig::load(&paths.config).unwrap(), AppConfig::default());
    }

    #[test]
    fn a_valid_config_is_loaded_as_is() {
        let (_dir, paths) = paths();
        let mut config = AppConfig::default();
        config.paste.trailing_space = false;
        config.save(&paths.config).unwrap();
        let loaded = load_config(&paths.config, 1);
        assert!(!loaded.created && loaded.notice.is_none());
        assert_eq!(loaded.config, config);
    }

    #[test]
    fn a_broken_config_is_backed_up_and_replaced() {
        let (_dir, paths) = paths();
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(&paths.config, r#"{"profiles":[{"id":"x"}]}"#).unwrap();
        let loaded = load_config(&paths.config, 42);

        assert_eq!(loaded.config, AppConfig::default());
        let backup = paths.root.join("config.json.bad-42");
        match loaded.notice {
            Some(StartupNotice::ConfigReset { backup: Some(path), reason }) => {
                assert_eq!(path, backup);
                assert!(!reason.is_empty());
            }
            other => panic!("unexpected notice: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), r#"{"profiles":[{"id":"x"}]}"#);
        assert_eq!(AppConfig::load(&paths.config).unwrap(), AppConfig::default());
    }

    #[test]
    fn a_config_from_a_newer_version_is_backed_up_too() {
        let (_dir, paths) = paths();
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(&paths.config, r#"{"schema_version":99}"#).unwrap();
        let loaded = load_config(&paths.config, 7);
        assert!(matches!(loaded.notice, Some(StartupNotice::ConfigReset { backup: Some(_), .. })));
        assert!(paths.root.join("config.json.bad-7").exists());
    }

    #[test]
    fn user_pack_missing_valid_and_broken() {
        let (_dir, paths) = paths();
        assert_eq!(load_user_pack(&paths.user_rules), (None, None));

        std::fs::create_dir_all(paths.user_rules.parent().unwrap()).unwrap();
        std::fs::write(&paths.user_rules, "schema: 1\nid: user\nname: Me\nterms: [Opit]\n").unwrap();
        assert_eq!(load_user_pack(&paths.user_rules).0.unwrap().terms, ["Opit"]);

        std::fs::write(&paths.user_rules, "schema: 1\nid: user\nnme: typo\n").unwrap();
        let (pack, notice) = load_user_pack(&paths.user_rules);
        assert!(pack.is_none());
        assert!(matches!(notice, Some(StartupNotice::UserRulesBroken { .. })));
    }
}
```

`crates/app/tests/logging_init.rs` (its own test binary, because a global subscriber can be installed only once per process):
```rust
//! Global subscriber test: its own process, so `init` can run exactly once.

use std::fs;

use opit_app::logging;

#[test]
fn init_writes_daily_file_without_ansi_and_logs_panic_location() {
    let dir = tempfile::tempdir().unwrap();
    // Eight stale files: startup pruning leaves 6, then today's file makes 7.
    for day in 1..=8 {
        fs::write(dir.path().join(format!("opit.2020-01-0{day}.log")), "old\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20)); // distinct creation times
    }
    fs::write(dir.path().join("keep-me.txt"), "not a log").unwrap();

    let guard = logging::init(dir.path()).expect("init");
    assert!(logging::init(dir.path()).is_err(), "second init must fail");

    tracing::info!(provider = "openai", "dictation finished");
    tracing::debug!("hidden at the default level");
    let result = std::panic::catch_unwind(|| panic!("secret transcript text"));
    assert!(result.is_err());
    drop(guard); // flush the non-blocking writer

    let mut names: Vec<String> =
        fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    println!("files: {names:?}");
    let today = names.iter().find(|n| !n.starts_with("opit.2020") && n.starts_with("opit.")).expect("today's file");
    assert!(today.len() == "opit.YYYY-MM-DD.log".len() && today.ends_with(".log"), "{today}");
    assert_eq!(names.iter().filter(|n| n.starts_with("opit.")).count(), 7, "{names:?}");
    assert!(names.contains(&"keep-me.txt".to_owned()));

    let text = fs::read_to_string(dir.path().join(today)).unwrap();
    println!("{text}");
    assert!(text.contains("dictation finished") && text.contains("provider=\"openai\""));
    assert!(!text.contains("hidden at the default level"));
    assert!(!text.contains('\u{1b}'), "no ANSI escapes in files");
    assert!(text.contains("panic") && text.contains("logging_init.rs"), "panic location logged");
    assert!(!text.contains("secret transcript text"), "panic payload must not be logged");
}
```

Create `crates/app/src/logging.rs` with only its doc line so the crate compiles far enough to report the missing items:
```rust
//! File logging: daily-rotated `opit.YYYY-MM-DD.log` (UTC date), last 7 files kept.
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib startup; cargo test -p opit-speech-to-text --test logging_init`
Expected: FAIL to compile — `cannot find type Paths`, `cannot find function load_config`, `cannot find function init in module logging`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/startup.rs`:
```rust
use std::path::{Path, PathBuf};

use opit_core::config::{APP_DIR_NAME, AppConfig};
use opit_core::rules::{RulePack, load_pack_file};
use tracing::{info, warn};

/// Overrides the data folder (development and manual tests).
pub const DATA_DIR_ENV: &str = "OPIT_DATA_DIR";

/// `%APPDATA%\opit-speech-to-text\…`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub user_rules: PathBuf,
    pub history_db: PathBuf,
    pub audio: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    pub fn under(root: PathBuf) -> Self {
        Self {
            config: root.join("config.json"),
            user_rules: root.join("rules").join("user.yaml"),
            history_db: root.join("history.db"),
            audio: root.join("audio"),
            logs: root.join("logs"),
            root,
        }
    }

    /// `OPIT_DATA_DIR` when set (development, manual tests), else roaming AppData.
    pub fn from_system() -> Option<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Some(Self::under(PathBuf::from(dir)));
        }
        dirs::config_dir().map(|dir| Self::under(dir.join(APP_DIR_NAME)))
    }
}

/// Something the user should hear about once the app is up.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartupNotice {
    ConfigReset { backup: Option<PathBuf>, reason: String },
    UserRulesBroken { reason: String },
}

pub struct LoadedConfig {
    pub config: AppConfig,
    /// No config existed; defaults were written (first run).
    pub created: bool,
    pub notice: Option<StartupNotice>,
}

pub fn load_config(path: &Path, now_ms: i64) -> LoadedConfig {
    let existed = path.exists();
    match AppConfig::load(path) {
        Ok(config) if existed => LoadedConfig { config, created: false, notice: None },
        Ok(config) => {
            if let Err(err) = config.save(path) {
                warn!(error = %err, "could not write the default config");
            }
            info!("first run: default config written");
            LoadedConfig { config, created: true, notice: None }
        }
        Err(err) => {
            warn!(error = %err, "config.json is unusable; starting from defaults");
            let backup = path.with_file_name(format!("config.json.bad-{now_ms}"));
            let moved = std::fs::rename(path, &backup).is_ok();
            let config = AppConfig::default();
            // Never overwrite a file we could not move aside: it may be fine, just locked.
            if moved && let Err(err) = config.save(path) {
                warn!(error = %err, "could not write the default config");
            }
            let notice = StartupNotice::ConfigReset { backup: moved.then_some(backup), reason: err.to_string() };
            LoadedConfig { config, created: false, notice: Some(notice) }
        }
    }
}

/// The personal pack, or `None` when it does not exist or cannot be parsed.
pub fn load_user_pack(path: &Path) -> (Option<RulePack>, Option<StartupNotice>) {
    if !path.exists() {
        return (None, None);
    }
    match load_pack_file(path) {
        Ok(pack) => (Some(pack), None),
        Err(err) => {
            warn!(error = %err, "user.yaml skipped");
            (None, Some(StartupNotice::UserRulesBroken { reason: err.to_string() }))
        }
    }
}
```

Replace `crates/app/src/logging.rs`:
```rust
//! File logging: daily-rotated `opit.YYYY-MM-DD.log` (UTC date), last 7 files kept.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// Log file name prefix.
pub const FILE_PREFIX: &str = "opit";
/// Log file name suffix.
pub const FILE_SUFFIX: &str = "log";
/// Rotated files kept, today's included.
pub const MAX_FILES: usize = 7;

#[derive(Debug, thiserror::Error)]
pub enum LogInitError {
    #[error("could not create the log folder: {0}")]
    Dir(#[from] std::io::Error),
    #[error("could not open the log file: {0}")]
    Appender(#[from] tracing_appender::rolling::InitError),
    #[error("a global logger is already installed: {0}")]
    Subscriber(String),
}

/// Installs the global subscriber and the panic hook. Keep the guard alive until exit;
/// dropping it flushes the background writer.
pub fn init(log_dir: &Path) -> Result<WorkerGuard, LogInitError> {
    // Checked first: building the appender already prunes old files and opens today's file.
    if tracing::dispatcher::has_been_set() {
        return Err(LogInitError::Subscriber("init was called twice".into()));
    }
    std::fs::create_dir_all(log_dir)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .filename_suffix(FILE_SUFFIX)
        .max_log_files(MAX_FILES)
        .build(log_dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    // RUST_LOG overrides; invalid directives are ignored rather than fatal.
    let filter = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|e| LogInitError::Subscriber(e.to_string()))?;
    install_panic_hook();
    Ok(guard)
}

/// Logs the panic location (never the payload: panic messages such as string-slicing errors can
/// quote user text), then runs the previous hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        match info.location() {
            Some(loc) => tracing::error!(thread, file = loc.file(), line = loc.line(), "panic"),
            None => tracing::error!(thread, "panic at an unknown location"),
        }
        previous(info);
    }));
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib startup::tests`
Expected: 6 passed.

Run: `cargo test -p opit-speech-to-text --test logging_init -- --nocapture`
Expected: 1 passed; the printed file list holds 7 `opit.*.log` files plus `keep-me.txt`.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src crates/app/tests
git commit -m "feat(app): recover from a broken config and log to rotating files

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Dictation controller

**Files:**
- Replace: `crates/app/src/controller/mod.rs`
- Test: `crates/app/src/controller/tests.rs`

**Interfaces:**
- Consumes: Task 3 traits and fakes, Task 4 `ErrorKind`/`DictationState`/`DictationStatus`/`Lang`/`Settings`/`SettingsHandle`, `opit_core::pipeline::{prepare, transcribe, PipelineContext, PreparedAudio, Transcript}`, `opit_core::history::NewDictation`.
- Produces (`opit_app::controller`):
  - `trait Providers: Send + Sync + 'static { type Client: Transcriber + Send + Sync + 'static; fn client(&self, &Profile) -> Result<Self::Client, SetupError>; fn warm_up(&self, &Self::Client); }`
  - `enum SetupError { MissingKey(String /* profile name */), Secret(SecretError), Client(String) }` with `kind() -> ErrorKind`.
  - `trait HistorySink: Send + Sync { fn record(&self, &NewDictation<'_>, Option<&PreparedAudio>) -> Result<i64, String>; }`
  - `trait UiEvents: Send + Sync { fn status_changed(&self, &DictationStatus); fn history_added(&self, i64); fn open_settings(&self); }`
  - `struct Env<P: Providers> { settings, providers, mic, hotkey, paster, overlay, sounds, history, events, retry_delay }` (all trait objects are `Arc<dyn …>`).
  - `enum Msg { Toggle, Stop, Cancel, Retry, Hotkey(HotkeyEvent), Overlay(OverlayAction), Shutdown, … internal }` — not `Debug`.
  - `fn channel() -> (ControllerHandle, Inbox)`; `fn run<P>(&ControllerHandle, Inbox, Env<P>) -> impl Future<Output = ()> + Send + 'static` (spawn it on the tokio runtime; ends after `Msg::Shutdown`).
  - `ControllerHandle` (`Clone`): `send(Msg)` (never blocks), `status() -> DictationStatus`, `hotkey_sink() -> HotkeySink`, `overlay_sink() -> OverlaySink`.
  - `fn meter(rms: f32) -> f32` — -60..0 dB → 0..1 for the overlay bar.

Behaviour to implement (the tests pin each line):
- **Toggle** (tray, UI, toggle-mode `ComboDown`): Idle → start; Recording → stop; Transcribing/Pasting → ignored.
- **Hotkey mapping:** toggle mode — `ComboDown` toggles, `LoneCtrl` stops a recording, `ComboUp` ignored; push-to-talk — `ComboDown` starts from Idle, `ComboUp` stops; `Escape` cancels in both modes.
- **Start:** build clients for the active profile (+ fallback; a fallback without a key is skipped with a warning). `MissingKey`/credential errors → Error with a Settings button, microphone never opened. Drop the kept retry audio, open the mic (errors → `Microphone`), warm up the primary client, arm the recording-limit timer, capture Esc, play Start, show "Listening" (or "Listening (default microphone)" after a device fallback).
- **Stop:** `Capture::finish`, then on a task: `prepare` on the blocking pool, `transcribe` with retry + fallback. Esc stays captured during transcription.
- **Results:** `TooShort`/`NoSpeech` → info overlay, nothing sent, nothing saved. Other errors → keep the prepared audio, Error (button: Settings for key problems, else Try again). Empty/hallucinated text → saved to history, not pasted, info overlay. Otherwise paste on the blocking pool (trailing space and restore flag from the config).
- **Pasted:** save history (text without the trailing space; `latency_ms` from stop/retry to paste; audio only when `save_audio`), then `Pasted` → Done sound + "Pasted (0.9 s)"; `ClipboardOnly` → Done sound + warning "In clipboard — press Ctrl+V"; `Failed` → Error `Paste`.
- **Cancel:** from Recording (drop the capture) or Transcribing (abort the task; a late result is ignored by session id); Pasting is not cancellable. Emits `Cancelled` then `Idle`, Cancel sound. `Shutdown` cancels quietly (hides the overlay, no Cancelled).
- **Mic failure mid-recording** (`CaptureEvent::Failed`) → Error `Microphone`, no request.
- Sounds only when `ui.sound_feedback`; history only when `history.enabled`. Logs carry ids, durations, char counts, profile ids — never text.

- [ ] **Step 1: Write the failing tests**

`crates/app/src/controller/tests.rs`:
```rust
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opit_core::config::{AppConfig, HotkeyMode};
use opit_core::history::NewDictation;
use opit_core::pipeline::{PreparedAudio, TranscriptStatus};
use opit_core::provider::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use tokio::sync::{Semaphore, mpsc};

use super::*;
use crate::platform::fake::*;
use crate::platform::{HotkeyEvent, MicError, OverlayAction, PasteOutcome, Sound, Tone};
use crate::settings::{Settings, SettingsHandle};

use DictationState::{Cancelled, Idle, Pasting, Recording, Transcribing};

// ---------- scripted providers ----------

struct ProviderState {
    scripts: Mutex<HashMap<String, VecDeque<Result<String, ProviderError>>>>,
    missing_keys: Mutex<HashSet<String>>,
    gate: Arc<Semaphore>,
    calls: AtomicUsize,
    warmups: AtomicUsize,
}

impl ProviderState {
    fn new() -> Self {
        Self {
            scripts: Mutex::default(),
            missing_keys: Mutex::default(),
            gate: Arc::new(Semaphore::new(1_000)),
            calls: AtomicUsize::new(0),
            warmups: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(SeqCst)
    }
}

struct TestProviders(Arc<ProviderState>);

struct ScriptedClient {
    profile: Profile,
    state: Arc<ProviderState>,
}

impl Transcriber for ScriptedClient {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    fn transcribe(
        &self,
        _request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        let state = self.state.clone();
        let id = self.profile.id.clone();
        async move {
            let _permit = state.gate.acquire().await.unwrap();
            state.calls.fetch_add(1, SeqCst);
            let next = state.scripts.lock().unwrap().get_mut(&id).and_then(VecDeque::pop_front);
            next.unwrap_or_else(|| panic!("unexpected request to {id}"))
                .map(|text| RawTranscript { text, dropped_segments: 0 })
        }
    }
}

impl Providers for TestProviders {
    type Client = ScriptedClient;

    fn client(&self, profile: &Profile) -> Result<ScriptedClient, SetupError> {
        if self.0.missing_keys.lock().unwrap().contains(&profile.id) {
            return Err(SetupError::MissingKey(profile.name.clone()));
        }
        Ok(ScriptedClient { profile: profile.clone(), state: self.0.clone() })
    }

    fn warm_up(&self, _client: &ScriptedClient) {
        self.0.warmups.fetch_add(1, SeqCst);
    }
}

// ---------- history + events doubles ----------

#[derive(Debug, Clone, PartialEq)]
struct Saved {
    profile_id: String,
    raw_text: String,
    text: String,
    status: TranscriptStatus,
    has_audio: bool,
}

#[derive(Default)]
struct FakeHistory {
    saved: Mutex<Vec<Saved>>,
}

impl FakeHistory {
    fn entries(&self) -> Vec<Saved> {
        self.saved.lock().unwrap().clone()
    }
}

impl HistorySink for FakeHistory {
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String> {
        let mut saved = self.saved.lock().unwrap();
        saved.push(Saved {
            profile_id: entry.profile_id.into(),
            raw_text: entry.raw_text.into(),
            text: entry.text.into(),
            status: entry.status,
            has_audio: audio.is_some(),
        });
        Ok(saved.len() as i64)
    }
}

struct FakeEvents {
    tx: mpsc::UnboundedSender<DictationStatus>,
    history: Mutex<Vec<i64>>,
    settings_opened: AtomicUsize,
}

impl UiEvents for FakeEvents {
    fn status_changed(&self, status: &DictationStatus) {
        let _ = self.tx.send(status.clone());
    }

    fn history_added(&self, id: i64) {
        self.history.lock().unwrap().push(id);
    }

    fn open_settings(&self) {
        self.settings_opened.fetch_add(1, SeqCst);
    }
}

// ---------- harness ----------

struct Harness {
    handle: ControllerHandle,
    states: mpsc::UnboundedReceiver<DictationStatus>,
    settings: SettingsHandle,
    providers: Arc<ProviderState>,
    mic: Arc<FakeMic>,
    hotkey: Arc<FakeHotkey>,
    paster: Arc<FakePaster>,
    overlay: Arc<FakeOverlay>,
    sounds: Arc<FakeSounds>,
    history: Arc<FakeHistory>,
    events: Arc<FakeEvents>,
}

fn settings(config: AppConfig) -> Settings {
    Settings::build(config, None, Some("en-US")).0
}

fn harness(config: AppConfig) -> Harness {
    let (tx, states) = mpsc::unbounded_channel();
    let events = Arc::new(FakeEvents { tx, history: Mutex::default(), settings_opened: AtomicUsize::new(0) });
    let providers = Arc::new(ProviderState::new());
    let settings = SettingsHandle::new(settings(config));
    let (mic, hotkey, paster) = (Arc::<FakeMic>::default(), Arc::<FakeHotkey>::default(), Arc::<FakePaster>::default());
    let (overlay, sounds, history) =
        (Arc::<FakeOverlay>::default(), Arc::<FakeSounds>::default(), Arc::<FakeHistory>::default());
    let (handle, inbox) = channel();
    tokio::spawn(run(
        &handle,
        inbox,
        Env {
            settings: settings.clone(),
            providers: TestProviders(providers.clone()),
            mic: mic.clone(),
            hotkey: hotkey.clone(),
            paster: paster.clone(),
            overlay: overlay.clone(),
            sounds: sounds.clone(),
            history: history.clone(),
            events: events.clone(),
            retry_delay: Duration::ZERO,
        },
    ));
    Harness { handle, states, settings, providers, mic, hotkey, paster, overlay, sounds, history, events }
}

impl Harness {
    fn script(&self, profile: &str, replies: Vec<Result<&str, ProviderError>>) {
        let replies = replies.into_iter().map(|r| r.map(str::to_string)).collect();
        self.providers.scripts.lock().unwrap().insert(profile.into(), replies);
    }

    fn send(&self, msg: Msg) {
        self.handle.send(msg);
    }

    fn hotkey(&self, event: HotkeyEvent) {
        self.send(Msg::Hotkey(event));
    }

    async fn next(&mut self) -> DictationState {
        let status = tokio::time::timeout(Duration::from_secs(5), self.states.recv())
            .await
            .expect("timed out waiting for a state change")
            .expect("controller stopped");
        status.state
    }

    async fn expect(&mut self, expected: &[DictationState]) {
        for want in expected {
            assert_eq!(&self.next().await, want);
        }
    }

    async fn expect_error(&mut self, kind: ErrorKind) {
        match self.next().await {
            DictationState::Error { kind: got, message } => {
                assert_eq!(got, kind);
                assert!(!message.is_empty());
            }
            other => panic!("expected an error, got {other:?}"),
        }
        self.expect(&[Idle]).await;
    }

    /// Lets the actor drain its queue (for "nothing happened" assertions).
    async fn settle(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }
}

fn with_fallback() -> AppConfig {
    let mut config = AppConfig::default();
    config.profiles[0].fallback_profile_id = Some("openai".into());
    config
}

// ---------- tests ----------

#[tokio::test]
async fn toggle_records_transcribes_pastes_and_saves() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("cloud code'u aç")]);

    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    assert!(h.hotkey.escape_captured(), "Esc cancels while recording");
    assert_eq!(h.overlay.last().level, Some(0.0));
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, Pasting, Idle]).await;

    assert_eq!(h.paster.texts(), ["Claude Code'u aç "]);
    assert!(h.paster.pasted.lock().unwrap()[0].1, "clipboard restore follows the config");
    let saved = h.history.entries();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].raw_text, "cloud code'u aç");
    assert_eq!(saved[0].text, "Claude Code'u aç");
    assert_eq!(
        (saved[0].profile_id.as_str(), saved[0].status, saved[0].has_audio),
        ("groq", TranscriptStatus::Ok, false)
    );
    assert_eq!(*h.events.history.lock().unwrap(), [1]);
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Done]);
    let done = h.overlay.last();
    assert_eq!(done.tone, Tone::Success);
    assert!(done.text.starts_with("Pasted ("), "{}", done.text);
    assert!(!h.hotkey.escape_captured());
    assert_eq!((h.mic.finished.load(SeqCst), h.providers.warmups.load(SeqCst)), (1, 1));
    assert_eq!(h.handle.status(), DictationStatus::default());
}

#[tokio::test]
async fn toggle_mode_starts_on_the_combo_and_stops_on_a_lone_ctrl() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("merhaba")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::ComboUp);
    h.settle().await;
    assert_eq!(h.handle.status().state, Recording, "releasing the combo does not stop in toggle mode");
    h.hotkey(HotkeyEvent::LoneCtrl);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["merhaba "]);
}

#[tokio::test]
async fn push_to_talk_records_while_the_combo_is_held() {
    let mut config = AppConfig::default();
    config.hotkey.mode = HotkeyMode::PushToTalk;
    let mut h = harness(config);
    h.script("groq", vec![Ok("bas konuş")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::LoneCtrl);
    h.settle().await;
    assert_eq!(h.handle.status().state, Recording, "lone Ctrl only stops in toggle mode");
    h.hotkey(HotkeyEvent::ComboUp);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["bas konuş "]);
}

#[tokio::test]
async fn triggers_while_busy_are_ignored() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("tek")]);
    h.providers.gate.forget_permits(1_000);

    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.send(Msg::Toggle);
    h.hotkey(HotkeyEvent::ComboDown);
    h.send(Msg::Retry);
    h.settle().await;
    assert_eq!(h.mic.starts(), 1, "no second recording while transcribing");
    assert_eq!(h.handle.status().state, Transcribing);

    h.providers.gate.add_permits(1_000);
    h.expect(&[Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["tek "]);
}

#[tokio::test]
async fn escape_cancels_a_recording() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Cancelled, Idle]).await;

    assert_eq!((h.mic.finished.load(SeqCst), h.mic.dropped.load(SeqCst)), (0, 1));
    assert_eq!(h.providers.calls(), 0);
    assert!(h.history.entries().is_empty());
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Cancel]);
    assert!(!h.hotkey.escape_captured());
}

#[tokio::test]
async fn escape_cancels_a_transcription() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("geç kalan")]);
    h.providers.gate.forget_permits(1_000);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    assert!(h.hotkey.escape_captured(), "Esc still cancels while transcribing");
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Cancelled, Idle]).await;

    h.providers.gate.add_permits(1_000);
    tokio::time::sleep(Duration::from_millis(50)).await;
    h.settle().await;
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    assert_eq!(h.handle.status(), DictationStatus::default());
}

#[tokio::test]
async fn microphone_failure_during_recording_is_an_error() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.emit(CaptureEvent::Level(0.2));
    h.mic.emit(CaptureEvent::Failed("device unplugged".into()));
    h.expect_error(ErrorKind::Microphone).await;

    assert_eq!(h.providers.calls(), 0);
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert!(!h.handle.status().can_retry);
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Error]);
    let error = h.overlay.last();
    assert_eq!((error.tone, error.button.is_none()), (Tone::Error, true));
    assert!(h.overlay.views.lock().unwrap().iter().any(|v| v.level.is_some_and(|l| l > 0.0)), "level meter moved");
}

#[tokio::test]
async fn a_microphone_that_cannot_open_never_enters_recording() {
    let mut h = harness(AppConfig::default());
    *h.mic.fail_start.lock().unwrap() = Some(MicError::NoDevice);
    h.send(Msg::Toggle);
    h.expect_error(ErrorKind::Microphone).await;
    assert!(!h.hotkey.escape_captured());
}

#[tokio::test]
async fn a_missing_api_key_blocks_recording_and_offers_settings() {
    let mut h = harness(AppConfig::default());
    h.providers.missing_keys.lock().unwrap().insert("groq".into());
    h.send(Msg::Toggle);
    h.expect_error(ErrorKind::MissingKey).await;
    assert_eq!(h.mic.starts(), 0, "no point recording without a key");
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::OpenSettings));

    h.send(Msg::Overlay(OverlayAction::OpenSettings));
    h.settle().await;
    assert_eq!(h.events.settings_opened.load(SeqCst), 1);
}

#[tokio::test]
async fn silence_is_not_sent() {
    let mut h = harness(AppConfig::default());
    *h.mic.recording.lock().unwrap() = Some(silent_recording());
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Idle]).await;
    assert_eq!(h.providers.calls(), 0);
    assert_eq!(h.overlay.last().text, "No speech detected");
    assert!(h.history.entries().is_empty());
}

#[tokio::test]
async fn a_provider_failure_keeps_the_audio_for_try_again() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::Server(500)), Err(ProviderError::Server(500)), Ok("tekrar")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::Server).await;
    assert!(h.handle.status().can_retry);
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::Retry));
    assert!(h.history.entries().is_empty());

    h.send(Msg::Overlay(OverlayAction::Retry));
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["tekrar "]);
    assert_eq!(h.mic.starts(), 1, "the kept audio is resent, nothing is re-recorded");
    assert_eq!(h.providers.calls(), 3);
    assert!(!h.handle.status().can_retry);
    assert_eq!(h.history.entries().len(), 1);
}

#[tokio::test]
async fn the_fallback_profile_answers_after_the_primary_fails() {
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::RateLimited), Err(ProviderError::RateLimited)]);
    h.script("openai", vec![Ok("yedek")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["yedek "]);
    assert_eq!(h.history.entries()[0].profile_id, "openai");
}

#[tokio::test]
async fn a_rejected_key_points_to_settings_but_keeps_the_audio() {
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::InvalidKey).await;
    assert_eq!(h.providers.calls(), 1, "401 is neither retried nor sent to the fallback");
    assert!(h.handle.status().can_retry);
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::OpenSettings));
}

#[tokio::test(start_paused = true)]
async fn the_recording_limit_sends_automatically() {
    let mut config = AppConfig::default();
    config.recording.max_seconds = 10;
    let mut h = harness(config);
    h.script("groq", vec![Ok("uzun kayıt")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["uzun kayıt "]);
}

#[tokio::test]
async fn empty_output_is_saved_but_not_pasted() {
    let mut config = AppConfig::default();
    config.ui.sound_feedback = false;
    let mut h = harness(config);
    h.script("groq", vec![Ok("   ")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Idle]).await;
    assert!(h.paster.texts().is_empty());
    assert_eq!(h.history.entries()[0].status, TranscriptStatus::Empty);
    assert_eq!(h.overlay.last().text, "No text came back");
    assert!(h.sounds.played.lock().unwrap().is_empty(), "sound feedback is off");
}

#[tokio::test]
async fn a_refused_paste_leaves_the_text_on_the_clipboard() {
    let mut h = harness(AppConfig::default());
    *h.paster.outcome.lock().unwrap() = PasteOutcome::ClipboardOnly;
    h.script("groq", vec![Ok("yönetici penceresi")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    let hint = h.overlay.last();
    assert_eq!(hint.tone, Tone::Warning);
    assert!(hint.text.contains("Ctrl+V"), "{}", hint.text);
    assert_eq!(h.history.entries().len(), 1);
}

#[tokio::test]
async fn a_failed_paste_is_an_error_but_the_text_is_saved() {
    let mut h = harness(AppConfig::default());
    *h.paster.outcome.lock().unwrap() = PasteOutcome::Failed("clipboard busy".into());
    h.script("groq", vec![Ok("kaybolmasın")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting]).await;
    h.expect_error(ErrorKind::Paste).await;
    assert_eq!(h.history.entries()[0].text, "kaybolmasın");
}

#[tokio::test]
async fn history_follows_the_live_config() {
    let mut config = AppConfig::default();
    config.history.enabled = false;
    let mut h = harness(config.clone());
    h.script("groq", vec![Ok("bir"), Ok("iki")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    assert!(h.history.entries().is_empty());

    config.history.enabled = true;
    config.history.save_audio = true;
    h.settings.replace(settings(config));
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    let saved = h.history.entries();
    assert_eq!((saved.len(), saved[0].text.as_str(), saved[0].has_audio), (1, "iki", true));
}

#[tokio::test]
async fn shutdown_releases_the_microphone_quietly() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Shutdown);
    h.expect(&[Idle]).await;
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert_eq!(h.overlay.hides.load(SeqCst), 1);
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start]);
}

#[tokio::test]
async fn transcript_text_never_reaches_the_logs() {
    let logs = captured_logs();
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::Server(503)), Err(ProviderError::Server(503))]);
    h.script("openai", vec![Ok("çok gizli cümle")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;

    let logs = String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();
    assert!(logs.contains("transcribed") && logs.contains("pasted"), "{logs}");
    assert!(!logs.contains("gizli"), "transcript leaked into the logs:\n{logs}");
}

/// Every log line of this test binary. A process-wide subscriber is used because a
/// thread-local `set_default` misses events when other tests run in parallel
/// (tracing caches callsite interest globally).
fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || CaptureWriter(writer.clone()))
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("only this test installs a subscriber");
        logs
    })
    .clone()
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_level_meter_uses_a_decibel_scale() {
    assert_eq!(meter(0.0), 0.0);
    assert_eq!(meter(f32::NAN), 0.0);
    assert_eq!(meter(1.0), 1.0);
    assert!((meter(0.1) - 2.0 / 3.0).abs() < 1e-4);
    assert_eq!(meter(0.000_1), 0.0);
}
```

Add below `mod status;` in `crates/app/src/controller/mod.rs`:
```rust
#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib controller::tests`
Expected: FAIL to compile — `cannot find trait Providers`, `cannot find function channel`, `cannot find type Msg`.

- [ ] **Step 3: Implement the controller**

Replace `crates/app/src/controller/mod.rs`:
```rust
//! Dictation state machine: Idle → Recording → Transcribing → Pasting → Idle, plus the
//! transient Cancelled and Error states. One actor task owns every piece of dictation
//! state; the hotkey hook, tray, overlay and UI only send it messages, so two dictations
//! can never overlap.

mod status;
#[cfg(test)]
mod tests;

use std::mem;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opit_core::audio::Recording;
use opit_core::audio::encode::EncodeError;
use opit_core::config::HotkeyMode;
use opit_core::history::NewDictation;
use opit_core::pipeline::{self, PipelineContext, PipelineError, PreparedAudio, Transcript, TranscriptStatus};
use opit_core::provider::{Profile, Transcriber};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tracing::{info, warn};

pub use status::{DictationState, DictationStatus, ErrorKind};

use crate::i18n::Lang;
use crate::platform::{
    Capture, CaptureEvent, CaptureSink, Hotkey, HotkeyEvent, HotkeySink, Microphone, Overlay, OverlayAction,
    OverlayButton, OverlaySink, OverlayView, PasteOutcome, Paster, SecretError, Sound, Sounds, Tone,
};
use crate::settings::{Settings, SettingsHandle};

const PASTED_HIDE: Duration = Duration::from_millis(1_500);
const INFO_HIDE: Duration = Duration::from_millis(2_500);
const CANCEL_HIDE: Duration = Duration::from_millis(1_000);
const WARNING_HIDE: Duration = Duration::from_secs(5);
const ERROR_HIDE: Duration = Duration::from_secs(5);
const ERROR_WITH_BUTTON_HIDE: Duration = Duration::from_secs(8);

/// Builds provider clients for profiles. The real implementation caches one HTTP client
/// per profile so dictations reuse pooled connections.
pub trait Providers: Send + Sync + 'static {
    type Client: Transcriber + Send + Sync + 'static;

    fn client(&self, profile: &Profile) -> Result<Self::Client, SetupError>;

    /// Best-effort connection warm-up while the user is still speaking.
    fn warm_up(&self, client: &Self::Client);
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("no API key is stored for profile {0}")]
    MissingKey(String),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("the provider client could not be created: {0}")]
    Client(String),
}

impl SetupError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            SetupError::MissingKey(_) => ErrorKind::MissingKey,
            SetupError::Secret(_) => ErrorKind::Credentials,
            SetupError::Client(_) => ErrorKind::Network,
        }
    }
}

pub trait HistorySink: Send + Sync {
    /// Stores one dictation (and its 16 kHz audio when given); returns the row id.
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String>;
}

/// Notifications for the tray and the web UI.
pub trait UiEvents: Send + Sync {
    fn status_changed(&self, status: &DictationStatus);
    fn history_added(&self, id: i64);
    fn open_settings(&self);
}

pub struct Env<P: Providers> {
    pub settings: SettingsHandle,
    pub providers: P,
    pub mic: Arc<dyn Microphone>,
    pub hotkey: Arc<dyn Hotkey>,
    pub paster: Arc<dyn Paster>,
    pub overlay: Arc<dyn Overlay>,
    pub sounds: Arc<dyn Sounds>,
    pub history: Arc<dyn HistorySink>,
    pub events: Arc<dyn UiEvents>,
    /// Wait before the one retry of a failed request (500 ms in the app, zero in tests).
    pub retry_delay: Duration,
}

/// Controller input. Deliberately not `Debug`: `Transcribed` carries transcript text,
/// which must never reach a log line.
pub enum Msg {
    /// Tray left click, UI button, or the toggle-mode hotkey: start or stop.
    Toggle,
    Stop,
    Cancel,
    Retry,
    Hotkey(HotkeyEvent),
    Overlay(OverlayAction),
    Shutdown,
    Capture {
        session: u64,
        event: CaptureEvent,
    },
    LimitReached {
        session: u64,
    },
    Transcribed {
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        result: Result<Transcript, PipelineError>,
    },
    Pasted {
        session: u64,
        outcome: PasteOutcome,
    },
}

#[derive(Clone)]
pub struct ControllerHandle {
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
}

pub struct Inbox(mpsc::UnboundedReceiver<Msg>);

/// Creates the handle first so the overlay and hotkey sinks can exist before the
/// controller that consumes them.
pub fn channel() -> (ControllerHandle, Inbox) {
    let (tx, rx) = mpsc::unbounded_channel();
    (ControllerHandle { tx, status: Arc::default() }, Inbox(rx))
}

/// The actor loop. Spawn it on the app's tokio runtime; it ends after `Msg::Shutdown`.
pub fn run<P: Providers>(
    handle: &ControllerHandle,
    inbox: Inbox,
    env: Env<P>,
) -> impl Future<Output = ()> + Send + 'static {
    let controller = Controller {
        env,
        tx: handle.tx.clone(),
        status: handle.status.clone(),
        phase: Phase::Idle,
        session: 0,
        retry: None,
    };
    controller.serve(inbox)
}

impl ControllerHandle {
    /// Never blocks; messages sent after shutdown are dropped.
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn status(&self) -> DictationStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn hotkey_sink(&self) -> HotkeySink {
        let tx = self.tx.clone();
        Arc::new(move |event| {
            let _ = tx.send(Msg::Hotkey(event));
        })
    }

    pub fn overlay_sink(&self) -> OverlaySink {
        let tx = self.tx.clone();
        Arc::new(move |action| {
            let _ = tx.send(Msg::Overlay(action));
        })
    }
}

struct Clients<C> {
    primary: C,
    fallback: Option<C>,
}

struct Job {
    settings: Arc<Settings>,
    /// When the user stopped recording (or pressed "Try again"); latency is measured from here.
    started: Instant,
}

enum Input {
    Raw(Recording),
    Prepared(Arc<PreparedAudio>),
}

enum Phase<C> {
    Idle,
    Recording {
        session: u64,
        capture: Box<dyn Capture>,
        settings: Arc<Settings>,
        clients: Clients<C>,
        limit: AbortHandle,
        label: &'static str,
    },
    Transcribing {
        session: u64,
        job: Job,
        task: AbortHandle,
    },
    Pasting {
        session: u64,
        job: Job,
        transcript: Transcript,
        audio: Option<Arc<PreparedAudio>>,
    },
}

struct Controller<P: Providers> {
    env: Env<P>,
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
    phase: Phase<P::Client>,
    session: u64,
    /// Prepared audio of the last failed dictation, for "Try again".
    retry: Option<Arc<PreparedAudio>>,
}

impl<P: Providers> Controller<P> {
    async fn serve(mut self, mut inbox: Inbox) {
        while let Some(msg) = inbox.0.recv().await {
            if let Msg::Shutdown = msg {
                self.cancel(false);
                break;
            }
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.toggle(),
            Msg::Stop => self.stop(),
            Msg::Cancel => self.cancel(true),
            Msg::Retry | Msg::Overlay(OverlayAction::Retry) => self.retry(),
            Msg::Overlay(OverlayAction::OpenSettings) => self.env.events.open_settings(),
            Msg::Hotkey(event) => self.on_hotkey(event),
            Msg::Capture { session, event } => self.on_capture(session, event),
            Msg::LimitReached { session } => {
                if self.recording_session() == Some(session) {
                    info!(session, "recording limit reached; sending");
                    self.stop();
                }
            }
            Msg::Transcribed { session, audio, result } => self.on_transcribed(session, audio, result),
            Msg::Pasted { session, outcome } => self.on_pasted(session, outcome),
            Msg::Shutdown => {}
        }
    }

    fn recording_session(&self) -> Option<u64> {
        match self.phase {
            Phase::Recording { session, .. } => Some(session),
            _ => None,
        }
    }

    fn toggle(&mut self) {
        match self.phase {
            Phase::Idle => self.start(),
            Phase::Recording { .. } => self.stop(),
            // One dictation at a time: triggers while busy are ignored.
            Phase::Transcribing { .. } | Phase::Pasting { .. } => info!("busy; trigger ignored"),
        }
    }

    fn on_hotkey(&mut self, event: HotkeyEvent) {
        let mode = self.env.settings.current().config.hotkey.mode;
        let idle = matches!(self.phase, Phase::Idle);
        let recording = matches!(self.phase, Phase::Recording { .. });
        match (event, mode) {
            (HotkeyEvent::Escape, _) => self.cancel(true),
            (HotkeyEvent::ComboDown, HotkeyMode::Toggle) => self.toggle(),
            (HotkeyEvent::LoneCtrl, HotkeyMode::Toggle) if recording => self.stop(),
            (HotkeyEvent::ComboDown, HotkeyMode::PushToTalk) if idle => self.start(),
            (HotkeyEvent::ComboUp, HotkeyMode::PushToTalk) if recording => self.stop(),
            _ => {}
        }
    }

    fn clients(&self, settings: &Settings) -> Result<Clients<P::Client>, SetupError> {
        let config = &settings.config;
        let profile = config.active_profile().ok_or_else(|| SetupError::Client("no active profile".into()))?;
        let primary = self.env.providers.client(profile)?;
        let fallback = config.fallback_for(profile).and_then(|p| match self.env.providers.client(p) {
            Ok(client) => Some(client),
            Err(err) => {
                warn!(profile = %p.id, error = %err, "fallback profile unavailable");
                None
            }
        });
        Ok(Clients { primary, fallback })
    }

    fn start(&mut self) {
        let settings = self.env.settings.current();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot start dictation");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.retry = None;
        self.session += 1;
        let session = self.session;
        let tx = self.tx.clone();
        let sink: CaptureSink = Arc::new(move |event| {
            let _ = tx.send(Msg::Capture { session, event });
        });
        let started = match self.env.mic.start(settings.config.recording.microphone.as_deref(), sink) {
            Ok(started) => started,
            Err(err) => {
                warn!(error = %err, "microphone failed to start");
                return self.fail(ErrorKind::Microphone, settings.lang);
            }
        };
        if started.fell_back {
            warn!(device = %started.device, "configured microphone unavailable; using the default device");
        }
        self.env.providers.warm_up(&clients.primary);

        let max = Duration::from_secs(u64::from(settings.config.recording.max_seconds));
        let tx = self.tx.clone();
        let limit = tokio::spawn(async move {
            tokio::time::sleep(max).await;
            let _ = tx.send(Msg::LimitReached { session });
        })
        .abort_handle();

        let label = if started.fell_back { settings.lang.listening_default_mic() } else { settings.lang.listening() };
        self.env.hotkey.set_capture_escape(true);
        self.play(Sound::Start);
        self.env.overlay.show(listening(label, 0.0));
        info!(session, device = %started.device, profile = %settings.config.active_profile_id, "recording started");
        self.phase = Phase::Recording { session, capture: started.capture, settings, clients, limit, label };
        self.set_state(DictationState::Recording);
    }

    fn stop(&mut self) {
        if self.recording_session().is_none() {
            return;
        }
        let Phase::Recording { session, capture, settings, clients, limit, .. } =
            mem::replace(&mut self.phase, Phase::Idle)
        else {
            unreachable!("checked above")
        };
        limit.abort();
        let recording = capture.finish();
        info!(session, audio_ms = recording.duration_ms(), "recording stopped");
        self.begin_transcription(session, settings, clients, Input::Raw(recording));
    }

    fn retry(&mut self) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        let Some(audio) = self.retry.clone() else {
            return;
        };
        let settings = self.env.settings.current();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot retry");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.session += 1;
        info!(session = self.session, "retrying the last dictation");
        self.env.hotkey.set_capture_escape(true);
        self.begin_transcription(self.session, settings, clients, Input::Prepared(audio));
    }

    fn begin_transcription(
        &mut self,
        session: u64,
        settings: Arc<Settings>,
        clients: Clients<P::Client>,
        input: Input,
    ) {
        self.env.overlay.show(view(Tone::Busy, settings.lang.transcribing(), None));
        let tx = self.tx.clone();
        let retry_delay = self.env.retry_delay;
        let task_settings = settings.clone();
        let task = tokio::spawn(async move {
            let audio = match input {
                Input::Prepared(audio) => audio,
                Input::Raw(recording) => {
                    let prepared = tokio::task::spawn_blocking(move || pipeline::prepare(&recording))
                        .await
                        .unwrap_or_else(|join| Err(PipelineError::Encode(EncodeError(join.to_string()))));
                    match prepared {
                        Ok(audio) => Arc::new(audio),
                        Err(err) => {
                            let _ = tx.send(Msg::Transcribed { session, audio: None, result: Err(err) });
                            return;
                        }
                    }
                }
            };
            let ctx = PipelineContext {
                primary: &clients.primary,
                fallback: clients.fallback.as_ref(),
                rules: &task_settings.rules,
                prompt_context: &task_settings.config.rules.prompt_context,
                retry_delay,
            };
            let result = pipeline::transcribe(&audio, &ctx).await;
            let _ = tx.send(Msg::Transcribed { session, audio: Some(audio), result });
        })
        .abort_handle();
        self.phase = Phase::Transcribing { session, job: Job { settings, started: Instant::now() }, task };
        self.set_state(DictationState::Transcribing);
    }

    fn on_capture(&mut self, session: u64, event: CaptureEvent) {
        let Phase::Recording { session: current, label, .. } = self.phase else {
            return;
        };
        if current != session {
            return;
        }
        match event {
            CaptureEvent::Level(level) => self.env.overlay.show(listening(label, level)),
            CaptureEvent::Failed(reason) => {
                warn!(session, %reason, "microphone stopped during recording");
                let Phase::Recording { capture, limit, settings, .. } = mem::replace(&mut self.phase, Phase::Idle)
                else {
                    unreachable!("checked above")
                };
                limit.abort();
                drop(capture);
                self.fail(ErrorKind::Microphone, settings.lang);
            }
        }
    }

    fn on_transcribed(
        &mut self,
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        result: Result<Transcript, PipelineError>,
    ) {
        if !matches!(self.phase, Phase::Transcribing { session: current, .. } if current == session) {
            return; // cancelled meanwhile
        }
        let Phase::Transcribing { job, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        self.env.hotkey.set_capture_escape(false);
        let lang = job.settings.lang;
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(PipelineError::TooShort) => {
                info!(session, "recording too short; not sent");
                return self.finish_info(lang.too_short());
            }
            Err(PipelineError::NoSpeech) => {
                info!(session, "no speech detected; not sent");
                return self.finish_info(lang.no_speech());
            }
            Err(err) => {
                warn!(session, error = %err, "transcription failed");
                self.retry = audio;
                return self.fail(ErrorKind::from(&err), lang);
            }
        };
        self.retry = None;
        info!(
            session,
            status = transcript.status.as_str(),
            profile = %transcript.profile_id,
            fallback = transcript.used_fallback,
            request_ms = transcript.request_ms,
            chars = transcript.text.chars().count(),
            rule_hits = transcript.hits.len(),
            "transcribed"
        );
        if transcript.status != TranscriptStatus::Ok || transcript.text.is_empty() {
            self.record(&job, &transcript, audio.as_deref());
            return self.finish_info(lang.empty());
        }

        let paste = &job.settings.config.paste;
        let text = if paste.trailing_space { format!("{} ", transcript.text) } else { transcript.text.clone() };
        let restore = paste.restore_clipboard;
        let paster = self.env.paster.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let outcome = paster.paste(&text, restore);
            let _ = tx.send(Msg::Pasted { session, outcome });
        });
        self.phase = Phase::Pasting { session, job, transcript, audio };
        self.set_state(DictationState::Pasting);
    }

    fn on_pasted(&mut self, session: u64, outcome: PasteOutcome) {
        if !matches!(self.phase, Phase::Pasting { session: current, .. } if current == session) {
            return;
        }
        let Phase::Pasting { job, transcript, audio, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        let lang = job.settings.lang;
        let elapsed = job.started.elapsed();
        self.record(&job, &transcript, audio.as_deref());
        match outcome {
            PasteOutcome::Pasted => {
                info!(session, total_ms = elapsed.as_millis() as u64, "pasted");
                self.play(Sound::Done);
                self.env.overlay.show(view(Tone::Success, lang.pasted(elapsed.as_secs_f32()), Some(PASTED_HIDE)));
            }
            PasteOutcome::ClipboardOnly => {
                info!(session, "target window refused input; text left on the clipboard");
                self.play(Sound::Done);
                self.env.overlay.show(view(Tone::Warning, lang.clipboard_only(), Some(WARNING_HIDE)));
            }
            PasteOutcome::Failed(reason) => {
                warn!(session, %reason, "paste failed");
                return self.fail(ErrorKind::Paste, lang);
            }
        }
        self.set_state(DictationState::Idle);
    }

    fn record(&self, job: &Job, transcript: &Transcript, audio: Option<&PreparedAudio>) {
        let history = &job.settings.config.history;
        if !history.enabled {
            return;
        }
        let entry = NewDictation {
            created_at_ms: now_ms(),
            profile_id: &transcript.profile_id,
            raw_text: &transcript.raw_text,
            text: &transcript.text,
            status: transcript.status,
            audio_ms: transcript.audio_ms,
            latency_ms: job.started.elapsed().as_millis() as u64,
        };
        let audio = if history.save_audio { audio } else { None };
        match self.env.history.record(&entry, audio) {
            Ok(id) => self.env.events.history_added(id),
            Err(err) => warn!(error = %err, "history write failed"),
        }
    }

    /// `announce: false` is the quiet variant used on shutdown.
    fn cancel(&mut self, announce: bool) {
        let lang = match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => return,
            pasting @ Phase::Pasting { .. } => {
                self.phase = pasting; // Ctrl+V is already on its way
                return;
            }
            Phase::Recording { session, capture, limit, settings, .. } => {
                limit.abort();
                drop(capture);
                info!(session, "recording cancelled");
                settings.lang
            }
            Phase::Transcribing { session, task, job } => {
                task.abort();
                info!(session, "transcription cancelled");
                job.settings.lang
            }
        };
        self.env.hotkey.set_capture_escape(false);
        if announce {
            self.play(Sound::Cancel);
            self.env.overlay.show(view(Tone::Neutral, lang.cancelled(), Some(CANCEL_HIDE)));
            self.set_state(DictationState::Cancelled);
        } else {
            self.env.overlay.hide();
        }
        self.set_state(DictationState::Idle);
    }

    fn fail(&mut self, kind: ErrorKind, lang: Lang) {
        self.env.hotkey.set_capture_escape(false);
        let message = lang.error(kind).to_string();
        let button = if kind.needs_settings() {
            Some(OverlayButton { action: OverlayAction::OpenSettings, label: lang.open_settings().into() })
        } else if self.retry.is_some() {
            Some(OverlayButton { action: OverlayAction::Retry, label: lang.retry().into() })
        } else {
            None
        };
        let hide_after = if button.is_some() { ERROR_WITH_BUTTON_HIDE } else { ERROR_HIDE };
        self.env.overlay.show(OverlayView {
            tone: Tone::Error,
            text: message.clone(),
            level: None,
            button,
            hide_after: Some(hide_after),
        });
        self.play(Sound::Error);
        self.set_state(DictationState::Error { kind, message });
        self.set_state(DictationState::Idle);
    }

    fn finish_info(&mut self, text: &str) {
        self.env.overlay.show(view(Tone::Neutral, text, Some(INFO_HIDE)));
        self.set_state(DictationState::Idle);
    }

    fn play(&self, sound: Sound) {
        if self.env.settings.current().config.ui.sound_feedback {
            self.env.sounds.play(sound);
        }
    }

    fn set_state(&mut self, state: DictationState) {
        let status = DictationStatus { state, can_retry: self.retry.is_some() };
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status.clone();
        self.env.events.status_changed(&status);
    }
}

fn view(tone: Tone, text: impl Into<String>, hide_after: Option<Duration>) -> OverlayView {
    OverlayView { tone, text: text.into(), level: None, button: None, hide_after }
}

fn listening(label: &str, rms: f32) -> OverlayView {
    OverlayView { tone: Tone::Busy, text: label.into(), level: Some(meter(rms)), button: None, hide_after: None }
}

/// Maps microphone RMS to a 0..1 bar on a -60..0 dB scale, so normal speech (≈ -20 dB)
/// fills about two thirds of the bar instead of a sliver.
pub fn meter(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib controller`
Expected: 23 passed (21 controller tests + 2 status tests). Run it three times; it must never flake (the log test installs a process-wide subscriber on purpose — a thread-local `set_default` misses events when other tests run in parallel).

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/controller
git commit -m "feat(app): add the dictation controller state machine

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Provider clients and history storage

**Files:**
- Create: `crates/app/src/providers.rs`, `crates/app/src/history_service.rs`
- Modify: `crates/app/src/lib.rs` (module list)

**Interfaces:**
- Consumes: `controller::{Providers, SetupError, HistorySink}`, `platform::SecretStore`, `platform::fake::FakeSecrets`, Task 1 `OpenAiCompatible::warm_up` and `Arc<T>: Transcriber`, `opit_core::history::{HistoryStore, AudioStore, NewDictation, HistoryError}`, `opit_core::audio::encode::{encode_wav, to_pcm16}`.
- Produces:
  - `providers::HttpProviders::new(Arc<dyn SecretStore>)` implementing `Providers` with `Client = Arc<OpenAiCompatible>`: keys come from the store (`api_key_ref: None` → no key; missing or blank → `MissingKey(profile.name)`); one client per profile id, rebuilt only when the profile or key changes; `warm_up` spawns `client.warm_up()` on tokio (call it from inside the runtime).
  - `history_service::HistoryService::{open(db, audio_dir), with_store(store, audio_dir), store() -> MutexGuard<HistoryStore>, delete(id), clear(), purge_expired_audio(now_ms, retention_days) -> usize}` implementing `HistorySink` (row first; with audio, a 16 kHz mono WAV `audio/<id>.wav` and its path on the row).
  - `history_service::DisabledHistory` — `HistorySink` stand-in when `history.db` cannot be opened.

- [ ] **Step 1: Wire the modules**

Add to the module list in `crates/app/src/lib.rs`:
```rust
pub mod history_service;
pub mod providers;
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/providers.rs`:
```rust
//! Real provider clients: API keys from the credential store, one cached HTTP client per
//! profile so consecutive dictations reuse the pooled (already TLS-handshaked) connection.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use opit_core::provider::presets;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::platform::fake::FakeSecrets;

    #[test]
    fn clients_are_cached_until_the_profile_or_key_changes() {
        let secrets = Arc::new(FakeSecrets::with("groq", "sk-1"));
        let providers = HttpProviders::new(secrets.clone());
        let groq = presets::groq();
        let first = providers.client(&groq).unwrap();
        assert!(Arc::ptr_eq(&first, &providers.client(&groq).unwrap()));

        secrets.set("groq", "sk-2").unwrap();
        let rekeyed = providers.client(&groq).unwrap();
        assert!(!Arc::ptr_eq(&first, &rekeyed));

        let mut edited = groq.clone();
        edited.model = "whisper-large-v3-turbo".into();
        assert!(!Arc::ptr_eq(&rekeyed, &providers.client(&edited).unwrap()));
    }

    #[test]
    fn a_missing_or_blank_key_is_reported_by_profile_name() {
        let secrets = Arc::new(FakeSecrets::with("openai", "  "));
        let providers = HttpProviders::new(secrets);
        assert_eq!(providers.client(&presets::groq()).err(), Some(SetupError::MissingKey("Groq".into())));
        assert_eq!(providers.client(&presets::openai()).err(), Some(SetupError::MissingKey("OpenAI".into())));
    }

    #[test]
    fn keyless_profiles_need_no_secret_and_store_errors_surface() {
        let secrets = Arc::new(FakeSecrets::default());
        *secrets.broken.lock().unwrap() = true;
        let providers = HttpProviders::new(secrets);
        let mut local = presets::custom("gpu", "GPU", "http://127.0.0.1:8888/v1", "large-v3");
        local.api_key_ref = None;
        assert!(providers.client(&local).is_ok());
        assert!(matches!(providers.client(&presets::groq()), Err(SetupError::Secret(_))));
    }

    #[tokio::test]
    async fn warm_up_touches_the_models_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
        let mut profile = presets::custom("local", "Local", &format!("{}/v1", server.uri()), "m");
        profile.api_key_ref = None;
        let providers = HttpProviders::new(Arc::new(FakeSecrets::default()));
        let client = providers.client(&profile).unwrap();
        providers.warm_up(&client);
        for _ in 0..100 {
            if !server.received_requests().await.unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("warm-up never reached the server");
    }
}
```

`crates/app/src/history_service.rs`:
```rust
//! History storage for the app: SQLite rows plus optional 16 kHz WAV files.

#[cfg(test)]
mod tests {
    use opit_core::pipeline::TranscriptStatus;

    use super::*;

    fn entry(created_at_ms: i64) -> NewDictation<'static> {
        NewDictation {
            created_at_ms,
            profile_id: "groq",
            raw_text: "merhaba",
            text: "Merhaba",
            status: TranscriptStatus::Ok,
            audio_ms: 1_000,
            latency_ms: 900,
        }
    }

    fn audio() -> PreparedAudio {
        PreparedAudio { samples: vec![0.1; 16_000], audio_ms: 1_000, speech_ms: 1_000 }
    }

    fn service(dir: &Path) -> HistoryService {
        HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), dir.join("audio"))
    }

    #[test]
    fn records_rows_with_and_without_audio() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let plain = history.record(&entry(1), None).unwrap();
        assert_eq!(history.store().get(plain).unwrap().unwrap().audio_path, None);

        let with_audio = history.record(&entry(2), Some(&audio())).unwrap();
        let path = history.store().get(with_audio).unwrap().unwrap().audio_path.unwrap();
        let wav = hound::WavReader::open(&path).unwrap();
        assert_eq!((wav.spec().sample_rate, wav.spec().channels, wav.len()), (16_000, 1, 16_000));
    }

    #[test]
    fn delete_and_clear_remove_audio_files() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let a = history.record(&entry(1), Some(&audio())).unwrap();
        history.record(&entry(2), Some(&audio())).unwrap();
        let path_a = history.store().get(a).unwrap().unwrap().audio_path.unwrap();
        history.delete(a).unwrap();
        assert!(!Path::new(&path_a).exists());
        history.clear().unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("audio")).unwrap().count(), 0);
        assert!(history.store().recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn expired_audio_is_purged_but_rows_stay() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let now = 100 * DAY_MS;
        let old = history.record(&entry(now - 31 * DAY_MS), Some(&audio())).unwrap();
        let fresh = history.record(&entry(now - DAY_MS), Some(&audio())).unwrap();
        assert_eq!(history.purge_expired_audio(now, 30).unwrap(), 1);
        assert_eq!(history.store().get(old).unwrap().unwrap().audio_path, None);
        assert!(history.store().get(fresh).unwrap().unwrap().audio_path.is_some());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib -- providers history_service`
Expected: FAIL to compile — `cannot find type HttpProviders`, `cannot find type HistoryService`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/providers.rs`:
```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use opit_core::provider::{OpenAiCompatible, Profile};

use crate::controller::{Providers, SetupError};
use crate::platform::SecretStore;

pub struct HttpProviders {
    secrets: Arc<dyn SecretStore>,
    cache: Mutex<HashMap<String, Cached>>,
}

/// Not `Debug`: it holds an API key.
struct Cached {
    profile: Profile,
    key: Option<String>,
    client: Arc<OpenAiCompatible>,
}

impl HttpProviders {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets, cache: Mutex::default() }
    }

    fn key_for(&self, profile: &Profile) -> Result<Option<String>, SetupError> {
        let Some(key_ref) = &profile.api_key_ref else {
            return Ok(None);
        };
        match self.secrets.get(key_ref)? {
            Some(key) if !key.trim().is_empty() => Ok(Some(key)),
            _ => Err(SetupError::MissingKey(profile.name.clone())),
        }
    }
}

impl Providers for HttpProviders {
    type Client = Arc<OpenAiCompatible>;

    fn client(&self, profile: &Profile) -> Result<Self::Client, SetupError> {
        let key = self.key_for(profile)?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cached) = cache.get(&profile.id)
            && cached.profile == *profile
            && cached.key == key
        {
            return Ok(cached.client.clone());
        }
        let client = Arc::new(
            OpenAiCompatible::new(profile.clone(), key.clone()).map_err(|e| SetupError::Client(e.to_string()))?,
        );
        cache.insert(profile.id.clone(), Cached { profile: profile.clone(), key, client: client.clone() });
        Ok(client)
    }

    fn warm_up(&self, client: &Self::Client) {
        let client = client.clone();
        tokio::spawn(async move { client.warm_up().await });
    }
}
```

Insert above the tests in `crates/app/src/history_service.rs`:
```rust
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use opit_core::audio::TARGET_RATE;
use opit_core::audio::encode::{encode_wav, to_pcm16};
use opit_core::history::{AudioStore, HistoryError, HistoryStore, NewDictation};
use opit_core::pipeline::PreparedAudio;
use tracing::warn;

use crate::controller::HistorySink;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub struct HistoryService {
    store: Mutex<HistoryStore>,
    audio: AudioStore,
}

impl HistoryService {
    pub fn open(db: &Path, audio_dir: PathBuf) -> Result<Self, HistoryError> {
        Ok(Self::with_store(HistoryStore::open(db)?, audio_dir))
    }

    pub fn with_store(store: HistoryStore, audio_dir: PathBuf) -> Self {
        Self { store: Mutex::new(store), audio: AudioStore::new(audio_dir) }
    }

    /// Direct access for the read-only history commands.
    pub fn store(&self) -> MutexGuard<'_, HistoryStore> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Deletes a row and its audio file.
    pub fn delete(&self, id: i64) -> Result<(), HistoryError> {
        let path = self.store().delete(id)?;
        remove_files(path.iter());
        Ok(())
    }

    pub fn clear(&self) -> Result<(), HistoryError> {
        let paths = self.store().clear()?;
        remove_files(paths.iter());
        Ok(())
    }

    /// Removes audio files older than `retention_days`; returns how many were removed.
    pub fn purge_expired_audio(&self, now_ms: i64, retention_days: u32) -> Result<usize, HistoryError> {
        let paths = self.store().take_expired_audio(now_ms - i64::from(retention_days) * DAY_MS)?;
        remove_files(paths.iter());
        Ok(paths.len())
    }
}

fn remove_files<'a>(paths: impl Iterator<Item = &'a String>) {
    for path in paths {
        if let Err(err) = AudioStore::remove(Path::new(path)) {
            warn!(error = %err, "could not delete a history audio file");
        }
    }
}

impl HistorySink for HistoryService {
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String> {
        let store = self.store();
        let id = store.insert(entry).map_err(|e| e.to_string())?;
        if let Some(audio) = audio {
            let wav = encode_wav(&to_pcm16(&audio.samples), TARGET_RATE).map_err(|e| e.to_string())?;
            let path = self.audio.save(id, &wav).map_err(|e| e.to_string())?;
            store.set_audio_path(id, &path.to_string_lossy()).map_err(|e| e.to_string())?;
        }
        Ok(id)
    }
}

/// Stand-in when `history.db` cannot be opened: dictation works, nothing is saved.
pub struct DisabledHistory;

impl HistorySink for DisabledHistory {
    fn record(&self, _entry: &NewDictation<'_>, _audio: Option<&PreparedAudio>) -> Result<i64, String> {
        Err("history is unavailable".into())
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib -- providers::tests history_service::tests`
Expected: 7 passed (4 + 3).

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): add cached provider clients and history storage

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: AppCore (logic behind commands and tray) and the tray menu model

**Files:**
- Create: `crates/app/src/app_core.rs`, `crates/app/src/tray_menu.rs`
- Modify: `crates/app/src/lib.rs` (module list)

**Interfaces:**
- Consumes: `controller::{ControllerHandle, DictationStatus, ErrorKind, channel}`, `history_service::HistoryService`, `settings::{Settings, SettingsHandle}`, `startup::{Paths, StartupNotice}`, platform traits and fakes, `i18n::Lang`.
- Produces:
  - `app_core::CommandError { code: &'static str, message: String, kind: Option<ErrorKind> }` (`Serialize`; codes `config`, `rules`, `history`, `secrets`, `provider`, `invalid_input`, `unavailable`; `From` for `ConfigError`, `PackError`, `HistoryError`, `SecretError`, `ProviderError`).
  - `app_core::Platform { secrets, hotkey, mic, overlay, autostart }` (all `Arc<dyn …>`).
  - `app_core::AppCore::new(paths, settings, controller, history: Option<Arc<HistoryService>>, platform, user_pack, system_locale, notices)` with: `config()`, `status()`, `take_notices()`, `startup_overlay() -> Option<OverlayView>`, `save_config(AppConfig) -> Result<AppConfig>` (normalize, save, rebuild rules, re-register the hotkey / move the overlay / update autostart only when those parts changed), `set_active_profile(id)`, `apply_hotkey(&AppConfig)`, `apply_autostart(bool)`, `set_hotkey_paused(bool)`, `hotkey_state() -> HotkeyState { paused, error }`, `has_api_key`, `set_api_key` (trims, rejects blank), `delete_api_key`, `test_connection(Profile, Option<String>)` (async; `None` = stored key), `user_rules_yaml()` (template when missing), `save_user_rules(yaml) -> Vec<RuleWarning>` (validate, atomic write, recompile), `rules_preview(text, draft_yaml) -> RulesPreview { text, hits, warnings, hallucination }`, `prompt_budget() -> BuiltPrompt`, `history_recent(limit ≤ 500, before_id)`, `history_search`, `history_delete`, `history_clear`, `microphones()`.
  - `app_core::USER_PACK_TEMPLATE`.
  - `tray_menu::{TrayItem::{Action, Check, Submenu, Separator}, TrayCommand::{Open, Profile(id), TogglePause, Retry, Quit}, build(&AppConfig, Lang, paused, can_retry) -> Vec<TrayItem>, parse(id) -> Option<TrayCommand>}`; ids: `open`, `profile:<id>`, `pause`, `retry`, `quit`.

The shortcut is effectively paused when the tray pause is on **or** `hotkey.enabled` is false. The tray pause is not persisted.

- [ ] **Step 1: Wire the modules**

Add to the module list in `crates/app/src/lib.rs`:
```rust
pub mod app_core;
pub mod tray_menu;
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/app_core.rs`:
```rust
//! Everything the invoke commands and the tray do, without Tauri types, so it can be
//! tested with the platform fakes. `commands.rs` is a thin wrapper over this.

#[cfg(test)]
mod tests {
    use opit_core::config::{HotkeyMode, OverlayPosition};
    use opit_core::history::{HistoryStore, NewDictation};
    use opit_core::pipeline::TranscriptStatus;

    use super::*;
    use crate::controller::{HistorySink, channel};
    use crate::platform::fake::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        core: AppCore,
        hotkey: Arc<FakeHotkey>,
        overlay: Arc<FakeOverlay>,
        autostart: Arc<FakeAutostart>,
        secrets: Arc<FakeSecrets>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("opit"));
        let settings = SettingsHandle::new(Settings::build(AppConfig::default(), None, Some("en-US")).0);
        let history =
            Arc::new(HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), paths.audio.clone()));
        let (hotkey, overlay) = (Arc::new(FakeHotkey::default()), Arc::new(FakeOverlay::default()));
        let (autostart, secrets) = (Arc::new(FakeAutostart::default()), Arc::new(FakeSecrets::default()));
        let platform = Platform {
            secrets: secrets.clone(),
            hotkey: hotkey.clone(),
            mic: Arc::new(FakeMic::default()),
            overlay: overlay.clone(),
            autostart: autostart.clone(),
        };
        let core = AppCore::new(paths, settings, channel().0, Some(history), platform, None, None, Vec::new());
        Fixture { _dir: dir, core, hotkey, overlay, autostart, secrets }
    }

    #[test]
    fn save_config_normalizes_writes_and_applies_side_effects() {
        let f = fixture();
        let mut config = f.core.config();
        config.recording.max_seconds = 9_999;
        config.hotkey.mode = HotkeyMode::PushToTalk;
        config.hotkey.keys = vec!["F9".into()];
        config.ui.overlay_position = OverlayPosition::TopCenter;
        config.ui.autostart = false;
        *f.autostart.enabled.lock().unwrap() = true;

        let saved = f.core.save_config(config).unwrap();
        assert_eq!(saved.recording.max_seconds, 600);
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
        assert_eq!(f.core.settings.current().config, saved);
        assert_eq!(*f.hotkey.registered.lock().unwrap(), [vec!["F9".to_string()]]);
        assert_eq!(*f.overlay.positions.lock().unwrap(), [OverlayPosition::TopCenter]);
        assert!(!*f.autostart.enabled.lock().unwrap());
    }

    #[test]
    fn unchanged_hotkey_is_not_reinstalled() {
        let f = fixture();
        let mut config = f.core.config();
        config.paste.trailing_space = false;
        f.core.save_config(config).unwrap();
        assert!(f.hotkey.registered.lock().unwrap().is_empty());
    }

    #[test]
    fn hotkey_failures_and_pause_are_reported() {
        let f = fixture();
        *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::UnknownKey("Hyper".into()));
        f.core.apply_hotkey(&f.core.config());
        assert_eq!(f.core.hotkey_state().error.as_deref(), Some("unknown key name: Hyper"));

        f.core.set_hotkey_paused(true);
        assert!(*f.hotkey.paused.lock().unwrap() && f.core.hotkey_state().paused);
        f.core.set_hotkey_paused(false);
        assert!(!*f.hotkey.paused.lock().unwrap());

        let mut config = f.core.config();
        config.hotkey.enabled = false;
        f.core.save_config(config).unwrap();
        assert!(*f.hotkey.paused.lock().unwrap(), "a disabled shortcut stays paused");
    }

    #[test]
    fn tray_profile_switch_is_saved() {
        let f = fixture();
        f.core.set_active_profile("openai").unwrap();
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap().active_profile_id, "openai");
        assert_eq!(f.core.set_active_profile("nope").unwrap_err().code, "invalid_input");
    }

    #[test]
    fn api_keys_are_trimmed_and_blank_ones_rejected() {
        let f = fixture();
        assert!(!f.core.has_api_key("groq").unwrap());
        f.core.set_api_key("groq", "  sk-live\n").unwrap();
        assert_eq!(f.secrets.map.lock().unwrap()["groq"], "sk-live");
        assert!(f.core.has_api_key("groq").unwrap());
        assert_eq!(f.core.set_api_key("groq", "   ").unwrap_err().code, "invalid_input");
        f.core.delete_api_key("groq").unwrap();
        assert!(!f.core.has_api_key("groq").unwrap());
    }

    #[test]
    fn user_rules_round_trip_and_take_effect() {
        let f = fixture();
        assert_eq!(f.core.user_rules_yaml().unwrap(), USER_PACK_TEMPLATE);
        let yaml = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\n";
        let warnings = f.core.save_user_rules(yaml).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(f.core.user_rules_yaml().unwrap(), yaml);
        assert_eq!(f.core.settings.current().rules.apply("opid çalışıyor"), "Opit çalışıyor");
    }

    #[test]
    fn invalid_user_rules_are_rejected_and_nothing_is_written() {
        let f = fixture();
        let err = f.core.save_user_rules("schema: 2\nid: user\nname: Me\n").unwrap_err();
        assert_eq!(err.code, "rules");
        assert!(!f.core.paths.user_rules.exists());
    }

    #[test]
    fn preview_uses_a_draft_without_saving_it() {
        let f = fixture();
        let draft = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\nreplacements:\n  - { from: '(', to: x, regex: true }\n";
        let preview = f.core.rules_preview("opid ve cloud code", Some(draft)).unwrap();
        assert_eq!(preview.text, "Opit ve Claude Code");
        assert_eq!(preview.warnings.len(), 1);
        assert!(preview.hits.len() >= 2);
        assert_eq!(f.core.rules_preview("opid", None).unwrap().text, "opid");
        assert!(f.core.rules_preview("Altyazı M.K.", None).unwrap().hallucination);
    }

    #[test]
    fn prompt_budget_reflects_the_context_sentence() {
        let f = fixture();
        let mut config = f.core.config();
        config.rules.prompt_context = "Yazılım konuşması".into();
        f.core.save_config(config).unwrap();
        let prompt = f.core.prompt_budget();
        assert!(prompt.prompt.unwrap().starts_with("Yazılım konuşması. Geçen terimler:"));
    }

    #[test]
    fn history_commands_work_and_are_capped() {
        let f = fixture();
        let history = f.core.history.clone().unwrap();
        for i in 0..3 {
            let entry = NewDictation {
                created_at_ms: i,
                profile_id: "groq",
                raw_text: "github",
                text: "GitHub",
                status: TranscriptStatus::Ok,
                audio_ms: 1,
                latency_ms: 1,
            };
            history.record(&entry, None).unwrap();
        }
        assert_eq!(f.core.history_recent(2, None).unwrap().len(), 2);
        assert_eq!(f.core.history_search("git", 10).unwrap().len(), 3);
        let first = f.core.history_recent(1, None).unwrap()[0].id;
        f.core.history_delete(first).unwrap();
        assert_eq!(f.core.history_recent(10, None).unwrap().len(), 2);
        f.core.history_clear().unwrap();
        assert!(f.core.history_recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn startup_overlay_picks_the_most_important_notice() {
        let f = fixture();
        assert_eq!(f.core.startup_overlay(), None);
        *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::Install("denied".into()));
        f.core.apply_hotkey(&f.core.config());
        assert_eq!(f.core.startup_overlay().unwrap().text, "Shortcut unavailable — use the tray icon");
        f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
        f.core.notices.lock().unwrap().push(StartupNotice::ConfigReset { backup: None, reason: "y".into() });
        let view = f.core.startup_overlay().unwrap();
        assert_eq!((view.tone, view.text.as_str()), (Tone::Warning, "Settings file was damaged; defaults loaded"));
    }

    #[test]
    fn notices_are_handed_out_once() {
        let f = fixture();
        f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
        assert_eq!(f.core.take_notices().len(), 1);
        assert!(f.core.take_notices().is_empty());
    }

    #[test]
    fn provider_errors_keep_their_kind_for_the_ui() {
        let err = CommandError::from(ProviderError::Unauthorized(401));
        assert_eq!((err.code, err.kind), ("provider", Some(ErrorKind::InvalidKey)));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "invalid_key");
    }
}
```

`crates/app/src/tray_menu.rs`:
```rust
//! Tray menu contents as plain data; `tray.rs` turns this into a Tauri menu.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_matches_the_brief() {
        let items = build(&AppConfig::default(), Lang::Tr, true, false);
        let TrayItem::Submenu { label, items: profiles } = &items[1] else { panic!("{items:?}") };
        assert_eq!(label, "Profil");
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0], TrayItem::Check { id: "profile:groq".into(), label: "Groq".into(), checked: true });
        assert_eq!(items[0], TrayItem::Action { id: "open".into(), label: "Aç".into(), enabled: true });
        assert_eq!(items[2], TrayItem::Check { id: "pause".into(), label: "Kısayolu duraklat".into(), checked: true });
        assert_eq!(items[3], TrayItem::Action { id: "retry".into(), label: "Tekrar dene".into(), enabled: false });
        assert_eq!(items[5], TrayItem::Action { id: "quit".into(), label: "Çıkış".into(), enabled: true });
    }

    #[test]
    fn every_id_parses_back() {
        for item in build(&AppConfig::default(), Lang::En, false, true) {
            match item {
                TrayItem::Action { id, .. } | TrayItem::Check { id, .. } => assert!(parse(&id).is_some(), "{id}"),
                TrayItem::Submenu { items, .. } => {
                    for sub in items {
                        let TrayItem::Check { id, .. } = sub else { panic!() };
                        assert_eq!(parse(&id), Some(TrayCommand::Profile(id["profile:".len()..].to_string())));
                    }
                }
                TrayItem::Separator => {}
            }
        }
        assert_eq!(parse("bogus"), None);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib -- app_core tray_menu`
Expected: FAIL to compile — `cannot find type AppCore`, `cannot find function build`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/app_core.rs`:
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use opit_core::config::{AppConfig, ConfigError};
use opit_core::history::{Dictation, HistoryError};
use opit_core::provider::{OpenAiCompatible, Profile, ProviderError};
use opit_core::rules::prompt::{BuiltPrompt, build_prompt};
use opit_core::rules::{PackError, RuleHit, RulePack, RuleSet, RuleWarning};
use serde::Serialize;
use tracing::{info, warn};

use crate::controller::{ControllerHandle, DictationStatus, ErrorKind};
use crate::history_service::HistoryService;
use crate::platform::{
    Autostart, Hotkey, HotkeyError, Microphone, Overlay, OverlayView, SecretError, SecretStore, Tone,
};
use crate::settings::{Settings, SettingsHandle};
use crate::startup::{Paths, StartupNotice};

/// Error shape every invoke command returns to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    /// Stable machine-readable code: config, rules, history, secrets, provider, invalid_input, unavailable.
    pub code: &'static str,
    pub message: String,
    /// Set for provider errors so the UI can localize them.
    pub kind: Option<ErrorKind>,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), kind: None }
    }
}

impl From<ConfigError> for CommandError {
    fn from(err: ConfigError) -> Self {
        Self::new("config", err.to_string())
    }
}

impl From<PackError> for CommandError {
    fn from(err: PackError) -> Self {
        Self::new("rules", err.to_string())
    }
}

impl From<HistoryError> for CommandError {
    fn from(err: HistoryError) -> Self {
        Self::new("history", err.to_string())
    }
}

impl From<SecretError> for CommandError {
    fn from(err: SecretError) -> Self {
        Self::new("secrets", err.to_string())
    }
}

impl From<ProviderError> for CommandError {
    fn from(err: ProviderError) -> Self {
        Self { code: "provider", message: err.to_string(), kind: Some(ErrorKind::from(&err)) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulesPreview {
    pub text: String,
    pub hits: Vec<RuleHit>,
    pub warnings: Vec<RuleWarning>,
    pub hallucination: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyState {
    pub paused: bool,
    /// Why the global hook is not working, if it is not.
    pub error: Option<String>,
}

/// Starter content for a personal pack that does not exist yet.
pub const USER_PACK_TEMPLATE: &str = "schema: 1\nid: user\nname: Personal\nlanguage: tr\nterms: []\n";

const STARTUP_NOTICE_HIDE: Duration = Duration::from_secs(6);

pub struct Platform {
    pub secrets: Arc<dyn SecretStore>,
    pub hotkey: Arc<dyn Hotkey>,
    pub mic: Arc<dyn Microphone>,
    pub overlay: Arc<dyn Overlay>,
    pub autostart: Arc<dyn Autostart>,
}

pub struct AppCore {
    pub paths: Paths,
    pub settings: SettingsHandle,
    pub controller: ControllerHandle,
    pub history: Option<Arc<HistoryService>>,
    pub platform: Platform,
    user_pack: Mutex<Option<RulePack>>,
    system_locale: Option<String>,
    hotkey_paused: AtomicBool,
    hotkey_error: Mutex<Option<String>>,
    notices: Mutex<Vec<StartupNotice>>,
    /// Serializes read-modify-write of config.json.
    config_lock: Mutex<()>,
}

impl AppCore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        paths: Paths,
        settings: SettingsHandle,
        controller: ControllerHandle,
        history: Option<Arc<HistoryService>>,
        platform: Platform,
        user_pack: Option<RulePack>,
        system_locale: Option<String>,
        notices: Vec<StartupNotice>,
    ) -> Self {
        Self {
            paths,
            settings,
            controller,
            history,
            platform,
            user_pack: Mutex::new(user_pack),
            system_locale,
            hotkey_paused: AtomicBool::new(false),
            hotkey_error: Mutex::default(),
            notices: Mutex::new(notices),
            config_lock: Mutex::default(),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.settings.current().config.clone()
    }

    pub fn status(&self) -> DictationStatus {
        self.controller.status()
    }

    /// Notices from start-up (broken config, broken user.yaml). Returned once.
    pub fn take_notices(&self) -> Vec<StartupNotice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The one message worth flashing on the overlay right after start-up, if any.
    pub fn startup_overlay(&self) -> Option<OverlayView> {
        let lang = self.settings.current().lang;
        let notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        let text = if notices.iter().any(|n| matches!(n, StartupNotice::ConfigReset { .. })) {
            lang.config_reset()
        } else if notices.iter().any(|n| matches!(n, StartupNotice::UserRulesBroken { .. })) {
            lang.user_rules_broken()
        } else if self.hotkey_state().error.is_some() {
            lang.hotkey_failed()
        } else {
            return None;
        };
        Some(OverlayView {
            tone: Tone::Warning,
            text: text.into(),
            level: None,
            button: None,
            hide_after: Some(STARTUP_NOTICE_HIDE),
        })
    }

    fn rebuild(&self, config: AppConfig) -> Vec<RuleWarning> {
        let user = self.user_pack.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let (settings, warnings) = Settings::build(config, user, self.system_locale.as_deref());
        for warning in &warnings {
            warn!(pack = %warning.pack_id, message = %warning.message, "rule skipped");
        }
        self.settings.replace(settings);
        warnings
    }

    /// Validates, saves and applies a config from the UI; returns what was stored.
    pub fn save_config(&self, mut config: AppConfig) -> Result<AppConfig, CommandError> {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        config.normalize();
        let old = self.config();
        config.save(&self.paths.config)?;
        self.rebuild(config.clone());
        if old.hotkey != config.hotkey {
            self.apply_hotkey(&config);
        }
        if old.ui.overlay_position != config.ui.overlay_position {
            self.platform.overlay.set_position(config.ui.overlay_position);
        }
        if old.ui.autostart != config.ui.autostart {
            self.apply_autostart(config.ui.autostart);
        }
        info!("config saved");
        Ok(config)
    }

    pub fn set_active_profile(&self, id: &str) -> Result<(), CommandError> {
        let mut config = self.config();
        if config.profile(id).is_none() {
            return Err(CommandError::new("invalid_input", format!("unknown profile {id}")));
        }
        config.active_profile_id = id.to_string();
        self.save_config(config).map(|_| ())
    }

    /// (Re-)installs the global hook for the configured keys.
    pub fn apply_hotkey(&self, config: &AppConfig) {
        let result = self.platform.hotkey.register(&config.hotkey.keys, self.controller.hotkey_sink());
        let error = result.err().map(|err: HotkeyError| {
            warn!(error = %err, "global shortcut unavailable");
            err.to_string()
        });
        *self.hotkey_error.lock().unwrap_or_else(PoisonError::into_inner) = error;
        self.sync_pause(config);
    }

    pub fn apply_autostart(&self, enabled: bool) {
        if let Err(err) = self.platform.autostart.set_enabled(enabled) {
            warn!(error = %err, "could not update autostart");
        }
    }

    fn sync_pause(&self, config: &AppConfig) {
        let paused = self.hotkey_paused.load(Ordering::SeqCst) || !config.hotkey.enabled;
        self.platform.hotkey.set_paused(paused);
    }

    pub fn set_hotkey_paused(&self, paused: bool) {
        self.hotkey_paused.store(paused, Ordering::SeqCst);
        self.sync_pause(&self.config());
    }

    pub fn hotkey_state(&self) -> HotkeyState {
        HotkeyState {
            paused: self.hotkey_paused.load(Ordering::SeqCst),
            error: self.hotkey_error.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }
    }

    // ----- API keys -----

    pub fn has_api_key(&self, key_ref: &str) -> Result<bool, CommandError> {
        Ok(self.platform.secrets.get(key_ref)?.is_some_and(|k| !k.trim().is_empty()))
    }

    pub fn set_api_key(&self, key_ref: &str, key: &str) -> Result<(), CommandError> {
        let key = key.trim();
        if key_ref.trim().is_empty() || key.is_empty() {
            return Err(CommandError::new("invalid_input", "the key and its name must not be empty"));
        }
        self.platform.secrets.set(key_ref, key)?;
        info!(key_ref, "API key stored");
        Ok(())
    }

    pub fn delete_api_key(&self, key_ref: &str) -> Result<(), CommandError> {
        self.platform.secrets.delete(key_ref)?;
        info!(key_ref, "API key deleted");
        Ok(())
    }

    /// Tests `profile` with `api_key`, or with the stored key when `api_key` is `None`.
    pub async fn test_connection(&self, profile: Profile, api_key: Option<String>) -> Result<(), CommandError> {
        let key = match api_key {
            Some(key) => Some(key),
            None => match &profile.api_key_ref {
                Some(key_ref) => self.platform.secrets.get(key_ref)?,
                None => None,
            },
        };
        let client = OpenAiCompatible::new(profile, key)?;
        Ok(client.test_connection().await?)
    }

    // ----- rules -----

    pub fn user_rules_yaml(&self) -> Result<String, CommandError> {
        match std::fs::read_to_string(&self.paths.user_rules) {
            Ok(yaml) => Ok(yaml),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(USER_PACK_TEMPLATE.to_string()),
            Err(err) => Err(CommandError::new("rules", err.to_string())),
        }
    }

    /// Validates and stores `user.yaml`, then recompiles the rules; returns skipped-rule warnings.
    pub fn save_user_rules(&self, yaml: &str) -> Result<Vec<RuleWarning>, CommandError> {
        let pack = RulePack::from_yaml(yaml)?;
        if let Some(dir) = self.paths.user_rules.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CommandError::new("rules", e.to_string()))?;
        }
        let tmp = self.paths.user_rules.with_extension("yaml.tmp");
        std::fs::write(&tmp, yaml)
            .and_then(|()| std::fs::rename(&tmp, &self.paths.user_rules))
            .map_err(|e| CommandError::new("rules", e.to_string()))?;
        *self.user_pack.lock().unwrap_or_else(PoisonError::into_inner) = Some(pack);
        info!("user rules saved");
        Ok(self.rebuild(self.config()))
    }

    /// Runs `text` through the rules, optionally with an unsaved `user.yaml` draft.
    pub fn rules_preview(&self, text: &str, draft_yaml: Option<&str>) -> Result<RulesPreview, CommandError> {
        let current = self.settings.current();
        let (rules, warnings) = match draft_yaml {
            None => (current.rules.clone(), Vec::new()),
            Some(yaml) => {
                let draft = RulePack::from_yaml(yaml)?;
                let packs = opit_core::rules::builtin::assemble(Some(draft), &current.config.rules.enabled_packs);
                let (rules, warnings) = RuleSet::compile(&packs);
                (Arc::new(rules), warnings)
            }
        };
        let hallucination = rules.is_hallucination(text.trim());
        let (text, hits) = rules.apply_traced(text.trim());
        Ok(RulesPreview { text, hits, warnings, hallucination })
    }

    /// The Whisper prompt the active profile would send right now.
    pub fn prompt_budget(&self) -> BuiltPrompt {
        let current = self.settings.current();
        let language = current.config.active_profile().map_or("tr", |p| p.language.as_str());
        build_prompt(&current.config.rules.prompt_context, current.rules.terms(), language)
    }

    // ----- history -----

    fn history(&self) -> Result<&HistoryService, CommandError> {
        self.history.as_deref().ok_or_else(|| CommandError::new("unavailable", "history is not available"))
    }

    pub fn history_recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().recent(limit.min(500), before_id)?)
    }

    pub fn history_search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().search(query, limit.min(500))?)
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CommandError> {
        Ok(self.history()?.delete(id)?)
    }

    pub fn history_clear(&self) -> Result<(), CommandError> {
        Ok(self.history()?.clear()?)
    }

    pub fn microphones(&self) -> Vec<String> {
        self.platform.mic.devices()
    }
}
```

Insert above the tests in `crates/app/src/tray_menu.rs`:
```rust
use opit_core::config::AppConfig;

use crate::i18n::Lang;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayItem {
    Action { id: String, label: String, enabled: bool },
    Check { id: String, label: String, checked: bool },
    Submenu { label: String, items: Vec<TrayItem> },
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    Profile(String),
    TogglePause,
    Retry,
    Quit,
}

const PROFILE_PREFIX: &str = "profile:";

pub fn build(config: &AppConfig, lang: Lang, paused: bool, can_retry: bool) -> Vec<TrayItem> {
    let action =
        |id: &str, label: &str, enabled: bool| TrayItem::Action { id: id.into(), label: label.into(), enabled };
    let profiles = config
        .profiles
        .iter()
        .map(|p| TrayItem::Check {
            id: format!("{PROFILE_PREFIX}{}", p.id),
            label: p.name.clone(),
            checked: p.id == config.active_profile_id,
        })
        .collect();
    vec![
        action("open", lang.tray_open(), true),
        TrayItem::Submenu { label: lang.tray_profile().into(), items: profiles },
        TrayItem::Check { id: "pause".into(), label: lang.tray_pause_hotkey().into(), checked: paused },
        action("retry", lang.retry(), can_retry),
        TrayItem::Separator,
        action("quit", lang.tray_quit(), true),
    ]
}

pub fn parse(id: &str) -> Option<TrayCommand> {
    match id {
        "open" => Some(TrayCommand::Open),
        "pause" => Some(TrayCommand::TogglePause),
        "retry" => Some(TrayCommand::Retry),
        "quit" => Some(TrayCommand::Quit),
        _ => id.strip_prefix(PROFILE_PREFIX).map(|p| TrayCommand::Profile(p.to_string())),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib -- app_core::tests tray_menu::tests`
Expected: 15 passed (13 + 2).

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src
git commit -m "feat(app): add AppCore command logic and the tray menu model

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Windows — credential store, autostart and sounds

**Files:**
- Create: `crates/app/src/platform/windows/mod.rs`, `crates/app/src/platform/windows/secrets.rs`, `crates/app/src/platform/windows/autostart.rs`, `crates/app/src/platform/windows/sounds.rs`
- Modify: `crates/app/src/platform/mod.rs` (declare `windows`)

**Interfaces:**
- Consumes: `platform::{SecretStore, SecretError, Autostart, Sounds, Sound}`, `opit_core::audio::encode::encode_wav`.
- Produces:
  - `KeyringStore::new() -> Result<KeyringStore, SecretError>` (service `opit-speech-to-text`; Credential Manager target `<key_ref>.opit-speech-to-text`, the same entry `cmdkey /generic:<key_ref>.opit-speech-to-text` creates), `KeyringStore::with_service(&str)`.
  - `RegistryAutostart::new(key_path, value_name, command)`, `RegistryAutostart::for_current_exe()` (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Opit Speech to Text`, data `"<exe>" --autostart`), `command()`; constants `RUN_KEY`, `VALUE_NAME`, `AUTOSTART_FLAG`.
  - `WinSounds::new()`, `warm_up()` (builds the cached WAVs) — tones are synthesized in code (no asset files) and played with `PlaySoundW(SND_MEMORY | SND_ASYNC | SND_NODEFAULT)` from `'static` buffers.

Notes: keyring v4 now points apps to `keyring-core` + a store crate, which is what this uses; its v3 feature flags (`windows-native`) no longer exist. `windows-registry` 0.100 needs rustc 1.95, so 0.6 is used. A missing registry key or value both surface as `0x80070002` and read as "disabled".

- [ ] **Step 1: Wire the module**

In `crates/app/src/platform/mod.rs`, below `pub mod keys;`, add:
```rust
#[cfg(windows)]
pub mod windows;
```

`crates/app/src/platform/windows/mod.rs`:
```rust
//! Win32 implementations of the platform traits.

pub mod autostart;
pub mod secrets;
pub mod sounds;

pub use autostart::RegistryAutostart;
pub use secrets::KeyringStore;
pub use sounds::WinSounds;
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/platform/windows/secrets.rs`:
```rust
//! API keys in the Windows Credential Manager (keyring-core + windows-native-keyring-store).
//!
//! Each secret is a generic credential with target name `<key_ref>.<service>`
//! (the store's default, keyring-v1-compatible naming).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes to the real Credential Manager"]
    fn round_trip_with_throwaway_key() {
        let store = KeyringStore::new().expect("credential store");
        let key_ref = format!("opit-test-{}", std::process::id());
        store.delete(&key_ref).unwrap();
        assert_eq!(store.get(&key_ref).unwrap(), None);

        store.set(&key_ref, "sk-test-ğüşiöç-123").unwrap();
        assert_eq!(store.get(&key_ref).unwrap().as_deref(), Some("sk-test-ğüşiöç-123"));
        store.set(&key_ref, "sk-overwritten").unwrap();
        assert_eq!(store.get(&key_ref).unwrap().as_deref(), Some("sk-overwritten"));

        store.delete(&key_ref).unwrap();
        assert_eq!(store.get(&key_ref).unwrap(), None);
        store.delete(&key_ref).expect("deleting a missing entry is ok");
    }
}
```

`crates/app/src/platform/windows/autostart.rs`:
```rust
//! Start-with-Windows via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\opit-speech-to-text-test";

    #[test]
    fn current_exe_command_is_quoted_with_flag() {
        let autostart = RegistryAutostart::for_current_exe().unwrap();
        assert!(autostart.command().starts_with('"'));
        assert!(autostart.command().ends_with("\" --autostart"));
    }

    #[test]
    fn enable_disable_round_trip() {
        let _ = CURRENT_USER.remove_tree(TEST_KEY);
        let command = r#""C:\Program Files\Opit\opit.exe" --autostart"#;
        let autostart = RegistryAutostart::new(TEST_KEY, VALUE_NAME, command);

        assert!(!autostart.is_enabled().unwrap(), "missing key reads as disabled");
        autostart.set_enabled(false).expect("disabling with no key is ok");

        autostart.set_enabled(true).unwrap();
        assert!(autostart.is_enabled().unwrap());
        assert_eq!(CURRENT_USER.open(TEST_KEY).unwrap().get_string(VALUE_NAME).unwrap(), command);

        let moved = RegistryAutostart::new(TEST_KEY, VALUE_NAME, r#""D:\opit.exe" --autostart"#);
        moved.set_enabled(true).expect("enabling again rewrites the command");
        assert_eq!(CURRENT_USER.open(TEST_KEY).unwrap().get_string(VALUE_NAME).unwrap(), moved.command());

        autostart.set_enabled(false).unwrap();
        assert!(!autostart.is_enabled().unwrap());
        autostart.set_enabled(false).expect("disabling twice is ok");

        CURRENT_USER.remove_tree(TEST_KEY).unwrap();
        assert!(CURRENT_USER.open(TEST_KEY).is_err());
    }
}
```

`crates/app/src/platform/windows/sounds.rs`:
```rust
//! UI sounds synthesized in code and played with `PlaySoundW` (no asset files).

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(pcm: &[i16]) -> i16 {
        pcm.iter().map(|s| s.saturating_abs()).max().unwrap_or(0)
    }

    #[test]
    fn tone_has_expected_length_peak_and_fades() {
        let pcm = tone(660.0, 70);
        assert_eq!(pcm.len(), 3087);
        let max = (AMPLITUDE * f32::from(i16::MAX)) as i16;
        assert!(peak(&pcm) <= max + 1 && peak(&pcm) > max / 10 * 9, "peak {}", peak(&pcm));
        assert_eq!(pcm[0], 0, "starts silent");
        assert!(pcm[pcm.len() - 1].abs() < 50, "ends near silence");
        assert!(peak(&pcm[..20]) < max / 2, "fade-in");
    }

    #[test]
    fn rest_is_silent() {
        assert!(tone(0.0, 40).iter().all(|&s| s == 0));
        assert_eq!(tone(0.0, 40).len(), 1764);
    }

    #[test]
    fn every_sound_is_a_short_wav() {
        for (i, wav) in wavs().iter().enumerate() {
            assert_eq!(&wav[..4], b"RIFF", "sound {i}");
            assert_eq!(&wav[8..12], b"WAVE", "sound {i}");
            let ms = (wav.len() - 44) as u32 / 2 * 1000 / RATE;
            assert!((100..=300).contains(&ms), "sound {i} is {ms} ms");
        }
    }

    /// Plays each sound. Run with `--ignored` (audible).
    #[test]
    #[ignore = "plays audio"]
    fn smoke_play_all() {
        let sounds = WinSounds::new();
        sounds.warm_up();
        for sound in [Sound::Start, Sound::Done, Sound::Cancel, Sound::Error] {
            println!("playing {sound:?}");
            sounds.play(sound);
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib platform::windows`
Expected: FAIL to compile — `cannot find type KeyringStore`, `cannot find type RegistryAutostart`, `cannot find function tone`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/platform/windows/secrets.rs`:
```rust
use std::sync::Arc;

use keyring_core::Error as KeyringError;
use keyring_core::api::CredentialStoreApi;
use windows_native_keyring_store::Store;

use crate::platform::{SecretError, SecretStore};

/// Service name used for every credential this app writes.
pub const SERVICE: &str = "opit-speech-to-text";

/// [`SecretStore`] backed by the Windows Credential Manager.
#[derive(Debug)]
pub struct KeyringStore {
    store: Arc<Store>,
    service: String,
}

impl KeyringStore {
    /// Opens the credential store with the app's service name.
    pub fn new() -> Result<Self, SecretError> {
        Self::with_service(SERVICE)
    }

    /// Opens the credential store with a custom service name (tests).
    pub fn with_service(service: &str) -> Result<Self, SecretError> {
        let store = Store::new().map_err(to_secret_error)?;
        Ok(Self { store, service: service.to_owned() })
    }

    fn entry(&self, key_ref: &str) -> Result<keyring_core::Entry, SecretError> {
        self.store.build(&self.service, key_ref, None).map_err(to_secret_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        match self.entry(key_ref)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(to_secret_error(e)),
        }
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(key_ref)?.set_password(secret).map_err(to_secret_error)
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        match self.entry(key_ref)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(to_secret_error(e)),
        }
    }
}

/// Uses `Display`, which never includes secret bytes (unlike `Debug` of `BadEncoding`).
fn to_secret_error(e: KeyringError) -> SecretError {
    SecretError(e.to_string())
}
```

Insert above the tests in `crates/app/src/platform/windows/autostart.rs`:
```rust
use windows_registry::CURRENT_USER;

use crate::platform::Autostart;

/// Per-user Run key.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Value name under the Run key.
pub const VALUE_NAME: &str = "Opit Speech to Text";
/// Command-line flag passed when Windows launches the app at sign-in.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: the key or value does not exist.
const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;

/// [`Autostart`] backed by a string value under an HKCU key.
#[derive(Debug, Clone)]
pub struct RegistryAutostart {
    key_path: String,
    value_name: String,
    command: String,
}

impl RegistryAutostart {
    /// Key path is relative to HKCU.
    pub fn new(key_path: impl Into<String>, value_name: impl Into<String>, command: impl Into<String>) -> Self {
        Self { key_path: key_path.into(), value_name: value_name.into(), command: command.into() }
    }

    /// The real Run entry: `"<current exe>" --autostart`.
    pub fn for_current_exe() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("could not locate the executable: {e}"))?;
        Ok(Self::new(RUN_KEY, VALUE_NAME, format!("\"{}\" {AUTOSTART_FLAG}", exe.display())))
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

impl Autostart for RegistryAutostart {
    /// True when the value exists, even if it points at an old exe path.
    fn is_enabled(&self) -> Result<bool, String> {
        let key = match CURRENT_USER.open(&self.key_path) {
            Ok(key) => key,
            Err(e) if e.code().0 == NOT_FOUND => return Ok(false),
            Err(e) => return Err(format!("could not read the startup setting: {e}")),
        };
        match key.get_type(&self.value_name) {
            Ok(_) => Ok(true),
            Err(e) if e.code().0 == NOT_FOUND => Ok(false),
            Err(e) => Err(format!("could not read the startup setting: {e}")),
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            let key = CURRENT_USER
                .create(&self.key_path)
                .map_err(|e| format!("could not open the startup registry key: {e}"))?;
            return key
                .set_string(&self.value_name, &self.command)
                .map_err(|e| format!("could not enable start with Windows: {e}"));
        }
        let key = match CURRENT_USER.options().read().write().open(&self.key_path) {
            Ok(key) => key,
            Err(e) if e.code().0 == NOT_FOUND => return Ok(()),
            Err(e) => return Err(format!("could not open the startup registry key: {e}")),
        };
        match key.remove_value(&self.value_name) {
            Ok(()) => Ok(()),
            Err(e) if e.code().0 == NOT_FOUND => Ok(()),
            Err(e) => Err(format!("could not disable start with Windows: {e}")),
        }
    }
}
```

Insert above the tests in `crates/app/src/platform/windows/sounds.rs`:
```rust
use std::f32::consts::TAU;
use std::sync::OnceLock;

use opit_core::audio::encode::encode_wav;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

use crate::platform::{Sound, Sounds};

const RATE: u32 = 44_100;
const AMPLITUDE: f32 = 0.25;
const FADE_MS: u32 = 5;

/// One note: frequency in Hz (0 = silence) and length in ms.
type Note = (f32, u32);

const START: &[Note] = &[(660.0, 70), (880.0, 70)];
const DONE: &[Note] = &[(784.0, 70), (1175.0, 110)];
const CANCEL: &[Note] = &[(440.0, 110)];
const ERROR: &[Note] = &[(330.0, 90), (0.0, 40), (262.0, 130)];

/// WAV files for every [`Sound`], built once. `SND_MEMORY | SND_ASYNC` reads the buffer while
/// playing, so it must live for the whole process: `'static` storage guarantees that.
static WAVS: OnceLock<[Vec<u8>; 4]> = OnceLock::new();

/// [`Sounds`] via the Win32 `PlaySoundW` API.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinSounds;

impl WinSounds {
    pub fn new() -> Self {
        Self
    }

    /// Builds the WAV cache ahead of the first `play`.
    pub fn warm_up(&self) {
        wavs();
    }
}

impl Sounds for WinSounds {
    fn play(&self, sound: Sound) {
        let wav = &wavs()[index(sound)];
        if wav.is_empty() {
            return;
        }
        // SAFETY: `wav` is a complete RIFF/WAVE image in 'static memory, as SND_MEMORY requires.
        // With SND_ASYNC the call returns immediately; a later call replaces the playing sound.
        let ok = unsafe { PlaySoundW(PCWSTR(wav.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
        if !ok.as_bool() {
            tracing::debug!(?sound, "PlaySoundW failed");
        }
    }
}

fn index(sound: Sound) -> usize {
    match sound {
        Sound::Start => 0,
        Sound::Done => 1,
        Sound::Cancel => 2,
        Sound::Error => 3,
    }
}

fn wavs() -> &'static [Vec<u8>; 4] {
    WAVS.get_or_init(|| {
        [START, DONE, CANCEL, ERROR].map(|notes| {
            encode_wav(&melody(notes), RATE).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "could not build a UI sound");
                Vec::new()
            })
        })
    })
}

fn melody(notes: &[Note]) -> Vec<i16> {
    notes.iter().flat_map(|&(freq, ms)| tone(freq, ms)).collect()
}

/// A sine note with linear fade in/out, mono 16-bit at [`RATE`].
fn tone(freq: f32, ms: u32) -> Vec<i16> {
    let len = (RATE * ms / 1000) as usize;
    let fade = ((RATE * FADE_MS / 1000) as usize).min(len / 2).max(1);
    (0..len)
        .map(|i| {
            if freq <= 0.0 {
                return 0;
            }
            let envelope = (i.min(len - 1 - i) as f32 / fade as f32).min(1.0);
            let sample = (TAU * freq * i as f32 / RATE as f32).sin() * AMPLITUDE * envelope;
            (sample * f32::from(i16::MAX)).round() as i16
        })
        .collect()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib platform::windows`
Expected: 5 passed, 2 ignored (autostart writes and removes `HKCU\Software\opit-speech-to-text-test`).

Run the hardware/OS smoke tests once by hand:
`cargo test -p opit-speech-to-text --lib -- --ignored --nocapture platform::windows::secrets platform::windows::sounds`
Expected: 2 passed; you hear four short sounds (start, done, cancel, error); the throwaway credential `opit-test-<pid>` is created and deleted.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/platform
git commit -m "feat(app): add Credential Manager, autostart and sound backends

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Windows — microphone capture (cpal)

**Files:**
- Create: `crates/app/src/platform/windows/microphone.rs`
- Modify: `crates/app/src/platform/windows/mod.rs`

**Interfaces:**
- Consumes: `platform::{Microphone, Capture, CaptureEvent, CaptureSink, MicError, Started}`, `opit_core::audio::Recording`.
- Produces: `CpalMicrophone::new()` implementing `Microphone`; constants `START_TIMEOUT` (3 s), `LEVEL_INTERVAL` (50 ms).

How it works: each capture owns a dedicated `opit-mic` thread that opens the device, builds and plays the stream, and parks on a stop channel; `start` waits for "opened" or an error, so open failures are synchronous. The callback downmixes every frame to mono into a shared buffer (the `Recording` it returns has `channels: 1` at the device's native rate) and emits `Level` at most every 50 ms. Only hard stream errors send `Failed` (once); xruns, `DeviceChanged` and `RealtimeDenied` do not end a recording. A named device that is missing or fails to open falls back to the default device (`fell_back = true`). The default device is resolved to its concrete endpoint, so changing the Windows default mid-recording does not invalidate the stream.

cpal 0.18 notes: `Device::name()` is gone — names come from `device.description()?.name()` (the localized friendly name, which is what `recording.microphone` in `config.json` holds); `device.id()` is the stable id; there is a single `cpal::Error` with `.kind()`; `build_input_stream` takes the config by value; streams start paused, so `play()` is required; the default config may now be I24/I32, so the conversion is generic over every sample format.

- [ ] **Step 1: Wire the module**

In `crates/app/src/platform/windows/mod.rs` add `pub mod microphone;` to the module list and `pub use microphone::CpalMicrophone;` to the re-exports (keep both lists alphabetical).

- [ ] **Step 2: Write the failing tests**

`crates/app/src/platform/windows/microphone.rs`:
```rust
//! Microphone capture with cpal (WASAPI on Windows).
//!
//! `cpal::Stream` is not `Send` on every backend, so each capture owns a dedicated thread that
//! opens the device, builds and plays the stream, then parks on a stop channel. Everything cpal
//! touches stays on that thread; the caller only sees channels and a shared sample buffer.

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn order_puts_default_first_and_dedupes() {
        let out = order_device_names(Some("B".into()), names(&["A", "B", "C", "A"]));
        assert_eq!(out, names(&["B", "A", "C"]));
        assert_eq!(order_device_names(None, names(&["A", "A"])), names(&["A"]));
        assert_eq!(order_device_names(Some("X".into()), vec![]), names(&["X"]));
    }

    #[test]
    fn plan_uses_named_device_then_default() {
        let available = names(&["Mic A", "Mic B"]);
        let (plan, fell_back) = plan_attempts(Some("Mic B"), &available);
        assert_eq!(plan, vec![Attempt::Named("Mic B".into()), Attempt::Default]);
        assert!(!fell_back);
    }

    #[test]
    fn plan_falls_back_when_named_device_is_missing() {
        let (plan, fell_back) = plan_attempts(Some("Gone"), &names(&["Mic A"]));
        assert_eq!(plan, vec![Attempt::Default]);
        assert!(fell_back);
    }

    #[test]
    fn plan_without_request_uses_default() {
        let (plan, fell_back) = plan_attempts(None, &[]);
        assert_eq!(plan, vec![Attempt::Default]);
        assert!(!fell_back);
    }

    #[test]
    fn meter_throttles_to_interval() {
        let t0 = Instant::now();
        let mut meter = LevelMeter::new(Duration::from_millis(50));
        assert_eq!(meter.push(&[0.5; 4], t0), Some(0.5));
        assert_eq!(meter.push(&[1.0; 4], t0 + Duration::from_millis(10)), None);
        assert_eq!(meter.push(&[1.0; 4], t0 + Duration::from_millis(49)), None);
        // Everything since the last emit counts: 8 samples of 1.0 and 8 of 0.0.
        let level = meter.push(&[0.0; 8], t0 + Duration::from_millis(50)).unwrap();
        assert!((level - 0.5_f32.sqrt()).abs() < 1e-6, "{level}");
    }

    #[test]
    fn meter_sanitizes_and_skips_empty_windows() {
        let t0 = Instant::now();
        let mut meter = LevelMeter::new(Duration::from_millis(50));
        assert_eq!(meter.push(&[], t0), None);
        assert_eq!(meter.push(&[f32::NAN, 4.0], t0), Some(0.5_f32.sqrt()));
    }

    #[test]
    fn frames_are_downmixed_to_mono() {
        let mut buf = vec![0.5];
        append_mono(&mut buf, &[0.2_f32, 0.4, -1.0, 1.0], 2);
        assert_eq!(buf.len(), 3);
        assert!((buf[1] - 0.3).abs() < 1e-6 && buf[2] == 0.0, "{buf:?}");
        let mut ints = Vec::new();
        append_mono(&mut ints, &[i16::MAX, 0, i16::MIN], 1);
        assert!((ints[0] - 1.0).abs() < 1e-3 && ints[1] == 0.0 && ints[2] == -1.0, "{ints:?}");
    }

    #[test]
    fn only_hard_errors_are_fatal() {
        assert!(!is_fatal(ErrorKind::Xrun));
        assert!(!is_fatal(ErrorKind::DeviceChanged));
        assert!(!is_fatal(ErrorKind::RealtimeDenied));
        assert!(is_fatal(ErrorKind::DeviceNotAvailable));
        assert!(is_fatal(ErrorKind::StreamInvalidated));
    }

    /// Records one second from the default microphone. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "needs a real microphone"]
    fn smoke_record_one_second() {
        let mic = CpalMicrophone::new();
        let devices = mic.devices();
        println!("devices: {devices:?}");
        let peak = Arc::new(Mutex::new(0.0_f32));
        let events = Arc::new(Mutex::new(0_usize));
        let (p, n) = (Arc::clone(&peak), Arc::clone(&events));
        let sink: CaptureSink = Arc::new(move |event| {
            if let CaptureEvent::Level(level) = event {
                let mut peak = p.lock().unwrap();
                *peak = peak.max(level);
                *n.lock().unwrap() += 1;
            }
        });
        let started = mic.start(None, Arc::clone(&sink)).expect("default mic");
        println!("opened {:?} fell_back={}", started.device, started.fell_back);
        std::thread::sleep(Duration::from_secs(1));
        let rec = started.capture.finish();
        println!(
            "sample_rate={} channels={} len={} duration_ms={} level_events={} peak_level={:.4}",
            rec.sample_rate,
            rec.channels,
            rec.samples.len(),
            rec.duration_ms(),
            events.lock().unwrap(),
            peak.lock().unwrap()
        );
        assert!(rec.duration_ms() > 700, "captured too little audio");

        for name in &devices {
            let t = Instant::now();
            let started = mic.start(Some(name), Arc::clone(&sink)).expect("named mic");
            println!("named {name:?} -> {:?} fell_back={} in {:?}", started.device, started.fell_back, t.elapsed());
            assert_eq!((&started.device, started.fell_back), (name, false));
            drop(started.capture); // cancel
        }

        let missing = mic.start(Some("No Such Microphone 123"), sink).expect("fallback");
        assert!(missing.fell_back);
        println!("fallback opened {:?}", missing.device);
        drop(missing.capture); // cancel
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::microphone`
Expected: FAIL to compile — `cannot find function order_device_names`, `cannot find type LevelMeter`, `cannot find function append_mono`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/platform/windows/microphone.rs`:
```rust
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Host, SampleFormat, SizedSample, Stream, StreamConfig};
use opit_core::audio::Recording;

use crate::platform::{Capture, CaptureEvent, CaptureSink, MicError, Microphone, Started};

/// How long `start` waits for the capture thread to open the device.
pub const START_TIMEOUT: Duration = Duration::from_secs(3);
/// Minimum gap between two `CaptureEvent::Level` events.
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(50);

/// cpal-backed [`Microphone`].
#[derive(Debug, Clone)]
pub struct CpalMicrophone {
    start_timeout: Duration,
}

impl Default for CpalMicrophone {
    fn default() -> Self {
        Self { start_timeout: START_TIMEOUT }
    }
}

impl CpalMicrophone {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Microphone for CpalMicrophone {
    fn devices(&self) -> Vec<String> {
        let host = cpal::default_host();
        let default = host.default_input_device().as_ref().and_then(device_name);
        let all = match host.input_devices() {
            Ok(devices) => devices.filter_map(|d| device_name(&d)).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "listing input devices failed");
                Vec::new()
            }
        };
        order_device_names(default, all)
    }

    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError> {
        let requested = device.map(str::to_owned);
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Opened, MicError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let thread_buffer = Arc::clone(&buffer);
        let thread = thread::Builder::new()
            .name("opit-mic".into())
            .spawn(move || capture_thread(requested, sink, thread_buffer, ready_tx, stop_rx))
            .map_err(|e| MicError::Open(format!("could not start the capture thread: {e}")))?;

        let opened = match ready_rx.recv_timeout(self.start_timeout) {
            Ok(Ok(opened)) => opened,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(RecvTimeoutError::Timeout) => {
                // The thread exits on its own once `stop_tx` is dropped.
                drop(stop_tx);
                return Err(MicError::Open("the microphone did not start in time".into()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                return Err(MicError::Open("the capture thread stopped unexpectedly".into()));
            }
        };

        if opened.fell_back {
            tracing::warn!(device = %opened.device, "requested microphone unavailable; using the default");
        }
        tracing::info!(
            device = %opened.device,
            sample_rate = opened.sample_rate,
            channels = opened.channels,
            "microphone started"
        );
        let capture = CpalCapture {
            stop: Some(stop_tx),
            thread: Some(thread),
            buffer,
            sample_rate: opened.sample_rate,
            channels: opened.channels,
        };
        Ok(Started { capture: Box::new(capture), device: opened.device, fell_back: opened.fell_back })
    }
}

/// A running capture. Dropping it stops the stream and discards the audio.
struct CpalCapture {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

impl CpalCapture {
    fn stop_and_join(&mut self) {
        // Dropping the sender wakes the capture thread, which drops the stream.
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Capture for CpalCapture {
    fn finish(mut self: Box<Self>) -> Recording {
        self.stop_and_join();
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap_or_else(PoisonError::into_inner));
        Recording { samples, sample_rate: self.sample_rate, channels: self.channels }
    }
}

impl Drop for CpalCapture {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// What the capture thread reports once the stream is playing.
struct Opened {
    device: String,
    fell_back: bool,
    sample_rate: u32,
    channels: u16,
}

fn capture_thread(
    requested: Option<String>,
    sink: CaptureSink,
    buffer: Arc<Mutex<Vec<f32>>>,
    ready: Sender<Result<Opened, MicError>>,
    stop: Receiver<()>,
) {
    let host = cpal::default_host();
    match open(&host, requested.as_deref(), &sink, &buffer) {
        Ok((stream, opened)) => {
            if ready.send(Ok(opened)).is_err() {
                return; // `start` already gave up; drop the stream.
            }
            // Blocks until `stop` is signalled or its sender is dropped.
            let _ = stop.recv();
            drop(stream);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
}

/// Which device to try, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Attempt {
    Named(String),
    Default,
}

/// Plans the open attempts: the named device when it exists (then the default as a fallback),
/// otherwise just the default. The bool is true when the plan already implies a fallback.
fn plan_attempts(requested: Option<&str>, available: &[String]) -> (Vec<Attempt>, bool) {
    match requested {
        Some(name) if available.iter().any(|d| d == name) => {
            (vec![Attempt::Named(name.to_owned()), Attempt::Default], false)
        }
        Some(_) => (vec![Attempt::Default], true),
        None => (vec![Attempt::Default], false),
    }
}

fn open(
    host: &Host,
    requested: Option<&str>,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<(Stream, Opened), MicError> {
    let inputs: Vec<(String, Device)> = match (requested, host.input_devices()) {
        (Some(_), Ok(devices)) => devices.filter_map(|d| device_name(&d).map(|n| (n, d))).collect(),
        _ => Vec::new(),
    };
    let names: Vec<String> = inputs.iter().map(|(n, _)| n.clone()).collect();
    let (attempts, mut fell_back) = plan_attempts(requested, &names);

    let mut last_error = MicError::NoDevice;
    for attempt in attempts {
        let device = match &attempt {
            Attempt::Named(name) => inputs.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone()),
            Attempt::Default => {
                fell_back |= requested.is_some();
                default_device(host)
            }
        };
        let Some(device) = device else { continue };
        let name = device_name(&device).unwrap_or_else(|| "Unknown microphone".into());
        match build_stream(&device, sink, buffer) {
            Ok((stream, config)) => {
                // Frames are downmixed in the callback, so the buffer is always mono.
                let opened = Opened { device: name, fell_back, sample_rate: config.sample_rate, channels: 1 };
                return Ok((stream, opened));
            }
            Err(e) => {
                tracing::warn!(device = %name, error = %e, "opening microphone failed");
                last_error = MicError::Open(e);
            }
        }
    }
    Err(last_error)
}

/// The system default input device, resolved to the concrete endpoint so the stream stays on
/// that device (WASAPI's "follow the default" streams report `StreamInvalidated` when the default
/// changes mid-recording).
fn default_device(host: &Host) -> Option<Device> {
    let default = host.default_input_device()?;
    default.id().ok().and_then(|id| host.device_by_id(&id)).or(Some(default))
}

fn device_name(device: &Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_owned()).filter(|n| !n.is_empty())
}

/// Default first, then the rest in enumeration order, without duplicates.
fn order_device_names(default: Option<String>, all: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(all.len() + 1);
    for name in default.into_iter().chain(all) {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn build_stream(
    device: &Device,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<(Stream, StreamConfig), String> {
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_typed::<f32>(device, config, sink, buffer),
        SampleFormat::F64 => build_typed::<f64>(device, config, sink, buffer),
        SampleFormat::I8 => build_typed::<i8>(device, config, sink, buffer),
        SampleFormat::I16 => build_typed::<i16>(device, config, sink, buffer),
        SampleFormat::I24 => build_typed::<cpal::I24>(device, config, sink, buffer),
        SampleFormat::I32 => build_typed::<i32>(device, config, sink, buffer),
        SampleFormat::U8 => build_typed::<u8>(device, config, sink, buffer),
        SampleFormat::U16 => build_typed::<u16>(device, config, sink, buffer),
        SampleFormat::U32 => build_typed::<u32>(device, config, sink, buffer),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, config))
}

fn build_typed<T>(
    device: &Device,
    config: StreamConfig,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let data_sink = Arc::clone(sink);
    let error_sink = Arc::clone(sink);
    let buffer = Arc::clone(buffer);
    let mut meter = LevelMeter::new(LEVEL_INTERVAL);
    let mut failed = false;
    let channels = usize::from(config.channels.max(1));

    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let level = {
                let mut buf = buffer.lock().unwrap_or_else(PoisonError::into_inner);
                let start = buf.len();
                append_mono(&mut buf, data, channels);
                meter.push(&buf[start..], Instant::now())
            };
            if let Some(level) = level {
                data_sink(CaptureEvent::Level(level));
            }
        },
        move |err: cpal::Error| {
            if is_fatal(err.kind()) && !failed {
                failed = true;
                tracing::error!(error = %err, "microphone stream failed");
                error_sink(CaptureEvent::Failed(err.to_string()));
            } else {
                tracing::debug!(error = %err, "microphone stream warning");
            }
        },
        None,
    )
}

/// Appends interleaved `data` as mono (channel average). Mono in memory halves the RAM of a
/// long stereo recording: 600 s at 48 kHz is 115 MB instead of 230 MB.
fn append_mono<T>(buf: &mut Vec<f32>, data: &[T], channels: usize)
where
    T: SizedSample,
    f32: FromSample<T>,
{
    buf.extend(
        data.chunks_exact(channels)
            .map(|frame| frame.iter().map(|&s| s.to_sample::<f32>()).sum::<f32>() / channels as f32),
    );
}

/// Xruns, reroutes and real-time refusals leave the stream running.
fn is_fatal(kind: ErrorKind) -> bool {
    !matches!(kind, ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied)
}

/// Accumulates energy and yields an RMS level at most once per `interval`.
#[derive(Debug)]
struct LevelMeter {
    interval: Duration,
    last_emit: Option<Instant>,
    sum_squares: f64,
    count: usize,
}

impl LevelMeter {
    fn new(interval: Duration) -> Self {
        Self { interval, last_emit: None, sum_squares: 0.0, count: 0 }
    }

    /// Adds samples; returns the RMS (0..1) of everything since the last emit when due.
    fn push(&mut self, samples: &[f32], now: Instant) -> Option<f32> {
        for &s in samples {
            let s = if s.is_finite() { f64::from(s.clamp(-1.0, 1.0)) } else { 0.0 };
            self.sum_squares += s * s;
        }
        self.count += samples.len();
        let due = self.last_emit.is_none_or(|last| now.duration_since(last) >= self.interval);
        if !due || self.count == 0 {
            return None;
        }
        let rms = (self.sum_squares / self.count as f64).sqrt() as f32;
        self.last_emit = Some(now);
        self.sum_squares = 0.0;
        self.count = 0;
        Some(rms.clamp(0.0, 1.0))
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::microphone`
Expected: 8 passed, 1 ignored.

Run the hardware smoke test (needs a microphone): `cargo test -p opit-speech-to-text --lib platform::windows::microphone -- --ignored --nocapture`
Expected: prints the device list, `channels=1`, `duration_ms` ≥ 700 and level events; every named device opens with `fell_back=false`; a missing name falls back with `fell_back=true`.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/platform/windows
git commit -m "feat(app): capture the microphone with cpal

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Windows — global hotkey (`WH_KEYBOARD_LL`)

**Files:**
- Create: `crates/app/src/platform/windows/hotkey.rs`
- Modify: `crates/app/src/platform/windows/mod.rs`

**Interfaces:**
- Consumes: `platform::{Hotkey, HotkeyError, HotkeySink}`, `platform::keys::{KeyTracker, VK_ESCAPE, parse_combo}`.
- Produces: `WinHotkey::new()` implementing `Hotkey` (first `register` installs the hook thread; later calls re-target the combo and swap the sink, keeping held keys); `WinHotkey::shutdown()` (also on drop); `platform::windows::call_guarded(f)` (catches panics inside `extern "system"` callbacks, where unwinding would abort the process).

How it works: a named `opit-hotkey` thread installs the hook and pumps messages. The hook proc ignores events flagged `LLKHF_INJECTED` (so our own Ctrl+V never counts), prunes held keys whose key-up was missed (`GetAsyncKeyState`, e.g. released on the UAC/lock screen) before feeding a key-down to `KeyTracker`, calls the sink outside the state lock, and returns `LRESULT(1)` to swallow Esc (down and its matching up) while capture is on and not paused. The sink runs while Windows waits for the hook (hooks slower than ~1 s are silently removed), so it must be non-blocking — the controller's sink is an unbounded channel send. The hook is process-global, so its state lives in statics and only one `WinHotkey` may own it.

windows-rs 0.62 notes: success-only BOOL functions return `Result<()>` (`PostThreadMessageW`), handle-returning ones return `Result<Handle>` (`SetWindowsHookExW`), `GetMessageW` still returns a raw `BOOL` (loop while `.0 > 0`), `CallNextHookEx(None, …)` takes an `Option`, and `PeekMessageW(PM_NOREMOVE)` must run before the thread id is published so an early `WM_QUIT` is not lost.

- [ ] **Step 1: Wire the module**

In `crates/app/src/platform/windows/mod.rs` add `pub mod hotkey;` to the module list and `pub use hotkey::WinHotkey;` to the re-exports, then append:
```rust

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs a user callback from inside an `extern "system"` callback. A panic unwinding out of
/// such a function aborts the process, so it is caught and dropped here.
pub(crate) fn call_guarded(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}
```

- [ ] **Step 2: Write the failing tests**

`crates/app/src/platform/windows/hotkey.rs`:
```rust
//! Global hotkey via a `WH_KEYBOARD_LL` hook running on a dedicated message-loop thread.
//!
//! A low-level hook is process-global and its callback carries no user data, so the hook
//! state lives in statics and only one [`WinHotkey`] may own the hook at a time.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::platform::HotkeyEvent;

    /// The hook is process-global; tests touching it must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    fn collecting_sink() -> (HotkeySink, Arc<Mutex<Vec<HotkeyEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        (Arc::new(move |e| seen2.lock().unwrap().push(e)), seen)
    }

    #[test]
    fn install_retarget_and_shutdown() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, _seen) = collecting_sink();
        hk.register(&names(&["RightCtrl", "RightShift"]), sink.clone()).expect("install hook");
        thread::sleep(Duration::from_millis(200));
        hk.register(&names(&["F13"]), sink.clone()).expect("retarget");
        assert_eq!(state().as_ref().map(|s| s.tracker.combo().to_vec()), Some(vec![0x7C]));
        hk.set_paused(true);
        hk.set_capture_escape(true);
        hk.shutdown();
        assert!(state().is_none());
        assert!(!OWNED.load(Ordering::Relaxed));
        // A fresh instance can install again after shutdown.
        let hk2 = WinHotkey::new();
        hk2.register(&names(&["F13"]), sink).expect("reinstall");
        drop(hk2);
    }

    #[test]
    fn bad_keys_fail_without_installing() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, _) = collecting_sink();
        assert_eq!(hk.register(&[], sink.clone()), Err(HotkeyError::Empty));
        assert_eq!(hk.register(&names(&["Nope"]), sink), Err(HotkeyError::UnknownKey("Nope".into())));
        assert!(hk.thread.lock().unwrap().is_none());
        assert!(!OWNED.load(Ordering::Relaxed));
    }

    #[test]
    fn second_owner_is_rejected() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let a = WinHotkey::new();
        let b = WinHotkey::new();
        let (sink, _) = collecting_sink();
        a.register(&names(&["F13"]), sink.clone()).unwrap();
        assert!(matches!(b.register(&names(&["F14"]), sink), Err(HotkeyError::Install(_))));
        drop(b);
        assert!(OWNED.load(Ordering::Relaxed), "dropping the rejected instance must not release the hook");
    }

    /// Injects F24 down/up through SendInput: the hook must ignore it (LLKHF_INJECTED).
    #[test]
    #[ignore = "injects real keyboard input (F24)"]
    fn injected_keys_are_ignored() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_F24,
        };
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, seen) = collecting_sink();
        hk.register(&names(&["F24"]), sink).unwrap();
        thread::sleep(Duration::from_millis(100));
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_F24, dwFlags: flags, ..Default::default() } },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        // SAFETY: valid INPUT array and size.
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2);
        thread::sleep(Duration::from_millis(200));
        assert!(seen.lock().unwrap().is_empty(), "injected keys produced {:?}", seen.lock().unwrap());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::hotkey`
Expected: FAIL to compile — `cannot find type WinHotkey`, `cannot find function state`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/platform/windows/hotkey.rs`:
```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};
use windows::core::PCWSTR;

use super::call_guarded;
use crate::platform::keys::{KeyTracker, VK_ESCAPE, parse_combo};
use crate::platform::{Hotkey, HotkeyError, HotkeySink};

struct HookState {
    tracker: KeyTracker,
    sink: HotkeySink,
}

/// Tracker + sink read by the hook callback; swapped by `register`.
static STATE: Mutex<Option<HookState>> = Mutex::new(None);
static PAUSED: AtomicBool = AtomicBool::new(false);
static CAPTURE_ESC: AtomicBool = AtomicBool::new(false);
/// The last Esc key-down was swallowed, so its key-up must be swallowed too.
static ESC_SWALLOWED: AtomicBool = AtomicBool::new(false);
/// Some `WinHotkey` has installed the hook.
static OWNED: AtomicBool = AtomicBool::new(false);

fn state() -> MutexGuard<'static, Option<HookState>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

struct HookThread {
    thread_id: u32,
    handle: JoinHandle<()>,
}

/// Global hotkey backed by a low-level keyboard hook.
///
/// The sink is called on the hook thread while Windows waits for the hook to return (the
/// system silently removes hooks slower than ~1 s), so it must never block: pass something
/// like a non-blocking channel send (`UnboundedSender::send`, `try_send`).
#[derive(Default)]
pub struct WinHotkey {
    thread: Mutex<Option<HookThread>>,
}

impl WinHotkey {
    /// Creates an idle hotkey; the hook is installed by the first `register`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes the hook and stops its thread. Idempotent; also run on drop.
    pub fn shutdown(&self) {
        let Some(hook) = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            return;
        };
        // SAFETY: plain FFI call; the thread created its message queue before reporting its id.
        let posted = unsafe { PostThreadMessageW(hook.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if posted.is_ok() {
            let _ = hook.handle.join();
        }
        *state() = None;
        PAUSED.store(false, Ordering::Relaxed);
        CAPTURE_ESC.store(false, Ordering::Relaxed);
        ESC_SWALLOWED.store(false, Ordering::Relaxed);
        OWNED.store(false, Ordering::Release);
    }
}

impl Drop for WinHotkey {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Hotkey for WinHotkey {
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError> {
        let combo = parse_combo(keys)?;
        let mut thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
        let first = thread.is_none();
        if first && OWNED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return Err(HotkeyError::Install("another WinHotkey already owns the keyboard hook".into()));
        }
        {
            let mut st = state();
            match st.as_mut() {
                Some(s) => {
                    s.tracker.retarget(combo);
                    s.sink = sink;
                }
                None => *st = Some(HookState { tracker: KeyTracker::new(combo), sink }),
            }
        }
        if first {
            match spawn_hook_thread() {
                Ok(t) => *thread = Some(t),
                Err(e) => {
                    *state() = None;
                    OWNED.store(false, Ordering::Release);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        PAUSED.store(paused, Ordering::Relaxed);
    }

    fn set_capture_escape(&self, on: bool) {
        CAPTURE_ESC.store(on, Ordering::Relaxed);
    }
}

fn spawn_hook_thread() -> Result<HookThread, HotkeyError> {
    let (tx, rx) = mpsc::sync_channel(1);
    let handle = thread::Builder::new()
        .name("opit-hotkey".into())
        .spawn(move || hook_thread(tx))
        .map_err(|e| HotkeyError::Install(e.to_string()))?;
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(HookThread { thread_id, handle }),
        Ok(Err(msg)) => {
            let _ = handle.join();
            Err(HotkeyError::Install(msg))
        }
        Err(_) => {
            let _ = handle.join();
            Err(HotkeyError::Install("the hook thread exited unexpectedly".into()))
        }
    }
}

/// Installs the hook and pumps messages (hook callbacks are dispatched from GetMessageW)
/// until WM_QUIT.
fn hook_thread(ready: SyncSender<Result<u32, String>>) {
    // SAFETY: FFI calls with valid arguments; `msg` outlives every call using it.
    unsafe {
        let mut msg = MSG::default();
        // Create this thread's message queue so a WM_QUIT posted right after start is not lost.
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        let module = GetModuleHandleW(PCWSTR::null()).ok().map(HINSTANCE::from);
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(e.message()));
                return;
            }
        };
        let _ = ready.send(Ok(GetCurrentThreadId()));
        // GetMessageW returns -1 on error, 0 on WM_QUIT.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT valid for this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        // Injected input (including our own SendInput Ctrl+V) never counts.
        if let Some(down) = down
            && !info.flags.contains(LLKHF_INJECTED)
            && handle_key(info.vkCode, down)
        {
            return LRESULT(1);
        }
    }
    // SAFETY: forwarding the unchanged arguments; the hook handle parameter is ignored.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn key_is_down(vk: u32) -> bool {
    // SAFETY: plain FFI call. The high bit is set while the key is physically held.
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// Updates the tracker, delivers events, and returns true when the key must be swallowed.
fn handle_key(vk: u32, down: bool) -> bool {
    let (events, sink) = {
        let mut guard = state();
        let Some(st) = guard.as_mut() else {
            return false;
        };
        // Drop keys whose key-up we missed (e.g. released on the secure desktop) so they
        // cannot block the combo forever. The current key's async state is not updated yet.
        let pruned =
            if down && !st.tracker.held().is_empty() { st.tracker.prune(|k| k == vk || key_is_down(k)) } else { None };
        ([pruned, st.tracker.on_key(vk, down)], st.sink.clone())
    };
    let paused = PAUSED.load(Ordering::Relaxed);
    if !paused {
        for event in events.into_iter().flatten() {
            call_guarded(|| sink(event));
        }
    }
    if vk != VK_ESCAPE {
        return false;
    }
    if down {
        let swallow = !paused && CAPTURE_ESC.load(Ordering::Relaxed);
        if swallow {
            ESC_SWALLOWED.store(true, Ordering::Relaxed);
        }
        swallow
    } else {
        ESC_SWALLOWED.swap(false, Ordering::Relaxed)
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::hotkey`
Expected: 3 passed, 1 ignored (these install a real hook for a moment; no key presses needed).

Run: `cargo test -p opit-speech-to-text --lib platform::windows::hotkey -- --ignored`
Expected: 1 passed — an injected F24 press produces no event.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/platform/windows
git commit -m "feat(app): add the low-level keyboard hook for the global shortcut

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Windows — clipboard and paste

**Files:**
- Create: `crates/app/src/platform/windows/clipboard.rs`, `crates/app/src/platform/windows/paster.rs`
- Modify: `crates/app/src/platform/windows/mod.rs`

**Interfaces:**
- Consumes: `platform::{Paster, PasteOutcome, CLIPBOARD_RESTORE_DELAY}`.
- Produces:
  - `clipboard::{Clipboard::open() (retrying), snapshot() -> Snapshot, empty(), set_bytes(format, bytes), mark_private(), set_private_text(text), restore(&Snapshot)}`, `is_hglobal_format`, `utf16_bytes`, `register_format`, `sequence_number`, `CF_UNICODETEXT`.
  - `WinPaster::new()` implementing `Paster`; helpers `put_text`, `restore_if_unchanged`, `wait_for_modifiers_released`, `ctrl_v_inputs`, `foreground_integrity_rid`, `own_integrity_rid`, `foreground_is_higher_integrity`.

Paste sequence: snapshot every HGLOBAL-backed clipboard format (GDI-handle formats such as `CF_BITMAP`/`CF_ENHMETAFILE` are skipped; their `CF_DIB` twin carries images) → write the text as `CF_UNICODETEXT` marked private → remember the clipboard sequence number → if the foreground window's process has a higher integrity level, stop with `ClipboardOnly` (UIPI would drop the keys) → wait up to 1 s for Shift/Ctrl/Alt/Win to be released → `SendInput` Ctrl↓ V↓ V↑ Ctrl↑ (a partial send is completed with key-ups so nothing stays stuck; fewer than 4 → `ClipboardOnly`) → when `restore` and the result is `Pasted`, a background thread restores the snapshot after 400 ms only if the sequence number is unchanged. On `ClipboardOnly` the text stays so the user can press Ctrl+V.

Gotcha found while verifying: `OpenClipboard(None)` can report success without owning the clipboard (a clipboard listener races it; `EmptyClipboard` then fails with 0x8007058A in ~50 % of rapid cycles). `Clipboard::open` therefore opens with a throwaway message-only window as owner and destroys it after `CloseClipboard`. Also: `GlobalUnlock` returns `Err` when the lock count reaches 0 — never `?` it; the `TOKEN_MANDATORY_LABEL` buffer is a `Vec<u64>` for alignment.

- [ ] **Step 1: Wire the modules**

In `crates/app/src/platform/windows/mod.rs` add `pub mod clipboard;` and `pub mod paster;` to the module list and `pub use paster::WinPaster;` to the re-exports.

- [ ] **Step 2: Write the failing tests**

`crates/app/src/platform/windows/clipboard.rs`:
```rust
//! Thin safe wrapper over the Win32 clipboard: open with retry, snapshot/restore every
//! HGLOBAL-backed format, and write text marked as private (kept out of Win+V history and
//! the cloud clipboard).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_filter_skips_gdi_and_private_handles() {
        for f in [1, 7, 8, 13, 15, 16, 17, 0xC000, 0xC123, 0xFFFF] {
            assert!(is_hglobal_format(f), "{f:#x} should be copied");
        }
        for f in [0, 2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E, 0x200, 0x2FF, 0x300, 0x3FF] {
            assert!(!is_hglobal_format(f), "{f:#x} should be skipped");
        }
    }

    #[test]
    fn utf16_bytes_is_nul_terminated_le() {
        assert_eq!(utf16_bytes(""), vec![0, 0]);
        assert_eq!(utf16_bytes("Aş"), vec![0x41, 0, 0x5F, 0x01, 0, 0]);
    }

    #[test]
    fn register_format_is_stable() {
        let a = register_format("OpitTestFormat").unwrap();
        assert!(a >= 0xC000);
        assert_eq!(register_format("OpitTestFormat"), Some(a));
    }
}
```

`crates/app/src/platform/windows/paster.rs`:
```rust
//! Paste by clipboard + synthesized Ctrl+V, with clipboard backup/restore and a UIPI check.

#[cfg(test)]
mod tests {
    use super::super::clipboard::{CF_UNICODETEXT, register_format, utf16_bytes};
    use super::*;

    #[test]
    fn ctrl_v_inputs_are_ctrl_down_v_down_v_up_ctrl_up() {
        let inputs = ctrl_v_inputs();
        let got: Vec<(u16, u32)> = inputs
            .iter()
            .map(|i| {
                assert_eq!(i.r#type, INPUT_KEYBOARD);
                // SAFETY: we built these as keyboard inputs.
                let ki = unsafe { i.Anonymous.ki };
                assert_eq!((ki.wScan, ki.time, ki.dwExtraInfo), (0, 0, 0));
                (ki.wVk.0, ki.dwFlags.0)
            })
            .collect();
        assert_eq!(got, vec![(0x11, 0), (0x56, 0), (0x56, KEYEVENTF_KEYUP.0), (0x11, KEYEVENTF_KEYUP.0)]);
    }

    #[test]
    fn own_integrity_is_known() {
        let rid = own_integrity_rid().expect("own integrity");
        assert!((0x1000..=0x4000).contains(&rid), "{rid:#x}");
    }

    fn read(format: u32) -> Option<Vec<u8>> {
        let clipboard = Clipboard::open().unwrap();
        clipboard.snapshot().formats.into_iter().find(|(f, _)| *f == format).map(|(_, b)| b)
    }

    fn text_of(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    }

    /// Round-trips text + a custom format through put_text / restore_if_unchanged and prints
    /// the integrity levels. Saves and puts back the user's clipboard around the test.
    #[test]
    #[ignore = "touches the real clipboard"]
    fn clipboard_backup_restore_smoke() {
        let user = Clipboard::open().unwrap().snapshot();
        let custom = register_format("OpitSmokeFormat").unwrap();
        let payload = vec![1u8, 2, 3, 4, 5, 250];
        {
            let cb = Clipboard::open().unwrap();
            cb.empty().unwrap();
            cb.set_bytes(CF_UNICODETEXT, &utf16_bytes("original ğüş")).unwrap();
            cb.set_bytes(custom, &payload).unwrap();
        }

        let (backup, seq) = put_text("dictated text", true).unwrap();
        let backup = backup.unwrap();
        println!("backup formats: {:?}", backup.formats.iter().map(|(f, b)| (*f, b.len())).collect::<Vec<_>>());
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "dictated text");
        let history = register_format("CanIncludeInClipboardHistory").unwrap();
        assert_eq!(read(history).unwrap()[..4], [0, 0, 0, 0]);
        assert_eq!(sequence_number(), seq, "reading must not bump the sequence number");

        assert_eq!(restore_if_unchanged(&backup, seq), Ok(true));
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "original ğüş");
        assert_eq!(read(custom).unwrap()[..payload.len()], payload[..]);

        // The user copies something after our write: restore must not clobber it.
        let (backup2, seq2) = put_text("second dictation", true).unwrap();
        {
            let cb = Clipboard::open().unwrap();
            cb.empty().unwrap();
            cb.set_bytes(CF_UNICODETEXT, &utf16_bytes("user copy")).unwrap();
        }
        assert_eq!(restore_if_unchanged(&backup2.unwrap(), seq2), Ok(false));
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "user copy");

        // Back-to-back writes (the OpenClipboard(NULL) race this guards against).
        for i in 0..40 {
            put_text(&format!("stress {i}"), false).unwrap();
        }
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "stress 39");

        // Empty original clipboard → restoring leaves it empty.
        Clipboard::open().unwrap().empty().unwrap();
        let (backup3, seq3) = put_text("third", true).unwrap();
        let backup3 = backup3.unwrap();
        assert!(backup3.formats.is_empty());
        assert_eq!(restore_if_unchanged(&backup3, seq3), Ok(true));
        assert!(Clipboard::open().unwrap().snapshot().formats.is_empty());

        println!("own integrity RID: {:#x?}", own_integrity_rid());
        println!("foreground integrity RID: {:#x?}", foreground_integrity_rid());
        println!("foreground is higher: {:?}", foreground_is_higher_integrity());

        Clipboard::open().unwrap().restore(&user).unwrap();
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib -- platform::windows::clipboard platform::windows::paster`
Expected: FAIL to compile — `cannot find function is_hglobal_format`, `cannot find function ctrl_v_inputs`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/platform/windows/clipboard.rs`:
```rust
use std::marker::PhantomData;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{HSTRING, w};

/// Standard clipboard format ids (winuser.h). Defined here to avoid the large `Win32_System_Ole`
/// feature, which is where windows-rs puts `CF_*`.
pub const CF_BITMAP: u32 = 2;
pub const CF_METAFILEPICT: u32 = 3;
pub const CF_PALETTE: u32 = 9;
pub const CF_UNICODETEXT: u32 = 13;
pub const CF_ENHMETAFILE: u32 = 14;
pub const CF_OWNERDISPLAY: u32 = 0x80;
pub const CF_DSPBITMAP: u32 = 0x82;
pub const CF_DSPMETAFILEPICT: u32 = 0x83;
pub const CF_DSPENHMETAFILE: u32 = 0x8E;
/// CF_PRIVATEFIRST..=CF_GDIOBJLAST: app-private handles and GDI objects, never HGLOBALs we may copy.
const PRIVATE_AND_GDI_RANGE: std::ops::RangeInclusive<u32> = 0x200..=0x3FF;

const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(20);

/// Registered format names that control clipboard history / cloud sync.
pub const EXCLUDE_FROM_MONITORING: &str = "ExcludeClipboardContentFromMonitorProcessing";
pub const CAN_INCLUDE_IN_HISTORY: &str = "CanIncludeInClipboardHistory";
pub const CAN_UPLOAD_TO_CLOUD: &str = "CanUploadToCloudClipboard";

/// True when data of `format` is an HGLOBAL that can be copied byte-for-byte.
pub fn is_hglobal_format(format: u32) -> bool {
    !matches!(
        format,
        0 | CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) && !PRIVATE_AND_GDI_RANGE.contains(&format)
}

/// Encodes `text` as NUL-terminated UTF-16 bytes (CF_UNICODETEXT payload).
pub fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
}

/// Registers (or looks up) a named clipboard format. None if registration failed.
pub fn register_format(name: &str) -> Option<u32> {
    // SAFETY: HSTRING is NUL-terminated and outlives the call.
    let id = unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) };
    (id != 0).then_some(id)
}

/// Current clipboard sequence number (changes whenever the clipboard content changes).
pub fn sequence_number() -> u32 {
    // SAFETY: plain FFI call.
    unsafe { GetClipboardSequenceNumber() }
}

/// Copy of every HGLOBAL-backed format on the clipboard. Empty means the clipboard was empty
/// (or held only formats we cannot copy).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub formats: Vec<(u32, Vec<u8>)>,
}

/// An open clipboard; closed on drop. Not `Send`: the clipboard is opened per thread.
///
/// Owned by a throwaway message-only window rather than NULL: with `OpenClipboard(NULL)`
/// Windows can report success while the clipboard is not actually open for us (EmptyClipboard
/// then fails with ERROR_CLIPBOARD_NOT_OPEN), which was reproducible here in 20/40 cycles.
pub struct Clipboard {
    owner: HWND,
    _not_send: PhantomData<*const ()>,
}

impl Clipboard {
    /// Opens the clipboard, retrying (10 × 20 ms) while another app holds it.
    pub fn open() -> Result<Self, String> {
        // SAFETY: creates a message-only STATIC window on this thread; destroyed on every path.
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|e| format!("could not create the clipboard owner window: {}", e.message()))?;
        let mut last = String::new();
        for attempt in 0..OPEN_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(OPEN_RETRY_DELAY);
            }
            // SAFETY: plain FFI call; paired with CloseClipboard in Drop.
            match unsafe { OpenClipboard(Some(owner)) } {
                Ok(()) => return Ok(Self { owner, _not_send: PhantomData }),
                Err(e) => last = e.message(),
            }
        }
        // SAFETY: our own window, created above on this thread.
        let _ = unsafe { DestroyWindow(owner) };
        Err(format!("the clipboard is busy: {last}"))
    }

    /// Copies all HGLOBAL formats. Formats that fail to render or lock are skipped.
    pub fn snapshot(&self) -> Snapshot {
        let mut formats = Vec::new();
        let mut format = 0;
        loop {
            // SAFETY: the clipboard is open on this thread.
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if !is_hglobal_format(format) {
                continue;
            }
            // SAFETY: the clipboard is open; the handle is owned by the clipboard.
            if let Ok(handle) = unsafe { GetClipboardData(format) }
                && let Some(bytes) = unsafe { copy_hglobal(HGLOBAL(handle.0)) }
            {
                formats.push((format, bytes));
            }
        }
        Snapshot { formats }
    }

    /// Empties the clipboard and takes ownership of it.
    pub fn empty(&self) -> Result<(), String> {
        // SAFETY: the clipboard is open on this thread.
        unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {}", e.message()))
    }

    /// Puts `bytes` on the clipboard as `format`. Call after `empty`.
    pub fn set_bytes(&self, format: u32, bytes: &[u8]) -> Result<(), String> {
        // SAFETY: we allocate a movable global, fill it while locked, and hand it to the
        // clipboard; on failure the global is still ours and is freed.
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| e.message())?;
            let ptr = GlobalLock(mem);
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(mem);
            if let Err(e) = SetClipboardData(format, Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(format!("SetClipboardData({format}) failed: {}", e.message()));
            }
        }
        Ok(())
    }

    /// Adds the formats that keep the current content out of clipboard history, Win+V and
    /// the cloud clipboard. Best effort: failures are ignored.
    pub fn mark_private(&self) {
        let zero = 0u32.to_le_bytes();
        for (name, data) in [
            (EXCLUDE_FROM_MONITORING, &zero[..]),
            (CAN_INCLUDE_IN_HISTORY, &zero[..]),
            (CAN_UPLOAD_TO_CLOUD, &zero[..]),
        ] {
            if let Some(id) = register_format(name) {
                let _ = self.set_bytes(id, data);
            }
        }
    }

    /// Replaces the clipboard with `text` (CF_UNICODETEXT), marked private.
    pub fn set_private_text(&self, text: &str) -> Result<(), String> {
        self.empty()?;
        self.set_bytes(CF_UNICODETEXT, &utf16_bytes(text))?;
        self.mark_private();
        Ok(())
    }

    /// Replaces the clipboard with `snapshot` (an empty snapshot leaves it empty). The restore
    /// itself is marked private so it does not add a duplicate Win+V history entry.
    pub fn restore(&self, snapshot: &Snapshot) -> Result<(), String> {
        self.empty()?;
        if snapshot.formats.is_empty() {
            return Ok(());
        }
        let privacy: Vec<u32> = [EXCLUDE_FROM_MONITORING, CAN_INCLUDE_IN_HISTORY, CAN_UPLOAD_TO_CLOUD]
            .iter()
            .filter_map(|n| register_format(n))
            .collect();
        let mut first_error = None;
        for (format, bytes) in snapshot.formats.iter().filter(|(f, _)| !privacy.contains(f)) {
            if let Err(e) = self.set_bytes(*format, bytes) {
                first_error.get_or_insert(e);
            }
        }
        self.mark_private();
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: we opened the clipboard in `open` with `owner`, a window of this thread.
        // Destroying the owner keeps non-delayed data and does not bump the sequence number.
        unsafe {
            let _ = CloseClipboard();
            let _ = DestroyWindow(self.owner);
        }
    }
}

/// Copies the bytes of a clipboard-owned HGLOBAL. None if it is not a lockable global.
///
/// # Safety
/// `mem` must come from `GetClipboardData` while the clipboard is open.
unsafe fn copy_hglobal(mem: HGLOBAL) -> Option<Vec<u8>> {
    // SAFETY: guaranteed by the caller; GlobalSize/GlobalLock fail cleanly on non-globals.
    unsafe {
        let size = GlobalSize(mem);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(mem);
        Some(bytes)
    }
}
```

Insert above the tests in `crates/app/src/platform/windows/paster.rs`:
```rust
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::Owned;

use super::clipboard::{Clipboard, Snapshot, sequence_number};
use crate::platform::{CLIPBOARD_RESTORE_DELAY, PasteOutcome, Paster};

/// Longest wait for the user to let go of modifiers before sending Ctrl+V anyway.
const MODIFIER_WAIT: Duration = Duration::from_secs(1);
const MODIFIER_POLL: Duration = Duration::from_millis(10);
const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];

/// Clipboard + Ctrl+V paster.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinPaster;

impl WinPaster {
    pub fn new() -> Self {
        Self
    }
}

impl Paster for WinPaster {
    fn paste(&self, text: &str, restore: bool) -> PasteOutcome {
        let (backup, seq) = match put_text(text, restore) {
            Ok(v) => v,
            Err(e) => return PasteOutcome::Failed(e),
        };
        wait_for_modifiers_released(MODIFIER_WAIT);
        if foreground_is_higher_integrity() == Some(true) {
            // UIPI would drop the input silently; leave the text on the clipboard for the user.
            return PasteOutcome::ClipboardOnly;
        }
        if !send_ctrl_v() {
            return PasteOutcome::ClipboardOnly;
        }
        // Restore only after a real paste: on ClipboardOnly the user still needs our text.
        if let Some(backup) = backup {
            let spawned = thread::Builder::new().name("opit-clipboard-restore".into()).spawn(move || {
                thread::sleep(CLIPBOARD_RESTORE_DELAY);
                let _ = restore_if_unchanged(&backup, seq);
            });
            drop(spawned); // Best effort: if the thread cannot start, our text simply stays.
        }
        PasteOutcome::Pasted
    }
}

/// Backs up the clipboard (when `backup`), writes `text` marked private and returns the
/// backup plus the clipboard sequence number right after our write.
pub fn put_text(text: &str, backup: bool) -> Result<(Option<Snapshot>, u32), String> {
    let clipboard = Clipboard::open()?;
    let snapshot = backup.then(|| clipboard.snapshot());
    clipboard.set_private_text(text)?;
    drop(clipboard);
    Ok((snapshot, sequence_number()))
}

/// Restores `snapshot` unless someone changed the clipboard after our write (`seq`).
/// Returns whether it restored.
pub fn restore_if_unchanged(snapshot: &Snapshot, seq: u32) -> Result<bool, String> {
    if sequence_number() != seq {
        return Ok(false);
    }
    let clipboard = Clipboard::open()?;
    // Re-check while we hold the clipboard so nobody can slip in between.
    if sequence_number() != seq {
        return Ok(false);
    }
    clipboard.restore(snapshot)?;
    Ok(true)
}

/// Waits (up to `timeout`) until Shift/Ctrl/Alt/Win are all physically released, so a
/// still-held hotkey modifier does not turn Ctrl+V into Ctrl+Shift+V. Returns false on timeout.
pub fn wait_for_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: plain FFI calls.
        let held = MODIFIERS.iter().any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0);
        if !held {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(MODIFIER_POLL);
    }
}

/// (virtual key, key-up) for Ctrl down, V down, V up, Ctrl up.
pub const CTRL_V_SEQUENCE: [(VIRTUAL_KEY, bool); 4] =
    [(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)];

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } },
    }
}

/// The four keyboard INPUTs for Ctrl+V.
pub fn ctrl_v_inputs() -> [INPUT; 4] {
    CTRL_V_SEQUENCE.map(|(vk, up)| key_input(vk, up))
}

/// Sends Ctrl+V; true only if all four events were accepted.
fn send_ctrl_v() -> bool {
    let inputs = ctrl_v_inputs();
    // SAFETY: valid INPUT slice and matching cbSize.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if sent != inputs.len() && sent > 0 {
        // Partially sent: never leave Ctrl or V logically stuck down.
        let release = [key_input(VK_V, true), key_input(VK_CONTROL, true)];
        // SAFETY: as above.
        unsafe { SendInput(&release, size_of::<INPUT>() as i32) };
    }
    sent == inputs.len()
}

/// Integrity RID (e.g. 0x2000 medium, 0x3000 high) of the process owning the foreground
/// window. None when there is no foreground window or its process cannot be queried.
pub fn foreground_integrity_rid() -> Option<u32> {
    // SAFETY: plain FFI calls; the out-pointer is valid for the call.
    let pid = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    };
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is owned and closed by `Owned`.
    let process = unsafe { Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?) };
    token_integrity_rid(*process)
}

/// Integrity RID of this process.
pub fn own_integrity_rid() -> Option<u32> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    token_integrity_rid(unsafe { GetCurrentProcess() })
}

/// Some(true) when the foreground app runs at a higher integrity level than we do (so UIPI
/// blocks our SendInput), None when unknown.
pub fn foreground_is_higher_integrity() -> Option<bool> {
    Some(foreground_integrity_rid()? > own_integrity_rid()?)
}

fn token_integrity_rid(process: HANDLE) -> Option<u32> {
    // SAFETY: every out-pointer points to live, correctly sized storage; the token handle is
    // closed by `Owned`; the SID pointer lives inside `buf`, which outlives its use.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let token = Owned::new(token);
        let mut len = 0u32;
        // First call only reports the needed size (and fails with ERROR_INSUFFICIENT_BUFFER).
        let _ = GetTokenInformation(*token, TokenIntegrityLevel, None, 0, &mut len);
        if (len as usize) < size_of::<TOKEN_MANDATORY_LABEL>() {
            return None;
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)]; // u64 for pointer alignment
        GetTokenInformation(*token, TokenIntegrityLevel, Some(buf.as_mut_ptr().cast()), len, &mut len).ok()?;
        let label = &*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib -- platform::windows::clipboard platform::windows::paster`
Expected: 5 passed, 1 ignored.

Run the clipboard smoke test (it saves and restores your clipboard around itself): `cargo test -p opit-speech-to-text --lib platform::windows::paster -- --ignored --nocapture`
Expected: 1 passed; prints `own integrity RID 0x2000` and the foreground RID; text plus a custom format come back byte-for-byte; the privacy DWORD is 0; a user copy inside the restore window is not overwritten; an empty clipboard stays empty.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/platform/windows
git commit -m "feat(app): paste via clipboard and SendInput with backup and restore

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Windows — status overlay

**Files:**
- Create: `crates/app/src/platform/windows/overlay.rs`
- Modify: `crates/app/src/platform/windows/mod.rs`

**Interfaces:**
- Consumes: `platform::{Overlay, OverlaySink, OverlayView, OverlayAction, OverlayButton, Tone}`, `opit_core::config::OverlayPosition`, `call_guarded`.
- Produces: `WinOverlay::new(position, sink: OverlaySink) -> Result<WinOverlay, String>` implementing `Overlay` (starts the window thread and waits for the HWND); pure helpers `RectI`, `scale(value, dpi)`, `overlay_height(has_button, dpi)`, `overlay_rect(work, position, dpi, height_px) -> RectI`.

How it works: a popup (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`) on its own per-monitor-DPI-aware thread, constant alpha, rounded region, dark background, GDI text in Segoe UI with a tone-coloured accent and an optional level bar. Other threads store the next view in a mutex and post one coalesced `WM_APP + 1`, so 20 Hz level updates never flood the queue. It appears on the work area of the monitor that holds the foreground window, 24 px from the edge given by `ui.overlay_position` (default right-centre). `hide_after` arms a timer. With a button the `WS_EX_TRANSPARENT` style is removed, the cursor becomes a hand, and a click anywhere fires the action through the sink (then hides); `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, so focus never leaves the user's app. Window state is only reached through `try_borrow_mut`, because `SetWindowPos`/`ShowWindow`/`SetWindowRgn` re-enter the window procedure; a layered window stays invisible until `SetLayeredWindowAttributes` has run.

- [ ] **Step 1: Wire the module**

In `crates/app/src/platform/windows/mod.rs` add `pub mod overlay;` to the module list and `pub use overlay::WinOverlay;` to the re-exports.

- [ ] **Step 2: Write the failing tests**

`crates/app/src/platform/windows/overlay.rs`:
```rust
//! Status overlay: a small layered, topmost, non-activating popup drawn with GDI on its own
//! UI thread. Other threads hand it state through a mutex and a coalesced `WM_APP + 1`.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::platform::{OverlayAction, OverlayButton};

    const WORK: RectI = RectI { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn scale_rounds() {
        assert_eq!(scale(260, 96), 260);
        assert_eq!(scale(260, 144), 390);
        assert_eq!(scale(24, 120), 30);
        assert_eq!(scale(10, 120), 13); // 12.5 rounds up
        assert_eq!(overlay_height(false, 96), 56);
        assert_eq!(overlay_height(true, 192), 152);
    }

    #[test]
    fn right_center_at_96_dpi() {
        let r = overlay_rect(WORK, OverlayPosition::RightCenter, 96, 56);
        assert_eq!(
            r,
            RectI { left: 1920 - 24 - 260, top: (1040 - 56) / 2, right: 1920 - 24, bottom: (1040 - 56) / 2 + 56 }
        );
    }

    #[test]
    fn all_positions_at_150_percent() {
        let h = overlay_height(false, 144); // 84
        let (w, m) = (390, 36);
        let r = overlay_rect(WORK, OverlayPosition::LeftCenter, 144, h);
        assert_eq!((r.left, r.top, r.width(), r.height()), (m, (1040 - h) / 2, w, h));
        let r = overlay_rect(WORK, OverlayPosition::TopCenter, 144, h);
        assert_eq!((r.left, r.top), ((1920 - w) / 2, m));
        let r = overlay_rect(WORK, OverlayPosition::BottomCenter, 144, h);
        assert_eq!((r.left, r.bottom), ((1920 - w) / 2, 1040 - m));
        let r = overlay_rect(WORK, OverlayPosition::RightCenter, 144, h);
        assert_eq!(r.right, 1920 - m);
    }

    #[test]
    fn work_area_offset_and_secondary_monitor() {
        // Secondary monitor left of the primary with a taskbar on top.
        let work = RectI { left: -2560, top: 48, right: 0, bottom: 1440 };
        let r = overlay_rect(work, OverlayPosition::TopCenter, 96, 56);
        assert_eq!((r.left, r.top), (-2560 + (2560 - 260) / 2, 48 + 24));
        let r = overlay_rect(work, OverlayPosition::RightCenter, 96, 56);
        assert_eq!(r.right, -24);
        assert_eq!(r.top, 48 + (1392 - 56) / 2);
    }

    #[test]
    fn tiny_work_area_is_clamped() {
        let work = RectI { left: 100, top: 100, right: 300, bottom: 140 };
        for pos in [
            OverlayPosition::RightCenter,
            OverlayPosition::LeftCenter,
            OverlayPosition::TopCenter,
            OverlayPosition::BottomCenter,
        ] {
            let r = overlay_rect(work, pos, 96, 56);
            assert!(
                r.left >= work.left && r.right <= work.right && r.top >= work.top && r.bottom <= work.bottom,
                "{pos:?} {r:?}"
            );
            assert_eq!((r.width(), r.height()), (200, 40));
        }
    }

    /// Creates the overlay, animates a Listening level for 1.5 s, shows an Error with a button
    /// for 1.5 s, hides and drops it.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_smoke() {
        let clicks = Arc::new(Mutex::new(Vec::new()));
        let clicks2 = Arc::clone(&clicks);
        let overlay =
            WinOverlay::new(OverlayPosition::RightCenter, Arc::new(move |a| clicks2.lock().unwrap().push(a))).unwrap();
        for i in 0..30 {
            let level = ((i as f32) * 0.45).sin().abs();
            overlay.show(OverlayView {
                tone: Tone::Neutral,
                text: "Listening…".into(),
                level: Some(level),
                button: None,
                hide_after: None,
            });
            thread::sleep(Duration::from_millis(50));
        }
        overlay.show(OverlayView {
            tone: Tone::Error,
            text: "The API key was rejected by the provider".into(),
            level: None,
            button: Some(OverlayButton { action: OverlayAction::OpenSettings, label: "Open settings".into() }),
            hide_after: Some(Duration::from_secs(5)),
        });
        thread::sleep(Duration::from_millis(1500));
        overlay.set_position(OverlayPosition::BottomCenter);
        thread::sleep(Duration::from_millis(300));
        overlay.hide();
        thread::sleep(Duration::from_millis(100));
        drop(overlay);
        println!("clicks: {:?}", clicks.lock().unwrap());
    }

    /// hide_after hides on the window thread by itself.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_auto_hide() {
        let overlay = WinOverlay::new(OverlayPosition::TopCenter, Arc::new(|_| {})).unwrap();
        overlay.show(OverlayView {
            tone: Tone::Success,
            text: "Pasted".into(),
            level: None,
            button: None,
            hide_after: Some(Duration::from_millis(300)),
        });
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay));
        thread::sleep(Duration::from_millis(500));
        assert!(!visible(&overlay));
    }

    fn visible(o: &WinOverlay) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        // SAFETY: plain FFI call.
        unsafe { IsWindowVisible(o.hwnd()) }.as_bool()
    }

    fn click_through(o: &WinOverlay) -> bool {
        // SAFETY: plain FFI call.
        let ex = unsafe { GetWindowLongPtrW(o.hwnd(), GWL_EXSTYLE) } as u32;
        ex & WS_EX_TRANSPARENT.0 != 0
    }

    /// A button makes the overlay clickable; a click (posted, so the real mouse is untouched)
    /// fires the action and hides; a view without a button is click-through again.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_button_click() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let overlay = WinOverlay::new(
            OverlayPosition::BottomCenter,
            Arc::new(move |a| {
                let _ = tx.lock().unwrap().send(a);
            }),
        )
        .unwrap();
        let view = |button: Option<OverlayButton>| OverlayView {
            tone: Tone::Warning,
            text: "Nothing was heard".into(),
            level: None,
            button,
            hide_after: None,
        };
        thread::sleep(Duration::from_millis(50));
        assert!(click_through(&overlay));
        overlay.show(view(Some(OverlayButton { action: OverlayAction::Retry, label: "Retry".into() })));
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay) && !click_through(&overlay));
        // SAFETY: plain FFI call.
        unsafe { PostMessageW(Some(overlay.hwnd()), WM_LBUTTONUP, WPARAM(0), LPARAM(0)) }.unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(OverlayAction::Retry));
        thread::sleep(Duration::from_millis(50));
        assert!(!visible(&overlay));
        overlay.show(view(None));
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay) && click_through(&overlay));
        // A click without a button does nothing.
        // SAFETY: plain FFI call.
        unsafe { PostMessageW(Some(overlay.hwnd()), WM_LBUTTONUP, WPARAM(0), LPARAM(0)) }.unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(visible(&overlay));
        assert!(rx.try_recv().is_err());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::overlay`
Expected: FAIL to compile — `cannot find function overlay_rect`, `cannot find type WinOverlay`.

- [ ] **Step 4: Implement**

Insert above the tests in `crates/app/src/platform/windows/overlay.rs`:
```rust
use std::cell::RefCell;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use opit_core::config::OverlayPosition;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint, FW_NORMAL,
    FW_SEMIBOLD, FillRect, GetMonitorInfoW, GetStockObject, HDC, HFONT, HGDIOBJ, InvalidateRect,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, RoundRect,
    SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWL_EXSTYLE, GetClientRect, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, IDC_HAND, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::call_guarded;
use crate::platform::{Overlay, OverlaySink, OverlayView, Tone};

/// Posted by other threads when `Pending` has something new.
const WM_APP_UPDATE: u32 = WM_APP + 1;
const HIDE_TIMER: usize = 1;
const CLASS_NAME: PCWSTR = w!("OpitOverlayWindow");
const ALPHA: u8 = 235;

/// Logical (96-dpi) layout constants.
const WIDTH: i32 = 260;
const HEIGHT: i32 = 56;
const HEIGHT_WITH_BUTTON: i32 = 76;
const MARGIN: i32 = 24;
const PAD: i32 = 16;
const CORNER: i32 = 16;
const DOT: i32 = 10;
const ROW: i32 = 22;
const BAR: i32 = 4;

const BG: COLORREF = rgb(30, 31, 36);
const FG: COLORREF = rgb(236, 236, 240);
const TRACK: COLORREF = rgb(62, 63, 72);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn tone_color(tone: Tone) -> COLORREF {
    match tone {
        Tone::Neutral => rgb(138, 180, 248),
        Tone::Busy => rgb(178, 146, 255),
        Tone::Success => rgb(80, 200, 120),
        Tone::Warning => rgb(245, 182, 66),
        Tone::Error => rgb(242, 95, 92),
    }
}

/// Integer rectangle in physical pixels (left/top inclusive, right/bottom exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

impl From<RECT> for RectI {
    fn from(r: RECT) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

/// Scales a 96-dpi length to `dpi`, rounding to nearest.
pub fn scale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

/// Overlay height in physical pixels for a view.
pub fn overlay_height(has_button: bool, dpi: u32) -> i32 {
    scale(if has_button { HEIGHT_WITH_BUTTON } else { HEIGHT }, dpi)
}

/// Where the overlay goes inside the monitor work area `work` (physical px). The overlay is
/// `WIDTH` logical px wide, `height_px` tall, `MARGIN` logical px from the chosen edge, and
/// is shrunk/clamped so it never leaves the work area.
pub fn overlay_rect(work: RectI, position: OverlayPosition, dpi: u32, height_px: i32) -> RectI {
    let w = scale(WIDTH, dpi).min(work.width()).max(0);
    let h = height_px.min(work.height()).max(0);
    let margin = scale(MARGIN, dpi);
    let center_x = work.left + (work.width() - w) / 2;
    let center_y = work.top + (work.height() - h) / 2;
    let (x, y) = match position {
        OverlayPosition::RightCenter => (work.right - margin - w, center_y),
        OverlayPosition::LeftCenter => (work.left + margin, center_y),
        OverlayPosition::TopCenter => (center_x, work.top + margin),
        OverlayPosition::BottomCenter => (center_x, work.bottom - margin - h),
    };
    let x = x.clamp(work.left, work.right - w);
    let y = y.clamp(work.top, work.bottom - h);
    RectI { left: x, top: y, right: x + w, bottom: y + h }
}

/// State handed from other threads to the window thread. Latest write wins.
#[derive(Default)]
struct Pending {
    /// Some(Some(view)) = show, Some(None) = hide.
    view: Option<Option<OverlayView>>,
    position: Option<OverlayPosition>,
    /// A WM_APP_UPDATE is already queued; don't post another (keeps level updates coalesced).
    posted: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Win32 overlay window. Cheap to call from any thread; all drawing happens on its own thread.
///
/// The sink is called on the overlay's UI thread when the button is clicked; it must not
/// block (use a non-blocking channel send).
pub struct WinOverlay {
    /// Raw HWND (HWND itself is not Send).
    hwnd: isize,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl WinOverlay {
    /// Starts the overlay thread and waits until its window exists. The overlay starts hidden.
    pub fn new(position: OverlayPosition, sink: OverlaySink) -> Result<WinOverlay, String> {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::sync_channel(1);
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("opit-overlay".into())
            .spawn(move || window_thread(position, sink, thread_shared, tx))
            .map_err(|e| format!("could not start the overlay thread: {e}"))?;
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(WinOverlay { hwnd, shared, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the overlay thread exited unexpectedly".into())
            }
        }
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    fn update(&self, f: impl FnOnce(&mut Pending)) {
        let mut pending = self.shared.lock();
        f(&mut pending);
        if !pending.posted {
            // SAFETY: plain FFI call; posting to a destroyed window just fails.
            pending.posted = unsafe { PostMessageW(Some(self.hwnd()), WM_APP_UPDATE, WPARAM(0), LPARAM(0)) }.is_ok();
        }
    }
}

impl Overlay for WinOverlay {
    fn show(&self, view: OverlayView) {
        self.update(|p| p.view = Some(Some(view)));
    }

    fn hide(&self) {
        self.update(|p| p.view = Some(None));
    }

    fn set_position(&self, position: OverlayPosition) {
        self.update(|p| p.position = Some(position));
    }
}

impl Drop for WinOverlay {
    fn drop(&mut self) {
        // WM_CLOSE → DestroyWindow (DefWindowProc) → WM_DESTROY → PostQuitMessage.
        // SAFETY: plain FFI call.
        let posted = unsafe { PostMessageW(Some(self.hwnd()), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok();
        if let Some(thread) = self.thread.take()
            && (posted || thread.is_finished())
        {
            let _ = thread.join();
        }
    }
}

// ---------- window thread ----------

struct Fonts {
    dpi: u32,
    text: HFONT,
    link: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let make = |px: i32, weight: u32, underline: u32| {
            // SAFETY: plain FFI call with a static face name.
            unsafe {
                CreateFontW(
                    -scale(px, dpi),
                    0,
                    0,
                    0,
                    weight as i32,
                    0,
                    underline,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe UI"),
                )
            }
        };
        Self { dpi, text: make(15, FW_NORMAL.0, 0), link: make(14, FW_SEMIBOLD.0, 1) }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: we created both fonts and they are not selected into any DC anymore.
        unsafe {
            let _ = DeleteObject(self.text.into());
            let _ = DeleteObject(self.link.into());
        }
    }
}

struct WinState {
    shared: Arc<Shared>,
    sink: OverlaySink,
    /// The view on screen; None = hidden.
    view: Option<OverlayView>,
    position: OverlayPosition,
    dpi: u32,
    fonts: Option<Fonts>,
    clickable: bool,
}

thread_local! {
    static WIN: RefCell<Option<WinState>> = const { RefCell::new(None) };
}

/// Runs `f` on the window state. Returns None if there is no state or it is already borrowed
/// (re-entrant window messages), so a nested call can never panic.
fn with_state<R>(f: impl FnOnce(&mut WinState) -> R) -> Option<R> {
    WIN.with(|cell| cell.try_borrow_mut().ok().and_then(|mut guard| guard.as_mut().map(f)))
}

fn window_thread(
    position: OverlayPosition,
    sink: OverlaySink,
    shared: Arc<Shared>,
    ready: SyncSender<Result<isize, String>>,
) {
    // SAFETY: FFI calls with valid arguments; the class name and title are static strings.
    let hwnd = unsafe {
        // Physical-pixel coordinates regardless of the process manifest.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = match GetModuleHandleW(PCWSTR::null()) {
            Ok(m) => m.into(),
            Err(e) => {
                let _ = ready.send(Err(format!("GetModuleHandleW failed: {}", e.message())));
                return;
            }
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            let _ = ready.send(Err(format!("RegisterClassW failed: {}", windows::core::Error::from_thread())));
            return;
        }
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        let hwnd =
            match CreateWindowExW(ex, CLASS_NAME, w!("Opit"), WS_POPUP, 0, 0, 1, 1, None, None, Some(instance), None) {
                Ok(h) => h,
                Err(e) => {
                    let _ = ready.send(Err(format!("CreateWindowExW failed: {}", e.message())));
                    return;
                }
            };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), ALPHA, LWA_ALPHA);
        hwnd
    };
    WIN.set(Some(WinState { shared, sink, view: None, position, dpi: 96, fonts: None, clickable: false }));
    let _ = ready.send(Ok(hwnd.0 as isize));

    // SAFETY: standard message loop; `msg` outlives every call.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    WIN.set(None); // drops fonts
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_UPDATE => {
            apply_pending(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // WM_PAINT covers every pixel
        WM_TIMER if wparam.0 == HIDE_TIMER => {
            hide_now(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR if with_state(|s| s.clickable) == Some(true) => {
            // SAFETY: plain FFI calls with a system cursor id.
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let clicked = with_state(|s| {
                let action = s.view.as_ref()?.button.as_ref()?.action;
                Some((action, Arc::clone(&s.sink)))
            })
            .flatten();
            if let Some((action, sink)) = clicked {
                hide_now(hwnd);
                call_guarded(|| sink(action));
            }
            LRESULT(0)
        }
        // We size ourselves for the target monitor; ignore the suggested rect.
        WM_DPICHANGED => LRESULT(0),
        WM_DESTROY => {
            // SAFETY: plain FFI call on the window's own thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default handling with the unchanged arguments.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Drains `Pending` and applies it. No state borrow is held across calls that can re-enter
/// the window procedure (SetWindowPos, ShowWindow, SetWindowLongPtrW, SetWindowRgn).
fn apply_pending(hwnd: HWND) {
    let Some(shared) = with_state(|s| Arc::clone(&s.shared)) else {
        return;
    };
    let (view, position) = {
        let mut p = shared.lock();
        p.posted = false;
        (p.view.take(), p.position.take())
    };
    let moved = position.and_then(|pos| {
        with_state(|s| {
            let changed = s.position != pos;
            s.position = pos;
            changed
        })
    }) == Some(true);
    match view {
        Some(Some(view)) => show_view(hwnd, view, moved),
        Some(None) => hide_now(hwnd),
        None if moved && with_state(|s| s.view.is_some()) == Some(true) => {
            let has_button = with_state(|s| s.view.as_ref().is_some_and(|v| v.button.is_some())).unwrap_or(false);
            place(hwnd, has_button);
        }
        None => {}
    }
}

fn show_view(hwnd: HWND, view: OverlayView, force_place: bool) {
    let has_button = view.button.is_some();
    let hide_after = view.hide_after;
    let Some(previous) = with_state(|s| s.view.replace(view)) else {
        return;
    };
    // SAFETY: timer calls on our own window.
    unsafe {
        match hide_after {
            Some(d) => {
                let ms = u32::try_from(d.as_millis()).unwrap_or(u32::MAX).max(1);
                SetTimer(Some(hwnd), HIDE_TIMER, ms, None);
            }
            None => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
            }
        }
    }
    set_clickable(hwnd, has_button);
    let height_changed = previous.as_ref().is_some_and(|p| p.button.is_some() != has_button);
    if previous.is_none() || force_place || height_changed {
        place(hwnd, has_button);
    }
    // SAFETY: plain FFI call on our own window.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn hide_now(hwnd: HWND) {
    with_state(|s| s.view = None);
    // SAFETY: plain FFI calls on our own window.
    unsafe {
        let _ = KillTimer(Some(hwnd), HIDE_TIMER);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Toggles WS_EX_TRANSPARENT: click-through unless the view has a button.
fn set_clickable(hwnd: HWND, clickable: bool) {
    if with_state(|s| std::mem::replace(&mut s.clickable, clickable)) == Some(clickable) {
        return;
    }
    // SAFETY: reading/writing our own window's extended style.
    unsafe {
        let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let ex = if clickable { WINDOW_EX_STYLE(ex.0 & !WS_EX_TRANSPARENT.0) } else { ex | WS_EX_TRANSPARENT };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex.0 as isize);
    }
}

/// Sizes and positions the window on the foreground window's monitor, then shows it.
fn place(hwnd: HWND, has_button: bool) {
    // SAFETY: FFI calls with valid out-pointers; the region is owned by the system after
    // SetWindowRgn succeeds.
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            dpi_x = 96;
        }
        let Some(position) = with_state(|s| {
            s.dpi = dpi_x;
            s.position
        }) else {
            return;
        };
        let r = overlay_rect(info.rcWork.into(), position, dpi_x, overlay_height(has_button, dpi_x));
        let (w, h) = (r.width(), r.height());
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, w, h, SWP_NOACTIVATE | SWP_FRAMECHANGED);
        let corner = scale(CORNER, dpi_x);
        let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, corner, corner);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: BeginPaint/EndPaint pair on our own window; always called so WM_PAINT is validated.
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    let snapshot = with_state(|s| {
        let view = s.view.clone()?;
        if s.fonts.as_ref().is_none_or(|f| f.dpi != s.dpi) {
            s.fonts = Some(Fonts::new(s.dpi));
        }
        let fonts = s.fonts.as_ref()?;
        Some((view, s.dpi, fonts.text, fonts.link))
    })
    .flatten();
    if let Some((view, dpi, text_font, link_font)) = snapshot {
        let mut rc = RECT::default();
        // SAFETY: valid out-pointer.
        if unsafe { GetClientRect(hwnd, &mut rc) }.is_ok() && rc.right > 0 && rc.bottom > 0 {
            // SAFETY: `hdc` is the paint DC for this window.
            unsafe { draw_buffered(hdc, rc.right, rc.bottom, &view, dpi, text_font, link_font) };
        }
    }
    // SAFETY: pairs with BeginPaint above.
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Renders into an off-screen bitmap and blits it in one go (no flicker).
///
/// # Safety
/// `hdc` must be a valid device context.
unsafe fn draw_buffered(hdc: HDC, w: i32, h: i32, view: &OverlayView, dpi: u32, text_font: HFONT, link_font: HFONT) {
    // SAFETY: every GDI object created here is deselected and deleted before returning.
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let bitmap = CreateCompatibleBitmap(hdc, w, h);
        let old_bitmap = SelectObject(mem, bitmap.into());
        draw(mem, w, h, view, dpi, text_font, link_font);
        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
    }
}

/// Fills `rect` with `color`.
unsafe fn fill(dc: HDC, rect: &RECT, color: COLORREF) {
    // SAFETY: brush created and deleted here.
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

/// Draws a filled rounded rectangle (or ellipse when `round` == the height) without outline.
unsafe fn round_rect(dc: HDC, r: RECT, round: i32, color: COLORREF, ellipse: bool) {
    // SAFETY: brush created, selected, deselected and deleted here; NULL_PEN is a stock object.
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(dc, brush.into());
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        // NULL_PEN draws nothing but shrinks the fill by one pixel; compensate.
        if ellipse {
            let _ = Ellipse(dc, r.left, r.top, r.right + 1, r.bottom + 1);
        } else {
            let _ = RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, round, round);
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe fn text(dc: HDC, s: &str, mut r: RECT, font: HFONT, color: COLORREF, align: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: font is a valid HFONT; the previous font is restored.
    unsafe {
        let old: HGDIOBJ = SelectObject(dc, font.into());
        SetTextColor(dc, color);
        DrawTextW(dc, &mut wide, &mut r, align | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
        SelectObject(dc, old);
    }
}

unsafe fn draw(dc: HDC, w: i32, h: i32, view: &OverlayView, dpi: u32, text_font: HFONT, link_font: HFONT) {
    let s = |v: i32| scale(v, dpi);
    let accent = tone_color(view.tone);
    let pad = s(PAD);
    let row = s(ROW);
    // Text row: top-aligned when a second element (button or level bar) sits below it.
    let row_top = if view.button.is_some() {
        s(12)
    } else if view.level.is_some() {
        s(9)
    } else {
        (h - row) / 2
    };
    let dot = s(DOT);
    let text_left = pad + dot + s(10);
    // SAFETY: `dc` is the memory DC set up by draw_buffered.
    unsafe {
        fill(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, BG);
        SetBkMode(dc, TRANSPARENT);

        let dot_top = row_top + (row - dot) / 2;
        round_rect(dc, RECT { left: pad, top: dot_top, right: pad + dot, bottom: dot_top + dot }, dot, accent, true);

        let text_rect = RECT { left: text_left, top: row_top, right: w - pad, bottom: row_top + row };
        text(dc, &view.text, text_rect, text_font, FG, DT_LEFT);

        if let Some(button) = &view.button {
            let bottom = h - s(12);
            let r = RECT { left: text_left, top: bottom - row, right: w - pad, bottom };
            text(dc, &button.label, r, link_font, accent, DT_RIGHT);
        } else if let Some(level) = view.level {
            let bar = s(BAR);
            let top = h - s(15);
            let track = RECT { left: text_left, top, right: w - pad, bottom: top + bar };
            round_rect(dc, track, bar, TRACK, false);
            let span = (track.right - track.left) as f32;
            let filled = (span * level.clamp(0.0, 1.0)).round() as i32;
            if filled > 0 {
                let fill_rect = RECT { right: track.left + filled.max(bar), ..track };
                round_rect(dc, fill_rect, bar, accent, false);
            }
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --lib platform::windows::overlay`
Expected: 5 passed, 3 ignored.

Run the window smoke tests and watch the right edge of the screen: `cargo test -p opit-speech-to-text --lib platform::windows::overlay -- --ignored --test-threads=1`
Expected: 3 passed; a dark pill shows "Listening…" with a moving level bar, then an error with an "Open settings" link, then "Pasted"; it hides by itself; keyboard focus never leaves the terminal.

Run: `cargo clippy -p opit-speech-to-text --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/platform/windows
git commit -m "feat(app): add the layered status overlay

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Tauri shell — events, window, tray, invoke API and start-up wiring

**Files:**
- Create: `crates/app/src/events.rs`, `crates/app/src/window.rs`, `crates/app/src/tray.rs`, `crates/app/src/commands.rs`
- Replace: `crates/app/src/lib.rs`, `ui/src/App.svelte`

**Interfaces:**
- Consumes: everything above.
- Produces (the contract Plan 3 builds on):
  - Events (`events.rs`): `dictation-status` → `DictationStatus` (`{ state: "idle" | "recording" | "transcribing" | "pasting" | "cancelled" | "error", can_retry, kind?, message? }`), `history-added` → row id, `config-changed` → `AppConfig`, `navigate` → route string (sent to an open window; a new window gets it as URL hash `#/<route>`).
  - Commands (`commands.rs`; JS passes camelCase argument names, e.g. `invoke("set_api_key", { keyRef, key })`; errors reject with `CommandError { code, message, kind }`):

    | Command | Arguments | Returns |
    |---|---|---|
    | `app_info` | — | `{ version, data_dir, log_dir }` |
    | `get_status` | — | `DictationStatus` |
    | `toggle_dictation` / `cancel_dictation` / `retry_dictation` | — | — |
    | `get_config` | — | `AppConfig` |
    | `save_config` | `config` | saved (normalized) `AppConfig`; emits `config-changed` |
    | `list_microphones` | — | `string[]`, default first |
    | `has_api_key` / `delete_api_key` | `keyRef` | `bool` / — |
    | `set_api_key` | `keyRef`, `key` | — |
    | `test_connection` | `profile`, `apiKey` (`null` = stored key) | — |
    | `get_user_rules` / `save_user_rules` | — / `yaml` | YAML string / `RuleWarning[]` |
    | `rules_preview` | `text`, `draftYaml?` | `{ text, hits, warnings, hallucination }` |
    | `prompt_budget` | — | `BuiltPrompt` |
    | `history_recent` | `limit`, `beforeId?` | `Dictation[]` |
    | `history_search` | `query`, `limit` | `Dictation[]` |
    | `history_delete` | `id` | — |
    | `history_clear` | — | — |
    | `get_hotkey_state` / `set_hotkey_paused` | — / `paused` | `{ paused, error }` |
    | `take_startup_notices` | — | `StartupNotice[]` (once) |
  - `window::{MAIN_WINDOW, show_main(app, route), build(app, route)}`, `tray::{TRAY_ID, create(app), refresh(app)}`, `events::{TauriEvents, config_changed, emit}`, `opit_app::{run, AUTOSTART_FLAG}`.

Tauri 2.12 facts this code relies on (verified in a scratch app):
- Building a WebView2 window inside a menu/tray handler can deadlock on Windows, so `show_main` builds on a fresh thread; in `setup` building directly is fine.
- `RunEvent::ExitRequested { code: None, .. }` is "the user closed the last window" → `prevent_exit()`; `app.exit(0)` arrives as `code: Some(0)` and must pass, so the tray Quit still works.
- `TrayIconBuilder::show_menu_on_left_click(false)` (the old `menu_on_left_click` is deprecated); left click = `TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }`. Windows flips a clicked `CheckMenuItem` by itself, so the menu is rebuilt after every change.
- `tokio::spawn` panics outside the runtime (in `setup`, tray and menu handlers), so the controller is started with `tauri::async_runtime::spawn`; inside it and in async commands tokio APIs work.
- Tray/menu updates are pushed through `run_on_main_thread`, because status changes arrive on the controller's tokio thread.
- Custom `#[tauri::command]`s need no capability entry (there is no app ACL manifest); `core:default` covers `listen`.
- Async commands that borrow `State` must return `Result`.

- [ ] **Step 1: Events, window and tray**

`crates/app/src/events.rs`:
```rust
//! Rust → web UI events. Payloads are the same serde types the commands return.

use opit_core::config::AppConfig;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::controller::{DictationStatus, UiEvents};
use crate::{tray, window};

/// Payload: `DictationStatus`.
pub const STATUS: &str = "dictation-status";
/// Payload: the new history row id (`number`).
pub const HISTORY_ADDED: &str = "history-added";
/// Payload: the saved `AppConfig`.
pub const CONFIG_CHANGED: &str = "config-changed";
/// Payload: a route name such as `"settings"`.
pub const NAVIGATE: &str = "navigate";

pub struct TauriEvents {
    app: AppHandle,
}

impl TauriEvents {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl UiEvents for TauriEvents {
    fn status_changed(&self, status: &DictationStatus) {
        emit(&self.app, STATUS, status);
        tray::refresh(&self.app);
    }

    fn history_added(&self, id: i64) {
        emit(&self.app, HISTORY_ADDED, id);
    }

    fn open_settings(&self) {
        window::show_main(&self.app, Some("settings"));
    }
}

pub fn config_changed(app: &AppHandle, config: &AppConfig) {
    emit(app, CONFIG_CHANGED, config);
    tray::refresh(app);
}

pub fn emit<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(err) = app.emit(event, payload) {
        warn!(event, error = %err, "could not emit an event");
    }
}
```

`crates/app/src/window.rs`:
```rust
//! Main window lifecycle. Closing the window destroys it (and its WebView); the app keeps
//! running in the tray and builds a fresh window when asked.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tracing::warn;

use crate::events::NAVIGATE;

pub const MAIN_WINDOW: &str = "main";

/// Focuses the main window or creates it. `route` (e.g. `"settings"`) is sent to an open
/// window as a `navigate` event, or becomes the URL hash of a new one.
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(route) = route {
            let _ = app.emit_to(MAIN_WINDOW, NAVIGATE, route);
        }
        return;
    }
    // Building a WebView2 window inside an event handler can deadlock on Windows, so the
    // build runs on its own thread (it posts to the event loop and waits there).
    let app = app.clone();
    let route = route.map(str::to_string);
    std::thread::spawn(move || {
        if let Err(err) = build(&app, route.as_deref()) {
            warn!(error = %err, "could not open the main window");
        }
    });
}

pub fn build(app: &AppHandle, route: Option<&str>) -> tauri::Result<WebviewWindow> {
    let url = match route {
        Some(route) => format!("index.html#/{route}"),
        None => "index.html".to_string(),
    };
    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App(url.into()))
        .title("Opit Speech to Text")
        .inner_size(900.0, 640.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .build()
}
```

`crates/app/src/tray.rs`:
```rust
//! Tray icon: left click toggles dictation, the right-click menu comes from `tray_menu`.

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tracing::warn;

use crate::app_core::AppCore;
use crate::controller::{DictationState, Msg};
use crate::tray_menu::{self, TrayCommand, TrayItem};
use crate::{events, window};

pub const TRAY_ID: &str = "main";

fn core(app: &AppHandle) -> Option<Arc<AppCore>> {
    app.try_state::<Arc<AppCore>>().map(|state| state.inner().clone())
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .tooltip(tooltip(app))
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_icon_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu and tooltip from the current state. Safe to call from any thread.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let result = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        match menu(&handle) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => warn!(error = %err, "could not rebuild the tray menu"),
        }
        let _ = tray.set_tooltip(Some(tooltip(&handle)));
    });
    if let Err(err) = result {
        warn!(error = %err, "could not schedule a tray refresh");
    }
}

fn tooltip(app: &AppHandle) -> String {
    let Some(core) = core(app) else {
        return "Opit Speech to Text".into();
    };
    let recording = core.status().state == DictationState::Recording;
    core.settings.current().lang.tray_tooltip(recording).to_string()
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let items = match core(app) {
        Some(core) => {
            let settings = core.settings.current();
            tray_menu::build(&settings.config, settings.lang, core.hotkey_state().paused, core.status().can_retry)
        }
        None => Vec::new(),
    };
    let kinds = items.iter().map(|item| realize(app, item)).collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn IsMenuItem<Wry>> = kinds.iter().map(|kind| kind as &dyn IsMenuItem<Wry>).collect();
    Menu::with_items(app, &refs)
}

fn realize(app: &AppHandle, item: &TrayItem) -> tauri::Result<MenuItemKind<Wry>> {
    Ok(match item {
        TrayItem::Action { id, label, enabled } => {
            MenuItemKind::MenuItem(MenuItem::with_id(app, id, label, *enabled, None::<&str>)?)
        }
        TrayItem::Check { id, label, checked } => {
            MenuItemKind::Check(CheckMenuItem::with_id(app, id, label, true, *checked, None::<&str>)?)
        }
        TrayItem::Submenu { label, items } => {
            let kinds = items.iter().map(|item| realize(app, item)).collect::<tauri::Result<Vec<_>>>()?;
            let refs: Vec<&dyn IsMenuItem<Wry>> = kinds.iter().map(|kind| kind as &dyn IsMenuItem<Wry>).collect();
            MenuItemKind::Submenu(Submenu::with_items(app, label, true, &refs)?)
        }
        TrayItem::Separator => MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
    })
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(core) = core(app) else {
        return;
    };
    match tray_menu::parse(event.id().as_ref()) {
        Some(TrayCommand::Open) => window::show_main(app, None),
        Some(TrayCommand::Profile(id)) => {
            if let Err(err) = core.set_active_profile(&id) {
                warn!(error = %err, "could not switch profile");
            }
            // Also re-syncs the check marks: Windows flips a clicked check item by itself.
            events::config_changed(app, &core.config());
        }
        Some(TrayCommand::TogglePause) => {
            core.set_hotkey_paused(!core.hotkey_state().paused);
            refresh(app);
        }
        Some(TrayCommand::Retry) => core.controller.send(Msg::Retry),
        Some(TrayCommand::Quit) => {
            core.controller.send(Msg::Shutdown);
            app.exit(0);
        }
        None => {}
    }
}

fn on_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event
        && let Some(core) = core(tray.app_handle())
    {
        core.controller.send(Msg::Toggle);
    }
}
```

- [ ] **Step 2: The invoke API**

`crates/app/src/commands.rs`:
```rust
//! Invoke API for the web UI. Plan 3 builds the pages on top of it. Each command is a thin
//! wrapper over [`AppCore`]; JS passes camelCase argument names (`keyRef` → `key_ref`).
//! Events the UI can listen to are in `events.rs`.

use std::sync::Arc;

use opit_core::config::AppConfig;
use opit_core::history::Dictation;
use opit_core::provider::Profile;
use opit_core::rules::RuleWarning;
use opit_core::rules::prompt::BuiltPrompt;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_core::{AppCore, CommandError, HotkeyState, RulesPreview};
use crate::controller::{DictationStatus, Msg};
use crate::startup::StartupNotice;
use crate::{events, tray};

type Core<'a> = State<'a, Arc<AppCore>>;
type Result<T> = std::result::Result<T, CommandError>;

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub log_dir: String,
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        app_info,
        get_status,
        toggle_dictation,
        cancel_dictation,
        retry_dictation,
        get_config,
        save_config,
        list_microphones,
        has_api_key,
        set_api_key,
        delete_api_key,
        test_connection,
        get_user_rules,
        save_user_rules,
        rules_preview,
        prompt_budget,
        history_recent,
        history_search,
        history_delete,
        history_clear,
        get_hotkey_state,
        set_hotkey_paused,
        take_startup_notices,
    ]
}

#[tauri::command]
fn app_info(core: Core<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: core.paths.root.display().to_string(),
        log_dir: core.paths.logs.display().to_string(),
    }
}

#[tauri::command]
fn get_status(core: Core<'_>) -> DictationStatus {
    core.status()
}

#[tauri::command]
fn toggle_dictation(core: Core<'_>) {
    core.controller.send(Msg::Toggle);
}

#[tauri::command]
fn cancel_dictation(core: Core<'_>) {
    core.controller.send(Msg::Cancel);
}

#[tauri::command]
fn retry_dictation(core: Core<'_>) {
    core.controller.send(Msg::Retry);
}

#[tauri::command]
fn get_config(core: Core<'_>) -> AppConfig {
    core.config()
}

#[tauri::command]
fn save_config(app: AppHandle, core: Core<'_>, config: AppConfig) -> Result<AppConfig> {
    let saved = core.save_config(config)?;
    events::config_changed(&app, &saved);
    Ok(saved)
}

/// Device enumeration can take a moment, so it runs off the main thread.
#[tauri::command]
async fn list_microphones(core: Core<'_>) -> Result<Vec<String>> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.microphones())
        .await
        .map_err(|e| CommandError::new("unavailable", e.to_string()))
}

#[tauri::command]
fn has_api_key(core: Core<'_>, key_ref: String) -> Result<bool> {
    core.has_api_key(&key_ref)
}

#[tauri::command]
fn set_api_key(core: Core<'_>, key_ref: String, key: String) -> Result<()> {
    core.set_api_key(&key_ref, &key)
}

#[tauri::command]
fn delete_api_key(core: Core<'_>, key_ref: String) -> Result<()> {
    core.delete_api_key(&key_ref)
}

/// `api_key: null` tests with the stored key.
#[tauri::command]
async fn test_connection(core: Core<'_>, profile: Profile, api_key: Option<String>) -> Result<()> {
    core.test_connection(profile, api_key).await
}

#[tauri::command]
fn get_user_rules(core: Core<'_>) -> Result<String> {
    core.user_rules_yaml()
}

#[tauri::command]
fn save_user_rules(core: Core<'_>, yaml: String) -> Result<Vec<RuleWarning>> {
    core.save_user_rules(&yaml)
}

#[tauri::command]
fn rules_preview(core: Core<'_>, text: String, draft_yaml: Option<String>) -> Result<RulesPreview> {
    core.rules_preview(&text, draft_yaml.as_deref())
}

#[tauri::command]
fn prompt_budget(core: Core<'_>) -> BuiltPrompt {
    core.prompt_budget()
}

#[tauri::command]
fn history_recent(core: Core<'_>, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>> {
    core.history_recent(limit, before_id)
}

#[tauri::command]
fn history_search(core: Core<'_>, query: String, limit: usize) -> Result<Vec<Dictation>> {
    core.history_search(&query, limit)
}

#[tauri::command]
fn history_delete(core: Core<'_>, id: i64) -> Result<()> {
    core.history_delete(id)
}

#[tauri::command]
fn history_clear(core: Core<'_>) -> Result<()> {
    core.history_clear()
}

#[tauri::command]
fn get_hotkey_state(core: Core<'_>) -> HotkeyState {
    core.hotkey_state()
}

#[tauri::command]
fn set_hotkey_paused(app: AppHandle, core: Core<'_>, paused: bool) -> HotkeyState {
    core.set_hotkey_paused(paused);
    tray::refresh(&app);
    core.hotkey_state()
}

#[tauri::command]
fn take_startup_notices(core: Core<'_>) -> Vec<StartupNotice> {
    core.take_notices()
}
```

- [ ] **Step 3: Start-up wiring**

Replace `crates/app/src/lib.rs`:
```rust
//! Opit Speech to Text desktop app: Tauri shell, platform layer and the dictation controller.

pub mod app_core;
pub mod commands;
pub mod controller;
pub mod events;
pub mod history_service;
pub mod i18n;
pub mod logging;
pub mod platform;
pub mod providers;
pub mod settings;
pub mod startup;
pub mod tray;
pub mod tray_menu;
pub mod window;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opit_core::provider::retry::RETRY_DELAY;
use tauri::{Manager, RunEvent};
use tracing::{error, info, warn};

use crate::app_core::{AppCore, Platform};
use crate::controller::{Env, HistorySink, Msg};
use crate::events::TauriEvents;
use crate::history_service::{DisabledHistory, HistoryService};
use crate::platform::windows::{
    CpalMicrophone, KeyringStore, RegistryAutostart, WinHotkey, WinOverlay, WinPaster, WinSounds,
};
use crate::platform::{Autostart, NoAutostart, NoOverlay, Overlay, SecretStore, UnavailableSecrets};
use crate::providers::HttpProviders;
use crate::settings::{Settings, SettingsHandle};
use crate::startup::Paths;

/// Windows passes this when it starts the app at sign-in; the app then starts in the tray.
pub const AUTOSTART_FLAG: &str = "--autostart";

pub fn run() {
    let start_hidden = std::env::args().any(|arg| arg == AUTOSTART_FLAG);
    let Some(paths) = Paths::from_system() else {
        eprintln!("Opit Speech to Text: the AppData folder could not be found");
        std::process::exit(1);
    };
    // Keep the guard alive for the whole run: dropping it flushes the log writer.
    let _log_guard = match logging::init(&paths.logs) {
        Ok(guard) => Some(guard),
        Err(err) => {
            eprintln!("logging disabled: {err}");
            None
        }
    };
    info!(version = env!("CARGO_PKG_VERSION"), autostart = start_hidden, "starting");

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this instance and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| window::show_main(app, None)))
        .invoke_handler(commands::handler())
        .setup(move |app| {
            setup(app, paths, start_hidden);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the Tauri application could not be built");

    app.run(|app, event| match event {
        // `code: None` = the user closed the last window: stay in the tray.
        // `app.exit(n)` arrives as `Some(n)` and is let through.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(core) = app.try_state::<Arc<AppCore>>() {
                core.controller.send(Msg::Shutdown);
            }
            info!("exiting");
        }
        _ => {}
    });
}

fn setup(app: &mut tauri::App, paths: Paths, start_hidden: bool) {
    let handle = app.handle().clone();
    let loaded = startup::load_config(&paths.config, now_ms());
    let (user_pack, rules_notice) = startup::load_user_pack(&paths.user_rules);
    let locale = i18n::system_locale();
    let config = loaded.config.clone();
    let (settings, warnings) = Settings::build(loaded.config, user_pack.clone(), locale.as_deref());
    for warning in &warnings {
        warn!(pack = %warning.pack_id, message = %warning.message, "rule skipped");
    }
    let settings = SettingsHandle::new(settings);

    let history = match HistoryService::open(&paths.history_db, paths.audio.clone()) {
        Ok(history) => {
            match history.purge_expired_audio(now_ms(), config.history.audio_retention_days) {
                Ok(0) => {}
                Ok(removed) => info!(removed, "expired audio files removed"),
                Err(err) => warn!(error = %err, "audio clean-up failed"),
            }
            Some(Arc::new(history))
        }
        Err(err) => {
            error!(error = %err, "history database unavailable; dictations will not be saved");
            None
        }
    };

    let (controller, inbox) = controller::channel();
    let overlay: Arc<dyn Overlay> = match WinOverlay::new(config.ui.overlay_position, controller.overlay_sink()) {
        Ok(overlay) => Arc::new(overlay),
        Err(err) => {
            error!(error = %err, "overlay unavailable");
            Arc::new(NoOverlay)
        }
    };
    let secrets: Arc<dyn SecretStore> = match KeyringStore::new() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            error!(error = %err, "credential store unavailable");
            Arc::new(UnavailableSecrets(err.to_string()))
        }
    };
    let autostart: Arc<dyn Autostart> = match RegistryAutostart::for_current_exe() {
        Ok(autostart) => Arc::new(autostart),
        Err(err) => Arc::new(NoAutostart(err)),
    };
    let hotkey = Arc::new(WinHotkey::new());
    let mic = Arc::new(CpalMicrophone::new());
    let sounds = Arc::new(WinSounds::new());
    sounds.warm_up();
    let history_sink: Arc<dyn HistorySink> = match &history {
        Some(history) => history.clone(),
        None => Arc::new(DisabledHistory),
    };

    let env = Env {
        settings: settings.clone(),
        providers: HttpProviders::new(secrets.clone()),
        mic: mic.clone(),
        hotkey: hotkey.clone(),
        paster: Arc::new(WinPaster::new()),
        overlay: overlay.clone(),
        sounds,
        history: history_sink,
        events: Arc::new(TauriEvents::new(handle.clone())),
        retry_delay: RETRY_DELAY,
    };
    tauri::async_runtime::spawn(controller::run(&controller, inbox, env));

    let notices = loaded.notice.into_iter().chain(rules_notice).collect();
    let platform = Platform { secrets, hotkey, mic, overlay: overlay.clone(), autostart };
    let core = Arc::new(AppCore::new(paths, settings, controller, history, platform, user_pack, locale, notices));
    core.apply_hotkey(&config);
    // Debug builds never register themselves to start with Windows.
    if !cfg!(debug_assertions) {
        core.apply_autostart(config.ui.autostart);
    }
    if let Some(view) = core.startup_overlay() {
        overlay.show(view);
    }
    app.manage(core);

    if let Err(err) = tray::create(&handle) {
        error!(error = %err, "tray icon unavailable");
    }
    if !(start_hidden || config.ui.start_in_tray)
        && let Err(err) = window::build(&handle, None)
    {
        error!(error = %err, "could not open the main window");
    }
    info!(first_run = loaded.created, "ready");
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
```

- [ ] **Step 4: UI skeleton that exercises the API**

Replace `ui/src/App.svelte`:
```svelte
<script lang="ts">
  // Plan 2 skeleton: proves the invoke/event wiring. Plan 3 replaces this with the real pages.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  interface DictationStatus {
    state: "idle" | "recording" | "transcribing" | "pasting" | "cancelled" | "error";
    can_retry: boolean;
    kind?: string;
    message?: string;
  }

  interface AppInfo {
    version: string;
    data_dir: string;
    log_dir: string;
  }

  let status = $state<DictationStatus | null>(null);
  let info = $state<AppInfo | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    let disposed = false;
    const unlisten = listen<DictationStatus>("dictation-status", (event) => {
      status = event.payload;
    });
    Promise.all([invoke<DictationStatus>("get_status"), invoke<AppInfo>("app_info")])
      .then(([s, i]) => {
        if (disposed) return;
        status ??= s;
        info = i;
      })
      .catch((e: unknown) => {
        error = String(e);
      });
    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  });

  function toggle() {
    invoke("toggle_dictation").catch((e: unknown) => {
      error = String(e);
    });
  }
</script>

<main>
  <h1>Opit Speech to Text</h1>
  {#if error}
    <p class="error">{error}</p>
  {/if}
  {#if status}
    <p>State: <strong>{status.state}</strong>{status.message ? ` — ${status.message}` : ""}</p>
    <button type="button" onclick={toggle}>{status.state === "recording" ? "Stop" : "Start"} dictation</button>
  {/if}
  {#if info}
    <p class="muted">v{info.version} · data: {info.data_dir}</p>
  {/if}
  <p class="muted">The full interface arrives in a later version. Settings live in config.json in the data folder.</p>
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    padding: 1.5rem;
  }
  .error {
    color: #b91c1c;
  }
  .muted {
    color: #6b7280;
    font-size: 0.875rem;
  }
</style>
```

Run: `cd ui; npm run check; npm run build; cd ..`
Expected: `0 ERRORS 0 WARNINGS`; build succeeds.

- [ ] **Step 5: Build, lint, test**

Run: `cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace`
Expected: no fmt diff, no clippy warnings; all tests pass (`opit-speech-to-text`: 108 lib tests + 8 ignored, 1 `logging_init`).

- [ ] **Step 6: Smoke-run against a scratch data folder**

Run (PowerShell, repo root):
```powershell
$env:OPIT_DATA_DIR = "$env:TEMP\opit-smoke"; Remove-Item -Recurse -Force $env:OPIT_DATA_DIR -ErrorAction Ignore
cargo tauri dev
```
Expected, within a few seconds:
- A tray icon appears and the window shows `State: idle`, the version and the data folder.
- `%TEMP%\opit-smoke` contains `config.json` (defaults), `history.db`, `logs\opit.<date>.log` with `starting`, `first run: default config written`, `ready first_run=true`, and no `WARN`/`ERROR` lines.
- Close the window: the process keeps running (tray icon stays). Tray right-click → Open reopens it. Tray → Quit ends the process.

Then break the config and start again:
```powershell
Set-Content "$env:OPIT_DATA_DIR\config.json" '{"profiles":[{"id":"x"}]'
cargo tauri dev
```
Expected: `config.json.bad-<ms>` next to a fresh `config.json`; the overlay flashes "Settings file was damaged; defaults loaded" (or the Turkish text on a Turkish system); the log has one `config.json is unusable` warning. While it runs, start `target\debug\opit-speech-to-text.exe` a second time: it exits immediately and the first instance's window comes to the front.

Remove the scratch folder and the variable afterwards: `Remove-Item -Recurse -Force $env:OPIT_DATA_DIR; Remove-Item Env:OPIT_DATA_DIR`.

- [ ] **Step 7: Commit**

```bash
git add crates/app/src ui/src/App.svelte
git commit -m "feat(app): wire tray, window lifecycle, invoke API and start-up

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: End-to-end manual verification, docs and roadmap

**Files:**
- Modify: `README.md` (how to run the preview with a hand-edited config)
- Modify: `docs/superpowers/plans/README.md` (Plan 2 status + outcome section)

**Interfaces:**
- Consumes: the finished app.
- Produces: a verified build and the hand-off notes Plan 3 starts from.

This task is manual: dictation needs a real microphone, a real API key and a human pressing keys. Use a **release** build for the RAM/latency checks (debug builds are much larger and slower). Record every result (pass/fail + numbers) in your notes; they go into the roadmap in Step 9.

- [ ] **Step 1: README — running the preview**

Append to `README.md`, before `## Measuring accuracy`:
````markdown
## Trying the preview

The settings UI is not built yet; the app runs from a hand-edited config.

1. Build and start it: `cargo tauri build --no-bundle`, then run
   `target\release\opit-speech-to-text.exe`. The first start writes
   `%APPDATA%\opit-speech-to-text\config.json` with defaults (Groq, Turkish, Right Ctrl +
   Right Shift, toggle mode) and puts an icon in the tray.
2. Store your API key in Windows Credential Manager (the command prompts for it, so it does
   not land in your shell history). `groq` is the profile's `api_key_ref`:
   ```
   cmdkey /generic:groq.opit-speech-to-text /user:groq /pass
   ```
3. Edit `config.json` if you like (for example `rules.prompt_context`, `rules.enabled_packs`,
   `hotkey.mode: "push_to_talk"`, `recording.microphone`) and restart the app from the tray.
4. Press **Right Ctrl + Right Shift**, speak, press it again (or tap **Ctrl** alone). The text is
   pasted where your cursor is. **Esc** cancels.

Logs are in `%APPDATA%\opit-speech-to-text\logs` (7 days). They never contain what you said.
````

- [ ] **Step 2: Build and prepare**

Run:
```powershell
cargo tauri build --no-bundle
cmdkey /generic:groq.opit-speech-to-text /user:groq /pass
```
Enter a real Groq key at the prompt. Quit any running instance, then start `target\release\opit-speech-to-text.exe`.
Expected: tray icon; the window opens; `%APPDATA%\opit-speech-to-text\config.json` exists (defaults on first run, or your existing file untouched); the log shows `ready` and no `WARN`/`ERROR`. Your personal `rules\user.yaml` from Plan 1 is picked up (no "Personal rules could not be read" overlay).

Hand-edit `config.json`: set `rules.prompt_context` to the context sentence from the header of your `user.yaml`, add `"fivem"` to `rules.enabled_packs` if you use it; save; tray → Quit; start the exe again.

- [ ] **Step 3: Core dictation (toggle mode)**

Open Notepad, put the cursor in it.
1. Press Right Ctrl + Right Shift → start sound, overlay "Dinleniyor"/"Listening" with a moving level bar; tray tooltip says recording.
2. Say "cloud code'u github'a pushla, sonra ox lib güncelle". Press the combo again → overlay "Çevriliyor…", then the text is pasted with rules applied (`Claude Code'u GitHub'a pushla, sonra …`; `ox_lib` needs the `fivem` pack) plus one trailing space; done sound; overlay "Yapıştırıldı (x,x sn)".
3. Repeat with 5 s of speech five times; note the overlay times. Expected: median < 1.5 s (spec success criterion).
4. Start again, speak, stop with a **lone Ctrl tap** (after releasing the combo) → pasted.
5. Start, speak, press **Esc** → cancel sound, "İptal edildi", nothing pasted, and Esc did not reach Notepad. Repeat, but press Esc while "Çevriliyor…" is showing → cancelled, nothing pasted later.
6. Ctrl+C / Ctrl+V in Notepad while idle must not start or stop anything.

- [ ] **Step 4: Edge cases**

1. Start and stop immediately (< 0.4 s) → "Kayıt çok kısa", nothing sent. Record 3 s of silence → "Konuşma algılanmadı", nothing sent (no request in the log).
2. Copy the word `PANO` somewhere, dictate, wait 1 s, press Ctrl+V → `PANO` is pasted (clipboard restored). Press Win+V → the dictated text is not in clipboard history.
3. Copy something **during** transcription → after the paste, your new copy is still on the clipboard.
4. Run Notepad **as administrator**, dictate into it → overlay "Panoda — Ctrl+V ile yapıştır", nothing typed; Ctrl+V pastes the text.
5. Set `recording.max_seconds` to 10, restart, dictate for 15 s → it sends by itself at 10 s. Set it back.
6. Change `hotkey.mode` to `"push_to_talk"`, restart: holding the combo records, releasing sends. Change it back.
7. Set `recording.microphone` to a wrong name, restart, dictate → overlay "Dinleniyor (varsayılan mikrofon)", works; log warns about the fallback. Unplug a USB mic mid-recording → overlay "Mikrofon kullanılamıyor", error sound, app keeps running.
8. Store a wrong key (`cmdkey /generic:groq.opit-speech-to-text /user:groq /pass`, type garbage), dictate → "API anahtarı geçersiz" with an "Ayarlar" link; clicking it opens the window. Store the right key again; tray → "Tekrar dene" is enabled and resends the kept audio (nothing re-recorded) → pasted.
9. Disconnect the network, dictate → error after the retry with a "Tekrar dene" link; reconnect, click it → pasted.
10. Delete the key (`cmdkey /delete:groq.opit-speech-to-text`), press the combo → "Bu profil için API anahtarı yok" immediately, no recording. Store it again.
11. Optional fallback: add a second profile (for example the OpenAI preset with its own `cmdkey` entry), set `fallback_profile_id` on `groq` to it, point `groq.base_url` at `http://127.0.0.1:9/v1`, dictate → text arrives from the fallback; the history row's profile is the fallback. Restore the config.

- [ ] **Step 5: Tray and window**

1. Tray left click toggles a dictation exactly like the combo.
2. Tray → Profil ▸ switches the active profile; `config.json` shows the new `active_profile_id`; the check mark follows.
3. Tray → Kısayolu duraklat → the combo does nothing; again → works.
4. Close the window → process stays; after ~10 s Task Manager shows no `msedgewebview2.exe` children under the app; the app's own memory (Details → Memory, private working set) in the tray is **< 40 MB**. Tray → Aç reopens the window.
5. Start the exe a second time → the running instance's window comes to the front; no second tray icon.
6. Tray → Çıkış ends the process.

- [ ] **Step 6: Data, logs and privacy**

1. After a few dictations, `history.db` has grown (its modified time updates after each dictation). With `history.save_audio: true` (restart), `audio\<id>.wav` files appear; with `history.enabled: false`, nothing new is written.
2. Search today's log for a distinctive word you dictated (`Select-String -Path "$env:APPDATA\opit-speech-to-text\logs\*.log" -Pattern "<word>"`) → no match. Search for your key's first 8 characters → no match.
3. Corrupt `config.json` (delete a brace), start → `config.json.bad-<ms>` appears, defaults are loaded, the overlay says so. Put your config back.
4. Put a typo in `rules\user.yaml` (for example `nme:`), start → overlay "Kişisel kurallar okunamadı"; dictation still works with the built-in packs. Fix it.

- [ ] **Step 7: Autostart (release build only)**

With `ui.autostart: true` (default) the release exe writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` → `Opit Speech to Text` = `"<exe>" --autostart`. Check it with `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v "Opit Speech to Text"`. Sign out and in → the app starts in the tray without a window. Set `ui.autostart: false`, restart → the value is gone.

- [ ] **Step 8: Run the ignored tests**

Run: `cargo test -p opit-speech-to-text -- --ignored --test-threads=1`
Expected: 8 passed (credential store, sounds, microphone, injected keys, clipboard, three overlay smokes).

- [ ] **Step 9: Update the roadmap**

In `docs/superpowers/plans/README.md`, set the Plan 2 row's status to **done** (with the branch name and the test count), and add a `## Plan 2 outcome — what Plan 3 builds on` section with:
- the command/event table from Task 14 (or a pointer to `crates/app/src/commands.rs` and `events.rs`);
- the measured numbers from Steps 3 and 5 (median latency, tray RAM);
- anything from Steps 3–7 that did not pass, as deferred findings;
- that Plan 3 may add wizard-only commands (live microphone level test, hotkey capture) on top of `AppCore`.

- [ ] **Step 10: Final checks and commit**

Run: `cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cd ui; npm run check; npm run build; cd ..`
Expected: all clean and green.

```bash
git add README.md docs/superpowers/plans/README.md
git commit -m "docs: record plan 2 verification and hand-off notes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
