//! X11-only backends. Never connect through XWayland to identify a Wayland foreground app.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt, GrabMode, ModMask, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::platform::{ForegroundApp, ForegroundInfo, Hotkey, HotkeyError, HotkeyEvent, HotkeySink};

/// A connection probe only; call from a blocking worker, never during GTK dispatch.
pub fn available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none() && x11rb::connect(None).is_ok()
}

fn install_error() -> HotkeyError {
    HotkeyError::Install("X11 shortcut registration failed or the shortcut is already in use".into())
}

fn keysym(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.len() == 1 && lower.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(u32::from(lower.as_bytes()[0]));
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|s| s.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)) {
        return Some(0xffbd + n);
    }
    if let Some(n) = lower.strip_prefix("numpad").and_then(|s| s.parse::<u32>().ok()).filter(|n| *n <= 9) {
        return Some(0xffb0 + n);
    }
    Some(match lower.as_str() {
        "space" => 0x20,
        "leftctrl" => 0xffe3,
        "rightctrl" => 0xffe4,
        "leftshift" => 0xffe1,
        "rightshift" => 0xffe2,
        "leftalt" => 0xffe9,
        "rightalt" => 0xffea,
        "leftwin" => 0xffeb,
        "rightwin" => 0xffec,
        "escape" => 0xff1b,
        "enter" => 0xff0d,
        "tab" => 0xff09,
        "backspace" => 0xff08,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "arrowleft" => 0xff51,
        "arrowright" => 0xff53,
        "arrowup" => 0xff52,
        "arrowdown" => 0xff54,
        "capslock" => 0xffe5,
        "scrolllock" => 0xff14,
        "numlock" => 0xff7f,
        "pause" => 0xff13,
        "printscreen" => 0xff61,
        "contextmenu" => 0xff67,
        "numpadmultiply" => 0xffaa,
        "numpadadd" => 0xffab,
        "numpadsubtract" => 0xffad,
        "numpaddecimal" => 0xffae,
        "numpaddivide" => 0xffaf,
        "semicolon" => b';' as u32,
        "equal" => b'=' as u32,
        "comma" => b',' as u32,
        "minus" => b'-' as u32,
        "period" => b'.' as u32,
        "slash" => b'/' as u32,
        "backquote" => b'`' as u32,
        "bracketleft" => b'[' as u32,
        "backslash" | "intlbackslash" => b'\\' as u32,
        "bracketright" => b']' as u32,
        "quote" => b'\'' as u32,
        "audiovolumemute" => 0x1008ff12,
        "audiovolumedown" => 0x1008ff11,
        "audiovolumeup" => 0x1008ff13,
        "mediatracknext" => 0x1008ff17,
        "mediatrackprevious" => 0x1008ff16,
        "mediastop" => 0x1008ff15,
        "mediaplaypause" => 0x1008ff14,
        _ => return None,
    })
}

fn modifier(name: &str) -> Option<ModMask> {
    match name.trim().to_ascii_lowercase().as_str() {
        "leftctrl" | "rightctrl" => Some(ModMask::CONTROL),
        "leftshift" | "rightshift" => Some(ModMask::SHIFT),
        "leftalt" | "rightalt" => Some(ModMask::M1),
        "leftwin" | "rightwin" => Some(ModMask::M4),
        _ => None,
    }
}

fn combo(keys: &[String]) -> Result<(u32, ModMask), HotkeyError> {
    super::shortcut_combo::Combo::new(keys)?;
    let mut mods = ModMask::default();
    let mut ordinary = None;
    for key in keys {
        if let Some(mask) = modifier(key) {
            mods |= mask;
        } else if ordinary.replace(keysym(key).ok_or_else(|| HotkeyError::UnknownKey(key.clone()))?).is_some() {
            return Err(HotkeyError::Install("X11 shortcuts need exactly one non-modifier key".into()));
        }
    }
    ordinary
        .map(|key| (key, mods))
        .ok_or_else(|| HotkeyError::Install("X11 shortcuts need one non-modifier key".into()))
}

struct Keymap {
    first: u8,
    width: usize,
    syms: Vec<u32>,
}

impl Keymap {
    fn read(conn: &RustConnection) -> Option<Self> {
        let setup = conn.setup();
        let reply = conn
            .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
            .ok()?
            .reply()
            .ok()?;
        Some(Self { first: setup.min_keycode, width: usize::from(reply.keysyms_per_keycode), syms: reply.keysyms })
    }

