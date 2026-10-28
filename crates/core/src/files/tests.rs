use super::*;
use crate::audio::encode::{AudioFormat, encode};
use crate::provider::{OpenAiCompatible, ResponseFormat, presets};
use crate::rules::RuleSet;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn segment(start_ms: u64, end_ms: u64, text: &str) -> Segment {
    Segment { start_ms, end_ms, text: text.into(), speaker: None }
}

#[test]
fn generated_wav_and_flac_decode_without_external_tools() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let tone = (0..16000).map(|i| (i as f32 * 0.15).sin() * 0.4).collect::<Vec<_>>();
    for format in [AudioFormat::Wav, AudioFormat::Flac] {
        let audio = encode(&tone, format).unwrap();
        let path = dir.path().join(audio.file_name());
        std::fs::write(&path, audio.bytes).unwrap();
        let decoded = decode::decode(&path, &cancel).unwrap_or_else(|e| panic!("{format:?}: {e:?}"));
        assert_eq!(decoded.len(), tone.len());
        assert!(decoded.iter().zip(&tone).all(|(a, b)| (a - b).abs() < 0.001));
    }
    let path = dir.path().join("stereo.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..48000 {
        writer.write_sample(12000_i16).unwrap();
        writer.write_sample(-4000_i16).unwrap();
    }
    writer.finalize().unwrap();
    let samples = decode::decode(&path, &cancel).unwrap();
    assert_eq!(samples.len(), 16000);
    assert!((samples[8000] - 4000.0 / 32768.0).abs() < 0.001);
    std::fs::write(&path, b"unsupported container").unwrap();
    assert!(matches!(decode::decode(&path, &cancel), Err(FileError::Decode("container"))));
    cancel.store(true, Ordering::Relaxed);
    assert!(matches!(decode::decode(&path, &cancel), Err(FileError::Cancelled)));
}

#[test]
fn tiny_final_chunks_remain_valid_fixed_block_flac() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tail.flac");
    for length in [1, 17, 4097] {
        std::fs::write(&path, encode(&vec![0.25; length], AudioFormat::Flac).unwrap().bytes).unwrap();
        let decoded = decode::decode(&path, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.len(), length);
    }
}

#[test]
fn cuts_at_quiet_frames_without_gaps_or_oversized_chunks() {
    let mut samples = vec![0.5; 50000];
    samples[14500..15500].fill(0.0);
    let ranges = chunk_ranges(&samples, 16000);
    assert!((14400..15500).contains(&ranges[0].end));
    assert_eq!(ranges.first().unwrap().start, 0);
    assert_eq!(ranges.last().unwrap().end, samples.len());
    assert!(ranges.windows(2).all(|r| r[0].end == r[1].start));
    assert!(ranges.iter().all(|r| r.len() <= 16000));
    assert_eq!(chunk_ranges(&[], 16000), Vec::<Range<usize>>::new());
}

#[test]
fn timestamps_offset_and_exports_preserve_numbering_and_unicode_width() {
    let mut segments = vec![segment(1250, 2500, "Hello world"), segment(3000, 4100, &"ş".repeat(87))];
    assert!(offset_segments(&mut segments, 16000 * 3600, 16000 * 5));
    assert_eq!(segments[0].start_ms, 3601250);
    let result =
        FileResult { text: "Hello world".into(), segments, timestamps_available: true, ..FileResult::default() };
    let srt = render_srt(&result, false).unwrap();
    assert!(srt.starts_with("1\n01:00:01,250 --> 01:00:02,500\nHello world\n\n2\n"));
    assert!(srt.lines().filter(|l| l.starts_with('ş')).all(|l| l.chars().count() <= 42));
    assert!(render_txt(&result, true, false).starts_with("[1:00:01] Hello world"));
    let mut short = result.clone();
    short.segments[0].start_ms = 1250;
    assert!(render_txt(&short, true, false).starts_with("[00:01]"));
    short.timestamps_available = false;
    assert!(render_srt(&short, false).is_none());
    assert_eq!(render_txt(&short, true, false), short.text);
    assert!(!offset_segments(&mut [segment(5, 2, "bad")], 0, 16000));
    assert!(!offset_segments(&mut [segment(0, 500, "a"), segment(100, 700, "b")], 0, 16000));
    assert!(timed_segment(Some(f64::NAN), Some(2.0), "bad", None).is_none());
}

