use super::{LocalError, Result, catalogue::Artifact};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Build,
    Model,
}

pub fn allowed(url: &reqwest::Url, source: Source) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    match source {
        Source::Build => {
            matches!(host, "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com")
        }
        Source::Model => {
            host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || (host.starts_with("cdn-lfs") && host.ends_with(".hf.co") && !host[..host.len() - 6].contains('.'))
        }
    }
}

pub fn verify(path: &Path, artifact: &Artifact) -> Result<()> {
    verify_cancellable(path, artifact, None)
}

fn verify_cancellable(path: &Path, artifact: &Artifact, cancel: Option<&watch::Receiver<bool>>) -> Result<()> {
    let mut file = std::fs::File::open(path).map_err(|_| LocalError::Io)?;
    if file.metadata().map_err(|_| LocalError::Io)?.len() != artifact.size {
        return Err(LocalError::Hash);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        if cancel.is_some_and(|rx| *rx.borrow()) {
            return Err(LocalError::Cancelled);
        }
        let count = file.read(&mut buffer).map_err(|_| LocalError::Io)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != artifact.sha256 {
        return Err(LocalError::Hash);
    }
    Ok(())
}

struct Part(PathBuf);
impl Drop for Part {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn fetch(
    artifact: Artifact,
    source: Source,
    destination: PathBuf,
    mut cancel: watch::Receiver<bool>,
    progress: impl Fn(u64) + Send,
) -> Result<()> {
    let url = reqwest::Url::parse(artifact.url).map_err(|_| LocalError::Url)?;
    if !allowed(&url, source) {
        return Err(LocalError::Url);
    }
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(7200))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 || !allowed(attempt.url(), source) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| LocalError::Download)?;
    let part = Part(destination.with_extension("part"));
    tokio::fs::create_dir_all(destination.parent().ok_or(LocalError::Io)?).await.map_err(|_| LocalError::Io)?;
    if *cancel.borrow() {
        return Err(LocalError::Cancelled);
    }
    let mut response = tokio::select! {
        biased;
        _ = cancel.changed() => return Err(LocalError::Cancelled),
        result = client.get(url).send() => result.map_err(|_| LocalError::Download)?,
    };
    if !response.status().is_success() || response.content_length().is_some_and(|n| n != artifact.size) {
        return Err(LocalError::Download);
    }
    let mut file = tokio::fs::File::create(&part.0).await.map_err(|_| LocalError::Io)?;
    let mut received = 0;
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.changed() => return Err(LocalError::Cancelled),
            result = response.chunk() => result.map_err(|_| LocalError::Download)?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        received += chunk.len() as u64;
        if received > artifact.size {
            return Err(LocalError::Hash);
        }
        file.write_all(&chunk).await.map_err(|_| LocalError::Io)?;
        // Finish the underlying write before cancellation can drop the file on Windows.
        file.flush().await.map_err(|_| LocalError::Io)?;
        progress(received);
    }
    file.sync_all().await.map_err(|_| LocalError::Io)?;
    drop(file);
    // Await the hash worker even when cancelled, so its file handle closes before cleanup.
    publish(part, destination, artifact, cancel).await
}

async fn publish(part: Part, destination: PathBuf, artifact: Artifact, cancel: watch::Receiver<bool>) -> Result<()> {
    let path = part.0.clone();
    let hash_cancel = cancel.clone();
    tokio::task::spawn_blocking(move || verify_cancellable(&path, &artifact, Some(&hash_cancel)))
        .await
        .map_err(|_| LocalError::Io)??;
    if *cancel.borrow() {
        return Err(LocalError::Cancelled);
    }
    tokio::fs::rename(&part.0, destination).await.map_err(|_| LocalError::Io)?;
    Ok(())
}

fn safe_name(name: &str) -> bool {
    let name = name.trim_end_matches('/');
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(['.', ' '])
                && !matches!(
                    p.split('.').next().unwrap_or("").to_ascii_uppercase().as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}

/// The destination must be new and private; no existing tree or links are followed.
pub fn extract(zip: &Path, destination: &Path, cancel: &watch::Receiver<bool>) -> Result<PathBuf> {
    std::fs::create_dir(destination).map_err(|_| LocalError::Io)?;
    let result = (|| {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(zip).map_err(|_| LocalError::Io)?)
            .map_err(|_| LocalError::Archive)?;
        if archive.len() > 10000 {
            return Err(LocalError::Archive);
        }
        let mut total = 0u64;
        for index in 0..archive.len() {
            if *cancel.borrow() {
                return Err(LocalError::Cancelled);
            }
            let mut entry = archive.by_index(index).map_err(|_| LocalError::Archive)?;
            let name = entry.name().map_err(|_| LocalError::Archive)?.into_owned();
            if !safe_name(&name)
                || entry.enclosed_name().is_none()
                || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err(LocalError::Archive);
            }
            total = total.checked_add(entry.size()).ok_or(LocalError::Archive)?;
            if total > 3_000_000_000 {
                return Err(LocalError::Archive);
            }
            let path = destination.join(name);
            if entry.is_dir() {
                std::fs::create_dir_all(path).map_err(|_| LocalError::Io)?;
            } else {
                std::fs::create_dir_all(path.parent().ok_or(LocalError::Archive)?).map_err(|_| LocalError::Io)?;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|_| LocalError::Archive)?;
                let mut buffer = [0; 65536];
                loop {
                    if *cancel.borrow() {
                        return Err(LocalError::Cancelled);
                    }
                    let n = entry.read(&mut buffer).map_err(|_| LocalError::Archive)?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buffer[..n]).map_err(|_| LocalError::Io)?;
                }
            }
        }
        find_server(destination)?.ok_or(LocalError::MissingServer)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(destination);
    }
    result
}

