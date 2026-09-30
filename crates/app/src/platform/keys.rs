//! Key names, virtual-key codes and the hotkey state machine. Pure and cross-platform so it
//! can be unit-tested anywhere; the Windows keyboard hook only feeds it (vk, down) pairs.

use super::{HotkeyError, HotkeyEvent};

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

fn is_ctrl(vk: u32) -> bool {
    matches!(vk, VK_CONTROL | VK_LCONTROL | VK_RCONTROL)
}

/// Turns a stream of physical key transitions into [`HotkeyEvent`]s.
#[derive(Debug, Clone)]
pub struct KeyTracker {
    combo: Vec<u32>,
    /// Every key currently held (combo or not), in press order.
    held: Vec<u32>,
    /// ComboDown was emitted and ComboUp has not been yet.
    combo_active: bool,
    /// A Ctrl key that went down alone and may still become a LoneCtrl on release.
    lone_candidate: Option<u32>,
}

impl KeyTracker {
    /// `combo` must be non-empty and deduplicated (see [`parse_combo`]).
    pub fn new(combo: Vec<u32>) -> Self {
        Self { combo, held: Vec::new(), combo_active: false, lone_candidate: None }
    }

    /// Switches to a new combo but keeps the held-key set, so keys already down while
    /// re-registering are still accounted for. Pending ComboDown/LoneCtrl state is dropped.
    pub fn retarget(&mut self, combo: Vec<u32>) {
        self.combo = combo;
        self.combo_active = false;
        self.lone_candidate = None;
    }

    /// The combo this tracker watches.
    pub fn combo(&self) -> &[u32] {
        &self.combo
    }

    /// Keys currently believed to be held.
    pub fn held(&self) -> &[u32] {
        &self.held
    }

    /// Feeds one physical transition. Auto-repeat downs of held keys are ignored.
    pub fn on_key(&mut self, vk: u32, down: bool) -> Option<HotkeyEvent> {
        if down { self.on_down(vk) } else { self.on_up(vk) }
    }

    fn on_down(&mut self, vk: u32) -> Option<HotkeyEvent> {
        if self.held.contains(&vk) {
            return None; // auto-repeat
        }
        let was_empty = self.held.is_empty();
        self.held.push(vk);

        if self.lone_candidate.is_some_and(|c| c != vk) {
            self.lone_candidate = None;
        }
        if is_ctrl(vk) && was_empty && self.combo != [vk] {
            self.lone_candidate = Some(vk);
        }

        if !self.combo_active && self.combo_is_exactly_held() {
            self.combo_active = true;
            self.lone_candidate = None;
            return Some(HotkeyEvent::ComboDown);
        }
        (vk == VK_ESCAPE).then_some(HotkeyEvent::Escape)
    }

    fn on_up(&mut self, vk: u32) -> Option<HotkeyEvent> {
        let was_held = match self.held.iter().position(|&k| k == vk) {
            Some(i) => {
                self.held.remove(i);
                true
            }
            None => false,
        };
        if self.combo_active && self.combo.contains(&vk) {
            self.combo_active = false;
            self.lone_candidate = None;
            return Some(HotkeyEvent::ComboUp);
        }
        if self.lone_candidate == Some(vk) {
            self.lone_candidate = None;
            if was_held {
                return Some(HotkeyEvent::LoneCtrl);
            }
        }
        None
    }

    /// Drops held keys for which `still_down(vk)` is false (their key-up was missed, e.g.
    /// released on the secure desktop). Returns ComboUp if that ends an active combo.
    pub fn prune(&mut self, mut still_down: impl FnMut(u32) -> bool) -> Option<HotkeyEvent> {
        let before = self.held.len();
        self.held.retain(|&vk| still_down(vk));
        if self.held.len() == before {
            return None;
        }
        if self.lone_candidate.is_some_and(|c| !self.held.contains(&c)) {
            self.lone_candidate = None;
        }
        if self.combo_active && !self.combo.iter().all(|k| self.held.contains(k)) {
            self.combo_active = false;
            return Some(HotkeyEvent::ComboUp);
        }
        None
    }

