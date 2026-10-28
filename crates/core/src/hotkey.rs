//! Shared shortcut parsing and collision validation. No platform dependencies.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("unknown key name: {0}")]
    UnknownKey(String),
    #[error("the shortcut has no keys")]
    Empty,
    #[error("the shortcuts overlap")]
    Collision,
    #[error("Escape is reserved for cancelling dictation")]
    PauseEscape,
    #[error("the keyboard hook could not be installed: {0}")]
    Install(String),
}

/// VK_ESCAPE.
pub const VK_ESCAPE: u32 = 0x1B;
/// Generic VK_CONTROL. Low-level hooks normally deliver the left/right codes instead.
pub const VK_CONTROL: u32 = 0x11;
/// VK_LCONTROL.
pub const VK_LCONTROL: u32 = 0xA2;
/// VK_RCONTROL.
pub const VK_RCONTROL: u32 = 0xA3;

/// Maps a key name (case-insensitive, e.g. "RightCtrl", "f13", "a", "0") to the virtual-key
/// code a `WH_KEYBOARD_LL` hook delivers for it. Modifiers are side-specific.
pub fn vk_for(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    let named = match lower.as_str() {
        "leftctrl" => 0xA2,
        "rightctrl" => 0xA3,
        "leftshift" => 0xA0,
        "rightshift" => 0xA1,
        "leftalt" => 0xA4,
        "rightalt" => 0xA5,
        "leftwin" => 0x5B,
        "rightwin" => 0x5C,
        "space" => 0x20,
        "capslock" => 0x14,
        "scrolllock" => 0x91,
        "pause" => 0x13,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "escape" => VK_ESCAPE,
        "tab" => 0x09,
        "enter" => 0x0D,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "arrowleft" => 0x25,
        "arrowup" => 0x26,
        "arrowright" => 0x27,
        "arrowdown" => 0x28,
        "printscreen" => 0x2C,
        "contextmenu" => 0x5D,
        "numlock" => 0x90,
        "numpad0" => 0x60,
        "numpad1" => 0x61,
        "numpad2" => 0x62,
        "numpad3" => 0x63,
        "numpad4" => 0x64,
        "numpad5" => 0x65,
        "numpad6" => 0x66,
        "numpad7" => 0x67,
        "numpad8" => 0x68,
        "numpad9" => 0x69,
        "numpadmultiply" => 0x6A,
        "numpadadd" => 0x6B,
        "numpadsubtract" => 0x6D,
        "numpaddecimal" => 0x6E,
        "numpaddivide" => 0x6F,
        "semicolon" => 0xBA,
        "equal" => 0xBB,
        "comma" => 0xBC,
        "minus" => 0xBD,
        "period" => 0xBE,
        "slash" => 0xBF,
        "backquote" => 0xC0,
        "bracketleft" => 0xDB,
        "backslash" => 0xDC,
        "bracketright" => 0xDD,
        "quote" => 0xDE,
        "intlbackslash" => 0xE2,
        "audiovolumemute" => 0xAD,
        "audiovolumedown" => 0xAE,
        "audiovolumeup" => 0xAF,
        "mediatracknext" => 0xB0,
        "mediatrackprevious" => 0xB1,
        "mediastop" => 0xB2,
        "mediaplaypause" => 0xB3,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    let bytes = lower.as_bytes();
    if bytes.len() == 1 {
        return match bytes[0] {
            c @ b'a'..=b'z' => Some(u32::from(c - b'a') + 0x41),
            c @ b'0'..=b'9' => Some(u32::from(c - b'0') + 0x30),
            _ => None,
        };
    }
    // F1..F24 → 0x70..0x87. Reject leading zeros ("F01") and signs so names stay canonical.
    let digits = lower.strip_prefix('f')?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match digits.parse::<u32>() {
        Ok(n @ 1..=24) => Some(0x6F + n),
        _ => None,
    }
}

/// Resolves key names into a deduplicated list of virtual-key codes (first occurrence wins).
pub fn parse_combo(names: &[String]) -> Result<Vec<u32>, HotkeyError> {
    if names.is_empty() {
        return Err(HotkeyError::Empty);
    }
    let mut combo = Vec::with_capacity(names.len());
    for name in names {
        let vk = vk_for(name).ok_or_else(|| HotkeyError::UnknownKey(name.clone()))?;
        if !combo.contains(&vk) {
            combo.push(vk);
        }
    }
    Ok(combo)
}

pub fn validate_all_shortcuts(main: &[String], pause: &[String], ask: &[String]) -> Result<(), HotkeyError> {
    validate_shortcuts(main, pause)?;
    validate_shortcuts(main, ask)?;
    if !pause.is_empty() {
        validate_shortcuts(pause, ask)?;
    }
    Ok(())
}

/// Reject equal shortcuts and subsets, which would trigger both actions on one press.
pub fn validate_shortcuts(main: &[String], pause: &[String]) -> Result<(), HotkeyError> {
    let main = parse_combo(main)?;
    if pause.is_empty() {
        return Ok(());
    }
    let pause = parse_combo(pause)?;
    if pause.contains(&VK_ESCAPE) {
        return Err(HotkeyError::PauseEscape);
    }
    if main.iter().all(|key| pause.contains(key)) || pause.iter().all(|key| main.contains(key)) {
        return Err(HotkeyError::Collision);
    }
    Ok(())
}

/// Main shortcut is required. Optional shortcuts reserve Escape and may not be
/// equal to, subsets of, or supersets of any preceding shortcut.
pub fn validate_list(shortcuts: &[&[String]]) -> Result<(), HotkeyError> {
    let Some((main, rest)) = shortcuts.split_first() else { return Err(HotkeyError::Empty) };
    parse_combo(main)?;
    for (index, keys) in rest.iter().enumerate() {
        if keys.is_empty() {
            continue;
        }
        for previous in &shortcuts[..=index] {
            if !previous.is_empty() {
                validate_shortcuts(previous, keys)?;
            }
        }
    }
    Ok(())
}
