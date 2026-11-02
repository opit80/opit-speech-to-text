//! Per-stream PulseAudio/pipewire-pulse ducking, exclusively on the media worker.
//! No stream titles or arbitrary properties are retained or logged.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use opit_core::config::MediaMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::platform::MediaControl;

const APP_ID: &str = "io.github.opit80.opit-speech-to-text";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_STREAMS: usize = 128;
const MAX_VOLUME: u32 = 0x7fff_ffff;
static AVAILABLE: AtomicBool = AtomicBool::new(false);

pub(crate) fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// Nonblocking pipe reads avoid both pipe-capacity deadlocks and an unbounded reader join.
/// Helpers never inherit stdin, stderr or the GUI's stdout. All calls have a deadline.
fn helper(program: &str, args: &[String], deadline: Instant) -> Result<Vec<u8>, ()> {
    let deadline = deadline.min(Instant::now() + Duration::from_millis(750));
    if Instant::now() >= deadline {
        return Err(());
    }
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let result = (|| {
        let mut stdout = child.stdout.take().ok_or(())?;
        let fd = stdout.as_raw_fd();
        // This pipe is owned only by this worker; no other reader shares its flags.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(());
        }
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 8192];
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    if let Some(status) = child.try_wait().map_err(|_| ())? {
                        return if status.success() { Ok(bytes) } else { Err(()) };
                    }
                }
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > MAX_BYTES {
                        return Err(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(()),
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

trait Pulse {
    fn call(&mut self, args: &[String], deadline: Instant) -> Result<Vec<u8>, ()>;
    fn snapshot(&mut self, deadline: Instant) -> Result<(String, ParsedStreams), ()> {
        let info: Value =
            serde_json::from_slice(&self.call(&strings(&["--format=json", "info"]), deadline)?).map_err(|_| ())?;
        let server = info["cookie"].as_str().filter(|s| !s.is_empty()).ok_or(())?.to_owned();
        let bytes = self.call(&strings(&["--format=json", "list", "sink-inputs"]), deadline)?;
        let clients = self.call(&strings(&["--format=json", "list", "clients"]), deadline)?;
        Ok((server, parse_streams(&bytes, &clients, process_identity, std::process::id())?))
    }
}
struct Pactl;
impl Pulse for Pactl {
    fn call(&mut self, args: &[String], deadline: Instant) -> Result<Vec<u8>, ()> {
        helper("pactl", args, deadline)
    }
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    index: u32,
    client: String,
    pid: Option<u32>,
    process_start: Option<u64>,
    serial: Option<String>,
    channels: Vec<String>,
}
#[derive(Clone)]
struct Stream {
    identity: Identity,
    volume: Vec<u32>,
    own: bool,
}

fn number(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok()).or_else(|| value.as_str()?.parse().ok())
}

#[derive(Clone, Default)]
struct ParsedStreams {
    streams: Vec<Stream>,
    seen: usize,
    invalid_identity: usize,
    invalid_channels: usize,
}

fn parse_streams(
    bytes: &[u8],
    clients: &[u8],
    process: impl Fn(u32) -> Option<(u32, u64)>,
    own_pid: u32,
) -> Result<ParsedStreams, ()> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let values = value.as_array().ok_or(())?;
    let clients: Value = serde_json::from_slice(clients).map_err(|_| ())?;
    let clients = clients.as_array().ok_or(())?;
    let mut parsed = ParsedStreams { seen: values.len(), ..Default::default() };
    for value in values {
        let props = &value["properties"];
        let Some((index, client)) = number(&value["index"]).zip(number(&value["client"])) else {
            parsed.invalid_identity += 1;
            continue;
        };
        let client_props = clients.iter().find(|c| number(&c["index"]) == Some(client)).map(|c| &c["properties"]);
        let client_pid =
            client_props.and_then(|p| number(&p["application.process.id"]).or_else(|| number(&p["pipewire.sec.pid"])));
        let pid = number(&props["application.process.id"]).or(client_pid);
        let process_info = pid.and_then(&process);
        let serial = props["object.serial"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        // Unattributed playback is unlikely to be ours. Duck it when a server serial
        // still guards index/client reuse; restore also checks channels, cookie and volumes.
        // A PID without readable /proc metadata uses the same serial-backed safeguard.
        if process_info.is_none() && serial.is_none() {
            parsed.invalid_identity += 1;
            continue;
        }
        let channel_values = (|| {
            let channels: Vec<String> = value["channel_map"].as_str()?.split(',').map(str::to_owned).collect();
            if channels.is_empty() || channels.len() > 32 || channels.iter().any(String::is_empty) {
                return None;
            }
            // JSON objects may be sorted by serde; channel_map is the authoritative order.
            let volume = channels
                .iter()
                .map(|name| number(&value["volume"][name]["value"]).filter(|v| *v <= MAX_VOLUME))
                .collect::<Option<Vec<_>>>()?;
            Some((channels, volume))
        })();
        let Some((channels, volume)) = channel_values else {
            parsed.invalid_channels += 1;
            continue;
        };
        let client_info = if client_pid == pid { process_info } else { client_pid.and_then(&process) };
        let own = pid == Some(own_pid)
            || client_pid == Some(own_pid)
            || process_info.is_some_and(|(parent, _)| parent == own_pid)
            || client_info.is_some_and(|(parent, _)| parent == own_pid)
            || props["application.id"].as_str() == Some(APP_ID)
            || client_props.is_some_and(|p| p["application.id"].as_str() == Some(APP_ID));
        parsed.streams.push(Stream {
            identity: Identity {
                index,
                client: client.to_string(),
                pid,
                process_start: process_info.map(|(_, start)| start),
                serial,
                channels,
            },
            volume,
            own,
        });
    }
    Ok(parsed)
}

fn process_identity(pid: u32) -> Option<(u32, u64)> {
    // comm may contain spaces/parentheses; fields after the final ')' start at state (#3).
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}

fn scaled(volume: &[u32], percent: u8) -> Vec<u32> {
    volume.iter().map(|v| ((u64::from(*v) * u64::from(percent.min(100)) + 50) / 100) as u32).collect()
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    identity: Identity,
    original: Vec<u32>,
    ducked: Vec<u32>,
}
#[derive(Default, Serialize, Deserialize)]
struct Record {
    version: u8,
    boot: String,
    server: String,
    entries: Vec<Entry>,
}
impl Record {
    fn decode(bytes: &[u8]) -> Result<Self, ()> {
        let record: Self = serde_json::from_slice(bytes).map_err(|_| ())?;
        if record.version != 1
            || record.entries.len() > MAX_STREAMS
            || record.boot.is_empty()
            || record.server.is_empty()
            || record.entries.iter().any(|e| {
                e.identity.channels.is_empty()
                    || e.identity.channels.len() > 32
                    || e.original.len() != e.identity.channels.len()
                    || e.ducked.len() != e.original.len()
                    || e.original.iter().chain(&e.ducked).any(|v| *v > MAX_VOLUME)
            })
        {
            return Err(());
        }
        Ok(record)
    }
}

/// Compare exact channel values. Detectable user edits and reused/vanished streams are retired.
/// A change away and back between snapshots cannot be distinguished from no change.
fn restore_plan<'a>(record: &'a Record, boot: &str, server: &str, streams: &[Stream]) -> Vec<&'a Entry> {
    if record.boot != boot || record.server != server {
        return Vec::new();
    }
    record
        .entries
        .iter()
        .filter(|entry| streams.iter().any(|s| !s.own && s.identity == entry.identity && s.volume == entry.ducked))
        .collect()
}

