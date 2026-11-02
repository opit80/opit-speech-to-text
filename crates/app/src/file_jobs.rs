//! Reusable app file worker. WebView commands accept only native-dialog paths;
//! authenticated local IPC supplies its own paths. Results stay in memory, so a finished result
//! keeps the shared work guard (which blocks update installation) until it is saved, copied,
//! discarded or replaced.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use opit_core::files::{self, FileCleanup, FileError, FileResult, Progress};
use opit_core::pipeline::{PipelineContext, PipelineError};
use opit_core::provider::{Profile, ProviderClient, ProviderKind, ResponseFormat};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::app_core::{AppCore, CommandError};
use crate::controller::{Providers, WorkGuard};
use crate::i18n::Lang;
use crate::providers::HttpProviders;

type Result<T> = std::result::Result<T, CommandError>;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FileOptions {
    pub language: Option<String>,
    pub cleanup: bool,
    pub speakers: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Decoding,
    Transcribing,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

impl Phase {
    fn running(self) -> bool {
        matches!(self, Self::Decoding | Self::Transcribing | Self::Cancelling)
    }
}

#[derive(Clone, Default, Serialize)]
pub struct FileState {
    pub revision: u64,
    pub name: Option<String>,
    pub phase: Phase,
    pub progress: Option<Progress>,
    pub result: Option<FileResult>,
    pub error: Option<CommandError>,
    pub options: FileOptions,
    /// A completed result exists only in memory and was not saved, copied or discarded yet.
    /// Within one revision it only ever changes from true to false.
    pub unsaved: bool,
}

/// Progress events carry no transcript. The UI retrieves a snapshot after completion.
/// Marking a result as saved or copied keeps the revision (the result is unchanged) and
/// sends an event with the same revision and `unsaved: false`.
#[derive(Clone, Serialize)]
pub struct FileEvent {
    pub revision: u64,
    pub phase: Phase,
    pub progress: Option<Progress>,
    pub unsaved: bool,
}

impl From<&FileState> for FileEvent {
    fn from(state: &FileState) -> Self {
        Self { revision: state.revision, phase: state.phase, progress: state.progress, unsaved: state.unsaved }
    }
}

#[derive(Default)]
struct Inner {
    path: Option<PathBuf>,
    state: FileState,
    cancel: Arc<AtomicBool>,
    dialog_open: bool,
    /// The work guard of a completed job whose result is unsaved.
    retained: Option<WorkGuard>,
}

impl Inner {
    fn release(&mut self) -> Option<WorkGuard> {
        self.state.unsaved = false;
        self.retained.take()
    }
}

#[derive(Default)]
pub struct FileJobs(Mutex<Inner>);

impl FileJobs {
    pub fn snapshot(&self) -> FileState {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).state.clone()
    }

    pub fn cancel(&self) {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.state.phase.running() {
            inner.cancel.store(true, Ordering::Relaxed);
            inner.state.phase = Phase::Cancelling;
            inner.state.revision += 1;
        }
    }

    fn begin(&self, options: FileOptions) -> Result<(PathBuf, Arc<AtomicBool>)> {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.state.phase.running() || inner.dialog_open {
            return Err(CommandError::new("file_busy", ""));
        }
        let path = inner.path.clone().ok_or_else(|| CommandError::new("file_pick", ""))?;
        // The new job replaces the previous result; its own guard covers the work from here.
        drop(inner.release());
        inner.cancel = Arc::new(AtomicBool::new(false));
        inner.state.revision += 1;
        inner.state.phase = Phase::Decoding;
        inner.state.result = None;
        inner.state.error = None;
        inner.state.progress = None;
        inner.state.options = options;
        Ok((path, inner.cancel.clone()))
    }

    fn progress(&self, progress: Progress) -> FileEvent {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !inner.cancel.load(Ordering::Relaxed) {
            inner.state.phase = Phase::Transcribing;
        }
        inner.state.progress = Some(progress);
        inner.state.revision += 1;
        FileEvent::from(&inner.state)
    }

    /// A completed result keeps `work` until it is saved, copied, discarded or replaced;
    /// otherwise the guard is released here.
    fn finish(&self, result: Result<FileResult>, work: Option<WorkGuard>) -> FileEvent {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        inner.state.revision += 1;
        if inner.cancel.load(Ordering::Relaxed) {
            inner.state.phase = Phase::Cancelled;
        } else {
            match result {
                Ok(result) => {
                    inner.state.phase = Phase::Completed;
                    inner.state.result = Some(result);
                    inner.state.unsaved = true;
                    inner.retained = work;
                }
                Err(error) => {
                    inner.state.phase = Phase::Failed;
                    inner.state.error = Some(error);
                }
            }
        }
        FileEvent::from(&inner.state)
    }

    /// The result of `revision` was saved or copied: it stays visible but no longer blocks
    /// update installation. Stale revisions change nothing.
    pub fn handled(&self, revision: u64) -> FileState {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.state.revision == revision {
            drop(inner.release());
        }
        inner.state.clone()
    }

    /// Drops the completed result of `revision`, keeping the picked file for another run.
    pub fn discard(&self, revision: u64) -> FileState {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.state.revision == revision && !inner.state.phase.running() && inner.state.result.is_some() {
            drop(inner.release());
            inner.state.revision += 1;
            inner.state.phase = Phase::Idle;
            inner.state.result = None;
            inner.state.progress = None;
        }
        inner.state.clone()
    }

    pub fn render(&self, format: OutputFormat, prefixes: bool, speakers: bool) -> Result<String> {
        self.render_revision(format, prefixes, speakers).map(|(text, _)| text)
    }

    fn render_revision(&self, format: OutputFormat, prefixes: bool, speakers: bool) -> Result<(String, u64)> {
        let inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let result = inner.state.result.as_ref().ok_or_else(|| CommandError::new("file_result", ""))?;
        let speakers = speakers && inner.state.options.speakers;
        let text = match format {
            OutputFormat::Txt => files::render_txt(result, prefixes, speakers),
            OutputFormat::Srt => {
                files::render_srt(result, speakers).ok_or_else(|| CommandError::new("file_timestamps", ""))?
            }
        };
        Ok((text, inner.state.revision))
    }
}

