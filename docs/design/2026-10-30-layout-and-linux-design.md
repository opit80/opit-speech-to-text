# Repository layout cleanup and Linux support: design

Date: 2026-10-30. Status: draft, awaiting the owner's approval.

## Intent

Opit Speech to Text is a Windows dictation tray app. The owner uses Arch-based Linux (CachyOS, KDE Plasma on
Wayland) and wants the app on general Linux distributions while Windows keeps working unchanged. The repository is
public and will get a new public release after this work and after UI updates the owner will do themself, so it should
read as tidy to strangers and Linux should feel finished rather than a partial port.

## Goals

1. A tidier repository: clear module ownership, UI feature folders, curated documentation, explicit OS boundaries.
2. The full dictation flow on Linux: toggle hotkey, record, transcribe, insert at the cursor, overlay feedback, tray.
3. Windows behaviour stays the same, including installer, updater and stored data.
4. Linux packages can be built locally and in CI, ready for a later release.

## Non-goals

- No tag, release or publication in this work, and no version bump. Pushes go to the feature branch only.
- The owner's UI redesign. Only Linux-required UI changes happen here (see "UI boundary").
- Writing `README.md`. The owner and the agent will write it together in a later step. This work changes it only to
  fix links broken by file moves, and collects facts for it (desktops run, packages, known limits) in
  `docs/development/linux-status.md`.
