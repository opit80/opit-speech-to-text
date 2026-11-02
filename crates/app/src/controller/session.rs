//! Linux session-only recovery and shortcut verification. Never persisted or logged.

use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct PendingResult {
    pub id: u64,
    pub text: String,
}

#[derive(Clone, Default, Serialize)]
pub struct PendingResults {
    pub revision: u64,
    pub results: Vec<PendingResult>,
}

#[derive(Clone, Default, Serialize)]
pub struct ShortcutTest {
    pub generation: u64,
    pub active: bool,
    pub confirmed: bool,
}

#[derive(Default)]
pub(super) struct SessionState {
    pending: PendingResults,
    shortcut: ShortcutTest,
    expires: Option<Instant>,
}

impl SessionState {
    pub fn pending(&self) -> PendingResults {
        self.pending.clone()
    }

    pub fn retain(&mut self, text: &str) {
        self.pending.revision += 1;
        self.pending.results.push(PendingResult { id: self.pending.revision, text: text.into() });
    }

    pub fn dismiss(&mut self, id: u64) -> PendingResults {
        let before = self.pending.results.len();
        self.pending.results.retain(|result| result.id != id);
        if before != self.pending.results.len() {
            self.pending.revision += 1;
        }
        self.pending()
    }

    pub fn set_shortcut_test(&mut self, active: bool) -> ShortcutTest {
        self.shortcut.generation += 1;
        self.shortcut.active = active;
        self.shortcut.confirmed = false;
        self.expires = active.then(|| Instant::now() + Duration::from_secs(60));
        self.shortcut_test()
    }

    pub fn shortcut_test(&mut self) -> ShortcutTest {
        if self.expires.is_some_and(|expiry| Instant::now() >= expiry) {
            self.shortcut.active = false;
            self.expires = None;
        }
        self.shortcut.clone()
    }

    /// Returns true while input belongs to the probe, including after confirmation until
    /// explicitly disarmed or expired. No recording or provider request starts in this mode.
    pub fn probe_input(&mut self, is_toggle: bool) -> bool {
        if !self.shortcut_test().active {
            return false;
        }
        if is_toggle {
            self.shortcut.confirmed = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_keeps_every_undelivered_result_until_explicitly_dismissed() {
        let mut state = SessionState::default();
        state.retain("first");
        state.retain("second");
        let snapshot = state.pending();
        assert_eq!(snapshot.results.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["first", "second"]);
        let remaining = state.dismiss(snapshot.results[0].id);
        assert_eq!(remaining.results[0].text, "second");
        assert!(remaining.revision > snapshot.revision);
        assert!(SessionState::default().pending().results.is_empty(), "a new session never recovers old text");
    }

    #[test]
    fn probe_consumes_input_and_requires_a_real_toggle() {
        let mut state = SessionState::default();
        assert!(!state.probe_input(true));
        state.set_shortcut_test(true);
        assert!(state.probe_input(false));
        assert!(!state.shortcut_test().confirmed);
        assert!(state.probe_input(true));
        assert!(state.shortcut_test().confirmed);
        assert!(state.probe_input(false), "release events must not start a hold recording");
        state.set_shortcut_test(false);
        assert!(!state.probe_input(true));
    }

    #[test]
    fn probe_expires_when_the_window_disappears() {
        let mut state = SessionState::default();
        state.set_shortcut_test(true);
        state.expires = Some(Instant::now() - Duration::from_secs(1));
        assert!(!state.probe_input(true));
        assert!(!state.shortcut_test().confirmed);
    }
}
