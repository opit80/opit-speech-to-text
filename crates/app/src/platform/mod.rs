//! Platform seams. Windows implementations live in `platform::windows`; tests use `platform::fake`.

pub(crate) mod factory;
#[cfg(test)]
pub mod fake;
pub mod keys;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(any(windows, test))]
mod media;
#[cfg(any(windows, target_os = "linux"))]
pub mod microphone;
pub(crate) mod overlay;
pub(crate) mod process;
#[cfg(windows)]
pub mod windows;

use std::sync::Arc;
use std::time::{Duration, Instant};

use opit_core::audio::Recording;
use opit_core::config::{MediaConfig, MediaMode, OverlayPosition, PasteAfter, PasteMethod};

// ---------- Foreground application ----------
/// Deliberately not Debug: window titles and process names must never reach logs.
#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForegroundInfo {
    pub exe: String,
    pub title: String,
}

pub trait ForegroundApp: Send + Sync {
    /// Cheap native reads on the controller start path, never on the hotkey hook thread.
    /// Do not read the title unless requested. Unreadable processes yield None.
    fn current(&self, include_title: bool) -> Option<ForegroundInfo>;
    /// Visible, non-tool top-level windows; exclude this process, deduplicate and sort by exe.
    /// Blocking: only call from a blocking worker. Errors contain no application information.
    fn list_running(&self) -> Result<Vec<ForegroundInfo>, String>;
}

// ---------- Media ----------
pub trait MediaControl: Send + Sync {
    /// Non-blocking, ordered and idempotent. Native work belongs to a dedicated thread.
    fn engage(&self, mode: MediaMode, percent: u8);
    fn release(&self);
    /// Drains restoration and stops the worker. Blocking; only call from a blocking thread.
    fn shutdown(&self);
}

/// Lives exactly as long as a dictation capture; unwinding also queues restoration.
pub struct MediaGuard(Option<Arc<dyn MediaControl>>);

impl MediaGuard {
    pub fn engage(media: Arc<dyn MediaControl>, config: &MediaConfig) -> Self {
        if config.enabled {
            let guard = Self(Some(media));
            guard.0.as_ref().unwrap().engage(config.mode, config.duck_volume_percent);
            guard
        } else {
            Self(None)
        }
    }
}

impl Drop for MediaGuard {
    fn drop(&mut self) {
        if let Some(media) = &self.0 {
            media.release();
        }
    }
}

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
/// Mono samples at the native device rate. Callback must only enqueue, never block.
pub type AudioSink = Arc<dyn Fn(Vec<f32>, u32) + Send + Sync>;

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
    /// Atomically replay captured mono samples then subscribe to new unpaused samples.
    /// Removing the sink must synchronize with any in-flight callback.
    fn set_audio_sink(&self, _sink: Option<AudioSink>) {}
    /// The stream remains open; samples captured while paused are discarded.
    fn set_paused(&self, paused: bool);
    /// Stops the stream and returns everything captured. Dropping without calling this cancels.
    fn finish(self: Box<Self>) -> Recording;
}

// ---------- Hotkey ----------
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    Edit,
    EditUp,
    Translate,
    TranslateUp,
    StyleCycle,
    Style(String),
    Ask,
    Pause,
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

pub use opit_core::hotkey::HotkeyError;

pub trait Hotkey: Send + Sync {
    fn supports_release(&self) -> bool {
        true
    }
    /// Portal permission requests can outlive the config write without blocking it.
    fn registration_pending(&self) -> bool {
        false
    }

    /// A backend that loses its connection after registration can report it on the next query.
    fn runtime_error(&self) -> Option<String> {
        None
    }
    /// Replace auxiliary shortcuts atomically. Voice modes also emit their matching release event.
    fn register_shortcuts(&self, shortcuts: &[(HotkeyEvent, Vec<String>)]) -> Result<(), HotkeyError>;
    /// Installs (or re-targets) the global hook for `keys` (names like "RightCtrl").
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError>;
    /// Paused: no events are delivered.
    fn set_paused(&self, paused: bool);
    /// While on, Esc key-downs are swallowed (not passed to the focused app).
    fn set_capture_escape(&self, on: bool);
}

