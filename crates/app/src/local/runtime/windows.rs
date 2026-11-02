use crate::local::{LocalError, Result, catalogue::Artifact};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

pub const BUILDS: &[Artifact] = &[
    Artifact {
        id: "blas",
        file: "whisper-blas-bin-x64.zip",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-blas-bin-x64.zip",
        size: 21715607,
        sha256: "b1158bd10edf92c9799149a6fc5d3e58da3f495241d6fb3423088130e88ffbf3",
    },
    Artifact {
        id: "cpu",
        file: "whisper-bin-x64.zip",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip",
        size: 8928640,
        sha256: "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb",
    },
    Artifact {
        id: "cuda12",
        file: "whisper-bin-win-cuda-12.4.0-x64.zip",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-win-cuda-12.4.0-x64.zip",
        size: 684913404,
        sha256: "afef0b881c500958921c3f5523b50e59ee2ec9b6f5cbd25b324c51ed308a957a",
    },
    Artifact {
        id: "cuda11",
        file: "whisper-bin-win-cuda-11.8.0-x64.zip",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-win-cuda-11.8.0-x64.zip",
        size: 284761231,
        sha256: "e81c86c542be6e3d7b7245392d4e1c52ab6e5c61dbe38721754bc0ab59d4d027",
    },
];

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