fn write_record(path: &Path, record: &Record) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    std::fs::create_dir_all(parent).map_err(|_| ())?;
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&serde_json::to_vec(record).map_err(|_| ())?).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    std::fs::rename(&temporary, path).map_err(|_| ())?;
    std::fs::File::open(parent).and_then(|dir| dir.sync_all()).map_err(|_| ())
}
fn read_record(path: &Path) -> Result<Record, ()> {
    let file = std::fs::File::open(path).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| ())?;
    if bytes.len() > MAX_BYTES {
        return Err(());
    }
    Record::decode(&bytes)
}

struct Duck<P> {
    pulse: P,
    path: PathBuf,
    boot: String,
    record: Record,
    warned: bool,
}
impl<P: Pulse> Duck<P> {
    fn warn(&mut self) {
        AVAILABLE.store(false, Ordering::Relaxed);
        if !self.warned {
            tracing::warn!("Linux media ducking unavailable or incomplete; pending restoration will be retried");
            self.warned = true;
        }
    }
    fn set(&mut self, entry: &Entry, volume: &[u32], deadline: Instant) -> Result<(), ()> {
        let mut args = vec!["set-sink-input-volume".into(), entry.identity.index.to_string()];
        args.extend(volume.iter().map(u32::to_string));
        self.pulse.call(&args, deadline).map(|_| ())
    }
    fn restore(&mut self) -> Result<(), ()> {
        if self.record.entries.is_empty() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let (server, streams) = self.pulse.snapshot(deadline)?;
        let plan: Vec<Entry> =
            restore_plan(&self.record, &self.boot, &server, &streams.streams).into_iter().cloned().collect();
        let mut failed = Vec::new();
        for entry in plan {
            if self.set(&entry, &entry.original, deadline).is_err() {
                failed.push(entry);
            }
        }
        self.record.entries = failed;
        // Failed commands retain their intent on disk; successful restores can be repeated safely.
        write_record(&self.path, &self.record)?;
        if self.record.entries.is_empty() {
            std::fs::remove_file(&self.path).map_err(|_| ())?;
            Ok(())
        } else {
            Err(())
        }
    }
    fn engage(&mut self, percent: u8) -> Result<(), ()> {
        // Never overwrite unresolved recovery from an earlier recording/crash.
        self.restore()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let (server, streams) = self.pulse.snapshot(deadline)?;
        self.record = Record { version: 1, boot: self.boot.clone(), server, entries: Vec::new() };
        let mut own = 0;
        let mut unchanged = 0;
        let mut limit = 0;
        let mut ducked_count = 0;
        let result = (|| {
            for stream in &streams.streams {
                if stream.own {
                    own += 1;
                    continue;
                }
                let ducked = scaled(&stream.volume, percent);
                if ducked == stream.volume {
                    unchanged += 1;
                    continue;
                }
                if self.record.entries.len() >= MAX_STREAMS {
                    limit += 1;
                    continue;
                }
                let entry = Entry { identity: stream.identity.clone(), original: stream.volume.clone(), ducked };
                self.record.entries.push(entry.clone());
                // Write-ahead intent also covers termination between set and its reply.
                write_record(&self.path, &self.record)?;
                self.set(&entry, &entry.ducked, deadline)?;
                ducked_count += 1;
            }
            Ok(())
        })();
        let skipped = streams.invalid_identity + streams.invalid_channels + own + unchanged + limit;
        tracing::debug!(
            seen = streams.seen,
            ducked = ducked_count,
            skipped,
            skipped_identity = streams.invalid_identity,
            skipped_channels = streams.invalid_channels,
            skipped_own = own,
            skipped_unchanged = unchanged,
            skipped_limit = limit,
            failed_or_unattempted = streams.seen - ducked_count - skipped,
            "Linux media ducking snapshot"
        );
        result
    }