    fn code(&self, sym: u32) -> Option<u8> {
        if self.width == 0 {
            return None;
        }
        self.syms
            .chunks(self.width)
            .position(|syms| syms.contains(&sym))
            .and_then(|index| u8::try_from(index).ok()?.checked_add(self.first))
    }
}

struct Worker {
    stop: mpsc::Sender<()>,
    join: std::thread::JoinHandle<()>,
}

#[derive(Default)]
pub struct X11Hotkey {
    main: Mutex<Option<(Vec<String>, HotkeySink)>>,
    worker: Mutex<Option<Worker>>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    was_running: Arc<AtomicBool>,
}

impl Drop for X11Hotkey {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.get_mut().unwrap().take() {
            let _ = worker.stop.send(());
            let _ = worker.join.join();
        }
    }
}

#[derive(Clone)]
struct Binding {
    sym: u32,
    code: u8,
    mods: ModMask,
    action: HotkeyEvent,
}

fn release_action(action: &HotkeyEvent) -> Option<HotkeyEvent> {
    match action {
        HotkeyEvent::ComboDown => Some(HotkeyEvent::ComboUp),
        HotkeyEvent::Edit => Some(HotkeyEvent::EditUp),
        HotkeyEvent::Translate => Some(HotkeyEvent::TranslateUp),
        _ => None,
    }
}

#[derive(Default)]
struct ActiveBindings(HashMap<u8, HotkeyEvent>);

impl ActiveBindings {
    fn press(&mut self, code: u8, action: HotkeyEvent) -> Option<HotkeyEvent> {
        if self.0.contains_key(&code) {
            return None;
        }
        self.0.insert(code, action.clone());
        Some(action)
    }
    fn release(&mut self, code: u8) -> Option<HotkeyEvent> {
        self.0.remove(&code).and_then(|action| release_action(&action))
    }
}

fn lock_variants(numlock: ModMask) -> [ModMask; 4] {
    [ModMask::default(), ModMask::LOCK, numlock, ModMask::LOCK | numlock]
}

fn grab(conn: &RustConnection, root: Window, binding: &Binding, numlock: ModMask) -> Result<(), HotkeyError> {
    for ignored in lock_variants(numlock) {
        conn.grab_key(false, root, binding.mods | ignored, binding.code, GrabMode::ASYNC, GrabMode::ASYNC)
            .map_err(|_| install_error())?
            .check()
            .map_err(|_| install_error())?;
    }
    Ok(())
}

fn numlock_mask(conn: &RustConnection, map: &Keymap) -> ModMask {
    conn.get_modifier_mapping()
        .ok()
        .and_then(|r| r.reply().ok())
        .and_then(|mapping| {
            let code = map.code(0xff7f)?;
            let width = mapping.keycodes.len() / 8;
            if width == 0 {
                return None;
            }
            mapping.keycodes.chunks(width).position(|keys| keys.contains(&code)).map(|i| ModMask::from(1u16 << i))
        })
        .unwrap_or_default()
}

impl Hotkey for X11Hotkey {
    fn supports_release(&self) -> bool {
        false
    }
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError> {
        combo(keys)?;
        *self.main.lock().unwrap() = Some((keys.to_vec(), sink));
        Ok(())
    }

