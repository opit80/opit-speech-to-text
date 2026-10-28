//! Configurable clipboard shortcuts or Unicode typing, with shared foreground/UIPI guards.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use opit_core::config::{PasteAfter, PasteMethod};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_INSERT, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::Owned;

use super::clipboard::{Clipboard, Snapshot, sequence_number};
use crate::platform::{CLIPBOARD_RESTORE_DELAY, PasteOutcome, PasteRequest, Paster, SelectionError};

// Selection capture and insertion share the clipboard, including delayed restoration.
static CLIPBOARD_OPERATION: Mutex<()> = Mutex::new(());

/// Longest wait for the user to let go of modifiers before sending insertion input.
const MODIFIER_WAIT: Duration = Duration::from_secs(1);
const MODIFIER_POLL: Duration = Duration::from_millis(10);
const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN];
/// At most 32 UTF-16 units per SendInput, with a short breather for the receiving app.
const TYPING_CHUNK_INPUTS: usize = 64;
const TYPING_CHUNK_DELAY: Duration = Duration::from_millis(5);

/// Blocking text insertion. The controller serializes calls, including clipboard restoration.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinPaster;

impl WinPaster {
    pub fn new() -> Self {
        Self
    }
}

impl Paster for WinPaster {
    fn read_selection(&self) -> Result<Option<String>, SelectionError> {
        let _operation = CLIPBOARD_OPERATION.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: plain foreground query; the handle is used only on this blocking thread.
        let target = unsafe { GetForegroundWindow() };
        if target.is_invalid() || selection_blocked() {
            return Ok(None);
        }
        let mut target_pid = 0;
        // SAFETY: the out-pointer is valid; target is only queried.
        unsafe {
            GetWindowThreadProcessId(target, Some(&mut target_pid));
        }
        let clipboard = Clipboard::open().map_err(|_| SelectionError::Unavailable)?;
        let snapshot = clipboard.snapshot();
        let sequence = sequence_number();
        drop(clipboard);
        // Fail closed if focus changed while delayed clipboard formats were rendered.
        if unsafe { GetForegroundWindow() } != target || selection_blocked() {
            return Ok(None);
        }
        let modifiers: Vec<_> = [VK_LCONTROL, VK_RCONTROL, VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .filter(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0)
            .collect();
        // One atomic input batch neutralizes held shortcut modifiers and restores them. This
        // also supports modifier-based push-to-talk without waiting for the release to copy.
        let inputs = copy_inputs(&modifiers);
        let sent = send_inputs(&inputs);
        if !sent {
            // A partial batch may have released a physically held shortcut modifier.
            let restore: Vec<_> = modifiers.iter().map(|&vk| key_input(vk, false)).collect();
            let _ = send_inputs(&restore);
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let changed = sequence_number();
            if changed != sequence {
                let clipboard = Clipboard::open().map_err(|_| SelectionError::Unavailable)?;
                // Preserve an unrelated user copy and never mistake it for the selection.
                if sequence_number() != changed || !clipboard.owned_by_process(target_pid) {
                    return Err(SelectionError::Unavailable);
                }
                let result = if sent && unsafe { GetForegroundWindow() } == target {
                    clipboard.selection_text()
                } else {
                    Err(SelectionError::Unavailable)
                };
                clipboard.restore(&snapshot).map_err(|_| SelectionError::Unavailable)?;
                return result;
            }
            if Instant::now() >= deadline {
                return if sent { Ok(None) } else { Err(SelectionError::Unavailable) };
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn can_auto_paste(&self) -> bool {
        !foreground_is_not_a_paste_target()
    }

    fn paste(&self, text: &str, request: PasteRequest) -> PasteOutcome {
        let _operation = CLIPBOARD_OPERATION.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if request.method == PasteMethod::Type {
            if !request.auto_paste {
                return clipboard_only(text);
            }
            wait_for_modifiers_released(MODIFIER_WAIT);
            if insertion_blocked() {
                return clipboard_only(text);
            }
            let chunks = typing_inputs(text);
            if chunks.is_empty() {
                return PasteOutcome::Failed("no text to insert".into());
            }
            for (index, chunk) in chunks.iter().enumerate() {
                if index > 0 {
                    thread::sleep(TYPING_CHUNK_DELAY);
                    // A long insertion must stop if focus moves to an unsafe target.
                    if insertion_blocked() {
                        return PasteOutcome::Failed("text input interrupted by a target change".into());
                    }
                }
                if !send_inputs(chunk) {
                    // Never copy or retry after partial typing: that could duplicate inserted text.
                    return PasteOutcome::Failed("text input could not be delivered".into());
                }
            }
            send_after(request.after);
            return PasteOutcome::Pasted;
        }
        let (backup, seq) = match put_text(text, request.restore) {
            Ok(v) => v,
            Err(e) => return PasteOutcome::Failed(e),
        };
        if !request.auto_paste {
            return PasteOutcome::ClipboardOnly;
        }
        wait_for_modifiers_released(MODIFIER_WAIT);
        if insertion_blocked() {
            // UIPI would drop the input silently; leave the text on the clipboard for the user.
            return PasteOutcome::ClipboardOnly;
        }
        if !send_inputs(&paste_inputs(request.method)) {
            return PasteOutcome::ClipboardOnly;
        }
        send_after(request.after);
        // Restore only after a real paste: on ClipboardOnly the user still needs our text.
        if let Some(backup) = backup {
            thread::sleep(CLIPBOARD_RESTORE_DELAY);
            let _ = restore_if_unchanged(&backup, seq);
        }
        PasteOutcome::Pasted
    }
}

fn clipboard_only(text: &str) -> PasteOutcome {
    match put_text(text, false) {
        Ok(_) => PasteOutcome::ClipboardOnly,
        Err(error) => PasteOutcome::Failed(error),
    }
}

fn insertion_blocked() -> bool {
    foreground_is_not_a_paste_target() || input_blocked(foreground_is_higher_integrity(), has_foreground_window())
}

fn selection_blocked() -> bool {
    insertion_blocked() || foreground_integrity_rid().is_some_and(|rid| rid >= 0x3000)
}

/// Whether insertion input must be skipped. Fail closed: a foreground window whose integrity we cannot
/// read (typically an elevated process of another user) is treated as elevated, because UIPI
/// would drop our keys silently and the restore would then erase the dictation. Without any
/// foreground window there is nothing to block, so we paste as before.
fn input_blocked(higher_integrity: Option<bool>, has_foreground_window: bool) -> bool {
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

/// Whether the foreground window must not get our insertion input: a shell window (clicking the tray
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

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } },
    }
}

fn chord_inputs(keys: &[VIRTUAL_KEY]) -> Vec<INPUT> {
    keys.iter().map(|&vk| key_input(vk, false)).chain(keys.iter().rev().map(|&vk| key_input(vk, true))).collect()
}

fn copy_inputs(held_modifiers: &[VIRTUAL_KEY]) -> Vec<INPUT> {
    held_modifiers
        .iter()
        .map(|&vk| key_input(vk, true))
        .chain(chord_inputs(&[VK_CONTROL, VK_C]))
        .chain(held_modifiers.iter().map(|&vk| key_input(vk, false)))
        .collect()
}

fn paste_inputs(method: PasteMethod) -> Vec<INPUT> {
    match method {
        PasteMethod::CtrlV => chord_inputs(&[VK_CONTROL, VK_V]),
        PasteMethod::ShiftInsert => chord_inputs(&[VK_SHIFT, VK_INSERT]),
        PasteMethod::CtrlShiftV => chord_inputs(&[VK_CONTROL, VK_SHIFT, VK_V]),
        PasteMethod::Type => Vec::new(),
    }
}

fn after_inputs(after: PasteAfter) -> Vec<INPUT> {
    match after {
        PasteAfter::None => Vec::new(),
        PasteAfter::Enter => chord_inputs(&[VK_RETURN]),
        PasteAfter::CtrlEnter => chord_inputs(&[VK_CONTROL, VK_RETURN]),
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    }
}

/// Pure builder: a Unicode scalar (including both surrogate units), each down/up pair,
/// and a CRLF newline always stay inside one bounded chunk.
fn typing_inputs(text: &str) -> Vec<Vec<INPUT>> {
    let mut chunks = Vec::new();
    let mut chunk = Vec::with_capacity(TYPING_CHUNK_INPUTS);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let mut units = [0; 2];
        let next = if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            chord_inputs(&[VK_RETURN])
        } else {
            ch.encode_utf16(&mut units)
                .iter()
                .flat_map(|&unit| [unicode_input(unit, false), unicode_input(unit, true)])
                .collect()
        };
        if chunk.len() + next.len() > TYPING_CHUNK_INPUTS {
            chunks.push(std::mem::replace(&mut chunk, Vec::with_capacity(TYPING_CHUNK_INPUTS)));
        }
        chunk.extend(next);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// True only when every event was accepted. Best-effort key releases prevent stuck keys
/// after a partial send; never retry the text or the shortcut.
fn send_inputs(inputs: &[INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    // SAFETY: valid INPUT slice and matching cbSize.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) } as usize;
    if sent != inputs.len() && sent > 0 {
        let release: Vec<_> = inputs
            .iter()
            .copied()
            .filter(|input| {
                // SAFETY: all callers supply keyboard inputs built above.
                unsafe { input.Anonymous.ki.dwFlags.contains(KEYEVENTF_KEYUP) }
            })
            .collect();
        // SAFETY: as above.
        unsafe { SendInput(&release, size_of::<INPUT>() as i32) };
    }
    sent == inputs.len()
}

fn send_after(after: PasteAfter) {
    if !send_inputs(&after_inputs(after)) {
        // Insertion already succeeded; reporting a failed paste could cause a duplicate retry.
        tracing::warn!("post-insert key could not be delivered");
    }
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
    fn copy_neutralizes_and_restores_held_shortcut_modifiers_in_one_batch() {
        let held = [VK_RCONTROL, VK_RSHIFT];
        let copy = copy_inputs(&held);
        assert_eq!(
            copy.iter().map(input_signature).collect::<Vec<_>>(),
            vec![
                (VK_RCONTROL.0, 0, KEYEVENTF_KEYUP.0),
                (VK_RSHIFT.0, 0, KEYEVENTF_KEYUP.0),
                (VK_CONTROL.0, 0, 0),
                (VK_C.0, 0, 0),
                (VK_C.0, 0, KEYEVENTF_KEYUP.0),
                (VK_CONTROL.0, 0, KEYEVENTF_KEYUP.0),
                (VK_RCONTROL.0, 0, 0),
                (VK_RSHIFT.0, 0, 0),
            ]
        );
    }

    fn input_signature(input: &INPUT) -> (u16, u16, u32) {
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        // SAFETY: the builder only creates keyboard inputs.
        let key = unsafe { input.Anonymous.ki };
        (key.wVk.0, key.wScan, key.dwFlags.0)
    }

    #[test]
    fn unicode_typing_preserves_utf16_and_pairs_every_down_with_up() {
        let chunks = typing_inputs("Ağ😀");
        assert_eq!(chunks.len(), 1);
        let got: Vec<_> = chunks[0].iter().map(input_signature).collect();
        let expected: Vec<_> = "Ağ😀"
            .encode_utf16()
            .flat_map(|unit| [(0, unit, KEYEVENTF_UNICODE.0), (0, unit, (KEYEVENTF_UNICODE | KEYEVENTF_KEYUP).0)])
            .collect();
        assert_eq!(got, expected);
        assert!(typing_inputs("").is_empty());
    }

    #[test]
    fn typing_newlines_are_enter_and_crlf_is_one_enter() {
        let chunks = typing_inputs("\r\n\n\r");
        let expected: Vec<_> =
            (0..3).flat_map(|_| [(VK_RETURN.0, 0, 0), (VK_RETURN.0, 0, KEYEVENTF_KEYUP.0)]).collect();
        assert_eq!(chunks.iter().flatten().map(input_signature).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn typing_chunks_never_split_surrogate_pairs_or_key_pairs() {
        let text = format!("{}😀{}\r\n😀", "a".repeat(TYPING_CHUNK_INPUTS / 2 - 1), "b".repeat(100));
        let chunks = typing_inputs(&text);
        assert!(chunks.len() > 2);
        assert_eq!(chunks[0].len(), TYPING_CHUNK_INPUTS - 2);
        for chunk in &chunks {
            assert!(chunk.len() <= TYPING_CHUNK_INPUTS);
            let mut units = Vec::new();
            let (pairs, remainder) = chunk.as_chunks::<2>();
            assert!(remainder.is_empty(), "a key pair crossed a chunk boundary");
            for pair in pairs {
                let (vk, unit, flags) = input_signature(&pair[0]);
                assert_eq!(input_signature(&pair[1]), (vk, unit, flags | KEYEVENTF_KEYUP.0));
                if flags == KEYEVENTF_UNICODE.0 {
                    units.push(unit);
                }
            }
            assert!(String::from_utf16(&units).is_ok(), "a surrogate pair crossed a chunk boundary");
        }
        let units: Vec<_> = chunks
            .iter()
            .flatten()
            .step_by(2)
            .map(|input| {
                let (vk, unit, _) = input_signature(input);
                if vk == VK_RETURN.0 { b'\n' as u16 } else { unit }
            })
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), text.replace("\r\n", "\n"));
    }

    #[test]
    fn clipboard_shortcuts_and_after_keys_have_balanced_ordered_inputs() {
        for (method, keys) in [
            (PasteMethod::CtrlV, vec![VK_CONTROL, VK_V]),
            (PasteMethod::ShiftInsert, vec![VK_SHIFT, VK_INSERT]),
            (PasteMethod::CtrlShiftV, vec![VK_CONTROL, VK_SHIFT, VK_V]),
        ] {
            let expected: Vec<_> = keys
                .iter()
                .map(|key| (key.0, 0, 0))
                .chain(keys.iter().rev().map(|key| (key.0, 0, KEYEVENTF_KEYUP.0)))
                .collect();
            assert_eq!(paste_inputs(method).iter().map(input_signature).collect::<Vec<_>>(), expected);
        }
        assert!(after_inputs(PasteAfter::None).is_empty());
        assert_eq!(
            after_inputs(PasteAfter::Enter).iter().map(input_signature).collect::<Vec<_>>(),
            vec![(VK_RETURN.0, 0, 0), (VK_RETURN.0, 0, KEYEVENTF_KEYUP.0)]
        );
        assert_eq!(
            after_inputs(PasteAfter::CtrlEnter).iter().map(input_signature).collect::<Vec<_>>(),
            vec![
                (VK_CONTROL.0, 0, 0),
                (VK_RETURN.0, 0, 0),
                (VK_RETURN.0, 0, KEYEVENTF_KEYUP.0),
                (VK_CONTROL.0, 0, KEYEVENTF_KEYUP.0)
            ]
        );
    }

    #[test]
    fn ctrl_v_inputs_are_ctrl_down_v_down_v_up_ctrl_up() {
        let inputs = paste_inputs(PasteMethod::CtrlV);
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
        assert!(input_blocked(Some(true), true));
        assert!(!input_blocked(Some(false), true));
        assert!(input_blocked(None, true));
        assert!(!input_blocked(None, false));
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
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    }

    /// Verifies replacement and retention on the actual Windows clipboard without sending keys.
    #[test]
    #[ignore = "touches the real clipboard"]
    fn latest_dictation_stays_on_clipboard_smoke() {
        struct RestoreUserClipboard(Snapshot);
        impl Drop for RestoreUserClipboard {
            fn drop(&mut self) {
                if let Ok(clipboard) = Clipboard::open() {
                    let _ = clipboard.restore(&self.0);
                }
            }
        }
        let _restore = RestoreUserClipboard(Clipboard::open().unwrap().snapshot());
        for text in ["first dictation", "latest dictation ğüş"] {
            let (backup, _) = put_text(text, false).unwrap();
            assert!(backup.is_none());
            thread::sleep(CLIPBOARD_RESTORE_DELAY + Duration::from_millis(100));
            assert_eq!(text_of(&read(CF_UNICODETEXT).unwrap()), text);
        }
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