pub async fn pick(app: AppHandle) -> Result<FileState> {
    let jobs = app.state::<Arc<FileJobs>>().inner().clone();
    let lang = app.state::<Arc<AppCore>>().settings.current().lang;
    let app_events = app.clone();
    {
        let mut inner = jobs.0.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.state.phase.running() || inner.dialog_open {
            return Err(CommandError::new("file_busy", ""));
        }
        inner.dialog_open = true;
    }
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(lang.file_media_filter(), &["wav", "mp3", "flac", "ogg", "m4a", "mp4", "mkv", "webm", "opus"])
            .blocking_pick_file()
            .map(|path| path.into_path())
    })
    .await;
    let mut inner = jobs.0.lock().unwrap_or_else(PoisonError::into_inner);
    inner.dialog_open = false;
    match selected {
        Ok(Some(Ok(path))) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            let revision = inner.state.revision + 1;
            // A new file replaces the previous result.
            drop(inner.release());
            inner.path = Some(path);
            inner.state = FileState { name, revision, ..FileState::default() };
            let state = inner.state.clone();
            drop(inner);
            crate::events::emit(&app_events, crate::events::FILE_PROGRESS, FileEvent::from(&state));
            Ok(state)
        }
        Ok(None) => Ok(inner.state.clone()),
        _ => Err(CommandError::new("file_pick", "")),
    }
}

fn client(core: &AppCore, mut profile: Profile, options: &FileOptions) -> Result<crate::providers::AppProvider> {
    profile.response_format = ResponseFormat::VerboseJson;
    if let Some(language) = &options.language {
        profile.language = language.clone();
    }
    if profile.provider == ProviderKind::LocalWhisper {
        profile.api_key_ref = None;
        return Ok(crate::providers::AppProvider::Local { profile, service: core.local.clone() });
    }
    let key = profile.api_key_ref.as_deref().map(|key| core.platform.secrets.get(key)).transpose()?.flatten();
    if profile.api_key_ref.is_some() && key.as_deref().is_none_or(|k| k.trim().is_empty()) {
        return Err(CommandError {
            code: "provider",
            message: String::new(),
            kind: Some(crate::controller::ErrorKind::MissingKey),
        });
    }
    let client = match profile.provider {
        ProviderKind::OpenaiCompatible => opit_core::provider::OpenAiCompatible::new(profile, key)
            .map(|c| ProviderClient::OpenaiCompatible(c.for_file()))
            .map_err(CommandError::from),
        ProviderKind::Elevenlabs => opit_core::provider::elevenlabs::ElevenLabs::new(profile, key)
            .map(|c| ProviderClient::Elevenlabs(c.with_file_options(options.speakers)))
            .map_err(CommandError::from),
        ProviderKind::LocalWhisper => unreachable!(),
        ProviderKind::Deepgram => opit_core::provider::deepgram::Deepgram::new(profile, key)
            .map(ProviderClient::Deepgram)
            .map_err(CommandError::from),
    }?;
    Ok(crate::providers::AppProvider::Cloud(client))
}

