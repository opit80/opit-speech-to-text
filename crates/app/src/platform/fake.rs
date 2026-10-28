//! In-memory platform doubles for controller and app-state tests.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use opit_core::audio::Recording;
use opit_core::config::OverlayPosition;

use super::*;

#[derive(Default)]
pub struct FakeForegroundApp {
    pub info: Mutex<Option<ForegroundInfo>>,
    pub calls: Mutex<Vec<bool>>,
    pub running: Mutex<Vec<ForegroundInfo>>,
}

impl ForegroundApp for FakeForegroundApp {
    fn current(&self, include_title: bool) -> Option<ForegroundInfo> {
        self.calls.lock().unwrap().push(include_title);
        self.info.lock().unwrap().clone().map(|mut info| {
            if !include_title {
                info.title.clear();
            }
            info
        })
    }

    fn list_running(&self) -> Result<Vec<ForegroundInfo>, String> {
        Ok(self.running.lock().unwrap().clone())
    }
}

#[derive(Default)]
pub struct FakeMedia {
    pub calls: Mutex<Vec<Option<(MediaMode, u8)>>>,
    active: Mutex<bool>,
}

impl MediaControl for FakeMedia {
    fn engage(&self, mode: MediaMode, percent: u8) {
        let mut active = self.active.lock().unwrap();
        if !*active {
            self.calls.lock().unwrap().push(Some((mode, percent)));
            *active = true;
        }
    }

    fn release(&self) {
        let mut active = self.active.lock().unwrap();
        if *active {
            self.calls.lock().unwrap().push(None);
            *active = false;
        }
    }

    fn shutdown(&self) {
        self.release();
    }
}

/// A 1 s 220 Hz tone at 48 kHz stereo: passes the silence gate.
pub fn speech_recording() -> Recording {
    let samples = (0..48_000)
        .flat_map(|i| {
            let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
            [s, s]
        })
        .collect();
    Recording { samples, sample_rate: 48_000, channels: 2 }
}

pub fn silent_recording() -> Recording {
    Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 }
}

#[derive(Default)]
pub struct FakeMic {
    pub recording: Mutex<Option<Recording>>,
    pub fail_start: Mutex<Option<MicError>>,
    pub fell_back: Mutex<bool>,
    pub requested: Mutex<Vec<Option<String>>>,
    pub sink: Mutex<Option<CaptureSink>>,
    pub finished: Arc<AtomicUsize>,
    pub dropped: Arc<AtomicUsize>,
    pub paused: Arc<Mutex<bool>>,
    pub samples: Arc<Mutex<Vec<f32>>>,
    pub captured: Arc<Mutex<Vec<Recording>>>,
    /// One-shot callback after opening a capture, before returning it to the caller.
    pub before_start_returns: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl FakeMic {
    pub fn append_samples(&self, samples: &[f32]) {
        if !*self.paused.lock().unwrap() {
            self.samples.lock().unwrap().extend_from_slice(samples);
        }
    }
    pub fn starts(&self) -> usize {
        self.requested.lock().unwrap().len()
    }

    /// Sends a capture event as the running stream would.
    pub fn emit(&self, event: CaptureEvent) {
        let sink = self.sink.lock().unwrap().clone().expect("no capture running");
        sink(event);
    }
}

impl Microphone for FakeMic {
    fn devices(&self) -> Vec<String> {
        vec!["Fake Mic".into()]
    }

    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError> {
        self.requested.lock().unwrap().push(device.map(str::to_string));
        if let Some(err) = self.fail_start.lock().unwrap().take() {
            return Err(err);
        }
        *self.sink.lock().unwrap() = Some(sink);
        let recording = self.recording.lock().unwrap().clone().unwrap_or_else(speech_recording);
        *self.paused.lock().unwrap() = false;
        self.samples.lock().unwrap().clear();
        let started = Started {
            capture: Box::new(FakeCapture {
                recording: Some(recording),
                finished: self.finished.clone(),
                dropped: self.dropped.clone(),
                paused: self.paused.clone(),
                samples: self.samples.clone(),
                captured: self.captured.clone(),
            }),
            device: "Fake Mic".into(),
            fell_back: *self.fell_back.lock().unwrap(),
        };
        let callback = self.before_start_returns.lock().unwrap().take();
        if let Some(callback) = callback {
            callback();
        }
        Ok(started)
    }
}

struct FakeCapture {
    recording: Option<Recording>,
    finished: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    paused: Arc<Mutex<bool>>,
    samples: Arc<Mutex<Vec<f32>>>,
    captured: Arc<Mutex<Vec<Recording>>>,
}

impl Capture for FakeCapture {
    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
    }
    fn finish(mut self: Box<Self>) -> Recording {
        self.finished.fetch_add(1, Ordering::SeqCst);
        let mut recording = self.recording.take().unwrap();
        recording.samples.extend(self.samples.lock().unwrap().iter());
        self.captured.lock().unwrap().push(recording.clone());
        recording
    }
}

