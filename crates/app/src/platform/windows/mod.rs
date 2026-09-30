//! Win32 implementations of the platform traits.

pub mod autostart;
pub mod microphone;
pub mod secrets;
pub mod sounds;

pub use autostart::RegistryAutostart;
pub use microphone::CpalMicrophone;
pub use secrets::KeyringStore;
pub use sounds::WinSounds;
