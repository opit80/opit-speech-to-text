//! WinRT transport and WASAPI session adapters, constructed only on the media worker.

use std::time::{Duration, Instant};

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Win32::Foundation::{ERROR_TIMEOUT, S_FALSE};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::{Error, Interface, Result, RuntimeType};
use windows_future::{AsyncStatus, IAsyncOperation};

use crate::platform::media::{Backend, Playback, PlaybackSession, QueuedMedia, VolumeSession};

/// No native handles cross this constructor or the platform seam.
pub fn new() -> impl crate::platform::MediaControl {
    QueuedMedia::new(NativeBackend::new)
}

const OPERATION_TIMEOUT: Duration = Duration::from_millis(300);

fn wait_operation<T: RuntimeType + 'static>(operation: IAsyncOperation<T>) -> Result<T> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        if operation.Status()? != AsyncStatus::Started {
            return operation.GetResults();
        }
        if Instant::now() >= deadline {
            let _ = operation.Cancel();
            tracing::warn!("media operation timed out");
            return Err(Error::from_hresult(ERROR_TIMEOUT.to_hresult()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct NativeBackend {
    _apartment: Apartment,
}

impl NativeBackend {
    fn new() -> Option<Self> {
        // RoInitialize also initializes COM. The dedicated thread owns all native interfaces.
        if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
            tracing::warn!("media apartment unavailable");
            return None;
        }
        Some(Self { _apartment: Apartment })
    }
}

impl Backend for NativeBackend {
    fn playback_sessions(&self) -> Vec<Box<dyn PlaybackSession>> {
        let sessions = Manager::RequestAsync().and_then(wait_operation).and_then(|manager| manager.GetSessions());
        let Ok(sessions) = sessions else {
            tracing::warn!("media session manager unavailable");
            return vec![];
        };
        (0..sessions.Size().unwrap_or(0))
            .filter_map(|index| sessions.GetAt(index).ok())
            .map(|session| Box::new(Transport(session)) as Box<dyn PlaybackSession>)
            .collect()
    }

    fn volume_sessions(&self) -> Vec<Box<dyn VolumeSession>> {
        // SAFETY: initialized MTA; every interface is used and dropped on this thread.
        let result = unsafe {
            let enumerator: Result<IMMDeviceEnumerator> = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL);
            enumerator.and_then(|enumerator| enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE))
        };
        let Ok(devices) = result else {
            tracing::warn!("audio session manager unavailable");
            return vec![];
        };
        let mut volumes: Vec<Box<dyn VolumeSession>> = Vec::new();
        unsafe {
            for index in 0..devices.GetCount().unwrap_or(0) {
                let Ok(device) = devices.Item(index) else { continue };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
                for index in 0..sessions.GetCount().unwrap_or(0) {
                    let Ok(session) = sessions.GetSession(index) else { continue };
                    let Ok(control) = session.cast::<IAudioSessionControl2>() else { continue };
                    // S_FALSE specifically means a non-system session; unknown failures are skipped.
                    if control.IsSystemSoundsSession() != S_FALSE
                        || !control.GetProcessId().is_ok_and(|pid| pid != 0 && pid != std::process::id())
                    {
                        continue;
                    }
                    if let Ok(volume) = session.cast::<ISimpleAudioVolume>() {
                        volumes.push(Box::new(Volume(volume)));
                    }
                }
            }
        }
        volumes
    }
}

struct Transport(Session);

impl PlaybackSession for Transport {
    fn playback(&self) -> Playback {
        match self.0.GetPlaybackInfo().and_then(|info| info.PlaybackStatus()) {
            Ok(PlaybackStatus::Playing) => Playback::Playing,
            Ok(PlaybackStatus::Paused) => Playback::Paused,
            _ => Playback::Other,
        }
    }

    fn pause(&self) -> bool {
        match self.0.TryPauseAsync().and_then(wait_operation) {
            Ok(paused) => paused,
            // A timed-out request may already have changed playback. Retain it if so.
            Err(_) => self.playback() == Playback::Paused,
        }
    }

    fn play(&self) {
        let _ = self.0.TryPlayAsync().and_then(wait_operation);
    }
}

struct Volume(ISimpleAudioVolume);

impl VolumeSession for Volume {
    fn volume(&self) -> Option<f32> {
        unsafe { self.0.GetMasterVolume().ok() }
    }

    fn set_volume(&self, volume: f32) -> bool {
        unsafe { self.0.SetMasterVolume(volume, std::ptr::null()).is_ok() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn async_operations_time_out_instead_of_waiting_for_a_hung_session() {
        // Construct and poll on an initialized apartment, just like production.
        let _apartment = NativeBackend::new().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let operation = IAsyncOperation::<bool>::spawn(move || {
            let _ = rx.recv_timeout(Duration::from_secs(2));
            Ok(true)
        });
        let started = Instant::now();
        assert!(wait_operation(operation).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = tx.send(());
    }
}
