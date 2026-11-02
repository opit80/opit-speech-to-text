# Documentation

## Guides

- [Using Opit Speech to Text](guides/using-the-app.md)
- [AI cleanup in Opit Speech to Text](guides/ai-cleanup.md)
- [Ask mode in Opit Speech to Text](guides/ask-mode.md)
- [Opit Speech to Text command-line control](guides/cli.md)
- [Linux shortcuts](guides/linux-shortcuts.md)
- [Measuring accuracy with `opit-eval`](guides/eval.md)
- [File transcription in Opit Speech to Text](guides/file-transcription.md)
- [Local transcription in Opit Speech to Text](guides/local-transcription.md)

The app uses personal `user.yaml` rules and an internal short-audio silence filter; it no
longer ships rule packs or an in-app Guide page. The guides above remain repository documentation.
The dev-only whole-app preview starts at Home (`ui/app-preview.html`).

## Reference

- [Per-app rules](reference/app-rules.md)
- [Cleanup styles: backend and UI contract](reference/cleanup-styles.md)
- [Recording queue and pause in Opit Speech to Text](reference/dictation-upgrades.md)
- [ElevenLabs Scribe v2 in Opit Speech to Text](reference/elevenlabs-scribe.md)
- [Learning dictionary: backend and UI contract](reference/learning-dictionary.md)
- [Live text while speaking](reference/live-text.md)
- [Media during microphone recording](reference/media-control.md)
- [Paste methods](reference/paste-methods.md)
- [Voice edit and translation](reference/voice-modes.md)

## Release

- [Releasing](release/RELEASING.md)
- [Release checklist](release/RELEASE-CHECKLIST.md)
- [SignPath Foundation application — notes for the maintainer](release/signpath-application.md)

## Historical release snapshots (2026-10-03)

- [First release runbook](archive/FIRST-RELEASE.md)
- [Pre-release readiness report — 2026-10-03](archive/PRE-RELEASE-REPORT.md)
- [Publication and updater report — 2026-10-03](archive/PUBLICATION-REPORT.md)
- [Linux continuation plan — 2026-11-02](archive/2026-11-02-linux-continuation-plan.md)
- [Linux continuation report — 2026-11-02](archive/2026-11-02-linux-continuation-report.md)

The archived reports retain dated CI, packaging and updater-download evidence and explicitly
pending clean-Windows installation, restart and hardware checks. They are not current support claims.

## Development

- [Building and developing](development/building.md)
- [Known issues, extraction ledger and pending acceptance](development/known-issues.md)
- [Linux status](development/linux-status.md)
- [Contribution and verification instructions](../CONTRIBUTING.md)
- [Agent working rules and current layout](../AGENTS.md)
- [0.2.0 release notes (unreleased draft)](releases/0.2.0.md)
- [0.1.3 release notes](releases/0.1.3.md)

## Design and archived layout plan

- [Original design brief (2026-09-30)](design/2026-09-30-opit-speech-to-text-design.md)
- [Layout cleanup and Linux design (2026-10-30)](design/2026-10-30-layout-and-linux-design.md)
- [Layout cleanup implementation plan](archive/2026-10-30-layout-cleanup.md)

Both specs are retained under `design/`; the layout plan is archived under `archive/`.
Archiving the plan does not establish CI or desktop acceptance; pending checks remain in the known-issues ledger.
Completed implementation plans were removed after extraction; their original filenames, source
commit and item-by-item dispositions remain in the known-issues ledger and Git history.
Linux desktop evidence and outstanding acceptance are in the Linux status page.
