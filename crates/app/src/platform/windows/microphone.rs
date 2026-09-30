//! Microphone capture with cpal (WASAPI on Windows).
//!
//! `cpal::Stream` is not `Send` on every backend, so each capture owns a dedicated thread that
//! opens the device, builds and plays the stream, then parks on a stop channel. Everything cpal
//! touches stays on that thread; the caller only sees channels and a shared sample buffer.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Host, SampleFormat, SizedSample, Stream, StreamConfig};
use opit_core::audio::Recording;

use crate::platform::{Capture, CaptureEvent, CaptureSink, MicError, Microphone, Started};

/// How long `start` waits for the capture thread to open the device.
pub const START_TIMEOUT: Duration = Duration::from_secs(3);
/// Minimum gap between two `CaptureEvent::Level` events.
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(50);

/// cpal-backed [`Microphone`].
#[derive(Debug, Clone)]
pub struct CpalMicrophone {
    start_timeout: Duration,
}

impl Default for CpalMicrophone {
    fn default() -> Self {
        Self { start_timeout: START_TIMEOUT }
    }
}

impl CpalMicrophone {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Microphone for CpalMicrophone {
    fn devices(&self) -> Vec<String> {
        let host = cpal::default_host();
        let default = host.default_input_device().as_ref().and_then(device_name);
        let all = match host.input_devices() {
            Ok(devices) => devices.filter_map(|d| device_name(&d)).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "listing input devices failed");
                Vec::new()
            }
        };
        order_device_names(default, all)
    }

    fn start(&self, device: Option<&str>, sink: CaptureSink) -> Result<Started, MicError> {
        let requested = device.map(str::to_owned);
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Opened, MicError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let thread_buffer = Arc::clone(&buffer);
        let thread = thread::Builder::new()
            .name("opit-mic".into())
            .spawn(move || capture_thread(requested, sink, thread_buffer, ready_tx, stop_rx))
            .map_err(|e| MicError::Open(format!("could not start the capture thread: {e}")))?;

        let opened = match ready_rx.recv_timeout(self.start_timeout) {
            Ok(Ok(opened)) => opened,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(RecvTimeoutError::Timeout) => {
                // The thread exits on its own once `stop_tx` is dropped.
                drop(stop_tx);
                return Err(MicError::Open("the microphone did not start in time".into()));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                return Err(MicError::Open("the capture thread stopped unexpectedly".into()));
            }
        };

        if opened.fell_back {
            tracing::warn!(device = %opened.device, "requested microphone unavailable; using the default");
        }
        tracing::info!(
            device = %opened.device,
            sample_rate = opened.sample_rate,
            channels = opened.channels,
            "microphone started"
        );
        let capture = CpalCapture {
            stop: Some(stop_tx),
            thread: Some(thread),
            buffer,
            sample_rate: opened.sample_rate,
            channels: opened.channels,
        };
        Ok(Started { capture: Box::new(capture), device: opened.device, fell_back: opened.fell_back })
    }
}

