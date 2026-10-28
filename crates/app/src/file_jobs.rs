//! One independent file worker. Native dialogs are the only source of paths; commands
//! accept options, never paths or arbitrary output content. Results stay in memory.

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
use crate::controller::Providers;
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
}

/// Progress events carry no transcript. The UI retrieves a snapshot after completion.
#[derive(Clone, Serialize)]
pub struct FileEvent {
    pub revision: u64,
    pub phase: Phase,
    pub progress: Option<Progress>,
}

#[derive(Default)]
struct Inner {
    path: Option<PathBuf>,
    state: FileState,
    cancel: Arc<AtomicBool>,
    dialog_open: bool,
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
        event(&inner.state)
    }

    fn finish(&self, result: Result<FileResult>) -> FileEvent {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        inner.state.revision += 1;
        if inner.cancel.load(Ordering::Relaxed) {
            inner.state.phase = Phase::Cancelled;
        } else {
            match result {
                Ok(result) => {
                    inner.state.phase = Phase::Completed;
                    inner.state.result = Some(result);
                }
                Err(error) => {
                    inner.state.phase = Phase::Failed;
                    inner.state.error = Some(error);
                }
            }
        }
        event(&inner.state)
    }

    pub fn render(&self, format: OutputFormat, prefixes: bool, speakers: bool) -> Result<String> {
        let inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let result = inner.state.result.as_ref().ok_or_else(|| CommandError::new("file_result", ""))?;
        let speakers = speakers && inner.state.options.speakers;
        match format {
            OutputFormat::Txt => Ok(files::render_txt(result, prefixes, speakers)),
            OutputFormat::Srt => {
                files::render_srt(result, speakers).ok_or_else(|| CommandError::new("file_timestamps", ""))
            }
        }
    }
}

fn event(state: &FileState) -> FileEvent {
    FileEvent { revision: state.revision, phase: state.phase, progress: state.progress }
}

pub async fn pick(app: AppHandle) -> Result<FileState> {
    let jobs = app.state::<Arc<FileJobs>>().inner().clone();
    let lang = app.state::<Arc<AppCore>>().settings.current().lang;
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
            inner.path = Some(path);
            inner.state = FileState { name, revision, ..FileState::default() };
        }
        Ok(None) => {}
        _ => return Err(CommandError::new("file_pick", "")),
    }
    Ok(inner.state.clone())
}

fn client(core: &AppCore, mut profile: Profile, options: &FileOptions) -> Result<ProviderClient> {
    profile.response_format = ResponseFormat::VerboseJson;
    if let Some(language) = &options.language {
        profile.language = language.clone();
    }
    let key = profile.api_key_ref.as_deref().map(|key| core.platform.secrets.get(key)).transpose()?.flatten();
    if profile.api_key_ref.is_some() && key.as_deref().is_none_or(|k| k.trim().is_empty()) {
        return Err(CommandError {
            code: "provider",
            message: String::new(),
            kind: Some(crate::controller::ErrorKind::MissingKey),
        });
    }
    match profile.provider {
        ProviderKind::OpenaiCompatible => opit_core::provider::OpenAiCompatible::new(profile, key)
            .map(|c| ProviderClient::OpenaiCompatible(c.for_file()))
            .map_err(Into::into),
        ProviderKind::Elevenlabs => opit_core::provider::elevenlabs::ElevenLabs::new(profile, key)
            .map(|c| ProviderClient::Elevenlabs(c.with_file_options(options.speakers)))
            .map_err(Into::into),
    }
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
    let (path, cancel) = jobs.begin(options.clone())?;
    let initial = jobs.snapshot();
    let handle = tauri::async_runtime::handle();
    let worker_jobs = jobs.clone();
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let primary = client(&core, profile.clone(), &options)?;
        let fallback = settings.config.fallback_for(&profile).cloned().and_then(|p| client(&core, p, &options).ok());
        let cleanup = if options.cleanup {
            HttpProviders::new(core.platform.secrets.clone()).cleanup_client(&settings.config.cleanup).ok()
        } else {
            None
        };
        let samples = files::decode::decode(&path, &cancel).map_err(|e| file_error(e, settings.lang, &path))?;
        let ctx = PipelineContext {
            primary: &primary,
            fallback: fallback.as_ref(),
            rules: &settings.rules,
            prompt_context: &settings.config.rules.prompt_context,
            retry_delay: opit_core::provider::retry::RETRY_DELAY,
            numbers_as_words: settings.config.rules.numbers_as_words,
        };
        let mut result = handle
            .block_on(files::transcribe(
                &samples,
                &ctx,
                cleanup.as_deref().map(|client| FileCleanup {
                    client,
                    ui_language: if settings.lang == Lang::Tr { "tr" } else { "en" },
                }),
                &cancel,
                files::TARGET_SAMPLES,
                |p| {
                    crate::events::emit(&worker_app, crate::events::FILE_PROGRESS, worker_jobs.progress(p));
                },
            ))
            .map_err(|e| file_error(e, settings.lang, &path))?;
        result.cleanup_failed |= options.cleanup && cleanup.is_none();
        Ok(result)
    });
    tauri::async_runtime::spawn(async move {
        let result = worker.await.unwrap_or_else(|_| Err(CommandError::new("file_worker", "")));
        crate::events::emit(&app, crate::events::FILE_PROGRESS, jobs.finish(result));
    });
    Ok(initial)
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
        let text = jobs.render(format, prefixes, speakers)?;
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
        jobs.finish(Ok(FileResult::default()));
        assert_eq!(jobs.snapshot().phase, Phase::Cancelled);
        assert!(jobs.snapshot().result.is_none());
        let (_, next) = jobs.begin(FileOptions::default()).unwrap();
        assert!(!next.load(Ordering::Relaxed));
        jobs.finish(Err(CommandError::new("file_worker", "")));
        assert_eq!(jobs.snapshot().phase, Phase::Failed);
        assert!(jobs.begin(FileOptions::default()).is_ok());
    }
}
