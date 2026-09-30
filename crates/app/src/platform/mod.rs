//! Platform seams. Windows implementations live in `platform::windows`; tests use `platform::fake`.

#[cfg(test)]
pub mod fake;
pub mod keys;

use std::sync::Arc;
use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::config::OverlayPosition;

// ---------- Microphone ----------
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MicError {
    #[error("no microphone was found")]
    NoDevice,
    #[error("the microphone could not be opened: {0}")]
    Open(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    /// RMS of the last ~50 ms, 0..1. Sent at most every 50 ms.
    Level(f32),
    /// The stream died (device unplugged, driver error). Sent once.
    Failed(String),
}

pub type CaptureSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

pub struct Started {
    pub capture: Box<dyn Capture>,
    /// Name of the device actually opened.
    pub device: String,
    /// The requested device was missing or failed, so the system default was opened instead.
    pub fell_back: bool,
}

pub trait Microphone: Send + Sync {
    /// Input device names, default device first.
    fn devices(&self) -> Vec<String>;
    /// Opens `device` (None = system default) and starts capturing into memory at the
    /// device's native rate/channels. Falls back to the default device when the named one
    /// is missing or fails to open.
    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError>;
}

pub trait Capture: Send {
    /// Stops the stream and returns everything captured. Dropping without calling this cancels.
    fn finish(self: Box<Self>) -> Recording;
}

// ---------- Hotkey ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// All combo keys are now held (fires once per press, not on auto-repeat).
    ComboDown,
    /// A combo key was released after ComboDown.
    ComboUp,
    /// A Ctrl key was pressed and released with no other key pressed in between,
    /// and it was not part of forming the combo.
    LoneCtrl,
    /// Esc went down.
    Escape,
}

pub type HotkeySink = Arc<dyn Fn(HotkeyEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("unknown key name: {0}")]
    UnknownKey(String),
    #[error("the shortcut has no keys")]
    Empty,
    #[error("the keyboard hook could not be installed: {0}")]
    Install(String),
}

pub trait Hotkey: Send + Sync {
    /// Installs (or re-targets) the global hook for `keys` (names like "RightCtrl").
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError>;
    /// Paused: no events are delivered.
    fn set_paused(&self, paused: bool);
    /// While on, Esc key-downs are swallowed (not passed to the focused app).
    fn set_capture_escape(&self, on: bool);
}

// ---------- Paster ----------
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteOutcome {
    /// Ctrl+V was sent.
    Pasted,
    /// The text is on the clipboard but Ctrl+V could not be sent (elevated target / UIPI).
    ClipboardOnly,
    /// The clipboard could not be written.
    Failed(String),
}

/// Old clipboard is restored this long after Ctrl+V.
pub const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(400);

pub trait Paster: Send + Sync {
    /// Backs up the clipboard, puts `text` on it, sends Ctrl+V, and (when `restore`)
    /// restores the old clipboard CLIPBOARD_RESTORE_DELAY later on a background thread.
    /// Blocking; call from a blocking thread.
    fn paste(&self, text: &str, restore: bool) -> PasteOutcome;
}

// ---------- Overlay ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Busy,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Retry,
    OpenSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayButton {
    pub action: OverlayAction,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayView {
    pub tone: Tone,
    pub text: String,
    /// Some(level 0..1) draws a level bar under the text.
    pub level: Option<f32>,
    /// Some → the overlay becomes clickable (not click-through) and clicking it fires the action.
    pub button: Option<OverlayButton>,
    /// Auto-hide after this long; None = stays until the next show/hide.
    pub hide_after: Option<Duration>,
}

pub type OverlaySink = Arc<dyn Fn(OverlayAction) + Send + Sync>;

pub trait Overlay: Send + Sync {
    fn show(&self, view: OverlayView);
    fn hide(&self);
    fn set_position(&self, position: OverlayPosition);
}

// ---------- Secrets ----------
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("credential store error: {0}")]
pub struct SecretError(pub String);

pub trait SecretStore: Send + Sync {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, key_ref: &str) -> Result<(), SecretError>;
}

// ---------- Sounds ----------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Start,
    Done,
    Cancel,
    Error,
}

pub trait Sounds: Send + Sync {
    /// Non-blocking.
    fn play(&self, sound: Sound);
}

// ---------- Autostart ----------
pub trait Autostart: Send + Sync {
    fn is_enabled(&self) -> Result<bool, String>;
    /// Idempotent; enabling rewrites the command so a moved exe keeps working.
    fn set_enabled(&self, enabled: bool) -> Result<(), String>;
}

// ---------- Stand-ins when a platform piece cannot start ----------

/// Used when the overlay window cannot be created; dictation keeps working with sounds and the tray.
pub struct NoOverlay;

impl Overlay for NoOverlay {
    fn show(&self, _view: OverlayView) {}
    fn hide(&self) {}
    fn set_position(&self, _position: OverlayPosition) {}
}

/// Used when the credential store cannot be opened; every call reports why.
pub struct UnavailableSecrets(pub String);

impl SecretStore for UnavailableSecrets {
    fn get(&self, _key_ref: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError(self.0.clone()))
    }

    fn set(&self, _key_ref: &str, _secret: &str) -> Result<(), SecretError> {
        Err(SecretError(self.0.clone()))
    }

    fn delete(&self, _key_ref: &str) -> Result<(), SecretError> {
        Err(SecretError(self.0.clone()))
    }
}

/// Used when the executable path is unknown, so no Run entry can be written.
pub struct NoAutostart(pub String);

impl Autostart for NoAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Err(self.0.clone())
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), String> {
        Err(self.0.clone())
    }
}
