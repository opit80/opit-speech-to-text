//! Core dictation logic for Opit Speech to Text.
//!
//! This crate is deliberately free of Tauri and OS-specific code so it can be
//! tested on any platform. The app crate supplies recordings and consumes
//! transcripts through [`pipeline`].

pub mod audio;
pub mod config;
pub mod provider;
pub mod rules;
pub mod text;
