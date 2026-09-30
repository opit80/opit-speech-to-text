//! Optional per-dictation WAV files (only when "save audio" is on).

use std::io;
use std::path::{Path, PathBuf};

pub struct AudioStore {
    dir: PathBuf,
}

impl AudioStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn save(&self, id: i64, wav: &[u8]) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{id}.wav"));
        std::fs::write(&path, wav)?;
        Ok(path)
    }

    /// Removes a stored file; a file that is already gone is not an error.
    pub fn remove(path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_into_a_created_dir_and_removes_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let store = AudioStore::new(dir.path().join("audio"));
        let path = store.save(42, b"RIFF").unwrap();
        assert_eq!(path.file_name().unwrap(), "42.wav");
        assert_eq!(std::fs::read(&path).unwrap(), b"RIFF");
        AudioStore::remove(&path).unwrap();
        assert!(!path.exists());
        AudioStore::remove(&path).unwrap();
    }
}
