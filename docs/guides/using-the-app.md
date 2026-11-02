# Using Opit Speech to Text

Opit Speech to Text is a tray app for Windows 10/11 and Linux. Press a shortcut, speak, and the
transcript is pasted where your cursor is or left on the clipboard. Transcription uses your own API
key. This guide covers both platforms; [Linux differences](#linux-differences) lists what changes on
Linux.

## First run

The app puts an icon in the tray and opens its window. A setup wizard opens on the first start:

1. **Language**: the language of the app's menus and messages (English or Turkish). The language
   you dictate in is set per profile.
2. **Provider**: ElevenLabs Scribe v2 (recommended), Groq, OpenAI, Deepgram, a local model
   (Windows only) or your own OpenAI-compatible server. Paste your API key and test the connection.
3. **Microphone**: pick a device and check that the level meter moves.
4. **Shortcut**: choose the dictation shortcut and, on Windows, the mode. The Windows default is
   **Right Ctrl + Right Shift**; the Linux default is `Ctrl+Shift+F9`. On Linux, **Test command**
   checks that the shortcut command reaches the running app before **Next** is enabled.
5. **Context and startup**: optionally describe what you dictate, and choose whether the app
   starts when you sign in.
6. **Try it**: dictate one sentence into a text editor, or start a test from the wizard itself.

**Skip** moves past only the current step and keeps its saved settings; unsaved provider drafts are
discarded. It never finishes setup. Use **Finish** (or **Finish without trying**) on the last step.
Until then, the wizard opens again whenever the window opens. The wizard has no AI cleanup step and
does not change saved cleanup settings.

## Dictating

Press the shortcut, speak, then press it again. On Windows you can also tap **Ctrl** alone to stop,
**Esc** cancels, and push-to-talk mode lets you hold the shortcut while you speak. A dictation
started from the app's own window goes to the clipboard instead of being pasted.

The latest dictation stays on the clipboard by default, and each new dictation replaces it. To bring
back the previous clipboard after automatic pasting, enable **Restore the clipboard after pasting**
in Settings → Advanced → Text insertion and overlay.

## Pages

The sidebar has Home, History, Files, Rules, Profiles and Settings. Usage opens from the History
header.

| Page | What it does |
|---|---|
| Home | Dictation status, a start/stop button, a profile switch when two or more profiles exist, and up to five latest dictations. Warns when the active profile has no API key. |
| History | Search every dictation (Turkish letters match loosely: `ı`/`i`, `ş`/`s` …), copy, delete, clear, and play the audio when audio saving is on. Select a misrecognised word and use **Add correction rule** to fix it in future dictations, with a before/after preview. |
| Files | Transcribe an audio or video file with the active profile while dictation stays available. See [File transcription](file-transcription.md). |
| Rules | Edit personal rules (`user.yaml`) as a table or raw YAML. Expand **Try on text** to preview changes, or the context section to set a context sentence and inspect the prompt budget. |
| Profiles | Transcription providers from presets: base URL, model, language, prompt options and a fallback profile. Store or remove the API key and test the connection. |
| Settings | Dictation, General, History and privacy, AI cleanup, Updates and about, and Advanced. |
| Usage (from the History header) | Dictation count, total recording time, written characters and average processing time for today, the last seven local calendar days or all retained history. Deleted records disappear from the totals; dictations made with history off are not counted. This is local activity, not provider billing or quota. |

Closing the window keeps the app running in the tray; open it again from the tray icon.

## Settings

Most Settings controls save immediately. AI cleanup, Ask, profiles and the rules editor have a
**Save** button.

**Simplified controls.** Settings keeps dictation, general, privacy and update controls open. AI
cleanup and Advanced start collapsed; errors reveal the affected controls. Extra shortcuts, recording
and media options, text insertion, history limits, Ask, per-app rules and folders are in Advanced.
Style shortcuts are in each style editor's **More** section. Profile editors keep language visible
and put format, processing and fallback options under **More**. Personal rules open by default; text
preview and context/prompt details expand on demand. Hidden settings retain their saved values and
continue to apply.

**AI cleanup** is optional. Saving a working key turns it on; see [AI cleanup](ai-cleanup.md).

**Media while recording.** **Pause or lower media while recording** is on by default. Its mode, in
Settings → Advanced → Recording and media, defaults to lowering (ducking) other apps' volume to 50%
of their own level; **Pause** pauses media that is playing and resumes only that afterwards.

**Number spelling.** Rules → **Write numbers as words** spells standalone integers in Turkish or
English, following the profile that produced the transcript (including fallback). It defaults off,
needs the profile's **Apply rules** option, preserves the raw transcript, and leaves dates, times,
decimals, leading-zero identifiers and unsupported languages unchanged. For example, `12 kişi`
becomes `On iki kişi`; turning the switch off preserves the provider's output.

**Personal rules and silence filtering.** Corrections, replacements and terminology come only from
your personal `user.yaml`; existing files keep the same format. The app has no built-in rule packs
or in-app Guide page. Known whole-output silence hallucinations (including repeated stock phrases)
are still filtered for audio of at most six seconds when **Apply rules** is on.

## Shortcuts

**Windows.** Single letters and modifiers, navigation, punctuation, numpad, media, Windows keys and
combinations can be selected. Capture by pressing the keys, or build a chord with **Choose a key
from the list** → **Add key** → **Apply shortcut**. The keys retain their normal function in other
apps; system shortcuts can be intercepted by Windows. Hardware-only keys such as Fn are unavailable,
and Num Lock affects numpad keys. If Escape is part of the configured shortcut, cancel a dictation
from Home instead. Capture ends after 30 seconds or when focus leaves the capture field.

**Linux.** Shortcuts are registered with the desktop as commands. Pick a ready-made combination,
record one in the window or type it, then choose **Install** on KDE and GNOME (**Apply shortcut**
on other desktops). See
[Linux shortcuts](linux-shortcuts.md).

## Where settings live

| | Windows | Linux |
|---|---|---|
| Settings and data | `%APPDATA%\opit-speech-to-text\` | `$XDG_CONFIG_HOME/opit-speech-to-text/` (normally `~/.config/opit-speech-to-text/`) |
| API keys | Windows Credential Manager, entries ending in `.opit-speech-to-text` | Secret Service through `secret-tool`, with attributes `service opit-speech-to-text` and `key_ref <reference>` |

The data folder holds `config.json` (settings and profiles), `rules/user.yaml` (your personal
rules), `history.db`, `audio/` (only when audio saving is on) and `logs/` (7 days). On Windows,
downloaded local models are kept in `%LOCALAPPDATA%\opit-speech-to-text\local\`. On Linux, media
ducking keeps a small `media-duck.json` recovery record in the data folder so that volumes can be
restored after a crash. API keys never go into `config.json`. The logs never contain what you said
or your keys.

## Start at login

The start-at-login switch (on by default) is applied when the setup wizard is finished, never
before. Debug builds never register it.

- **Windows** (**Start with Windows**): writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Opit Speech to Text`.
- **Linux** (**Start with Linux**): writes the XDG autostart entry
  `$XDG_CONFIG_HOME/autostart/io.github.opit80.opit-speech-to-text.desktop` (normally under
  `~/.config/autostart/`). For an AppImage it points at the AppImage file, not its temporary mount.

Turning the switch off removes the entry.

## Updates

The Windows installer and the Linux AppImage update themselves. The app asks GitHub for a newer
version 20 seconds after it starts and then once a day while it runs. Settings → Updates and about
→ **Check for updates automatically** controls this; turn it off and the app never contacts GitHub
on its own (**Check now** still works).

When a newer version exists, a banner and Settings → Updates and about offer **Install and
restart**. The app downloads the update, checks its signature against the key built into it, and
installs it: on Windows it runs the installer with a small progress window, on Linux it replaces
the AppImage. It then starts again. Your settings, history, rules and API keys are kept. The button
is disabled while a dictation is recording, transcribing or pasting: finish the dictation first.

The Linux deb package and other non-AppImage Linux installs do not update themselves: Settings
shows that updates come from your package manager. On Windows you can also update by hand:
download the newer installer and run it over the old one.

## Uninstall

### Windows

Windows Settings → Apps → Installed apps → *Opit Speech to Text* → Uninstall.

The uninstaller always removes the program, its shortcuts and the "Start with Windows" entry. Tick
**Delete the application data** to also remove:

- `%APPDATA%\opit-speech-to-text\` (settings, `rules\user.yaml`, history, audio, logs),
- downloaded local models in `%LOCALAPPDATA%\opit-speech-to-text\local\`,
- the WebView data in `%LOCALAPPDATA%\io.github.opit80.opit-speech-to-text`,
- the preset and current profile API keys in Windows Credential Manager. Keys orphaned by an
  earlier configuration reset may remain; remove these manually as described below.

**Back up `rules\user.yaml` first if you want to keep your rules.**

If you uninstalled without ticking the box, the data stays for a reinstall. To remove it by hand,
delete the folders above and the Credential Manager entries whose names end in
`.opit-speech-to-text`: list them with `cmdkey /list`, then delete each one, for example
`cmdkey /delete:groq.opit-speech-to-text`.

### Linux

Before removing the app, turn off **Start with Linux** and use **Remove** on each installed
shortcut in Settings, so that Opit's autostart entry and desktop shortcut entries are removed.

- **deb**: remove the package with your package manager (for example `sudo apt remove
  opit-speech-to-text`; check the installed name with `dpkg -l | grep -i opit`).
- **AppImage**: delete the AppImage file.

Removing the app never deletes your data. To remove it by hand, delete
`~/.config/opit-speech-to-text/` (or the folder under `$XDG_CONFIG_HOME`) and clear each stored key,
for example `secret-tool clear service opit-speech-to-text key_ref groq`. **Back up
`rules/user.yaml` first if you want to keep your rules.**

## Privacy

- Recorded audio goes only to the transcription provider of the active profile, or to its fallback
  profile if you set one and the first attempt fails.
- When AI cleanup is on, the transcript (after local rules) goes to the cleanup service you
  configured. Voice edit and dictate-and-translate also send text to that cleanup connection when
  you use them. Ask sends your request to the Ask agent you configured. No audio is sent to these
  services.
- Local model downloads happen only when you ask for them. Local models are available on Windows
  only.
- The update check to GitHub can be turned off (see [Updates](#updates)).
- There is no telemetry. The logs never contain what you said or your API keys.

## Linux differences

Linux builds ship as an AppImage and a deb package. The owner has used the app on KDE Plasma
(Wayland). GNOME and X11 paths are implemented but not yet desktop-verified; see
[Linux status](../development/linux-status.md).

- **Toggle only.** Desktop command shortcuts deliver presses, not releases, so there is no
  hold-to-talk. A saved hold preference acts as toggle and is kept unchanged.
- **Cancel.** The cancel shortcut is optional and empty by default; Escape is not grabbed during
  recording. Cancel from the overlay or tray, or install a cancel shortcut.
- **Media.** Duck lowers each playing stream through `pactl` (PipeWire with pipewire-pulse, or
  PulseAudio); Pause uses `playerctl`. Missing helpers produce a warning, not a dialog.
- **Pasting.** On Wayland, Opit asks for keyboard access through the RemoteDesktop portal and types
  the paste keys through libei, then tries `ydotool`, then leaves the text on the clipboard with a
  notification. Each app launch asks for the portal permission on its first paste. On X11 it uses
  XTest. The clipboard uses `wl-clipboard` on Wayland and `xclip` on X11. The **Type** paste method
  is not offered. Results that could not be delivered stay on Home for this session.
- **Overlay.** A native GTK status panel: a layer-shell surface on Wayland compositors that support
  it, a dock window on X11. Without a supported surface, status appears as notifications.
- **Tray.** GNOME needs an AppIndicator extension for the tray icon. Without a tray, the main window
  is the access point and closing it quits the app.
- **Not available on Linux:** local models, voice edit of selected text (it refuses safely), and
  per-app rules outside X11 sessions.
