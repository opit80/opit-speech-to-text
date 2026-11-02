//! Ordered paste completion and history persistence.

use super::*;

impl<P: Providers> Controller<P> {
    pub(super) fn finish_transcription(
        &mut self,
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        result: Result<Transcript, PipelineError>,
    ) {
        if !matches!(self.phase, Phase::Transcribing { session: current, .. } if current == session) {
            return; // cancelled meanwhile
        }
        let Phase::Transcribing { mut job, cancel, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        let lang = job.settings.lang;
        if job.ask && *cancel.borrow() {
            if let Some(reply) = job.text_reply.take() {
                reply.finish(Err(ErrorKind::Cancelled));
            }
            return self.finish_info(lang.cancelled());
        }
        if job.ask
            && let Err(error) = &result
        {
            let kind = ErrorKind::from(error);
            if let Some(reply) = job.text_reply.take() {
                reply.finish(Err(kind));
            }
            return self.fail(kind, lang);
        }
        if (!job.ask || job.ask_test)
            && let Some(reply) = job.text_reply.take()
        {
            match result {
                Ok(transcript) => {
                    self.retry = None;
                    self.record(&job, &transcript, audio.as_deref());
                    reply.finish(Ok(transcript.text));
                    if self.recording.is_some() {
                        self.show_recording();
                    } else {
                        self.env.overlay.hide();
                    }
                    self.set_state(DictationState::Idle);
                }
                Err(error) => {
                    let kind = ErrorKind::from(&error);
                    self.retry = audio;
                    reply.finish(Err(kind));
                    self.fail(kind, lang);
                }
            }
            return;
        }
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(PipelineError::TooShort) => {
                info!(session, "recording too short; not sent");
                return self.finish_info(lang.too_short());
            }
            Err(PipelineError::NoSpeech { levels }) => {
                info!(session, "no speech detected; not sent");
                return self.finish_info(&lang.no_speech(levels.speech_db));
            }
            Err(err) => {
                warn!(session, kind = ?ErrorKind::from(&err), "transcription failed");
                if let Some(reply) = job.reply.take() {
                    let _ = reply.send(Err(ErrorKind::from(&err)));
                }
                self.retry = if job.ask || job.voice.is_some() { None } else { audio };
                return self.fail(ErrorKind::from(&err), lang);
            }
        };
        self.retry = None;
        info!(
            session,
            status = transcript.status.as_str(),
            profile = %transcript.profile_id,
            fallback = transcript.used_fallback,
            request_ms = transcript.request_ms,
            chars = transcript.text.chars().count(),
            rule_hits = transcript.hits.len(),
            "transcribed"
        );
        if job.voice.is_some()
            && let Some(error) = transcript.cleanup_error
        {
            self.record(&job, &transcript, audio.as_deref());
            let message = if error == CleanupError::Truncated {
                lang.answer_truncated()
            } else if matches!(error, CleanupError::Rejected | CleanupError::BadResponse) {
                lang.voice_failed()
            } else {
                lang.error(error.into())
            };
            return self.fail_with_message(error.into(), lang, message.into());
        }
        if transcript.status != TranscriptStatus::Ok || transcript.text.is_empty() {
            self.record(&job, &transcript, audio.as_deref());
            if let Some(reply) = job.reply.take() {
                let _ = reply.send(Ok(()));
            }
            return self.finish_info(lang.empty());
        }

        let paste = &job.paste;
        let text =
            if !job.ask && paste.trailing_space { format!("{} ", transcript.text) } else { transcript.text.clone() };
        let restore = job.history_id.is_none()
            && paste.restore_clipboard
            && (!job.ask || job.settings.config.ask.output == opit_core::ask::Output::Paste);
        let auto_paste =
            job.auto_paste && (!job.ask || job.settings.config.ask.output == opit_core::ask::Output::Paste);
        let request = PasteRequest { method: paste.method, after: paste.after, restore, auto_paste };
        let paster = self.env.paster.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let outcome = paster.paste(&text, request);
            let _ = tx.send(Msg::Pasted { session, outcome });
        });
        self.phase = Phase::Pasting { session, job, transcript, audio };
        self.set_state(DictationState::Pasting);
    }

    pub(super) fn on_pasted(&mut self, session: u64, outcome: PasteOutcome) {
        if !matches!(self.phase, Phase::Pasting { session: current, .. } if current == session) {
            return;
        }
        let Phase::Pasting { mut job, transcript, audio, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        #[cfg(target_os = "linux")]
        if !matches!(outcome, PasteOutcome::Pasted) {
            self.session_state.lock().unwrap_or_else(PoisonError::into_inner).retain(&transcript.text);
            self.env.events.pending_results_changed();
        }
        let lang = job.settings.lang;
        let elapsed = job.started.elapsed();
        self.record(&job, &transcript, audio.as_deref());
        if let Some(reply) = job.text_reply.take() {
            reply.finish(if matches!(outcome, PasteOutcome::Failed(_)) {
                Err(ErrorKind::Paste)
            } else {
                Ok(transcript.text.clone())
            });
        }
        if let Some(reply) = job.reply.take() {
            let _ = reply.send(if matches!(outcome, PasteOutcome::Failed(_)) { Err(ErrorKind::Paste) } else { Ok(()) });
        }
        match outcome {
            PasteOutcome::Pasted => {
                info!(session, total_ms = elapsed.as_millis() as u64, "pasted");
                self.play(Sound::Done);
                self.show(view(Tone::Success, lang.pasted(elapsed.as_secs_f32()), Some(PASTED_HIDE)));
            }
            PasteOutcome::ClipboardOnly => {
                info!(session, "text left on the clipboard");
                self.play(Sound::Done);
                self.show(view(Tone::Warning, lang.clipboard_only(), Some(WARNING_HIDE)));
            }
            PasteOutcome::Failed(reason) => {
                warn!(session, %reason, "paste failed");
                return self.fail_with_message(
                    ErrorKind::Paste,
                    lang,
                    lang.paste_failed(job.settings.config.history.enabled).to_string(),
                );
            }
        }
        if let Some(error) = transcript.cleanup_error {
            let message = if job.ask {
                lang.ask_cleanup_failed().into()
            } else {
                lang.cleanup_failed(error, matches!(outcome, PasteOutcome::ClipboardOnly))
            };
            self.show(view(Tone::Warning, message, Some(WARNING_HIDE)));
        }
        self.set_state(DictationState::Idle);
    }

    fn record(&self, job: &Job, transcript: &Transcript, audio: Option<&PreparedAudio>) {
        let history = &job.settings.config.history;
        if !history.enabled || job.history_id.is_some() || job.voice.is_some() {
            return;
        }
        let entry = NewDictation {
            source_text: None,
            instruction: None,
            mode: if job.ask { "ask" } else { "dictation" },
            agent: job.ask.then_some(job.settings.config.ask.agent.name()),
            created_at_ms: now_ms(),
            profile_id: &transcript.profile_id,
            language: transcript.language.as_deref(),
            cleanup_status: transcript.cleanup_status,
            cleanup_model: transcript.cleanup_model.as_deref(),
            cleanup_style_id: (!job.ask).then_some(job.cleanup_style.id.as_str()),
            raw_text: &transcript.raw_text,
            text: &transcript.text,
            status: transcript.status,
            audio_ms: transcript.audio_ms,
            latency_ms: job.started.elapsed().as_millis() as u64,
            error_code: None,
        };
        let audio = if history.save_audio { audio } else { None };
        match self.env.history.record(&entry, audio, history.max_entries) {
            Ok(id) => self.env.events.history_added(id),
            Err(err) => warn!(error = %err, "history write failed"),
        }
    }

    /// `announce: false` is the quiet variant used on shutdown.
    pub(super) fn cancel(&mut self, announce: bool) {
        if let Some(pending) = &mut self.preparing {
            pending.cancelled.store(true, Ordering::Relaxed);
            return;
        }
        if let Some(r) = self.recording.take() {
            r.limit.abort();
            drop(r.capture);
            drop(r.media);
            if announce {
                self.play(Sound::Cancel);
            }
            if matches!(self.phase, Phase::Idle) && self.waiting.is_empty() {
                if announce {
                    self.show(view(Tone::Neutral, r.settings.lang.cancelled(), Some(CANCEL_HIDE)));
                    self.set_state(DictationState::Cancelled);
                    self.set_state(DictationState::Idle);
                }
            } else {
                self.show(view(Tone::Busy, r.settings.lang.transcribing(), None));
            }
            return;
        }
        let lang = match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => return,
            pasting @ Phase::Pasting { .. } => {
                self.phase = pasting; // Ctrl+V is already on its way
                return;
            }
            Phase::Salvaging { session, settings, task } => {
                task.abort();
                info!(session, "cancelled after the microphone was lost");
                settings.lang
            }
            Phase::Transcribing { session, task, job, cancel, progress, cleaning, asking } => {
                if announce && cleaning && job.voice.is_none() && !job.ask {
                    // Retain the skip even if the completed worker has dropped its receiver.
                    cancel.send_replace(true);
                    self.phase = Phase::Transcribing { session, task, job, cancel, progress, cleaning: false, asking };
                    self.show_progress();
                    self.set_state(DictationState::Transcribing);
                    return;
                }
                if progress.is_some() || asking {
                    let _ = cancel.send(true);
                    self.phase = Phase::Transcribing { session, task, job, cancel, progress, cleaning, asking };
                    return;
                }
                let _ = cancel.send(true);
                task.abort();
                info!(session, "transcription cancelled");
                job.settings.lang
            }
        };
        if announce {
            self.play(Sound::Cancel);
            self.show(view(Tone::Neutral, lang.cancelled(), Some(CANCEL_HIDE)));
            self.set_state(DictationState::Cancelled);
        } else {
            self.env.overlay.hide();
        }
        self.set_state(DictationState::Idle);
    }
}

