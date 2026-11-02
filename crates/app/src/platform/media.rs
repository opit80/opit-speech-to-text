//! Ordered media work and restoration policy. All backend objects stay on one worker thread.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;

use opit_core::config::MediaMode;

use super::MediaControl;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Playback {
    Playing,
    Paused,
    Other,
}

pub(super) trait PlaybackSession {
    fn playback(&self) -> Playback;
    fn pause(&self) -> bool;
    fn play(&self);
}

pub(super) trait VolumeSession {
    fn volume(&self) -> Option<f32>;
    fn set_volume(&self, volume: f32) -> bool;
}

pub(super) trait Backend {
    /// Only eligible sessions: volume enumeration excludes our process and system sounds.
    fn playback_sessions(&self) -> Vec<Box<dyn PlaybackSession>>;
    fn volume_sessions(&self) -> Vec<Box<dyn VolumeSession>>;
}

struct Ducked {
    session: Box<dyn VolumeSession>,
    original: f32,
    applied: f32,
}

/// Declared after the backend on the worker stack, so restoration precedes COM teardown.
#[derive(Default)]
struct Restoration {
    mode: Option<MediaMode>,
    paused: Vec<Box<dyn PlaybackSession>>,
    ducked: Vec<Ducked>,
}

impl Restoration {
    fn engage(&mut self, backend: &dyn Backend, mode: MediaMode, percent: u8, wanted: &AtomicBool) {
        if self.mode.is_some() {
            return;
        }
        self.mode = Some(mode);
        match mode {
            MediaMode::Pause => {
                for session in backend.playback_sessions() {
                    if !wanted.load(Ordering::Acquire) {
                        break;
                    }
                    if session.playback() != Playback::Playing {
                        continue;
                    }
                    // Retain before mutation: unwinding after a mutation must also restore it.
                    self.paused.push(session);
                    if !self.paused.last().unwrap().pause() {
                        self.paused.pop();
                    }
                }
                tracing::info!(count = self.paused.len(), "paused media sessions");
            }
            MediaMode::Duck => {
                for session in backend.volume_sessions() {
                    if !wanted.load(Ordering::Acquire) {
                        break;
                    }
                    let Some(original) = session.volume().filter(|v| v.is_finite() && (0.0..=1.0).contains(v)) else {
                        continue;
                    };
                    let applied = original * f32::from(percent.min(100)) / 100.0;
                    self.ducked.push(Ducked { session, original, applied });
                    if !self.ducked.last().unwrap().session.set_volume(applied) {
                        self.ducked.pop();
                    }
                }
                tracing::info!(count = self.ducked.len(), "ducked media sessions");
            }
        }
    }

    fn release(&mut self) {
        if self.mode.take().is_none() {
            return;
        }
        let mut resumed = 0;
        for session in self.paused.drain(..) {
            if session.playback() == Playback::Paused {
                session.play();
                resumed += 1;
            }
        }
        let mut restored = 0;
        for ducked in self.ducked.drain(..) {
            // WASAPI stores normalized floats; tolerate only round-off, not user adjustments.
            if ducked.session.volume().is_some_and(|current| (current - ducked.applied).abs() <= 0.000_001)
                && ducked.session.set_volume(ducked.original)
            {
                restored += 1;
            }
        }
        tracing::info!(resumed, restored, "released media sessions");
    }
}

impl Drop for Restoration {
    fn drop(&mut self) {
        self.release();
    }
}

enum Command {
    Engage(MediaMode, u8),
    Release,
    Shutdown,
}

struct Client {
    tx: mpsc::Sender<Command>,
    worker: Option<JoinHandle<()>>,
    engaged: bool,
    stopped: bool,
}

pub(super) struct QueuedMedia {
    client: Mutex<Client>,
    wanted: Arc<AtomicBool>,
}

impl QueuedMedia {
    /// The factory and the backend's COM apartment are both constructed on the worker.
    pub fn new<B: Backend + 'static>(factory: impl FnOnce() -> Option<B> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let wanted = Arc::new(AtomicBool::new(false));
        let worker_wanted = wanted.clone();
        let worker = std::thread::Builder::new().name("opit-media".into()).spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                let Some(backend) = factory() else { return };
                let mut guard = Restoration::default();
                while let Ok(command) = rx.recv() {
                    match command {
                        Command::Engage(mode, percent) => guard.engage(&backend, mode, percent, &worker_wanted),
                        Command::Release => guard.release(),
                        Command::Shutdown => break,
                    }
                }
                // Also runs on channel disconnect and unwind, before the backend is dropped.
            }));
            if result.is_err() {
                tracing::warn!("media worker stopped unexpectedly");
            }
        });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(_) => {
                tracing::warn!("media worker unavailable");
                None
            }
        };
        Self { client: Mutex::new(Client { tx, worker, engaged: false, stopped: false }), wanted }
    }
}

