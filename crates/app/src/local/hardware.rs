use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// PowerShell/CIM start-up can take many seconds on a busy, low-memory machine. The probe always
/// runs on a blocking worker, so a generous limit costs no UI responsiveness.
#[cfg(windows)]
const CIM_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(windows)]
const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_secs(10);
/// After a failed probe, callers get a fallback without probing again for this long, so a
/// persistently failing probe cannot delay every local server start.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Hardware {
    pub ram_bytes: u64,
    pub vram_mb: u64,
    pub nvidia: bool,
    pub physical_cores: u16,
}

#[cfg(windows)]
fn probe(mut command: std::process::Command, timeout: std::time::Duration) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Read};
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use std::time::Instant;

    command.creation_flags(0x00000004 | 0x08000000).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut tree = crate::ask::process::Tree::new().map_err(|_| Error::other("hardware probe unavailable"))?;
    let mut child = command.spawn()?;
    if tree.attach(&child).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::other("hardware probe supervision failed"));
    }
    let stdout = child.stdout.take().ok_or_else(|| Error::other("hardware probe output unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            return Err(Error::other("hardware probe output too large"));
        }
        Ok(bytes)
    });
    let start = Instant::now();
    let result = loop {
        if start.elapsed() >= timeout {
            break Err(Error::new(ErrorKind::TimedOut, "hardware probe timed out"));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => break Err(Error::other("hardware probe failed")),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(error) => break Err(error),
        }
    };
    tree.stop();
    let _ = child.kill();
    let _ = child.wait();
    let bytes = reader.join().map_err(|_| Error::other("hardware probe reader failed"))?;
    result?;
    bytes
}

/// Caches the first successful probe for the whole run. A failed or timed-out probe is never
/// cached: it would hide CUDA builds and recommend the smallest model until a restart.
#[derive(Default)]
pub struct HardwareCache(Mutex<CacheState>);

#[derive(Default)]
struct CacheState {
    found: Option<Hardware>,
    failed_at: Option<Instant>,
}

impl HardwareCache {
    /// Concurrent callers wait for one probe instead of starting their own.
    pub fn get(&self, detect: impl FnOnce() -> Option<Hardware>, now: Instant) -> Hardware {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = &state.found {
            return found.clone();
        }
        if state.failed_at.is_some_and(|at| now.saturating_duration_since(at) < RETRY_AFTER) {
            return fallback();
        }
        match detect() {
            Some(found) => {
                state.found = Some(found.clone());
                state.failed_at = None;
                found
            }
            None => {
                state.failed_at = Some(now);
                fallback()
            }
        }
    }
}

/// What callers use while the hardware is unknown.
fn fallback() -> Hardware {
    Hardware { physical_cores: auto_threads(), ..Default::default() }
}

/// `None` when the system probe failed or timed out; a missing GPU memory reading is not a failure.
pub fn detect() -> Option<Hardware> {
    #[cfg(windows)]
    {
        let system =
            std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        let mut command = std::process::Command::new(system.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
        command.args(["-NoProfile","-NonInteractive","-Command", "$ErrorActionPreference='Stop'; $c=Get-CimInstance Win32_ComputerSystem; $p=Get-CimInstance Win32_Processor; $g=Get-CimInstance Win32_VideoController; @{ram_bytes=[UInt64]$c.TotalPhysicalMemory;physical_cores=[int](($p|Measure-Object NumberOfCores -Sum).Sum);nvidia=[bool]($g|Where-Object Name -Match NVIDIA)}|ConvertTo-Json -Compress"]);
        let bytes = probe(command, CIM_TIMEOUT).ok()?;
        let mut hardware = serde_json::from_slice::<Hardware>(&bytes).ok()?;
        if hardware.nvidia {
            let mut command = std::process::Command::new(system.join("System32/nvidia-smi.exe"));
            command.args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"]);
            if let Ok(output) = probe(command, NVIDIA_SMI_TIMEOUT) {
                hardware.vram_mb = String::from_utf8_lossy(&output)
                    .lines()
                    .filter_map(|s| s.trim().parse::<u64>().ok())
                    .max()
                    .unwrap_or(0);
            }
        }
        if hardware.physical_cores == 0 {
            hardware.physical_cores = auto_threads();
        }
        Some(hardware)
    }
    #[cfg(not(windows))]
    {
        Some(fallback())
    }
}

fn auto_threads() -> u16 {
    std::thread::available_parallelism().map(|n| (n.get() / 2).clamp(1, u16::MAX as usize) as u16).unwrap_or(1)
}

pub fn recommendation(h: &Hardware) -> &'static str {
    let ram_gb = h.ram_bytes / (1024 * 1024 * 1024);
    if ram_gb >= 16 || (ram_gb >= 8 && h.vram_mb >= 4096) {
        "ggml-large-v3-turbo-q5_0.bin"
    } else if ram_gb >= 8 {
        "ggml-small-q5_1.bin"
    } else if ram_gb >= 4 {
        "ggml-base-q5_1.bin"
    } else {
        "ggml-tiny-q5_1.bin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn blocked_probe_child() {
        if std::env::var_os("OPIT_BLOCKED_PROBE").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    #[cfg(windows)]
    #[test]
    fn hardware_probe_times_out_and_reaps_a_blocked_child() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "local::hardware::tests::blocked_probe_child", "--nocapture"])
            .env("OPIT_BLOCKED_PROBE", "1");
        let start = std::time::Instant::now();
        let result = probe(command, std::time::Duration::from_millis(100));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
    #[test]
    fn only_a_successful_probe_is_cached_and_a_failure_is_retried_later() {
        let cache = HardwareCache::default();
        let start = Instant::now();
        let mut probes = 0;
        let unknown = cache.get(
            || {
                probes += 1;
                None
            },
            start,
        );
        assert_eq!(probes, 1);
        assert!(!unknown.nvidia);
        assert_eq!(unknown.ram_bytes, 0);
        assert!(unknown.physical_cores >= 1, "threads still get a sane default");
        let soon = cache.get(|| panic!("no new probe right after a failure"), start + Duration::from_secs(1));
        assert!(!soon.nvidia);

        let gpu = Hardware { ram_bytes: 16 * 1024 * 1024 * 1024, vram_mb: 8192, nvidia: true, physical_cores: 8 };
        let found = cache.get(|| Some(gpu.clone()), start + RETRY_AFTER);
        assert!(found.nvidia, "a slow first probe must not hide CUDA builds for the whole run");
        assert_eq!(recommendation(&found), "ggml-large-v3-turbo-q5_0.bin");
        let cached = cache.get(|| panic!("a successful probe is cached"), start + RETRY_AFTER * 100);
        assert_eq!((cached.nvidia, cached.physical_cores), (true, 8));
    }

    #[test]
    fn recommendation_uses_ram_and_vram() {
        assert_eq!(recommendation(&Hardware::default()), "ggml-tiny-q5_1.bin");
        let mut h = Hardware { ram_bytes: 8 * 1024 * 1024 * 1024, ..Default::default() };
        assert_eq!(recommendation(&h), "ggml-small-q5_1.bin");
        h.vram_mb = 4096;
        assert_eq!(recommendation(&h), "ggml-large-v3-turbo-q5_0.bin");
    }
}
