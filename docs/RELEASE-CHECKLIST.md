# Release checklist

Run this for every release on a **clean Windows 10 or 11 x64** (a fresh VM or Windows Sandbox,
signed in as a standard user), with the installer from the **draft** GitHub Release. Tick every
line; anything that fails blocks publishing. Record the version, date, machine and results in the
release notes draft.

## Before tagging
- [ ] `version` under `[workspace.package]` in `Cargo.toml` is the new version; CI is green on `main`.
- [ ] `git tag vX.Y.Z` points at that commit; the *Release* workflow finished and created a draft
      with `opit-speech-to-text_X.Y.Z_x64-setup.exe`, its `.sig` and `latest.json`.
- [ ] `latest.json` → `version` is X.Y.Z, `platforms."windows-x86_64".url` ends in the asset name
      above, `platforms."windows-x86_64".signature` equals the `.sig` content.

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
- [ ] Paste failure path: an admin window gets the text on the clipboard with the overlay hint
      ("In clipboard — press Ctrl+V to paste").
- [ ] Tray: left click starts/stops; menu Open / Profile ▸ / Pause shortcut / Quit work.
- [ ] Close the window → app stays in the tray; Task Manager RAM in the tray < 40 MB: ____ MB.
- [ ] Sign out and in: the app starts in the tray (autostart), shortcut works.
- [ ] Settings → About shows version X.Y.Z. Settings → Updates: "Check for updates automatically"
      is on; **Check now** says "You have the latest version." For the very first release (no
      published release yet) it says "Could not check for updates: Could not fetch a valid
      release JSON from the remote" instead, and no banner appears.

## Update

Run this from the second release on. For the very first release: publish vX.Y.Z, then cut
vX.Y.Z+1 at once and run this section before telling anyone about the release.

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
