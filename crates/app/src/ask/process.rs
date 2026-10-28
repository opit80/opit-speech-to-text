//! Child supervision: Windows children start suspended, enter a kill-on-close Job Object,
//! then resume. Pipe workers are joined after the entire tree has stopped.
use crate::controller::ErrorKind;
use opit_core::ask::MAX_ANSWER;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub fn run(
    executable: PathBuf,
    args: Vec<String>,
    directory: PathBuf,
    input: String,
    timeout: Duration,
    cancel: watch::Receiver<bool>,
) -> Result<Vec<u8>, ErrorKind> {
    if *cancel.borrow() || cancel.has_changed().is_err() {
        return Err(ErrorKind::Cancelled);
    }
    if super::cli::is_shim(&executable) && directory.to_string_lossy().starts_with(r"\\") {
        // cmd.exe cannot retain a UNC working directory. Fail closed rather than
        // silently running a workspace-enabled agent from the Windows directory.
        return Err(ErrorKind::Rejected);
    }
    let mut command = build_command(executable, args)?;
    #[cfg(test)]
    command.env("OPIT_ASK_CHILD", "1");
    command.current_dir(directory).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000004 | 0x08000000); // CREATE_SUSPENDED | CREATE_NO_WINDOW
    }
    let mut tree = Tree::new()?;
    let mut child = command.spawn().map_err(|_| ErrorKind::Rejected)?;
    if let Err(error) = tree.attach(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let mut stdin = child.stdin.take().ok_or(ErrorKind::Rejected)?;
    let stdout = child.stdout.take().ok_or(ErrorKind::Rejected)?;
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take((MAX_ANSWER + 1) as u64).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let result = loop {
        if *cancel.borrow() || cancel.has_changed().is_err() {
            break Err(ErrorKind::Cancelled);
        }
        if start.elapsed() >= timeout {
            break Err(ErrorKind::Timeout);
        }
        match child.try_wait() {
            Ok(Some(status)) => break if status.success() { Ok(()) } else { Err(ErrorKind::Rejected) },
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break Err(ErrorKind::Rejected),
        }
    };
    tree.stop();
    let _ = child.kill();
    let _ = child.wait();
    let wrote = writer.join().map_err(|_| ErrorKind::Rejected)?.map_err(|_| ErrorKind::Rejected);
    let bytes = reader.join().map_err(|_| ErrorKind::BadResponse)?.map_err(|_| ErrorKind::BadResponse);
    result?;
    wrote?;
    let bytes = bytes?;
    if bytes.len() > MAX_ANSWER {
        return Err(ErrorKind::TooLarge);
    }
    Ok(bytes)
}

fn build_command(executable: PathBuf, args: Vec<String>) -> Result<Command, ErrorKind> {
    if super::cli::is_shim(&executable) {
        // cmd.exe interprets metacharacters even inside quoted argv. Reject them in
        // the resolved path; model/session values are already restricted by cli::arguments.
        let path = executable.to_str().ok_or(ErrorKind::Rejected)?;
        if path.chars().any(|c| "&|<>^%!\"\r\n".contains(c)) {
            return Err(ErrorKind::Rejected);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let executable = PathBuf::from(path.replace('/', "\\"));
            let system = std::env::var_os("SystemRoot").ok_or(ErrorKind::Rejected)?;
            let mut command = Command::new(PathBuf::from(system).join("System32").join("cmd.exe"));
            command.args(["/d", "/s", "/v:off", "/c"]).raw_arg("\"").arg(executable).args(args).raw_arg("\"");
            return Ok(command);
        }
        #[cfg(not(windows))]
        return Err(ErrorKind::Rejected);
    }
    let mut command = Command::new(executable);
    command.args(args);
    Ok(command)
}

#[cfg(windows)]
pub(crate) struct Tree(windows::Win32::Foundation::HANDLE);

// The owned job handle has no thread affinity and is accessed through exclusive ownership.
#[cfg(windows)]
unsafe impl Send for Tree {}

#[cfg(windows)]
impl Tree {
    pub(crate) fn new() -> Result<Self, ErrorKind> {
        use windows::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|_| ErrorKind::Rejected)?;
            let tree = Self(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(|_| ErrorKind::Rejected)?;
            Ok(tree)
        }
    }
    pub(crate) fn attach(&mut self, child: &std::process::Child) -> Result<(), ErrorKind> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::{Diagnostics::ToolHelp::*, JobObjects::AssignProcessToJobObject, Threading::*};
        // The child cannot execute or create descendants before assignment succeeds.
        unsafe {
            AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())).map_err(|_| ErrorKind::Rejected)?;
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(|_| ErrorKind::Rejected)?;
            let mut entry = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
            let mut next = Thread32First(snapshot, &mut entry);
            let mut resumed = false;
            while next.is_ok() {
                if entry.th32OwnerProcessID == child.id()
                    && let Ok(thread) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                {
                    resumed |= ResumeThread(thread) != u32::MAX;
                    let _ = CloseHandle(thread);
                }
                next = Thread32Next(snapshot, &mut entry);
            }
            let _ = CloseHandle(snapshot);
            if resumed { Ok(()) } else { Err(ErrorKind::Rejected) }
        }
    }
    pub(crate) fn stop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
