# Plan 4 — Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Opit Speech to Text installable and updatable: a per-user NSIS installer whose uninstaller can delete the user's data, in-app updates from GitHub Releases through `tauri-plugin-updater` with a minisign-signed `latest.json`, a tag-triggered GitHub Actions workflow that builds a draft release, the user-facing README sections, the manual release checklist and the SignPath Foundation application notes. Nothing is published: the last task builds a signed installer locally for the user to look at.

**Architecture:** The updater runs in Rust only. A small, tested state machine (`crates/app/src/updates.rs`) wraps the plugin, three invoke commands and one event (`update-state`) expose it, and the UI shows a banner in the app shell plus an "Updates" section in Settings. The Tauri updater commands are not exposed to the WebView, so no capability changes. Installer behaviour is Tauri's NSIS template plus one hooks file (`crates/app/windows/hooks.nsh`). A Node script with `node:test` tests (`scripts/release/latest-json.mjs`) turns a finished `cargo tauri build` into release assets and `latest.json`. The release workflow and the local release build both use it.

**Tech Stack:** Rust 2024 (rust-version 1.90), Tauri 2.12.0, **tauri-plugin-updater 2.13.1** (new; needs `tauri ^2.12`, reqwest 0.13, MSRV 1.90, all matching the workspace), **tauri-cli 2.12.0** (`cargo tauri`, bundles with tauri-bundler 2.10.0); UI: Svelte 5.57, TypeScript 6.0, Vitest 5.0; Node 24 (`node:test`) for the release script; GitHub Actions `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md`. The relevant parts are §1 (installer < 15 MB, first dictation < 2 min), §2 rows *Kurulum*, *Güncelleme*, *Kod imzalama*, §7 (wizard "Windows ile başlat" checked), §9 (*Manuel yayın kontrol listesi* and the *CI* line) and §10 (data locations, "verilerimi de sil"). Read it together with `docs/superpowers/plans/README.md`, especially "Plan 3 outcome — what Plan 4 builds on" and "Behaviour Plan 4 must know about". The spec's decisions are locked.

## Global Constraints

- Names: product **Opit Speech to Text**; package/binary `opit-speech-to-text` (the bundled exe is `opit-speech-to-text.exe`); identifier `io.github.opit80.opit-speech-to-text`; data dir `%APPDATA%\opit-speech-to-text\`.
- GitHub repo: **`opit80/opit-speech-to-text`**. Updater endpoint, verbatim: **`https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json`**.
- Installer: NSIS, `installMode: "currentUser"` (no admin, no UAC; installs to `%LOCALAPPDATA%\Opit Speech to Text`). Spec §1: the installer must be **< 15 MB**.
- v1 is **not code-signed** (SmartScreen warning documented in the README). Updates **are** signed with minisign (Tauri updater key).
- **Signing key**: the private key lives only at `%USERPROFILE%\.tauri\opit-speech-to-text.key` (plus `.pub` and `.password` next to it), outside the repo. It is **never committed, never printed into a file in the repo, and never overwritten** once it exists. Only the public key goes into `crates/app/tauri.conf.json`. In CI it comes from the GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- **Tauri CLI:** this repo uses the cargo-installed CLI, `cargo tauri` (`tauri-cli 2.12.0`, the same version as the `tauri` crate in `Cargo.lock`). `ui/package.json` has no `@tauri-apps/cli`, and this plan does not add one. Run `cargo tauri …` from the repo root. If it is missing, install it with `cargo install tauri-cli --version 2.12.0 --locked`. CI installs that exact version.
- **Nothing is published:** no GitHub repo creation, no `git push`, no tags, no GitHub Release, no `gh` calls that write. Stay on branch `feat/plan-4-release`.
- Code, comments, README and docs are in **English**. Every user-facing UI string exists in **en + tr**, only in `ui/src/lib/i18n/en.ts` / `tr.ts`, with correct Turkish orthography.
- `opit-core` stays free of Tauri and Windows crates; `cargo test -p opit-core` must still pass on Linux.
- Transcript text and API keys are never logged and never put in error messages (the `--delete-credentials` helper logs nothing at all).
- Config writes from the UI go through `saveConfig` in `ui/src/lib/app.svelte.ts`; blocking Rust work never runs on the main thread.
- Checks before every commit that touches code: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; in `ui/`: `npm run check` (0 errors, 0 warnings), `npm test`, `npm run build`. From Task 5 on also `node --test scripts/release/latest-json.test.mjs`.
- Every commit message ends with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (use a second `-m`).

## Rulings on the two open questions

**`ui.autostart` vs the wizard's "Start with Windows" step.** Spec §7 has the step pre-checked, so the default stays `true`. Today, though, the release exe writes the Run value on its very first start, before the user has seen that step. Plan 4 fixes this.

Ruling: `ui.autostart` keeps its default `true`, but the app writes the Run value only when `ui.autostart && ui.setup_done` (`app_core::autostart_wanted`). Start-up applies that rule, so a fresh install removes any stale value until the wizard is finished or skipped, and finishing or skipping applies the user's choice at once. The installer never writes the Run key. Tauri's uninstaller already deletes the value named after `productName`, which is our `VALUE_NAME`. Cost if wrong: a user who closes the wizard without finishing or skipping it gets no autostart until they do. That is one click, and reverting is a one-line change in `autostart_wanted`. This also meets SignPath's rule that the software does not change system configuration without a warning.

**"Delete my data on uninstall" (spec §10).**

Ruling: "delete my data" is Tauri's own uninstaller checkbox, "Delete the application data" / "Uygulama verilerini sil". It is unchecked by default and ignored in update mode. Out of the box it removes `%APPDATA%\<identifier>` and `%LOCALAPPDATA%\<identifier>` (WebView2 data). Our `hooks.nsh` extends it to `%APPDATA%\opit-speech-to-text\` and to this app's Credential Manager entries: the uninstaller runs `opit-speech-to-text.exe --delete-credentials` while the exe still exists. When the box is left unticked, the uninstaller removes only the program, the shortcuts and the Run value, and the data stays for a reinstall. Cost if wrong: an unticked box leaves data behind (the README documents manual removal), and a mis-guarded hook would wipe data during an update. Task 1's guard test and the checklist's update line exist to catch that.

## Design decisions made in this plan

- **Updater lives in Rust.** No `@tauri-apps/plugin-updater` and no WebView permissions. Commands: `get_update_state() -> UpdateState`, `check_for_updates() -> UpdateState` and `install_update() -> ()`. Event: `update-state` with an `UpdateState` payload. Release builds check 20 s after start and then every 24 h while `ui.check_updates` is on (new `UiConfig` field, default `true`, switch in Settings → Updates). Debug builds never auto-check and refuse to install. The Settings buttons are disabled there, with the "Updates work only in the installed app" hint.
- **Install** = `Update::download_and_install`. The plugin runs the new NSIS installer with `/P /UPDATE /R` (passive progress bar, update mode, relaunch) and exits the app. It is refused while a dictation is recording, transcribing or pasting.
- **`requireSignedVersion: true` from the first release on.** tauri-cli 2.12 writes `version:<x.y.z>` into every updater signature's trusted comment (`bundle.rs` passes `settings.version_string()`), so a `latest.json` that announces a different version than the signed file is rejected. Task 7 checks the comment on the real `.sig`.
- **Release assets get fixed ASCII names** (`opit-speech-to-text_<version>_x64-setup.exe` plus `.sig`). GitHub turns spaces in asset names into dots, so the bundler's name, `Opit Speech to Text_<version>_x64-setup.exe`, would break the manifest URL.
- **Version source:** `version` under `[workspace.package]` in the root `Cargo.toml`. `tauri.conf.json` has no `version`. A release tag must be exactly `v<that version>`, and the workflow checks this before building.
- **Drafts only.** The workflow creates a draft release, and `releases/latest/…` ignores drafts, so nothing reaches users until a person publishes the draft after running `docs/RELEASE-CHECKLIST.md`.
- **Installer metadata:** `publisher: "opit80"` (it is also the HKCU registry key `Software\opit80\Opit Speech to Text`, so set it before the first release), `copyright`, `license: "MIT"`, `homepage`, `shortDescription`. There is no license page in the installer.
- **SignPath:** this plan only writes the application notes (`docs/signpath-application.md`). Applying needs a published release and the user's own accounts. Wiring SignPath into the workflow comes after acceptance and is not in this plan.

## Review Focus

1. **No release published yet, offline, or GitHub down → the update check fails, and that must stay quiet.** A failed automatic check only logs and sets `CheckFailed`. It never shows a banner, never toasts and never delays start-up. Settings → Updates shows the reason. Tests: `a_failed_check_is_kept_as_a_state_and_drops_the_old_update` (Task 3) and `bannerVersion` "stays quiet … failed checks included" (Task 4). Task 7's checklist expects "Could not fetch a valid release JSON from the remote" from **Check now** until a release exists.
2. **Installing an update while a dictation runs would kill it, and the installer must never run twice.** Rust refuses (`install_needs_an_update_and_no_running_dictation`, Task 3). The UI disables the button while `dictationBusy` (Vitest, Task 4).
3. **An update must not delete user data or the Run value.** The updater runs the installer with `/UPDATE`, and the old uninstaller can run in update mode. Every destructive hook line is guarded by both `$DeleteAppDataCheckboxState = 1` and `$UpdateMode <> 1`. Test: `destructive_uninstall_hooks_need_the_checkbox_and_skip_updates` (Task 1). Manual: checklist "Update" section (Task 6).
4. **The manifest must point at a URL that exists and at the version that was signed.** Fixed asset names, no spaces in URLs, and tag = Cargo version (`node:test`, Task 5). `requireSignedVersion` plus the trusted-comment check on the real `.sig` (Task 7).
5. **A lost, overwritten or mismatched signing key strands every installed copy.** Task 2 never regenerates an existing key. Task 7 fails if the build log contains "does not match the public key". The final report tells the user to back up the key and its password.

---

## File Map

```
crates/core/src/config.rs                 + UiConfig.check_updates (default true)               [Task 3]
crates/app/src/app_core.rs                + autostart_wanted(); save_locked applies it           [Task 1]
crates/app/src/lib.rs                     --delete-credentials early exit; autostart_wanted at   [Task 1]
                                          start-up; updater plugin                               [Task 2]
                                          UpdateService + auto-check                             [Task 3]
crates/app/src/uninstall.rs               NEW: credential_refs, delete_credentials, run          [Task 1]
crates/app/src/updates.rs                 NEW: UpdateInfo, UpdateState, UpdateMachine, UpdateService,
                                          current/check/install/spawn_auto_check                 [Task 3]
crates/app/src/commands.rs                + get_update_state, check_for_updates, install_update  [Task 3]
crates/app/src/events.rs                  + UPDATE_STATE ("update-state")                        [Task 3]
crates/app/windows/hooks.nsh              NEW: NSIS uninstall hooks                              [Task 1]
crates/app/tauri.conf.json                bundle metadata + nsis                                 [Task 1]
                                          createUpdaterArtifacts + plugins.updater               [Task 2]
