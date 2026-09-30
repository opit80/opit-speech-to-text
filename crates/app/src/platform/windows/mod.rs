//! Win32 implementations of the platform traits.

pub mod autostart;
pub mod secrets;
pub mod sounds;

pub use autostart::RegistryAutostart;
pub use secrets::KeyringStore;
pub use sounds::WinSounds;