pub(crate) struct Tree;
#[cfg(not(windows))]
impl Tree {
    pub(crate) fn new() -> Result<Self, ErrorKind> {
        Err(ErrorKind::Rejected)
    }
    pub(crate) fn attach(&mut self, _: &std::process::Child) -> Result<(), ErrorKind> {
        Err(ErrorKind::Rejected)
    }
    pub(crate) fn stop(&mut self) {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn fake_child() {
        if std::env::var_os("OPIT_ASK_CHILD").is_none() {
            return;
        }
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        if let Some(path) = input.strip_prefix("tree:") {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ask::process::tests::fake_grandchild", "--nocapture"])
                .env("OPIT_ASK_GRANDCHILD_PID", path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
        assert!(!std::env::args().any(|arg| arg == input));
        if input == "wait" {
            std::thread::sleep(Duration::from_secs(30));
        }
        println!("stdin received: {input}");
    }
    #[test]
    fn fake_grandchild() {
        if let Some(path) = std::env::var_os("OPIT_ASK_GRANDCHILD_PID") {
            std::fs::write(path, std::process::id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
    }
    fn child_input(input: &str, timeout: Duration, cancel: watch::Receiver<bool>) -> Result<Vec<u8>, ErrorKind> {
        // Launch a fixed test child, never a real agent or a shell command.
        let executable = std::env::current_exe().unwrap();
        run(
            executable,
            vec!["--exact".into(), "ask::process::tests::fake_child".into(), "--nocapture".into()],
            std::env::current_dir().unwrap(),
            input.into(),
            timeout,
            cancel,
        )
    }
    #[test]
    fn stdin_preserves_request_and_cmd_shims_with_spaces_are_supported() {
        let (_tx, rx) = watch::channel(false);
        let request = "private request & --danger %PATH% | injected";
        let bytes = child_input(request, Duration::from_secs(5), rx.clone()).unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains(request));
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("shim with spaces.cmd");
        std::fs::write(&shim, "@echo off\r\nfindstr .*\r\n").unwrap();
        let bytes = run(
            shim,
            vec!["--model".into(), "safe-model".into()],
            dir.path().into(),
            format!("{request}\n"),
            Duration::from_secs(5),
            rx,
        )
        .unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains(request));
        // An npm-style shim must preserve every fixed/validated argument, including JSON
        // settings and empty tool lists, while forwarding request text only on stdin.
        std::fs::write(dir.path().join("capture.cjs"), "let input='';process.stdin.setEncoding('utf8');process.stdin.on('data',x=>input+=x);process.stdin.on('end',()=>console.log(JSON.stringify({args:process.argv.slice(2),input})));" ).unwrap();
        std::fs::write(dir.path().join("shim with spaces.cmd"), "@echo off\r\nnode.exe \"%~dp0capture.cjs\" %*\r\n")
            .unwrap();
        let (_tx, rx) = watch::channel(false);
        for agent in [opit_core::ask::Agent::Claude, opit_core::ask::Agent::Codex] {
            let config = opit_core::ask::AskConfig { agent, ..Default::default() };
            let args = super::super::cli::arguments(&config, Some("12345678-1234-1234-1234-123456789abc")).unwrap();
            let bytes = run(
                dir.path().join("shim with spaces.cmd"),
                args.clone(),
                dir.path().into(),
                request.into(),
                Duration::from_secs(5),
                rx.clone(),
            )
            .unwrap();
            let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(result["args"], serde_json::json!(args));
            assert_eq!(result["input"], request);
        }
        assert!(build_command(dir.path().join("unsafe&shim.cmd"), Vec::new()).is_err());
    }
    #[test]
    fn timeout_kills_descendants() {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grandchild.pid");
        let (_tx, rx) = watch::channel(false);
        assert_eq!(
            child_input(&format!("tree:{}", path.display()), Duration::from_millis(800), rx),
            Err(ErrorKind::Timeout)
        );
        let pid: u32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                assert_eq!(WaitForSingleObject(handle, 2000), WAIT_OBJECT_0);
                let _ = CloseHandle(handle);
            }
        }
    }
    #[test]
    fn timeout_and_cancel_terminate_child() {
        let (_tx, rx) = watch::channel(false);
        let start = Instant::now();
        assert_eq!(child_input("wait", Duration::from_millis(150), rx), Err(ErrorKind::Timeout));
        assert!(start.elapsed() < Duration::from_secs(5));
        let (tx, rx) = watch::channel(false);
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let _ = tx.send(true);
            std::thread::sleep(Duration::from_millis(100));
        });
        assert_eq!(child_input("wait", Duration::from_secs(5), rx), Err(ErrorKind::Cancelled));
        cancel.join().unwrap();
    }
}
