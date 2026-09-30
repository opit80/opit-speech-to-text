//! Paste by clipboard + synthesized Ctrl+V, with clipboard backup/restore and a UIPI check.

use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::Owned;

use super::clipboard::{Clipboard, Snapshot, sequence_number};
use crate::platform::{CLIPBOARD_RESTORE_DELAY, PasteOutcome, Paster};

/// Longest wait for the user to let go of modifiers before sending Ctrl+V anyway.
const MODIFIER_WAIT: Duration = Duration::from_secs(1);
const MODIFIER_POLL: Duration = Duration::from_millis(10);
const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];

/// Clipboard + Ctrl+V paster.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinPaster;

impl WinPaster {
    pub fn new() -> Self {
        Self
    }
}

impl Paster for WinPaster {
    fn paste(&self, text: &str, restore: bool) -> PasteOutcome {
        let (backup, seq) = match put_text(text, restore) {
            Ok(v) => v,
            Err(e) => return PasteOutcome::Failed(e),
        };
        wait_for_modifiers_released(MODIFIER_WAIT);
        if foreground_is_not_a_paste_target() {
            return PasteOutcome::ClipboardOnly;
        }
        if ctrl_v_blocked(foreground_is_higher_integrity(), has_foreground_window()) {
            // UIPI would drop the input silently; leave the text on the clipboard for the user.
            return PasteOutcome::ClipboardOnly;
        }
        if !send_ctrl_v() {
            return PasteOutcome::ClipboardOnly;
        }
        // Restore only after a real paste: on ClipboardOnly the user still needs our text.
        if let Some(backup) = backup {
            let spawned = thread::Builder::new().name("opit-clipboard-restore".into()).spawn(move || {
                thread::sleep(CLIPBOARD_RESTORE_DELAY);
                let _ = restore_if_unchanged(&backup, seq);
            });
            drop(spawned); // Best effort: if the thread cannot start, our text simply stays.
        }
        PasteOutcome::Pasted
    }
}

/// Whether Ctrl+V must be skipped. Fail closed: a foreground window whose integrity we cannot
/// read (typically an elevated process of another user) is treated as elevated, because UIPI
/// would drop our keys silently and the restore would then erase the dictation. Without any
/// foreground window there is nothing to block, so we paste as before.
fn ctrl_v_blocked(higher_integrity: Option<bool>, has_foreground_window: bool) -> bool {
    higher_integrity.unwrap_or(has_foreground_window)
}

/// Window classes of the shell: taskbar, tray overflow flyout, desktop.
const SHELL_WINDOW_CLASSES: [&str; 6] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "Progman",
    "WorkerW",
];

/// Whether the foreground window must not get our Ctrl+V: a shell window (clicking the tray
/// icon leaves the taskbar or the overflow flyout in front) or one of our own windows (the
/// settings window's Stop button). The keys would land there and the restore would then take
/// the dictation off the clipboard, so the text stays on the clipboard instead.
fn not_a_paste_target(class: Option<&str>, own_process: bool) -> bool {
    own_process || class.is_some_and(|class| SHELL_WINDOW_CLASSES.contains(&class))
}

/// [`not_a_paste_target`] for the current foreground window; false when there is none.
fn foreground_is_not_a_paste_target() -> bool {
    // SAFETY: plain FFI calls; the out-pointer and the buffer are valid for each call.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut buf = [0u16; 256];
        let len = usize::try_from(GetClassNameW(hwnd, &mut buf)).unwrap_or(0);
        let class = (len > 0).then(|| String::from_utf16_lossy(&buf[..len]));
        not_a_paste_target(class.as_deref(), pid != 0 && pid == GetCurrentProcessId())
    }
}

fn has_foreground_window() -> bool {
    // SAFETY: plain FFI call.
    !unsafe { GetForegroundWindow() }.is_invalid()
}

/// Backs up the clipboard (when `backup`), writes `text` marked private and returns the
/// backup plus the clipboard sequence number right after our write.
pub fn put_text(text: &str, backup: bool) -> Result<(Option<Snapshot>, u32), String> {
    let clipboard = Clipboard::open()?;
    let snapshot = backup.then(|| clipboard.snapshot());
    clipboard.set_private_text(text)?;
    drop(clipboard);
    Ok((snapshot, sequence_number()))
}

