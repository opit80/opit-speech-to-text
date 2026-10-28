use super::{LocalError, Result};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::Instant;

pub enum Process {
    Native {
        child: Child,
        tree: crate::ask::process::Tree,
    },
    #[cfg(test)]
    Fake(std::sync::Arc<std::sync::atomic::AtomicBool>),
}
impl Process {
    pub fn terminate(&mut self) {
        match self {
            Self::Native { child, tree } => {
                tree.stop();
                let _ = child.kill();
                let _ = child.wait();
            }
            #[cfg(test)]
            Self::Fake(alive) => alive.store(false, std::sync::atomic::Ordering::SeqCst),
        }
    }
    pub fn alive(&mut self) -> bool {
        match self {
            Self::Native { child, .. } => matches!(child.try_wait(), Ok(None)),
            #[cfg(test)]
            Self::Fake(alive) => alive.load(std::sync::atomic::Ordering::SeqCst),
        }
    }
    pub fn launch(exe: &Path, model: &Path, port: u16, threads: u16, gpu: bool) -> Result<Self> {
        let mut command = Command::new(exe);
        command
            .current_dir(exe.parent().ok_or(LocalError::MissingServer)?)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--model")
            .arg(model)
            .arg("--threads")
            .arg(threads.max(1).to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !gpu {
            command.arg("--no-gpu");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000004 | 0x08000000);
        }
        let mut tree = crate::ask::process::Tree::new().map_err(|_| LocalError::Launch)?;
        let mut child = command.spawn().map_err(|_| LocalError::Launch)?;
        if tree.attach(&child).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LocalError::Launch);
        }
        Ok(Self::Native { child, tree })
    }
}

#[derive(Default)]
pub struct RestartBudget(u8);
impl RestartBudget {
    pub fn crashed(&mut self) -> Result<()> {
        if self.0 >= 1 {
            return Err(LocalError::Crash);
        }
        self.0 += 1;
        Ok(())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn free_port() -> Result<u16> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .and_then(|s| s.local_addr())
        .map(|a| a.port())
        .map_err(|_| LocalError::Port)
}

pub fn idle_expired(last_used: Instant, now: Instant, minutes: u16) -> bool {
    minutes != 0 && now.saturating_duration_since(last_used) >= Duration::from_secs(u64::from(minutes) * 60)
}

pub async fn readiness<F, Fut>(timeout: Duration, mut probe: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, probe()).await {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(LocalError::Readiness),
        }
        if Instant::now() >= deadline {
            return Err(LocalError::Readiness);
        }
        tokio::time::sleep_until((Instant::now() + Duration::from_millis(200)).min(deadline)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_crash_restart_is_allowed_until_the_session_is_reset() {
        let mut budget = RestartBudget::default();
        assert!(budget.crashed().is_ok());
        assert!(matches!(budget.crashed(), Err(LocalError::Crash)));
        assert!(matches!(budget.crashed(), Err(LocalError::Crash)));
        budget = RestartBudget::default();
        assert!(budget.crashed().is_ok());
    }
    #[test]
    fn port_is_ephemeral_and_idle_zero_never_stops() {
        assert_ne!(free_port().unwrap(), 0);
        let start = Instant::now();
        assert!(!idle_expired(start, start + Duration::from_secs(599), 10));
        assert!(idle_expired(start, start + Duration::from_secs(600), 10));
        assert!(!idle_expired(start, start + Duration::from_secs(999999), 0));
    }
    #[tokio::test(start_paused = true)]
    async fn readiness_waits_then_succeeds_or_times_out() {
        let mut calls = 0;
        readiness(Duration::from_secs(2), || {
            calls += 1;
            let ready = calls == 3;
            async move { Ok(ready) }
        })
        .await
        .unwrap();
        assert_eq!(calls, 3);
        assert!(matches!(readiness(Duration::from_secs(1), || async { Ok(false) }).await, Err(LocalError::Readiness)));
        assert!(matches!(
            readiness(Duration::from_secs(1), || async { Err(LocalError::Crash) }).await,
            Err(LocalError::Crash)
        ));
    }
}