impl MediaControl for QueuedMedia {
    fn engage(&self, mode: MediaMode, percent: u8) {
        let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
        if client.stopped || client.engaged {
            return;
        }
        self.wanted.store(true, Ordering::Release);
        client.engaged = client.tx.send(Command::Engage(mode, percent)).is_ok();
    }

    fn release(&self) {
        let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
        self.wanted.store(false, Ordering::Release);
        if client.engaged {
            let _ = client.tx.send(Command::Release);
            client.engaged = false;
        }
    }

    fn shutdown(&self) {
        let worker = {
            let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
            self.wanted.store(false, Ordering::Release);
            client.stopped = true;
            client.engaged = false;
            let _ = client.tx.send(Command::Shutdown);
            client.worker.take()
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for QueuedMedia {
    fn drop(&mut self) {
        // A last-owner drop may happen on the actor. Queue cleanup without joining there;
        // the app's explicit shutdown joins from spawn_blocking before process exit.
        self.wanted.store(false, Ordering::Release);
        let client = self.client.get_mut().unwrap_or_else(PoisonError::into_inner);
        let _ = client.tx.send(Command::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Clone)]
    struct Player {
        state: Rc<Cell<Playback>>,
        pauses: Rc<Cell<u32>>,
        plays: Rc<Cell<u32>>,
        refuse: bool,
        panic_after_pause: bool,
        paused_signal: Option<mpsc::Sender<()>>,
    }
    impl Player {
        fn new(state: Playback) -> Self {
            Self {
                state: Rc::new(Cell::new(state)),
                pauses: Default::default(),
                plays: Default::default(),
                refuse: false,
                panic_after_pause: false,
                paused_signal: None,
            }
        }
    }
    impl PlaybackSession for Player {
        fn playback(&self) -> Playback {
            self.state.get()
        }
        fn pause(&self) -> bool {
            self.pauses.set(self.pauses.get() + 1);
            if self.refuse {
                return false;
            }
            self.state.set(Playback::Paused);
            if let Some(signal) = &self.paused_signal {
                let _ = signal.send(());
            }
            assert!(!self.panic_after_pause, "synthetic media worker panic");
            true
        }
        fn play(&self) {
            self.plays.set(self.plays.get() + 1);
            self.state.set(Playback::Playing);
        }
    }
    #[derive(Clone)]
    struct Volume(Rc<Cell<f32>>);
    impl VolumeSession for Volume {
        fn volume(&self) -> Option<f32> {
            Some(self.0.get())
        }
        fn set_volume(&self, volume: f32) -> bool {
            self.0.set(volume);
            true
        }
    }
    #[derive(Default)]
    struct FakeBackend {
        players: Vec<Player>,
        volumes: Vec<Volume>,
    }
    impl Backend for FakeBackend {
        fn playback_sessions(&self) -> Vec<Box<dyn PlaybackSession>> {
            self.players.iter().map(|p| Box::new(p.clone()) as Box<dyn PlaybackSession>).collect()
        }
        fn volume_sessions(&self) -> Vec<Box<dyn VolumeSession>> {
            self.volumes.iter().map(|v| Box::new(v.clone()) as Box<dyn VolumeSession>).collect()
        }
    }

    #[test]
    fn pause_only_restores_successfully_paused_players_still_paused() {
        let players = vec![
            Player::new(Playback::Playing),
            Player::new(Playback::Paused),
            Player::new(Playback::Other),
            Player::new(Playback::Playing),
            Player::new(Playback::Playing),
            Player { refuse: true, ..Player::new(Playback::Playing) },
        ];
        let backend = FakeBackend { players: players.clone(), ..Default::default() };
        let mut guard = Restoration::default();
        guard.engage(&backend, MediaMode::Pause, 20, &AtomicBool::new(true));
        guard.engage(&backend, MediaMode::Pause, 20, &AtomicBool::new(true));
        players[3].state.set(Playback::Playing);
        players[4].state.set(Playback::Other);
        guard.release();
        guard.release();
        assert_eq!(players.iter().map(|p| p.pauses.get()).collect::<Vec<_>>(), [1, 0, 0, 1, 1, 1]);
        assert_eq!(players.iter().map(|p| p.plays.get()).collect::<Vec<_>>(), [1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn duck_restores_exact_originals_except_user_changes_and_ignores_new_sessions() {
        for percent in [0, 20, 100] {
            let volumes = vec![Volume(Rc::new(Cell::new(0.713))), Volume(Rc::new(Cell::new(0.4)))];
            let mut backend = FakeBackend { volumes: volumes.clone(), ..Default::default() };
            let mut guard = Restoration::default();
            guard.engage(&backend, MediaMode::Duck, percent, &AtomicBool::new(true));
            assert_eq!(volumes[0].0.get(), 0.713 * f32::from(percent) / 100.0);
            volumes[1].0.set(0.61);
            backend.volumes.push(Volume(Rc::new(Cell::new(0.8))));
            drop(guard);
            assert_eq!(volumes[0].0.get(), 0.713);
            assert_eq!(volumes[1].0.get(), 0.61);
            assert_eq!(backend.volumes[2].0.get(), 0.8);
        }
    }

    #[test]
    fn restoration_runs_when_worker_logic_unwinds_after_mutation() {
        let player = Player { panic_after_pause: true, ..Player::new(Playback::Playing) };
        let backend = FakeBackend { players: vec![player.clone()], ..Default::default() };
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let mut guard = Restoration::default();
                guard.engage(&backend, MediaMode::Pause, 20, &AtomicBool::new(true));
            }))
            .is_err()
        );
        assert_eq!(player.plays.get(), 1);
    }

    #[test]
    fn queued_work_is_ordered_nonblocking_idempotent_and_shutdown_drains_restoration() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let (paused_tx, paused_rx) = mpsc::channel();
        let media = QueuedMedia::new(move || {
            entered_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            struct Observed {
                backend: FakeBackend,
                tx: mpsc::Sender<(u32, u32)>,
            }
            impl Backend for Observed {
                fn playback_sessions(&self) -> Vec<Box<dyn PlaybackSession>> {
                    self.backend.playback_sessions()
                }
                fn volume_sessions(&self) -> Vec<Box<dyn VolumeSession>> {
                    vec![]
                }
            }
            impl Drop for Observed {
                fn drop(&mut self) {
                    let p = &self.backend.players[0];
                    self.tx.send((p.pauses.get(), p.plays.get())).unwrap();
                }
            }
            Some(Observed {
                backend: FakeBackend {
                    players: vec![Player { paused_signal: Some(paused_tx), ..Player::new(Playback::Playing) }],
                    ..Default::default()
                },
                tx: result_tx,
            })
        });
        entered_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        media.engage(MediaMode::Pause, 20);
        media.engage(MediaMode::Duck, 20);
        media.release();
        media.release();
        media.engage(MediaMode::Pause, 20);
        continue_tx.send(()).unwrap();
        paused_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        media.shutdown();
        let (pauses, plays) = result_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(pauses, plays);
        assert!((1..=2).contains(&pauses));
        media.shutdown();
        media.engage(MediaMode::Pause, 20);
    }

    #[test]
    fn worker_panic_restores_before_its_apartment_is_dropped() {
        let (result_tx, result_rx) = mpsc::channel();
        let media = QueuedMedia::new(move || {
            struct PanickingBackend {
                player: Player,
                tx: mpsc::Sender<(u32, u32)>,
            }
            impl Backend for PanickingBackend {
                fn playback_sessions(&self) -> Vec<Box<dyn PlaybackSession>> {
                    vec![Box::new(self.player.clone())]
                }
                fn volume_sessions(&self) -> Vec<Box<dyn VolumeSession>> {
                    vec![]
                }
            }
            impl Drop for PanickingBackend {
                fn drop(&mut self) {
                    let _ = self.tx.send((self.player.pauses.get(), self.player.plays.get()));
                }
            }
            Some(PanickingBackend {
                player: Player { panic_after_pause: true, ..Player::new(Playback::Playing) },
                tx: result_tx,
            })
        });
        media.engage(MediaMode::Pause, 20);
        assert_eq!(result_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(), (1, 1));
        media.shutdown();
    }
}
