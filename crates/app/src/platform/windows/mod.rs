//! Win32 implementations of the platform traits.

pub mod autostart;
pub mod hotkey;
pub mod microphone;
pub mod secrets;
pub mod sounds;

pub use autostart::RegistryAutostart;
pub use hotkey::WinHotkey;
pub use microphone::CpalMicrophone;
pub use secrets::KeyringStore;
pub use sounds::WinSounds;

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs a user callback from inside an `extern "system"` callback. A panic unwinding out of
/// such a function aborts the process, so it is caught and dropped here.
pub(crate) fn call_guarded(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}
