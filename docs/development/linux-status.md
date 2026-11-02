# Linux status

This page records observed Linux evidence. It is not a desktop acceptance claim.

## Owner desktop checks (2026-11-02, KDE Plasma 6.8 Wayland, CachyOS)

Observed by the owner on the release build from this tree:

- KDE command shortcut: Install registers it, the status line reads it back, Ctrl+Shift+F9
  starts and stops dictation, and the saved shortcut is still registered after a restart.
- Dictation is transcribed and pasted at the cursor through RemoteDesktop/libei.
- Duck mode lowers Spotify while recording and restores its exact previous volume (78 %);
  no crash record is left behind.
- The redesigned native overlay matches the Windows layout.
- The ready-made shortcut picker and in-window Record capture work, and pressing an already
  registered shortcut during capture shows the desktop-shortcut note without starting dictation.

GNOME, X11, other Wayland desktops, crash recovery of ducked streams and packaged AppImage/deb
runs remain unobserved.

## Stream identity and startup shortcut repair (2026-11-02, sandbox)

The owner observed Spotify remaining at full volume: its sink-input properties contained no
`application.process.id`; the PID existed only on client 513. The implementation now resolves
stream PID first, then client `application.process.id`, then client `pipewire.sec.pid`, with one
JSON client-list call per snapshot. Channel-map order, including `aux0,aux1`, is preserved.
Own PID, immediate-child and application-ID checks include client metadata even when a different
PID is present on the stream. Unattributed streams are ducked when a nonempty server serial
still guards index/client reuse; without either process identity or serial they are skipped.
Exact-volume, channel, server-cookie and boot checks still guard restoration.

Each engagement emits one metadata-only debug summary: streams seen/ducked/skipped and identity,
channel, own-process, unchanged-volume and stream-limit skip counts; interrupted batches also
count failed/unattempted streams. No stream names or arbitrary properties are logged.

After config load, the existing blocking startup worker now reconciles every configured KDE/GNOME
action, including optional cancel and individual styles. Mapping changes and saves that retry a
reported failure use the same path, serialized with startup and explicit registry requests.
Readback of the saved combination and current command makes matching registrations a no-op.
Missing/changed combinations or outdated executable commands are installed; cleared actions are
removed first, and only app-owned desktop files/GNOME paths are eligible. KDE login fallback
is retried when no matching live binding exists. Known conflicts, invalid Linux combinations
(including saved bare letters), helper failures and login-only fallback use the existing hotkey
status/banner path without a dialog. One failing action does not abort the remaining actions.
GNOME additionally checks other custom-binding paths before installing a conflicting combination;
this does not enumerate every built-in GNOME shortcut. Saved config bytes are not migrated.

The optional zbus property-cache warning change was left out: ashpd 0.13.13 constructs its
Request/Session proxies internally without exposing cache configuration. Filtering the whole
`zbus::proxy` warning target would also hide unrelated failures.

Verification below uses synthetic JSON, a fake Pulse backend, temporary recovery files and pure
reconcile plans. No GUI, real volume, audio server, D-Bus, desktop registry, real app data or
credentials were accessed. Desktop acceptance remains pending: Spotify duck/restore and own-audio
exclusion, KDE/GNOME restart without pressing Install, actual key activation, conflicts, cleared
entries and preservation of foreign shortcuts. Windows compilation remains a separate gate.

The required checks ran sequentially after the final code changes:

| Command | Actual result |
| --- | --- |
| `cargo fmt --all --check` | Exit 0; no diagnostics. |
| `CARGO_TARGET_DIR=target/agent-build cargo clippy --workspace --all-targets --offline -- -D warnings` | Exit 0; dev profile finished in 1.25 s. |
| `CARGO_TARGET_DIR=target/agent-build cargo test -p opit-speech-to-text --offline --no-fail-fast linux` | Exit 0; 62 passed, 0 failed, 0 ignored, 284 filtered out; other targets had no matching tests. Build 8.59 s, selected tests 0.13 s. |
| `cd ui && npm run check && npm test` | Exit 0; Svelte: 0 errors, 0 warnings; Vitest: 27 files, 176 passed (302 ms). |

Eight new tests cover client PID precedence, the observed aux-channel JSON shape, own-client
exclusion, serial-backed unattributed restoration/reuse, one client-list call per snapshot,
KDE/GNOME reconciliation planning, empty-action cleanup, invalid saved keys, conflicts and action
mapping. These checks establish no desktop or Windows acceptance.