/// A running capture. Dropping it stops the stream and discards the audio.
struct CpalCapture {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

impl CpalCapture {
    fn stop_and_join(&mut self) {
        // Dropping the sender wakes the capture thread, which drops the stream.
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Capture for CpalCapture {
    fn finish(mut self: Box<Self>) -> Recording {
        self.stop_and_join();
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap_or_else(PoisonError::into_inner));
        Recording { samples, sample_rate: self.sample_rate, channels: self.channels }
    }
}

impl Drop for CpalCapture {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// What the capture thread reports once the stream is playing.
struct Opened {
    device: String,
    fell_back: bool,
    sample_rate: u32,
    channels: u16,
}

fn capture_thread(
    requested: Option<String>,
    sink: CaptureSink,
    buffer: Arc<Mutex<Vec<f32>>>,
    ready: Sender<Result<Opened, MicError>>,
    stop: Receiver<()>,
) {
    let host = cpal::default_host();
    match open(&host, requested.as_deref(), &sink, &buffer) {
        Ok((stream, opened)) => {
            if ready.send(Ok(opened)).is_err() {
                return; // `start` already gave up; drop the stream.
            }
            // Blocks until `stop` is signalled or its sender is dropped.
            let _ = stop.recv();
            drop(stream);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
}

/// Which device to try, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Attempt {
    Named(String),
    Default,
}

/// Plans the open attempts: the named device when it exists (then the default as a fallback),
/// otherwise just the default. The bool is true when the plan already implies a fallback.
fn plan_attempts(requested: Option<&str>, available: &[String]) -> (Vec<Attempt>, bool) {
    match requested {
        Some(name) if available.iter().any(|d| d == name) => {
            (vec![Attempt::Named(name.to_owned()), Attempt::Default], false)
        }
        Some(_) => (vec![Attempt::Default], true),
        None => (vec![Attempt::Default], false),
    }
}

fn open(
    host: &Host,
    requested: Option<&str>,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<(Stream, Opened), MicError> {
    let inputs: Vec<(String, Device)> = match (requested, host.input_devices()) {
        (Some(_), Ok(devices)) => devices.filter_map(|d| device_name(&d).map(|n| (n, d))).collect(),
        _ => Vec::new(),
    };
    let names: Vec<String> = inputs.iter().map(|(n, _)| n.clone()).collect();
    let (attempts, mut fell_back) = plan_attempts(requested, &names);

    let mut last_error = MicError::NoDevice;
    for attempt in attempts {
        let device = match &attempt {
            Attempt::Named(name) => inputs.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone()),
            Attempt::Default => {
                fell_back |= requested.is_some();
                default_device(host)
            }
        };
        let Some(device) = device else { continue };
        let name = device_name(&device).unwrap_or_else(|| "Unknown microphone".into());
        match build_stream(&device, sink, buffer) {
            Ok((stream, config)) => {
                // Frames are downmixed in the callback, so the buffer is always mono.
                let opened = Opened { device: name, fell_back, sample_rate: config.sample_rate, channels: 1 };
                return Ok((stream, opened));
            }
            Err(e) => {
                tracing::warn!(device = %name, error = %e, "opening microphone failed");
                last_error = MicError::Open(e);
            }
        }
    }
    Err(last_error)
}

/// The system default input device, resolved to the concrete endpoint so the stream stays on
/// that device (WASAPI's "follow the default" streams report `StreamInvalidated` when the default
/// changes mid-recording).
fn default_device(host: &Host) -> Option<Device> {
    let default = host.default_input_device()?;
    default.id().ok().and_then(|id| host.device_by_id(&id)).or(Some(default))
}

fn device_name(device: &Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_owned()).filter(|n| !n.is_empty())
}

/// Default first, then the rest in enumeration order, without duplicates.
fn order_device_names(default: Option<String>, all: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(all.len() + 1);
    for name in default.into_iter().chain(all) {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn build_stream(
    device: &Device,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<(Stream, StreamConfig), String> {
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_typed::<f32>(device, config, sink, buffer),
        SampleFormat::F64 => build_typed::<f64>(device, config, sink, buffer),
        SampleFormat::I8 => build_typed::<i8>(device, config, sink, buffer),
        SampleFormat::I16 => build_typed::<i16>(device, config, sink, buffer),
        SampleFormat::I24 => build_typed::<cpal::I24>(device, config, sink, buffer),
        SampleFormat::I32 => build_typed::<i32>(device, config, sink, buffer),
        SampleFormat::U8 => build_typed::<u8>(device, config, sink, buffer),
        SampleFormat::U16 => build_typed::<u16>(device, config, sink, buffer),
        SampleFormat::U32 => build_typed::<u32>(device, config, sink, buffer),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, config))
}

fn build_typed<T>(
    device: &Device,
    config: StreamConfig,
    sink: &CaptureSink,
    buffer: &Arc<Mutex<Vec<f32>>>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let data_sink = Arc::clone(sink);
    let error_sink = Arc::clone(sink);
    let buffer = Arc::clone(buffer);
    let mut meter = LevelMeter::new(LEVEL_INTERVAL);
    let mut failed = false;
    let channels = usize::from(config.channels.max(1));

    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let level = {
                let mut buf = buffer.lock().unwrap_or_else(PoisonError::into_inner);
                let start = buf.len();
                append_mono(&mut buf, data, channels);
                meter.push(&buf[start..], Instant::now())
            };
            if let Some(level) = level {
                data_sink(CaptureEvent::Level(level));
            }
        },
        move |err: cpal::Error| {
            if is_fatal(err.kind()) && !failed {
                failed = true;
                tracing::error!(error = %err, "microphone stream failed");
                error_sink(CaptureEvent::Failed(err.to_string()));
            } else {
                tracing::debug!(error = %err, "microphone stream warning");
            }
        },
        None,
    )
}

/// Appends interleaved `data` as mono (channel average). Mono in memory halves the RAM of a
/// long stereo recording: 600 s at 48 kHz is 115 MB instead of 230 MB.
fn append_mono<T>(buf: &mut Vec<f32>, data: &[T], channels: usize)
where
    T: SizedSample,
    f32: FromSample<T>,
{
    buf.extend(
        data.chunks_exact(channels)
            .map(|frame| frame.iter().map(|&s| s.to_sample::<f32>()).sum::<f32>() / channels as f32),
    );
}

/// Xruns, reroutes and real-time refusals leave the stream running.
fn is_fatal(kind: ErrorKind) -> bool {
    !matches!(kind, ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied)
}

/// Accumulates energy and yields an RMS level at most once per `interval`.
#[derive(Debug)]
struct LevelMeter {
    interval: Duration,
    last_emit: Option<Instant>,
    sum_squares: f64,
    count: usize,
}

impl LevelMeter {
    fn new(interval: Duration) -> Self {
        Self { interval, last_emit: None, sum_squares: 0.0, count: 0 }
    }