- Rewriting git history.
- Local model server on Linux, and audio ducking on Linux. Both are follow-ups.
- Architectures other than x86_64 Linux (builder's call to revisit, marked below).

## Anti-goals: failure even if it technically works

- A Windows regression, or anything that strands installed copies.
- Losing a dictation: if insertion cannot happen, the text must still reach the user.
- A silent feature gap: anything unavailable on the current Linux desktop must say so in the UI, not fail quietly.
- Claiming support for a desktop nobody has run the app on.

## Decisions by the owner

- Linux and Windows are both supported, on general distributions.
- The owner dictates with toggle (start/stop), not hold-to-talk.
- All previously uncommitted work is one commit (`81f4072`), done before any move.
- Verification: each batch passes Windows CI on GitHub; the owner tries it by hand on Windows at the end. Feature branch
  pushes are allowed; `main`, tags and releases are not touched.
- No release in this session. UI updates by the owner come before the public release.

## Decisions by the author, open to objection

Scope and order:

- **Order:** documentation cleanup, Linux-enabling moves, tidy moves, UI feature folders, then the Linux port.
- **UI move before the owner's UI work,** to avoid conflicts. The owner holds UI edits until that batch lands.
- **Workspace roots stay:** `crates/`, `ui/`, `rules/`, `scripts/`, `crates/app/windows/` and crate names do not change.

Layout target:

```text
crates/app/src/
  shell/        Tauri lifecycle and presentation: startup, logging, events, i18n, tray, window, updates
  app_core/     split by concern: config, credentials, input, rules, history, types, tests/
  commands/     handlers by domain, one registry, command names unchanged
  controller/   handle, recording, transcription, delivery, live, status, tests/
  ask/          conversation and agent protocol
  local/        portable model logic; runtime/ holds OS-specific server metadata and discovery
  platform/     traits, platform factory, keys, media, fake, process.rs, microphone.rs (shared cpal)
    windows/    existing backends, plus process, hardware, uninstall
    linux/      hotkey, paster, clipboard, overlay, secrets, autostart, media, sounds, foreground, hardware
    unix/       process-group supervision
crates/core/src/cleanup/   config, prompts, response, transport, styles; public facade unchanged
crates/ipc/src/            protocol, framing, transport/{windows,unix}
ui/src/lib/                bridge/, state/, navigation/, components/ (primitives only), i18n/, utils/
ui/src/features/           home, dictation, history, profiles, rules, settings, setup, files, guide, usage, updates
ui/src/preview/            preview bootstrap
docs/                      guides/, reference/, development/, design/, release/, archive/, releases/, README.md index
```

Move batches, each one green and one commit, in this order:

1. Documentation: regroup `docs/`, extract still-valid deferred work and manual checks from `docs/superpowers/plans/`, then
   remove the plan files from the tree (git history keeps them). Keep the original design and this one under
   `docs/design/`. Keep tracked `.claude/settings.json`, which disables AI attribution. Replace status and history in `AGENTS.md` with current
   rules, updating its layout table. Fix public doc links to ignored `.superpowers` logs and local build paths.
2. Shared process supervision (`ask/process.rs` to `platform/process.rs`) and Windows hardware probing.
3. Platform factory replacing direct construction in `lib.rs`; isolate uninstall; share the cpal microphone source.
4. `shell/`.
5. `app_core/`.
6. Controller tests, then controller responsibilities (two commits).
7. Core `cleanup/`.
8. `commands/`.
9. IPC protocol, framing and transport. Keep `opit_ipc::windows` working through a re-export at first.
10. `local/` runtime separation.
11. UI `features/`, `lib/` groups, `preview/`.

Skipped on purpose: splitting `Settings.svelte` (left to the owner's UI work), splitting i18n dictionaries, `overlay.rs`,
`paster.rs`, `config.rs` or `history/`, and moving rule packs or scripts.

Linux behaviour:

| Area | Decision |
|---|---|
| Sessions | KDE and GNOME on Wayland, and X11, are primary targets. wlroots compositors are best effort. |
| Hotkey | Wayland: GlobalShortcuts portal, with the user confirming the binding in the desktop's dialog, restored on later runs where the portal allows. X11: `XGrabKey`. Toggle is required. Hold-to-talk is offered only where release events are confirmed; otherwise the option is disabled with an explanation. |
| Stop and cancel | Stop is the same hotkey. "Lone Ctrl stops" is not offered on Wayland. Global Escape swallowing is impossible there, so cancel is a separate shortcut plus an overlay button where the overlay is clickable. |
| Insertion | Clipboard plus simulated paste. Wayland: RemoteDesktop portal with libei; X11: XTest. |
| Missing portal | A Wayland session without the GlobalShortcuts portal is not blocked: setup explains it, and the user can bind `opit` CLI toggle in their system shortcuts. No minimum distro version is promised; `linux-status.md` lists what was run. |
| Insertion failure | Text stays on the clipboard and a notification says to press Ctrl+V. Results not yet inserted are also kept in memory for the session and shown in the main window, because a queued result can overwrite the clipboard and a clipboard write can fail. Nothing extra is written to disk when History is off, and nothing survives a restart in that case. |
| No tray host | GNOME without the AppIndicator extension has no tray. The app detects this: closing the window then quits instead of hiding, autostart opens the window instead of starting hidden, and the UI recommends the extension. Launching the app again focuses the existing window. |
| Clipboard restore | Best effort under the same rules as Windows: restore only if unchanged. Exact mechanism is the builder's, with the constraint that a failed restore cannot lose the dictation. |
| Overlay | Wayland: layer-shell, no focus, switchable input region. X11: override or dock-type window with shape input. Where neither exists (GNOME Wayland has no layer-shell), status appears through tray and notifications, and the UI says so. |
| Per-app rules | X11 works. Wayland has no general foreground-window API, so the feature shows as unavailable with an explanation. |
| Secrets | Secret Service (KWallet, GNOME Keyring). If none is running, setup says so and the key is not stored in plain text. |
| Autostart | `.desktop` file in `~/.config/autostart`, with `--autostart`. |
| Media | Pause through MPRIS. The default action on Linux is pause, because ducking is not available there. |
| Ask agents and CLI | Unix socket scoped to the user with `0600` permissions; process-group supervision replacing Job Objects. |
| Voice edit | Same insertion mechanism sends the copy shortcut. Fails closed if the selection cannot be read safely. |
| Local models | Hidden on Linux for now. Users can point at their own OpenAI-compatible server. |
| Data | `dirs` paths already resolve correctly. No migration. Removal: packages leave user data, and the docs list the paths. |

UI boundary: this work changes only what Linux requires. Windows-only wording is generalised or made
platform-specific in both `en.ts` and `tr.ts`, the `.exe` requirement in `ui/src/features/settings/app-rules.ts` becomes
platform-aware, `Win` key labels become `Super` on Linux, and a new capabilities command tells the UI what the
current platform and session can do, so that it can disable features with an explanation.

Packaging: AppImage, `.deb` and a PKGBUILD. Autoupdate works only in AppImage; other packages show that updates come
from the package manager. `latest-json.mjs` and `release.yml` gain Linux support without changing Windows asset names.
CI builds the Linux app and stops excluding it from checks once it compiles.

## Spike before the port

Before the Linux port, a throwaway prototype on the owner's KDE Wayland session checks the assumptions that come only from
protocol documents: portal shortcut with press and release, libei paste into a real app, layer-shell overlay without
focus theft, and the clipboard mechanism. Findings update this document. The spike is not kept.

### Findings (2026-10-31, KDE Plasma 6.8 Wayland, portal versions 2)

| Question | Result | What the port must do |
|---|---|---|
| Portal shortcut | Works. Press and release both arrive with correct timing (2013 ms and 314 ms holds, keys sent through `ydotool`). Hold-to-talk is viable on KDE. | Offer hold on KDE. Other desktops remain unverified. |
| App identity | An unsandboxed process is refused ("An app id is required"). `org.freedesktop.host.portal.Registry.Register` must come before other portal calls, and it fails ("App info not found") unless `<app id>.desktop` is installed. | Ship the `.desktop` file with every package. An AppImage writes its own on first run, before it asks for a shortcut. |
| Shortcut id | Each new id opens a KDE dialog and leaves an entry in `kglobalshortcutsrc`, even for the same keys. A known id binds again without a dialog. | Use fixed ids. Never derive one from a process or session. |
| Trigger keys | `Ctrl+Alt+D` never fired on the owner's machine, while `Ctrl+Shift+F9` did. The owner's layout uses `lv3:alt_switch`, which makes Alt a third-level key; this is the likely cause, not separately proven. The Windows default, `RightCtrl+RightShift`, is modifier-only and was not tried. | The Linux default has no Alt and no modifier-only combination, and users can change it. Setup confirms the shortcut fires before moving on, because a bound shortcut can still be dead. |
| libei paste | Works. A focused test window received Ctrl+V and its text length matched the clipboard. Evdev key codes were enough. | Use this path on KDE. |
| Paste permission | The restore token was accepted. `Start` took 7 ms on one later run and 2524 ms on another, and the owner did not observe whether a dialog appeared. | Open question: test whether later sessions run with no prompt, before relying on it for each dictation. |
| Overlay | Works. A GTK3 layer-shell surface appeared, and the owner could keep typing anywhere. | Use it where layer-shell exists. |
| Clipboard | Reads and writes work with no window of the app's own, through data-control. The test text was still readable after the process exited, most likely held by Klipper. Reading straight after a write returned the previous value, so the spike's restore check failed. | Do not treat an immediate read as the new value. Wait or verify the write before comparing, and keep clipboard tests away from a real user clipboard. |

Fallbacks, from the approach used by [dikte](https://github.com/yusufipk/dikte), a GPL-3.0 project. This project is MIT, so ideas only and no code:

- Paste where the RemoteDesktop portal is missing or refused: `ydotool`, when its daemon is available, reported plainly in setup.
- Shortcut where the portal is missing: bind the `opit` CLI toggle in system shortcuts, as already decided. Setup may do this itself through `gsettings` on GNOME.
- A `/dev/input` listener is not adopted. It needs the `input` group and can read every key the user types.

## Constraints

- All rules in `AGENTS.md`: English code and docs, en/tr parity, no transcript or key text in logs, blocking work off
  the main thread, no raw colours, `crates/core` free of Tauri and OS crates.
- Installed-copy invariants that must not change: product name, identifier `io.github.opit80.opit-speech-to-text`,
  executable names, `APP_DIR_NAME`, credential service and suffix, autostart value, updater endpoint and public key,
  asset naming, signing key, and the `$UpdateMode` guards.
- Layout batches change no behaviour and are not mixed with feature work.
- Layout batches keep today's Linux exclusion of the app crate until the port makes it compile.
- Windows cannot be built on the owner's Linux machine here. CI on Windows is the gate for each batch.

## Verification

- CI currently runs only on `main` and pull requests. The first commit adds `feat/**` to the push trigger in `ci.yml`, so
  the feature branch gets Windows and Linux CI without opening a pull request. That first push is the baseline for
  `81f4072`, whose app crate files have never been compiled.
- Every batch: the commands in `AGENTS.md`, sequentially, then Windows and Linux CI.
- Workers' sandbox blocks loopback sockets, so tests that bind sockets are run outside it; this is expected.
- Linux port: manual pass on KDE Wayland by the owner. `docs/development/linux-status.md` states which desktops were
  actually run and marks others as untested.
- End of work: the owner's manual Windows pass, using `docs/release/RELEASE-CHECKLIST.md` for the dictation, installer and update parts.

## Left to the builder

Crate choices for portal, libei, layer-shell and Secret Service. Exact internal file splits within the target tree.
Whether X11 and Wayland backends are separate files or sit behind one dispatcher. Exact capabilities command shape. Arch
targets beyond x86_64 if they are free. How the notification is sent, given that it needs no new UI dependency.
