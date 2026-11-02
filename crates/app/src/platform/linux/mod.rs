//! Linux backends. KDE/GNOME own command shortcuts; other X11 desktops use grabs.

mod autostart;
pub(crate) mod cli;
mod ei;
mod input;
pub(crate) mod media;
pub(crate) mod overlay;
mod portal;
pub(crate) mod session;
pub(crate) mod shortcut_combo;
pub(crate) mod shortcuts;
pub(crate) mod x11;

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use opit_core::config::{AppConfig, PasteAfter, PasteMethod};

use crate::app_core::Platform;
use crate::platform::factory::PlatformServices;
use crate::platform::{
    Autostart, CLIPBOARD_RESTORE_DELAY, DebugAutostart, ForegroundApp, ForegroundInfo, NoAutostart, OverlaySink,
    PasteOutcome, PasteRequest, Paster, SecretError, SecretStore, SelectionError, Sound, Sounds,
};

pub(crate) fn create(config: &AppConfig, overlay_sink: OverlaySink) -> PlatformServices {
    let x11 = session::current() == session::SessionKind::X11;
    let autostart: Arc<dyn Autostart> = if cfg!(debug_assertions) {
        Arc::new(DebugAutostart)
    } else {
        match autostart::DesktopAutostart::for_current_exe() {
            Ok(service) => Arc::new(service),
            Err(error) => Arc::new(NoAutostart(error)),
        }
    };
    PlatformServices {
        platform: Platform {
            foreground: if x11 { Arc::new(x11::X11Foreground) } else { Arc::new(LinuxForeground) },
            media: Arc::new(media::LinuxMedia::new()),
            secrets: Arc::new(LinuxSecrets),
            hotkey: if shortcuts::backend() == shortcuts::Backend::X11 {
                Arc::new(x11::X11Hotkey::default())
            } else {
                Arc::new(shortcuts::CommandHotkey::default())
            },
            mic: Arc::new(crate::platform::microphone::CpalMicrophone::new()),
            overlay: Arc::new(overlay::LinuxOverlay::new(config.ui.overlay_position, overlay_sink)),
            autostart,
        },
        paster: Arc::new(LinuxPaster),
        sounds: Arc::new(LinuxSounds),
    }
}

/// Runs a short-lived helper without blocking the caller, and reaps it so no zombie remains.
fn run_detached(mut command: Command) {
    let Ok(mut child) = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let _ = std::thread::Builder::new().name("opit-linux-helper".into()).spawn(move || {
        let _ = child.wait();
    });
}

struct LinuxForeground;

impl ForegroundApp for LinuxForeground {
    fn current(&self, _include_title: bool) -> Option<ForegroundInfo> {
        None
    }

    fn list_running(&self) -> Result<Vec<ForegroundInfo>, String> {
        Err("Per-app window discovery is unavailable on this Linux session".into())
    }
}

struct LinuxSecrets;

impl SecretStore for LinuxSecrets {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        let output = Command::new("secret-tool")
            .args(["lookup", "service", "opit-speech-to-text", "key_ref", key_ref])
            .output()
            .map_err(|_| SecretError("Secret Service is unavailable".into()))?;
        if !output.status.success() {
            return Ok(None);
        }
        String::from_utf8(output.stdout)
            .map(|value| Some(value.trim_end_matches('\n').to_owned()))
            .map_err(|_| SecretError("Secret Service returned invalid text".into()))
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError> {
        let mut child = Command::new("secret-tool")
            .args(["store", "--label=Opit Speech to Text", "service", "opit-speech-to-text", "key_ref", key_ref])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SecretError("Secret Service is unavailable".into()))?;
        child
            .stdin
            .take()
            .ok_or_else(|| SecretError("Secret Service is unavailable".into()))?
            .write_all(secret.as_bytes())
            .map_err(|_| SecretError("Secret Service write failed".into()))?;
        if child.wait().map_err(|_| SecretError("Secret Service write failed".into()))?.success() {
            Ok(())
        } else {
            Err(SecretError("Secret Service refused the credential".into()))
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        let status = Command::new("secret-tool")
            .args(["clear", "service", "opit-speech-to-text", "key_ref", key_ref])
            .status()
            .map_err(|_| SecretError("Secret Service is unavailable".into()))?;
        if status.success() { Ok(()) } else { Err(SecretError("Secret Service deletion failed".into())) }
    }
}

struct LinuxPaster;

impl LinuxPaster {
    fn wayland() -> bool {
        session::current() == session::SessionKind::Wayland
    }