#[test]
fn subtitle_cleanup_rejects_moved_dropped_shortened_or_changed_markers() {
    let segments = vec![segment(0, 1000, "hello there"), segment(1000, 2000, "how are you")];
    assert_eq!(
        validate_subtitle_cleanup(&segments, "[0:1000] Hello there.\n[1000:2000] How are you?").unwrap().len(),
        2
    );
    for answer in [
        "[0:1000] Hi\n[1000:2000] How are you?",
        "[0:1000] Hello there.",
        "[1:1000] Hello there.\n[1000:2000] How are you?",
        "[1000:2000] How are you?\n[0:1000] Hello there.",
    ] {
        assert_eq!(validate_subtitle_cleanup(&segments, answer), Err(CleanupError::BadResponse));
    }
}

#[tokio::test]
async fn two_chunk_http_job_offsets_segments_and_reuses_retry_ladder() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"text":"hello there", "segments":[{"start":0.1,"end":0.8,"text":"hello there"}]}),
        ))
        .expect(2)
        .mount(&server)
        .await;
    let mut profile = presets::custom("file", "file", &format!("{}/v1", server.uri()), "test");
    profile.response_format = ResponseFormat::VerboseJson;
    let client = OpenAiCompatible::new(profile, None).unwrap();
    let rules = RuleSet::empty();
    let ctx = PipelineContext {
        primary: &client,
        fallback: None,
        rules: &rules,
        prompt_context: "",
        retry_delay: Duration::ZERO,
        numbers_as_words: false,
    };
    let mut progress = Vec::new();
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("audio.wav");
    std::fs::write(&path, encode(&vec![0.4; 32000], AudioFormat::Wav).unwrap().bytes).unwrap();
    let cancel = AtomicBool::new(false);
    let samples = decode::decode(&path, &cancel).unwrap();
    let result = transcribe(&samples, &ctx, None, &cancel, 16000, |p| progress.push(p)).await.unwrap();
    assert_eq!(result.segments.iter().map(|s| s.start_ms).collect::<Vec<_>>(), [100, 1100]);
    assert!(result.timestamps_available);
    assert_eq!(result.text, "hello there\nhello there");
    assert_eq!(progress.last().unwrap().percent, 100);
    assert!(render_srt(&result, false).unwrap().contains("2\n00:00:01,100 --> 00:00:01,800"));
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| String::from_utf8_lossy(&r.body).contains("verbose_json")));
}

#[tokio::test]
async fn file_mode_falls_back_to_plain_json_and_disables_all_subtitles() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(400))
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"plain text"})))
        .expect(1)
        .mount(&server)
        .await;
    let profile = presets::custom("file", "file", &format!("{}/v1", server.uri()), "test");
    let client = OpenAiCompatible::new(profile, None).unwrap().for_file();
    let rules = RuleSet::empty();
    let ctx = PipelineContext {
        primary: &client,
        fallback: None,
        rules: &rules,
        prompt_context: "",
        retry_delay: Duration::ZERO,
        numbers_as_words: false,
    };
    let result = transcribe(&vec![0.4; 16000], &ctx, None, &AtomicBool::new(false), 16000, |_| {}).await.unwrap();
    assert_eq!(result.text, "plain text");
    assert!(!result.timestamps_available);
    assert!(render_srt(&result, false).is_none());
    let requests = server.received_requests().await.unwrap();
    assert!(String::from_utf8_lossy(&requests[0].body).contains("verbose_json"));
    assert!(!String::from_utf8_lossy(&requests[1].body).contains("verbose_json"));
}

