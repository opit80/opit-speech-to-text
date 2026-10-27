//! History storage for the app: SQLite rows plus optional 16 kHz WAV files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use opit_core::audio::TARGET_RATE;
use opit_core::audio::encode::{encode_wav, to_pcm16};
use opit_core::history::{AudioStore, HistoryError, HistoryStore, NewDictation};
use opit_core::pipeline::PreparedAudio;
use tracing::warn;

use crate::controller::HistorySink;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub struct HistoryService {
    store: Mutex<HistoryStore>,
    audio: AudioStore,
    audio_dir: PathBuf,
    retrying: Mutex<HashSet<i64>>,
}

pub struct RetryClaim {
    history: Arc<HistoryService>,
    id: i64,
}

impl Drop for RetryClaim {
    fn drop(&mut self) {
        self.history.retrying.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
    }
}

impl HistoryService {
    pub fn open(db: &Path, audio_dir: PathBuf) -> Result<Self, HistoryError> {
        Ok(Self::with_store(HistoryStore::open(db)?, audio_dir))
    }

    pub fn with_store(store: HistoryStore, audio_dir: PathBuf) -> Self {
        Self {
            store: Mutex::new(store),
            audio: AudioStore::new(audio_dir.clone()),
            audio_dir,
            retrying: Mutex::default(),
        }
    }

    /// Direct access for the read-only history commands.
    pub fn store(&self) -> MutexGuard<'_, HistoryStore> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn claim_retry(self: &Arc<Self>, id: i64) -> Option<RetryClaim> {
        self.retrying
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id)
            .then(|| RetryClaim { history: self.clone(), id })
    }

    /// Deletes a row and its audio file.
    pub fn delete(&self, id: i64) -> Result<(), HistoryError> {
        let path = self.store().delete(id)?;
        self.remove_files(path.iter());
        Ok(())
    }

    pub fn clear(&self) -> Result<(), HistoryError> {
        let paths = self.store().clear()?;
        self.remove_files(paths.iter());
        Ok(())
    }

    /// Removes audio files older than `retention_days`; returns how many were removed.
    pub fn purge_expired_audio(&self, now_ms: i64, retention_days: u32) -> Result<usize, HistoryError> {
        let paths = self.store().take_expired_audio(now_ms - i64::from(retention_days) * DAY_MS)?;
        self.remove_files(paths.iter());
        Ok(paths.len())
    }
    fn remove_files<'a>(&self, paths: impl Iterator<Item = &'a String>) {
        let Ok(dir) = self.audio_dir.canonicalize() else { return };
        for path in paths {
            let Ok(file) = Path::new(path).canonicalize() else { continue };
            if !file.starts_with(&dir) {
                continue;
            }
            if AudioStore::remove(&file).is_err() {
                warn!("could not delete a history audio file");
            }
        }
    }
}

impl HistorySink for HistoryService {
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String> {
        let store = self.store();
        let id = store.insert(entry).map_err(|e| e.to_string())?;
        if let Some(audio) = audio {
            let wav = encode_wav(&to_pcm16(&audio.samples), TARGET_RATE).map_err(|e| e.to_string())?;
            let path = self.audio.save(id, &wav).map_err(|e| e.to_string())?;
            store.set_audio_path(id, &path.to_string_lossy()).map_err(|e| e.to_string())?;
        }
        Ok(id)
    }

    fn replace(&self, id: i64, entry: &NewDictation<'_>, keep_audio: bool) -> Result<(), String> {
        let store = self.store();
        let path = store.get(id).map_err(|_| "history unavailable")?.and_then(|row| row.audio_path);
        if !store.replace(id, entry, keep_audio).map_err(|_| "history unavailable")? {
            return Err("history row was deleted".into());
        }
        if !keep_audio {
            self.remove_files(path.iter());
        }
        Ok(())
    }
}

/// Stand-in when `history.db` cannot be opened: dictation works, nothing is saved.
pub struct DisabledHistory;

impl HistorySink for DisabledHistory {
    fn record(&self, _entry: &NewDictation<'_>, _audio: Option<&PreparedAudio>) -> Result<i64, String> {
        Err("history is unavailable".into())
    }