pub(super) struct HistoryContext {
    pub(super) style_id: String,
    pub(super) voice: Option<VoiceContext>,
}

/// Dictation failure audio ignores save_audio, but obeys history.enabled at the worker call site.
/// A History retry replaces its existing row before any clipboard operation.
pub(super) async fn save_transcription(
    history: Arc<dyn HistorySink>,
    settings: Arc<Settings>,
    history_id: Option<i64>,
    audio: Arc<PreparedAudio>,
    result: Result<Transcript, PipelineError>,
    elapsed: Duration,
    context: HistoryContext,
) -> Result<i64, String> {
    tokio::task::spawn_blocking(move || {
        let HistoryContext { style_id, voice } = context;
        let failure_code = match &result {
            Err(PipelineError::Provider(error)) => Some(error.code()),
            Err(PipelineError::RetryCancelled) => Some("cancelled"),
            Err(_) => Some("audio"),
            Ok(t) => t.cleanup_error.map(|error| match error {
                CleanupError::InvalidKey => "invalid_key",
                CleanupError::PaymentRequired => "payment_required",
                CleanupError::ModelNotFound => "model_not_found",
                CleanupError::RateLimited => "rate_limited",
                CleanupError::Server => "server",
                _ => "rejected",
            }),
        };
        let transcript = result.ok();
        let entry = NewDictation {
            source_text: voice.as_ref().and_then(|v| {
                if v.mode == VoiceMode::Edit { v.selection.as_deref() } else { v.source_text.as_deref() }
            }),
            instruction: voice.as_ref().and_then(|v| v.instruction.as_deref()),
            mode: voice.as_ref().map_or("dictation", |v| v.mode.name()),
            agent: None,
            created_at_ms: now_ms(),
            profile_id: transcript.as_ref().map_or(settings.config.active_profile_id.as_str(), |t| &t.profile_id),
            raw_text: transcript.as_ref().map_or("", |t| &t.raw_text),
            text: transcript.as_ref().map_or("", |t| &t.text),
            status: transcript.as_ref().map_or(TranscriptStatus::Failed, |t| t.status),
            audio_ms: audio.audio_ms,
            latency_ms: elapsed.as_millis() as u64,
            error_code: failure_code,
            language: transcript.as_ref().and_then(|t| t.language.as_deref()),
            cleanup_status: transcript.as_ref().and_then(|t| t.cleanup_status),
            cleanup_model: transcript.as_ref().and_then(|t| t.cleanup_model.as_deref()),
            cleanup_style_id: voice.is_none().then_some(&style_id),
        };
        match history_id {
            Some(id) => {
                history.replace(id, &entry, failure_code.is_some() || settings.config.history.save_audio).map(|()| id)
            }
            None => history.record(
                &entry,
                (voice.is_none() || settings.config.history.save_audio).then_some(audio.as_ref()),
                settings.config.history.max_entries,
            ),
        }
    })
    .await
    .map_err(|_| "history worker stopped".to_string())?
}
