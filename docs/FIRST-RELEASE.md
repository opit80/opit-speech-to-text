# First release runbook

Candidate: **0.1.3**. Repository: **opit80/opit-speech-to-text**, public, MIT license.
These are the configured defaults; publication is a separate maintainer action.
The maintainer's installed local app is already 0.1.2. The candidate is 0.1.3 so the default
updater comparator can offer it to that installation; publishing another 0.1.2 would not.

## Local preparation

1. Review the complete source candidate, including previously uncommitted features. Keep
   unrelated changes out of release commits. Make sure `Cargo.toml`, UI package/lock versions
   and `Cargo.lock` agree.
2. Run the Rust, UI and release-helper checks in `CONTRIBUTING.md`.
3. Scan both Git history and candidate files for credentials and personal data. Keep the
   existing updater key outside the repository; confirm its backup separately.
4. Build with Tauri CLI 2.12.0 in a separate `CARGO_TARGET_DIR`. Supply the existing private
   key/password through the process environment, never through command-line arguments.
5. Prepare and verify the artifacts:

   ```sh
   node scripts/release/latest-json.mjs assets v0.1.3 <nsis-bundle-dir> release-assets/0.1.3
   node scripts/release/latest-json.mjs verify v0.1.3 release-assets/0.1.3
   ```

   Verification fails for the wrong key, tampered installer/comment, wrong signed version,
   inconsistent UI versions, wrong manifest/download URL or changed checksums.
   On Windows, set `OPIT_RELEASE_ASSETS` to that artifact directory and run
   `cargo test -p opit-speech-to-text --test updater_artifacts -- --ignored`. This exercises the
   actual Tauri updater on a loopback fixture: successful download and signature verification,
   rejection of modified installer bytes, and rejection of a forged manifest version. It never
   starts an installer, window, microphone or keyring operation. Production HTTPS settings stay
   unchanged. The release workflow runs it on its own generated artifacts before any draft upload.
6. Run `docs/RELEASE-CHECKLIST.md` in a clean Windows VM. Do not use the maintainer's running
   app or real data for destructive installer/uninstaller tests. `OPIT_DATA_DIR` alone does
   not isolate Windows credentials, autostart, installer registry entries or the clipboard.

## GitHub preparation (external actions)

1. Confirm the account and repository name. Create the repository without an initial
   README/license to preserve this repository's history. Review the code/history before making
   it public; a public repository is already publication of the source.
2. Enable MFA, private vulnerability reporting and protection for `main`. Add `origin` and
   push only reviewed release commits; merge the feature branch only after maintainer agreement.
3. Add repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from the existing secure files. Do not print or rotate
   the key. Confirm an offline/password-manager backup before retiring any local password copy.
4. Wait for Windows/Linux/UI CI to pass on the exact release commit.
5. Create/push the versioned tag **v0.1.3**, not a tag named `latest`.
6. The Release workflow builds verified assets and creates a **draft**. A rerun may replace
   draft assets, but refuses to overwrite an already published release.
7. Review draft notes from `docs/releases/0.1.3.md` and repeat installer acceptance using the
   actual CI-produced installer. Local binaries do not substitute for CI artifact verification.

## Publish as Latest

Only after acceptance, publish the draft with **Pre-release disabled** and
**Set as latest release enabled**. The equivalent explicit maintainer command is:

```sh
gh release edit v0.1.3 --repo opit80/opit-speech-to-text --draft=false --prerelease=false --latest
```

Verify unauthenticated access to
`https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json`
and its installer URL. The manifest must advertise 0.1.3 and the exact verified signature.

For a repeatable production-endpoint check on Windows, download the four release files and set
`OPIT_RELEASE_ASSETS` to that directory, then run:

```sh
cargo test -p opit-speech-to-text --test updater_live -- --ignored
```

This uses the actual Tauri updater and the configured public GitHub HTTPS endpoint. It verifies
that 0.1.2 discovers/downloads the signed release, downloaded bytes match the verified assets,
and an already-current version gets no update. It never installs or restarts the running app.

Test a lower installed version updating to 0.1.3, or use the next actual release to test
0.1.3 → a higher version. Do not create a fake release solely to increment the number.
Use a disposable Windows environment and verify restart, version, preserved data and the
dictation-busy guard. Complete this before broad distribution.

SignPath/Windows code signing remains a separate application/integration step. The existing
updater signature is required regardless of whether Authenticode signing is added later.
