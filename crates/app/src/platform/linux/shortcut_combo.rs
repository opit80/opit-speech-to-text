//! Pure Linux shortcut normalization and desktop accelerator formats.

use crate::platform::HotkeyError;

fn keysym_name(key: &str) -> String {
    let named = match key.trim().to_ascii_lowercase().as_str() {
        "space" => "space",
        "enter" => "Return",
        "escape" => "Escape",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "capslock" => "Caps_Lock",
        "scrolllock" => "Scroll_Lock",
        "numlock" => "Num_Lock",
        "pause" => "Pause",
        "printscreen" => "Print",
        "contextmenu" => "Menu",
        "numpad0" => "KP_0",
        "numpad1" => "KP_1",
        "numpad2" => "KP_2",
        "numpad3" => "KP_3",
        "numpad4" => "KP_4",
        "numpad5" => "KP_5",
        "numpad6" => "KP_6",
        "numpad7" => "KP_7",
        "numpad8" => "KP_8",
        "numpad9" => "KP_9",
        "numpadadd" => "KP_Add",
        "numpadsubtract" => "KP_Subtract",
        "numpadmultiply" => "KP_Multiply",
        "numpaddivide" => "KP_Divide",
        "numpaddecimal" => "KP_Decimal",
        "backquote" => "grave",
        "minus" => "minus",
        "equal" => "equal",
        "bracketleft" => "bracketleft",
        "bracketright" => "bracketright",
        "backslash" | "intlbackslash" => "backslash",
        "semicolon" => "semicolon",
        "quote" => "apostrophe",
        "comma" => "comma",
        "period" => "period",
        "slash" => "slash",
        "audiovolumemute" => "XF86AudioMute",
        "audiovolumeup" => "XF86AudioRaiseVolume",
        "audiovolumedown" => "XF86AudioLowerVolume",
        "mediaplaypause" => "XF86AudioPlay",
        "mediastop" => "XF86AudioStop",
        "mediatracknext" => "XF86AudioNext",
        "mediatrackprevious" => "XF86AudioPrev",
        _ => {
            return if key.trim().len() == 1 {
                key.trim().to_ascii_lowercase()
            } else {
                key.trim().to_ascii_uppercase()
            };
        }
    };
    named.to_owned()
}