impl Drop for FakeCapture {
    fn drop(&mut self) {
        if self.recording.is_some() {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[derive(Default)]
pub struct FakeHotkey {
    pub registered: Mutex<Vec<Vec<String>>>,
    pub shortcuts: Mutex<Vec<(HotkeyEvent, Vec<String>)>>,
    pub sink: Mutex<Option<HotkeySink>>,
    pub fail_register: Mutex<Option<HotkeyError>>,
    pub paused: Mutex<bool>,
    pub capture_escape: Mutex<Vec<bool>>,
}

impl FakeHotkey {
    pub fn escape_captured(&self) -> bool {
        self.capture_escape.lock().unwrap().last().copied().unwrap_or(false)
    }
}

impl Hotkey for FakeHotkey {
    fn register_shortcuts(&self, shortcuts: &[(HotkeyEvent, Vec<String>)]) -> Result<(), HotkeyError> {
        *self.shortcuts.lock().unwrap() = shortcuts.to_vec();
        Ok(())
    }
    fn register(&self, keys: &[String], sink: HotkeySink) -> Result<(), HotkeyError> {
        if let Some(err) = self.fail_register.lock().unwrap().take() {
            return Err(err);
        }
        self.registered.lock().unwrap().push(keys.to_vec());
        *self.sink.lock().unwrap() = Some(sink);
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
    }

    fn set_capture_escape(&self, on: bool) {
        self.capture_escape.lock().unwrap().push(on);
    }
}

pub type PasteGate = (Mutex<bool>, Condvar);

pub struct FakePaster {
    pub outcome: Mutex<PasteOutcome>,
    pub pasted: Mutex<Vec<(String, bool)>>,
    pub policies: Mutex<Vec<bool>>,
    pub requests: Mutex<Vec<PasteRequest>>,
    pub clipboard_writes: Mutex<Vec<String>>,
    pub typed: Mutex<Vec<String>>,
    pub after_keys: Mutex<Vec<PasteAfter>>,
    pub allow_auto: Mutex<Option<bool>>,
    pub gate: Mutex<Option<Arc<PasteGate>>>,
}

impl Default for FakePaster {
    fn default() -> Self {
        Self {
            outcome: Mutex::new(PasteOutcome::Pasted),
            pasted: Mutex::default(),
            policies: Mutex::default(),
            requests: Mutex::default(),
            clipboard_writes: Mutex::default(),
            typed: Mutex::default(),
            after_keys: Mutex::default(),
            allow_auto: Mutex::default(),
            gate: Mutex::default(),
        }
    }
}

impl FakePaster {
    pub fn texts(&self) -> Vec<String> {
        self.pasted.lock().unwrap().iter().map(|(text, _)| text.clone()).collect()
    }
}

impl Paster for FakePaster {
    fn can_auto_paste(&self) -> bool {
        self.allow_auto.lock().unwrap().unwrap_or(true)
    }
    fn paste(&self, text: &str, request: PasteRequest) -> PasteOutcome {
        self.policies.lock().unwrap().push(request.auto_paste);
        self.requests.lock().unwrap().push(request);
        self.pasted.lock().unwrap().push((text.to_string(), request.restore));
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            let _ = gate.1.wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(5), |open| !*open).unwrap();
        }
        let outcome = match self.outcome.lock().unwrap().clone() {
            PasteOutcome::Pasted if !request.auto_paste => PasteOutcome::ClipboardOnly,
            outcome => outcome,
        };
        if outcome == PasteOutcome::Pasted && request.method == PasteMethod::Type {
            self.typed.lock().unwrap().push(text.to_string());
        } else if !matches!(outcome, PasteOutcome::Failed(_)) {
            self.clipboard_writes.lock().unwrap().push(text.to_string());
        }
        if outcome == PasteOutcome::Pasted && request.after != PasteAfter::None {
            self.after_keys.lock().unwrap().push(request.after);
        }
        outcome
    }
}

#[derive(Default)]
pub struct FakeOverlay {
    pub views: Mutex<Vec<OverlayView>>,
    pub hides: AtomicUsize,
    pub positions: Mutex<Vec<OverlayPosition>>,
}

impl FakeOverlay {
    pub fn last(&self) -> OverlayView {
        self.views.lock().unwrap().last().cloned().expect("overlay never shown")
    }
}

impl Overlay for FakeOverlay {
    fn show(&self, view: OverlayView) {
        self.views.lock().unwrap().push(view);
    }

    fn hide(&self) {
        self.hides.fetch_add(1, Ordering::SeqCst);
    }

    fn set_position(&self, position: OverlayPosition) {
        self.positions.lock().unwrap().push(position);
    }
}

#[derive(Default)]
pub struct FakeSounds {
    pub played: Mutex<Vec<Sound>>,
}

impl Sounds for FakeSounds {
    fn play(&self, sound: Sound) {
        self.played.lock().unwrap().push(sound);
    }
}

#[derive(Default)]
pub struct FakeSecrets {
    pub map: Mutex<HashMap<String, String>>,
    pub broken: Mutex<bool>,
}

impl FakeSecrets {
    pub fn with(key_ref: &str, secret: &str) -> Self {
        let secrets = Self::default();
        secrets.map.lock().unwrap().insert(key_ref.into(), secret.into());
        secrets
    }
}

impl SecretStore for FakeSecrets {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        if *self.broken.lock().unwrap() {
            return Err(SecretError("locked".into()));
        }
        Ok(self.map.lock().unwrap().get(key_ref).cloned())
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError> {
        self.map.lock().unwrap().insert(key_ref.into(), secret.into());
        Ok(())
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        self.map.lock().unwrap().remove(key_ref);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeAutostart {
    pub enabled: Mutex<bool>,
}

impl Autostart for FakeAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        Ok(*self.enabled.lock().unwrap())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        *self.enabled.lock().unwrap() = enabled;
        Ok(())
    }
}
