//! Recording, voice preparation and start-time settings snapshots.

use super::*;

fn effective_hotkey_mode(mode: HotkeyMode, supports_release: bool) -> HotkeyMode {
    if supports_release { mode } else { HotkeyMode::Toggle }
}

/// Read from the private snapshot, after any per-app overrides at recording start.
pub(super) fn effective_paste(settings: &Settings) -> PasteConfig {
    settings.config.paste.clone()
}

/// Read from the same snapshot as the profile and paste policy.
pub(super) fn effective_style(settings: &Settings) -> CleanupStyle {
    settings.config.cleanup.active_style()
}

impl<P: Providers> Controller<P> {
    pub(super) fn ask(&mut self, text: Option<String>, reply: Option<oneshot::Sender<Result<String, ErrorKind>>>) {
        let reply = reply.map(|reply| TextReply(Some(reply)));
        if self.update_holds != 0
            || self.waiting.len() + usize::from(self.recording.is_some() || self.preparing.is_some()) >= MAX_WAITING
        {
            if let Some(reply) = reply {
                reply.finish(Err(ErrorKind::Rejected));
            }
            return;
        }
        if let Some(text) = text {
            if text.trim().is_empty() || text.len() > opit_core::ask::MAX_REQUEST {
                if let Some(reply) = reply {
                    reply.finish(Err(ErrorKind::Rejected));
                }
                return;
            }
            self.session += 1;
            let settings = self.env.settings.current();
            self.waiting.push_back(Pending {
                session: self.session,
                job: Job {
                    voice: None,
                    ask: true,
                    ask_test: false,
                    text_reply: reply,
                    paste: effective_paste(&settings),
                    cleanup_style: effective_style(&settings),
                    settings,
                    started: Instant::now(),
                    auto_paste: self.env.paster.can_auto_paste(),
                    history_id: None,
                    reply: None,
                },
                clients: None,
                input: Input::Text(text),
                salvage: false,
            });
        } else if self.recording.is_none() {
            self.start();
            if let Some(recording) = self.recording.as_mut() {
                recording.ask = true;
                recording.text_reply = reply;
            } else if let Some(reply) = reply {
                reply.finish(Err(ErrorKind::Rejected));
            }
        } else if let Some(reply) = reply {
            reply.finish(Err(ErrorKind::Rejected));
        }
    }

    pub(super) fn toggle(&mut self, auto_paste: bool) {
        if self.recording.is_some() {
            self.stop(auto_paste);
        } else {
            self.start();
        }
    }