    fn register_shortcuts(&self, shortcuts: &[(HotkeyEvent, Vec<String>)]) -> Result<(), HotkeyError> {
        let (main, sink) = self.main.lock().unwrap().clone().ok_or(HotkeyError::Empty)?;
        let mut requested = vec![(HotkeyEvent::ComboDown, combo(&main)?)];
        for (action, keys) in shortcuts {
            if !keys.is_empty() {
                requested.push((action.clone(), combo(keys)?));
            }
        }
        let mut current = self.worker.lock().unwrap();
        if let Some(worker) = current.take() {
            let _ = worker.stop.send(());
            let _ = worker.join.join();
        }
        let paused = self.paused.clone();
        let running = self.running.clone();
        let was_running = self.was_running.clone();
        let (stop, stop_rx) = mpsc::channel();
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let join = std::thread::Builder::new().name("opit-x11-shortcuts".into()).spawn(move || {
            let Ok((conn, screen)) = x11rb::connect(None) else { let _ = ready.send(Err(install_error())); return; };
            struct Running(Arc<AtomicBool>);
            impl Drop for Running { fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); } }
            let _running = Running(running.clone());
            // Detectable repeat delivers repeated key-downs only. A physical key-up is never
            // mistaken for the synthetic release half of a core repeat pair across reads.
            let repeat = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
            let extension = conn.xkb_use_extension(1, 0).ok().and_then(|r| r.reply().ok());
            let flags = extension.filter(|r| r.supported).and_then(|_| {
                conn.xkb_per_client_flags(xkb::ID::USE_CORE_KBD.into(), repeat, repeat, xkb::BoolCtrl::default(), xkb::BoolCtrl::default(), xkb::BoolCtrl::default()).ok()?.reply().ok()
            });
            if !flags.is_some_and(|r| (r.value & repeat) == repeat) { let _ = ready.send(Err(install_error())); return; }
            let root = conn.setup().roots[screen].root;
            let Some(map) = Keymap::read(&conn) else { let _ = ready.send(Err(install_error())); return; };
            let mut bindings = Vec::new();
            for (action, (sym, mods)) in requested {
                let Some(code) = map.code(sym) else { let _ = ready.send(Err(install_error())); return; };
                if bindings.iter().any(|b: &Binding| b.code == code && b.mods == mods) {
                    let _ = ready.send(Err(HotkeyError::Collision)); return;
                }
                bindings.push(Binding { sym, code, mods, action });
            }
            let mut numlock = numlock_mask(&conn, &map);
            for binding in &bindings {
                if let Err(error) = grab(&conn, root, binding, numlock) { let _ = ready.send(Err(error)); return; }
            }
            running.store(true, Ordering::SeqCst);
            was_running.store(true, Ordering::SeqCst);
            let _ = ready.send(Ok(()));
            let mut active = ActiveBindings::default();
            let mut pending = VecDeque::new();
            loop {
                if stop_rx.try_recv().is_ok() { break; }
                loop {
                    match conn.poll_for_event() {
                        Ok(Some(event)) => pending.push_back(event),
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
                while let Some(event) = pending.pop_front() {
                    match event {
                        Event::KeyPress(event) => {
                            if active.0.contains_key(&event.detail) || paused.load(Ordering::SeqCst) { continue; }
                            let state = u16::from(event.state) & !(u16::from(ModMask::LOCK | numlock));
                            if let Some(binding) = bindings.iter().find(|b| b.code == event.detail && u16::from(b.mods) == state)
                                && let Some(action) = active.press(event.detail, binding.action.clone()) { sink(action); }
                        }
                        Event::KeyRelease(event) => {
                            // Core X11 auto-repeat is a release/press pair with identical timestamp.
                            if matches!(pending.front(), Some(Event::KeyPress(next)) if next.detail == event.detail && next.time == event.time) {
                                pending.pop_front(); continue;
                            }
                            if let Some(action) = active.release(event.detail)
                                && !paused.load(Ordering::SeqCst) { sink(action); }
                        }
                        Event::MappingNotify(_) => {
                            // Release the action that was actually activated, then resolve the
                            // new keymap and NumLock modifier before reacquiring passive grabs.
                            for (_, action) in active.0.drain() {
                                if let Some(action) = release_action(&action) { sink(action); }
                            }
                            let Some(map) = Keymap::read(&conn) else { return; };
                            let Some(next) = bindings.iter().map(|binding| Some(Binding {
                                sym: binding.sym, code: map.code(binding.sym)?, mods: binding.mods, action: binding.action.clone(),
                            })).collect::<Option<Vec<_>>>() else { return; };
                            if conn.ungrab_key(x11rb::NONE as u8, root, ModMask::ANY).ok().is_none_or(|r| r.check().is_err()) { return; }
                            numlock = numlock_mask(&conn, &map);
                            for binding in &next { if grab(&conn, root, binding, numlock).is_err() { return; } }
                            bindings = next;
                            let _ = conn.flush();
                        }
                        _ => {}
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }).map_err(|_| install_error())?;
        *current = Some(Worker { stop, join });
        ready_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| install_error())?
    }

    fn runtime_error(&self) -> Option<String> {
        (self.was_running.load(Ordering::SeqCst) && !self.running.load(Ordering::SeqCst))
            .then(|| "X11 shortcut connection or re-registration failed; save the shortcut to try again".into())
    }
    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
    fn set_capture_escape(&self, _on: bool) {
        // Linux cancellation is opt-in through cancel_keys, never an implicit Escape grab.
    }
}

fn evdev_keysym(code: u32) -> Option<u32> {
    Some(match code {
        29 => 0xffe3,
        42 => 0xffe1,
        47 => b'v' as u32,
        46 => b'c' as u32,
        28 => 0xff0d,
        110 => 0xff63,
        _ => return None,
    })
}

fn resolve_keys(map: &Keymap, keys: &[(u32, bool)]) -> Option<Vec<(u8, bool)>> {
    let mut held = Vec::new();
    let mut result = Vec::with_capacity(keys.len());
    for (evdev, down) in keys {
        let code = map.code(evdev_keysym(*evdev)?)?;
        if *down {
            if held.contains(&code) {
                return None;
            }
            held.push(code);
        } else {
            let index = held.iter().position(|key| *key == code)?;
            held.remove(index);
        }
        result.push((code, *down));
    }
    (held.is_empty() && !result.is_empty()).then_some(result)
}

/// Send a complete shortcut only after all its keys and the XTest extension were resolved.
/// Does not touch the clipboard; caller serializes this with clipboard operations.
pub fn send_keys(keys: &[(u32, bool)]) -> bool {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        return false;
    };
    if conn.xtest_get_version(2, 2).ok().and_then(|r| r.reply().ok()).is_none() {
        return false;
    }
    let Some(map) = Keymap::read(&conn) else {
        return false;
    };
    let Some(resolved) = resolve_keys(&map, keys) else {
        return false;
    };
    // Never release a physical key held by the user or combine with an unrelated modifier.
    let Some(held) = conn.query_keymap().ok().and_then(|r| r.reply().ok()) else {
        return false;
    };
    if held.keys.iter().any(|byte| *byte != 0) {
        return false;
    }
    let root = conn.setup().roots[screen].root;
    let mut pressed = Vec::new();
    for (code, down) in resolved {
        let sent = conn
            .xtest_fake_input(
                if down { xproto::KEY_PRESS_EVENT } else { xproto::KEY_RELEASE_EVENT },
                code,
                x11rb::CURRENT_TIME,
                root,
                0,
                0,
                0,
            )
            .ok()
            .is_some_and(|cookie| cookie.check().is_ok());
        if !sent {
            for code in pressed.into_iter().rev() {
                let _ = conn.xtest_fake_input(xproto::KEY_RELEASE_EVENT, code, x11rb::CURRENT_TIME, root, 0, 0, 0);
            }
            let _ = conn.flush();
            return false;
        }
        if down {
            pressed.push(code);
        } else {
            pressed.retain(|key| *key != code);
        }
    }
    conn.flush().is_ok()
}

fn atom(conn: &RustConnection, name: &[u8]) -> Option<u32> {
    conn.intern_atom(false, name).ok()?.reply().ok().map(|r| r.atom)
}

fn windows(conn: &RustConnection, root: Window, name: &[u8]) -> Option<Vec<u32>> {
    let reply = conn.get_property(false, root, atom(conn, name)?, AtomEnum::WINDOW, 0, 16384).ok()?.reply().ok()?;
    Some(reply.value32()?.collect())
}

fn foreground_info(conn: &RustConnection, window: Window, include_title: bool) -> Option<ForegroundInfo> {
    let pid = conn
        .get_property(false, window, atom(conn, b"_NET_WM_PID")?, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;
    if pid == std::process::id() {
        return None;
    }
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?.file_name()?.to_str()?.to_owned();
    let title = if include_title {
        let name = atom(conn, b"_NET_WM_NAME")?;
        let utf8 = atom(conn, b"UTF8_STRING")?;
        let reply = conn.get_property(false, window, name, utf8, 0, 4096).ok()?.reply().ok()?;
        String::from_utf8(reply.value).ok()?
    } else {
        String::new()
    };
    Some(ForegroundInfo { exe, title })
}

pub struct X11Foreground;

impl ForegroundApp for X11Foreground {
    fn current(&self, include_title: bool) -> Option<ForegroundInfo> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;
        let window = windows(&conn, root, b"_NET_ACTIVE_WINDOW")?.into_iter().next()?;
        foreground_info(&conn, window, include_title)
    }

    fn list_running(&self) -> Result<Vec<ForegroundInfo>, String> {
        let read = || -> Option<Vec<ForegroundInfo>> {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots[screen].root;
            let clients = windows(&conn, root, b"_NET_CLIENT_LIST")?;
            let mut apps = BTreeMap::new();
            let window_type = atom(&conn, b"_NET_WM_WINDOW_TYPE")?;
            let tool_types: Vec<u32> = [
                b"_NET_WM_WINDOW_TYPE_DESKTOP".as_slice(),
                b"_NET_WM_WINDOW_TYPE_DOCK",
                b"_NET_WM_WINDOW_TYPE_TOOLBAR",
                b"_NET_WM_WINDOW_TYPE_MENU",
                b"_NET_WM_WINDOW_TYPE_SPLASH",
                b"_NET_WM_WINDOW_TYPE_TOOLTIP",
                b"_NET_WM_WINDOW_TYPE_NOTIFICATION",
                b"_NET_WM_WINDOW_TYPE_DND",
            ]
            .iter()
            .filter_map(|name| atom(&conn, name))
            .collect();
            for window in clients {
                let Some(attrs) = conn.get_window_attributes(window).ok().and_then(|r| r.reply().ok()) else {
                    continue;
                };
                if attrs.override_redirect || attrs.map_state != xproto::MapState::VIEWABLE {
                    continue;
                }
                let kinds = conn
                    .get_property(false, window, window_type, AtomEnum::ATOM, 0, 32)
                    .ok()
                    .and_then(|r| r.reply().ok());
                if kinds
                    .as_ref()
                    .and_then(|reply| reply.value32())
                    .is_some_and(|mut values| values.any(|kind| tool_types.contains(&kind)))
                {
                    continue;
                }
                let Some(info) = foreground_info(&conn, window, true) else {
                    continue;
                };
                apps.entry(info.exe.clone()).or_insert(info);
            }
            Some(apps.into_values().collect())
        };
        read().ok_or_else(|| "X11 window discovery is unavailable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn x11_combo_requires_one_ordinary_key_and_detects_shared_modifier_sides() {
        assert_eq!(
            combo(&keys(&["LeftCtrl", "LeftShift", "F9"])).unwrap(),
            (0xffc6, ModMask::CONTROL | ModMask::SHIFT)
        );
        assert_eq!(combo(&keys(&["RightCtrl", "RightShift", "F9"])), combo(&keys(&["LeftCtrl", "LeftShift", "F9"])));
        assert!(combo(&keys(&["LeftCtrl", "RightShift"])).is_err());
        assert!(combo(&keys(&["A", "B"])).is_err());
    }

    #[test]
    fn keycode_resolution_uses_server_mapping_and_rejects_missing_keys() {
        let map = Keymap { first: 8, width: 2, syms: vec![b'a' as u32, b'A' as u32, b'v' as u32, b'V' as u32] };
        assert_eq!(map.code(b'v' as u32), Some(9));
        assert_eq!(map.code(b'V' as u32), Some(9));
        assert_eq!(map.code(b'c' as u32), None);
        assert_eq!(evdev_keysym(46), Some(b'c' as u32));
        assert_eq!(evdev_keysym(0), None);
    }

    #[test]
    fn injection_preflight_rejects_unbalanced_or_missing_keys_before_sending_anything() {
        let map = Keymap { first: 8, width: 1, syms: vec![0xffe3, b'v' as u32] };
        assert_eq!(
            resolve_keys(&map, &[(29, true), (47, true), (47, false), (29, false)]),
            Some(vec![(8, true), (9, true), (9, false), (8, false)])
        );
        assert_eq!(resolve_keys(&map, &[(29, true)]), None);
        assert_eq!(resolve_keys(&map, &[(29, false)]), None);
        assert_eq!(resolve_keys(&map, &[(29, true), (29, true), (29, false)]), None);
        assert_eq!(resolve_keys(&map, &[(46, true), (46, false)]), None);
        assert_eq!(resolve_keys(&map, &[]), None);
    }

    #[test]
    fn repeated_downs_do_not_toggle_or_release_the_wrong_shared_key_binding() {
        let mut active = ActiveBindings::default();
        // Both Ctrl+F9 and Shift+F9 may share one physical keycode. Only the
        // action activated by the initial modifiers may receive the release.
        assert_eq!(active.press(75, HotkeyEvent::ComboDown), Some(HotkeyEvent::ComboDown));
        assert_eq!(active.press(75, HotkeyEvent::Edit), None);
        assert_eq!(active.press(75, HotkeyEvent::ComboDown), None);
        assert_eq!(active.release(75), Some(HotkeyEvent::ComboUp));
        assert_eq!(active.release(75), None);
        assert_eq!(active.press(75, HotkeyEvent::Edit), Some(HotkeyEvent::Edit));
        assert_eq!(active.release(75), Some(HotkeyEvent::EditUp));
    }

    #[test]
    fn lock_variants_and_release_actions_are_explicit() {
        assert_eq!(
            lock_variants(ModMask::M2),
            [ModMask::default(), ModMask::LOCK, ModMask::M2, ModMask::LOCK | ModMask::M2]
        );
        assert_eq!(release_action(&HotkeyEvent::Edit), Some(HotkeyEvent::EditUp));
        assert_eq!(release_action(&HotkeyEvent::Translate), Some(HotkeyEvent::TranslateUp));
        assert_eq!(release_action(&HotkeyEvent::Pause), None);
    }
}
