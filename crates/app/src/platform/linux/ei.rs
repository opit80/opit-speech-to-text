//! Session-lived RemoteDesktop keyboard access over the libei protocol.
//!
//! Open lazily, on a blocking paste worker, never at startup. Permission denial disables
//! further requests until restart. No restore token or input content is persisted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

use ashpd::desktop::remote_desktop::{
    ConnectToEISOptions, DeviceType, RemoteDesktop, SelectDevicesOptions, StartOptions,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode, Session};
use futures_util::StreamExt;
use reis::ei;
use reis::event::{Connection, Device, DeviceCapability, EiEvent};
use reis::tokio::EiConvertEventStream;

use super::input::Attempt;

const OPEN_LIMIT: Duration = Duration::from_secs(25);
const REPLY_LIMIT: Duration = Duration::from_secs(30);

struct Request {
    keys: Vec<(u32, bool)>,
    expires: Instant,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::Sender<Attempt>,
}

/// ashpd sessions do not close on Drop. Also close when a permission request times out.
struct SessionGuard(Option<Session<RemoteDesktop>>);

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = session.close().await;
            });
        }
    }
}

fn monotonic_micros() -> Option<u64> {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a valid writable timespec is supplied and CLOCK_MONOTONIC is supported on Linux.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return None;
    }
    Some(u64::try_from(time.tv_sec).ok()?.checked_mul(1_000_000)? + u64::try_from(time.tv_nsec).ok()? / 1_000)
}

/// Must run off the Tauri main thread. Requests contain key codes only, never text.
pub(super) fn send(keys: &[(u32, bool)]) -> Attempt {
    static WORKER: OnceLock<Option<tokio::sync::mpsc::UnboundedSender<Request>>> = OnceLock::new();
    let Some(tx) = WORKER.get_or_init(start_worker) else { return Attempt::Unavailable };
    let cancelled = Arc::new(AtomicBool::new(false));
    let (reply, rx) = mpsc::channel();
    let request =
        Request { keys: keys.to_vec(), expires: Instant::now() + OPEN_LIMIT, cancelled: cancelled.clone(), reply };
    if tx.send(request).is_err() {
        return Attempt::Unavailable;
    }
    match rx.recv_timeout(REPLY_LIMIT) {
        Ok(attempt) => attempt,
        Err(_) => {
            cancelled.store(true, Ordering::SeqCst);
            // A late reply might mean input was sent. Never replay through ydotool.
            Attempt::Uncertain
        }
    }
}

fn start_worker() -> Option<tokio::sync::mpsc::UnboundedSender<Request>> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Request>();
    super::portal::spawn(move || async move {
        let mut client: Option<Client> = None;
        let mut refused = false;
        loop {
            let request = if let Some(active) = client.as_mut() {
                tokio::select! {
                    biased;
                    event = active.events.next() => {
                        if active.handle(event).is_err() { client = None; refused = true; }
                        continue;
                    }
                    request = rx.recv() => request,
                }
            } else {
                rx.recv().await
            };
            let Some(request) = request else { break };
            if request.cancelled.load(Ordering::SeqCst) || Instant::now() >= request.expires {
                let _ = request.reply.send(Attempt::Unavailable);
                continue;
            }
            if client.is_none() && !refused {
                client = tokio::time::timeout_at(tokio::time::Instant::from_std(request.expires), Client::open())
                    .await
                    .ok()
                    .and_then(Result::ok);
                refused = client.is_none();
            }
            let attempt = match client.as_mut() {
                Some(active) if !request.cancelled.load(Ordering::SeqCst) && Instant::now() < request.expires => {
                    active.send(&request.keys)
                }
                _ => Attempt::Unavailable,
            };
            if matches!(attempt, Attempt::Uncertain) {
                client = None;
                refused = true;
            }
            let _ = request.reply.send(attempt);
        }
        drop(client);
    })
    .ok()?;
    Some(tx)
}

struct Client {
    _session: SessionGuard,
    connection: Connection,
    events: EiConvertEventStream,
    keyboard: Option<Device>,
    sequence: u32,
}

impl Client {
    async fn open() -> Result<Self, ()> {
        super::portal::register_host_app().await.map_err(|_| ())?;
        let portal = RemoteDesktop::new().await.map_err(|_| ())?;
        let session = SessionGuard(Some(portal.create_session(CreateSessionOptions::default()).await.map_err(|_| ())?));
        let opened = async {
            let session = session.0.as_ref().ok_or(())?;
            portal
                .select_devices(
                    session,
                    SelectDevicesOptions::default()
                        .set_devices(Some(DeviceType::Keyboard.into()))
                        .set_persist_mode(PersistMode::DoNot),
                )
                .await
                .map_err(|_| ())?
                .response()
                .map_err(|_| ())?;
            let selected = portal
                .start(session, None, StartOptions::default())
                .await
                .map_err(|_| ())?
                .response()
                .map_err(|_| ())?;
            if !selected.devices().contains(DeviceType::Keyboard) {
                return Err(());
            }
            let fd = portal.connect_to_eis(session, ConnectToEISOptions::default()).await.map_err(|_| ())?;
            let context = ei::Context::new(std::os::unix::net::UnixStream::from(fd)).map_err(|_| ())?;
            context.handshake_tokio("Opit Speech to Text", ei::handshake::ContextType::Sender).await.map_err(|_| ())
        }
        .await;
        let (connection, events) = opened?;
        let mut client = Self { _session: session, connection, events, keyboard: None, sequence: 0 };
        while client.keyboard.is_none() {
            let event = client.events.next().await;
            client.handle(event)?;
        }
        Ok(client)
    }

    fn handle(&mut self, event: Option<Result<EiEvent, reis::Error>>) -> Result<(), ()> {
        match event.ok_or(())?.map_err(|_| ())? {
            EiEvent::SeatAdded(event) => {
                event.seat.bind_capabilities(DeviceCapability::Keyboard.into());
                self.connection.flush().map_err(|_| ())?;
            }
            EiEvent::DeviceResumed(event) if event.device.has_capability(DeviceCapability::Keyboard) => {
                self.keyboard = Some(event.device);
            }
            EiEvent::DevicePaused(event) if self.keyboard.as_ref() == Some(&event.device) => self.keyboard = None,
            EiEvent::DeviceRemoved(event) if self.keyboard.as_ref() == Some(&event.device) => self.keyboard = None,
            EiEvent::Disconnected(_) => return Err(()),
            _ => {}
        }
        Ok(())
    }

    fn send(&mut self, keys: &[(u32, bool)]) -> Attempt {
        let Some(device) = self.keyboard.as_ref() else { return Attempt::Unavailable };
        let Some(keyboard) = device.interface::<ei::Keyboard>() else { return Attempt::Unavailable };
        let Some(time) = monotonic_micros() else { return Attempt::Unavailable };
        self.sequence = self.sequence.wrapping_add(1);
        let serial = self.connection.serial();
        device.device().start_emulating(serial, self.sequence);
        for &(code, down) in keys {
            keyboard.key(code, if down { ei::keyboard::KeyState::Press } else { ei::keyboard::KeyState::Released });
            device.device().frame(serial, time);
        }
        device.device().stop_emulating(serial);
        if self.connection.flush().is_ok() { Attempt::Sent } else { Attempt::Uncertain }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ei_timestamps_use_the_system_monotonic_clock() {
        let first = monotonic_micros().unwrap();
        assert!(first > 0);
        assert!(monotonic_micros().unwrap() >= first);
    }
}
