//! Opit Speech to Text's offline model installation and shared server supervisor.
pub mod catalogue;
mod download;
mod hardware;
mod server;

use crate::settings::SettingsHandle;
use catalogue::{Artifact, BUILDS, MODELS};
use opit_core::config::LocalConfig;
use opit_core::provider::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;

type Result<T> = std::result::Result<T, LocalError>;
#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("local storage could not be accessed")]
    Io,
    #[error("download did not match the pinned size or SHA-256")]
    Hash,
    #[error("download host is not allowed")]
    Url,
    #[error("download failed")]
    Download,
    #[error("operation cancelled")]
    Cancelled,
    #[error("archive contains an unsafe or unsupported entry")]
    Archive,
    #[error("the installed build does not contain whisper-server.exe")]
    MissingServer,
    #[error("install the selected server build and model first")]
    NotInstalled,
    #[error("local server could not be started")]
    Launch,
    #[error("local server did not become ready in time")]
    Readiness,
    #[error("local server crashed again after its automatic restart")]
    Crash,
    #[error("a free loopback port could not be reserved")]
    Port,
    #[error("choose an absolute ASCII models folder, for example C:\\OpitModels")]
    Folder,
    #[error("another local model operation is running")]
    Busy,
    #[error("unknown local model or server build")]
    Unknown,
    #[error("the application is exiting")]
    Closed,
}
impl From<LocalError> for crate::app_core::CommandError {
    fn from(error: LocalError) -> Self {
        let code = match error {
            LocalError::Folder => "local_folder",
            LocalError::Hash => "local_hash",
            LocalError::MissingServer => "local_server_missing",
            LocalError::NotInstalled => "local_not_installed",
            LocalError::Cancelled => "local_cancelled",
            LocalError::Busy => "local_busy",
            LocalError::Readiness => "local_readiness",
            _ => "local",
        };
        Self::new(code, error.to_string())
    }
}

#[derive(Clone, Default, Serialize)]
pub struct Progress {
    pub id: String,
    pub received: u64,
    pub total: u64,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub builds: Vec<Artifact>,
    pub models: Vec<Artifact>,
    pub installed: Vec<String>,
    pub progress: Option<Progress>,
    pub running_model: Option<String>,
    pub running_build: Option<String>,
    pub recommended: &'static str,
    pub hardware: hardware::Hardware,
    pub folder: String,
}
struct Running {
    ready: bool,
    process: server::Process,
    endpoint: String,
    model: String,
    config: LocalConfig,
    root: PathBuf,
}
struct Session {
    running: Option<Running>,
    restarts: server::RestartBudget,
    last_used: Instant,
}
impl Default for Session {
    fn default() -> Self {
        Self { running: None, restarts: server::RestartBudget::default(), last_used: Instant::now() }
    }
}
type ProgressSink = Arc<dyn Fn(Progress) + Send + Sync>;
type LoadingSink = Arc<dyn Fn(bool) + Send + Sync>;
pub struct LocalService {
    default_root: PathBuf,
    settings: SettingsHandle,
    gate: tokio::sync::Mutex<()>,
    operation: tokio::sync::Mutex<()>,
    session: Arc<Mutex<Session>>,
    hardware: hardware::HardwareCache,
    progress: Mutex<Option<Progress>>,
    cancel: Mutex<Option<watch::Sender<bool>>>,
    closed: AtomicBool,
    loading: Mutex<Option<LoadingSink>>,
}

pub fn validate_config(config: &LocalConfig) -> Result<()> {
    if !BUILDS.iter().any(|b| b.id == config.build) {
        return Err(LocalError::Unknown);
    }
    if let Some(folder) = &config.models_folder
        && (!folder.is_ascii() || !Path::new(folder).is_absolute() || folder.contains(['\0', '\r', '\n']))
    {
        return Err(LocalError::Folder);
    }
    Ok(())
}