## Current shortcut design (2026-11-02, desktop acceptance pending)

Linux shortcuts now use desktop-registered commands on KDE and GNOME. The GlobalShortcuts
portal implementation has been removed. The shared portal runtime and one-time host registration
remain for RemoteDesktop/libei paste. No Dikte implementation was read or copied.

- KDE: hidden per-action desktop files in `$XDG_DATA_HOME/applications` (normally
  `~/.local/share/applications`). Command/service identities belong only to this app; replacing a
  command or combination removes the previous owned `_launch` entry and desktop file.
- GNOME: custom keybindings with stable app-owned paths. List edits preserve foreign entries
  and their order. Registration is effective immediately according to GNOME's custom-binding contract.
- Other X11 desktops: existing x11rb grabs, XKB repeat handling and keyboard remapping remain.
  No implicit recording-time Escape grab remains. Other Wayland desktops show the exact command
  for manual binding and perform no automatic registration.
- Linux has typed/picked combinations, no WebView key capture. The new-choice default remains
  `Ctrl+Shift+F9`. Hold is hidden and behaves as toggle on press-only Linux backends; the stored
  preference is unchanged. Cancel is optional and empty by default; overlay/tray cancellation remains.
- The app executable accepts `--shortcut <action>` before GTK, single-instance and app-data startup.
  It uses the existing authenticated Unix socket and exits quietly with success if no app is running.
  It never starts a GUI as a side effect. Desktop commands use the absolute executable, or persistent
  `$APPIMAGE` path. Explicit `OPIT_DATA_DIR` overrides are preserved as a quoted environment
  argument, so desktop commands reach scratch instances too. See the [shortcut guide](../guides/linux-shortcuts.md) for the complete action list.
- Setup requires an executable/socket round trip using the same relay and a harmless internal `probe`
  action. It does not open a microphone, even if a probe child arrives after cancellation. This proves
  command delivery, **not** desktop key activation; that remains an owner acceptance check.
- No saved config is migrated or rewritten by startup. Explicit UI choices use the existing save chain.

### KDE live-registration source evidence

