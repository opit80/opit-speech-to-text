//! One bounded streaming worker per recording. Capture callbacks only enqueue samples.

use super::*;
use crate::platform::AudioSink;
use opit_core::{
    audio::resample::StreamingResampler,
    provider::{ProviderError, RawTranscript},
};
use tokio::task::JoinHandle;

pub(super) struct LiveRecording {
    input: Option<mpsc::Sender<(Vec<f32>, u32)>>,
    failed: Arc<AtomicBool>,
    worker: JoinHandle<Result<RawTranscript, ProviderError>>,
}

impl Drop for LiveRecording {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(super) struct LiveResult {
    recording: LiveRecording,
    deadline: tokio::time::Instant,
}

impl LiveRecording {
    pub fn start<P: Providers>(
        providers: Arc<P>,
        profile: Profile,
        terms: Vec<String>,
        session: u64,
        tx: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        // About 12 seconds at 50 callbacks/second. Overflow invalidates the live result,
        // preserving the full recording for batch instead of growing memory without limit.
        let (input, mut audio) = mpsc::channel::<(Vec<f32>, u32)>(600);
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        let worker = tokio::spawn(async move {
            let mut connection = tokio::time::timeout(Duration::from_secs(10), providers.connect_live(profile, terms))
                .await
                .map_err(|_| ProviderError::Timeout)??;
            let mut converter: Option<StreamingResampler> = None;
            let mut rate = 0;
            let mut pcm = Vec::new();
            let mut keepalive = tokio::time::interval(Duration::from_secs(4));
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            keepalive.tick().await;
            loop {
                if worker_failed.load(Ordering::Relaxed) {
                    return Err(ProviderError::Network("live audio buffer overflow".into()));
                }
                tokio::select! {
                    packet = audio.recv() => {
                        let Some((samples, sample_rate)) = packet else {
                            if let Some(converter) = &mut converter { pcm.extend(converter.finish()); }
                            if !pcm.is_empty() { connection.send_audio(&pcm).await?; }
                            return connection.finish().await;
                        };
                        if converter.is_none() {
                            rate = sample_rate;
                            converter = StreamingResampler::new(rate, 16_000);
                        }
                        if sample_rate != rate || converter.is_none() {
                            return Err(ProviderError::BadResponse("invalid live audio rate".into()));
                        }
                        pcm.extend(converter.as_mut().unwrap().push(&samples));
                        while pcm.len() >= 1600 {
                            connection.send_audio(&pcm[..1600]).await?;
                            pcm.drain(..1600);
                        }
                    }
                    update = connection.next_update() => {
                        if let Some(update) = update? { let _ = tx.send(Msg::LiveText { session, text: tail(&update.text) }); }
                    }
                    _ = keepalive.tick() => connection.keep_alive().await?,
                }
            }
        });
        Self { input: Some(input), failed, worker }
    }

    pub fn sink(&self) -> AudioSink {
        let input = self.input.as_ref().unwrap().clone();
        let failed = self.failed.clone();
        Arc::new(move |samples, rate| {
            if !failed.load(Ordering::Relaxed) && input.try_send((samples, rate)).is_err() {
                failed.store(true, Ordering::Relaxed);
            }
        })
    }

    pub fn stop(mut self) -> LiveResult {
        self.input.take();
        let abort = self.worker.abort_handle();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(4)).await;
            abort.abort();
        });
        LiveResult { recording: self, deadline: tokio::time::Instant::now() + Duration::from_secs(4) }
    }
}

impl LiveResult {
    pub async fn resolve(mut self) -> Result<RawTranscript, ProviderError> {
        let result = tokio::time::timeout_at(self.deadline, &mut self.recording.worker)
            .await
            .map_err(|_| ProviderError::Timeout)?
            .map_err(|_| ProviderError::Network("live worker stopped".into()))??;
        if self.recording.failed.load(Ordering::Relaxed) {
            return Err(ProviderError::Network("live audio buffer overflow".into()));
        }
        if result.text.trim().is_empty() {
            return Err(ProviderError::BadResponse("empty live final".into()));
        }
        Ok(result)
    }
}

pub(super) fn tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= 120 {
        return text.into();
    }
    format!("…{}", text.chars().skip(count - 119).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_tail_is_unicode_safe_and_bounded() {
        assert_eq!(tail("ğüş"), "ğüş");
        let value = tail(&"ğ".repeat(150));
        assert!(value.starts_with('…'));
        assert_eq!(value.chars().count(), 120);
    }
}
