# Pre-release readiness report — 2026-10-03

**Historical preparation snapshot.** Publication was subsequently authorized and completed;
see [PUBLICATION-REPORT.md](PUBLICATION-REPORT.md) for the live release and download evidence.

**Outcome:** the local 0.1.3 source candidate, updater fixes, automated checks and signed release
package are prepared. **This is not approval to publish:** clean-Windows acceptance, source
commit/merge, GitHub setup/CI and live installation/restart remain unverified or unperformed.

The first published release should be stable and explicitly marked **Latest**. The maintainer's
running installed app is already 0.1.2, so publishing another 0.1.2 would not offer it an update.
The workspace, Rust lockfile and UI package/lock versions now agree on **0.1.3**.

## Completed changes

- Installer bytes, Minisign key identity, cryptographic signatures and the signed version are
  verified before creating release assets. A mismatched signing key is a hard failure.
- `latest.json` is checked against the installer/signature/version/download URL. `SHA256SUMS`
  covers the installer, `.sig` and manifest. Only the four expected files are uploaded.
- Tag, workspace and UI package/lock versions must agree. Tag names are passed to workflow
  commands as environment data rather than interpolated shell source.
- Draft creation is rerunnable: an existing draft can be refreshed; a published release is
  refused. The workflow remains draft-only and never publishes automatically.
- The release workflow also runs formatting, clippy, UI checks/tests, release-helper tests and
  the actual updater download test before uploading a draft.
- A late check-command reply no longer overwrites a newer update event in the UI.
- Destructive uninstall cleanup confirms the app-close decision before deleting credentials,
  requires the data checkbox and skips updates and silent runs. Cleanup documentation now
  accurately covers preset/current profile credentials; orphaned credentials may remain.
- MIT license is retained. Contribution and security-reporting instructions, first-release
  runbook and 0.1.3 release notes are prepared. Secret/build/agent-workspace ignore rules are added.

## Verification performed on Windows 11

| Check | Observed result |
|---|---|
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --workspace` | 328 passed, 0 failed; 11 hardware/desktop tests and 1 artifact test ignored by default |
| Actual artifact test, explicitly selected after packaging | 1 passed, 0 failed |
| `npm ci` | 62 packages installed, audit reported 0 vulnerabilities |
| `npm run check` | 0 errors, 0 warnings |
| `npm test` | 58 tests in 12 files passed |
| `npm run build` | Passed; the dev-only browser preview is excluded |
| `node --test scripts/release/*.test.mjs` | 14 passed, 0 failed |
| Actionlint 1.7.12 | CI and release workflows passed static validation |
| Gitleaks 8.30.1, Git history | 76 commits scanned, no findings |
| Gitleaks 8.30.1, complete source candidate | No findings; final inventory and redacted report saved with local evidence |
| `cargo tauri build --ci` | Signed NSIS installer and updater signature produced in a separate target directory |
| Release asset verification | Signature, signed version, manifest and SHA-256 checksums passed |

The real updater test uses a Tauri mock application at version 0.1.2 and a loopback fixture server.
It downloads the actual 0.1.3 installer, verifies it, rejects modified installer bytes, rejects a
forged manifest version, and offers neither the current version nor a downgrade. **It never
executes an installer or proves GUI/Windows installation/restart/data-preservation behavior.**

The new Tauri mock integration test initially failed to load with `STATUS_ENTRYPOINT_NOT_FOUND`:
its separate executable lacked the Common Controls 6 manifest needed for `TaskDialogIndirect`.
A dedicated test manifest fixed the loader failure; subsequent tests passed. The application
keeps Tauri's own manifest. See [Microsoft's manifest guidance](https://learn.microsoft.com/en-us/windows/win32/controls/cookbook-overview).

The existing 0.1.0 installer was also verified against the unchanged application public key,
providing a continuity check for the updater key. Private key/password contents were not printed,
copied into the repository, regenerated or deleted. An independent secure backup is not confirmed.

## Prepared artifacts

Directory: `release-assets/0.1.3/` (Git-ignored).

- `opit-speech-to-text_0.1.3_x64-setup.exe`: **6,102,987 bytes (5.82 MiB)**, below the 15 MB target.
- `.sig`, `latest.json` and `SHA256SUMS` are alongside it.
- Installer ProductVersion/FileVersion: **0.1.3**.
- Installer SHA-256: `8c477b91b1d2554460688d6ee2e7697d3d4d4797077f1a610abe120f7f39a5b9`.
- Updater-signed, **not Windows Authenticode-signed**; SignPath integration remains separate work.

Local evidence is under `target/release-readiness/`: test/build logs, redacted Gitleaks reports,
source file/hash inventory, complete source candidate and its archive. The archive contains the
review candidate working tree, not a tagged release. Detection scans do not guarantee the absence
of every possible secret or personal detail; publication still requires source/history review.

## Git and user-data boundary

- Branch: **codex/release-readiness**, created from **codex/usage-guide** at `aefb2c9`.
- Pre-existing edits and untracked feature files remain in place. The candidate build includes
  these local features. They were not silently staged or committed as unrelated work.
- No commit, merge to `main`, remote addition, push, tag or GitHub release creation was performed.
- The original working-tree diff/status were recorded before this task. Build/scan outputs are ignored.
- The installed 0.1.2 application remained running as process 24756. It was not stopped or updated;
  real app data, Credential Manager entries, clipboard and autostart were not exercised by installer tests.

## Remaining before public release

1. Review and finalize the candidate source into scoped commits; approve the local merge to
   `main`. The dirty working tree is not a reproducible release tag yet.
2. Confirm a secure independent backup of the existing updater key and password.
3. Run the complete [release checklist](RELEASE-CHECKLIST.md) in a disposable clean Windows
   environment: installation, wizard/real microphone/provider, clipboard/hotkeys, autostart,
   uninstall cancellation/data options, latency and tray RAM. Windows Sandbox and WSL are not
   installed here; the existing user's running app is not a safe destructive test environment.
4. Confirm repository/account choice, create/configure GitHub, enable private vulnerability
   reporting/MFA, add Secrets and send reviewed source. Making a public repository publishes the
   source immediately; this task performed no external publication.
5. Obtain actual Windows/Linux/UI GitHub CI results for the release commit. Actionlint and local
   Windows checks do not prove hosted CI or Linux execution.
6. Create/push `v0.1.3`, review the CI-produced draft and repeat installer acceptance on its files.
7. Publish with Pre-release off and Latest on, then verify unauthenticated manifest/asset access
   and a real lower-version update: install, restart, new version, preserved data and dictation guard.

Execution steps and the explicit Latest publication command are in
[FIRST-RELEASE.md](FIRST-RELEASE.md); draft notes are in [releases/0.1.3.md](releases/0.1.3.md).