#[tokio::test]
async fn cancellation_drops_an_inflight_request_and_never_starts_another_chunk() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
                .set_body_json(serde_json::json!({"text":"late"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client =
        OpenAiCompatible::new(presets::custom("file", "file", &format!("{}/v1", server.uri()), "test"), None).unwrap();
    let rules = RuleSet::empty();
    let ctx = PipelineContext {
        primary: &client,
        fallback: None,
        rules: &rules,
        prompt_context: "",
        retry_delay: Duration::ZERO,
        numbers_as_words: false,
    };
    let cancel = AtomicBool::new(false);
    let trigger = async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        cancel.store(true, Ordering::Relaxed);
    };
    let samples = vec![0.4; 32000];
    let run = transcribe(&samples, &ctx, None, &cancel, 16000, |_| {});
    let (result, ()) =
        tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(run, trigger) }).await.unwrap();
    assert!(matches!(result, Err(FileError::Cancelled)));
}

#[tokio::test]
async fn cleanup_never_repeats_replacements_with_or_without_timestamps() {
    for timed in [false, true] {
        let server = MockServer::start().await;
        let segments = if timed {
            serde_json::json!([{"start":0,"end":1,"text":"alpha foo omega github"}])
        } else {
            serde_json::json!([])
        };
        Mock::given(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text":"alpha foo omega github", "segments": segments
            })))
            .mount(&server)
            .await;
        Mock::given(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices":[{"message":{"content":format!("[0:{}] alpha foo bar omega github", if timed { 1000 } else { 0 })},"finish_reason":"stop"}]
            })))
            .mount(&server).await;
        let client =
            OpenAiCompatible::new(presets::custom("file", "file", &format!("{}/v1", server.uri()), "test"), None)
                .unwrap()
                .for_file();
        let cleanup = CleanupClient::new(
            crate::cleanup::CleanupConfig {
                base_url: format!("{}/v1", server.uri()),
                model: "test".into(),
                api_key_ref: None,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let pack = crate::rules::RulePack::from_yaml(
            "schema: 1\nid: test\nname: test\nlanguage: en\nterms: [GitHub]\nreplacements:\n  - { from: foo, to: foo bar }\n"
        ).unwrap();
        let rules = RuleSet::compile(&[pack]).0;
        let ctx = PipelineContext {
            primary: &client,
            fallback: None,
            rules: &rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
            numbers_as_words: false,
        };
        let result = transcribe(
            &vec![0.4; 16000],
            &ctx,
            Some(FileCleanup { client: &cleanup, ui_language: "en" }),
            &AtomicBool::new(false),
            16000,
            |_| {},
        )
        .await
        .unwrap();
        assert!(!result.cleanup_failed);
        assert_eq!(result.text, "alpha foo bar omega GitHub");
        assert_eq!(result.timestamps_available, timed);
        if timed {
            assert_eq!(result.segments[0].text, result.text);
        }
    }
}

#[tokio::test]
async fn subtitle_cleanup_http_preserves_markers_and_falls_back_on_shortening() {
    for (answer, failed, expected) in
        [("[100:800] Hello there!", false, "Hello there!"), ("[100:800] Hello", true, "hello there")]
    {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"text":"hello there","language":"en","segments":[{"start":0.1,"end":0.8,"text":"hello there"}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"choices":[{"message":{"content":answer},"finish_reason":"stop"}]}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client =
            OpenAiCompatible::new(presets::custom("file", "file", &format!("{}/v1", server.uri()), "test"), None)
                .unwrap()
                .for_file();
        let cleanup = crate::cleanup::CleanupClient::new(
            crate::cleanup::CleanupConfig {
                base_url: format!("{}/v1", server.uri()),
                model: "test".into(),
                api_key_ref: None,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let rules = RuleSet::empty();
        let ctx = PipelineContext {
            primary: &client,
            fallback: None,
            rules: &rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
            numbers_as_words: false,
        };
        let result = transcribe(
            &vec![0.4; 16000],
            &ctx,
            Some(FileCleanup { client: &cleanup, ui_language: "en" }),
            &AtomicBool::new(false),
            16000,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(result.cleanup_failed, failed);
        assert_eq!(result.segments[0].text, expected);
        assert_eq!((result.segments[0].start_ms, result.segments[0].end_ms), (100, 800));
        let requests = server.received_requests().await.unwrap();
        let request: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert!(request["messages"][0]["content"].as_str().unwrap().contains("Never merge, split, drop, shorten"));
    }
}