    fn initialize(&mut self) -> Result<(), ()> {
        match std::fs::metadata(&self.path) {
            Ok(_) => self.record = read_record(&self.path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
        self.restore()?;
        self.pulse.snapshot(Instant::now() + Duration::from_secs(2))?;
        // Verify persistence before advertising availability or changing any stream.
        self.record = Record { version: 1, boot: self.boot.clone(), server: "probe".into(), entries: Vec::new() };
        write_record(&self.path, &self.record)?;
        std::fs::remove_file(&self.path).map_err(|_| ())?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum MediaCommand {
    Engage(MediaMode, u8),
    Release,
    Shutdown,
}

pub(crate) struct LinuxMedia {
    tx: mpsc::Sender<MediaCommand>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl LinuxMedia {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("opit-linux-media".into())
            .spawn(move || {
                let mut duck = crate::startup::Paths::from_system().and_then(|paths| {
                    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
                    Some(Duck {
                        pulse: Pactl,
                        path: paths.root.join("media-duck.json"),
                        boot,
                        record: Record::default(),
                        warned: false,
                    })
                });
                let mut ready = false;
                if let Some(duck) = &mut duck {
                    ready = duck.initialize().is_ok();
                    if !ready {
                        duck.warn();
                    }
                } else {
                    tracing::warn!("Linux media ducking unavailable: recovery storage unavailable");
                }
                AVAILABLE.store(ready, Ordering::Relaxed);
                let mut paused = Vec::new();
                let mut engaged = false;
                while let Ok(command) = rx.recv() {
                    match command {
                        MediaCommand::Engage(mode, percent) if !engaged => {
                            engaged = true;
                            match mode {
                                MediaMode::Pause => paused = pause_players(),
                                MediaMode::Duck => {
                                    if let Some(duck) = &mut duck {
                                        // A startup failure must be retried before replacing any recovery intent.
                                        if (!ready && duck.initialize().is_err()) || duck.engage(percent).is_err() {
                                            ready = false;
                                            duck.warn();
                                            // Roll back partial engagement; keep failures for release/shutdown/startup.
                                            if duck.restore().is_err() {
                                                duck.warn();
                                            }
                                        } else {
                                            ready = true;
                                            AVAILABLE.store(true, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                        }
                        MediaCommand::Engage(_, _) => {}
                        MediaCommand::Release | MediaCommand::Shutdown => {
                            resume_players(&mut paused);
                            if let Some(duck) = &mut duck
                                && duck.restore().is_err()
                            {
                                duck.warn();
                            }
                            engaged = false;
                            if matches!(command, MediaCommand::Shutdown) {
                                break;
                            }
                        }
                    }
                }
                // Sender loss is also an exit path; the disk record survives failure or abrupt exit.
                resume_players(&mut paused);
                if let Some(duck) = &mut duck
                    && duck.restore().is_err()
                {
                    duck.warn();
                }
                AVAILABLE.store(false, Ordering::Relaxed);
            })
            .ok();
        Self { tx, worker: Mutex::new(worker) }
    }
}
impl MediaControl for LinuxMedia {
    fn engage(&self, mode: MediaMode, percent: u8) {
        let _ = self.tx.send(MediaCommand::Engage(mode, percent));
    }
    fn release(&self) {
        let _ = self.tx.send(MediaCommand::Release);
    }
    fn shutdown(&self) {
        let _ = self.tx.send(MediaCommand::Shutdown);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}
impl Drop for LinuxMedia {
    fn drop(&mut self) {
        // Queue cleanup without blocking the Tauri thread; normal exit explicitly joins via shutdown.
        let _ = self.tx.send(MediaCommand::Shutdown);
    }
}

fn player_status(player: &str, expected: &[u8], deadline: Instant) -> bool {
    helper("playerctl", &strings(&["--player", player, "status"]), deadline).is_ok_and(|s| s.starts_with(expected))
}
fn player_action(player: &str, action: &str, deadline: Instant) -> bool {
    helper("playerctl", &strings(&["--player", player, action]), deadline).is_ok()
}
fn pause_players() -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let Ok(output) = helper("playerctl", &strings(&["--list-all"]), deadline) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output)
        .lines()
        .filter(|player| player_status(player, b"Playing", deadline) && player_action(player, "pause", deadline))
        .map(str::to_owned)
        .collect()
}
fn resume_players(paused: &mut Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    for player in paused.drain(..) {
        if player_status(&player, b"Paused", deadline) {
            player_action(&player, "play", deadline);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stream(index: u32, volume: Vec<u32>) -> Stream {
        Stream {
            identity: Identity {
                index,
                client: "12".into(),
                pid: Some(42),
                process_start: Some(123),
                serial: Some(index.to_string()),
                channels: vec!["front-left".into(), "front-right".into()],
            },
            volume,
            own: false,
        }
    }
    fn entry(index: u32) -> Entry {
        let stream = stream(index, vec![65535, 32769]);
        Entry { identity: stream.identity, original: stream.volume.clone(), ducked: scaled(&stream.volume, 50) }
    }
    fn record(entries: Vec<Entry>) -> Record {
        Record { version: 1, boot: "boot".into(), server: "cookie".into(), entries }
    }
    fn fixture() -> Value {
        json!({"index": 19, "client": "12", "channel_map": "front-right,front-left",
            "volume": {"front-left": {"value": 12001, "value_percent": "18%"}, "front-right": {"value": 65003}},
            "properties": {"application.process.id": "42", "object.serial": "33", "media.name": "private title"}})
    }
    #[test]
    fn media_parse_preserves_exact_values_and_channel_map_order() {
        let streams = parse_streams(&serde_json::to_vec(&vec![fixture()]).unwrap(), b"[]", |_| Some((1, 123)), 7)
            .unwrap()
            .streams;
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].volume, [65003, 12001]);
        assert_eq!(streams[0].identity.index, 19);
        assert_eq!(streams[0].identity.client, "12");
        assert_eq!(streams[0].identity.channels, ["front-right", "front-left"]);
        assert!(!streams[0].own);
    }
    #[test]
    fn media_parse_excludes_own_process_feedback_child_and_app_id() {
        let bytes = serde_json::to_vec(&vec![fixture()]).unwrap();
        assert!(parse_streams(&bytes, b"[]", |_| Some((1, 123)), 42).unwrap().streams[0].own);
        assert!(parse_streams(&bytes, b"[]", |_| Some((7, 123)), 7).unwrap().streams[0].own);
        let mut value = fixture();
        value["properties"]["application.id"] = json!(APP_ID);
        let bytes = serde_json::to_vec(&vec![value]).unwrap();
        assert!(parse_streams(&bytes, b"[]", |_| Some((1, 123)), 7).unwrap().streams[0].own);
    }
    #[test]
    fn media_parse_fails_closed_on_missing_identity_and_malformed_channels() {
        for field in ["client", "channel_map", "volume"] {
            let mut value = fixture();
            value.as_object_mut().unwrap().remove(field);
            let bytes = serde_json::to_vec(&vec![value]).unwrap();
            assert!(parse_streams(&bytes, b"[]", |_| Some((1, 123)), 7).unwrap().streams.is_empty());
        }
        let mut value = fixture();
        value["volume"]["front-left"]["value"] = json!(u64::MAX);
        assert!(
            parse_streams(&serde_json::to_vec(&vec![value]).unwrap(), b"[]", |_| Some((1, 123)), 7)
                .unwrap()
                .streams
                .is_empty()
        );
        assert!(parse_streams(b"not json", b"[]", |_| None, 7).is_err());
        assert!(parse_streams(b"{}", b"[]", |_| None, 7).is_err());
        assert!(parse_streams(b"[]", b"[]", |_| None, 7).unwrap().streams.is_empty());
        assert_eq!(
            parse_streams(&serde_json::to_vec(&vec![fixture()]).unwrap(), b"[]", |_| None, 7).unwrap().streams.len(),
            1
        );
    }
    // Shape observed on the owner's PipeWire desktop: no stream PID, string client,
    // aux channel names, and process metadata on the separate client object only.
    fn pipewire_fixture() -> Value {
        json!({"index": 19, "client": "513", "channel_map": "aux0,aux1",
        "volume": {"aux0": {"value": 65003}, "aux1": {"value": 12001}},
        "properties": {
            "adapt.follower.spa-node": "", "application.name": "Spotify", "client.id": "513",
            "clock.quantum-limit": "8192", "factory.id": "7", "library.name": "audioconvert/libspa-audioconvert",
            "media.category": "Playback", "media.class": "Stream/Output/Audio", "media.name": "private title",
            "media.role": "Music", "media.type": "Audio", "module-stream-restore.id": "sink-input-by-application-name:Spotify",
            "node.async": "true", "node.autoconnect": "true", "node.loop.class": "data.rt",
            "node.loop.name": "data-loop.0", "node.name": "playback", "node.rate": "1/44100",
            "node.want-driver": "true", "object.id": "514", "object.register": "false",
            "object.serial": "700", "port.group": "stream.0", "stream.is-live": "true"
        }})
    }
    fn pipewire_clients() -> Value {
        json!([{"index": 513, "properties": {"application.process.id": "112768", "pipewire.sec.pid": "112768"}}])
    }
    fn parse_fixture(stream: Value, clients: Value, own: u32) -> ParsedStreams {
        parse_streams(
            &serde_json::to_vec(&vec![stream]).unwrap(),
            &serde_json::to_vec(&clients).unwrap(),
            |_| Some((7, 123)),
            own,
        )
        .unwrap()
    }
    #[test]
    fn media_pipewire_client_fallback_preserves_aux_channels_and_pid_precedence() {
        let parsed = parse_fixture(pipewire_fixture(), pipewire_clients(), 1);
        assert_eq!(parsed.seen, 1);
        assert_eq!(parsed.invalid_identity + parsed.invalid_channels, 0);
        let stream = &parsed.streams[0];
        assert_eq!(stream.identity.pid, Some(112768));
        assert_eq!(stream.identity.client, "513");
        assert_eq!(stream.identity.channels, ["aux0", "aux1"]);
        assert_eq!(stream.volume, [65003, 12001]);
        assert!(!stream.own);
        let mut clients = pipewire_clients();
        clients[0]["properties"].as_object_mut().unwrap().remove("application.process.id");
        assert_eq!(parse_fixture(pipewire_fixture(), clients, 1).streams[0].identity.pid, Some(112768));
        let mut stream = pipewire_fixture();
        stream["properties"]["application.process.id"] = json!(42);
        assert_eq!(parse_fixture(stream, pipewire_clients(), 1).streams[0].identity.pid, Some(42));
    }
    #[test]
    fn media_own_client_pid_parent_and_application_id_are_excluded() {
        assert!(parse_fixture(pipewire_fixture(), pipewire_clients(), 112768).streams[0].own);
        assert!(parse_fixture(pipewire_fixture(), pipewire_clients(), 7).streams[0].own);
        let mut stream = pipewire_fixture();
        stream["properties"]["application.process.id"] = json!(42);
        assert!(parse_fixture(stream, pipewire_clients(), 112768).streams[0].own);
        let mut clients = pipewire_clients();
        clients[0]["properties"] = json!({"application.id": APP_ID});
        assert!(parse_fixture(pipewire_fixture(), clients, 1).streams[0].own);
    }
    #[test]
    fn media_unattributed_serial_stream_is_ducked_and_restore_rejects_reuse() {
        let parsed = parse_fixture(pipewire_fixture(), json!([]), 1);
        let mut stream = parsed.streams[0].clone();
        assert_eq!(stream.identity.pid, None);
        assert_eq!(stream.identity.process_start, None);
        let original = record(vec![Entry {
            identity: stream.identity.clone(),
            original: stream.volume.clone(),
            ducked: scaled(&stream.volume, 50),
        }]);
        stream.volume = scaled(&stream.volume, 50);
        assert_eq!(restore_plan(&original, "boot", "cookie", &[stream.clone()]).len(), 1);
        stream.identity.serial = Some("701".into());
        assert!(restore_plan(&original, "boot", "cookie", &[stream]).is_empty());
        let mut value = pipewire_fixture();
        value["properties"].as_object_mut().unwrap().remove("object.serial");
        let parsed = parse_fixture(value, json!([]), 1);
        assert!(parsed.streams.is_empty());
        assert_eq!(parsed.invalid_identity, 1);
    }
    #[test]
    fn media_snapshot_reads_clients_once_and_accepts_server_cookie() {
        struct SnapshotPulse(Vec<Vec<String>>);
        impl Pulse for SnapshotPulse {
            fn call(&mut self, args: &[String], _: Instant) -> Result<Vec<u8>, ()> {
                self.0.push(args.to_vec());
                let value = match args.last().unwrap().as_str() {
                    "info" => json!({"cookie": "server-cookie"}),
                    "sink-inputs" => json!([pipewire_fixture()]),
                    "clients" => pipewire_clients(),
                    _ => panic!("unexpected helper"),
                };
                Ok(serde_json::to_vec(&value).unwrap())
            }
        }
        let mut pulse = SnapshotPulse(Vec::new());
        let (server, parsed) = pulse.snapshot(Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(server, "server-cookie");
        assert_eq!(parsed.streams.len(), 1);
        assert_eq!(
            pulse.0,
            [
                strings(&["--format=json", "info"]),
                strings(&["--format=json", "list", "sink-inputs"]),
                strings(&["--format=json", "list", "clients"])
            ]
        );
    }
    #[test]
    fn media_volume_scales_each_channel_without_overflow_or_amplification() {
        assert_eq!(scaled(&[65535, 32769, 0], 50), [32768, 16385, 0]);
        assert_eq!(scaled(&[100, 200], 20), [20, 40]);
        assert_eq!(scaled(&[MAX_VOLUME], 0), [0]);
        assert_eq!(scaled(&[MAX_VOLUME], 100), [MAX_VOLUME]);
        assert_eq!(scaled(&[MAX_VOLUME], 255), [MAX_VOLUME]);
    }
    #[test]
    fn media_crash_record_round_trip_is_private_and_exact() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/media-duck.json");
        let original = record(vec![entry(1), entry(2)]);
        write_record(&path, &original).unwrap();
        let loaded = read_record(&path).unwrap();
        assert!(loaded.entries == original.entries);
        assert_eq!(loaded.boot, original.boot);
        assert_eq!(loaded.server, original.server);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!path.with_extension("tmp").exists());
        assert!(!std::fs::read_to_string(path).unwrap().contains("private title"));
    }
    #[test]
    fn media_crash_record_rejects_unknown_version_and_invalid_volume_layout() {
        let mut value = serde_json::to_value(record(vec![entry(1)])).unwrap();
        value["version"] = json!(2);
        assert!(Record::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        value["version"] = json!(1);
        value["entries"][0]["ducked"] = json!([1]);
        assert!(Record::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(Record::decode(b"{}").is_err());
    }
    #[test]
    fn media_restore_plan_preserves_user_changes_and_ignores_new_or_vanished_streams() {
        let original = record(vec![entry(1), entry(2), entry(3)]);
        let unchanged = stream(1, entry(1).ducked);
        let changed = stream(2, vec![123, 456]);
        let new = stream(4, vec![321, 654]);
        let plan = restore_plan(&original, "boot", "cookie", &[unchanged, changed, new]);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].identity.index, 1);
        assert_eq!(plan[0].original, [65535, 32769]);
    }
    #[test]
    fn media_restore_plan_rejects_reused_indices_processes_channels_and_servers() {
        let original = record(vec![entry(1)]);
        let unchanged = stream(1, entry(1).ducked);
        for field in 0..6 {
            let mut changed = unchanged.clone();
            match field {
                0 => changed.identity.client = "other".into(),
                1 => changed.identity.process_start = Some(124),
                2 => changed.identity.pid = Some(43),
                3 => changed.identity.serial = Some("other".into()),
                4 => changed.identity.channels.reverse(),
                _ => changed.own = true,
            }
            assert!(restore_plan(&original, "boot", "cookie", &[changed]).is_empty());
        }
        assert!(restore_plan(&original, "other boot", "cookie", std::slice::from_ref(&unchanged)).is_empty());
        assert!(restore_plan(&original, "boot", "other cookie", &[unchanged]).is_empty());
    }

    struct FakePulse {
        streams: Vec<Stream>,
        fail: bool,
        path: PathBuf,
        sets: Vec<Vec<String>>,
    }
    impl Pulse for FakePulse {
        fn snapshot(&mut self, _deadline: Instant) -> Result<(String, ParsedStreams), ()> {
            Ok((
                "cookie".into(),
                ParsedStreams { streams: self.streams.clone(), seen: self.streams.len(), ..Default::default() },
            ))
        }
        fn call(&mut self, args: &[String], _deadline: Instant) -> Result<Vec<u8>, ()> {
            assert_eq!(args[0], "set-sink-input-volume"); // Never master-sink or transport control.
            assert!(!read_record(&self.path)?.entries.is_empty()); // Intent precedes every write.
            self.sets.push(args.to_vec());
            if self.fail {
                return Err(());
            }
            let index: u32 = args[1].parse().unwrap();
            let stream = self.streams.iter_mut().find(|s| s.identity.index == index).ok_or(())?;
            stream.volume = args[2..].iter().map(|v| v.parse().unwrap()).collect();
            Ok(Vec::new())
        }
    }
    fn duck(dir: &Path) -> Duck<FakePulse> {
        let path = dir.join("media-duck.json");
        let mut own = stream(2, vec![1234, 5678]);
        own.own = true;
        Duck {
            pulse: FakePulse {
                streams: vec![stream(1, vec![65535, 32769]), own],
                fail: false,
                path: path.clone(),
                sets: Vec::new(),
            },
            path,
            boot: "boot".into(),
            record: record(Vec::new()),
            warned: false,
        }
    }
    #[test]
    fn media_engage_restore_and_repeat_release_keep_exact_own_and_new_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let mut duck = duck(dir.path());
        duck.initialize().unwrap();
        duck.engage(50).unwrap();
        assert_eq!(duck.pulse.streams[0].volume, [32768, 16385]);
        assert_eq!(duck.pulse.streams[1].volume, [1234, 5678]);
        duck.pulse.streams.push(stream(3, vec![777, 888]));
        duck.restore().unwrap();
        duck.restore().unwrap();
        assert_eq!(duck.pulse.streams[0].volume, [65535, 32769]);
        assert_eq!(duck.pulse.streams[2].volume, [777, 888]);
        assert_eq!(duck.pulse.sets.len(), 2);
        assert!(!duck.path.exists());
    }
    #[test]
    fn media_restart_recovers_written_intent_and_retries_failed_restoration() {
        let dir = tempfile::tempdir().unwrap();
        let mut before_crash = duck(dir.path());
        before_crash.engage(50).unwrap();
        let mut restart = duck(dir.path());
        restart.pulse.streams = before_crash.pulse.streams;
        restart.pulse.fail = true;
        assert!(restart.initialize().is_err());
        assert_eq!(read_record(&restart.path).unwrap().entries.len(), 1);
        restart.pulse.fail = false;
        restart.initialize().unwrap();
        assert_eq!(restart.pulse.streams[0].volume, [65535, 32769]);
        assert!(!restart.path.exists());
    }
    #[test]
    fn media_failed_engagement_and_manual_volume_edits_are_safe_to_release() {
        let dir = tempfile::tempdir().unwrap();
        let mut duck = duck(dir.path());
        duck.pulse.fail = true;
        assert!(duck.engage(50).is_err());
        duck.restore().unwrap(); // Set failed before changing the stream.
        assert_eq!(duck.pulse.streams[0].volume, [65535, 32769]);
        duck.pulse.fail = false;
        duck.engage(50).unwrap();
        duck.pulse.streams[0].volume = vec![555, 999];
        duck.restore().unwrap();
        assert_eq!(duck.pulse.streams[0].volume, [555, 999]);
        assert!(!duck.path.exists());
    }
    #[test]
    fn media_persistence_failure_and_unreadable_recovery_prevent_ducking() {
        let dir = tempfile::tempdir().unwrap();
        let mut duck = duck(dir.path());
        std::fs::write(&duck.path, b"broken record").unwrap();
        assert!(duck.initialize().is_err());
        assert!(duck.pulse.sets.is_empty());
        std::fs::remove_file(&duck.path).unwrap();
        std::fs::create_dir(&duck.path).unwrap();
        assert!(duck.engage(50).is_err());
        assert!(duck.pulse.sets.is_empty());
    }
}
