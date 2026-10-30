//! Native Job Object supervision for suspended Windows child processes.
use crate::controller::ErrorKind;

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