    fn voice_shortcut(&mut self, mode: VoiceMode, up: bool) {
        let hotkey_mode = effective_hotkey_mode(
            self.recording.as_ref().map_or_else(
                || {
                    self.preparing.as_ref().map_or_else(
                        || self.env.settings.current().config.hotkey.mode,
                        |p| p.settings.config.hotkey.mode,
                    )
                },
                |r| r.settings.config.hotkey.mode,
            ),
            self.env.hotkey.supports_release(),
        );
        if let Some(preparing) = &mut self.preparing {
            if preparing.mode == mode && (!up || hotkey_mode == HotkeyMode::PushToTalk) {
                preparing.cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }
        if let Some(recording) = &self.recording {
            if recording.voice.as_ref().is_some_and(|v| v.mode == mode)
                && ((!up && hotkey_mode == HotkeyMode::Toggle) || (up && hotkey_mode == HotkeyMode::PushToTalk))
            {
                self.stop(true);
            }
            return;
        }
        if !up {
            self.start_voice(mode);
        }
    }

    fn start_voice(&mut self, mode: VoiceMode) {
        if self.preparing.is_some() || self.recording.is_some() || self.held_for_update() {
            return;
        }
        if self.waiting.len() >= MAX_WAITING {
            self.show(view(Tone::Warning, self.env.settings.current().lang.queue_full(), Some(INFO_HIDE)));
            return;
        }
        let settings = self.dictation_settings();
        self.session += 1;
        self.retry = None;
        let session = self.session;
        let providers = self.env.providers.clone();
        let mic = self.env.mic.clone();
        let paster = self.env.paster.clone();
        let config = settings.config.clone();
        let tx = self.tx.clone();
        let sink: CaptureSink = Arc::new(move |event| {
            let _ = tx.send(Msg::Capture { session, event });
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let connection = opit_core::voice_modes::connection_config(
                &config.cleanup,
                (mode == VoiceMode::Translate).then_some(&config.translate),
            );
            let client = providers.cleanup_client(&connection).map_err(|_| VoiceStartError::Connection)?;
            let profile = config.active_profile().ok_or(VoiceStartError::Connection)?;
            let primary = providers.client(profile).map_err(VoiceStartError::Setup)?;
            let fallback = config.fallback_for(profile).and_then(|p| providers.client(p).ok());
            let selection = if mode == VoiceMode::Edit {
                paster.read_selection().map_err(VoiceStartError::Selection)?
            } else {
                None
            };
            if worker_cancelled.load(Ordering::Relaxed) {
                return Err(VoiceStartError::Connection);
            }
            let started =
                mic.start(config.recording.microphone.as_deref(), sink).map_err(|_| VoiceStartError::Microphone)?;
            Ok(VoicePrepared {
                voice: VoiceContext { mode, selection, source_text: None, instruction: None, client },
                clients: Clients { primary, fallback },
                started,
            })
        });
        let tx = self.tx.clone();
        let task = tokio::spawn(async move {
            let result = worker.await.unwrap_or(Err(VoiceStartError::Connection));
            let _ = tx.send(Msg::VoiceReady(session));
            result
        });
        self.preparing =
            Some(Preparing { session, mode, settings: settings.clone(), cancelled, microphone_failed: false, task });
        self.env.hotkey.set_capture_escape(true);
        self.show(view(Tone::Busy, settings.lang.voice_preparing(), None));
        self.set_state(DictationState::Transcribing);
    }

    pub(super) async fn voice_ready(&mut self, session: u64) {
        if !self.preparing.as_ref().is_some_and(|p| p.session == session) {
            return;
        }
        let pending = self.preparing.take().unwrap();
        let lang = pending.settings.lang;
        let result = pending.task.await.unwrap_or(Err(VoiceStartError::Connection));
        if pending.cancelled.load(Ordering::Relaxed) {
            drop(result);
            self.show(view(Tone::Neutral, lang.cancelled(), Some(CANCEL_HIDE)));
            self.set_state(DictationState::Cancelled);
            self.set_state(DictationState::Idle);
            return;
        }
        if pending.microphone_failed {
            drop(result);
            return self.fail(ErrorKind::Microphone, lang);
        }
        match result {
            Ok(ready) => {
                self.activate_recording(session, pending.settings, ready.clients, ready.started, Some(ready.voice))
            }
            Err(VoiceStartError::Connection) => {
                self.fail_with_message(ErrorKind::MissingKey, lang, lang.voice_connection_required().into())
            }
            Err(VoiceStartError::Selection(error)) => self.fail_with_message(
                ErrorKind::Rejected,
                lang,
                if error == SelectionError::TooLong { lang.selection_too_long() } else { lang.selection_failed() }
                    .into(),
            ),
            Err(VoiceStartError::Setup(error)) => self.fail(error.kind(), lang),
            Err(VoiceStartError::Microphone) => self.fail(ErrorKind::Microphone, lang),
        }
    }

    pub(super) fn on_hotkey(&mut self, event: HotkeyEvent) {
        match event {
            HotkeyEvent::Edit => return self.voice_shortcut(VoiceMode::Edit, false),
            HotkeyEvent::EditUp => return self.voice_shortcut(VoiceMode::Edit, true),
            HotkeyEvent::Translate => return self.voice_shortcut(VoiceMode::Translate, false),
            HotkeyEvent::TranslateUp => return self.voice_shortcut(VoiceMode::Translate, true),
            _ => {}
        }
        let mode = effective_hotkey_mode(
            self.recording
                .as_ref()
                .map_or_else(|| self.env.settings.current().config.hotkey.mode, |r| r.settings.config.hotkey.mode),
            self.env.hotkey.supports_release(),
        );
        let recording = self.recording.is_some();
        match (event, mode) {
            (HotkeyEvent::Escape, _) => self.cancel(true),
            (HotkeyEvent::Ask, _) => {
                if recording {
                    self.stop(true);
                } else {
                    self.ask(None, None);
                }
            }
            (HotkeyEvent::Pause, _) => self.pause_resume(),
            (HotkeyEvent::ComboDown, HotkeyMode::Toggle) => self.toggle(true),
            (HotkeyEvent::LoneCtrl, HotkeyMode::Toggle) if recording => self.stop(true),
            (HotkeyEvent::ComboDown, HotkeyMode::PushToTalk) if !recording => self.start(),
            (HotkeyEvent::ComboUp, HotkeyMode::PushToTalk)
                if self.recording.as_ref().is_some_and(|r| r.voice.is_none()) =>
            {
                self.stop(true)
            }
            _ => {}
        }
    }

    pub(super) fn select_style(&mut self, id: Option<String>) {
        let settings = self.env.settings.current();
        let config = &settings.config.cleanup;
        let style = match id {
            Some(id) => config.style(&id).cloned(),
            None => {
                let current = self
                    .recording
                    .as_ref()
                    .filter(|r| r.voice.is_none())
                    .map(|r| r.cleanup_style.id.as_str())
                    .or_else(|| self.pending_style.as_ref().map(|pending| pending.style.id.as_str()))
                    .unwrap_or(&config.active_style_id);
                config.next_style(current)
            }
        };
        let Some(style) = style else { return };
        self.queue_style(style);
    }

    pub(super) fn queue_style(&mut self, style: CleanupStyle) {
        let recording = self.recording.as_ref().filter(|r| r.voice.is_none());
        if self.pending_style.is_none() {
            self.style_fallback = Some(recording.map_or_else(
                || self.env.settings.current().config.cleanup.active_style(),
                |r| r.cleanup_style.clone(),
            ));
        }
        self.style_generation += 1;
        let generation = self.style_generation;
        if self.style_writer.send(StyleWrite { generation, id: style.id.clone() }).is_err() {
            return;
        }
        self.pending_style =
            Some(PendingStyle { generation, style: style.clone(), recording_session: recording.map(|r| r.session) });
        // A following Stop must use this choice even while persistence is blocked.
        self.style_changed(style);
    }

    pub(super) fn style_saved(&mut self, generation: u64, saved: Option<CleanupStyle>) {
        let Some(pending) = &self.pending_style else { return };
        if generation > pending.generation {
            return;
        }
        if let Some(style) = &saved {
            self.style_fallback = Some(style.clone());
        }
        if generation != pending.generation {
            return;
        }
        let pending = self.pending_style.take().unwrap();
        let fallback = self.style_fallback.take();
        // Never apply an older acknowledgement to a newer recording or explicit tray/UI choice.
        let current_session = self.recording.as_ref().map(|r| r.session);
        if saved.is_none()
            && current_session == pending.recording_session
            && let Some(style) = fallback
        {
            self.style_changed(style);
        }
    }

    pub(super) fn style_settings(&self) -> Arc<Settings> {
        let settings = self.env.settings.current();
        let Some(pending) = &self.pending_style else { return settings };
        let mut config = settings.config.clone();
        config.cleanup.active_style_id = pending.style.id.clone();
        if let Some(style) = config.cleanup.styles.iter_mut().find(|s| s.id == pending.style.id) {
            *style = pending.style.clone();
        } else {
            config.cleanup.styles.push(pending.style.clone());
        }
        Arc::new(Settings { config, rules: settings.rules.clone(), lang: settings.lang })
    }

    pub(super) fn style_changed(&mut self, style: CleanupStyle) {
        if let Some(recording) = self.recording.as_mut() {
            if recording.voice.is_none() {
                recording.cleanup_style = style;
            }
            self.show_recording();
        } else {
            let lang = self.env.settings.current().lang;
            self.show(view(Tone::Neutral, lang.style_name(&style), Some(Duration::from_millis(1500))));
        }
    }

    pub(super) fn start(&mut self) {
        if self.preparing.is_some() {
            return;
        }
        if self.held_for_update() {
            return;
        }
        if self.waiting.len() >= MAX_WAITING {
            let lang = self.env.settings.current().lang;
            self.env.overlay.show(view(Tone::Warning, lang.queue_full(), Some(INFO_HIDE)));
            return;
        }
        let settings = self.dictation_settings();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot start dictation");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.retry = None;
        self.session += 1;
        let session = self.session;
        let tx = self.tx.clone();
        let sink: CaptureSink = Arc::new(move |event| {
            let _ = tx.send(Msg::Capture { session, event });
        });
        let started = match self.env.mic.start(settings.config.recording.microphone.as_deref(), sink) {
            Ok(started) => started,
            Err(err) => {
                warn!(error = %err, "microphone failed to start");
                return self.fail(ErrorKind::Microphone, settings.lang);
            }
        };
        self.activate_recording(session, settings, clients, started, None);
    }

    fn activate_recording(
        &mut self,
        session: u64,
        settings: Arc<Settings>,
        clients: Clients<P::Client>,
        started: Started,
        voice: Option<VoiceContext>,
    ) {
        let media = MediaGuard::engage(self.env.media.clone(), &settings.config.media);
        if started.fell_back {
            warn!(device = %started.device, "configured microphone unavailable; using the default device");
        }
        self.env.providers.warm_up(&clients.primary);
        let cleanup_config = &settings.config.cleanup;
        let uses_cleanup = settings.config.active_profile().is_some_and(|p| p.apply_cleanup)
            || settings
                .config
                .active_profile()
                .and_then(|p| settings.config.fallback_for(p))
                .is_some_and(|p| p.apply_cleanup);
        if voice.is_none() && cleanup_config.enabled && uses_cleanup && cleanup_config.active_style_id != "raw" {
            let config = cleanup_config.for_style(&effective_style(&settings));
            let providers = self.env.providers.clone();
            tokio::spawn(async move {
                if let Ok(Ok(client)) = tokio::task::spawn_blocking(move || providers.cleanup_client(&config)).await {
                    client.warm_up().await;
                }
            });
        }

        let max = Duration::from_secs(u64::from(settings.config.recording.max_seconds));
        let limit = self.arm_limit(session, max);

        let label = if let Some(voice) = &voice {
            match voice.mode {
                VoiceMode::Edit => settings.lang.edit_label(),
                VoiceMode::Translate => settings.lang.translate_label(),
            }
        } else if started.fell_back {
            settings.lang.listening_default_mic()
        } else {
            settings.lang.listening()
        };
        self.env.hotkey.set_capture_escape(true);
        self.play(Sound::Start);
        info!(session, device = %started.device, profile = %settings.config.active_profile_id, "recording started");
        let live = settings
            .config
            .active_profile()
            .filter(|p| settings.config.recording.live_text && p.supports_streaming())
            .map(|profile| {
                let terms = opit_core::rules::prompt::keywords(settings.rules.terms());
                let live = live::LiveRecording::start(
                    self.env.providers.clone(),
                    profile.clone(),
                    terms,
                    session,
                    self.tx.clone(),
                );
                started.capture.set_audio_sink(Some(live.sink()));
                live
            });
        self.recording = Some(ActiveRecording {
            live,
            live_text: String::new(),
            voice,
            media,
            ask: false,
            text_reply: None,
            session,
            capture: started.capture,
            paste: effective_paste(&settings),
            cleanup_style: effective_style(&settings),
            settings,
            clients,
            limit,
            label,
            remaining: max,
            resumed_at: tokio::time::Instant::now(),
            paused: false,
            level: 0.0,
        });
        self.show_recording();
        self.set_state(DictationState::Recording);
    }

    fn dictation_settings(&self) -> Arc<Settings> {
        let settings = self.style_settings();
        let app_rules = &settings.config.app_rules;
        if !app_rules.enabled || !app_rules.rules.iter().any(|r| r.enabled) {
            return settings;
        }
        let Some(app) = self.env.foreground.current(app_rules.needs_title()) else { return settings };
        let Some(rule) = app_rules.matching(&app.exe, &app.title) else { return settings };
        let mut config = settings.config.clone();
        rule.apply(&mut config);
        info!("app rule applied");
        Arc::new(Settings { config, rules: settings.rules.clone(), lang: settings.lang })
    }

    pub(super) fn stop(&mut self, auto_paste: bool) {
        let Some(r) = self.recording.take() else { return };
        r.limit.abort();
        let auto_paste = auto_paste && self.env.paster.can_auto_paste();
        let recording = r.capture.finish();
        let live = r.live.map(live::LiveRecording::stop);
        drop(r.media);
        self.waiting.push_back(Pending {
            session: r.session,
            job: Job {
                voice: r.voice,
                text_reply: r.text_reply,
                ask: r.ask,
                ask_test: false,
                paste: r.paste,
                cleanup_style: r.cleanup_style,
                settings: r.settings,
                started: Instant::now(),
                auto_paste,
                history_id: None,
                reply: None,
            },
            clients: Some(r.clients),
            input: Input::Raw(recording, live),
            salvage: false,
        });
    }

    pub(super) fn on_capture(&mut self, session: u64, event: CaptureEvent) {
        if let Some(preparing) = &mut self.preparing
            && preparing.session == session
        {
            preparing.microphone_failed |= matches!(event, CaptureEvent::Failed(_));
            return;
        }
        let Some(r) = self.recording.as_mut().filter(|r| r.session == session) else { return };
        match event {
            CaptureEvent::Level(level) => {
                if !r.paused {
                    r.level = level;
                    self.show_recording();
                }
            }
            CaptureEvent::Failed(reason) => {
                warn!(session, %reason, "microphone stopped during recording");
                let r = self.recording.take().unwrap();
                r.limit.abort();
                if r.ask || r.voice.is_some() {
                    if let Some(reply) = r.text_reply {
                        reply.finish(Err(ErrorKind::Microphone));
                    }
                    drop(r.capture);
                    drop(r.media);
                    return self.fail(ErrorKind::Microphone, r.settings.lang);
                }
                if let Some(reply) = r.text_reply {
                    reply.finish(Err(ErrorKind::Microphone));
                }
                // Keep what was said so far (spec §8): prepare it, but send nothing on our own.
                let recording = r.capture.finish();
                drop(r.media);
                self.waiting.push_back(Pending {
                    session,
                    job: Job {
                        voice: None,
                        ask: false,
                        ask_test: false,
                        text_reply: None,
                        paste: r.paste,
                        cleanup_style: r.cleanup_style,
                        settings: r.settings,
                        started: Instant::now(),
                        auto_paste: false,
                        history_id: None,
                        reply: None,
                    },
                    clients: Some(r.clients),
                    input: Input::Raw(recording, None),
                    salvage: true,
                });
            }
        }
    }

    pub(super) fn arm_limit(&self, session: u64, remaining: Duration) -> AbortHandle {
        let tx = self.tx.clone();
        let deadline = tokio::time::Instant::now() + remaining;
        tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            let _ = tx.send(Msg::LimitReached { session });
        })
        .abort_handle()
    }

    pub(super) fn pause_resume(&mut self) {
        let Some(r) = self.recording.as_mut() else { return };
        r.limit.abort();
        if !r.paused {
            r.remaining = r.remaining.saturating_sub(r.resumed_at.elapsed());
        }
        r.paused = !r.paused;
        r.capture.set_paused(r.paused);
        r.resumed_at = tokio::time::Instant::now();
        if !r.paused {
            let (session, remaining) = (r.session, r.remaining);
            let limit = self.arm_limit(session, remaining);
            self.recording.as_mut().unwrap().limit = limit;
        }
        self.show_recording();
        self.set_state(DictationState::Recording);
    }
}