crates/app/tests/tauri_conf.rs            NEW: config/hook consistency tests                     [Task 1, 2]
Cargo.toml, crates/app/Cargo.toml         + tauri-plugin-updater 2.13.1                          [Task 2]
ui/src/lib/types.ts, api.ts, events.ts    UpdateInfo/UpdateState, 3 commands, 1 event, check_updates [Task 4]
ui/src/lib/app.svelte.ts                  app.update, app.updateDismissed                        [Task 4]
ui/src/lib/update.ts (+ update.test.ts)   NEW: bannerVersion, progressPercent, statusMessage, dictationBusy [Task 4]
ui/src/lib/components/UpdateInstallButton.svelte, UpdateProgress.svelte  NEW                     [Task 4]
ui/src/App.svelte, ui/src/pages/Settings.svelte, ui/src/lib/i18n/en.ts, tr.ts                    [Task 4]
scripts/release/latest-json.mjs (+ .test.mjs)  NEW                                               [Task 5]
.github/workflows/release.yml             NEW                                                    [Task 5]
.github/workflows/ci.yml                  + release-scripts job                                  [Task 5]
.gitignore                                + /release-assets/                                     [Task 5]
README.md                                 Install, SmartScreen, Update, Uninstall, Privacy, Releasing [Task 6]
docs/RELEASE-CHECKLIST.md                 NEW                                                    [Task 6]
docs/signpath-application.md              NEW                                                    [Task 6]
docs/superpowers/plans/README.md          Plan 4 row + "Plan 4 outcome"                          [Task 7]
%USERPROFILE%\.tauri\opit-speech-to-text.key{,.pub,.password}   OUTSIDE the repo                 [Task 2]
```

## UI contract added by this plan

| Command | Args | Returns | Notes |
|---|---|---|---|
| `get_update_state` | — | `UpdateState` | |
| `check_for_updates` | — | `UpdateState` | runs one check (or returns the current state while one runs); emits `update-state` |
| `install_update` | — | — | rejects `unavailable` (no update / dictation running / debug build) or `update` (download or launch failed). On success the app exits |

| Event | Payload |
|---|---|
| `update-state` | `UpdateState` = `{kind:"idle"}` \| `{kind:"checking"}` \| `{kind:"up_to_date"}` \| `{kind:"available", info}` \| `{kind:"installing", info, downloaded, total: number\|null}` \| `{kind:"check_failed", message}`; `info` = `{ version, current_version, notes: string\|null }` |

`AppConfig.ui` gains `check_updates: boolean`.

---

### Task 1: Installer — per-user NSIS, autostart after the wizard, "delete my data" on uninstall

**Files:**
- Modify: `crates/app/src/app_core.rs` (new `autostart_wanted`, `save_locked`, tests)
- Modify: `crates/app/src/lib.rs` (`pub mod uninstall;`, early `--delete-credentials` exit, start-up autostart)
- Create: `crates/app/src/uninstall.rs`
- Create: `crates/app/windows/hooks.nsh`
- Modify: `crates/app/tauri.conf.json` (`bundle` metadata + `bundle.windows.nsis`)
- Create: `crates/app/tests/tauri_conf.rs`

**Interfaces:**
- Consumes: `AppCore::{save_config, update_config, apply_autostart}`, `platform::{SecretStore, Autostart}`, `platform::windows::{KeyringStore, autostart::VALUE_NAME}`, `startup::Paths::from_system`, `opit_core::provider::presets::{groq, openai}`, `opit_core::config::APP_DIR_NAME`, `platform::fake::{FakeSecrets, FakeAutostart}`.
- Produces:
  - `pub fn app_core::autostart_wanted(config: &AppConfig) -> bool`
  - `pub const uninstall::DELETE_CREDENTIALS_FLAG: &str = "--delete-credentials"`
  - `pub fn uninstall::credential_refs(config_json: Option<&str>) -> Vec<String>`
  - `pub fn uninstall::delete_credentials(store: &dyn SecretStore, refs: &[String]) -> usize`
  - `pub fn uninstall::run() -> i32`
  - `crates/app/tests/tauri_conf.rs` with helpers `conf() -> serde_json::Value` and `HOOKS: &str` (Task 2 appends a test)

- [ ] **Step 1: Write the failing tests**

In `crates/app/src/app_core.rs` tests, edit `save_config_normalizes_writes_and_applies_side_effects`: replace

```rust
        config.ui.autostart = false;
        *f.autostart.enabled.lock().unwrap() = true;
```

with

```rust
        config.ui.setup_done = true;
```

and replace its last line `assert!(!*f.autostart.enabled.lock().unwrap());` with `assert!(*f.autostart.enabled.lock().unwrap(), "finishing the wizard applies the default autostart");`. Then add:

```rust
    #[test]
    fn autostart_needs_both_the_setting_and_a_finished_wizard() {
        let mut config = AppConfig::default();
        assert!(config.ui.autostart && !config.ui.setup_done, "defaults: on, wizard not done");
        assert!(!autostart_wanted(&config));
        config.ui.setup_done = true;
        assert!(autostart_wanted(&config));
        config.ui.autostart = false;
        assert!(!autostart_wanted(&config));
    }

    #[test]
    fn autostart_waits_until_the_wizard_is_finished_or_skipped() {
        let f = fixture();
        f.core
            .update_config(|c| {
                c.paste.trailing_space = false;
                Ok(())
            })
            .unwrap();
        assert!(!*f.autostart.enabled.lock().unwrap(), "nothing is registered before the wizard ends");

        f.core
            .update_config(|c| {
                c.ui.setup_done = true;
                Ok(())
            })
            .unwrap();
        assert!(*f.autostart.enabled.lock().unwrap(), "finish/skip applies the (default) choice");

        f.core
            .update_config(|c| {
                c.ui.autostart = false;
                Ok(())
            })
            .unwrap();
        assert!(!*f.autostart.enabled.lock().unwrap());
    }

    #[test]
    fn a_wizard_that_turned_autostart_off_never_registers_it() {
        let f = fixture();
        f.core
            .update_config(|c| {
                c.ui.autostart = false;
                Ok(())
            })
            .unwrap();
        f.core
            .update_config(|c| {
                c.ui.setup_done = true;
                Ok(())
            })
            .unwrap();
        assert!(!*f.autostart.enabled.lock().unwrap());
    }
