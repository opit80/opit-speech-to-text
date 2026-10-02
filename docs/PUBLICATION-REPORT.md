# Publication and updater report — 2026-10-03

**Outcome:** [v0.1.3](https://github.com/opit80/opit-speech-to-text/releases/tag/v0.1.3) is published
as a stable **Latest** release. Source, release assets and updater downloads are public. This report
covers publication/download verification; the user explicitly excluded updating/restarting the
installed app.

## Repository and automation

- Public MIT repository: [opit80/opit-speech-to-text](https://github.com/opit80/opit-speech-to-text).
- Reviewed source was committed on feature branches and merged locally into `main` with `--no-ff`.
- Release tag `v0.1.3` points to `806fda89fa0853ae5926eb8677cb1e9372ca4370`.
- Private vulnerability reporting is enabled. Main requires Windows/Linux Rust, UI and
  release-script status checks for protected updates; force-push and branch deletion are disabled.
  Repository administrators retain GitHub's configured bypass behavior.
- Existing signing key/password were supplied as encrypted GitHub repository Secrets using
  stdin, without printing values or rotating/deleting the original files.
- [Source CI run 37069498081](https://github.com/opit80/opit-speech-to-text/actions/runs/37069498081)
  completed successfully on the release commit: Windows and Linux Rust, UI and release helpers.
- [Release run 37070626398](https://github.com/opit80/opit-speech-to-text/actions/runs/37070626398)
  completed successfully: checks, signed NSIS build, cryptographic verification, actual updater
  fixture download tests and draft upload. The draft was then explicitly published with
  `draft=false`, `prerelease=false` and Latest enabled.

The hosted Rust 1.99 Clippy found a constant-sized `chunks_exact` use in a UTF-16 test helper.
It was corrected to `as_chunks::<2>()`; no warnings or tests were suppressed. The release workflow
also explicitly installs rustfmt/clippy. Older failed/superseded runs remain in GitHub history;
the tagged commit's source CI and release run both passed.

## Published assets

| Asset | Size |
|---|---:|
| `opit-speech-to-text_0.1.3_x64-setup.exe` | 6,072,366 bytes (5.79 MiB) |
| `opit-speech-to-text_0.1.3_x64-setup.exe.sig` | 452 bytes |
| `latest.json` | 796 bytes |
| `SHA256SUMS` | 294 bytes |

Installer SHA-256:
`5a103b1640d52502666f1bb79ecd5e6cb586598d5e94188f345d7bb02fdf2cb6`.

The actual CI installer differs from the earlier locally built review candidate. Release files
were downloaded independently into `release-assets/github-0.1.3/` and verified with
`node scripts/release/latest-json.mjs verify v0.1.3 release-assets/github-0.1.3`.
Signature, signed version, manifest, download URL and checksums passed. Installer metadata reports
0.1.3; the package is below the 15 MB target. It is updater-signed, not Authenticode-signed.

## Production download evidence

- GitHub's latest-release API returns `v0.1.3`, `draft=false`, `prerelease=false`.
- Unauthenticated HTTPS GET of
  [latest.json](https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json)
  returned **HTTP 200** and version **0.1.3**.
- Its installer URL returned **HTTP 200** without authentication. Downloaded bytes exactly match
  the independently verified CI artifact and the SHA-256 above.
- Minisign verification against the application's unchanged public key passed, including the
  signed version. Existing installed copies therefore retain the same updater trust key.
- `cargo test -p opit-speech-to-text --test updater_live -- --ignored` passed against the actual
  configured public GitHub endpoint. A Tauri mock application at 0.1.2 discovered 0.1.3, downloaded
  and verified its installer; a current-version mock received no update. No installer was executed.

Local evidence is in `target/release-readiness/`: `public-download-proof.json`,
`published-updater-test.log`, build/check logs and release notes. Build/download outputs are ignored.

## Explicit verification boundary

The existing installed application remained at **0.1.2** and was not stopped, updated or restarted.
Real app data, clipboard, API credentials and autostart were not modified by this publication task.
Clean-machine installation/uninstallation, actual restart/data preservation, microphone/provider
dictation, tray RAM and SmartScreen interaction remain manual acceptance work in
[RELEASE-CHECKLIST.md](RELEASE-CHECKLIST.md). These are not claimed as passed.

The next release uses the same versioned-tag → verified draft → reviewed Latest publication
workflow. Do not move the published tag, replace its installer or regenerate the updater key.