const MODIFIERS: [(&str, &str, i32); 4] = [
    ("Ctrl", "Control", 0x0400_0000),
    ("Shift", "Shift", 0x0200_0000),
    ("Alt", "Alt", 0x0800_0000),
    ("Meta", "Super", 0x1000_0000),
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Combo {
    modifiers: i32,
    key: String,
}

impl Combo {
    pub fn new(keys: &[String]) -> Result<Self, HotkeyError> {
        crate::platform::keys::parse_combo(keys)?;
        let mut modifiers = 0;
        let mut ordinary = Vec::new();
        for key in keys {
            match key.trim().to_ascii_lowercase().as_str() {
                "leftctrl" | "rightctrl" => modifiers |= MODIFIERS[0].2,
                "leftshift" | "rightshift" => modifiers |= MODIFIERS[1].2,
                "leftalt" | "rightalt" => modifiers |= MODIFIERS[2].2,
                "leftwin" | "rightwin" => modifiers |= MODIFIERS[3].2,
                _ => ordinary.push(key.trim().to_owned()),
            }
        }
        if ordinary.len() != 1 {
            return Err(HotkeyError::Install("Linux shortcuts need exactly one non-modifier key".into()));
        }
        let key = ordinary.remove(0);
        if modifiers == 0
            && matches!(
                crate::platform::keys::vk_for(&key),
                Some(
                    0x08
                    | 0x09
                    | 0x0D
                    | 0x14
                    | 0x20..=0x28
                    | 0x2D..=0x39
                    | 0x41..=0x5A
                    | 0x60..=0x6F
                    | 0x90
                    | 0xBA..=0xC0
                    | 0xDB..=0xDE
                    | 0xE2,
                )
            )
        {
            return Err(HotkeyError::Install("Typing keys need Ctrl, Shift, Alt or Super on Linux".into()));
        }
        Ok(Self { modifiers, key })
    }

    pub fn gnome(&self) -> String {
        let mods = MODIFIERS
            .iter()
            .filter(|(_, _, mask)| self.modifiers & mask != 0)
            .map(|(_, name, _)| format!("<{name}>"))
            .collect::<String>();
        format!("{mods}{}", keysym_name(&self.key))
    }

    pub fn qt(&self) -> i32 {
        self.modifiers | qt_key(&self.key)
    }

    pub fn kde(&self) -> String {
        qt_text(self.qt())
    }
}

fn qt_key(key: &str) -> i32 {
    let lower = key.to_ascii_lowercase();
    if lower.len() == 1 {
        return i32::from(lower.as_bytes()[0].to_ascii_uppercase());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<i32>().ok()) {
        return 0x0100_002f + n;
    }
    if let Some(n) = lower.strip_prefix("numpad").and_then(|n| n.parse::<i32>().ok()) {
        return 0x2000_0030 + n;
    }
    match lower.as_str() {
        "space" => 0x20,
        "escape" => 0x0100_0000,
        "tab" => 0x0100_0001,
        "backspace" => 0x0100_0003,
        "enter" => 0x0100_0004,
        "insert" => 0x0100_0006,
        "delete" => 0x0100_0007,
        "pause" => 0x0100_0008,
        "printscreen" => 0x0100_0009,
        "home" => 0x0100_0010,
        "end" => 0x0100_0011,
        "arrowleft" => 0x0100_0012,
        "arrowup" => 0x0100_0013,
        "arrowright" => 0x0100_0014,
        "arrowdown" => 0x0100_0015,
        "pageup" => 0x0100_0016,
        "pagedown" => 0x0100_0017,
        "capslock" => 0x0100_0024,
        "numlock" => 0x0100_0025,
        "scrolllock" => 0x0100_0026,
        "contextmenu" => 0x0100_0055,
        "audiovolumedown" => 0x0100_0070,
        "audiovolumemute" => 0x0100_0071,
        "audiovolumeup" => 0x0100_0072,
        "mediaplaypause" => 0x0100_0086,
        "mediastop" => 0x0100_0081,
        "mediatrackprevious" => 0x0100_0082,
        "mediatracknext" => 0x0100_0083,
        "numpadadd" => 0x2000_002b,
        "numpadsubtract" => 0x2000_002d,
        "numpadmultiply" => 0x2000_002a,
        "numpaddivide" => 0x2000_002f,
        "numpaddecimal" => 0x2000_002e,
        "backquote" => 0x60,
        "minus" => 0x2d,
        "equal" => 0x3d,
        "bracketleft" => 0x5b,
        "bracketright" => 0x5d,
        "backslash" | "intlbackslash" => 0x5c,
        "semicolon" => 0x3b,
        "quote" => 0x27,
        "comma" => 0x2c,
        "period" => 0x2e,
        "slash" => 0x2f,
        _ => unreachable!("Combo::new validates every key name"),
    }
}

pub(super) fn qt_text(code: i32) -> String {
    let mut parts = MODIFIERS
        .iter()
        .filter(|(_, _, mask)| code & mask != 0)
        .map(|(name, _, _)| (*name).to_owned())
        .collect::<Vec<_>>();
    if code & 0x2000_0000 != 0 {
        parts.push("Num".into());
    }
    let key = code & 0x01ff_ffff;
    let name = match key {
        0x0100_0000 => "Esc",
        0x0100_0001 => "Tab",
        0x0100_0003 => "Backspace",
        0x0100_0004 => "Return",
        0x0100_0006 => "Ins",
        0x0100_0007 => "Del",
        0x0100_0008 => "Pause",
        0x0100_0009 => "Print",
        0x0100_0010 => "Home",
        0x0100_0011 => "End",
        0x0100_0012 => "Left",
        0x0100_0013 => "Up",
        0x0100_0014 => "Right",
        0x0100_0015 => "Down",
        0x0100_0016 => "PgUp",
        0x0100_0017 => "PgDown",
        0x0100_0024 => "CapsLock",
        0x0100_0025 => "NumLock",
        0x0100_0026 => "ScrollLock",
        0x0100_0055 => "Menu",
        0x0100_0070 => "Volume Down",
        0x0100_0071 => "Volume Mute",
        0x0100_0072 => "Volume Up",
        0x0100_0086 => "Toggle Media Play/Pause",
        0x0100_0081 => "Media Stop",
        0x0100_0082 => "Media Previous",
        0x0100_0083 => "Media Next",
        0x20 => "Space",
        0x0100_0030..=0x0100_0047 => {
            parts.push(format!("F{}", key - 0x0100_002f));
            return parts.join("+");
        }
        _ => {
            parts.push(char::from_u32(key as u32).map_or_else(|| format!("0x{key:x}"), |c| c.to_string()));
            return parts.join("+");
        }
    };
    parts.push(name.into());
    parts.join("+")
}

/// Normalize modifier sides for desktop collision detection without modifying saved config.
pub(crate) fn validate_list(shortcuts: &[&[String]]) -> Result<(), HotkeyError> {
    let mut seen = Vec::new();
    for keys in shortcuts.iter().filter(|keys| !keys.is_empty()) {
        let combo = Combo::new(keys)?.qt();
        if seen.contains(&combo) {
            return Err(HotkeyError::Collision);
        }
        seen.push(combo);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn combo(keys: &[&str]) -> Result<Combo, HotkeyError> {
        Combo::new(&keys.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn desktop_formats_share_canonical_modifier_order() {
        let c = combo(&["RightShift", "LeftCtrl", "F9"]).unwrap();
        assert_eq!(c.kde(), "Ctrl+Shift+F9");
        assert_eq!(c.gnome(), "<Control><Shift>F9");
        assert_eq!(c.qt(), 0x0600_0038 + 0x0100_0000);
        let c = combo(&["LeftWin", "RightAlt", "Space"]).unwrap();
        assert_eq!(c.kde(), "Alt+Meta+Space");
        assert_eq!(c.gnome(), "<Alt><Super>space");
        assert_eq!(combo(&["LeftCtrl", "A"]).unwrap().gnome(), "<Control>a");
    }
    #[test]
    fn refuses_typing_keys_unknown_names_and_modifier_only_chords() {
        for keys in [&["A"][..], &["Space"], &["RightCtrl", "RightShift"], &["Ctrl", "A"], &["F9", "F10"]] {
            assert!(combo(keys).is_err());
        }
        assert!(combo(&["F9"]).is_ok());
        assert!(combo(&["LeftCtrl", "Space"]).is_ok());
    }
    #[test]
    fn side_normalization_detects_collisions_without_rewriting_config() {
        let a = vec!["LeftCtrl".into(), "F9".into()];
        let b = vec!["RightCtrl".into(), "F9".into()];
        assert_eq!(validate_list(&[&a, &b]), Err(HotkeyError::Collision));
        assert_eq!(a[0], "LeftCtrl");
    }
    #[test]
    fn every_supported_name_has_desktop_formats() {
        for key in [
            "F24",
            "Numpad0",
            "NumpadAdd",
            "Enter",
            "ArrowLeft",
            "Backquote",
            "MediaPlayPause",
            "AudioVolumeUp",
            "PageDown",
        ] {
            let c = combo(&["LeftCtrl", key]).unwrap();
            assert!(!c.kde().is_empty());
            assert!(!c.gnome().is_empty());
        }
    }
}