    fn copy(text: &str) -> Result<(), String> {
        let mut child = if Self::wayland() {
            Command::new("wl-copy")
                .args(["--type", "text/plain;charset=utf-8"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        } else {
            Command::new("xclip")
                .args(["-selection", "clipboard", "-in"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        }
        .map_err(|_| "Clipboard command is unavailable".to_owned())?;
        child
            .stdin
            .take()
            .ok_or("Clipboard input is unavailable")?
            .write_all(text.as_bytes())
            .map_err(|_| "Clipboard write failed".to_owned())?;
        if child.wait().map_err(|_| "Clipboard write failed")?.success() {
            Ok(())
        } else {
            Err("Clipboard write failed".into())
        }
    }

    fn read_clipboard() -> Option<String> {
        let output = if Self::wayland() {
            Command::new("wl-paste").args(["--no-newline", "--type", "text/plain;charset=utf-8"]).output()
        } else {
            Command::new("xclip").args(["-selection", "clipboard", "-out"]).output()
        }
        .ok()?;
        if output.status.success() { String::from_utf8(output.stdout).ok() } else { None }
    }

    fn restore_if_unchanged(old: Option<String>, current: &str) {
        let Some(old) = old else { return };
        std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
        // Data-control may report the previous selection just after a write.
        // Never restore unless the expected new text becomes observable.
        for _ in 0..10 {
            match Self::read_clipboard() {
                Some(value) if value == current => {
                    let _ = Self::copy(&old);
                    return;
                }
                Some(value) if value != old => return,
                _ => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
    }

    fn paste_keys(method: PasteMethod) -> Option<Vec<&'static str>> {
        Some(match method {
            PasteMethod::CtrlV => vec!["29:1", "47:1", "47:0", "29:0"],
            PasteMethod::ShiftInsert => vec!["42:1", "110:1", "110:0", "42:0"],
            PasteMethod::CtrlShiftV => vec!["29:1", "42:1", "47:1", "47:0", "42:0", "29:0"],
            PasteMethod::Type => return None,
        })
    }

    fn send_keys(keys: &[&str]) -> bool {
        let Some(codes) = keys
            .iter()
            .map(|key| {
                let (code, state) = key.split_once(':')?;
                Some((code.parse::<u32>().ok()?, state == "1"))
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if !Self::wayland() {
            return session::current() == session::SessionKind::X11 && x11::send_keys(&codes);
        }
        input::with_fallback(ei::send(&codes), || Self::ydotool(keys))
    }

    fn ydotool(keys: &[&str]) -> bool {
        Command::new("ydotool")
            .arg("key")
            .args(keys)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

impl Paster for LinuxPaster {
    /// No safe selection capture exists yet, so Edit reports that rather than treating the
    /// selection as empty and generating new text over it.
    fn read_selection(&self) -> Result<Option<String>, SelectionError> {
        Err(SelectionError::Unavailable)
    }

    fn can_auto_paste(&self) -> bool {
        true
    }

    fn paste(&self, text: &str, request: PasteRequest) -> PasteOutcome {
        let old = if request.restore && request.auto_paste { Self::read_clipboard() } else { None };
        if let Err(error) = Self::copy(text) {
            return PasteOutcome::Failed(error);
        }
        if request.auto_paste
            && let Some(keys) = Self::paste_keys(request.method)
            && Self::send_keys(&keys)
        {
            match request.after {
                PasteAfter::None => {}
                PasteAfter::Enter => {
                    let _ = Self::send_keys(&["28:1", "28:0"]);
                }
                PasteAfter::CtrlEnter => {
                    let _ = Self::send_keys(&["29:1", "28:1", "28:0", "29:0"]);
                }
            }
            if request.restore {
                Self::restore_if_unchanged(old, text);
            }
            return PasteOutcome::Pasted;
        }
        // A failed or unavailable input injector never restores the old clipboard.
        // The result remains available for manual Ctrl+V.
        let lang = crate::shell::i18n::resolve(None, crate::shell::i18n::system_locale().as_deref());
        let mut notification = Command::new("notify-send");
        notification.args(["Opit Speech to Text", lang.clipboard_only()]);
        run_detached(notification);
        PasteOutcome::ClipboardOnly
    }
}

struct LinuxSounds;

impl Sounds for LinuxSounds {
    fn play(&self, sound: Sound) {
        let id = match sound {
            Sound::Start => "message",
            Sound::Done => "complete",
            Sound::Cancel => "dialog-warning",
            Sound::Error => "dialog-error",
        };
        let mut player = Command::new("canberra-gtk-play");
        player.args(["-i", id]);
        run_detached(player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_edit_fails_closed_without_selection_capture() {
        assert_eq!(LinuxPaster.read_selection(), Err(SelectionError::Unavailable));
    }

    #[test]
    fn paste_shortcut_uses_evdev_codes_and_releases_modifiers() {
        assert_eq!(LinuxPaster::paste_keys(PasteMethod::CtrlV), Some(vec!["29:1", "47:1", "47:0", "29:0"]));
        assert_eq!(LinuxPaster::paste_keys(PasteMethod::Type), None);
    }
}
