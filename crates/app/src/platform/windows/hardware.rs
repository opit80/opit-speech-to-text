//! Windows hardware detection through supervised, time-bounded probes.
use crate::local::runtime::hardware::{Hardware, auto_threads};
use std::time::Duration;

/// PowerShell/CIM start-up can take many seconds on a busy, low-memory machine. The probe always
/// runs on a blocking worker, so a generous limit costs no UI responsiveness.
#[cfg(windows)]
const CIM_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(windows)]
const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(windows)]
fn probe(mut command: std::process::Command, timeout: std::time::Duration) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Read};
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use std::time::Instant;

    command.creation_flags(0x00000004 | 0x08000000).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut tree = crate::platform::process::Tree::new().map_err(|_| Error::other("hardware probe unavailable"))?;
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

/// `None` when the system probe failed or timed out; a missing GPU memory reading is not a failure.
pub(crate) fn detect() -> Option<Hardware> {
    let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
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
            .args(["--exact", "platform::windows::hardware::tests::blocked_probe_child", "--nocapture"])
            .env("OPIT_BLOCKED_PROBE", "1");
        let start = std::time::Instant::now();
        let result = probe(command, std::time::Duration::from_millis(100));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