Live registration is confirmed **as a source-level contract**, not a tested desktop result.
KDE's [GlobalAccelModel::addApplication / saveAction](https://github.com/KDE/plasma-desktop/blob/master/kcms/keys/globalaccelmodel.cpp)
loads a service with `doRegister([desktop-id, "", friendly-name, ""])`, unregisters that temporary action,
and assigns keys with `setForeignShortcutKeys`. The daemon's
[service component creation and services configuration](https://github.com/KDE/kglobalacceld/blob/master/src/globalshortcutsregistry.cpp)
loads `.desktop` services and their `[services][desktop-id]` settings.
[KServiceActionComponent](https://github.com/KDE/kglobalacceld/blob/master/src/kserviceactioncomponent.cpp)
launches the service's Exec on `_launch` presses. The
[D-Bus interface](https://github.com/KDE/kglobalaccel/blob/master/src/org.kde.KGlobalAccel.xml) and
[daemon implementation](https://github.com/KDE/kglobalacceld/blob/master/src/kglobalacceld.cpp)
provide registration, assignment, availability checks and `shortcutKeys` readback.

Opit independently builds these public arguments (Qt `a(ai)` sequences), updates the service cache,
sets foreign keys, makes them present with `setShortcutKeys` flags 6, and checks readback. It also
writes/reads `_launch` with `kwriteconfig6 --notify` / `kreadconfig6`. If cache activation, D-Bus or
live readback cannot be confirmed, the UI reports **after next login**. Availability checks refuse
known collisions rather than taking another component's shortcut. No KWin restart or logout is forced.

Required owner checks: KDE and GNOME install/change/remove/restart; collision refusal; KDE live
readback and login fallback; preservation of unrelated shortcuts; paths containing spaces/quotes/%;
AppImage/deb/Arch/plain-binary command delivery; paused/disabled shortcut behavior; toggle with a
saved hold preference; optional cancel; every voice/style action; wizard command proof versus actual
key activation; other X11 repeat/remap/grabs; other Wayland manual binding. Voice edit still fails
closed because Linux selection capture is unavailable. Windows compilation remains a Windows gate.

### Shortcut verification (restricted sandbox)

The required gates ran sequentially after the final implementation; no release build or GUI was run.

| Command | Actual final result |
| --- | --- |
| `cargo fmt --all --check` | Exit 0; no diagnostics. |
| `CARGO_TARGET_DIR=target/agent-build cargo clippy --workspace --all-targets --offline -- -D warnings` | Exit 0; finished in 1.23 s. |
| `CARGO_TARGET_DIR=target/agent-build cargo test --workspace --offline --no-fail-fast` | Exit 101; total 549 passed, 92 failed, 3 ignored across targets. All 22 shortcut-focused app tests and all 4 shared portal runtime/host-registration tests pass. |
| `cd ui && npm run check` | Exit 0; 0 errors and 0 warnings. |
| `cd ui && npm test` | Exit 0; 27 files, 176 tests passed (296 ms). |
| `cd ui && npm run build` | Exit 0; 233 modules; production build completed in 511 ms. |

Of the 92 Rust failures, 91 explicitly report socket `Operation not permitted` (OS error 1).
The remaining `local::runtime::server::tests::port_is_ephemeral_and_idle_zero_never_stops` unwraps
`LocalError::Port`; its `free_port` maps a failed loopback TCP bind to that error. An independent
scratch TCP bind also returned errno 1 here. Failing targets are core's library, `cleanup_http`,
`cleanup_styles_http`, `pipeline_http`, eval's `run_eval`, and the app library. They require an
unrestricted socket-capable runner; this full run is **not** a passing workspace test claim.
No failures in the new shortcut/relay tests remain. The three ignored tests remain unverified.

Additional pure checks: nine synthetic Qt key sequences round-tripped through the installed Qt6
`QKeySequence` formatter/parser without creating an application or connecting to D-Bus. The UI
mechanical detector returned `[]`. Windows capture tests remain present, but Windows compilation
and native behavior were not exercised. No actual `gsettings`, `kwriteconfig6`, kglobalaccel,
GUI, real app data or credential operations were performed. No commit, push, tag or release was made.

### Shortcut change inventory

This list describes this task's delta in the shared dirty tree. The separate media ducking and
cleanup auto-enable work is preserved; `media.rs`, `CleanupSettings.svelte`, `cleanup.ts` and its
tests were not edited by this task.

| Scope | Files |
| --- | --- |
| Linux backends | `crates/app/src/platform/linux/{cli.rs,mod.rs,x11.rs,portal.rs,shortcut_combo.rs,shortcuts.rs,shortcuts/kde.rs,shortcuts/gnome.rs}`; removed `hotkey.rs`; removed only the ashpd `global_shortcuts` feature in `crates/app/Cargo.toml` |
| App wiring / controller | `crates/app/src/platform/{mod.rs,fake.rs}`, `crates/app/src/app_core/{config.rs,input.rs}`, `crates/app/src/controller/{mod.rs,handle.rs,recording.rs,tests/mod.rs}`, `crates/app/src/ipc.rs` |
| Commands / protocol / config | `crates/app/src/commands/{app.rs,input.rs,mod.rs,shortcuts.rs}`, `crates/ipc/src/{lib.rs,protocol.rs}`, `crates/core/src/config.rs` |
| UI | `ui/src/features/dictation/{HotkeyInput.svelte,hotkey.ts,hotkey.test.ts}`, `ui/src/features/settings/{Settings.svelte,AskSettings.svelte,CleanupStyles.svelte}`, `ui/src/features/home/Home.svelte`, `ui/src/features/setup/steps/StepShortcut.svelte` |
| Bridge / languages / preview | `ui/src/lib/bridge/{api.ts,types.ts}`, `ui/src/lib/i18n/{en.ts,tr.ts}`, `ui/src/preview/app-preview.ts` |
| Documentation | `docs/README.md`, `docs/guides/{cli.md,linux-shortcuts.md}`, this status page. The root README has no portal claim and needed no shortcut edit. |

The older portal shortcut repair below is historical and superseded by this design. Its RemoteDesktop
runtime/registration findings and the separate media/cleanup work remain relevant.

## Earlier desktop evidence (2026-10-31)

CachyOS, KDE Plasma 6.8 on Wayland, 2026-10-31, debug build served by the Vite dev server. Runs used a throwaway
`OPIT_DATA_DIR`. No provider request was made and no real API key was used. An earlier sandboxed run reported that
GTK could not start; that came from the sandbox, and the app starts normally in the desktop session.

### Observed in that desktop session

| Check | Result |
| --- | --- |
| Startup, first-run config, logs, quit on SIGTERM | Pass. The first-run shortcut is Ctrl+Shift+F9 and the media mode is pause. |
| Setup wizard, step 1 | Renders with no console error. Steps 2 to 7 were not walked. |
| Home, History, Files, Rules, Profiles, Settings, Usage, Guide | Each route loads in WebKitGTK with no console error, unhandled rejection or horizontal overflow. This was checked from inside the page, so nobody has looked at the rendered pages. |
| Settings save | Pass through `save_config`. |
| Tray | Registers with the KDE StatusNotifierWatcher. Its menu was not clicked. |
| Second launch | Exits 0 and the first process stays alive. Focusing the existing window was not observed. |
| `opit status` over the Unix socket | Pass; the socket has mode 0600. |
| Microphone | Default, PipeWire, PulseAudio and the three listed cards open through the level test. No audio was kept and no dictation was run. |
| Secret Service | `secret-tool` store, lookup, clear and missing lookup pass with a disposable value under a separate test service name. The app's own key flow was not run. |

A socket under a very long `OPIT_DATA_DIR` fails to bind because of the Unix path limit. The app logs a warning and keeps running.

## Continuation (2026-11-02)

The dirty tree was re-read before work: branch `feat/dictation-upgrades`, HEAD `5397be5`, with the
previous Linux edits present. Those edits were preserved. No provider was contacted, no real app data or credential
was read, the desktop input and clipboard were not operated, no desktop entry was installed and `~/.tauri` was not read.
No commit, push, tag or release was made.

### Backend status

“Implemented, unobserved” means source and automated checks exist; it does **not** claim a working
native desktop interaction. The earlier desktop observations above do not accept the new backends.

| Area | Current implementation and acceptance |
| --- | --- |
| Wayland insertion | Implemented, unobserved: session-lived RemoteDesktop keyboard permission and `reis` (libei protocol), then ydotool only if no EI input was sent, then clipboard-only. Uncertain EI delivery is never replayed through ydotool. Permission denial/connection loss disables new permission requests until restart. Permissions/timing/focus restoration require owner tests. No restore token is persisted. |
| Result recovery | Automated fake-backend tests pass: multiple undelivered results survive clipboard write failure and History-off in session memory. Home reloads a native snapshot and shows full text with explicit copy/dismiss. Nothing extra is persisted; restart clears it. Native WebView interaction remains unobserved. |
| Linux shortcuts | KDE/GNOME command registries, other X11 native grabs, other Wayland manual commands; see the current shortcut design above. Desktop activation remains unobserved. |
| Hold mode | Linux exposes `shortcut_release=false` and `shortcut_capture=false`. Stored hold preferences act as toggle without changing saved config. |
| Overlay | Implemented, unobserved: native GTK3 layer-shell on supporting Wayland compositors, GTK dock window on X11, non-focusable and input region only while a button is present. Cancel, retry and settings actions use existing controller handling. No supported surface means status notifications/tray. `overlay` reads actual initialized native-surface availability. |
| X11 | Implemented, unobserved: XGrabKey, XKB repeat suppression, remapping, runtime failure reporting, XTest keyboard-map resolution/preflight, EWMH foreground app/list. Wayland never uses XWayland foreground discovery. Per-app capability requires an X11 session/connection. Clipboard requires xclip. |
| Voice edit | **Not implemented:** no provably fresh selection capture. Both Linux sessions still fail closed with `SelectionError::Unavailable`, unit-tested; they do not treat an unreadable selection as empty. |
| Tray | Implemented, unobserved: asynchronous StatusNotifierWatcher host probe, monitored every five seconds; icon creation alone is not considered availability. Without a host, open the main window and allow close to quit. UI recommends GNOME AppIndicator. Host appearance/disappearance refreshes UI capability. |
| Autostart / CLI | Entry text and AppImage path selection are unit-tested. Autostart points at the persistent APPIMAGE path, not `/tmp/.mount_*`. AppImage `--cli` executes its resource CLI before GTK/data/single-instance startup; UI copies a quoted persistent image command. Packaged execution is unobserved. |
| Media / secrets / local models | Named-player pause/resume and Secret Service helpers are preserved. Per-stream ducking is implemented but unobserved (see the media section below); local models remain unavailable. No new live acceptance. |
| Updates | Release AppImages expose signed updater check/install and restart after replacement; debug and native packages remain disabled/package-manager-only. Runtime gating is unit-tested. No update download, installation or restart was performed. |

### Dependencies

All new direct dependencies are Linux-only. `reis` implements the MIT libei protocol in Rust without
native libei; `x11rb` implements XGrabKey/XTest/XKB without libXtst; `gtk` exposes Tauri's existing GTK3
runtime; `gtk-layer-shell` provides the native layer-shell surface. ashpd's `remote_desktop` feature
provides permission/session/EIS access; `screencast` is needed for ashpd 0.13's shared response type,
not for screen capture (only the keyboard device is requested). No code was copied from dikte.

### Packaging

AppImage/deb config, a local-checkout PKGBUILD, signed Linux asset preparation/verification and a
single Windows/Linux manifest aggregation job are implemented. Windows installer asset names,
updater endpoint/public key and NSIS guards are unchanged. CI checks/builds the full Linux app.
The PKGBUILD snapshots the current source, including uncommitted edits; it is not yet a checksum-pinned
public/AUR source recipe. See [packaging instructions](../../crates/app/linux/PACKAGING.md).

A deb was built locally with Tauri CLI 2.12.0, unsigned and with updater artifacts turned off. It holds both
binaries, both desktop entries and the CLI, and its control file carries the configured dependencies. That build
predates adding `libayatana-appindicator3-1` and `libasound2` to the list. The deb has not been installed.
The AppImage, the Arch package, signatures, update and restart, and every remote CI workflow have not run.

Tauri CLI 2.12.0 panics during its build step when the CLI binary itself sits under a non-ASCII path, because it
passes a mangled path in the environment. Running it from an ASCII path avoids this.

### Verification

Run on the desktop session, outside the implementation sandbox, on 2026-11-02:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Pass. |
| `cargo test --workspace` | 600 passed, 0 failed, 3 ignored. |
| App crate tests with no display, session bus or runtime directory | 301 passed, 0 failed. |
| `npm run check`, `npm test`, `npm run build` | 0 errors, 0 warnings; 152 tests; build passes. |
| `node --test scripts/release/*.test.mjs` | 19 passed. |
| Debug build in KDE Wayland, all eight pages and the setup route | No console error, rejection or horizontal overflow. Rendered pages were not looked at. |
| Capabilities in that session | Overlay and tray report available; hold, per-app rules, ducking and updates report unavailable. |
| `opit status` against the running app | Pass. |
| Release binary with embedded UI | Starts and reaches ready with a scratch data folder; run for eight seconds. |
| Windows target | **Not compiled.** `cargo check --target x86_64-pc-windows-gnu` stops in the `aws-lc-sys` build script because this machine has no MinGW compiler. |

The [continuation report](../archive/2026-11-02-linux-continuation-report.md) keeps the file inventory. Its test
failures came from the sandbox, which blocked socket binding, and do not reproduce here.

### Pending owner checks / open decisions

- Run Windows full-workspace CI and the manual Windows acceptance; no Windows build ran here.
- Install the deb; build the AppImage and Arch packages; validate persistent AppImage autostart and `--cli toggle`.
- Test KDE/GNOME command registration and actual start/stop/cancel; see the current shortcut acceptance list above.
- RemoteDesktop permission is not persisted, so each app launch asks on its first paste, for up to 25 seconds. Decide whether to use the restore token after testing it.
- Test insertion/clipboard restoration and multiple queued results in a disposable editor; copy a
  recovery result manually, with History off and clipboard helper unavailable.
- Test layer-shell/X11 focus and click-through, no-layer-shell notifications, GNOME without a tray,
  host loss/reappearance, close/quit/autostart and each page/theme. Nobody has looked at the rendered pages.
- Test X11 hotkeys, keymap changes, XTest and per-app rules. No X11 desktop has been run.
- Implement safe selection capture/voice edit. Linux command shortcuts cannot provide hold mode.
- Cancel is optional and empty by default; verify deliberate installation and desktop conflicts.
- Publishable source PKGBUILD and signed AppImage replacement/restart remain release acceptance work.


## Historical portal shortcut repair (2026-11-02, sandbox; superseded)

This source review and repair does not establish KDE/Wayland runtime acceptance. The owner supplied
one-process logs (first desktop-entry error followed by repeated 15-second timeouts) and a separate
Gio probe whose initial PATH problem was subsequently fixed. No GUI, portal call, real app data,
credentials, installed desktop entry or running process was accessed by this repair.

### Findings and implementation

- **Runtime lifetime confirmed in dependency source.** ashpd 0.13.13 `src/proxy.rs:27-43` caches a
  process-wide `zbus::Connection`. zbus 5.19.0 `src/abstractions/executor.rs` uses `tokio::task::spawn`
  for its Tokio driver. The old hotkey worker dropped its current-thread runtime after both failed
  setup and supersession, leaving the cached connection without its driver. Both hotkey and EI now
  create their ashpd clients on one process-lived current-thread runtime with a `LocalSet` (reis has
  non-Send callbacks). The worker remains driven between attempts and while EI is idle.
- **Duplicate Register is invalid.** ashpd `src/registry.rs` sends Register on every call and does
  not cache success. The local portal binary contains `Connection already associated with an
  application ID`; the [Registry contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html)
  explicitly permits registration only once per connection, before other portal methods. Both
  clients now share serialized registration that caches success only. Failure remains retryable;
  abandoning a caller does not cancel an in-flight Register or lose its eventual success.
  Saving unchanged keys retries an unavailable shortcut. The delayed startup worker reads current
  settings under the config-save lock so it cannot reinstall an earlier startup snapshot.
- **Interactive bind confirmed in upstream source, not on this desktop.** KDE's
  [Plasma 6.8 BindShortcuts implementation](https://raw.githubusercontent.com/KDE/xdg-desktop-portal-kde/Plasma/6.8/src/globalshortcuts.cpp)
  opens a dialog for new action IDs; returning IDs can complete immediately. ashpd `Proxy::request`
  waits for `Request::Response` inside `bind_shortcuts().await`. The new asynchronous attempt has
  a 120-second setup/bind deadline, runs outside startup and config-save waits, and reports timeout
  through the existing hotkey error/banner path. `sync_pause` applies immediately after scheduling.
  `HotkeyState.pending` drives a translated informational banner while waiting; the Change button
  finishes saving without waiting for the desktop dialog. Existing Linux state polling publishes
  subsequent transitions (normally within five seconds).
- **Supersession.** A watch-backed generation cancels pending setup and active signal waits. Late
  success, failure and session loss cannot publish over a newer attempt. Session guards close on
  refusal, timeout, supersession and loss, including sessions buffered in the creator channel or
  created after cancellation. Session creation is detached until its reply so a late session can
  be closed. Cleanup calls have a five-second bound; actual remote closure requires a responsive
  portal. Activation/deactivation signals are filtered by session path as well as generation.
- **Safe Linux keys and defaults.** UI capture and picker validation reject unmodified letters,
  digits, punctuation, keypad and editing/navigation keys; the Rust trigger builder refuses them
  too, including in the X11 backend. Saved invalid chords reach the unavailable path without a
  panic or config migration. Function/media keys remain usable alone. Core Linux defaults and UI
  Reset use Ctrl+Shift+F9, distinct from fixed Ctrl+Shift+Escape cancel. Windows defaults and
  validation stay unchanged. Existing saved chords, including modifier-only Windows chords, are
  preserved; choose a supported Linux chord explicitly.
- **Keysym correction.** Keypad, punctuation and media names now use XKB names (`KP_Add`,
  `apostrophe`, `XF86AudioPlay`, etc.) instead of uppercased Web key identifiers. KDE's
  [upstream parser](https://raw.githubusercontent.com/KDE/xdg-desktop-portal-kde/master/src/xdgshortcut.cpp)
  accepts uppercase CTRL/SHIFT/ALT/LOGO modifiers, resolves XKB names case-insensitively, then converts
  them through Qt. The Plasma 6.8 BindShortcuts source calls that parser; its exact 6.8 parser file
  could not be fetched. A local libxkbcommon-only check resolved all 114 emitted named/letter/digit/
  function key names checked, with zero invalid names. This does not prove Qt conversion, keymap,
  KGlobalAccel acceptance or actual desktop activation (especially XF86 and keypad keys).

### Repair verification

Results below are from the restricted implementation sandbox, separate from earlier desktop results.

The machine rebooted during the first verification attempt. Edited files were re-read, the remaining
startup/import changes completed, and every required command was run again sequentially after reboot.
Rust build/check commands below used `CARGO_TARGET_DIR=target/agent-build`; no release build ran.

| Command | Result after reboot |
| --- | --- |
| `cargo fmt --all --check` | Exit 0. |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | Exit 0; dev profile completed in 1.27 s. An earlier unused import from the startup refactor was fixed before this passing run. |
| `cargo test -p opit-speech-to-text --offline --no-fail-fast hotkey` | Exit 0; 26 passed, 0 failed, 0 ignored, 287 filtered out in the library; other targets had no matching tests. |
| `cd ui && npm run check` | Exit 0; 0 errors and 0 warnings. |
| `cd ui && npm test` | Exit 0; 27 test files, 169 passed. |
| `cd ui && npm run build` | Exit 0; 233 modules transformed, completed in 514 ms. This is the requested Vite UI build, not a Rust release build. |
| Local libxkbcommon name check (repeated after reboot) | 114 names resolved; 0 invalid. No portal or desktop connection. |

The Rust tests cover shared runtime survival with non-Send futures, register-once concurrency,
failure/retry and abandoned-waiter behavior, superseded state publication and dropped response waits,
virtual-time timeout, Linux trigger refusal/default/cancel collision, saved-config preservation,
first-run config and delayed-startup/current-config ordering. UI tests cover Linux typing-key refusal,
standalone function/media keys and platform-specific reset defaults. No socket `EPERM` occurred in
these selected checks; socket-binding tests and portal interactions were not exercised. No Windows
target was compiled, GUI started, process killed, desktop config changed, commit made or release built.

### Still requires the owner's desktop

- Retry in one process after a missing desktop entry is installed; approve a first-time KDE dialog
  after more than 15 seconds, reject it, and let it expire after two minutes. Verify pending and
  unavailable banners, and actual activation/cancel behavior.
- Change a shortcut during binding and after a successful session, then use EI paste in both
  initialization orders. Verify that old dialogs/sessions close and old keys stop firing.
- KDE treats preferred triggers as suggestions and retains returning action IDs; verify whether
  changes need its shortcut settings/configuration UI. No action IDs were changed by this repair.
- Test punctuation, keypad, media and layout-specific bindings through Qt/KGlobalAccel. Confirm
  desktop conflicts and that bare typing keys cannot be captured by any saved Linux config.
- Test Windows compilation/behavior on its runner. It cannot be compiled here. Portal service
  restart/re-registration is not implemented by this repair; restart the app after portal loss.


## Media ducking and cleanup credential saves (2026-11-02, sandbox)

Implemented, **not desktop-accepted**. No GUI, audio server, real application data, credentials,
provider requests or volume changes were used for this work. Tests use synthetic streams and a
fake Pulse backend with temporary recovery files.

### Design and boundaries

- The existing `opit-linux-media` worker owns every media helper call, `/proc` identity lookup
  and recovery-file operation. `pactl --format=json list sink-inputs` supplies channel maps and
  exact integer volumes; `set-sink-input-volume` sets each channel in channel-map order. No master
  sink command is used. The [upstream pactl implementation](https://github.com/pulseaudio/pulseaudio/blob/master/src/utils/pactl.c)
  defines this JSON/command contract. No dependency was added. PipeWire with pipewire-pulse and
  PulseAudio with JSON-capable pactl are the intended servers.
- Duck scales each stream's own channel values by the configured percentage (default 50), rounded
  to the nearest integer. Zero mutes the playback stream; 100 leaves it unchanged. Own-process,
  immediate-child feedback and matching application-ID streams are excluded, including client metadata.
  Streams without a client or complete channel map are left alone, as are streams with neither
  verifiable process identity nor a server serial.
- Only the engagement snapshot is ducked, up to 128 streams. New streams remain untouched to avoid
  unexpected changes to new playback/feedback; vanished streams are retired. Release compares the
  stream index, client, PID/start time, optional PipeWire serial, channel map, boot ID and server
  cookie. It restores exact original values only when current values still equal the ducked values.
  Detectable manual volume changes survive. Snapshot-to-command races, a volume change away and
  back between snapshots, or a server quantizing our requested values cannot be detected perfectly
  with short-lived pactl calls; these require desktop checks.
- `media-duck.json` in the app data root stores only identity, original and expected ducked channel
  values. An atomic, synced, mode-0600 write precedes every volume change. Release, Shutdown,
  sender loss and Drop-triggered Shutdown attempt recovery; startup retries the same journal before
  advertising availability or engaging again. Failed restorations retain their record. Malformed
  recovery data disables new ducking rather than overwriting it. No stream titles/media names are
  recorded or logged. Missing pactl, incompatible output, unavailable storage or helper failure
  produces one warning per worker lifetime and no dialog; partial engagement is rolled back.
- Each helper has a 750 ms deadline and a 4 MiB output limit; duck/restore command batches have
  five-second budgets. Recovery can be retried on release, shutdown and startup if a batch fails.
  These limits bound helper commands, not filesystem latency. Pause retains named-player playerctl
  status/pause/play behavior with the same helper bounds. `media_duck` reads worker readiness;
  both Settings choices and the Duck volume control are offered on Linux. The first-run Linux
  Pause override was removed; existing saved configurations are not rewritten.
- On every OS, saving a new cleanup key or a key-free connection runs a connection test in that
  user-triggered flow. A successful provider test enables cleanup through serialized `saveConfig`.
  A failed or raw-style skipped test does not enable it. Ordinary saves/tests of an already stored
  key do not re-enable a manual off choice. Saving the off switch for a key-free connection also
  skips automatic testing. Test with a newly entered key saves that tested connection as well.
  A response for a connection replaced during testing cannot enable the replacement. Core defaults
  and existing configurations are not migrated. Keys and response bodies are never logged.

### Acceptance still needed

Use a disposable app data root and multiple actual playback streams with unequal channel levels.
Verify 0/50/100 percent, app feedback exclusion, unchanged master volume, manual per-stream changes,
new/vanished streams, release/cancel/quit, unavailable/hung pactl and crash/restart recovery on the
same audio server. Also check server restart/index reuse and real pipewire-pulse channel rounding.
Confirm both Settings choices and translated hints, then save a valid/invalid cleanup key and a
key-free connection, disable cleanup afterwards, and repeat with raw style. No desktop or real
provider behavior is proved by the automated checks below.

### Verification for this change

The required commands ran sequentially in the restricted sandbox. Rust clippy/test used
`CARGO_TARGET_DIR=target/agent-build`; all commands exited 0. The first UI run reported one
unused media CSS selector; it was removed and the full UI check/test/build sequence was rerun.

| Command | Final result |
| --- | --- |
| `cargo fmt --all --check` | Exit 0, no diagnostics. |
| `CARGO_TARGET_DIR=target/agent-build cargo clippy --workspace --all-targets --offline -- -D warnings` | Exit 0, finished in 1.22 s. |
| `CARGO_TARGET_DIR=target/agent-build cargo test -p opit-speech-to-text --offline --no-fail-fast media` | Exit 0; library: 28 passed, 0 failed, 0 ignored, 299 filtered out; other targets had no matching tests. Build finished in 3.76 s, selected tests in 0.05 s. |
| `cd ui && npm run check` | Exit 0; 0 errors, 0 warnings. |
| `cd ui && npm test` | Exit 0; 27 test files, 173 passed, 0 failed (277 ms). |
| `cd ui && npm run build` | Exit 0; 233 modules, production build completed in 475 ms. |

The media filter includes 12 new Linux media tests and two startup media tests, plus existing
shared worker/controller and incidental name matches. It covers parsing, channel ordering,
self/feedback exclusion, arithmetic, recovery round trip, manual changes, missing/new/reused
streams, failed writes/restores, retry and unchanged saved config bytes. UI verification includes
four new pure cleanup policy tests. No live helper, audio server, provider or GUI was exercised.
Windows compilation and native behavior remain unverified.

Files changed by this task (other edits in the same dirty files were retained):

- `crates/app/src/platform/linux/media.rs` (new), media wiring/imports in `platform/linux/mod.rs`,
  the `media_duck` field in `commands/app.rs`, and media defaults/tests in `shell/startup.rs`.
- The media section of `ui/src/features/settings/Settings.svelte`,
  `CleanupSettings.svelte`, `cleanup.ts`, `cleanup.test.ts`, and `ui/src/lib/i18n/en.ts` / `tr.ts`.
- This Linux status page. Shortcut/hotkey/input implementation files were not edited.