```

Create `crates/app/src/uninstall.rs` with only the tests first (plus `pub mod uninstall;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::FakeSecrets;

    #[test]
    fn refs_come_from_the_presets_and_every_profile() {
        let json = r#"{"profiles":[
            {"id":"groq","api_key_ref":"groq"},
            {"id":"home","api_key_ref":" home-gpu "},
            {"id":"local","api_key_ref":null},
            {"id":"blank","api_key_ref":""}
        ]}"#;
        assert_eq!(credential_refs(Some(json)), ["groq", "home-gpu", "openai"]);
    }

    #[test]
    fn a_missing_or_damaged_config_still_gives_the_preset_refs() {
        assert_eq!(credential_refs(None), ["groq", "openai"]);
        assert_eq!(credential_refs(Some("{damaged")), ["groq", "openai"]);
        assert_eq!(credential_refs(Some(r#"{"profiles":"nope"}"#)), ["groq", "openai"]);
        assert_eq!(credential_refs(Some(r#"{"schema_version":99,"profiles":[{"api_key_ref":"new"}]}"#)), [
            "groq", "new", "openai"
        ]);
    }

    #[test]
    fn deletes_only_this_apps_refs() {
        let store = FakeSecrets::with("groq", "k1");
        store.set("home-gpu", "k2").unwrap();
        store.set("someone-else", "k3").unwrap();
        let refs = credential_refs(Some(r#"{"profiles":[{"api_key_ref":"home-gpu"}]}"#));
        assert_eq!(delete_credentials(&store, &refs), 0);
        assert_eq!(store.map.lock().unwrap().keys().collect::<Vec<_>>(), ["someone-else"]);
    }
}
```

Create `crates/app/tests/tauri_conf.rs`:

```rust
//! The installer, the uninstaller hooks and the app must agree on names and paths.

use opit_app::platform::windows::autostart::VALUE_NAME;

const CONF: &str = include_str!("../tauri.conf.json");
const HOOKS: &str = include_str!("../windows/hooks.nsh");

fn conf() -> serde_json::Value {
    serde_json::from_str(CONF).expect("tauri.conf.json is valid JSON")
}

#[test]
fn the_uninstaller_removes_the_run_value_the_app_writes() {
    // Tauri's uninstaller deletes HKCU\...\Run\<productName> (outside update mode).
    assert_eq!(conf()["productName"], VALUE_NAME);
}

#[test]
fn the_installer_is_per_user_in_english_and_turkish() {
    let conf = conf();
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(conf["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(nsis["installMode"], "currentUser");
    assert_eq!(nsis["languages"], serde_json::json!(["English", "Turkish"]));
    assert_eq!(nsis["installerHooks"], "windows/hooks.nsh");
    assert_eq!(conf["bundle"]["publisher"], "opit80");
}

#[test]
fn destructive_uninstall_hooks_need_the_checkbox_and_skip_updates() {
    let mut destructive = 0;
    for block in HOOKS.split("!macro ").skip(1) {
        let body = block.split("!macroend").next().unwrap();
        if ["RmDir", "ExecWait", "DeleteRegValue", "DeleteRegKey"].iter().any(|cmd| body.contains(cmd)) {
            destructive += 1;
            assert!(body.contains("${If} $DeleteAppDataCheckboxState = 1"), "unguarded hook: {block}");
            assert!(body.contains("${AndIf} $UpdateMode <> 1"), "hook runs during updates: {block}");
        }
    }
    assert_eq!(destructive, 2, "PREUNINSTALL (credentials) and POSTUNINSTALL (data folder)");
}

#[test]
fn the_hooks_target_the_apps_data_folder_and_cleanup_flag() {
    // RmDir /r "$APPDATA\opit-speech-to-text"
    let data = format!("RmDir /r \"$APPDATA\\{}\"", opit_core::config::APP_DIR_NAME);
    // "$INSTDIR\${MAINBINARYNAME}.exe" --delete-credentials
    let cleanup = format!("\"$INSTDIR\\${{MAINBINARYNAME}}.exe\" {}", opit_app::uninstall::DELETE_CREDENTIALS_FLAG);
    assert!(HOOKS.contains(&data), "missing: {data}");
    assert!(HOOKS.contains(&cleanup), "missing: {cleanup}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p opit-speech-to-text autostart` and `cargo test -p opit-speech-to-text --test tauri_conf`.
Expected: compile errors (`autostart_wanted`, `credential_refs`, `windows/hooks.nsh` not found).

- [ ] **Step 3: Implement**

`crates/app/src/app_core.rs`. Add next to `apply_autostart`, as a free function:

```rust
/// Start with Windows only once the first-run wizard was finished or skipped, so a fresh install
/// never registers itself before the user has seen the choice (spec §7, Plan 4 ruling).
pub fn autostart_wanted(config: &AppConfig) -> bool {
    config.ui.autostart && config.ui.setup_done
}
```

In `save_locked` replace

```rust
        if old.ui.autostart != config.ui.autostart {
            self.apply_autostart(config.ui.autostart);
        }
```

with

```rust
        if autostart_wanted(&old) != autostart_wanted(&config) {
            self.apply_autostart(autostart_wanted(&config));
        }
```

`crates/app/src/lib.rs`: add `pub mod uninstall;`. As the first statement of `run()`:

```rust
    // The uninstaller's clean-up (windows/hooks.nsh): no window, no tray, no log file.
    if std::env::args().any(|arg| arg == uninstall::DELETE_CREDENTIALS_FLAG) {
        std::process::exit(uninstall::run());
    }
```

In `setup()` replace `core.apply_autostart(config.ui.autostart);` with `core.apply_autostart(app_core::autostart_wanted(&config));`. Start-up still re-applies every run, which keeps the Run command pointing at the current exe and removes a stale value while the wizard is not done.

`crates/app/src/uninstall.rs` (above the tests):

```rust
//! `opit-speech-to-text.exe --delete-credentials`, run by the NSIS uninstaller
//! (`crates/app/windows/hooks.nsh`) when "Delete the application data" is ticked. The uninstaller
//! deletes the data folder itself; API keys live in Windows Credential Manager, which only the app
//! knows how to address.

use std::collections::BTreeSet;

use opit_core::provider::presets;

use crate::platform::SecretStore;
use crate::platform::windows::KeyringStore;
use crate::startup::Paths;

/// Command-line flag the uninstaller passes. `run()` in lib.rs handles it before anything starts.
pub const DELETE_CREDENTIALS_FLAG: &str = "--delete-credentials";

/// Every key ref this app may have stored: the presets' refs plus each profile's `api_key_ref` in
/// `config.json`. The file is read as plain JSON (no schema, no migration), so a damaged or newer
/// config still gives up its refs.
pub fn credential_refs(config_json: Option<&str>) -> Vec<String> {
    let mut refs: BTreeSet<String> =
        [presets::groq(), presets::openai()].into_iter().filter_map(|profile| profile.api_key_ref).collect();
    let config = config_json.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let profiles = config.as_ref().and_then(|c| c.get("profiles")).and_then(|p| p.as_array());
    for profile in profiles.into_iter().flatten() {
        let key_ref = profile.get("api_key_ref").and_then(|r| r.as_str()).map(str::trim);
        if let Some(key_ref) = key_ref.filter(|r| !r.is_empty()) {
            refs.insert(key_ref.to_string());
        }
    }
    refs.into_iter().collect()
}

/// Deletes each ref; a missing entry counts as deleted. Returns how many deletions failed.
pub fn delete_credentials(store: &dyn SecretStore, refs: &[String]) -> usize {
    refs.iter().filter(|key_ref| store.delete(key_ref).is_err()).count()
}

/// Exit code 0 when every entry is gone, 1 otherwise. Logs nothing: logging would recreate the
/// data folder the uninstaller is about to delete.
pub fn run() -> i32 {
    let config = Paths::from_system().and_then(|paths| std::fs::read_to_string(paths.config).ok());
    let refs = credential_refs(config.as_deref());
    match KeyringStore::new() {
        Ok(store) if delete_credentials(&store, &refs) == 0 => 0,
        _ => 1,
    }
}
```

Create `crates/app/windows/hooks.nsh`:

```nsis
; NSIS hooks for the Opit Speech to Text installer
; (tauri.conf.json > bundle > windows > nsis > installerHooks).
;
; Tauri's uninstaller already removes the HKCU Run value named after productName and, when
; "Delete the application data" is ticked, %APPDATA%\<identifier> and %LOCALAPPDATA%\<identifier>
; (WebView2 data). Our own data lives in %APPDATA%\opit-speech-to-text and the API keys in
; Windows Credential Manager, so the same checkbox removes those too.
; Never in update mode: the updater runs the installer with /UPDATE and the data must survive it.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; The exe is still in place here. It deletes this app's Credential Manager entries and exits.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-credentials'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\opit-speech-to-text"
  ${EndIf}
!macroend
```

(`$DeleteAppDataCheckboxState`, `$UpdateMode` and `${MAINBINARYNAME}` come from tauri-bundler 2.10.0's `installer.nsi`. The confirm page sets the checkbox state before `Section Uninstall` runs the PRE hook. `CheckIfAppIsRunning` runs after the PRE hook. Our helper exits before Tauri starts, so it never trips that check.)

`crates/app/tauri.conf.json`: replace the `bundle` object with:

```json
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "publisher": "opit80",
    "copyright": "Copyright (c) 2026 opit80",
    "license": "MIT",
    "homepage": "https://github.com/opit80/opit-speech-to-text",
    "shortDescription": "Voice dictation for Windows with your own API key",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.ico"],
    "windows": {
      "nsis": {
        "installMode": "currentUser",
        "languages": ["English", "Turkish"],
        "installerHooks": "windows/hooks.nsh"
      }
    }
  }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text` → all pass (the new `app_core`, `uninstall` and `tauri_conf` tests included).

- [ ] **Step 5: Build the installer once to compile the hooks** (no signing key exists yet, so this is an unsigned bundle):

```powershell
cargo tauri build
```

Expected: `target\release\bundle\nsis\Opit Speech to Text_0.1.0_x64-setup.exe` exists, and NSIS reports no errors or warnings about `hooks.nsh`. The first run downloads NSIS and `nsis_tauri_utils` into `%LOCALAPPDATA%\tauri` and needs network. If the bundler cannot find the hooks file, `installerHooks` is resolved relative to `crates/app/`, so check the path and do not move the file. Do **not** run the installer.

- [ ] **Step 6: Full checks**

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ui; npm run check; npm test; npm run build; cd ..
```

- [ ] **Step 7: Commit**

```powershell
git add crates/app/src/app_core.rs crates/app/src/lib.rs crates/app/src/uninstall.rs crates/app/windows/hooks.nsh crates/app/tauri.conf.json crates/app/tests/tauri_conf.rs
git commit -m "feat(installer): per-user NSIS installer, autostart after the wizard, delete-my-data on uninstall" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Updater signing key and plugin configuration

**Files:**
- Outside the repo: `%USERPROFILE%\.tauri\opit-speech-to-text.key`, `.key.pub`, `.key.password`
- Modify: `Cargo.toml` (workspace dependency), `crates/app/Cargo.toml`
- Modify: `crates/app/tauri.conf.json` (`bundle.createUpdaterArtifacts`, `plugins.updater`)
- Modify: `crates/app/src/lib.rs` (register the plugin)
- Modify: `crates/app/tests/tauri_conf.rs` (one test)

**Interfaces:**
- Consumes: `conf()` from Task 1's `tests/tauri_conf.rs`.
- Produces: the updater plugin registered on the Tauri builder (`tauri_plugin_updater::UpdaterExt::updater()` works on any `AppHandle`); public key in `tauri.conf.json`; key files at the paths above, which Task 7 uses as `TAURI_SIGNING_PRIVATE_KEY` (a file path is accepted) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

- [ ] **Step 1: Generate the keypair (once, never overwrite)** in PowerShell:

```powershell
$dir = Join-Path $env:USERPROFILE ".tauri"
$key = Join-Path $dir "opit-speech-to-text.key"
if (Test-Path $key) {
  Write-Host "Key already exists at $key; reusing it (never regenerate: installed apps trust its public key)."
} else {
  New-Item -ItemType Directory -Force $dir | Out-Null
  $password = [Convert]::ToHexString([System.Security.Cryptography.RandomNumberGenerator]::GetBytes(24))
  Set-Content -Path "$key.password" -Value $password -NoNewline
  cargo tauri signer generate --ci -w $key -p $password
}
Get-Item $key, "$key.pub", "$key.password" | Select-Object FullName, Length
```

Expected: three files exist. **Do not** print the private key or the password into any file inside the repo, a commit message or the task report. Then confirm with `git status --short` that nothing under `%USERPROFILE%` is involved (it is outside the repo).

- [ ] **Step 2: Write the failing test.** Append to `crates/app/tests/tauri_conf.rs`:

```rust
#[test]
fn the_updater_reads_the_github_manifest_and_requires_signed_versions() {
    let conf = conf();
    let updater: tauri_plugin_updater::Config =
        serde_json::from_value(conf["plugins"]["updater"].clone()).expect("plugins.updater parses");
    let endpoints: Vec<&str> = updater.endpoints.iter().map(|url| url.as_str()).collect();
    assert_eq!(endpoints, ["https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json"]);
    assert!(updater.require_signed_version);
    assert!(!updater.allow_downgrades);
    assert_eq!(updater.windows.as_ref().unwrap().install_mode.to_string(), "passive");
    assert!(
        updater.pubkey.len() > 40 && !updater.pubkey.contains(char::is_whitespace),
        "paste the single base64 line from %USERPROFILE%\\.tauri\\opit-speech-to-text.key.pub"
    );
    assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
}
```

Add the dependency first, or the test does not compile. Root `Cargo.toml` `[workspace.dependencies]`, keeping alphabetical order:

```toml
tauri-plugin-updater = "2.13.1"
```

`crates/app/Cargo.toml` `[dependencies]`:

```toml
tauri-plugin-updater.workspace = true
```

Run: `cargo test -p opit-speech-to-text --test tauri_conf` → FAIL (`plugins` is missing).

- [ ] **Step 3: Configure.** In `crates/app/tauri.conf.json` add `"createUpdaterArtifacts": true,` right after `"targets": ["nsis"],` in `bundle`, and a top-level `plugins` object after `bundle`:

```json
  "plugins": {
    "updater": {
      "pubkey": "<the single line from %USERPROFILE%\\.tauri\\opit-speech-to-text.key.pub, verbatim>",
      "endpoints": ["https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json"],
      "requireSignedVersion": true,
      "windows": { "installMode": "passive" }
    }
  }
```

Read the `.pub` file with `Get-Content "$env:USERPROFILE\.tauri\opit-speech-to-text.key.pub" -Raw` and paste its content without the trailing newline. The `.pub` file holds only the public key, which is safe to commit.

Register the plugin in `crates/app/src/lib.rs`, right after the single-instance plugin:

```rust
        // In-app updates (updates.rs). Its commands are not exposed to the WebView.
        .plugin(tauri_plugin_updater::Builder::new().build())
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text --test tauri_conf` → PASS. Then `cargo run -p opit-speech-to-text` with a scratch `OPIT_DATA_DIR` must still start (the plugin parses its config at start-up). Close it from the tray.

- [ ] **Step 5: Full checks** (same commands as Task 1 Step 6). Note: from now on `cargo tauri build` with bundling needs `TAURI_SIGNING_PRIVATE_KEY`. Use `--no-bundle` or `--no-sign` for local builds without the key (Task 6 documents this).

- [ ] **Step 6: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/app/Cargo.toml crates/app/tauri.conf.json crates/app/src/lib.rs crates/app/tests/tauri_conf.rs
git commit -m "feat(updater): updater public key and tauri-plugin-updater config" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: App — update check and install service

**Files:**
- Modify: `crates/core/src/config.rs` (`UiConfig.check_updates`)
- Create: `crates/app/src/updates.rs`
- Modify: `crates/app/src/events.rs` (`UPDATE_STATE`)
- Modify: `crates/app/src/commands.rs` (3 commands)
- Modify: `crates/app/src/lib.rs` (`pub mod updates;`, manage `UpdateService`, auto-check)

**Interfaces:**
- Consumes: the plugin registered in Task 2 (`tauri_plugin_updater::{Update, UpdaterExt}`); `CommandError::new`; `events::emit`; `SettingsHandle::current`; `AppCore::status`; `controller::DictationState`.
- Produces:
  - `UiConfig.check_updates: bool` (serde default `true`)
  - `updates::{UpdateInfo, UpdateState, UpdateMachine<T>, UpdateService}`; `updates::{current(&AppHandle) -> UpdateState, check(&AppHandle) -> UpdateState (async), install(&AppHandle, dictation_busy: bool) -> Result<(), CommandError> (async), spawn_auto_check(AppHandle, SettingsHandle)}`
  - `events::UPDATE_STATE = "update-state"`
  - Commands `get_update_state`, `check_for_updates`, `install_update` (see "UI contract added by this plan")

- [ ] **Step 1: Write the failing core test** in `crates/core/src/config.rs` tests:

```rust
    #[test]
    fn update_checks_default_on_also_for_older_configs() {
        assert!(AppConfig::default().ui.check_updates);
        let c = AppConfig::from_json(r#"{"schema_version":1,"ui":{"start_in_tray":true}}"#).unwrap();
        assert!(c.ui.check_updates);
    }
```

Run `cargo test -p opit-core update_checks` → FAIL (no field).

- [ ] **Step 2: Implement it.** In `UiConfig` add, after `setup_done`:

```rust
    /// Ask GitHub for a newer version at start-up and once a day (release builds only).
    pub check_updates: bool,
```

and `check_updates: true,` in `impl Default for UiConfig`. Fix any `UiConfig { … }` struct literal that no longer compiles by adding the field. Run `cargo test -p opit-core` → PASS.

- [ ] **Step 3: Write the failing state-machine tests.** Create `crates/app/src/updates.rs` with the tests module below (and `pub mod updates;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn info(version: &str) -> UpdateInfo {
        UpdateInfo { version: version.into(), current_version: "0.1.0".into(), notes: None }
    }

    fn available() -> UpdateMachine<&'static str> {
        let mut m = UpdateMachine::default();
        assert!(m.begin_check());
        m.finish_check(Ok(Some((info("0.2.0"), "update"))));
        m
    }

    #[test]
    fn a_check_runs_once_at_a_time_and_ends_in_a_result() {
        let mut m = UpdateMachine::<&str>::default();
        assert_eq!(m.state(), UpdateState::Idle);
        assert!(m.begin_check());
        assert!(!m.begin_check(), "a second check waits for the first");
        assert_eq!(m.state(), UpdateState::Checking);
        m.finish_check(Ok(None));
        assert_eq!(m.state(), UpdateState::UpToDate);
        assert!(m.begin_check());
        m.finish_check(Ok(Some((info("0.2.0"), "update"))));
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
    }

    #[test]
    fn a_failed_check_is_kept_as_a_state_and_drops_the_old_update() {
        let mut m = available();
        assert!(m.begin_check());
        m.finish_check(Err("Could not fetch a valid release JSON from the remote".into()));
        assert_eq!(
            m.state(),
            UpdateState::CheckFailed { message: "Could not fetch a valid release JSON from the remote".into() }
        );
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable");
    }

    #[test]
    fn install_needs_an_update_and_no_running_dictation() {
        let mut m = UpdateMachine::<&str>::default();
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable");

        let mut m = available();
        let err = m.begin_install(true).unwrap_err();
        assert!(err.message.contains("dictation"), "{}", err.message);
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") }, "a refusal changes nothing");

        assert_eq!(m.begin_install(false).unwrap(), "update");
        assert_eq!(m.state(), UpdateState::Installing { info: info("0.2.0"), downloaded: 0, total: None });
        assert!(!m.begin_check(), "no check while installing");
        assert_eq!(m.begin_install(false).unwrap_err().code, "unavailable", "no second install");
    }

    #[test]
    fn progress_is_reported_once_per_whole_percent() {
        let mut m = available();
        m.begin_install(false).unwrap();
        let reports = (0..1000).filter(|_| m.progress(1, Some(1000))).count();
        assert_eq!(reports, 101, "marks 0..=100");
        assert_eq!(m.state(), UpdateState::Installing { info: info("0.2.0"), downloaded: 1000, total: Some(1000) });
    }

    #[test]
    fn progress_without_a_size_is_reported_per_mib() {
        let mut m = available();
        m.begin_install(false).unwrap();
        let reports = (0..8).filter(|_| m.progress(512 * 1024, None)).count();
        assert_eq!(reports, 5, "0.5 … 4 MiB → marks 0, 1, 2, 3, 4");
    }

    #[test]
    fn a_failed_install_can_be_retried() {
        let mut m = available();
        m.begin_install(false).unwrap();
        m.progress(10, Some(100));
        m.install_failed();
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
        assert_eq!(m.begin_install(false).unwrap(), "update");
        assert!(m.progress(1, Some(100)), "progress starts over");
    }

    #[test]
    fn progress_outside_an_install_is_ignored() {
        let mut m = available();
        assert!(!m.progress(10, Some(100)));
        assert_eq!(m.state(), UpdateState::Available { info: info("0.2.0") });
    }

    #[test]
    fn the_ui_payload_shape() {
        use serde_json::json;
        assert_eq!(serde_json::to_value(UpdateState::UpToDate).unwrap(), json!({"kind": "up_to_date"}));
        assert_eq!(
            serde_json::to_value(UpdateState::Available { info: info("0.2.0") }).unwrap(),
            json!({"kind": "available", "info": {"version": "0.2.0", "current_version": "0.1.0", "notes": null}})
        );
        let installing =
            serde_json::to_value(UpdateState::Installing { info: info("0.2.0"), downloaded: 5, total: None }).unwrap();
        assert_eq!(installing["kind"], "installing");
        assert_eq!(installing["downloaded"], 5);
        assert_eq!(installing["total"], serde_json::Value::Null);
        assert_eq!(
            serde_json::to_value(UpdateState::CheckFailed { message: "x".into() }).unwrap(),
            json!({"kind": "check_failed", "message": "x"})
        );
    }
}
```

Run `cargo test -p opit-speech-to-text updates` → compile FAIL.

- [ ] **Step 4: Implement `updates.rs`** (above the tests):

```rust
//! In-app updates. `tauri-plugin-updater` reads the GitHub Releases manifest (`plugins > updater`
//! in tauri.conf.json), verifies the minisign signature and runs the new NSIS installer in passive
//! mode. [`UpdateMachine`] holds what the UI shows and is tested on its own; the functions below
//! wire it to the plugin and the `update-state` event.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tracing::{info, warn};

use crate::app_core::CommandError;
use crate::events;
use crate::settings::SettingsHandle;

/// The first automatic check waits until start-up has settled.
pub const AUTO_CHECK_DELAY: Duration = Duration::from_secs(20);
/// Then the app checks once a day while it keeps running in the tray.
pub const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

impl From<&Update> for UpdateInfo {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
        }
    }
}

/// What the UI shows; payload of the `update-state` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateState {
    /// No check in this run yet.
    Idle,
    Checking,
    UpToDate,
    Available { info: UpdateInfo },
    /// Downloading. The app exits when the installer starts.
    Installing { info: UpdateInfo, downloaded: u64, total: Option<u64> },
    /// Offline, no release published yet, a bad manifest … Shown only on Settings → Updates.
    CheckFailed { message: String },
}

/// The update state plus the pending update `T` (the plugin's `Update`; a plain value in tests).
pub struct UpdateMachine<T> {
    state: UpdateState,
    pending: Option<T>,
    /// Last progress mark sent to the UI: whole percent, or MiB while the size is unknown.
    reported: Option<u64>,
}

impl<T> Default for UpdateMachine<T> {
    fn default() -> Self {
        Self { state: UpdateState::Idle, pending: None, reported: None }
    }
}

impl<T: Clone> UpdateMachine<T> {
    pub fn state(&self) -> UpdateState {
        self.state.clone()
    }

    /// True when the caller should check now; false while a check or an install is running.
    pub fn begin_check(&mut self) -> bool {
        if matches!(self.state, UpdateState::Checking | UpdateState::Installing { .. }) {
            return false;
        }
        self.state = UpdateState::Checking;
        true
    }

    pub fn finish_check(&mut self, result: Result<Option<(UpdateInfo, T)>, String>) {
        let (state, pending) = match result {
            Ok(Some((info, update))) => (UpdateState::Available { info }, Some(update)),
            Ok(None) => (UpdateState::UpToDate, None),
            Err(message) => (UpdateState::CheckFailed { message }, None),
        };
        self.state = state;
        self.pending = pending;
    }

    /// Hands out the pending update for installing. Refused without one, and while a dictation is
    /// in flight (the installer closes the app).
    pub fn begin_install(&mut self, dictation_busy: bool) -> Result<T, CommandError> {
        let (UpdateState::Available { info }, Some(update)) = (&self.state, &self.pending) else {
            return Err(CommandError::new("unavailable", "there is no update to install; check for updates first"));
        };
        if dictation_busy {
            return Err(CommandError::new("unavailable", "finish the current dictation first"));
        }
        let (info, update) = (info.clone(), update.clone());
        self.state = UpdateState::Installing { info, downloaded: 0, total: None };
        self.reported = None;
        Ok(update)
    }

    /// Adds a downloaded chunk. True when the UI should hear about it.
    pub fn progress(&mut self, chunk: u64, total: Option<u64>) -> bool {
        let UpdateState::Installing { downloaded, total: size, .. } = &mut self.state else {
            return false;
        };
        *downloaded += chunk;
        if total.is_some() {
            *size = total;
        }
        let mark = match *size {
            Some(size) if size > 0 => (*downloaded).min(size) * 100 / size,
            _ => *downloaded >> 20,
        };
        if self.reported == Some(mark) {
            return false;
        }
        self.reported = Some(mark);
        true
    }

    /// The download or the installer launch failed; the same update can be tried again.
    pub fn install_failed(&mut self) {
        if let UpdateState::Installing { info, .. } = &self.state {
            let info = info.clone();
            self.state = UpdateState::Available { info };
        }
    }
}

/// Managed Tauri state.
#[derive(Default)]
pub struct UpdateService(Mutex<UpdateMachine<Update>>);

impl UpdateService {
    fn with<R>(&self, f: impl FnOnce(&mut UpdateMachine<Update>) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn state(&self) -> UpdateState {
        self.with(|m| m.state())
    }
}

pub fn current(app: &AppHandle) -> UpdateState {
    app.state::<UpdateService>().state()
}

fn publish(app: &AppHandle) -> UpdateState {
    let state = current(app);
    events::emit(app, events::UPDATE_STATE, state.clone());
    state
}

/// Runs one check, or returns the current state while a check or an install is running.
pub async fn check(app: &AppHandle) -> UpdateState {
    if !app.state::<UpdateService>().with(|m| m.begin_check()) {
        return current(app);
    }
    publish(app);
    let found = match app.updater() {
        Ok(updater) => updater.check().await.map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    match &found {
        Ok(Some(update)) => info!(version = %update.version, "update available"),
        Ok(None) => info!("the app is up to date"),
        Err(err) => warn!(error = %err, "update check failed"),
    }
    let result = found.map(|found| found.map(|update| (UpdateInfo::from(&update), update)));
    app.state::<UpdateService>().with(|m| m.finish_check(result));
    publish(app)
}

/// Downloads, verifies and starts the installer. On Windows a successful install never returns:
/// the plugin starts the installer and exits the process.
pub async fn install(app: &AppHandle, dictation_busy: bool) -> Result<(), CommandError> {
    if cfg!(debug_assertions) {
        return Err(CommandError::new("unavailable", "only the installed app can update itself"));
    }
    let update = app.state::<UpdateService>().with(|m| m.begin_install(dictation_busy))?;
    publish(app);
    info!(version = %update.version, "downloading the update");
    let progress_app = app.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                if progress_app.state::<UpdateService>().with(|m| m.progress(chunk as u64, total)) {
                    publish(&progress_app);
                }
            },
            || info!("update downloaded; starting the installer"),
        )
        .await;
    if let Err(err) = result {
        warn!(error = %err, "update install failed");
        app.state::<UpdateService>().with(|m| m.install_failed());
        publish(app);
        return Err(CommandError::new("update", err.to_string()));
    }
    Ok(())
}

/// Release builds: one check after [`AUTO_CHECK_DELAY`], then every [`AUTO_CHECK_INTERVAL`],
/// each only while `ui.check_updates` is on. Failures are logged and kept as `CheckFailed`.
pub fn spawn_auto_check(app: AppHandle, settings: SettingsHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_CHECK_DELAY).await;
        loop {
            if settings.current().config.ui.check_updates {
                check(&app).await;
            }
            tokio::time::sleep(AUTO_CHECK_INTERVAL).await;
        }
    });
}
```

`events.rs`, next to the other names:

```rust
/// Payload: `UpdateState` (updates.rs).
pub const UPDATE_STATE: &str = "update-state";
```

`commands.rs`: import `crate::updates::{self, UpdateState}` and `crate::controller::DictationState`, add the three names to `generate_handler!`, and:

```rust
#[tauri::command]
fn get_update_state(app: AppHandle) -> UpdateState {
    updates::current(&app)
}

/// Runs a check now (or returns the state of the one already running).
#[tauri::command]
async fn check_for_updates(app: AppHandle) -> UpdateState {
    updates::check(&app).await
}

/// On success the app exits and the installer takes over.
#[tauri::command]
async fn install_update(app: AppHandle, core: Core<'_>) -> Result<()> {
    let busy = matches!(
        core.status().state,
        DictationState::Recording | DictationState::Transcribing | DictationState::Pasting
    );
    updates::install(&app, busy).await
}
```

`lib.rs`: add `pub mod updates;`. At the top of `setup()`, before anything can emit or invoke, add `app.manage(updates::UpdateService::default());` (`app.state::<UpdateService>()` panics if the state is not managed). Right after `let settings = SettingsHandle::new(settings);` add:

```rust
    // Release builds only: a dev build must never pull an installer over the installed app.
    if !cfg!(debug_assertions) {
        updates::spawn_auto_check(handle.clone(), settings.clone());
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p opit-speech-to-text updates::tests` → the 8 tests pass. Then `cargo test --workspace`.

- [ ] **Step 6: Full checks** (Task 1 Step 6 commands). `npm run check` passes even though `types.ts` lacks `check_updates` until Task 4.

- [ ] **Step 7: Commit**

```powershell
git add crates/core/src/config.rs crates/app/src/updates.rs crates/app/src/events.rs crates/app/src/commands.rs crates/app/src/lib.rs
git commit -m "feat(app): update check and install service" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: UI — update notice and Settings → Updates

**Files:**
- Modify: `ui/src/lib/types.ts`, `ui/src/lib/api.ts`, `ui/src/lib/events.ts`, `ui/src/lib/app.svelte.ts`
- Create: `ui/src/lib/update.ts`, `ui/src/lib/update.test.ts`
- Create: `ui/src/lib/components/UpdateInstallButton.svelte`, `ui/src/lib/components/UpdateProgress.svelte`
- Modify: `ui/src/App.svelte`, `ui/src/pages/Settings.svelte`
- Modify: `ui/src/lib/i18n/en.ts`, `ui/src/lib/i18n/tr.ts`

**Interfaces:**
- Consumes: Task 3 commands and event (see "UI contract added by this plan"); `saveConfig`, `t`, `errorText`, `app`; components `Banner`, `Button`, `Switch`.
- Produces: `app.update: UpdateState`, `app.updateDismissed: string | null`; `update.ts` exports `bannerVersion(state, dismissed): string | null`, `progressPercent(downloaded, total): number | null`, `statusMessage(state): { key: MessageKey; params?: Params }`, `dictationBusy(state: DictationState): boolean`.

- [ ] **Step 1: Types, API, events.** `types.ts`: add `check_updates: boolean;` to `AppConfig.ui` after `setup_done`, and:

```ts
export interface UpdateInfo {
  version: string;
  current_version: string;
  notes: string | null;
}

/** Mirrors `updates::UpdateState` (crates/app/src/updates.rs). */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo; downloaded: number; total: number | null }
  | { kind: "check_failed"; message: string };
```

`api.ts` (import `UpdateState`):

```ts
  getUpdateState: () => invoke<UpdateState>("get_update_state"),
  checkForUpdates: () => invoke<UpdateState>("check_for_updates"),
  /** On success the app exits and the installer runs, so this may never settle. */
  installUpdate: () => invoke<void>("install_update"),
```

`events.ts`: `export const onUpdateState = (h: (s: UpdateState) => void) => on("update-state", h);`

- [ ] **Step 2: Write the failing Vitest tests** in `ui/src/lib/update.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { bannerVersion, dictationBusy, progressPercent, statusMessage } from "./update";
import type { DictationState, UpdateInfo, UpdateState } from "./types";

const info: UpdateInfo = { version: "0.2.0", current_version: "0.1.0", notes: null };

describe("bannerVersion", () => {
  it("announces an available update until that version is dismissed", () => {
    expect(bannerVersion({ kind: "available", info }, null)).toBe("0.2.0");
    expect(bannerVersion({ kind: "available", info }, "0.2.0")).toBeNull();
    expect(bannerVersion({ kind: "available", info }, "0.1.5")).toBe("0.2.0");
  });
  it("stays quiet for every other state, failed checks included", () => {
    const quiet: UpdateState[] = [
      { kind: "idle" },
      { kind: "checking" },
      { kind: "up_to_date" },
      { kind: "check_failed", message: "Could not fetch a valid release JSON from the remote" },
      { kind: "installing", info, downloaded: 1, total: 2 },
    ];
    for (const state of quiet) expect(bannerVersion(state, null), state.kind).toBeNull();
  });
});

describe("progressPercent", () => {
  it("floors and clamps", () => {
    expect(progressPercent(0, 200)).toBe(0);
    expect(progressPercent(199, 200)).toBe(99);
    expect(progressPercent(200, 200)).toBe(100);
    expect(progressPercent(250, 200)).toBe(100);
  });
  it("is null while the size is unknown", () => {
    expect(progressPercent(10, null)).toBeNull();
    expect(progressPercent(10, 0)).toBeNull();
  });
});

describe("statusMessage", () => {
  it("maps every state to a message with its values", () => {
    expect(statusMessage({ kind: "idle" })).toEqual({ key: "update.status_idle" });
    expect(statusMessage({ kind: "checking" })).toEqual({ key: "update.status_checking" });
    expect(statusMessage({ kind: "up_to_date" })).toEqual({ key: "update.status_up_to_date" });
    expect(statusMessage({ kind: "available", info })).toEqual({
      key: "update.status_available",
      params: { version: "0.2.0", current: "0.1.0" },
    });
    expect(statusMessage({ kind: "installing", info, downloaded: 0, total: null })).toEqual({
      key: "update.status_installing",
      params: { version: "0.2.0" },
    });
    expect(statusMessage({ kind: "check_failed", message: "offline" })).toEqual({
      key: "update.status_failed",
      params: { message: "offline" },
    });
  });
});

describe("dictationBusy", () => {
  it("is true only while a dictation is in flight", () => {
    const busy: DictationState[] = ["recording", "transcribing", "pasting"];
    const free: DictationState[] = ["idle", "cancelled", "error"];
    for (const s of busy) expect(dictationBusy(s), s).toBe(true);
    for (const s of free) expect(dictationBusy(s), s).toBe(false);
  });
});
```

Run `cd ui; npm test` → FAIL (module `./update` missing).

- [ ] **Step 3: Implement `ui/src/lib/update.ts`:**

```ts
// Pure helpers for the update notice (App.svelte) and Settings → Updates.
import type { MessageKey, Params } from "./i18n";
import type { DictationState, UpdateState } from "./types";

/** The version the shell banner announces: only an available update the user has not dismissed. */
export function bannerVersion(state: UpdateState, dismissed: string | null): string | null {
  if (state.kind !== "available") return null;
  return state.info.version === dismissed ? null : state.info.version;
}

/** Whole percent downloaded, or null while the size is unknown. */
export function progressPercent(downloaded: number, total: number | null): number | null {
  if (total === null || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((downloaded * 100) / total)));
}

/** The status line on Settings → Updates. */
export function statusMessage(state: UpdateState): { key: MessageKey; params?: Params } {
  switch (state.kind) {
    case "idle":
      return { key: "update.status_idle" };
    case "checking":
      return { key: "update.status_checking" };
    case "up_to_date":
      return { key: "update.status_up_to_date" };
    case "available":
      return { key: "update.status_available", params: { version: state.info.version, current: state.info.current_version } };
    case "installing":
      return { key: "update.status_installing", params: { version: state.info.version } };
    case "check_failed":
      return { key: "update.status_failed", params: { message: state.message } };
  }
}

/** Installing closes the app, so it waits while a dictation is recording, transcribing or pasting. */
export function dictationBusy(state: DictationState): boolean {
  return state === "recording" || state === "transcribing" || state === "pasting";
}
```

- [ ] **Step 4: Strings.** Add to `en.ts` and `tr.ts` (same keys; put the `settings.*` keys with the other Settings keys and the `update.*` block after them; `error.code.update` with the other `error.code.*` keys):

| key | en | tr |
|---|---|---|
| `settings.updates` | Updates | Güncellemeler |
| `settings.check_updates` | Check for updates automatically | Güncellemeleri otomatik denetle |
| `settings.check_updates_hint` | Asks GitHub for the latest version at start-up and once a day. Nothing else is sent. | Açılışta ve sonra günde bir kez GitHub'a en son sürümü sorar. Başka hiçbir şey gönderilmez. |
| `update.debug` | Updates work only in the installed app | Güncellemeler yalnızca kurulu uygulamada çalışır |
| `update.available` | Version {version} is available. | {version} sürümü hazır. |
| `update.install` | Install and restart | Yükle ve yeniden başlat |
| `update.check_now` | Check now | Şimdi denetle |
| `update.busy` | Finish the current dictation first. | Önce süren dikteyi bitirin. |
| `update.progress` | Download progress | İndirme ilerlemesi |
| `update.install_note` | The app closes, the installer runs, and the app opens again. Your settings, history and keys are kept. | Uygulama kapanır, kurulum çalışır ve uygulama yeniden açılır. Ayarlarınız, geçmişiniz ve anahtarlarınız korunur. |
| `update.status_idle` | Not checked yet. | Henüz denetlenmedi. |
| `update.status_checking` | Checking for updates… | Güncellemeler denetleniyor… |
| `update.status_up_to_date` | You have the latest version. | En son sürümü kullanıyorsunuz. |
| `update.status_available` | Version {version} is available (you have {current}). | {version} sürümü hazır (sizdeki: {current}). |
| `update.status_installing` | Downloading version {version}… | {version} sürümü indiriliyor… |
| `update.status_failed` | Could not check for updates: {message} | Güncellemeler denetlenemedi: {message} |
| `error.code.update` | Update failed: {message} | Güncelleme başarısız: {message} |

- [ ] **Step 5: Live state.** `ui/src/lib/app.svelte.ts`:
- `AppState` gains `update: UpdateState;` and `updateDismissed: string | null;`. The initial values are `{ kind: "idle" }` and `null`.
- In `initApp`, add `let updateSeen = false;`. Add a subscription `onUpdateState((u) => { updateSeen = true; app.update = u; })` to the `Promise.allSettled` list.
- Add `api.getUpdateState()` as the sixth entry of the `Promise.all` (destructure `update`). After loading, set `if (!updateSeen) app.update = update;`. This is the same "event wins over the initial load" pattern used for status and hotkey.

- [ ] **Step 6: Components.** `ui/src/lib/components/UpdateProgress.svelte`:

```svelte
<script lang="ts">
  import { t } from "../app.svelte";
  import { progressPercent } from "../update";

  let { downloaded, total }: { downloaded: number; total: number | null } = $props();
  const percent = $derived(progressPercent(downloaded, total));
</script>

{#if percent === null}
  <progress aria-label={t("update.progress")}></progress>
{:else}
  <progress max="100" value={percent} aria-label={t("update.progress")}></progress>
{/if}

<style>
  progress { width: 100%; }
</style>
```

`ui/src/lib/components/UpdateInstallButton.svelte`:

```svelte
<script lang="ts">
  import { api } from "../api";
  import { app, errorText, t } from "../app.svelte";
  import { dictationBusy } from "../update";
  import Button from "./Button.svelte";

  let busy = $state(false);
  let error = $state<string | null>(null);
  const dictating = $derived(dictationBusy(app.status.state));
  const debugBuild = $derived(app.info?.debug_build ?? false);

  async function install() {
    busy = true;
    error = null;
    try {
      await api.installUpdate(); // on success the app exits before this returns
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<span class="install">
  <Button variant="primary" {busy} disabled={dictating || debugBuild || app.update.kind !== "available"}
    title={t("update.install_note")} onclick={() => void install()}>{t("update.install")}</Button>
  {#if dictating}<span class="muted">{t("update.busy")}</span>{/if}
  {#if error}<span class="error" role="alert">{error}</span>{/if}
</span>

<style>
  .install { display: inline-flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .error { color: var(--danger); font-size: var(--text-sm); }
</style>
```

- [ ] **Step 7: Shell banner.** `ui/src/App.svelte`. In the script:

```ts
  import { bannerVersion } from "./lib/update";
  import UpdateInstallButton from "./lib/components/UpdateInstallButton.svelte";
  import UpdateProgress from "./lib/components/UpdateProgress.svelte";

  const updateVersion = $derived(bannerVersion(app.update, app.updateDismissed));
  const installing = $derived(app.update.kind === "installing" ? app.update : null);
```

Extend the notices condition to `{#if app.notices.length || app.hotkey.error || app.hotkey.paused || resumeError || updateVersion || installing}`. Inside `.notices`, after the `resumeError` banner:

```svelte
          {#if installing}
            <Banner tone="info">
              {t("update.status_installing", { version: installing.info.version })}
              <UpdateProgress downloaded={installing.downloaded} total={installing.total} />
            </Banner>
          {:else if updateVersion}
            <Banner tone="info" ondismiss={() => (app.updateDismissed = updateVersion)}>
              {t("update.available", { version: updateVersion })}
              {#snippet action()}<UpdateInstallButton />{/snippet}
            </Banner>
          {/if}
```

The wizard (`#/setup`) renders outside the shell and never shows this banner. Dismissing hides it for this run only.

- [ ] **Step 8: Settings → Updates.** `ui/src/pages/Settings.svelte`. Script additions:

```ts
  import UpdateInstallButton from "../lib/components/UpdateInstallButton.svelte";
  import UpdateProgress from "../lib/components/UpdateProgress.svelte";
  import { statusMessage } from "../lib/update";

  const debugBuild = $derived(app.info?.debug_build ?? false);
  const updateStatus = $derived(statusMessage(app.update));
  const installing = $derived(app.update.kind === "installing" ? app.update : null);

  async function checkNow() {
    errors.update_check = null;
    try {
      app.update = await api.checkForUpdates();
    } catch (e) {
      if (!disposed) errors.update_check = errorText(e);
    }
  }
```

Add `checkUpdates: c.ui.check_updates,` to the `view` object. Insert a section before "About":

```svelte
      <section class="card stack" aria-labelledby="settings-updates">
        <h2 id="settings-updates">{t("settings.updates")}</h2>
        <Switch label={t("settings.check_updates")} checked={view.checkUpdates} disabled={debugBuild}
          hint={debugBuild ? t("update.debug") : t("settings.check_updates_hint")}
          onchange={(v) => void save("check_updates", (c) => { c.ui.check_updates = v; })} />
        {@render fieldError("check_updates")}
        <p role="status">{t(updateStatus.key, updateStatus.params)}</p>
        {#if installing}<UpdateProgress downloaded={installing.downloaded} total={installing.total} />{/if}
        <div class="row">
          <Button busy={app.update.kind === "checking"} disabled={debugBuild || installing !== null}
            onclick={() => void checkNow()}>{t("update.check_now")}</Button>
          {#if app.update.kind === "available"}<UpdateInstallButton />{/if}
        </div>
        {@render fieldError("update_check")}
      </section>
```

- [ ] **Step 9: Run tests and checks**

```powershell
cd ui; npm test; npm run check; npm run build; cd ..
cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
```

Expected: all Vitest files pass (the new `update.test.ts` and the i18n parity tests with the new keys). `svelte-check` reports 0 errors and 0 warnings.

- [ ] **Step 10: Look at it** with `cargo tauri dev` and a scratch `OPIT_DATA_DIR`. Settings → Updates shows the switch disabled with "Updates work only in the installed app" and the "Check now" button disabled. "Not checked yet." is the status. Switch the language to Turkish and back.

- [ ] **Step 11: Commit**

```powershell
git add ui/src
git commit -m "feat(ui): update notice and Settings > Updates" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Release workflow and updater manifest script

**Files:**
- Create: `scripts/release/latest-json.mjs`, `scripts/release/latest-json.test.mjs`
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml` (new `release-scripts` job)
- Modify: `.gitignore` (`/release-assets/`)

**Interfaces:**
- Consumes: the bundler output of Tasks 1–2: `target/release/bundle/nsis/Opit Speech to Text_<version>_x64-setup.exe` + `.exe.sig`; root `Cargo.toml` `[workspace.package] version`.
- Produces: CLI `node scripts/release/latest-json.mjs check-version <tag>` and `… assets <tag> <bundle-dir> <out-dir>`, which writes `<out-dir>/opit-speech-to-text_<v>_x64-setup.exe`, the matching `.sig` and `latest.json`. Task 7 runs `assets` locally.

- [ ] **Step 1: Write the failing tests** in `scripts/release/latest-json.test.mjs`:

```js
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  assetName, downloadUrl, findInstaller, manifest, rfc3339, versionFromTag, workspaceVersion,
} from "./latest-json.mjs";

test("release tags are v<major>.<minor>.<patch>", () => {
  assert.equal(versionFromTag("v0.1.0"), "0.1.0");
  for (const bad of ["0.1.0", "v0.1", "v0.1.0-rc.1", "release-1"]) assert.throws(() => versionFromTag(bad), bad);
});

test("the app version is the one under [workspace.package]", () => {
  const toml = '[workspace]\nmembers = ["a"]\n\n[workspace.package]\nversion = "0.3.1"\nedition = "2024"\n\n[workspace.dependencies]\nversion = "9.9.9"\n';
  assert.equal(workspaceVersion(toml), "0.3.1");
  assert.equal(workspaceVersion(toml.replaceAll("\n", "\r\n")), "0.3.1");
  assert.throws(() => workspaceVersion('[package]\nversion = "1.0.0"\n'));
  const real = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8");
  assert.match(workspaceVersion(real), /^\d+\.\d+\.\d+$/);
});

test("asset names and URLs are ASCII without spaces", () => {
  assert.equal(assetName("0.1.0"), "opit-speech-to-text_0.1.0_x64-setup.exe");
  assert.equal(
    downloadUrl("0.1.0"),
    "https://github.com/opit80/opit-speech-to-text/releases/download/v0.1.0/opit-speech-to-text_0.1.0_x64-setup.exe",
  );
  assert.doesNotMatch(downloadUrl("0.1.0"), /\s/);
});

test("finds the bundler's installer for the version and insists on its signature", () => {
  const files = [
    "Opit Speech to Text_0.1.0_x64-setup.exe",
    "Opit Speech to Text_0.1.0_x64-setup.exe.sig",
    "Opit Speech to Text_0.0.9_x64-setup.exe",
  ];
  assert.equal(findInstaller(files, "0.1.0"), "Opit Speech to Text_0.1.0_x64-setup.exe");
  assert.throws(() => findInstaller(files.slice(0, 1), "0.1.0"), /\.sig is missing/);
  assert.throws(() => findInstaller(files, "0.2.0"), /found 0/);
});

test("latest.json uses the updater's static format", () => {
  const json = manifest({ version: "0.1.0", signature: "c2lnbmF0dXJl\n", pubDate: "2026-10-01T12:00:00Z" });
  assert.deepEqual(json, {
    version: "0.1.0",
    notes: "https://github.com/opit80/opit-speech-to-text/releases/tag/v0.1.0",
    pub_date: "2026-10-01T12:00:00Z",
    platforms: { "windows-x86_64": { signature: "c2lnbmF0dXJl", url: downloadUrl("0.1.0") } },
  });
  assert.throws(() => manifest({ version: "0.1.0", signature: " \n", pubDate: "x" }), /empty/);
});

test("pub_date is RFC 3339 without milliseconds", () => {
  assert.equal(rfc3339(new Date(Date.UTC(2026, 9, 1, 12, 0, 0, 123))), "2026-10-01T12:00:00Z");
});
```

Run `node --test scripts/release/latest-json.test.mjs` → FAIL (module not found).

- [ ] **Step 2: Implement `scripts/release/latest-json.mjs`:**

```js
#!/usr/bin/env node
// Release helper for .github/workflows/release.yml and the local release build.
//
//   node scripts/release/latest-json.mjs check-version <tag>
//   node scripts/release/latest-json.mjs assets <tag> <nsis-bundle-dir> <out-dir>
//
// `assets` copies the signed NSIS installer and its .sig under a fixed ASCII name (GitHub turns
// spaces in asset names into dots, which would break the manifest URL) and writes latest.json,
// which tauri-plugin-updater reads from
// https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json
import { copyFileSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const REPO = "opit80/opit-speech-to-text";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** "v1.2.3" → "1.2.3". Pre-release tags are not used in v1. */
export function versionFromTag(tag) {
  const match = /^v(\d+\.\d+\.\d+)$/.exec(tag);
  if (!match) throw new Error(`"${tag}" is not a release tag like v1.2.3`);
  return match[1];
}

/** `version` under [workspace.package] in the root Cargo.toml: the app's version. */
export function workspaceVersion(cargoToml) {
  let inSection = false;
  for (const raw of cargoToml.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      inSection = line === "[workspace.package]";
    } else if (inSection) {
      const match = /^version\s*=\s*"([^"]+)"$/.exec(line);
      if (match) return match[1];
    }
  }
  throw new Error("no version under [workspace.package] in Cargo.toml");
}

export function assetName(version) {
  return `opit-speech-to-text_${version}_x64-setup.exe`;
}

export function downloadUrl(version) {
  return `https://github.com/${REPO}/releases/download/v${version}/${assetName(version)}`;
}

/** RFC 3339 without milliseconds, for `pub_date`. */
export function rfc3339(date) {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** The bundler's `<productName>_<version>_x64-setup.exe` among `files`; its `.sig` must be there too. */
export function findInstaller(files, version) {
  const suffix = `_${version}_x64-setup.exe`;
  const found = files.filter((name) => name.endsWith(suffix));
  if (found.length !== 1) throw new Error(`expected one *${suffix} in the bundle folder, found ${found.length}`);
  if (!files.includes(`${found[0]}.sig`)) {
    throw new Error(`${found[0]}.sig is missing: build with TAURI_SIGNING_PRIVATE_KEY set`);
  }
  return found[0];
}

/** latest.json in tauri-plugin-updater's static format. */
export function manifest({ version, signature, pubDate }) {
  const sig = signature.trim();
  if (!sig) throw new Error("the signature is empty");
  return {
    version,
    notes: `https://github.com/${REPO}/releases/tag/v${version}`,
    pub_date: pubDate,
    platforms: { "windows-x86_64": { signature: sig, url: downloadUrl(version) } },
  };
}

function checkVersion(tag) {
  const version = versionFromTag(tag);
  const cargo = workspaceVersion(readFileSync(join(ROOT, "Cargo.toml"), "utf8"));
  if (version !== cargo) throw new Error(`tag ${tag} does not match the app version ${cargo} in Cargo.toml`);
  return version;
}

function assets(tag, bundleDir, outDir) {
  const version = checkVersion(tag);
  const installer = findInstaller(readdirSync(bundleDir), version);
  const signature = readFileSync(join(bundleDir, `${installer}.sig`), "utf8");
  mkdirSync(outDir, { recursive: true });
  copyFileSync(join(bundleDir, installer), join(outDir, assetName(version)));
  writeFileSync(join(outDir, `${assetName(version)}.sig`), signature);
  const json = manifest({ version, signature, pubDate: rfc3339(new Date()) });
  writeFileSync(join(outDir, "latest.json"), `${JSON.stringify(json, null, 2)}\n`);
  console.log(`release assets for v${version} written to ${outDir}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [command, ...args] = process.argv.slice(2);
  try {
    if (command === "check-version" && args.length === 1) console.log(checkVersion(args[0]));
    else if (command === "assets" && args.length === 3) assets(args[0], args[1], args[2]);
    else throw new Error("usage: latest-json.mjs check-version <tag> | assets <tag> <bundle-dir> <out-dir>");
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
```

Run `node --test scripts/release/latest-json.test.mjs` → all pass. Also `node scripts/release/latest-json.mjs check-version v0.1.0` → prints `0.1.0`; `… check-version v9.9.9` → exits 1 with the mismatch message.

- [ ] **Step 3: Release workflow** `.github/workflows/release.yml`:

```yaml
name: Release

# Pushing a tag vX.Y.Z builds the signed NSIS installer and the updater manifest and attaches
# them to a DRAFT GitHub Release. Nothing reaches users until a person runs
# docs/RELEASE-CHECKLIST.md and publishes the draft (releases/latest ignores drafts).
on:
  push:
    tags: ["v*.*.*"]

permissions:
  contents: write

jobs:
  release:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: ui/package-lock.json
      - name: Check the tag against the app version
        run: node scripts/release/latest-json.mjs check-version "${{ github.ref_name }}"
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install UI dependencies
        working-directory: ui
        run: npm ci
      - name: Install the Tauri CLI (same version as the tauri crate)
        run: cargo install tauri-cli --version 2.12.0 --locked
      - name: Test
        run: cargo test --workspace
      - name: Build the signed installer
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: cargo tauri build --ci
      - name: Prepare the release assets and latest.json
        run: node scripts/release/latest-json.mjs assets "${{ github.ref_name }}" target/release/bundle/nsis release-assets
      - name: Create the draft release
        shell: bash
        env:
          GH_TOKEN: ${{ github.token }}
        run: >-
          gh release create "${{ github.ref_name }}" release-assets/*
          --draft --verify-tag --generate-notes
          --title "Opit Speech to Text ${{ github.ref_name }}"
```

(`shell: bash` is required because PowerShell does not expand `release-assets/*`.)

- [ ] **Step 4: CI job.** Append to `.github/workflows/ci.yml` under `jobs:`:

```yaml
  release-scripts:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - run: node --test scripts/release/latest-json.test.mjs
```

Add `/release-assets/` to `.gitignore`.

- [ ] **Step 5: Validate the YAML locally** (no network, nothing pushed). Run `node -e "for (const f of ['.github/workflows/release.yml','.github/workflows/ci.yml']) { const s=require('fs').readFileSync(f,'utf8'); if (/\t/.test(s)) throw new Error(f+': tab'); }"`, and read both files once more for indentation. If `actionlint` is installed, run it too. Do **not** install new tools for this.

- [ ] **Step 6: Full checks** (Task 1 Step 6) plus `node --test scripts/release/latest-json.test.mjs`.

- [ ] **Step 7: Commit**

```powershell
git add scripts/release .github/workflows/release.yml .github/workflows/ci.yml .gitignore
git commit -m "ci: tag-triggered draft release workflow and updater manifest script" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docs — README, release checklist, SignPath application notes

**Files:**
- Modify: `README.md`
- Create: `docs/RELEASE-CHECKLIST.md`
- Create: `docs/signpath-application.md`

**Interfaces:**
- Consumes: the behaviour of Tasks 1–5 (names, paths, flags, workflow, Settings → Updates).
- Produces: user docs. Task 7 links them from the plans README.

- [ ] **Step 1: README.** Make these changes, keeping the existing style (short paragraphs, tables, English):
  1. Replace the "Status" note with: v1 is in pre-release; installers will be on the [Releases page](https://github.com/opit80/opit-speech-to-text/releases).
  2. New section **Install** (before "Development"):
     - Download `opit-speech-to-text_<version>_x64-setup.exe` from Releases and run it. It installs for your user only (`%LOCALAPPDATA%\Opit Speech to Text`), without an admin prompt. Windows 10/11 x64. The installer adds WebView2 if it is missing (needs internet).
     - **SmartScreen:** v1 installers are not code-signed yet, so Windows may show "Windows protected your PC". Click **More info → Run anyway**. Explain why: the project has applied for free open-source signing via SignPath Foundation. Updates themselves are signed: the app installs only an update whose signature matches the key built into it.
     - The installer can start the app at the end. The first-run wizard follows.
  3. New section **Updates**: the app asks GitHub for a newer version 20 s after start and once a day (Settings → Updates → "Check for updates automatically"; turn it off to never contact GitHub on its own). A banner and Settings → Updates offer **Install and restart**. The app closes, the installer runs with a small progress window and the app starts again. Settings, history, rules and keys are kept. Installing waits while a dictation is running. Manual alternative: download the newer installer and run it over the old one.
  4. New section **Uninstall**: Windows Settings → Apps → Installed apps → *Opit Speech to Text* → Uninstall. The uninstaller always removes the program, its shortcuts and the "Start with Windows" entry. Tick **Delete the application data** to also remove `%APPDATA%\opit-speech-to-text\` (settings, `rules\user.yaml`, history, audio, logs), the WebView data in `%LOCALAPPDATA%\io.github.opit80.opit-speech-to-text` and the API keys this app stored in Credential Manager. **Back up `rules\user.yaml` first if you want to keep your rules.** Manual clean-up when the box was not ticked: delete that folder and remove the `*.opit-speech-to-text` entries in Credential Manager (`cmdkey /list`, then `cmdkey /delete:groq.opit-speech-to-text` per entry).
  5. New short section **Privacy**: audio goes only to the provider of the active profile. The only other network request is the update check to GitHub (opt-out above). No telemetry. Logs never contain transcripts or keys.
  6. In "Development", add after the command block: `cargo tauri build` now signs updater artifacts and needs `TAURI_SIGNING_PRIVATE_KEY`/`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Without the key use `cargo tauri build --no-bundle` (exe only) or `cargo tauri build --no-sign` (unsigned installer, cannot serve as an update). The `cargo install tauri-cli` line pins `--version 2.12.0`.
  7. New section **Releasing** (maintainers): bump `version` under `[workspace.package]` in `Cargo.toml`, commit, then `git tag vX.Y.Z` and push the tag. The *Release* workflow builds the signed installer and `latest.json` into a **draft** release. Run [`docs/RELEASE-CHECKLIST.md`](docs/RELEASE-CHECKLIST.md) on a clean Windows with the draft's installer, then publish the draft. Publishing is what makes `releases/latest/download/latest.json` visible to the installed apps. Secrets: `TAURI_SIGNING_PRIVATE_KEY` (content of the private key file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never regenerate the key: installed apps accept only updates signed with it.
  8. Replace the paragraph that starts "`ui.autostart` defaults to `true`, so the release exe adds itself …" with: "Start with Windows" (on by default) is applied when the setup wizard is finished or skipped, never before. It writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Opit Speech to Text`, and turning the switch off removes it.
  9. In "Using the app", change "Build and start it: …" to say: install it (see Install), or for development build it as described in Development.

- [ ] **Step 2: `docs/RELEASE-CHECKLIST.md`** with this content (the spec §9 list, made concrete):

```markdown
# Release checklist

Run this for every release on a **clean Windows 10 or 11 x64** (a fresh VM or Windows Sandbox,
signed in as a standard user), with the installer from the **draft** GitHub Release. Tick every
line; anything that fails blocks publishing. Record the version, date, machine and results in the
release notes draft.

## Before tagging
- [ ] `version` under `[workspace.package]` in `Cargo.toml` is the new version; CI is green on `main`.
- [ ] `git tag vX.Y.Z` points at that commit; the *Release* workflow finished and created a draft
      with `opit-speech-to-text_X.Y.Z_x64-setup.exe`, its `.sig` and `latest.json`.
- [ ] `latest.json` → `version` is X.Y.Z, `url` ends in the asset name above, `signature` equals the
      `.sig` content.

## Install
- [ ] Download the installer in a browser (so it carries the internet mark). SmartScreen warns
      ("Windows protected your PC") because v1 is unsigned; **More info → Run anyway** works.
- [ ] Installer size < 15 MB (spec §1). No UAC prompt. Language follows Windows (English/Turkish).
- [ ] Installs to `%LOCALAPPDATA%\Opit Speech to Text`; Start menu shortcut works; "Create desktop
      shortcut" on the last page works.
- [ ] `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v "Opit Speech to Text"` →
      not found before the wizard is finished or skipped.

## First run (spec §1: under 2 minutes)
- [ ] The wizard opens; language → provider + key + **Test** → microphone + level → shortcut →
      rule packs + "Start with Windows" (checked) → try. Time from first window to the first
      pasted dictation: ____ (must be < 2 min).
- [ ] After Finish, the Run value exists and points at the installed exe with `--autostart`.

## Daily use
- [ ] Shortcut (Right Ctrl + Right Shift): dictation pastes into Notepad, a browser text field and
      an Office/Electron app; Esc cancels; Ctrl alone stops.
- [ ] Paste failure path: an admin window gets the text on the clipboard with the overlay hint.
- [ ] Tray: left click starts/stops; menu Open / Profile ▸ / Pause shortcut / Exit work.
- [ ] Close the window → app stays in the tray; Task Manager RAM in the tray < 40 MB: ____ MB.
- [ ] Sign out and in: the app starts in the tray (autostart), shortcut works.
- [ ] Settings → Updates: version shown; **Check now** says "You have the latest version" (or
      reports the newer release if one is published).

## Update (from the second release on; for the very first release, publish vX.Y.Z, then cut
## vX.Y.Z+1 at once and run this section before telling anyone about the release)
- [ ] Install the previous published version, finish the wizard, make two dictations, store a key.
- [ ] Publish the new draft. In the old app: banner "Version … is available" appears within a
      minute of start (or use **Check now**); **Install and restart** is disabled during a
      dictation.
- [ ] Install: passive installer window, app restarts on its own, Settings → About shows the new
      version.
- [ ] Data kept: settings, history, `rules\user.yaml`, API key, Run value (still pointing at the
      exe), no "Delete the application data" side effects.

## Uninstall
- [ ] Uninstall **without** "Delete the application data": program, shortcuts and Run value are
      gone; `%APPDATA%\opit-speech-to-text` and the Credential Manager entries remain.
- [ ] Reinstall, then uninstall **with** the box ticked: `%APPDATA%\opit-speech-to-text`,
      `%LOCALAPPDATA%\io.github.opit80.opit-speech-to-text` and every `*.opit-speech-to-text`
      Credential Manager entry (`cmdkey /list`) are gone.

## Publish
- [ ] Edit the draft notes (what changed, known issues, the SmartScreen note), then **Publish**.
- [ ] `https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json` serves
      the new version.
```

- [ ] **Step 3: `docs/signpath-application.md`** with this content:

```markdown
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
```

- [ ] **Step 4: Check the docs.** Every path, flag and key name in the README and the two new docs matches the code: `--delete-credentials`, `windows/hooks.nsh`, `ui.check_updates`, the endpoint URL, the asset name, the `Opit Speech to Text` Run value, `%LOCALAPPDATA%\Opit Speech to Text`. Check this with `rg` over the repo.

- [ ] **Step 5: Commit**

```powershell
git add README.md docs/RELEASE-CHECKLIST.md docs/signpath-application.md
git commit -m "docs: install, update, uninstall, release checklist and SignPath notes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Local signed release build and the Plan 4 outcome

Nothing is published. This task builds the installer the user will look at before any release and records how to view it.

**Files:**
- Modify: `docs/superpowers/plans/README.md` (Plan 4 row, new "Plan 4 outcome" section)
- Not committed: `target\release\bundle\nsis\*`, `release-assets\*`

**Interfaces:**
- Consumes: key files from Task 2, `scripts/release/latest-json.mjs assets` from Task 5.
- Produces: `target\release\bundle\nsis\Opit Speech to Text_0.1.0_x64-setup.exe` (+ `.sig`) and `release-assets\` (renamed installer, `.sig`, `latest.json`).

- [ ] **Step 1: Full automated run**

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --test scripts/release/latest-json.test.mjs
cd ui; npm ci; npm run check; npm test; npm run build; cd ..
```

Keep the summary lines (test counts per crate, Vitest file/test counts, svelte-check files/errors/warnings, build sizes) for Step 5.

- [ ] **Step 2: Signed release build** (PowerShell, from the repo root; the key and password stay in env vars of this shell only):

```powershell
$key = Join-Path $env:USERPROFILE ".tauri\opit-speech-to-text.key"
$env:TAURI_SIGNING_PRIVATE_KEY = $key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content "$key.password" -Raw).Trim()
cargo tauri build --ci 2>&1 | Tee-Object -Variable buildLog
if ($buildLog -match "does not match the public key") { throw "the private key does not match plugins.updater.pubkey" }
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Expected: `target\release\bundle\nsis\Opit Speech to Text_0.1.0_x64-setup.exe` and `…setup.exe.sig`.

- [ ] **Step 3: Verify the artifacts**

```powershell
$nsis = "target\release\bundle\nsis"
$exe = Get-Item "$nsis\Opit Speech to Text_0.1.0_x64-setup.exe"
"{0:N1} MB" -f ($exe.Length / 1MB)                       # must be < 15 MB (spec §1)
(Get-FileHash $exe.FullName -Algorithm SHA256).Hash
$sig = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String((Get-Content "$($exe.FullName).sig" -Raw).Trim()))
$sig -split "`n" | Where-Object { $_ -like "trusted comment:*" }   # must contain "version:0.1.0"
node scripts/release/latest-json.mjs assets v0.1.0 $nsis release-assets
Get-Content release-assets\latest.json
```

The trusted comment must contain `version:0.1.0`, or `requireSignedVersion` would reject every update. `latest.json` must show version `0.1.0` and the `opit-speech-to-text_0.1.0_x64-setup.exe` URL. Do **not** run the installer: installing changes the user's machine (Run key, Start menu), and the user does that in Step 4's checklist.

- [ ] **Step 4: Write the Plan 4 outcome** in `docs/superpowers/plans/README.md`:
  - Plan 4 row status: **code complete on `feat/plan-4-release`** (date, 7 tasks), not merged, nothing published; link stays.
  - New section **"Plan 4 outcome"** after "Plan 3 outcome", containing:
    - **Installer for review:** the absolute path of the setup exe, its size in MB, its SHA-256, and the trusted-comment line. Note that `release-assets\` holds the renamed copy and `latest.json`.
    - **Rulings applied:** the two Ruling lines from this plan, verbatim.
    - **View the app (user, before any release):**
      1. Back up `%APPDATA%\opit-speech-to-text\rules\user.yaml` (the installed app uses the real data folder).
      2. Run the setup exe. A locally built file has no internet mark, so SmartScreen does not appear here; it will for downloaded releases. Expect no UAC prompt, English or Turkish pages, install to `%LOCALAPPDATA%\Opit Speech to Text`, and "Run" on the last page.
      3. If `setup_done` is still false in your config, the wizard opens. The Run value must not exist before Finish/Skip (`reg query … /v "Opit Speech to Text"`) and must exist after it, if left checked.
      4. Settings → Updates: the switch is on, and **Check now** shows "Could not check for updates: Could not fetch a valid release JSON from the remote". This is expected until the GitHub repo and a release exist. No banner appears.
      5. One real dictation with the shortcut, the tray menu, and tray RAM in Task Manager (< 40 MB).
      6. Optional: uninstall without ticking "Delete the application data" and confirm the data stays. Tick it only on a machine whose data you have backed up.
    - **Automated run:** the summary lines from Step 1.
    - **What the user must do before the first release:**
      - Back up the key file, `.pub` and the password (password manager). They cannot be recreated, and losing them strands every installed copy.
      - Create the GitHub repo `opit80/opit-speech-to-text` and push `main`.
      - Add the secrets `TAURI_SIGNING_PRIVATE_KEY` (content of `%USERPROFILE%\.tauri\opit-speech-to-text.key`) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (content of the `.password` file), then delete the `.password` file.
      - Merge, tag `v0.1.0`, run `docs/RELEASE-CHECKLIST.md` on a clean Windows and publish the draft.
      - Apply to SignPath Foundation with `docs/signpath-application.md` after the first release, with MFA on.
      - The manual passes still open from Plans 2 and 3.
    - **Deferred:** SignPath integration in the workflow; release notes shown inside the app (the `notes` field is not displayed); tray menu entry for updates; and anything else found while executing this plan.

- [ ] **Step 5: Commit** (docs only; `target/` and `release-assets/` are ignored):

```powershell
git status --short   # only docs/superpowers/plans/README.md
git add docs/superpowers/plans/README.md
git commit -m "docs: plan 4 outcome, local signed release build" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review notes (for the executor)

- **Spec coverage:**
  - §2 *Kurulum* (NSIS currentUser): Task 1.
  - §2 *Güncelleme* (`tauri-plugin-updater`, GitHub Releases, minisign): Tasks 2, 3 and 4, plus the manifest in Task 5.
  - §2 *Kod imzalama* (v1 unsigned, SmartScreen in the README, SignPath application): Task 6.
  - §9 *Manuel yayın kontrol listesi*: Task 6 `docs/RELEASE-CHECKLIST.md`.
  - §9 *CI* tag release (NSIS + updater manifest → GitHub Release): Task 5. It creates a draft, which is the publishing gate.
  - §10 "verilerimi de sil": Task 1.
  - §1 installer < 15 MB: Task 7 measures it.
- **Plan 3 "Behaviour Plan 4 must know about":** autostart is ruled and implemented in Task 1, delete-data is ruled and implemented in Task 1, and hash routing and i18n/saveConfig are used in Task 4.
- **Names used across tasks:**
  - `DELETE_CREDENTIALS_FLAG = "--delete-credentials"` (Task 1, used in `hooks.nsh` and the docs).
  - `autostart_wanted` (Task 1).
  - `UpdateState` kinds `idle | checking | up_to_date | available | installing | check_failed` (Task 3 Rust and Task 4 TS).
  - `update-state` (Task 3 `events::UPDATE_STATE`, Task 4 `onUpdateState`).
  - Commands `get_update_state` / `check_for_updates` / `install_update` ↔ `api.getUpdateState` / `checkForUpdates` / `installUpdate`.
  - `ui.check_updates` (Task 3 core, Task 4 TS and Settings).
  - Asset name `opit-speech-to-text_<v>_x64-setup.exe` (Task 5, Tasks 6 and 7 docs).
- **Not in this plan:** publishing anything, SignPath wiring, MSI, auto-update without asking (the user always clicks Install), and showing release notes in the app.
