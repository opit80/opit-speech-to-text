//! Global hotkey via a `WH_KEYBOARD_LL` hook running on a dedicated message-loop thread.
//!
//! A low-level hook is process-global and its callback carries no user data, so the hook
//! state lives in statics and only one [`WinHotkey`] may own the hook at a time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};
use windows::core::PCWSTR;

use super::call_guarded;
use crate::platform::keys::{KeyTracker, VK_ESCAPE, parse_combo};
use crate::platform::{Hotkey, HotkeyError, HotkeySink};

struct HookState {
    tracker: KeyTracker,
    sink: HotkeySink,
}

/// Tracker + sink read by the hook callback; swapped by `register`.
static STATE: Mutex<Option<HookState>> = Mutex::new(None);
static PAUSED: AtomicBool = AtomicBool::new(false);
static CAPTURE_ESC: AtomicBool = AtomicBool::new(false);
/// The last Esc key-down was swallowed, so its key-up must be swallowed too.
static ESC_SWALLOWED: AtomicBool = AtomicBool::new(false);
/// Some `WinHotkey` has installed the hook.
static OWNED: AtomicBool = AtomicBool::new(false);

fn state() -> MutexGuard<'static, Option<HookState>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

struct HookThread {
    thread_id: u32,
    handle: JoinHandle<()>,
}

/// Global hotkey backed by a low-level keyboard hook.
///
/// The sink is called on the hook thread while Windows waits for the hook to return (the
/// system silently removes hooks slower than ~1 s), so it must never block: pass something
/// like a non-blocking channel send (`UnboundedSender::send`, `try_send`).
#[derive(Default)]
pub struct WinHotkey {
    thread: Mutex<Option<HookThread>>,
}

