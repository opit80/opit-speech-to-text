//! Core dictation logic for Opit Speech to Text.
//!
//! This crate is deliberately free of Tauri and OS-specific code so it can be
//! tested on any platform. The app crate supplies recordings and consumes
//! transcripts through [`pipeline`].

pub mod app_rules;
pub mod audio;
pub mod config;
pub mod files;
pub mod history;
pub mod hotkey;
pub mod pipeline;
pub mod provider;
pub mod rules;
pub mod text;
pub mod voice_modes;

pub mod ask;
pub mod cleanup;
