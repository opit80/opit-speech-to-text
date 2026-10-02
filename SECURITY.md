# Security policy

## Supported versions

Security fixes are provided for the latest stable release only.

## Reporting a vulnerability

Do not put vulnerabilities, API keys, transcripts or recordings in public issues.
Use
**Security → Advisories → Report a vulnerability** on
`https://github.com/opit80/opit-speech-to-text`.

Include the affected version, reproduction steps, expected/actual behavior and impact.
Use synthetic data and remove credentials and personal information from attachments.

## Update trust

The installed app verifies the updater signature against its embedded public key and requires
the release version in the signed comment. The private key is never part of the repository.
Updater signatures are separate from Windows Authenticode code signing. A valid updater
signature does not mean that Windows will suppress SmartScreen warnings.