impl WinHotkey {
    /// Creates an idle hotkey; the hook is installed by the first `register`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes the hook and stops its thread. Idempotent; also run on drop.
    pub fn shutdown(&self) {
        let Some(hook) = self.thread.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            return;
        };
        // SAFETY: plain FFI call; the thread created its message queue before reporting its id.
        let posted = unsafe { PostThreadMessageW(hook.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if posted.is_ok() {
            let _ = hook.handle.join();
        }
        *state() = None;
        PAUSED.store(false, Ordering::Relaxed);
        CAPTURE_ESC.store(false, Ordering::Relaxed);
        ESC_SWALLOWED.store(false, Ordering::Relaxed);
        OWNED.store(false, Ordering::Release);
    }
}

impl Drop for WinHotkey {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Hotkey for WinHotkey {
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError> {
        let combo = parse_combo(keys)?;
        let mut thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
        let first = thread.is_none();
        if first && OWNED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return Err(HotkeyError::Install("another WinHotkey already owns the keyboard hook".into()));
        }
        {
            let mut st = state();
            match st.as_mut() {
                Some(s) => {
                    s.tracker.retarget(combo);
                    s.sink = sink;
                }
                None => *st = Some(HookState { tracker: KeyTracker::new(combo), sink }),
            }
        }
        if first {
            match spawn_hook_thread() {
                Ok(t) => *thread = Some(t),
                Err(e) => {
                    *state() = None;
                    OWNED.store(false, Ordering::Release);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        PAUSED.store(paused, Ordering::Relaxed);
    }

    fn set_capture_escape(&self, on: bool) {
        CAPTURE_ESC.store(on, Ordering::Relaxed);
    }
}

fn spawn_hook_thread() -> Result<HookThread, HotkeyError> {
    let (tx, rx) = mpsc::sync_channel(1);
    let handle = thread::Builder::new()
        .name("opit-hotkey".into())
        .spawn(move || hook_thread(tx))
        .map_err(|e| HotkeyError::Install(e.to_string()))?;
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(HookThread { thread_id, handle }),
        Ok(Err(msg)) => {
            let _ = handle.join();
            Err(HotkeyError::Install(msg))
        }
        Err(_) => {
            let _ = handle.join();
            Err(HotkeyError::Install("the hook thread exited unexpectedly".into()))
        }
    }
}

/// Installs the hook and pumps messages (hook callbacks are dispatched from GetMessageW)
/// until WM_QUIT.
fn hook_thread(ready: SyncSender<Result<u32, String>>) {
    // SAFETY: FFI calls with valid arguments; `msg` outlives every call using it.
    unsafe {
        let mut msg = MSG::default();
        // Create this thread's message queue so a WM_QUIT posted right after start is not lost.
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
        let module = GetModuleHandleW(PCWSTR::null()).ok().map(HINSTANCE::from);
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(e.message()));
                return;
            }
        };
        let _ = ready.send(Ok(GetCurrentThreadId()));
        // GetMessageW returns -1 on error, 0 on WM_QUIT.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT valid for this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        // Injected input (including our own SendInput Ctrl+V) never counts.
        if let Some(down) = down
            && !info.flags.contains(LLKHF_INJECTED)
            && handle_key(info.vkCode, down)
        {
            return LRESULT(1);
        }
    }
    // SAFETY: forwarding the unchanged arguments; the hook handle parameter is ignored.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn key_is_down(vk: u32) -> bool {
    // SAFETY: plain FFI call. The high bit is set while the key is physically held.
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// Updates the tracker, delivers events, and returns true when the key must be swallowed.
fn handle_key(vk: u32, down: bool) -> bool {
    let (events, sink) = {
        let mut guard = state();
        let Some(st) = guard.as_mut() else {
            return false;
        };
        // Drop keys whose key-up we missed (e.g. released on the secure desktop) so they
        // cannot block the combo forever. The current key's async state is not updated yet.
        let pruned =
            if down && !st.tracker.held().is_empty() { st.tracker.prune(|k| k == vk || key_is_down(k)) } else { None };
        ([pruned, st.tracker.on_key(vk, down)], st.sink.clone())
    };
    let paused = PAUSED.load(Ordering::Relaxed);
    if !paused {
        for event in events.into_iter().flatten() {
            call_guarded(|| sink(event));
        }
    }
    if vk != VK_ESCAPE {
        return false;
    }
    if down {
        let swallow = !paused && CAPTURE_ESC.load(Ordering::Relaxed);
        if swallow {
            ESC_SWALLOWED.store(true, Ordering::Relaxed);
        }
        swallow
    } else {
        ESC_SWALLOWED.swap(false, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::platform::HotkeyEvent;

    /// The hook is process-global; tests touching it must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    fn collecting_sink() -> (HotkeySink, Arc<Mutex<Vec<HotkeyEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        (Arc::new(move |e| seen2.lock().unwrap().push(e)), seen)
    }

    #[test]
    fn install_retarget_and_shutdown() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, _seen) = collecting_sink();
        hk.register(&names(&["RightCtrl", "RightShift"]), sink.clone()).expect("install hook");
        thread::sleep(Duration::from_millis(200));
        hk.register(&names(&["F13"]), sink.clone()).expect("retarget");
        assert_eq!(state().as_ref().map(|s| s.tracker.combo().to_vec()), Some(vec![0x7C]));
        hk.set_paused(true);
        hk.set_capture_escape(true);
        hk.shutdown();
        assert!(state().is_none());
        assert!(!OWNED.load(Ordering::Relaxed));
        // A fresh instance can install again after shutdown.
        let hk2 = WinHotkey::new();
        hk2.register(&names(&["F13"]), sink).expect("reinstall");
        drop(hk2);
    }

    #[test]
    fn bad_keys_fail_without_installing() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, _) = collecting_sink();
        assert_eq!(hk.register(&[], sink.clone()), Err(HotkeyError::Empty));
        assert_eq!(hk.register(&names(&["Nope"]), sink), Err(HotkeyError::UnknownKey("Nope".into())));
        assert!(hk.thread.lock().unwrap().is_none());
        assert!(!OWNED.load(Ordering::Relaxed));
    }

    #[test]
    fn second_owner_is_rejected() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let a = WinHotkey::new();
        let b = WinHotkey::new();
        let (sink, _) = collecting_sink();
        a.register(&names(&["F13"]), sink.clone()).unwrap();
        assert!(matches!(b.register(&names(&["F14"]), sink), Err(HotkeyError::Install(_))));
        drop(b);
        assert!(OWNED.load(Ordering::Relaxed), "dropping the rejected instance must not release the hook");
    }

    /// Injects F24 down/up through SendInput: the hook must ignore it (LLKHF_INJECTED).
    #[test]
    #[ignore = "injects real keyboard input (F24)"]
    fn injected_keys_are_ignored() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_F24,
        };
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let hk = WinHotkey::new();
        let (sink, seen) = collecting_sink();
        hk.register(&names(&["F24"]), sink).unwrap();
        thread::sleep(Duration::from_millis(100));
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_F24, dwFlags: flags, ..Default::default() } },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        // SAFETY: valid INPUT array and size.
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2);
        thread::sleep(Duration::from_millis(200));
        assert!(seen.lock().unwrap().is_empty(), "injected keys produced {:?}", seen.lock().unwrap());
    }
}
