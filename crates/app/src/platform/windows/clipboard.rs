//! Thin safe wrapper over the Win32 clipboard: open with retry, snapshot/restore every
//! HGLOBAL-backed format, and write text marked as private (kept out of Win+V history and
//! the cloud clipboard).

use std::marker::PhantomData;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{HSTRING, w};

/// Standard clipboard format ids (winuser.h). Defined here to avoid the large `Win32_System_Ole`
/// feature, which is where windows-rs puts `CF_*`.
pub const CF_BITMAP: u32 = 2;
pub const CF_METAFILEPICT: u32 = 3;
pub const CF_PALETTE: u32 = 9;
pub const CF_UNICODETEXT: u32 = 13;
pub const CF_ENHMETAFILE: u32 = 14;
pub const CF_OWNERDISPLAY: u32 = 0x80;
pub const CF_DSPBITMAP: u32 = 0x82;
pub const CF_DSPMETAFILEPICT: u32 = 0x83;
pub const CF_DSPENHMETAFILE: u32 = 0x8E;
/// CF_PRIVATEFIRST..=CF_GDIOBJLAST: app-private handles and GDI objects, never HGLOBALs we may copy.
const PRIVATE_AND_GDI_RANGE: std::ops::RangeInclusive<u32> = 0x200..=0x3FF;

const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(20);

/// Registered format names that control clipboard history / cloud sync.
pub const EXCLUDE_FROM_MONITORING: &str = "ExcludeClipboardContentFromMonitorProcessing";
pub const CAN_INCLUDE_IN_HISTORY: &str = "CanIncludeInClipboardHistory";
pub const CAN_UPLOAD_TO_CLOUD: &str = "CanUploadToCloudClipboard";

/// True when data of `format` is an HGLOBAL that can be copied byte-for-byte.
pub fn is_hglobal_format(format: u32) -> bool {
    !matches!(
        format,
        0 | CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) && !PRIVATE_AND_GDI_RANGE.contains(&format)
}

/// Encodes `text` as NUL-terminated UTF-16 bytes (CF_UNICODETEXT payload).
pub fn utf16_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
}

/// Registers (or looks up) a named clipboard format. None if registration failed.
pub fn register_format(name: &str) -> Option<u32> {
    // SAFETY: HSTRING is NUL-terminated and outlives the call.
    let id = unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) };
    (id != 0).then_some(id)
}

/// Current clipboard sequence number (changes whenever the clipboard content changes).
pub fn sequence_number() -> u32 {
    // SAFETY: plain FFI call.
    unsafe { GetClipboardSequenceNumber() }
}

/// Copy of every HGLOBAL-backed format on the clipboard. Empty means the clipboard was empty
/// (or held only formats we cannot copy).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub formats: Vec<(u32, Vec<u8>)>,
}

/// An open clipboard; closed on drop. Not `Send`: the clipboard is opened per thread.
///
/// Owned by a throwaway message-only window rather than NULL: with `OpenClipboard(NULL)`
/// Windows can report success while the clipboard is not actually open for us (EmptyClipboard
/// then fails with ERROR_CLIPBOARD_NOT_OPEN), which was reproducible here in 20/40 cycles.
pub struct Clipboard {
    owner: HWND,
    _not_send: PhantomData<*const ()>,
}

