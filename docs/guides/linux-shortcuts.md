# Linux shortcuts

Linux shortcuts are registered with the desktop as commands. The default dictation shortcut is
`Ctrl+Shift+F9`. There is no GlobalShortcuts portal or `/dev/input` listener. Windows keeps its
existing capture and hook behavior. Existing config files are not migrated or rewritten at startup.

Each Linux shortcut field offers three ways to fill in a combination:

- **Ready-made shortcuts**: pick `Ctrl+Shift+F9`, `Ctrl+Shift+F10`, `Shift+Super+F8`,
  `Ctrl+Shift+F11` or `Ctrl+Alt+Space`, or **Custom** to start from an empty field.
- **Record**: press the keys while the Opit window has focus. Modifiers alone keep it waiting;
  a refused combination shows why and keeps recording. **Esc**, **Cancel capture**, leaving the
  field or window, or 10 seconds without a valid combination ends recording and restores the
  previous text. While recording, shortcut commands relayed from the desktop are ignored. If one
  arrives, recording stops with a note that the combination is already a desktop shortcut;
  pick another one.
- Typed text, such as `Ctrl+Shift+F9`.

Choosing or recording a combination only fills the field. Nothing is registered with the desktop
until you choose **Install** (shown on KDE and GNOME; other desktops show **Apply shortcut**),
which validates and saves the combination and then registers it.
Bare typing keys are refused. Ctrl/Control, Shift, Alt and Super/Meta/Win map to the existing
config key names. Modifier sides are equivalent to the desktop. Each input shows the registration
read from the desktop; **Refresh shortcut status** reads it again. **Remove** affects only Opit's
entry for that action. Optional shortcuts can be left empty. In a style editor, apply and save
the style first, then install its shortcut from the style list.

On KDE and GNOME, Opit also reconciles its own entries with the saved settings when it starts and
when a saved change alters a shortcut mapping: matching entries are left alone, missing or
outdated ones are installed, and cleared actions are removed. Only Opit's own entries are touched.
Conflicts, invalid saved combinations and helper failures are reported in the shortcut status,
without a dialog.

| Desktop | Registration |
| --- | --- |
| KDE, X11 or Wayland | Hidden desktop command entries and kglobalaccel. Live assignment uses D-Bus, then checks readback and keeps `kglobalshortcutsrc` consistent. If live assignment cannot be confirmed, the status says it applies after next login. Known key conflicts are refused. Requires `kwriteconfig6`, `kreadconfig6` and `kbuildsycoca6`. |
| GNOME, X11 or Wayland | `gsettings` custom keybindings in stable app-owned paths; every other list entry is preserved. Applies immediately. |
| Other X11 desktops | Existing x11rb grabs; **Apply shortcut** saves the combination and registers it in the running app. |
| Other Wayland desktops | No automatic registration. Copy the exact command shown for the action into the desktop's shortcut settings. |

Commands use the absolute current executable, or the persistent AppImage path, followed by
`--shortcut <action>`. They work without a separate CLI executable. Keep an AppImage/plain binary
at the same path; after moving it, reinstall the shortcut from the new copy. If the app is closed,
the command exits quietly and never starts a GUI. Start Opit normally or enable its autostart.
An explicit `OPIT_DATA_DIR` override is carried into registered/manual commands with a quoted
`/usr/bin/env` argument, so a scratch instance uses the same socket even when launched by the desktop.

| Configurable action | Command action |
| --- | --- |
| Start/stop dictation | `dictation` |
| Pause/resume recording | `pause` |
| Cancel (optional; empty by default) | `cancel` |
| Ask (microphone; press again to send) | `ask` |
| Edit selected text by voice | `edit` |
| Dictate and translate | `translate` |
| Cycle cleanup styles | `style_cycle` |
| Select a particular cleanup style | `style:<id>` |

All configurable actions have commands; none is hidden for lack of a command. Linux voice edit
still refuses safely because fresh selection capture is unavailable. The internal `probe` action
is reserved for setup verification, not a configurable shortcut.

Command shortcuts deliver presses only. Hold-to-talk is hidden on Linux, and an existing saved
hold preference acts as toggle without changing that preference. Escape is never automatically
grabbed during recording. Use the overlay/tray cancel action or deliberately configure the optional
cancel shortcut. Disabling or pausing shortcuts also suppresses desktop command presses.

The setup shortcut step tests a real subprocess and Unix-socket round trip using the app executable.
It records no audio and is harmless even if the child arrives after the test is closed. This proves
that the command reaches the running app; it does not prove the desktop activates the selected keys.
After installation, test the keys in a disposable editor. KDE login fallback may require logging in
again before that check. The owner has checked installation, registry readback and the default
`Ctrl+Shift+F9` trigger on KDE Plasma (Wayland). GNOME, X11 and other-Wayland paths are implemented
but not yet desktop-verified; see [Linux status](../development/linux-status.md) for evidence and
pending acceptance.