/// Restores `snapshot` unless someone changed the clipboard after our write (`seq`).
/// Returns whether it restored.
pub fn restore_if_unchanged(snapshot: &Snapshot, seq: u32) -> Result<bool, String> {
    if sequence_number() != seq {
        return Ok(false);
    }
    let clipboard = Clipboard::open()?;
    // Re-check while we hold the clipboard so nobody can slip in between.
    if sequence_number() != seq {
        return Ok(false);
    }
    clipboard.restore(snapshot)?;
    Ok(true)
}

/// Waits (up to `timeout`) until Shift/Ctrl/Alt/Win are all physically released, so a
/// still-held hotkey modifier does not turn Ctrl+V into Ctrl+Shift+V. Returns false on timeout.
pub fn wait_for_modifiers_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: plain FFI calls.
        let held = MODIFIERS.iter().any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0);
        if !held {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(MODIFIER_POLL);
    }
}

/// (virtual key, key-up) for Ctrl down, V down, V up, Ctrl up.
pub const CTRL_V_SEQUENCE: [(VIRTUAL_KEY, bool); 4] =
    [(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)];

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } },
    }
}

/// The four keyboard INPUTs for Ctrl+V.
pub fn ctrl_v_inputs() -> [INPUT; 4] {
    CTRL_V_SEQUENCE.map(|(vk, up)| key_input(vk, up))
}

/// Sends Ctrl+V; true only if all four events were accepted.
fn send_ctrl_v() -> bool {
    let inputs = ctrl_v_inputs();
    // SAFETY: valid INPUT slice and matching cbSize.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if sent != inputs.len() && sent > 0 {
        // Partially sent: never leave Ctrl or V logically stuck down.
        let release = [key_input(VK_V, true), key_input(VK_CONTROL, true)];
        // SAFETY: as above.
        unsafe { SendInput(&release, size_of::<INPUT>() as i32) };
    }
    sent == inputs.len()
}

/// Integrity RID (e.g. 0x2000 medium, 0x3000 high) of the process owning the foreground
/// window. None when there is no foreground window or its process cannot be queried.
pub fn foreground_integrity_rid() -> Option<u32> {
    // SAFETY: plain FFI calls; the out-pointer is valid for the call.
    let pid = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    };
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is owned and closed by `Owned`.
    let process = unsafe { Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?) };
    token_integrity_rid(*process)
}

/// Integrity RID of this process.
pub fn own_integrity_rid() -> Option<u32> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    token_integrity_rid(unsafe { GetCurrentProcess() })
}

/// Some(true) when the foreground app runs at a higher integrity level than we do (so UIPI
/// blocks our SendInput), None when unknown.
pub fn foreground_is_higher_integrity() -> Option<bool> {
    Some(foreground_integrity_rid()? > own_integrity_rid()?)
}

