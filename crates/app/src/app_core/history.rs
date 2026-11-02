use std::sync::PoisonError;

use opit_core::history::{Dictation, UsageStats};
use opit_core::learning::Dismissals;

use super::{AppCore, CommandError, HistoryUpdate, MAX_HISTORY_TEXT_BYTES};
use crate::history_service::HistoryService;

impl AppCore {
    // ----- history -----

    fn history(&self) -> Result<&HistoryService, CommandError> {
        self.history.as_deref().ok_or_else(|| CommandError::new("unavailable", "history is not available"))
    }

    pub fn history_update_text(&self, id: i64, text: &str) -> Result<HistoryUpdate, CommandError> {
        if id <= 0 || text.trim().is_empty() || text.len() > MAX_HISTORY_TEXT_BYTES || text.contains('\0') {
            return Err(CommandError::new(
                "invalid_input",
                "provide a valid history id and nonempty text of at most 100000 UTF-8 bytes without NUL",
            ));
        }
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let settings = self.settings.current();
        let history = self.history()?;
        let store = history.store();
        let before = store.get(id)?.ok_or_else(|| CommandError::new("unavailable", "history row is missing"))?;
        if before.status != opit_core::pipeline::TranscriptStatus::Ok {
            return Err(CommandError::new("invalid_input", "only successful transcripts can be edited"));
        }
        let suggestions = if settings.config.learning.enabled && before.mode == "dictation" {
            // Load preferences before committing the edit, so a read failure is safely retryable.
            let dismissed = Dismissals::load(&self.paths.root.join("learning.json")).map_err(Self::learning_error)?;
            opit_core::learning::filter_suggestions(
                opit_core::learning::diff_terms(&before.text, text),
                true,
                &settings.rules,
                &dismissed,
            )
        } else {
            Vec::new()
        };
        let dictation =
            store.update_text(id, text)?.ok_or_else(|| CommandError::new("unavailable", "history row is missing"))?;
        Ok(HistoryUpdate { dictation, suggestions })
    }

    pub fn history_recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().recent(limit.min(500), before_id)?)
    }

    pub fn usage_stats(&self, since_ms: Option<i64>, until_ms: i64) -> Result<UsageStats, CommandError> {
        Ok(self.history()?.store().usage(since_ms, until_ms)?)
    }

    pub fn history_search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().search(query, limit.min(500))?)
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CommandError> {
        Ok(self.history()?.delete(id)?)
    }

    pub fn history_clear(&self) -> Result<(), CommandError> {
        Ok(self.history()?.clear()?)
    }

    /// The WAV bytes of a dictation's saved audio. Only files inside the app's audio folder
    /// are read, whatever path the database holds.
    pub fn history_audio(&self, id: i64) -> Result<Vec<u8>, CommandError> {
        let unavailable = || CommandError::new("unavailable", "this dictation has no saved audio");
        let row = self.history()?.store().get(id)?.ok_or_else(unavailable)?;
        let path = std::path::PathBuf::from(row.audio_path.ok_or_else(unavailable)?);
        let dir = self.paths.audio.canonicalize().map_err(|_| unavailable())?;
        let file = path.canonicalize().map_err(|_| unavailable())?;
        if !file.starts_with(&dir) {
            return Err(CommandError::new("invalid_input", "the audio file is outside the audio folder"));
        }
        std::fs::read(&file).map_err(|e| CommandError::new("history", e.to_string()))
    }

    /// Loads only a retained failure. Called on the blocking pool, never on the UI thread.
    pub fn history_retry_audio(&self, id: i64) -> Result<opit_core::pipeline::PreparedAudio, CommandError> {
        if !self.settings.current().config.history.enabled {
            return Err(CommandError::new("unavailable", "history is disabled"));
        }
        let row = self
            .history()?
            .store()
            .get(id)?
            .ok_or_else(|| CommandError::new("unavailable", "history row is missing"))?;
        if matches!(row.mode.as_str(), "edit" | "translate") {
            return Err(CommandError::new("invalid_input", "voice edits and translations must be recorded again"));
        }
        if row.status != opit_core::pipeline::TranscriptStatus::Failed {
            return Err(CommandError::new("invalid_input", "only failed dictations can be retried"));
        }
        let bytes = self.history_audio(id)?;
        let invalid = || CommandError::new("invalid_input", "saved audio is not a prepared WAV");
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|_| invalid())?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.sample_rate != 16_000
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err(invalid());
        }
        let samples = reader
            .samples::<i16>()
            .map(|s| s.map(|v| f32::from(v) / 32768.0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?;
        if samples.is_empty() {
            return Err(invalid());
        }
        let audio_ms = samples.len() as u64 * 1000 / 16_000;
        Ok(opit_core::pipeline::PreparedAudio { samples, audio_ms, speech_ms: audio_ms })
    }
}
