# Releasing

For maintainers. Releases are built by `.github/workflows/release.yml` and always end as a
**draft**; nothing reaches users until a person publishes it.

## Steps

1. Bump `version` under `[workspace.package]` in `Cargo.toml` and keep `Cargo.lock`,
   `ui/package.json` and `ui/package-lock.json` in sync. Add release notes under
   [`docs/releases/`](../releases/). Review and commit the changes on `main`.
2. `git tag vX.Y.Z` (exactly `v` plus that version) and push the tag. The *Release* workflow runs
   three jobs:
   - **windows** (`windows-latest`): checks the tag against `Cargo.toml` and the UI package files,
     refuses to continue if that release is already published, runs the Rust and UI checks and
     release-helper tests, builds the signed NSIS installer, prepares its `.sig`, `latest.json` and
     `SHA256SUMS`, verifies the installer signature and signed version against the app's public
     key, and runs the updater download test without installing.
   - **linux** (`ubuntu-22.04`, for an older glibc baseline): the same tag and published-release
     checks, Rust and UI checks, then `cargo tauri build --ci --bundles appimage,deb`. The AppImage
     gets an updater signature and is verified against the public key; the deb is not an updater
     target and is covered by `SHA256SUMS` only.
   - **draft**: downloads both platform artifacts, merges them into **one** `latest.json` (with
     `windows-x86_64` and `linux-x86_64` entries) and one `SHA256SUMS`, verifies every asset again
     and creates or refreshes the draft release. A rerun refreshes a draft; it refuses to overwrite
     an already published release.
3. Run [the release checklist](RELEASE-CHECKLIST.md) on a clean Windows with the draft's installer.
   The checklist has no Linux packaging section yet: install the draft's deb and AppImage on a Linux
   desktop and work through the pending owner checks in
   [Linux status](../development/linux-status.md). Then publish the draft with **Pre-release off**
   and **Set as latest release on**.
   Publishing is what makes
   `https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json` visible to
   installed apps; drafts are ignored.

## Release assets

`scripts/release/latest-json.mjs` names every asset; for version `X.Y.Z`:

| File | Purpose |
|---|---|
| `opit-speech-to-text_X.Y.Z_x64-setup.exe` | Windows installer and updater target |
| `opit-speech-to-text_X.Y.Z_x64-setup.exe.sig` | Updater signature for the installer |
| `opit-speech-to-text_X.Y.Z_amd64.AppImage` | Linux AppImage and updater target |
| `opit-speech-to-text_X.Y.Z_amd64.AppImage.sig` | Updater signature for the AppImage |
| `opit-speech-to-text_X.Y.Z_amd64.deb` | Linux deb for manual or package-manager installation (no self-update) |
| `latest.json` | Updater manifest for both platforms |
| `SHA256SUMS` | SHA-256 checksums of every other asset |

The helper commands, used by the workflow and runnable locally:

```sh
node scripts/release/latest-json.mjs check-version vX.Y.Z
node scripts/release/latest-json.mjs assets vX.Y.Z <nsis-bundle-dir> <windows-out>
node scripts/release/latest-json.mjs linux-assets vX.Y.Z <appimage-dir> <deb-dir> <linux-out>
node scripts/release/latest-json.mjs merge-assets vX.Y.Z <windows-out> <linux-out> <combined-out>
node scripts/release/latest-json.mjs verify vX.Y.Z <assets-dir>
```

## Signing key

Repository secrets: `TAURI_SIGNING_PRIVATE_KEY` (the content of the private key file) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Both the Windows and Linux jobs use them. Never regenerate the
key: installed apps accept only updates signed with it, so a new key strands every installed copy.
Only the public key belongs in `crates/app/tauri.conf.json`.

Updater signatures are separate from Windows Authenticode code signing; the installer is not
code-signed yet, so SmartScreen may warn. See [SignPath application notes](signpath-application.md).

For the history of the first publication, see [the first release runbook](../archive/FIRST-RELEASE.md).