fn token_integrity_rid(process: HANDLE) -> Option<u32> {
    // SAFETY: every out-pointer points to live, correctly sized storage; the token handle is
    // closed by `Owned`; the SID pointer lives inside `buf`, which outlives its use.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let token = Owned::new(token);
        let mut len = 0u32;
        // First call only reports the needed size (and fails with ERROR_INSUFFICIENT_BUFFER).
        let _ = GetTokenInformation(*token, TokenIntegrityLevel, None, 0, &mut len);
        if (len as usize) < size_of::<TOKEN_MANDATORY_LABEL>() {
            return None;
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)]; // u64 for pointer alignment
        GetTokenInformation(*token, TokenIntegrityLevel, Some(buf.as_mut_ptr().cast()), len, &mut len).ok()?;
        let label = &*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::super::clipboard::{CF_UNICODETEXT, register_format, utf16_bytes};
    use super::*;

    #[test]
    fn ctrl_v_inputs_are_ctrl_down_v_down_v_up_ctrl_up() {
        let inputs = ctrl_v_inputs();
        let got: Vec<(u16, u32)> = inputs
            .iter()
            .map(|i| {
                assert_eq!(i.r#type, INPUT_KEYBOARD);
                // SAFETY: we built these as keyboard inputs.
                let ki = unsafe { i.Anonymous.ki };
                assert_eq!((ki.wScan, ki.time, ki.dwExtraInfo), (0, 0, 0));
                (ki.wVk.0, ki.dwFlags.0)
            })
            .collect();
        assert_eq!(got, vec![(0x11, 0), (0x56, 0), (0x56, KEYEVENTF_KEYUP.0), (0x11, KEYEVENTF_KEYUP.0)]);
    }

    #[test]
    fn unknown_integrity_blocks_ctrl_v_only_with_a_foreground_window() {
        assert!(ctrl_v_blocked(Some(true), true));
        assert!(!ctrl_v_blocked(Some(false), true));
        assert!(ctrl_v_blocked(None, true));
        assert!(!ctrl_v_blocked(None, false));
    }

    #[test]
    fn shell_windows_and_our_own_windows_get_no_ctrl_v() {
        for class in SHELL_WINDOW_CLASSES {
            assert!(not_a_paste_target(Some(class), false), "{class}");
        }
        assert!(not_a_paste_target(Some("Chrome_WidgetWin_1"), true), "our own WebView");
        assert!(not_a_paste_target(None, true));
        assert!(!not_a_paste_target(Some("Notepad"), false));
        assert!(!not_a_paste_target(Some("Chrome_WidgetWin_1"), false));
        assert!(!not_a_paste_target(None, false));
    }

    #[test]
    fn own_integrity_is_known() {
        let rid = own_integrity_rid().expect("own integrity");
        assert!((0x1000..=0x4000).contains(&rid), "{rid:#x}");
    }

    fn read(format: u32) -> Option<Vec<u8>> {
        let clipboard = Clipboard::open().unwrap();
        clipboard.snapshot().formats.into_iter().find(|(f, _)| *f == format).map(|(_, b)| b)
    }

    fn text_of(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    }

    /// Round-trips text + a custom format through put_text / restore_if_unchanged and prints
    /// the integrity levels. Saves and puts back the user's clipboard around the test.
    #[test]
    #[ignore = "touches the real clipboard"]
    fn clipboard_backup_restore_smoke() {
        let user = Clipboard::open().unwrap().snapshot();
        let custom = register_format("OpitSmokeFormat").unwrap();
        let payload = vec![1u8, 2, 3, 4, 5, 250];
        {
            let cb = Clipboard::open().unwrap();
            cb.empty().unwrap();
            cb.set_bytes(CF_UNICODETEXT, &utf16_bytes("original ğüş")).unwrap();
            cb.set_bytes(custom, &payload).unwrap();
        }

        let (backup, seq) = put_text("dictated text", true).unwrap();
        let backup = backup.unwrap();
        println!("backup formats: {:?}", backup.formats.iter().map(|(f, b)| (*f, b.len())).collect::<Vec<_>>());
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "dictated text");
        let history = register_format("CanIncludeInClipboardHistory").unwrap();
        assert_eq!(read(history).unwrap()[..4], [0, 0, 0, 0]);
        assert_eq!(sequence_number(), seq, "reading must not bump the sequence number");

        assert_eq!(restore_if_unchanged(&backup, seq), Ok(true));
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "original ğüş");
        assert_eq!(read(custom).unwrap()[..payload.len()], payload[..]);

        // The user copies something after our write: restore must not clobber it.
        let (backup2, seq2) = put_text("second dictation", true).unwrap();
        {
            let cb = Clipboard::open().unwrap();
            cb.empty().unwrap();
            cb.set_bytes(CF_UNICODETEXT, &utf16_bytes("user copy")).unwrap();
        }
        assert_eq!(restore_if_unchanged(&backup2.unwrap(), seq2), Ok(false));
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "user copy");

        // Back-to-back writes (the OpenClipboard(NULL) race this guards against).
        for i in 0..40 {
            put_text(&format!("stress {i}"), false).unwrap();
        }
        assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), "stress 39");

        // Empty original clipboard → restoring leaves it empty.
        Clipboard::open().unwrap().empty().unwrap();
        let (backup3, seq3) = put_text("third", true).unwrap();
        let backup3 = backup3.unwrap();
        assert!(backup3.formats.is_empty());
        assert_eq!(restore_if_unchanged(&backup3, seq3), Ok(true));
        assert!(Clipboard::open().unwrap().snapshot().formats.is_empty());

        println!("own integrity RID: {:#x?}", own_integrity_rid());
        println!("foreground integrity RID: {:#x?}", foreground_integrity_rid());
        println!("foreground is higher: {:?}", foreground_is_higher_integrity());

        Clipboard::open().unwrap().restore(&user).unwrap();
    }
}
