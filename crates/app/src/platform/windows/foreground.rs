//! Best-effort foreground snapshots and the Settings application picker. No content is logged.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use crate::platform::{ForegroundApp, ForegroundInfo};

pub struct WinForegroundApp;

fn process_id(hwnd: HWND) -> Option<u32> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn process_exe(pid: u32) -> Option<String> {
    // Maximum Windows extended path length. A single query handles long installations too.
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let result =
        unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len) };
    // Close on both success and failure; no handle survives the snapshot.
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    let exe = opit_core::text::fold(path.rsplit(['\\', '/']).next()?);
    (!exe.is_empty()).then_some(exe)
}

fn window_title(hwnd: HWND) -> String {
    // GetWindowText reads the cached caption of another process; it does not send WM_GETTEXT
    // to that process. A fixed buffer avoids a second length query racing a caption change.
    let mut buffer = vec![0u16; 32768];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

impl ForegroundApp for WinForegroundApp {
    fn current(&self, include_title: bool) -> Option<ForegroundInfo> {
        let hwnd = unsafe { GetForegroundWindow() };
        let pid = process_id(hwnd)?;
        let exe = process_exe(pid)?;
        let title = if include_title { window_title(hwnd) } else { String::new() };
        // A closing/replaced window or a focus change must not mix application identities.
        if hwnd != unsafe { GetForegroundWindow() } || process_id(hwnd) != Some(pid) {
            return None;
        }
        Some(ForegroundInfo { exe, title })
    }

    fn list_running(&self) -> Result<Vec<ForegroundInfo>, String> {
        let mut apps = BTreeMap::<String, String>::new();
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            // Unwinding across a Win32 callback would abort the process.
            let result = catch_unwind(AssertUnwindSafe(|| {
                if !unsafe { IsWindowVisible(hwnd) }.as_bool()
                    || unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOOLWINDOW.0 != 0
                {
                    return;
                }
                let Some(pid) = process_id(hwnd).filter(|pid| *pid != std::process::id()) else { return };
                let Some(exe) = process_exe(pid) else { return };
                let title = window_title(hwnd);
                if process_id(hwnd) != Some(pid) {
                    return;
                }
                // SAFETY: EnumWindows is synchronous and data points to the caller's map.
                let apps = unsafe { &mut *(data.0 as *mut BTreeMap<String, String>) };
                apps.entry(exe)
                    .and_modify(|old| {
                        if old.is_empty() {
                            *old = title.clone();
                        }
                    })
                    .or_insert(title);
            }));
            result.is_ok().into()
        }
        unsafe { EnumWindows(Some(visit), LPARAM(&mut apps as *mut _ as isize)) }
            .map_err(|_| "the application list could not be read".to_owned())?;
        Ok(apps.into_iter().map(|(exe, title)| ForegroundInfo { exe, title }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_lookup_returns_only_a_file_name_and_handles_missing_processes() {
        let exe = process_exe(std::process::id()).expect("the current process is readable");
        assert!(exe.ends_with(".exe"));
        assert!(!exe.contains(['\\', '/', ':']));
        assert_eq!(exe, exe.to_lowercase());
        assert!(process_exe(0).is_none());
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn desktop_picker_has_sorted_unique_file_names() {
        let apps = WinForegroundApp.list_running().expect("enumeration succeeds");
        assert!(apps.iter().all(|app| !app.exe.is_empty() && !app.exe.contains(['\\', '/', ':'])));
        assert!(apps.windows(2).all(|pair| pair[0].exe < pair[1].exe));
        // Exercise the real foreground read without displaying or logging either field.
        if let Some(info) = WinForegroundApp.current(false) {
            assert!(info.title.is_empty());
        }
    }
}