    /// Adds samples; returns the RMS (0..1) of everything since the last emit when due.
    fn push(&mut self, samples: &[f32], now: Instant) -> Option<f32> {
        for &s in samples {
            let s = if s.is_finite() { f64::from(s.clamp(-1.0, 1.0)) } else { 0.0 };
            self.sum_squares += s * s;
        }
        self.count += samples.len();
        let due = self.last_emit.is_none_or(|last| now.duration_since(last) >= self.interval);
        if !due || self.count == 0 {
            return None;
        }
        let rms = (self.sum_squares / self.count as f64).sqrt() as f32;
        self.last_emit = Some(now);
        self.sum_squares = 0.0;
        self.count = 0;
        Some(rms.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn order_puts_default_first_and_dedupes() {
        let out = order_device_names(Some("B".into()), names(&["A", "B", "C", "A"]));
        assert_eq!(out, names(&["B", "A", "C"]));
        assert_eq!(order_device_names(None, names(&["A", "A"])), names(&["A"]));
        assert_eq!(order_device_names(Some("X".into()), vec![]), names(&["X"]));
    }

    #[test]
    fn plan_uses_named_device_then_default() {
        let available = names(&["Mic A", "Mic B"]);
        let (plan, fell_back) = plan_attempts(Some("Mic B"), &available);
        assert_eq!(plan, vec![Attempt::Named("Mic B".into()), Attempt::Default]);
        assert!(!fell_back);
    }

    #[test]
    fn plan_falls_back_when_named_device_is_missing() {
        let (plan, fell_back) = plan_attempts(Some("Gone"), &names(&["Mic A"]));
        assert_eq!(plan, vec![Attempt::Default]);
        assert!(fell_back);
    }

    #[test]
    fn plan_without_request_uses_default() {
        let (plan, fell_back) = plan_attempts(None, &[]);
        assert_eq!(plan, vec![Attempt::Default]);
        assert!(!fell_back);
    }

    #[test]
    fn meter_throttles_to_interval() {
        let t0 = Instant::now();
        let mut meter = LevelMeter::new(Duration::from_millis(50));
        assert_eq!(meter.push(&[0.5; 4], t0), Some(0.5));
        assert_eq!(meter.push(&[1.0; 4], t0 + Duration::from_millis(10)), None);
        assert_eq!(meter.push(&[1.0; 4], t0 + Duration::from_millis(49)), None);
        // Everything since the last emit counts: 8 samples of 1.0 and 8 of 0.0.
        let level = meter.push(&[0.0; 8], t0 + Duration::from_millis(50)).unwrap();
        assert!((level - 0.5_f32.sqrt()).abs() < 1e-6, "{level}");
    }

    #[test]
    fn meter_sanitizes_and_skips_empty_windows() {
        let t0 = Instant::now();
        let mut meter = LevelMeter::new(Duration::from_millis(50));
        assert_eq!(meter.push(&[], t0), None);
        assert_eq!(meter.push(&[f32::NAN, 4.0], t0), Some(0.5_f32.sqrt()));
    }

    #[test]
    fn frames_are_downmixed_to_mono() {
        let mut buf = vec![0.5];
        append_mono(&mut buf, &[0.2_f32, 0.4, -1.0, 1.0], 2);
        assert_eq!(buf.len(), 3);
        assert!((buf[1] - 0.3).abs() < 1e-6 && buf[2] == 0.0, "{buf:?}");
        let mut ints = Vec::new();
        append_mono(&mut ints, &[i16::MAX, 0, i16::MIN], 1);
        assert!((ints[0] - 1.0).abs() < 1e-3 && ints[1] == 0.0 && ints[2] == -1.0, "{ints:?}");
    }

    #[test]
    fn only_hard_errors_are_fatal() {
        assert!(!is_fatal(ErrorKind::Xrun));
        assert!(!is_fatal(ErrorKind::DeviceChanged));
        assert!(!is_fatal(ErrorKind::RealtimeDenied));
        assert!(is_fatal(ErrorKind::DeviceNotAvailable));
        assert!(is_fatal(ErrorKind::StreamInvalidated));
    }

    /// Records one second from the default microphone. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "needs a real microphone"]
    fn smoke_record_one_second() {
        let mic = CpalMicrophone::new();
        let devices = mic.devices();
        println!("devices: {devices:?}");
        let peak = Arc::new(Mutex::new(0.0_f32));
        let events = Arc::new(Mutex::new(0_usize));
        let (p, n) = (Arc::clone(&peak), Arc::clone(&events));
        let sink: CaptureSink = Arc::new(move |event| {
            if let CaptureEvent::Level(level) = event {
                let mut peak = p.lock().unwrap();
                *peak = peak.max(level);
                *n.lock().unwrap() += 1;
            }
        });
        let started = mic.start(None, Arc::clone(&sink)).expect("default mic");
        println!("opened {:?} fell_back={}", started.device, started.fell_back);
        std::thread::sleep(Duration::from_secs(1));
        let rec = started.capture.finish();
        println!(
            "sample_rate={} channels={} len={} duration_ms={} level_events={} peak_level={:.4}",
            rec.sample_rate,
            rec.channels,
            rec.samples.len(),
            rec.duration_ms(),
            events.lock().unwrap(),
            peak.lock().unwrap()
        );
        assert!(rec.duration_ms() > 700, "captured too little audio");

        for name in &devices {
            let t = Instant::now();
            let started = mic.start(Some(name), Arc::clone(&sink)).expect("named mic");
            println!("named {name:?} -> {:?} fell_back={} in {:?}", started.device, started.fell_back, t.elapsed());
            assert_eq!((&started.device, started.fell_back), (name, false));
            drop(started.capture); // cancel
        }

        let missing = mic.start(Some("No Such Microphone 123"), sink).expect("fallback");
        assert!(missing.fell_back);
        println!("fallback opened {:?}", missing.device);
        drop(missing.capture); // cancel
    }
}
