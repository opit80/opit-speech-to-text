use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

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
        crate::platform::windows::hardware::detect()
    }
    #[cfg(not(windows))]
    {
        Some(fallback())
    }
}

pub(crate) fn auto_threads() -> u16 {
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
