//! Key names, virtual-key codes and the hotkey state machine. Pure and cross-platform so it
//! can be unit-tested anywhere; the Windows keyboard hook only feeds it (vk, down) pairs.

use super::HotkeyEvent;
pub use opit_core::hotkey::*;

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
    /// re-registering are still accounted for. An unchanged key set keeps its pending release
    /// and lone-Ctrl state; a changed combination drops that activation state.
    pub fn retarget(&mut self, combo: Vec<u32>) {
        if same_combo(&self.combo, &combo) {
            self.combo = combo;
            return;
        }
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
        (vk == VK_ESCAPE && !self.combo.contains(&VK_ESCAPE)).then_some(HotkeyEvent::Escape)
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

pub fn same_combo(left: &[u32], right: &[u32]) -> bool {
    left.len() == right.len() && left.iter().all(|key| right.contains(key))
}

/// Preserve each unchanged action's tracker, including an active push-to-talk release.
/// New/changed bindings inherit physical held keys without synthesizing a key-down.
pub fn retarget_shortcuts(
    main: &KeyTracker,
    shortcuts: &mut Vec<(HotkeyEvent, KeyTracker)>,
    mappings: Vec<(HotkeyEvent, Vec<u32>)>,
) {
    let mut old = std::mem::take(shortcuts);
    *shortcuts = mappings
        .into_iter()
        .map(|(action, combo)| {
            let tracker = match old
                .iter()
                .position(|(previous, tracker)| *previous == action && same_combo(tracker.combo(), &combo))
            {
                Some(index) => old.remove(index).1,
                None => KeyTracker { held: main.held().to_vec(), ..KeyTracker::new(combo) },
            };
            (action, tracker)
        })
        .collect();
}

/// Prune missed key-ups using physical state before feeding a new key-down. Inside the
/// hook, async key state predates this event: a fresh press reads as up, a repeat as down.
pub fn track_transition(
    tracker: &mut KeyTracker,
    vk: u32,
    down: bool,
    key_is_down: impl Fn(u32) -> bool,
) -> [Option<HotkeyEvent>; 2] {
    let pruned = if down && !tracker.held().is_empty() { tracker.prune(key_is_down) } else { None };
    [pruned, tracker.on_key(vk, down)]
}

/// All registered actions share the same key stream and suppression rules.
pub fn track_shortcuts(
    main: &mut KeyTracker,
    shortcuts: &mut [(HotkeyEvent, KeyTracker)],
    vk: u32,
    down: bool,
    key_is_down: impl Fn(u32) -> bool,
) -> Vec<HotkeyEvent> {
    let mut actions = Vec::new();
    let mut handled = false;
    for (action, tracker) in shortcuts {
        let events = track_transition(tracker, vk, down, &key_is_down);
        if events.contains(&Some(HotkeyEvent::ComboDown)) {
            actions.push(action.clone());
        }
        if events.contains(&Some(HotkeyEvent::ComboUp)) {
            match action {
                HotkeyEvent::Edit => actions.push(HotkeyEvent::EditUp),
                HotkeyEvent::Translate => actions.push(HotkeyEvent::TranslateUp),
                _ => {}
            }
        }
        handled |= events.iter().flatten().any(|e| matches!(e, HotkeyEvent::ComboDown | HotkeyEvent::ComboUp));
    }
    let mut events: Vec<_> = track_transition(main, vk, down, key_is_down).into_iter().flatten().collect();
    if handled {
        events.retain(|e| !matches!(e, HotkeyEvent::LoneCtrl | HotkeyEvent::Escape));
    }
    events.extend(actions);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::*;

    #[test]
    fn retargeting_an_unchanged_combination_preserves_its_release_and_lone_ctrl() {
        let mut tracker = rctrl_rshift();
        assert_eq!(run(&mut tracker, &[(RCTRL, true), (RSHIFT, true)]), [ComboDown]);
        tracker.retarget(vec![RSHIFT, RCTRL]);
        assert_eq!(tracker.on_key(RSHIFT, false), Some(ComboUp));
        assert_eq!(tracker.on_key(RCTRL, false), None);
        assert_eq!(tracker.on_key(LCTRL, true), None);
        tracker.retarget(vec![RCTRL, RSHIFT]);
        assert_eq!(tracker.on_key(LCTRL, false), Some(LoneCtrl));
    }

    #[test]
    fn reconfiguring_other_shortcuts_preserves_held_dictation_edit_and_translate_releases() {
        for (pressed, released, vk) in
            [(ComboDown, ComboUp, 0x7C), (Edit, EditUp, 0x7D), (Translate, TranslateUp, 0x7E)]
        {
            let mut main = KeyTracker::new(vec![0x7C]);
            let mut shortcuts = vec![(Edit, KeyTracker::new(vec![0x7D])), (Translate, KeyTracker::new(vec![0x7E]))];
            assert_eq!(track_shortcuts(&mut main, &mut shortcuts, vk, true, |_| true), [pressed]);
            main.retarget(vec![0x7C]);
            retarget_shortcuts(
                &main,
                &mut shortcuts,
                vec![(StyleCycle, vec![0x7F]), (Translate, vec![0x7E]), (Edit, vec![0x7D])],
            );
            assert!(track_shortcuts(&mut main, &mut shortcuts, vk, true, |_| true).is_empty());
            assert_eq!(track_shortcuts(&mut main, &mut shortcuts, vk, false, |_| true), [released]);
            assert!(track_shortcuts(&mut main, &mut shortcuts, vk, false, |_| true).is_empty());
        }
    }

    #[test]
    fn new_bindings_do_not_synthesize_a_release_for_an_unreported_press() {
        let mut main = KeyTracker::new(vec![0x7C]);
        let mut shortcuts = Vec::new();
        assert!(track_shortcuts(&mut main, &mut shortcuts, 0xA2, true, |_| true).is_empty());
        retarget_shortcuts(&main, &mut shortcuts, vec![(Edit, vec![0xA2])]);
        assert_eq!(track_shortcuts(&mut main, &mut shortcuts, 0xA2, false, |_| true), [LoneCtrl]);
    }

    #[test]
    fn voice_shortcuts_deliver_down_and_up_without_lone_ctrl() {
        for (action, release) in [(Edit, EditUp), (Translate, TranslateUp)] {
            let mut main = KeyTracker::new(vec![0x7C]);
            let mut shortcuts = vec![(action.clone(), KeyTracker::new(vec![0xA2]))];
            let mut events = Vec::new();
            for down in [true, true, false] {
                events.extend(track_shortcuts(&mut main, &mut shortcuts, 0xA2, down, |_| true));
            }
            assert_eq!(events, [action, release]);
        }
    }

    #[test]
    fn every_auxiliary_action_fires_once_without_a_lone_ctrl_stop() {
        for action in [Pause, Ask, StyleCycle, Style("formal".into())] {
            let mut main = KeyTracker::new(vec![0x7C]);
            let mut shortcuts = vec![(action.clone(), KeyTracker::new(vec![0xA2]))];
            let mut events = Vec::new();
            for (vk, down) in [(0xA2, true), (0xA2, true), (0xA2, false), (0x7C, true), (0x7C, false)] {
                events.extend(track_shortcuts(&mut main, &mut shortcuts, vk, down, |_| true));
            }
            assert_eq!(events, [action, ComboDown, ComboUp]);
        }
    }

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

    #[test]
    fn ask_shortcut_is_optional_and_cannot_overlap_either_existing_combo() {
        let main = names(&["F13"]);
        let pause = names(&["F14"]);
        assert!(validate_all_shortcuts(&main, &pause, &[]).is_ok());
        assert!(validate_all_shortcuts(&main, &pause, &names(&["F15"])).is_ok());
        for ask in [names(&["F13"]), names(&["F14"]), names(&["LeftCtrl", "F13"]), names(&["Escape"])] {
            assert!(validate_all_shortcuts(&main, &pause, &ask).is_err());
        }
    }

    #[test]
    fn optional_pause_shortcut_rejects_collisions_and_reserved_escape() {
        assert!(validate_shortcuts(&names(&["F13"]), &[]).is_ok());
        assert!(validate_shortcuts(&names(&["F13"]), &names(&["F14"])).is_ok());
        for pause in [&["f13"][..], &["LeftCtrl", "F13"][..]] {
            assert_eq!(validate_shortcuts(&names(&["F13"]), &names(pause)), Err(HotkeyError::Collision));
        }
        assert_eq!(validate_shortcuts(&names(&["LeftCtrl", "F13"]), &names(&["F13"])), Err(HotkeyError::Collision));
        assert_eq!(validate_shortcuts(&names(&["F13"]), &names(&["Escape"])), Err(HotkeyError::PauseEscape));
        assert!(validate_shortcuts(&names(&["F13"]), &names(&["invalid"])).is_err());
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
        assert_eq!(vk_for("Escape"), Some(VK_ESCAPE));
    }

    #[test]
    fn flexible_keys_and_single_key_shortcuts_work() {
        for (name, vk) in [
            ("Tab", 0x09),
            ("Enter", 0x0D),
            ("ArrowDown", 0x28),
            ("Numpad1", 0x61),
            ("Backquote", 0xC0),
            ("MediaPlayPause", 0xB3),
        ] {
            assert_eq!(vk_for(name), Some(vk));
            let mut tracker = KeyTracker::new(parse_combo(&names(&[name])).unwrap());
            assert_eq!(tracker.on_key(vk, true), Some(ComboDown));
            assert_eq!(tracker.on_key(vk, true), None);
            assert_eq!(tracker.on_key(vk, false), Some(ComboUp));
        }
        let mut tracker = KeyTracker::new(vec![VK_ESCAPE]);
        assert_eq!(
            tracker.on_key(VK_ESCAPE, true),
            Some(ComboDown),
            "configured Escape is a shortcut, not cancellation"
        );
        assert_eq!(tracker.on_key(VK_ESCAPE, false), Some(ComboUp));
    }

    #[test]
    fn escape_in_a_chord_does_not_cancel_before_the_chord_is_complete() {
        let mut tracker = KeyTracker::new(vec![VK_ESCAPE, KEY_A]);
        assert_eq!(tracker.on_key(VK_ESCAPE, true), None);
        assert_eq!(tracker.on_key(KEY_A, true), Some(ComboDown));
        assert_eq!(tracker.on_key(KEY_A, false), Some(ComboUp));
        assert_eq!(tracker.on_key(VK_ESCAPE, false), None);
        assert_eq!(rctrl_rshift().on_key(VK_ESCAPE, true), Some(Escape));
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