    fn replace(&self, _id: i64, _entry: &NewDictation<'_>, _keep_audio: bool) -> Result<(), String> {
        Err("history is unavailable".into())
    }
}

#[cfg(test)]
mod tests {
    use opit_core::pipeline::TranscriptStatus;

    use super::*;

    fn entry(created_at_ms: i64) -> NewDictation<'static> {
        NewDictation {
            created_at_ms,
            profile_id: "groq",
            raw_text: "merhaba",
            text: "Merhaba",
            status: TranscriptStatus::Ok,
            audio_ms: 1_000,
            latency_ms: 900,
            error_code: None,
            language: None,
        }
    }

    fn audio() -> PreparedAudio {
        PreparedAudio { samples: vec![0.1; 16_000], audio_ms: 1_000, speech_ms: 1_000 }
    }

    fn service(dir: &Path) -> HistoryService {
        HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), dir.join("audio"))
    }

    #[test]
    fn records_rows_with_and_without_audio() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let plain = history.record(&entry(1), None).unwrap();
        assert_eq!(history.store().get(plain).unwrap().unwrap().audio_path, None);

        let with_audio = history.record(&entry(2), Some(&audio())).unwrap();
        let path = history.store().get(with_audio).unwrap().unwrap().audio_path.unwrap();
        let wav = hound::WavReader::open(&path).unwrap();
        assert_eq!((wav.spec().sample_rate, wav.spec().channels, wav.len()), (16_000, 1, 16_000));
    }

    #[test]
    fn delete_and_clear_remove_audio_files() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let a = history.record(&entry(1), Some(&audio())).unwrap();
        history.record(&entry(2), Some(&audio())).unwrap();
        let path_a = history.store().get(a).unwrap().unwrap().audio_path.unwrap();
        history.delete(a).unwrap();
        assert!(!Path::new(&path_a).exists());
        history.clear().unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("audio")).unwrap().count(), 0);
        assert!(history.store().recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn expired_audio_is_purged_but_rows_stay() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        let now = 100 * DAY_MS;
        let old = history.record(&entry(now - 31 * DAY_MS), Some(&audio())).unwrap();
        let fresh = history.record(&entry(now - DAY_MS), Some(&audio())).unwrap();
        assert_eq!(history.purge_expired_audio(now, 30).unwrap(), 1);
        assert_eq!(history.store().get(old).unwrap().unwrap().audio_path, None);
        assert!(history.store().get(fresh).unwrap().unwrap().audio_path.is_some());
    }

    #[test]
    fn failure_audio_survives_retention_and_retry_controls_its_removal() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        for keep in [false, true] {
            let mut failed = entry(123);
            failed.status = TranscriptStatus::Failed;
            failed.error_code = Some("network");
            let id = history.record(&failed, Some(&audio())).unwrap();
            let path = history.store().get(id).unwrap().unwrap().audio_path.unwrap();
            assert_eq!(history.purge_expired_audio(100 * DAY_MS, 1).unwrap(), 0);
            assert!(Path::new(&path).exists());
            // A further failed attempt keeps the same row and WAV.
            failed.error_code = Some("timeout");
            history.replace(id, &failed, true).unwrap();
            assert_eq!(history.store().get(id).unwrap().unwrap().error_code.as_deref(), Some("timeout"));
            history.replace(id, &entry(999), keep).unwrap();
            let row = history.store().get(id).unwrap().unwrap();
            assert_eq!(row.created_at_ms, 123);
            assert_eq!(row.audio_path.is_some(), keep);
            assert_eq!(Path::new(&path).exists(), keep);
            history.delete(id).unwrap();
        }
    }

    #[test]
    fn delete_never_removes_a_path_outside_the_audio_directory() {
        let dir = tempfile::tempdir().unwrap();
        let history = service(dir.path());
        std::fs::create_dir_all(dir.path().join("audio")).unwrap();
        let outside = dir.path().join("keep.txt");
        std::fs::write(&outside, "keep").unwrap();
        let id = history.record(&entry(1), None).unwrap();
        let traversal = dir.path().join("audio").join("..").join("keep.txt");
        history.store().set_audio_path(id, &traversal.to_string_lossy()).unwrap();
        history.delete(id).unwrap();
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "keep");
    }
}