/// Reusable app hook, independent of invoke commands. Jobs snapshot active settings at start.
pub fn start(app: AppHandle, core: Arc<AppCore>, mut options: FileOptions) -> Result<FileState> {
    if let Some(language) = &options.language {
        if language != "auto" && opit_core::provider::language::normalize(language).is_none() {
            return Err(CommandError::new("file_language", ""));
        }
        if language != "auto" {
            options.language = opit_core::provider::language::normalize(language);
        }
    }
    let settings = core.settings.current();
    let profile =
        settings.config.active_profile().cloned().ok_or_else(|| CommandError::new("config", "no active profile"))?;
    if options.speakers && profile.provider != ProviderKind::Elevenlabs {
        return Err(CommandError::new("file_speakers", ""));
    }
    let jobs = app.state::<Arc<FileJobs>>().inner().clone();
    let work = core.controller.begin_file_work().ok_or_else(|| CommandError::new("file_blocked_by_update", ""))?;
    let (path, cancel) = jobs.begin(options.clone())?;
    let initial = jobs.snapshot();
    let handle = tauri::async_runtime::handle();
    let worker_jobs = jobs.clone();
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        // The guard travels with the result; a panic drops it while unwinding.
        let result = transcribe_path(
            &core,
            settings,
            &options,
            &path,
            &cancel,
            |p| {
                crate::events::emit(&worker_app, crate::events::FILE_PROGRESS, worker_jobs.progress(p));
            },
            handle.inner(),
        );
        (result, work)
    });
    tauri::async_runtime::spawn(async move {
        let (result, work) = match worker.await {
            Ok((result, work)) => (result, Some(work)),
            Err(_) => (Err(CommandError::new("file_worker", "")), None),
        };
        crate::events::emit(&app, crate::events::FILE_PROGRESS, jobs.finish(result, work));
    });
    Ok(initial)
}

/// Runs decoding and encoding on a blocking worker, using the same engine and
/// credential/profile policy for the UI and Opit Speech to Text's CLI.
pub fn transcribe_path(
    core: &AppCore,
    settings: Arc<crate::settings::Settings>,
    options: &FileOptions,
    path: &std::path::Path,
    cancel: &AtomicBool,
    progress: impl Fn(Progress),
    runtime: &tokio::runtime::Handle,
) -> Result<FileResult> {
    let profile =
        settings.config.active_profile().cloned().ok_or_else(|| CommandError::new("config", "no active profile"))?;
    let primary = client(core, profile.clone(), options)?;
    let fallback = settings.config.fallback_for(&profile).cloned().and_then(|p| client(core, p, options).ok());
    let wants_cleanup = options.cleanup && settings.config.cleanup.active_style_id != "raw";
    let cleanup = if wants_cleanup {
        HttpProviders::new(core.platform.secrets.clone()).cleanup_client(&settings.config.cleanup).ok()
    } else {
        None
    };
    let samples = files::decode::decode(path, cancel).map_err(|e| file_error(e, settings.lang, path))?;
    let ctx = PipelineContext {
        primary: &primary,
        fallback: fallback.as_ref(),
        rules: &settings.rules,
        prompt_context: &settings.config.rules.prompt_context,
        retry_delay: opit_core::provider::retry::RETRY_DELAY,
        numbers_as_words: settings.config.rules.numbers_as_words,
    };
    let mut result = runtime
        .block_on(files::transcribe(
            &samples,
            &ctx,
            cleanup
                .as_deref()
                .map(|client| FileCleanup { client, ui_language: if settings.lang == Lang::Tr { "tr" } else { "en" } }),
            cancel,
            files::TARGET_SAMPLES,
            progress,
        ))
        .map_err(|e| file_error(e, settings.lang, path))?;
    result.cleanup_failed |= wants_cleanup && cleanup.is_none();
    Ok(result)
}

