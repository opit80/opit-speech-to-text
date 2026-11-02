# Linux packaging

These recipes are implemented but no AppImage, deb or Arch package has been built in the
2026-11-02 continuation. CI and release workflows have not been run remotely.

## Local packages

Run packaging in a disposable source checkout. It builds release binaries, stages the CLI
under `target/cli-bundle/`, and writes output under `target/agent-build/`.

```sh
export CARGO_TARGET_DIR=target/agent-build
export CARGO_BUILD_JOBS=1
cargo tauri build --config crates/app/tauri.linux.conf.json --bundles appimage,deb --no-sign
```

For an Arch package, run `makepkg` from `crates/app/linux/`. This is a **local-checkout recipe**,
including uncommitted files, not an AUR source download recipe. It snapshots only source roots,
uses `cargo fetch --locked` and `npm ci`, and does not run the app or install anything. It needs
an already-installed Rust toolchain meeting the workspace Rust version and the declared build
packages. `makepkg --printsrcinfo` and `bash -n PKGBUILD` are metadata/syntax checks only.
The recipe's `pkgver` must match `[workspace.package].version`. A later public source recipe
must point at the final published source and record its checksum; no such source exists yet.

Runtime packages are explicit because WebKitGTK/GTK render the window, gtk-layer-shell provides
the Wayland overlay, ALSA provides audio devices, AppIndicator supplies the tray, libsecret
provides `secret-tool`, and wl-clipboard/xclip provide clipboard access. X11 sessions must expose
the XTest server extension; `x11rb` speaks the protocol without linking libXtst. Optional helpers provide media (`playerctl`), the Wayland input
fallback (`ydotool` plus its separately configured daemon), notifications (`libnotify`), sounds
(`libcanberra`), and portal permissions (`xdg-desktop-portal` plus the desktop's backend).
No helper is installed or configured by the application build. GNOME needs an AppIndicator
extension for a tray icon; without a tray the main window remains the access point.

For AppImage system shortcuts, use `'/absolute/path/to/Opit.AppImage' --cli toggle` (the UI copies
the quoted command prefix). `--cli` remounts the image and executes its bundled `opit` before GTK,
app data initialization or the single-instance plugin. Autostart also points at the persistent
AppImage file, never its temporary mount. These packaged launch paths still need an owner test.

Packages install the app and `opit` CLI, an app-id desktop entry required by the shortcut portal,
and the icon. Removal leaves user data in the locations returned by `dirs` (normally
`$XDG_CONFIG_HOME/opit-speech-to-text`, defaulting to `~/.config/opit-speech-to-text`).
Do not remove those paths automatically. API credentials are managed separately by Secret Service.

## Release pipeline

Windows and Linux build jobs each prepare and verify a private artifact directory. A single
`draft` job downloads both, verifies each signature and signed version with the existing public
updater key, and writes **one** `latest.json` and `SHA256SUMS`. Only that job uploads the draft.
No build job publishes a release. Windows asset names and the updater endpoint stay unchanged.

`latest-json.mjs` keeps the existing Windows-only `assets` and `verify` commands. It adds:

```sh
node scripts/release/latest-json.mjs linux-assets vX.Y.Z <appimage-dir> <deb-dir> <linux-out>
node scripts/release/latest-json.mjs merge-assets vX.Y.Z <windows-out> <linux-out> <combined-out>
node scripts/release/latest-json.mjs verify vX.Y.Z <combined-out>
```

The manifest includes `windows-x86_64` and `linux-x86_64`; only AppImage is an updater target.
The deb is listed in SHA256SUMS for a package manager/manual installation, not self-update.
AppImage bytes and their trusted version comments are verified before writing a manifest;
modified packages, URLs, unknown platforms or checksums fail verification. The tests generate
unrelated disposable keys only in memory; they never use the application's private signing key.

The Linux release runner uses Ubuntu 22.04 to limit the glibc baseline, following
[Tauri's AppImage guidance](https://v2.tauri.app/distribute/appimage/). Build dependencies follow
[Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/) with ALSA and
`libgtk-layer-shell-dev` added for the audio and overlay backends. Package execution and the
actual minimum supported distribution remain acceptance work.
