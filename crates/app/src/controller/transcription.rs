//! Transcription workers, retry and cleanup.

use super::delivery::{HistoryContext, save_transcription};
use super::recording::{effective_paste, effective_style};
use super::*;

/// Logs safe metadata even when the worker is aborted during cleanup.
pub(super) struct CleanupLog {
    session: u64,
    model: String,
    started: Instant,
    stats: cleanup::CleanupStats,
    outcome: &'static str,
    error: Option<CleanupError>,
}

impl CleanupLog {
    pub(super) fn new(session: u64, model: String) -> Self {
        Self {
            session,
            model,
            started: Instant::now(),
            stats: cleanup::CleanupStats::default(),
            outcome: "skipped",
            error: None,
        }
    }

    pub(super) fn finished(&mut self, error: Option<CleanupError>) {
        self.error = error;
        self.outcome = if error.is_some() { "failed" } else { "ok" };
    }
}

impl Drop for CleanupLog {
    fn drop(&mut self) {
        let error_kind = self
            .error
            .map(|e| format!("{e:?}"))
            .unwrap_or_else(|| if self.outcome == "skipped" { "Cancelled" } else { "None" }.into());
        info!(
            session = self.session, outcome = self.outcome, error_kind,
            attempts = self.stats.attempts(), elapsed_ms = self.started.elapsed().as_millis() as u64,
            model = %self.model, "cleanup finished"
        );
    }
}

impl<P: Providers> Controller<P> {
    pub(super) fn clients(&self, settings: &Settings) -> Result<Clients<P::Client>, SetupError> {
        let config = &settings.config;
        let profile = config.active_profile().ok_or_else(|| SetupError::Client("no active profile".into()))?;
        let primary = self.env.providers.client(profile)?;
        let fallback = config.fallback_for(profile).and_then(|p| match self.env.providers.client(p) {
            Ok(client) => Some(client),
            Err(err) => {
                warn!(profile = %p.id, error = %err, "fallback profile unavailable");
                None
            }
        });
        Ok(Clients { primary, fallback })
    }

    pub(super) fn retry(&mut self) {
        if !self.is_idle() {
            return;
        }
        let Some(audio) = self.retry.clone() else {
            return;
        };
        if self.held_for_update() {
            return;
        }
        let settings = self.style_settings();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot retry");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.session += 1;
        info!(session = self.session, "retrying the last dictation");
        self.env.hotkey.set_capture_escape(true);
        let job = Job {
            voice: None,
            ask: false,
            ask_test: false,
            text_reply: None,
            paste: effective_paste(&settings),
            cleanup_style: effective_style(&settings),
            settings,
            started: Instant::now(),
            auto_paste: self.env.paster.can_auto_paste(),
            history_id: None,
            reply: None,
        };
        self.begin_transcription(self.session, job, Some(clients), Input::Prepared(audio));
    }

