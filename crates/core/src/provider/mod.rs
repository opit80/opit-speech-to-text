//! Speech-to-text providers. v1 has one adapter: OpenAI-compatible HTTP.

pub mod profile;

pub use profile::{Profile, ResponseFormat, presets};
