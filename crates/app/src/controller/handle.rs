//! Controller input handles and guards for updates and file work.

use super::*;

#[derive(Clone)]
pub struct ControllerHandle {
    pub(super) tx: mpsc::UnboundedSender<Msg>,
    pub(super) status: Arc<Mutex<DictationStatus>>,
    work: Arc<Mutex<WorkState>>,
    #[cfg(target_os = "linux")]
    pub(super) session_state: Arc<Mutex<session::SessionState>>,
}

#[derive(Default)]
struct WorkState {
    files: usize,
    updates: usize,
}

/// Why an update may not install right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldRefusal {
    /// A dictation is recording, waiting, transcribing or pasting (or the controller stopped).
    Dictation,
    /// A file job is running or holds a result that was not saved, copied or discarded yet.
    FileWork,
}

/// Shared by UI/IPC workers and update installation. Held until the worker finishes,
/// including when its async caller is cancelled. A UI file job keeps it while its finished
/// result exists only in memory.
pub struct WorkGuard {
    state: Arc<Mutex<WorkState>>,
    update: bool,
}

impl Drop for WorkGuard {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.update {
            state.updates -= 1;
        } else {
            state.files -= 1;
        }
    }
}

pub struct Inbox(pub(super) mpsc::UnboundedReceiver<Msg>);

#[derive(Clone)]
#[cfg_attr(not(target_os = "linux"), derive(Copy))]
pub enum ControlAction {
    #[cfg(target_os = "linux")]
    Shortcut(crate::platform::HotkeyEvent),
    #[cfg(target_os = "linux")]
    ShortcutProbe,
    Toggle,
    Start,
    Stop,
    Cancel,
    Pause,
    Resume,
    Retry,
}

/// While alive, new dictations and "Try again" are refused; dropping it lifts the hold.
pub struct UpdateHold {
    tx: mpsc::UnboundedSender<Msg>,
    _work: WorkGuard,
}

impl Drop for UpdateHold {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::ReleaseUpdateHold);
    }
}

/// Creates the handle first so the overlay and hotkey sinks can exist before the
/// controller that consumes them.
pub fn channel() -> (ControllerHandle, Inbox) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        ControllerHandle {
            tx,
            status: Arc::default(),
            work: Arc::default(),
            #[cfg(target_os = "linux")]
            session_state: Arc::default(),
        },
        Inbox(rx),
    )
}

impl ControllerHandle {
    #[cfg(target_os = "linux")]
    pub fn pending_results(&self) -> PendingResults {
        self.session_state.lock().unwrap_or_else(PoisonError::into_inner).pending()
    }

    #[cfg(target_os = "linux")]
    pub fn dismiss_pending_result(&self, id: u64) -> PendingResults {
        self.session_state.lock().unwrap_or_else(PoisonError::into_inner).dismiss(id)
    }

    #[cfg(target_os = "linux")]
    pub fn shortcut_test(&self) -> ShortcutTest {
        self.session_state.lock().unwrap_or_else(PoisonError::into_inner).shortcut_test()
    }

    #[cfg(target_os = "linux")]
    pub async fn set_shortcut_test(&self, active: bool) -> Result<ShortcutTest, ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::ShortcutTest { active, reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    pub async fn control(&self, action: ControlAction) -> Result<DictationStatus, ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::Control { action, reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    pub async fn test_ask(&self, config: opit_core::ask::AskConfig) -> Result<String, ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::TestAsk { config: Box::new(config), reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    pub async fn ask(&self, text: Option<String>) -> Result<String, ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::Ask { text, reply: Some(reply) });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    pub async fn record(&self, seconds: u32) -> Result<String, ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::Record { seconds, reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }
    /// Never blocks; messages sent after shutdown are dropped.
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn status(&self) -> DictationStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub async fn retry_history(&self, id: i64, audio: PreparedAudio) -> Result<(), ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::RetryHistory { id, audio: Arc::new(audio), reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    /// Stops new dictations so an update can install. Refused while file work is active, and
    /// while a dictation is recording, waiting, transcribing or pasting (or after shutdown). The
    /// controller answers in order with its other messages, so no dictation can slip in between
    /// the answer and the hold.
    pub async fn hold_for_update(&self) -> Result<UpdateHold, HoldRefusal> {
        let work = {
            let mut state = self.work.lock().unwrap_or_else(PoisonError::into_inner);
            if state.files != 0 {
                return Err(HoldRefusal::FileWork);
            }
            state.updates += 1;
            WorkGuard { state: self.work.clone(), update: true }
        };
        let (reply, answer) = oneshot::channel();
        self.send(Msg::HoldForUpdate(reply));
        if answer.await.unwrap_or(false) {
            Ok(UpdateHold { tx: self.tx.clone(), _work: work })
        } else {
            Err(HoldRefusal::Dictation)
        }
    }

    /// `None` while an update holds the work state (it is installing or about to).
    pub fn begin_file_work(&self) -> Option<WorkGuard> {
        let mut state = self.work.lock().unwrap_or_else(PoisonError::into_inner);
        if state.updates != 0 {
            return None;
        }
        state.files += 1;
        Some(WorkGuard { state: self.work.clone(), update: false })
    }

    pub fn hotkey_sink(&self) -> HotkeySink {
        let tx = self.tx.clone();
        Arc::new(move |event| {
            let _ = tx.send(Msg::Hotkey(event));
        })
    }

    pub fn overlay_sink(&self) -> OverlaySink {
        let tx = self.tx.clone();
        Arc::new(move |action| {
            let _ = tx.send(Msg::Overlay(action));
        })
    }
}
