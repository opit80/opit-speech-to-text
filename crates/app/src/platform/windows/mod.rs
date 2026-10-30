//! Win32 implementations of the platform traits.

pub mod autostart;
pub mod clipboard;
pub(crate) mod factory;
pub mod foreground;
pub(crate) mod hardware;
pub mod hotkey;
pub mod media;
pub mod overlay;
pub mod paster;
pub(crate) mod process;
pub mod secrets;
pub mod sounds;
pub mod uninstall;

pub use super::microphone;
pub use super::microphone::CpalMicrophone;
pub use autostart::RegistryAutostart;
pub use hotkey::WinHotkey;
pub use overlay::WinOverlay;
pub use paster::WinPaster;
pub use secrets::KeyringStore;
pub use sounds::WinSounds;

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs a user callback from inside an `extern "system"` callback. A panic unwinding out of
/// such a function aborts the process, so it is caught and dropped here.
pub(crate) fn call_guarded(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}