// ---------- Paster ----------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    Unavailable,
    TooLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteOutcome {
    /// All insertion input was delivered to the target.
    Pasted,
    /// The text is on the clipboard but no insertion input was sent: elevated target (UIPI), or the
    /// foreground is a shell window or one of our own windows.
    ClipboardOnly,
    /// The clipboard could not be written, or typing was only partially delivered.
    Failed(String),
}

/// Old clipboard is restored this long after the paste keystroke.
pub const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteRequest {
    pub method: PasteMethod,
    pub after: PasteAfter,
    pub restore: bool,
    pub auto_paste: bool,
}

pub trait Paster: Send + Sync {
    /// Capture and restore the clipboard before recording. No content-bearing errors.
    /// Blocking; serialized with paste operations by the platform implementation.
    fn read_selection(&self) -> Result<Option<String>, SelectionError> {
        Ok(None)
    }

    /// Snapshot at recording stop; later focus changes must not enable auto-paste.
    fn can_auto_paste(&self) -> bool {
        true
    }
    /// Inserts text using the requested method. Successful typing never touches the clipboard.
    /// Unsafe targets and disabled auto-paste copy to the clipboard instead, without restoring it.
    /// Sends `after` only after insertion succeeds. Clipboard methods wait for any requested
    /// restore before returning, preserving queue ordering.
    /// Blocking; call from a blocking thread.
    fn paste(&self, text: &str, request: PasteRequest) -> PasteOutcome;
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
    #[cfg(target_os = "linux")]
    Cancel,
    Retry,
    OpenSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayButton {
    pub action: OverlayAction,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordingTiming {
    pub session: u64,
    /// Active recording time at `updated_at`, excluding all pauses.
    pub elapsed: Duration,
    pub updated_at: Instant,
    pub paused: bool,
}

impl RecordingTiming {
    pub fn elapsed_at(&self, now: Instant) -> Duration {
        overlay::recording_elapsed(self, now)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayView {
    /// Private live preview; the native view fits its tail into two lines.
    pub live_text: Option<String>,
    pub tone: Tone,
    pub text: String,
    /// Some(level 0..1) supplies the latest microphone level for the waveform.
    pub level: Option<f32>,
    pub recording: Option<RecordingTiming>,
    /// Some → the overlay becomes clickable (not click-through) and clicking it fires the action.
    pub button: Option<OverlayButton>,
    /// Auto-hide after this long; None = stays until the next show/hide.
    pub hide_after: Option<Duration>,
}

pub type OverlaySink = Arc<dyn Fn(OverlayAction) + Send + Sync>;

pub trait Overlay: Send + Sync {
    fn available(&self) -> bool {
        true
    }
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
    fn available(&self) -> bool {
        false
    }
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

/// Used by debug builds so nothing can write the Run key with a path into `target\debug`.
pub struct DebugAutostart;

impl Autostart for DebugAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(false)
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), String> {
        tracing::info!("debug builds leave autostart alone");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_guard_releases_during_unwind_and_disabled_guard_does_nothing() {
        let media = Arc::new(fake::FakeMedia::default());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = MediaGuard::engage(media.clone(), &MediaConfig::default());
            panic!("synthetic recording worker panic");
        }));
        assert!(result.is_err());
        assert_eq!(*media.calls.lock().unwrap(), [Some((MediaMode::Duck, 50)), None]);
        let config = MediaConfig { enabled: false, ..Default::default() };
        drop(MediaGuard::engage(media.clone(), &config));
        assert_eq!(media.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn debug_autostart_accepts_requests_but_stays_disabled() {
        let autostart = DebugAutostart;
        assert_eq!(autostart.set_enabled(true), Ok(()));
        assert_eq!(autostart.is_enabled(), Ok(false));
    }
}