pub fn find_server(root: &Path) -> Result<Option<PathBuf>> {
    if !root.is_dir() {
        return Ok(None);
    }
    let mut pending = vec![root.to_owned()];
    let mut found = None;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).map_err(|_| LocalError::Io)? {
            let entry = entry.map_err(|_| LocalError::Io)?;
            let kind = entry.file_type().map_err(|_| LocalError::Io)?;
            if kind.is_symlink() {
                return Err(LocalError::Archive);
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if entry.file_name().to_string_lossy().eq_ignore_ascii_case("whisper-server.exe") {
                if found.is_some() {
                    return Err(LocalError::Archive);
                }
                found = Some(entry.path());
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archives_reject_links_missing_server_and_cancel_without_leaving_an_install() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.add_symlink("link", "../outside", zip::write::SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
        let (tx, rx) = watch::channel(false);
        let destination = dir.path().join("out");
        assert!(matches!(extract(&path, &destination, &rx), Err(LocalError::Archive)));
        assert!(!destination.exists());
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("runtime.dll", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"test").unwrap();
        zip.finish().unwrap();
        assert!(matches!(extract(&path, &destination, &rx), Err(LocalError::MissingServer)));
        assert!(!destination.exists());
        tx.send(true).unwrap();
        assert!(matches!(extract(&path, &destination, &rx), Err(LocalError::Cancelled)));
        assert!(!destination.exists());
    }
    #[tokio::test]
    async fn publication_is_atomic_and_hash_failure_or_cancel_removes_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.bin");
        let part = dest.with_extension("part");
        let artifact = Artifact {
            id: "test",
            file: "test",
            url: "",
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let (tx, rx) = watch::channel(false);
        std::fs::write(&part, b"abc").unwrap();
        publish(Part(part.clone()), dest.clone(), artifact, rx.clone()).await.unwrap();
        assert!(!part.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(&part, b"bad").unwrap();
        assert!(matches!(publish(Part(part.clone()), dest.clone(), artifact, rx.clone()).await, Err(LocalError::Hash)));
        assert!(!part.exists());
        assert!(!dest.exists());
        std::fs::write(&part, b"abc").unwrap();
        tx.send(true).unwrap();
        assert!(matches!(publish(Part(part.clone()), dest.clone(), artifact, rx).await, Err(LocalError::Cancelled)));
        assert!(!part.exists());
        assert!(!dest.exists());
    }
    #[test]
    fn hashes_and_size_must_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test");
        std::fs::write(&path, b"abc").unwrap();
        let mut artifact = Artifact {
            id: "test",
            file: "test",
            url: "",
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        assert!(verify(&path, &artifact).is_ok());
        artifact.sha256 = super::super::catalogue::MODELS[0].sha256;
        assert!(matches!(verify(&path, &artifact), Err(LocalError::Hash)));
        artifact.size = 4;
        assert!(matches!(verify(&path, &artifact), Err(LocalError::Hash)));
    }
    #[test]
    fn host_allowlist_is_https_and_source_specific() {
        for (url, source, expected) in [
            ("https://github.com/a", Source::Build, true),
            ("http://github.com/a", Source::Build, false),
            ("https://release-assets.githubusercontent.com/a", Source::Build, true),
            ("https://github.com.evil.test/a", Source::Build, false),
            ("https://evil.github.com/a", Source::Build, false),
            ("https://cdn-lfs-us-1.hf.co/a", Source::Model, true),
            ("https://cdn-lfs.hf.co/a", Source::Model, true),
            ("https://cdn-lfs.evil.hf.co/a", Source::Model, false),
            ("https://cdn-lfs.hf.co.evil/a", Source::Model, false),
            ("https://cdn.huggingface.co/a", Source::Model, true),
            ("https://huggingface.co/a", Source::Build, false),
            ("https://u@huggingface.co/a", Source::Model, false),
            ("https://huggingface.co:444/a", Source::Model, false),
        ] {
            assert_eq!(allowed(&reqwest::Url::parse(url).unwrap(), source), expected, "{url}");
        }
    }
    #[test]
    fn archives_reject_traversal_and_find_nested_server_with_dll() {
        for name in ["../outside", "/outside", "C:/outside", "a/../../x", "a\\..\\x", "a/CON", "a./x"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"test").unwrap();
            zip.finish().unwrap();
            let (_tx, rx) = watch::channel(false);
            assert!(extract(&path, &dir.path().join("out"), &rx).is_err(), "{name}");
            assert!(!dir.path().join("out").exists());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in ["nested/bin/whisper-server.exe", "nested/bin/runtime.dll"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"test").unwrap();
        }
        zip.finish().unwrap();
        let (_tx, rx) = watch::channel(false);
        let exe = extract(&path, &dir.path().join("out"), &rx).unwrap();
        assert!(exe.with_file_name("runtime.dll").exists());
    }
}