    fn combo_is_exactly_held(&self) -> bool {
        self.held.len() == self.combo.len() && self.combo.iter().all(|k| self.held.contains(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::*;

    const LCTRL: u32 = 0xA2;
    const RCTRL: u32 = 0xA3;
    const LSHIFT: u32 = 0xA0;
    const RSHIFT: u32 = 0xA1;
    const RALT: u32 = 0xA5;
    const KEY_A: u32 = 0x41;
    const KEY_C: u32 = 0x43;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    /// Feeds `(vk, down)` pairs and collects every emitted event.
    fn run(t: &mut KeyTracker, seq: &[(u32, bool)]) -> Vec<HotkeyEvent> {
        seq.iter().filter_map(|&(vk, down)| t.on_key(vk, down)).collect()
    }

    fn rctrl_rshift() -> KeyTracker {
        KeyTracker::new(vec![RCTRL, RSHIFT])
    }

    #[test]
    fn vk_for_modifiers_are_side_specific() {
        assert_eq!(vk_for("LeftCtrl"), Some(0xA2));
        assert_eq!(vk_for("RightCtrl"), Some(0xA3));
        assert_eq!(vk_for("LeftShift"), Some(0xA0));
        assert_eq!(vk_for("RightShift"), Some(0xA1));
        assert_eq!(vk_for("LeftAlt"), Some(0xA4));
        assert_eq!(vk_for("RightAlt"), Some(0xA5));
        assert_eq!(vk_for("LeftWin"), Some(0x5B));
        assert_eq!(vk_for("RightWin"), Some(0x5C));
    }

    #[test]
    fn vk_for_is_case_insensitive_and_trims() {
        assert_eq!(vk_for("rightctrl"), Some(0xA3));
        assert_eq!(vk_for("RIGHTCTRL"), Some(0xA3));
        assert_eq!(vk_for(" Space "), Some(0x20));
        assert_eq!(vk_for("f5"), Some(0x74));
        assert_eq!(vk_for("q"), Some(0x51));
    }

    #[test]
    fn vk_for_function_keys() {
        assert_eq!(vk_for("F1"), Some(0x70));
        assert_eq!(vk_for("F12"), Some(0x7B));
        assert_eq!(vk_for("F13"), Some(0x7C));
        assert_eq!(vk_for("F24"), Some(0x87));
        assert_eq!(vk_for("F"), Some(0x46), "a lone F is the letter");
        for bad in ["F0", "F25", "F01", "F+1", "F-1", "F1a", "Fx"] {
            assert_eq!(vk_for(bad), None, "{bad}");
        }
    }

    #[test]
    fn vk_for_letters_and_digits() {
        assert_eq!(vk_for("A"), Some(0x41));
        assert_eq!(vk_for("Z"), Some(0x5A));
        assert_eq!(vk_for("0"), Some(0x30));
        assert_eq!(vk_for("9"), Some(0x39));
        assert_eq!(vk_for("AB"), None);
        assert_eq!(vk_for("!"), None);
        assert_eq!(vk_for(""), None);
    }

    #[test]
    fn vk_for_navigation_and_locks() {
        assert_eq!(vk_for("Space"), Some(0x20));
        assert_eq!(vk_for("CapsLock"), Some(0x14));
        assert_eq!(vk_for("ScrollLock"), Some(0x91));
        assert_eq!(vk_for("Pause"), Some(0x13));
        assert_eq!(vk_for("Insert"), Some(0x2D));
        assert_eq!(vk_for("Home"), Some(0x24));
        assert_eq!(vk_for("End"), Some(0x23));
        assert_eq!(vk_for("PageUp"), Some(0x21));
        assert_eq!(vk_for("PageDown"), Some(0x22));
        assert_eq!(vk_for("Ctrl"), None, "generic modifiers are not accepted");
        assert_eq!(vk_for("Escape"), None);
    }

    #[test]
    fn parse_combo_errors_and_dedupe() {
        assert_eq!(parse_combo(&[]), Err(HotkeyError::Empty));
        assert_eq!(parse_combo(&names(&["RightCtrl", "Hyper"])), Err(HotkeyError::UnknownKey("Hyper".into())));
        assert_eq!(parse_combo(&names(&["RightCtrl", "rightctrl", "RightShift"])), Ok(vec![RCTRL, RSHIFT]));
        assert_eq!(parse_combo(&names(&["F13"])), Ok(vec![0x7C]));
    }

    #[test]
    fn combo_either_order_fires_once() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![ComboUp]);

        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RSHIFT, true), (RCTRL, true)]), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RCTRL, false), (RSHIFT, false)]), vec![ComboUp]);
    }

    #[test]
    fn auto_repeat_does_not_refire() {
        let mut t = rctrl_rshift();
        let seq = [(RCTRL, true), (RCTRL, true), (RSHIFT, true), (RSHIFT, true), (RCTRL, true), (RSHIFT, true)];
        assert_eq!(run(&mut t, &seq), vec![ComboDown]);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![ComboUp]);
    }

    #[test]
    fn combo_up_fires_once_whatever_release_order() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true)]);
        assert_eq!(t.on_key(RCTRL, false), Some(ComboUp));
        assert_eq!(t.on_key(RSHIFT, false), None);
        // Pressing again works.
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
    }

    #[test]
    fn re_pressing_released_combo_key_fires_again() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true), (RSHIFT, false)]);
        // RCtrl still held; pressing RShift again re-forms the combo.
        assert_eq!(t.on_key(RSHIFT, true), Some(ComboDown));
    }

    #[test]
    fn combo_release_does_not_produce_lone_ctrl() {
        let mut t = rctrl_rshift();
        let ev = run(&mut t, &[(RCTRL, true), (RSHIFT, true), (RSHIFT, false), (RCTRL, false)]);
        assert_eq!(ev, vec![ComboDown, ComboUp]);
        // A later lone left Ctrl tap is a LoneCtrl.
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn lone_ctrl_both_sides_and_generic() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RCTRL, false)]), vec![LoneCtrl]);
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
        assert_eq!(run(&mut t, &[(VK_CONTROL, true), (VK_CONTROL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn ctrl_shortcut_is_not_lone_ctrl() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (KEY_C, true), (KEY_C, false), (LCTRL, false)]), vec![]);
        // Other key released first does not matter either.
        assert_eq!(run(&mut t, &[(LCTRL, true), (KEY_C, true), (LCTRL, false), (KEY_C, false)]), vec![]);
    }

    #[test]
    fn ctrl_pressed_while_other_key_held_is_not_lone() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(KEY_A, true), (LCTRL, true), (KEY_A, false), (LCTRL, false)]), vec![]);
    }

    #[test]
    fn both_ctrls_is_not_lone() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (RCTRL, true), (RCTRL, false), (LCTRL, false)]), vec![]);
    }

    #[test]
    fn single_ctrl_combo_never_lone() {
        let mut t = KeyTracker::new(vec![RCTRL]);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RCTRL, true), (RCTRL, false)]), vec![ComboDown, ComboUp]);
        // The other Ctrl is still a LoneCtrl.
        assert_eq!(run(&mut t, &[(LCTRL, true), (LCTRL, false)]), vec![LoneCtrl]);
    }

    #[test]
    fn single_ctrl_combo_blocked_by_extra_key_is_still_not_lone() {
        let mut t = KeyTracker::new(vec![RCTRL]);
        assert_eq!(run(&mut t, &[(KEY_A, true), (RCTRL, true), (KEY_A, false), (RCTRL, false)]), vec![]);
    }

    #[test]
    fn extra_key_held_blocks_combo() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(KEY_A, true), (RCTRL, true), (RSHIFT, true)]), vec![]);
        // Releasing the extra key does not retro-fire; only a combo key-down can form it.
        assert_eq!(t.on_key(KEY_A, false), None);
        assert_eq!(run(&mut t, &[(RSHIFT, false), (RCTRL, false)]), vec![]);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
    }

    #[test]
    fn extra_modifier_blocks_combo() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LSHIFT, true), (RCTRL, true), (RSHIFT, true)]), vec![]);
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RALT, true), (RSHIFT, true)]), vec![]);
    }

    #[test]
    fn extra_key_during_active_combo_keeps_it_active() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(RCTRL, true), (RSHIFT, true)]);
        assert_eq!(run(&mut t, &[(KEY_A, true), (KEY_A, false)]), vec![]);
        assert_eq!(t.on_key(RCTRL, false), Some(ComboUp));
    }

    #[test]
    fn escape_fires_on_down_not_repeat_or_up() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(VK_ESCAPE, true), (VK_ESCAPE, true), (VK_ESCAPE, true)]), vec![Escape]);
        assert_eq!(t.on_key(VK_ESCAPE, false), None);
        assert_eq!(t.on_key(VK_ESCAPE, true), Some(Escape));
    }

    #[test]
    fn escape_during_combo_fires_and_cancels_lone_ctrl() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true), (VK_ESCAPE, true)]), vec![ComboDown, Escape]);
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(LCTRL, true), (VK_ESCAPE, true), (VK_ESCAPE, false), (LCTRL, false)]), vec![Escape]);
    }

    #[test]
    fn unmatched_key_up_is_ignored() {
        let mut t = rctrl_rshift();
        assert_eq!(run(&mut t, &[(RCTRL, false), (KEY_A, false), (LCTRL, false)]), vec![]);
        assert!(t.held().is_empty());
    }

    #[test]
    fn prune_drops_stale_keys_and_ends_combo() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(KEY_A, true)]); // A's key-up gets lost
        assert_eq!(t.prune(|vk| vk != KEY_A), None);
        assert_eq!(run(&mut t, &[(RCTRL, true), (RSHIFT, true)]), vec![ComboDown]);
        assert_eq!(t.prune(|vk| vk != RSHIFT), Some(ComboUp));
        assert_eq!(t.held(), &[RCTRL]);
        assert_eq!(t.prune(|_| true), None);
    }

    #[test]
    fn retarget_keeps_held_keys() {
        let mut t = rctrl_rshift();
        run(&mut t, &[(KEY_A, true)]);
        t.retarget(vec![0x7C]);
        assert_eq!(t.combo(), &[0x7C]);
        assert_eq!(t.on_key(0x7C, true), None, "A is still held");
        run(&mut t, &[(KEY_A, false), (0x7C, false)]);
        assert_eq!(t.on_key(0x7C, true), Some(ComboDown));
    }
}