    pub(super) fn begin_transcription(
        &mut self,
        session: u64,
        job: Job,
        clients: Option<Clients<P::Client>>,
        input: Input,
    ) {
        let settings = job.settings.clone();
        let mut voice = job.voice.clone();
        let is_voice = voice.is_some();
        let is_ask = job.ask;
        if is_ask || is_voice {
            self.retry = None;
        }
        let is_ask_test = job.ask_test;
        let ask_service = self.env.providers.ask_service();
        self.show(view(Tone::Busy, settings.lang.transcribing(), None));
        let tx = self.tx.clone();
        let retry_delay = self.env.retry_delay;
        let task_settings = settings.clone();
        let style = job.cleanup_style.clone();
        let providers = self.env.providers.clone();
        let worker_tx = tx.clone();
        let history = self.env.history.clone();
        let history_id = job.history_id;
        let started = job.started;
        let (cancel, cancel_rx) = watch::channel(false);
        let worker = tokio::spawn(async move {
            let tx = worker_tx;
            let mut cleanup_input = None;
            let (audio, mut result) = match input {
                Input::Text(text) => (
                    None,
                    Ok(Transcript {
                        raw_text: text.clone(),
                        text,
                        status: TranscriptStatus::Ok,
                        profile_id: task_settings.config.active_profile_id.clone(),
                        detected_language: None,
                        language: None,
                        cleanup_status: None,
                        cleanup_model: None,
                        cleanup_error: None,
                        used_fallback: false,
                        audio_ms: 0,
                        request_ms: 0,
                        hits: Vec::new(),
                    }),
                ),
                input => {
                    let clients = clients.as_ref().expect("audio jobs have speech clients");
                    let mut live = None;
                    let audio = match input {
                        Input::Text(_) => unreachable!(),
                        Input::Prepared(audio) => audio,
                        Input::Raw(recording, streaming) => {
                            live = streaming;
                            match prepare_off_thread(recording, task_settings.config.recording.clone()).await {
                                Ok(audio) => Arc::new(audio),
                                Err(err) => {
                                    let _ = tx.send(Msg::Transcribed {
                                        session,
                                        audio: None,
                                        result: Err(err),
                                        cleanup_input: None,
                                    });
                                    return;
                                }
                            }
                        }
                    };
                    let ctx = PipelineContext {
                        primary: &clients.primary,
                        fallback: clients.fallback.as_ref(),
                        rules: &task_settings.rules,
                        prompt_context: &task_settings.config.rules.prompt_context,
                        retry_delay,
                        numbers_as_words: task_settings.config.rules.numbers_as_words,
                    };
                    let streamed = if let Some(live) = live {
                        match live.resolve().await {
                            Ok(raw) => Some(pipeline::from_raw(raw, &audio, &ctx)),
                            Err(error) => {
                                warn!(
                                    session,
                                    reason = error.code(),
                                    "live transcription failed; using recorded audio"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };
                    let mut result = if let Some(result) = streamed {
                        result
                    } else {
                        pipeline::transcribe_with_wait(&audio, &ctx, |progress, delay| {
                            wait_for_retry(tx.clone(), session, cancel_rx.clone(), progress, delay)
                        })
                        .await
                    };
                    let use_cleanup = !is_voice && (!is_ask || task_settings.config.ask.cleanup_request);
                    if use_cleanup && let Ok(transcript) = &mut result {
                        let profile = task_settings
                            .config
                            .profiles
                            .iter()
                            .find(|p| p.id == transcript.profile_id)
                            .unwrap_or_else(|| clients.primary.profile());
                        let selected = if is_ask {
                            task_settings.config.cleanup.style("clean").cloned().unwrap_or_else(|| style.clone())
                        } else {
                            style.clone()
                        };
                        let mut cleanup_config = task_settings.config.cleanup.for_style(&selected);
                        if is_ask {
                            cleanup_config.enabled = true;
                        }
                        let config = &cleanup_config;
                        if cleanup::eligible(transcript, config, profile)
                            || (is_ask && transcript.status == TranscriptStatus::Ok && !transcript.text.is_empty())
                        {
                            let mut log = CleanupLog::new(session, config.model.clone());
                            let _ = tx.send(Msg::Cleaning { session });
                            let ui_language = if task_settings.lang == Lang::Tr { "tr" } else { "en" };
                            let language = cleanup::prompt_language(
                                transcript.detected_language.as_deref(),
                                &profile.language,
                                ui_language,
                            );
                            let system = cleanup::system_prompt(
                                config,
                                language,
                                &task_settings.rules,
                                &task_settings.config.rules.prompt_context,
                            );
                            transcript.cleanup_model = Some(config.model.clone());
                            if !is_ask {
                                cleanup_input = Some(transcript.text.clone());
                            }
                            let provider = providers.clone();
                            let client_config = config.clone();
                            let mut skip = cancel_rx.clone();
                            let skipped = tokio::select! {
                                biased;
                                _ = skip.wait_for(|cancelled| *cancelled), if !is_ask => true,
                                _ = async {
                                    let client = tokio::task::spawn_blocking(move || provider.cleanup_client(&client_config))
                                        .await
                                        .unwrap_or(Err(CleanupError::Rejected));
                                    let client = client.as_ref().map(Arc::as_ref).map_err(|error| *error);
                                    cleanup::apply_with_stats(
                                        transcript, client, &system, &task_settings.rules, profile.apply_rules, &log.stats,
                                    ).await;
                                } => false,
                            };
                            if skipped {
                                // Pipeline rules are already applied; dropping the cleanup future preserves them.
                                transcript.cleanup_status = Some(cleanup::CleanupStatus::Skipped);
                                transcript.cleanup_error = None;
                                transcript.request_ms += log.started.elapsed().as_millis() as u64;
                            } else {
                                log.finished(transcript.cleanup_error);
                            }
                        }
                    }
                    (Some(audio), result)
                }
            };
            if let Some(voice) = &mut voice
                && let Ok(transcript) = &mut result
                && transcript.status == TranscriptStatus::Ok
                && !transcript.text.is_empty()
            {
                match voice.mode {
                    VoiceMode::Edit => voice.instruction = Some(transcript.text.clone()),
                    VoiceMode::Translate => voice.source_text = Some(transcript.text.clone()),
                }
                let _ = tx.send(Msg::Cleaning { session });
                let language = cleanup::prompt_language(
                    transcript.detected_language.as_deref(),
                    transcript.language.as_deref().unwrap_or("auto"),
                    if task_settings.lang == Lang::Tr { "tr" } else { "en" },
                );
                let timer = Instant::now();
                let mut log = CleanupLog::new(
                    session,
                    opit_core::voice_modes::connection_config(
                        &task_settings.config.cleanup,
                        (voice.mode == VoiceMode::Translate).then_some(&task_settings.config.translate),
                    )
                    .model,
                );
                let answer = match voice.mode {
                    VoiceMode::Edit => {
                        opit_core::voice_modes::edit_with_stats(
                            &voice.client,
                            voice.selection.as_deref(),
                            &transcript.text,
                            language,
                            &log.stats,
                        )
                        .await
                    }
                    VoiceMode::Translate => {
                        opit_core::voice_modes::translate_with_stats(
                            &voice.client,
                            &transcript.text,
                            &task_settings.config.translate.target_language,
                            &log.stats,
                        )
                        .await
                    }
                };
                log.finished(answer.as_ref().err().copied());
                transcript.request_ms += timer.elapsed().as_millis() as u64;
                transcript.cleanup_model = Some(
                    opit_core::voice_modes::connection_config(
                        &task_settings.config.cleanup,
                        (voice.mode == VoiceMode::Translate).then_some(&task_settings.config.translate),
                    )
                    .model,
                );
                match answer {
                    Ok(text) => {
                        transcript.text = text;
                        transcript.cleanup_status = Some(cleanup::CleanupStatus::Ok);
                    }
                    Err(error) => {
                        transcript.text.clear();
                        transcript.status = TranscriptStatus::Failed;
                        transcript.cleanup_status = Some(cleanup::CleanupStatus::Failed);
                        transcript.cleanup_error = Some(error);
                    }
                }
            }
            if is_ask
                && let Ok(transcript) = &mut result
                && transcript.status == TranscriptStatus::Ok
                && !transcript.text.is_empty()
            {
                let (reply, ready) = oneshot::channel();
                let _ = tx.send(Msg::BeginAsk { session, reply });
                if !ready.await.unwrap_or(false) {
                    return;
                }
                let answer = match ask_service {
                    Some(service) => {
                        let isolated;
                        let service = if is_ask_test {
                            isolated = service.isolated();
                            &isolated
                        } else {
                            service.as_ref()
                        };
                        service
                            .answer(task_settings.config.ask.clone(), transcript.text.clone(), cancel_rx.clone())
                            .await
                    }
                    None => Err(ErrorKind::Rejected),
                };
                match answer {
                    Ok(answer) => transcript.text = answer,
                    Err(kind) => {
                        let _ = tx.send(Msg::AskFailed { session, kind });
                        return;
                    }
                }
            }
            if !is_ask && task_settings.config.history.enabled && (is_voice || result.is_err()) {
                let audio = audio.clone().expect("speech retry has prepared audio");
                let saved = save_transcription(
                    history,
                    task_settings,
                    history_id,
                    audio.clone(),
                    result.clone(),
                    started.elapsed(),
                    HistoryContext { style_id: style.id.clone(), voice: voice.clone() },
                )
                .await;
                match saved {
                    Ok(id) => {
                        let _ = tx.send(Msg::HistoryStored(id));
                    }
                    Err(_) => {
                        warn!("history write failed");
                        if history_id.is_some() {
                            result = Err(PipelineError::Encode(EncodeError("history retry could not be saved".into())));
                        }
                    }
                }
            }
            let _ = tx.send(Msg::Transcribed { session, audio, result, cleanup_input });
        });
        let task = worker.abort_handle();
        tokio::spawn(async move {
            if worker.await.is_err() {
                let result = Err(PipelineError::Encode(EncodeError("transcription worker stopped".into())));
                let _ = tx.send(Msg::Transcribed { session, audio: None, result, cleanup_input: None });
            }
        });
        self.phase = Phase::Transcribing { session, job, task, cancel, progress: None, cleaning: false, asking: false };
        self.set_state(DictationState::Transcribing);
    }

    pub(super) fn retry_history(
        &mut self,
        id: i64,
        audio: Arc<PreparedAudio>,
        reply: oneshot::Sender<Result<(), ErrorKind>>,
    ) {
        let duplicate = matches!(&self.phase, Phase::Transcribing { job, .. } | Phase::Pasting { job, .. } if job.history_id == Some(id))
            || self.waiting.iter().any(|p| p.job.history_id == Some(id));
        let settings = self.style_settings();
        // A live recording already owns one future queue slot.
        let occupied = self.waiting.len() + usize::from(self.recording.is_some() || self.preparing.is_some());
        if duplicate || occupied >= MAX_WAITING || self.update_holds != 0 || !settings.config.history.enabled {
            let _ = reply.send(Err(ErrorKind::Rejected));
            return;
        }
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(error) => {
                let _ = reply.send(Err(error.kind()));
                return;
            }
        };
        self.session += 1;
        self.waiting.push_back(Pending {
            session: self.session,
            job: Job {
                voice: None,
                ask: false,
                ask_test: false,
                text_reply: None,
                paste: effective_paste(&settings),
                cleanup_style: effective_style(&settings),
                settings,
                started: Instant::now(),
                auto_paste: false,
                history_id: Some(id),
                reply: Some(reply),
            },
            clients: Some(clients),
            input: Input::Prepared(audio),
            salvage: false,
        });
    }

    pub(super) fn on_salvaged(&mut self, session: u64, result: Result<PreparedAudio, PipelineError>) {
        if !matches!(self.phase, Phase::Salvaging { session: current, .. } if current == session) {
            return; // cancelled meanwhile
        }
        let Phase::Salvaging { settings, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        match result {
            Ok(audio) => {
                info!(session, audio_ms = audio.audio_ms, "audio before the microphone loss kept for Try again");
                self.retry = Some(Arc::new(audio));
            }
            Err(err) => info!(session, reason = %err, "nothing worth keeping before the microphone loss"),
        }
        self.fail(ErrorKind::Microphone, settings.lang);
    }

    pub(super) fn on_transcribed(
        &mut self,
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        mut result: Result<Transcript, PipelineError>,
        cleanup_input: Option<String>,
    ) {
        let Phase::Transcribing { session: current, job, cancel, cleaning, task, .. } = &mut self.phase else {
            return;
        };
        if *current != session {
            return;
        }
        // Escape may have been queued after the worker finished cleanup. Accept
        // its decision here, before either history persistence or paste starts.
        if !job.ask
            && job.voice.is_none()
            && *cancel.borrow()
            && let (Ok(transcript), Some(input)) = (&mut result, cleanup_input)
        {
            transcript.text = input;
            transcript.cleanup_status = Some(cleanup::CleanupStatus::Skipped);
            transcript.cleanup_error = None;
        }
        if job.history_id.is_some() && job.settings.config.history.enabled && result.is_ok() {
            let history = self.env.history.clone();
            let settings = job.settings.clone();
            let history_id = job.history_id;
            let elapsed = job.started.elapsed();
            let context = HistoryContext { style_id: job.cleanup_style.id.clone(), voice: job.voice.clone() };
            let tx = self.tx.clone();
            *cleaning = false;
            // Successful history retries are saved only after cleanup acceptance.
            // The controller remains responsive while persistence runs off-thread.
            *task = tokio::spawn(async move {
                let saved = save_transcription(
                    history,
                    settings,
                    history_id,
                    audio.clone().expect("history retry has audio"),
                    result.clone(),
                    elapsed,
                    context,
                )
                .await;
                if let Ok(id) = saved {
                    let _ = tx.send(Msg::HistoryStored(id));
                } else {
                    warn!("history write failed");
                    result = Err(PipelineError::Encode(EncodeError("history retry could not be saved".into())));
                }
                let _ = tx.send(Msg::RetrySaved { session, audio, result });
            })
            .abort_handle();
            return;
        }
        self.finish_transcription(session, audio, result);
    }
}

/// Publishes only safe metadata. The cancellation signal also wakes a sleeping wait.
pub(super) async fn wait_for_retry(
    tx: mpsc::UnboundedSender<Msg>,
    session: u64,
    mut cancelled: watch::Receiver<bool>,
    progress: RetryWait,
    delay: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if *cancelled.borrow() {
            return false;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if !delay.is_zero() {
            let mut progress = progress;
            progress.seconds = remaining.as_millis().div_ceil(1000) as u64;
            let _ = tx.send(Msg::RetryProgress { session, progress: Some(progress) });
        }
        tokio::select! {
            _ = cancelled.changed() => { return false; }
            _ = tokio::time::sleep(remaining.min(Duration::from_secs(1))) => {}
        }
    }
    if !delay.is_zero() {
        let _ = tx.send(Msg::RetryProgress { session, progress: None });
    }
    true
}

/// Runs the CPU-heavy prepare step (resample + silence gate) on the blocking pool.
pub(super) async fn prepare_off_thread(
    recording: Recording,
    config: opit_core::config::RecordingConfig,
) -> Result<PreparedAudio, PipelineError> {
    tokio::task::spawn_blocking(move || pipeline::prepare_with_config(&recording, &config))
        .await
        .unwrap_or_else(|join| Err(PipelineError::Encode(EncodeError(join.to_string()))))
}