impl Clipboard {
    /// Opens the clipboard, retrying (10 × 20 ms) while another app holds it.
    pub fn open() -> Result<Self, String> {
        // SAFETY: creates a message-only STATIC window on this thread; destroyed on every path.
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|e| format!("could not create the clipboard owner window: {}", e.message()))?;
        let mut last = String::new();
        for attempt in 0..OPEN_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(OPEN_RETRY_DELAY);
            }
            // SAFETY: plain FFI call; paired with CloseClipboard in Drop.
            match unsafe { OpenClipboard(Some(owner)) } {
                Ok(()) => return Ok(Self { owner, _not_send: PhantomData }),
                Err(e) => last = e.message(),
            }
        }
        // SAFETY: our own window, created above on this thread.
        let _ = unsafe { DestroyWindow(owner) };
        Err(format!("the clipboard is busy: {last}"))
    }

    /// Copies all HGLOBAL formats. Formats that fail to render or lock are skipped.
    pub fn snapshot(&self) -> Snapshot {
        let mut formats = Vec::new();
        let mut format = 0;
        loop {
            // SAFETY: the clipboard is open on this thread.
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if !is_hglobal_format(format) {
                continue;
            }
            // SAFETY: the clipboard is open; the handle is owned by the clipboard.
            if let Ok(handle) = unsafe { GetClipboardData(format) }
                && let Some(bytes) = unsafe { copy_hglobal(HGLOBAL(handle.0)) }
            {
                formats.push((format, bytes));
            }
        }
        Snapshot { formats }
    }

    /// Empties the clipboard and takes ownership of it.
    pub fn empty(&self) -> Result<(), String> {
        // SAFETY: the clipboard is open on this thread.
        unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard failed: {}", e.message()))
    }

    /// Puts `bytes` on the clipboard as `format`. Call after `empty`.
    pub fn set_bytes(&self, format: u32, bytes: &[u8]) -> Result<(), String> {
        // SAFETY: we allocate a movable global, fill it while locked, and hand it to the
        // clipboard; on failure the global is still ours and is freed.
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| e.message())?;
            let ptr = GlobalLock(mem);
            if ptr.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(mem);
            if let Err(e) = SetClipboardData(format, Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(format!("SetClipboardData({format}) failed: {}", e.message()));
            }
        }
        Ok(())
    }

    /// Adds the formats that keep the current content out of clipboard history, Win+V and
    /// the cloud clipboard. Best effort: failures are ignored.
    pub fn mark_private(&self) {
        let zero = 0u32.to_le_bytes();
        for (name, data) in [
            (EXCLUDE_FROM_MONITORING, &zero[..]),
            (CAN_INCLUDE_IN_HISTORY, &zero[..]),
            (CAN_UPLOAD_TO_CLOUD, &zero[..]),
        ] {
            if let Some(id) = register_format(name) {
                let _ = self.set_bytes(id, data);
            }
        }
    }

    /// Replaces the clipboard with `text` (CF_UNICODETEXT), marked private.
    pub fn set_private_text(&self, text: &str) -> Result<(), String> {
        self.empty()?;
        self.set_bytes(CF_UNICODETEXT, &utf16_bytes(text))?;
        self.mark_private();
        Ok(())
    }

    /// Replaces the clipboard with `snapshot` (an empty snapshot leaves it empty). The restore
    /// itself is marked private so it does not add a duplicate Win+V history entry.
    pub fn restore(&self, snapshot: &Snapshot) -> Result<(), String> {
        self.empty()?;
        if snapshot.formats.is_empty() {
            return Ok(());
        }
        let privacy: Vec<u32> = [EXCLUDE_FROM_MONITORING, CAN_INCLUDE_IN_HISTORY, CAN_UPLOAD_TO_CLOUD]
            .iter()
            .filter_map(|n| register_format(n))
            .collect();
        let mut first_error = None;
        for (format, bytes) in snapshot.formats.iter().filter(|(f, _)| !privacy.contains(f)) {
            if let Err(e) = self.set_bytes(*format, bytes) {
                first_error.get_or_insert(e);
            }
        }
        self.mark_private();
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: we opened the clipboard in `open` with `owner`, a window of this thread.
        // Destroying the owner keeps non-delayed data and does not bump the sequence number.
        unsafe {
            let _ = CloseClipboard();
            let _ = DestroyWindow(self.owner);
        }
    }
}

/// Copies the bytes of a clipboard-owned HGLOBAL. None if it is not a lockable global.
///
/// # Safety
/// `mem` must come from `GetClipboardData` while the clipboard is open.
unsafe fn copy_hglobal(mem: HGLOBAL) -> Option<Vec<u8>> {
    // SAFETY: guaranteed by the caller; GlobalSize/GlobalLock fail cleanly on non-globals.
    unsafe {
        let size = GlobalSize(mem);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(mem);
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_filter_skips_gdi_and_private_handles() {
        for f in [1, 7, 8, 13, 15, 16, 17, 0xC000, 0xC123, 0xFFFF] {
            assert!(is_hglobal_format(f), "{f:#x} should be copied");
        }
        for f in [0, 2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E, 0x200, 0x2FF, 0x300, 0x3FF] {
            assert!(!is_hglobal_format(f), "{f:#x} should be skipped");
        }
    }

    #[test]
    fn utf16_bytes_is_nul_terminated_le() {
        assert_eq!(utf16_bytes(""), vec![0, 0]);
        assert_eq!(utf16_bytes("Aş"), vec![0x41, 0, 0x5F, 0x01, 0, 0]);
    }

    #[test]
    fn register_format_is_stable() {
        let a = register_format("OpitTestFormat").unwrap();
        assert!(a >= 0xC000);
        assert_eq!(register_format("OpitTestFormat"), Some(a));
    }
}