pub fn storage_root(default_root: &Path, config: &LocalConfig, redirected: bool) -> Result<PathBuf> {
    validate_config(config)?;
    let root = if redirected {
        default_root.to_owned()
    } else {
        config
            .models_folder
            .as_ref()
            .map(|p| Path::new(p).join("opit-speech-to-text-local"))
            .unwrap_or_else(|| default_root.to_owned())
    };
    if !root.as_os_str().is_ascii() {
        return Err(LocalError::Folder);
    }
    Ok(root)
}

impl LocalService {
    pub fn new(default_root: PathBuf, settings: SettingsHandle) -> Arc<Self> {
        Arc::new(Self {
            default_root,
            settings,
            gate: tokio::sync::Mutex::new(()),
            operation: tokio::sync::Mutex::new(()),
            session: Arc::new(Mutex::new(Session::default())),
            hardware: hardware::HardwareCache::default(),
            progress: Mutex::new(None),
            cancel: Mutex::new(None),
            closed: AtomicBool::new(false),
            loading: Mutex::new(None),
        })
    }
    fn root(&self, config: &LocalConfig) -> Result<PathBuf> {
        storage_root(
            &self.default_root,
            config,
            std::env::var_os(crate::startup::DATA_DIR_ENV).is_some_and(|s| !s.is_empty()),
        )
    }
    fn hardware(&self) -> hardware::Hardware {
        self.hardware.get(hardware::detect, std::time::Instant::now())
    }
    pub fn set_loading(&self, sink: LoadingSink) {
        *self.loading.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
    }
    fn loading(&self, value: bool) {
        if let Some(sink) = self.loading.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            sink(value);
        }
    }
    pub fn cancel(&self) {
        if let Some(tx) = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            let _ = tx.send(true);
        }
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        self.cancel();
        self.stop().await
    }
    /// Runs on a blocking worker: filesystem and hardware enumeration never block the WebView.
    pub fn snapshot(&self) -> Result<Snapshot> {
        let config = &self.settings.current().config.local;
        let root = self.root(config)?;
        let mut installed = Vec::new();
        for model in MODELS {
            if std::fs::metadata(root.join("models").join(model.file)).is_ok_and(|m| m.len() == model.size) {
                installed.push(model.id.to_owned());
            }
        }
        for build in BUILDS {
            if download::find_server(&root.join("bin").join(build.id))?.is_some() {
                installed.push(build.id.to_owned());
            }
        }
        let hardware = self.hardware();
        let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let running_model =
            session.running.as_mut().and_then(|r| if r.process.alive() { Some(r.model.clone()) } else { None });
        let running_build =
            running_model.as_ref().and_then(|_| session.running.as_ref().map(|r| r.config.build.clone()));
        Ok(Snapshot {
            builds: BUILDS.iter().copied().filter(|b| hardware.nvidia || !b.id.starts_with("cuda")).collect(),
            models: MODELS.to_vec(),
            installed,
            progress: self.progress.lock().unwrap_or_else(PoisonError::into_inner).clone(),
            running_model,
            running_build,
            recommended: hardware::recommendation(&hardware),
            hardware,
            folder: root.to_string_lossy().into_owned(),
        })
    }
    pub async fn install(self: &Arc<Self>, id: String, sink: ProgressSink) -> Result<()> {
        let _operation = self.operation.try_lock().map_err(|_| LocalError::Busy)?;
        let _gate = self.gate.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Err(LocalError::Closed);
        }
        let (artifact, source) = artifact(&id)?;
        let root = self.root(&self.settings.current().config.local)?;
        let (tx, rx) = watch::channel(false);
        *self.cancel.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        let destination = if source == download::Source::Build {
            root.join(artifact.file)
        } else {
            root.join("models").join(artifact.file)
        };
        let update = |received| {
            let p = Progress { id: id.clone(), received, total: artifact.size };
            *self.progress.lock().unwrap_or_else(PoisonError::into_inner) = Some(p.clone());
            sink(p);
        };
        update(0);
        let result = async {
            // Stop before replacing a loaded model or its DLLs.
            self.stop().await?;
            download::fetch(artifact, source, destination.clone(), rx.clone(), update).await?;
            if source == download::Source::Build {
                let session = self.session.clone();
                let closed = self.clone();
                tokio::task::spawn_blocking(move || {
                    if closed.closed.load(Ordering::SeqCst) {
                        return Err(LocalError::Closed);
                    }
                    let bin = root.join("bin");
                    std::fs::create_dir_all(&bin).map_err(|_| LocalError::Io)?;
                    let pending = bin.join(format!("{}.part", artifact.id));
                    if pending.exists() {
                        std::fs::remove_dir_all(&pending).map_err(|_| LocalError::Io)?;
                    }
                    let result = download::extract(&destination, &pending, &rx);
                    let _ = std::fs::remove_file(&destination);
                    result?;
                    if *rx.borrow() {
                        let _ = std::fs::remove_dir_all(&pending);
                        return Err(LocalError::Cancelled);
                    }
                    let _lock = session.lock().unwrap_or_else(PoisonError::into_inner);
                    let target = bin.join(artifact.id);
                    if target.exists() {
                        std::fs::remove_dir_all(&target).map_err(|_| LocalError::Io)?;
                    }
                    std::fs::rename(pending, target).map_err(|_| LocalError::Io)
                })
                .await
                .map_err(|_| LocalError::Io)??;
            }
            Ok(())
        }
        .await;
        *self.cancel.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.progress.lock().unwrap_or_else(PoisonError::into_inner) = None;
        result
    }
    pub async fn delete(&self, id: String) -> Result<()> {
        let _operation = self.operation.try_lock().map_err(|_| LocalError::Busy)?;
        let _gate = self.gate.lock().await;
        let (artifact, source) = artifact(&id)?;
        let root = self.root(&self.settings.current().config.local)?;
        self.stop().await?;
        tokio::task::spawn_blocking(move || {
            let path = if source == download::Source::Build {
                root.join("bin").join(artifact.id)
            } else {
                root.join("models").join(artifact.file)
            };
            if !path.exists() {
                return Ok(());
            }
            if source == download::Source::Build { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
                .map_err(|_| LocalError::Io)
        })
        .await
        .map_err(|_| LocalError::Io)?
    }
    async fn stop(&self) -> Result<()> {
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let old = std::mem::take(&mut *session.lock().unwrap_or_else(PoisonError::into_inner));
            // Termination waits on the child; never keep the session locked while waiting.
            drop(old);
        })
        .await
        .map_err(|_| LocalError::Io)
    }

    /// Slow preparation is independent of session ownership. Closing prevents a late
    /// initializer from publishing or launching a process after shutdown completes.
    fn initialize<T>(
        &self,
        prepare: impl FnOnce() -> Result<T>,
        launch: impl FnOnce(T) -> Result<Running>,
    ) -> Result<(String, bool)> {
        let prepared = prepare()?;
        let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        if self.closed.load(Ordering::SeqCst) {
            return Err(LocalError::Closed);
        }
        let running = launch(prepared)?;
        let endpoint = running.endpoint.clone();
        session.running = Some(running);
        Ok((endpoint, true))
    }
    async fn endpoint(self: &Arc<Self>, profile: &Profile) -> Result<String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(LocalError::Closed);
        }
        let config = self.settings.current().config.local.clone();
        let root = self.root(&config)?;
        let model = MODELS.iter().find(|a| a.id == profile.model).copied().ok_or(LocalError::Unknown)?;
        let service = self.clone();
        let config2 = config.clone();
        let endpoint = tokio::task::spawn_blocking(move || {
            let previous = {
                let mut session = service.session.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(running) = session.running.as_mut() {
                    if running.model == model.id && running.config == config2 && running.root == root {
                        if running.process.alive() {
                            return Ok((running.endpoint.clone(), !running.ready));
                        }
                        session.restarts.crashed()?;
                    } else {
                        session.restarts = server::RestartBudget::default();
                    }
                }
                session.running.take()
            };
            drop(previous);
            service.initialize(
                || {
                    let exe = download::find_server(&root.join("bin").join(&config2.build))?
                        .ok_or(LocalError::NotInstalled)?;
                    let model_path = root.join("models").join(model.file);
                    if !model_path.is_file() {
                        return Err(LocalError::NotInstalled);
                    }
                    download::verify(&model_path, &model)?;
                    let port = server::free_port()?;
                    let threads =
                        if config2.threads == 0 { service.hardware().physical_cores.max(1) } else { config2.threads };
                    Ok((exe, model_path, port, threads))
                },
                |(exe, model_path, port, threads)| {
                    let process = server::Process::launch(&exe, &model_path, port, threads, config2.gpu)?;
                    let endpoint = format!("http://127.0.0.1:{port}");
                    Ok(Running {
                        ready: false,
                        process,
                        endpoint: endpoint.clone(),
                        model: model.id.into(),
                        config: config2.clone(),
                        root: root.clone(),
                    })
                },
            )
        })
        .await
        .map_err(|_| LocalError::Launch)??;
        if endpoint.1 {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| LocalError::Launch)?;
            let url = format!("{}/health", endpoint.0);
            let result = server::readiness(Duration::from_secs(180), || async {
                if self.closed.load(Ordering::SeqCst) {
                    return Err(LocalError::Closed);
                }
                if !self
                    .session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .running
                    .as_mut()
                    .is_some_and(|r| r.process.alive())
                {
                    return Err(LocalError::Crash);
                }
                Ok(client.get(&url).timeout(Duration::from_secs(2)).send().await.is_ok_and(|r| r.status().is_success()))
            })
            .await;
            if result.is_err() {
                let session = self.session.clone();
                tokio::task::spawn_blocking(move || {
                    if let Some(running) = session.lock().unwrap_or_else(PoisonError::into_inner).running.as_mut() {
                        running.process.terminate();
                    }
                })
                .await
                .map_err(|_| LocalError::Io)?;
            }
            result?;
            if let Some(running) = self.session.lock().unwrap_or_else(PoisonError::into_inner).running.as_mut() {
                running.ready = true;
            }
        }
        Ok(endpoint.0)
    }
    pub async fn transcribe(
        self: &Arc<Self>,
        profile: &Profile,
        request: &TranscribeRequest<'_>,
    ) -> std::result::Result<RawTranscript, ProviderError> {
        let _gate = self.gate.lock().await;
        let result = async {
            let needs_loading =
                self.session.lock().unwrap_or_else(PoisonError::into_inner).running.as_mut().is_none_or(|r| {
                    r.model != profile.model
                        || !r.ready
                        || !r.process.alive()
                        || r.config != self.settings.current().config.local
                });
            if needs_loading {
                self.loading(true);
            }
            let endpoint = self.endpoint(profile).await;
            if needs_loading {
                self.loading(false);
            }
            let endpoint = endpoint.map_err(provider_error)?;
            let mut resolved = profile.clone();
            resolved.base_url = endpoint;
            let result = opit_core::provider::local::LocalWhisper::new(resolved)?.transcribe(request).await;
            let crashed = self
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .running
                .as_mut()
                .is_some_and(|r| !r.process.alive());
            if result.is_err() && crashed {
                let endpoint = self.endpoint(profile).await.map_err(provider_error)?;
                let mut resolved = profile.clone();
                resolved.base_url = endpoint;
                return opit_core::provider::local::LocalWhisper::new(resolved)?.transcribe(request).await;
            }
            result
        }
        .await;
        self.session.lock().unwrap_or_else(PoisonError::into_inner).last_used = Instant::now();
        result
    }
    pub async fn test(self: &Arc<Self>, profile: &Profile) -> Result<()> {
        let _gate = self.gate.lock().await;
        self.endpoint(profile).await?;
        self.session.lock().unwrap_or_else(PoisonError::into_inner).last_used = Instant::now();
        Ok(())
    }
    async fn idle_tick(self: &Arc<Self>) {
        let Ok(_gate) = self.gate.try_lock() else {
            return;
        };
        let minutes = self.settings.current().config.local.idle_minutes;
        let (expired, crashed_model) = {
            let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            let expired = server::idle_expired(session.last_used, Instant::now(), minutes);
            let crashed_model =
                session.running.as_mut().and_then(|r| if r.process.alive() { None } else { Some(r.model.clone()) });
            (expired, crashed_model)
        };
        if expired {
            let _ = self.stop().await;
        } else if let Some(model) = crashed_model {
            let mut profile = opit_core::provider::presets::local();
            profile.model = model;
            let _ = self.endpoint(&profile).await;
        }
    }
    pub fn start_background(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let Some(service) = weak.upgrade() else {
                    break;
                };
                if service.closed.load(Ordering::SeqCst) {
                    break;
                }
                service.idle_tick().await;
            }
        });
        if self.settings.current().config.local.preload {
            let service = self.clone();
            tokio::spawn(async move {
                let profile = service
                    .settings
                    .current()
                    .config
                    .active_profile()
                    .filter(|p| p.provider == opit_core::provider::ProviderKind::LocalWhisper)
                    .cloned();
                if let Some(profile) = profile {
                    let _ = service.test(&profile).await;
                }
            });
        }
    }
}
fn artifact(id: &str) -> Result<(Artifact, download::Source)> {
    if let Some(a) = BUILDS.iter().find(|a| a.id == id) {
        return Ok((*a, download::Source::Build));
    }
    MODELS.iter().find(|a| a.id == id).copied().map(|a| (a, download::Source::Model)).ok_or(LocalError::Unknown)
}
fn provider_error(error: LocalError) -> ProviderError {
    ProviderError::Network(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn existing_retry_ladder_supports_cloud_and_local_fallback_in_both_directions() {
        use crate::providers::AppProvider;
        use opit_core::pipeline::{PipelineContext, PreparedAudio};
        use opit_core::provider::{ProviderClient, presets};
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/inference"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"sample", "language":"turkish"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"cloud sample"})))
            .mount(&server)
            .await;
        let rules = opit_core::rules::RuleSet::compile(&[]).0;
        let audio = PreparedAudio { samples: vec![0.1; 16000], audio_ms: 1000, speech_ms: 1000 };
        let mut cloud_profile = presets::groq();
        cloud_profile.base_url = server.uri();
        cloud_profile.api_key_ref = None;
        let dir = tempfile::tempdir().unwrap();
        let (service, _) = fake_service(dir.path(), server.uri());
        let local = AppProvider::Local { profile: presets::local(), service };
        let mut broken_cloud = cloud_profile.clone();
        broken_cloud.base_url = format!("{}/unavailable", server.uri());
        // A transient HTTP error is required for the established fallback policy.
        Mock::given(method("POST"))
            .and(path("/unavailable/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let cloud = AppProvider::Cloud(ProviderClient::new(broken_cloud, None).unwrap());
        let ctx = PipelineContext {
            primary: &cloud,
            fallback: Some(&local),
            rules: &rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
            numbers_as_words: false,
        };
        let result = opit_core::pipeline::transcribe(&audio, &ctx).await.unwrap();
        assert_eq!(result.language.as_deref(), Some("tr"));
        let settings = SettingsHandle::new(
            crate::settings::Settings::build(opit_core::config::AppConfig::default(), None, None).0,
        );
        let unavailable = AppProvider::Local {
            profile: presets::local(),
            service: LocalService::new(dir.path().join("absent"), settings),
        };
        let cloud = AppProvider::Cloud(ProviderClient::new(cloud_profile, None).unwrap());
        let ctx = PipelineContext {
            primary: &unavailable,
            fallback: Some(&cloud),
            rules: &rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
            numbers_as_words: false,
        };
        assert_eq!(opit_core::pipeline::transcribe(&audio, &ctx).await.unwrap().text, "cloud sample");
    }
    fn fake_service(root: &Path, endpoint: String) -> (Arc<LocalService>, Arc<AtomicBool>) {
        let settings = SettingsHandle::new(
            crate::settings::Settings::build(opit_core::config::AppConfig::default(), None, None).0,
        );
        let service = LocalService::new(root.to_owned(), settings);
        let alive = Arc::new(AtomicBool::new(true));
        service.session.lock().unwrap().running = Some(Running {
            ready: true,
            process: server::Process::Fake(alive.clone()),
            endpoint,
            model: opit_core::provider::presets::local().model,
            config: LocalConfig::default(),
            root: root.to_owned(),
        });
        (service, alive)
    }
    #[tokio::test(start_paused = true)]
    async fn idle_shutdown_skips_inflight_requests_and_exit_always_kills_fake_process() {
        let dir = tempfile::tempdir().unwrap();
        let (service, alive) = fake_service(dir.path(), "http://127.0.0.1:1".into());
        let busy = service.gate.lock().await;
        tokio::time::advance(Duration::from_secs(600)).await;
        service.idle_tick().await;
        assert!(alive.load(Ordering::SeqCst));
        drop(busy);
        service.idle_tick().await;
        assert!(!alive.load(Ordering::SeqCst));
        let (service, alive) = fake_service(dir.path(), "http://127.0.0.1:1".into());
        let mut config = service.settings.current().config.clone();
        config.local.idle_minutes = 0;
        service.settings.replace(crate::settings::Settings::build(config, None, None).0);
        tokio::time::advance(Duration::from_secs(60000)).await;
        service.idle_tick().await;
        assert!(alive.load(Ordering::SeqCst));
        service.shutdown().await.unwrap();
        assert!(!alive.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn shutdown_finishes_while_initialization_is_blocked_and_prevents_late_launch() {
        let dir = tempfile::tempdir().unwrap();
        let (service, alive) = fake_service(dir.path(), "http://127.0.0.1:1".into());
        let (entered, entering) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let initializer = service.clone();
        let worker = tokio::task::spawn_blocking(move || {
            initializer.initialize(
                || {
                    let _ = entered.send(());
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                },
                |()| panic!("shutdown must prevent a late process launch"),
            )
        });
        entering.await.unwrap();
        let shutdown = tokio::time::timeout(Duration::from_secs(1), service.shutdown()).await;
        // Always release the blocked worker, including when testing a regression.
        release.send(()).unwrap();
        shutdown.expect("initialization must not block shutdown").unwrap();
        assert!(!alive.load(Ordering::SeqCst));
        assert!(matches!(worker.await.unwrap(), Err(LocalError::Closed)));
        assert!(service.session.lock().unwrap().running.is_none());
    }
    #[tokio::test]
    async fn shared_local_server_preserves_language_without_keys_and_updates_idle_time() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/inference"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"sample", "language":"turkish"})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (service, alive) = fake_service(dir.path(), server.uri());
        let profile = opit_core::provider::presets::local();
        let audio =
            opit_core::audio::encode::encode(&[0.0; 16000], opit_core::audio::encode::AudioFormat::Wav).unwrap();
        for _ in 0..2 {
            let result = service
                .transcribe(&profile, &TranscribeRequest { audio: &audio, audio_ms: 1000, prompt: None, keywords: &[] })
                .await
                .unwrap();
            assert_eq!(result.language.as_deref(), Some("tr"));
            assert!(alive.load(Ordering::SeqCst));
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
    #[test]
    fn catalogue_is_complete_and_pinned() {
        let mut ids = std::collections::HashSet::new();
        for (items, source) in [(BUILDS, download::Source::Build), (MODELS, download::Source::Model)] {
            for a in items {
                assert!(ids.insert(a.id));
                assert!(a.size > 0);
                assert_eq!(a.sha256.len(), 64);
                assert!(a.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
                assert!(download::allowed(&reqwest::Url::parse(a.url).unwrap(), source));
                assert!(a.url.ends_with(a.file));
            }
        }
        assert_eq!(BUILDS.len(), 4);
        assert_eq!(MODELS.len(), 10);
    }
    #[test]
    fn folder_is_ascii_and_test_root_overrides_custom_folder() {
        let root = std::env::current_dir().unwrap().join("scratch");
        let mut config = LocalConfig {
            models_folder: Some(root.join("custom").to_string_lossy().into_owned()),
            ..Default::default()
        };
        assert_eq!(storage_root(&root, &config, true).unwrap(), root);
        assert!(storage_root(&root, &config, false).unwrap().ends_with("opit-speech-to-text-local"));
        config.models_folder = Some("relative".into());
        assert!(validate_config(&config).is_err());
        config.models_folder = Some("C:\\mödels".into());
        assert!(validate_config(&config).is_err());
    }
}
