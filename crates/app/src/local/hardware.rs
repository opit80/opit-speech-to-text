use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Hardware {
    pub ram_bytes: u64,
    pub vram_mb: u64,
    pub nvidia: bool,
    pub physical_cores: u16,
}

pub fn detect() -> Hardware {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system =
            std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        let result = std::process::Command::new(system.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
            .args(["-NoProfile","-NonInteractive","-Command", "$ErrorActionPreference='Stop'; $c=Get-CimInstance Win32_ComputerSystem; $p=Get-CimInstance Win32_Processor; $g=Get-CimInstance Win32_VideoController; @{ram_bytes=[UInt64]$c.TotalPhysicalMemory;physical_cores=[int](($p|Measure-Object NumberOfCores -Sum).Sum);nvidia=[bool]($g|Where-Object Name -Match NVIDIA)}|ConvertTo-Json -Compress"])
            .creation_flags(0x08000000).output();
        let mut hardware = result
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<Hardware>(&o.stdout).ok())
            .unwrap_or_default();
        if hardware.nvidia
            && let Ok(output) = std::process::Command::new(system.join("System32/nvidia-smi.exe"))
                .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
                .creation_flags(0x08000000)
                .output()
        {
            hardware.vram_mb = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|s| s.trim().parse::<u64>().ok())
                .max()
                .unwrap_or(0);
        }
        if hardware.physical_cores == 0 {
            hardware.physical_cores = auto_threads();
        }
        hardware
    }
    #[cfg(not(windows))]
    {
        Hardware { physical_cores: auto_threads(), ..Default::default() }
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
    #[test]
    fn recommendation_uses_ram_and_vram() {
        assert_eq!(recommendation(&Hardware::default()), "ggml-tiny-q5_1.bin");
        let mut h = Hardware { ram_bytes: 8 * 1024 * 1024 * 1024, ..Default::default() };
        assert_eq!(recommendation(&h), "ggml-small-q5_1.bin");
        h.vram_mb = 4096;
        assert_eq!(recommendation(&h), "ggml-large-v3-turbo-q5_0.bin");
    }
}