fn file_error(error: FileError, lang: Lang, path: &std::path::Path) -> CommandError {
    match error {
        FileError::Pipeline(PipelineError::Provider(error)) => error.into(),
        FileError::Decode(format) => {
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let name = match extension.as_str() {
                "wav" => "WAV",
                "mp3" => "MP3",
                "flac" => "FLAC",
                "ogg" => "OGG/Vorbis",
                "m4a" | "mp4" => "M4A/MP4",
                "mkv" => "MKV",
                "webm" => "WebM",
                "opus" => "Opus",
                _ => "media",
            };
            CommandError::new("file_decode", lang.file_decode(name, format))
        }
        FileError::Limit => CommandError::new("file_limit", ""),
        FileError::Cancelled | FileError::Pipeline(PipelineError::RetryCancelled) => {
            CommandError::new("file_cancelled", "")
        }
        _ => CommandError::new("file_worker", ""),
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Txt,
    Srt,
}

pub async fn save(app: AppHandle, format: OutputFormat, prefixes: bool, speakers: bool) -> Result<bool> {
    let jobs = app.state::<Arc<FileJobs>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (text, revision) = jobs.render_revision(format, prefixes, speakers)?;
        let extension = match format {
            OutputFormat::Txt => "txt",
            OutputFormat::Srt => "srt",
        };
        let selected = app
            .dialog()
            .file()
            .add_filter(extension, &[extension])
            .set_file_name(format!("transcript.{extension}"))
            .blocking_save_file();
        let Some(path) = selected else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|_| CommandError::new("file_save", ""))?;
        std::fs::write(path, text).map_err(|_| CommandError::new("file_save", ""))?;
        let state = jobs.handled(revision);
        crate::events::emit(&app, crate::events::FILE_PROGRESS, FileEvent::from(&state));
        Ok(true)
    })
    .await
    .map_err(|_| CommandError::new("file_save", ""))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_picked_files_start_one_worker_and_cancellation_wins_completion_race() {
        let jobs = FileJobs::default();
        assert_eq!(jobs.begin(FileOptions::default()).unwrap_err().code, "file_pick");
        jobs.0.lock().unwrap().path = Some(PathBuf::from("test.wav"));
        let (_, cancel) = jobs.begin(FileOptions::default()).unwrap();
        assert_eq!(jobs.begin(FileOptions::default()).unwrap_err().code, "file_busy");
        jobs.cancel();
        assert!(cancel.load(Ordering::Relaxed));
        jobs.finish(Ok(FileResult::default()), None);
        assert_eq!(jobs.snapshot().phase, Phase::Cancelled);
        assert!(jobs.snapshot().result.is_none());
        let (_, next) = jobs.begin(FileOptions::default()).unwrap();
        assert!(!next.load(Ordering::Relaxed));
        jobs.finish(Err(CommandError::new("file_worker", "")), None);
        assert_eq!(jobs.snapshot().phase, Phase::Failed);
        assert!(jobs.begin(FileOptions::default()).is_ok());
    }

    fn picked() -> FileJobs {
        let jobs = FileJobs::default();
        jobs.0.lock().unwrap().path = Some(PathBuf::from("test.wav"));
        jobs
    }

    /// Refused for file work without reaching the (absent) controller actor.
    fn update_blocked(handle: &crate::controller::ControllerHandle) -> bool {
        let mut hold = std::pin::pin!(handle.hold_for_update());
        let waker = std::task::Waker::noop();
        matches!(
            hold.as_mut().poll(&mut std::task::Context::from_waker(waker)),
            std::task::Poll::Ready(Err(crate::controller::HoldRefusal::FileWork))
        )
    }

    #[test]
    fn an_unsaved_result_blocks_updates_until_saved_copied_discarded_or_replaced() {
        let (handle, _inbox) = crate::controller::channel();
        let complete = |jobs: &FileJobs| {
            jobs.begin(FileOptions::default()).unwrap();
            jobs.finish(Ok(FileResult::default()), Some(handle.begin_file_work().unwrap()));
            assert!(jobs.snapshot().unsaved);
            assert!(update_blocked(&handle), "an update would discard the in-memory result");
        };
        let jobs = picked();

        // Saved or copied: the result stays visible.
        complete(&jobs);
        let revision = jobs.snapshot().revision;
        assert!(jobs.handled(revision - 1).unsaved, "a stale revision changes nothing");
        assert!(update_blocked(&handle));
        let state = jobs.handled(revision);
        assert_eq!((state.revision, state.unsaved, state.result.is_some()), (revision, false, true));
        assert!(!update_blocked(&handle));

        // Discarded.
        complete(&jobs);
        let revision = jobs.snapshot().revision;
        let state = jobs.discard(revision);
        assert_eq!((state.phase, state.unsaved, state.result.is_none()), (Phase::Idle, false, true));
        assert!(state.revision > revision);
        assert!(!update_blocked(&handle));

        // Replaced by a new job: only the new worker's guard remains.
        complete(&jobs);
        let running = handle.begin_file_work().unwrap();
        jobs.begin(FileOptions::default()).unwrap();
        assert!(!jobs.snapshot().unsaved);
        drop(running);
        assert!(!update_blocked(&handle));

        // Failed and cancelled jobs keep nothing.
        jobs.finish(Err(CommandError::new("file_worker", "")), Some(handle.begin_file_work().unwrap()));
        assert!(!update_blocked(&handle));
        jobs.begin(FileOptions::default()).unwrap();
        jobs.cancel();
        jobs.finish(Ok(FileResult::default()), Some(handle.begin_file_work().unwrap()));
        assert!(!jobs.snapshot().unsaved);
        assert!(!update_blocked(&handle));
    }
}
