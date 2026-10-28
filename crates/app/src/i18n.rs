//! Strings the Rust side shows itself (overlay, tray). The Svelte UI has its own i18n.

use serde::Serialize;

use crate::controller::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Tr,
}

/// `ui_language` from the config wins; `None` follows the system locale (e.g. `tr-TR`).
pub fn resolve(ui_language: Option<&str>, system_locale: Option<&str>) -> Lang {
    let pick = ui_language.or(system_locale).unwrap_or("en");
    if pick.to_ascii_lowercase().starts_with("tr") { Lang::Tr } else { Lang::En }
}

pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

impl Lang {
    pub fn edit_label(self) -> &'static str {
        self.pick("Edit", "Düzenle")
    }
    pub fn translate_label(self) -> &'static str {
        self.pick("Translate", "Çevir")
    }
    pub fn voice_preparing(self) -> &'static str {
        self.pick("Preparing voice mode…", "Sesli mod hazırlanıyor…")
    }
    pub fn voice_connection_required(self) -> &'static str {
        self.pick(
            "Configure the cleanup connection and API key before using Edit or Translate.",
            "Düzenle veya Çevir için önce metin düzenleme bağlantısını ve API anahtarını ayarlayın.",
        )
    }
    pub fn voice_failed(self) -> &'static str {
        self.pick(
            "Edit or translation failed. No text was pasted.",
            "Düzenleme veya çeviri başarısız. Metin yapıştırılmadı.",
        )
    }
    pub fn selection_failed(self) -> &'static str {
        self.pick(
            "The selection could not be read safely. Try again.",
            "Seçili metin güvenli biçimde okunamadı. Yeniden deneyin.",
        )
    }
    pub fn selection_too_long(self) -> &'static str {
        self.pick("Select at most 32,000 characters.", "En fazla 32.000 karakter seçin.")
    }
    pub fn application_list_failed(self) -> &'static str {
        match self {
            Self::En => "The application list could not be read.",
            Self::Tr => "Uygulama listesi okunamadı.",
        }
    }

    pub fn loading_local(self) -> &'static str {
        self.pick("Loading local model…", "Yerel model yükleniyor…")
    }
    pub fn ask_cleanup_failed(self) -> &'static str {
        self.pick(
            "Request cleanup failed. The agent received the recognized request.",
            "İstek düzenlenemedi. Algılanan istek ajana gönderildi.",
        )
    }
    pub fn ask_invalid(self) -> &'static str {
        self.pick("Invalid Ask settings or working directory.", "Sorma ayarları veya çalışma dizini geçersiz.")
    }
    pub fn ask_detection_failed(self) -> &'static str {
        self.pick("Agent detection failed.", "Ajan algılama başarısız.")
    }
    pub fn tray_ask(self) -> &'static str {
        self.pick("Ask…", "Sor…")
    }
    pub fn new_conversation(self) -> &'static str {
        self.pick("New conversation", "Yeni konuşma")
    }
    pub fn asking(self, agent: &str, seconds: u64) -> String {
        match self {
            Self::En => format!("Asking {agent}… {seconds}s"),
            Self::Tr => format!("{agent} yanıtlıyor… {seconds} sn"),
        }
    }

    pub fn file_media_filter(self) -> &'static str {
        self.pick("Audio / video", "Ses / video")
    }

    pub fn file_decode(self, container: &str, format: &str) -> String {
        let format = match format {
            "read" => self.pick("file read", "dosya okuma"),
            "container" => self.pick("container format", "kapsayıcı biçimi"),
            "audio track" => self.pick("audio track", "ses parçası"),
            "audio codec" => self.pick("audio codec", "ses kodeği"),
            "packet" => self.pick("audio packet", "ses paketi"),
            "sample rate" => self.pick("sample rate", "örnekleme hızı"),
            "sample rate change" => self.pick("sample rate change", "örnekleme hızı değişimi"),
            "channels" => self.pick("channel layout", "kanal düzeni"),
            "empty audio" => self.pick("empty audio", "boş ses"),
            _ => format,
        };
        let container = if container == "media" { self.pick("media", "medya") } else { container };
        match self {
            Self::En => format!(
                "Cannot decode {container} ({format}). The file may be damaged or its audio codec is unsupported."
            ),
            Self::Tr => {
                format!("{container} ({format}) çözümlenemedi. Dosya bozuk olabilir veya ses kodeği desteklenmiyor.")
            }
        }
    }
    pub fn shortcut_collision(self) -> &'static str {
        self.pick("The shortcuts overlap", "Kısayollar çakışıyor")
    }

    pub fn pause_escape_reserved(self) -> &'static str {
        self.pick("Escape is reserved for cancelling dictation", "Escape, dikteyi iptal etmek için ayrılmıştır")
    }
    pub fn queue_full(self) -> &'static str {
        self.pick("Queue full: wait for a dictation to finish", "Kuyruk dolu: bir diktenin tamamlanmasını bekleyin")
    }

    pub fn recording_paused(self) -> &'static str {
        self.pick("Recording paused", "Kayıt duraklatıldı")
    }

    pub fn tray_recording_pause(self, paused: bool) -> &'static str {
        if paused {
            self.pick("Resume recording", "Kaydı sürdür")
        } else {
            self.pick("Pause recording", "Kaydı duraklat")
        }
    }

    pub fn with_waiting(self, label: &str, waiting: usize) -> String {
        if waiting == 0 {
            return label.into();
        }
        match self {
            // The overlay truncates long labels; keep the count visible even with a fallback mic label.
            Self::En => format!("{waiting} waiting · {label}"),
            Self::Tr => format!("{waiting} bekliyor · {label}"),
        }
    }
    fn pick(self, en: &'static str, tr: &'static str) -> &'static str {
        match self {
            Lang::En => en,
            Lang::Tr => tr,
        }
    }

    pub fn listening(self) -> &'static str {
        self.pick("Listening", "Dinleniyor")
    }

    pub fn listening_default_mic(self) -> &'static str {
        self.pick("Listening (default microphone)", "Dinleniyor (varsayılan mikrofon)")
    }

    pub fn cleaning(self) -> &'static str {
        self.pick("Cleaning text…", "Metin düzenleniyor…")
    }

    pub fn cleaning_skippable(self) -> &'static str {
        self.pick("Cleaning text… Esc to skip", "Metin düzenleniyor… Atlamak için Esc")
    }

    pub fn answer_truncated(self) -> &'static str {
        self.pick("The model ran out of answer tokens", "Modelin yanıt token sınırına ulaşıldı")
    }

    pub fn cleanup_failed(self, error: opit_core::cleanup::CleanupError, clipboard_only: bool) -> String {
        use opit_core::cleanup::CleanupError::*;
        let reason = match error {
            MissingKey => self.error(ErrorKind::MissingKey),
            InvalidKey => self.error(ErrorKind::InvalidKey),
            Network => self.error(ErrorKind::Network),
            Timeout => self.error(ErrorKind::Timeout),
            Rejected => self.error(ErrorKind::Rejected),
            BadResponse => self.error(ErrorKind::BadResponse),
            Truncated => self.answer_truncated(),
        };
        let destination = if clipboard_only {
            self.pick("Original text on clipboard", "Özgün metin panoda")
        } else {
            self.pick("Original text pasted", "Özgün metin yapıştırıldı")
        };
        format!("{}: {reason}. {destination}", self.pick("Cleanup failed", "Düzenleme başarısız"))
    }

    pub fn transcribing(self) -> &'static str {
        self.pick("Transcribing…", "Çevriliyor…")
    }

    pub fn retry_wait(self, progress: opit_core::provider::retry::RetryWait) -> String {
        let reason = match progress.kind {
            "rate_limited" => self.error(ErrorKind::RateLimited),
            "server" => self.error(ErrorKind::Server),
            "timeout" => self.error(ErrorKind::Timeout),
            _ => self.error(ErrorKind::Network),
        };
        let (seconds, attempt, total) = (progress.seconds, progress.attempt, progress.total);
        match self {
            Self::En => format!("{reason} — retrying in {seconds} s ({attempt}/{total})"),
            Self::Tr => format!("{reason} — {seconds} sn sonra tekrar denenecek ({attempt}/{total})"),
        }
    }

    /// `Pasted (0.9 s)` / `Yapıştırıldı (0,9 sn)`.
    pub fn pasted(self, seconds: f32) -> String {
        match self {
            Lang::En => format!("Pasted ({seconds:.1} s)"),
            Lang::Tr => format!("Yapıştırıldı ({} sn)", format!("{seconds:.1}").replace('.', ",")),
        }
    }

    pub fn clipboard_only(self) -> &'static str {
        self.pick("In clipboard — press Ctrl+V to paste", "Panoda — Ctrl+V ile yapıştır")
    }

    /// The clipboard could not be written, so nothing was pasted.
    pub fn paste_failed(self, history_on: bool) -> &'static str {
        if history_on {
            self.pick("Could not paste — text is in History", "Yapıştırılamadı — metin Geçmiş'te")
        } else {
            self.pick("Could not paste the text", "Metin yapıştırılamadı")
        }
    }

    pub fn no_speech(self, db: f32) -> String {
        let label = self.pick("No speech detected", "Konuşma algılanmadı");
        format!("{label} ({db:.0} dB)")
    }

    pub fn too_short(self) -> &'static str {
        self.pick("Recording too short", "Kayıt çok kısa")
    }

    pub fn empty(self) -> &'static str {
        self.pick("No text came back", "Metin çıkmadı")
    }

    pub fn cancelled(self) -> &'static str {
        self.pick("Cancelled", "İptal edildi")
    }

    pub fn retry(self) -> &'static str {
        self.pick("Try again", "Tekrar dene")
    }

    pub fn open_settings(self) -> &'static str {
        self.pick("Settings", "Ayarlar")
    }

    pub fn config_reset(self) -> &'static str {
        self.pick("Settings file was damaged; defaults loaded", "Ayar dosyası bozuktu; varsayılanlar yüklendi")
    }

    pub fn user_rules_broken(self) -> &'static str {
        self.pick("Personal rules could not be read", "Kişisel kurallar okunamadı")
    }

    pub fn update_installing(self) -> &'static str {
        self.pick("Installing an update…", "Güncelleme yükleniyor…")
    }

    pub fn hotkey_failed(self) -> &'static str {
        self.pick("Shortcut unavailable — use the tray icon", "Kısayol kurulamadı — tray simgesini kullan")
    }

    pub fn error(self, kind: ErrorKind) -> &'static str {
        use ErrorKind::*;
        match kind {
            MissingKey => self.pick("No API key for this profile", "Bu profil için API anahtarı yok"),
            InvalidKey => self.pick("API key is invalid", "API anahtarı geçersiz"),
            TooLarge => self.pick("Recording exceeds the provider limit", "Kayıt sağlayıcı sınırını aşıyor"),
            RateLimited => self.pick("Provider is rate limiting", "Sağlayıcı istek sınırına takıldı"),
            Server => self.pick("Provider server error", "Sağlayıcı sunucu hatası"),
            Network => self.pick("Network error", "Ağ hatası"),
            Timeout => self.pick("Request timed out", "İstek zaman aşımına uğradı"),
            BadResponse => self.pick("Provider sent an unreadable answer", "Sağlayıcı okunamayan yanıt verdi"),
            Rejected => self.pick("Provider rejected the request", "Sağlayıcı isteği reddetti"),
            Audio => self.pick("Audio could not be encoded", "Ses kodlanamadı"),
            Microphone => self.pick("Microphone unavailable", "Mikrofon kullanılamıyor"),
            Credentials => self.pick("Credential Manager error", "Kimlik bilgisi deposu hatası"),
            Paste => self.pick("Could not paste — text is in History", "Yapıştırılamadı — metin Geçmiş'te"),
            Cancelled => self.pick("Retry cancelled — audio kept", "Tekrar deneme iptal edildi — ses saklandı"),
        }
    }

    pub fn tray_open(self) -> &'static str {
        self.pick("Open", "Aç")
    }

    pub fn tray_profile(self) -> &'static str {
        self.pick("Profile", "Profil")
    }

    pub fn tray_style(self) -> &'static str {
        self.pick("Style", "Stil")
    }

    pub fn style_name(self, style: &opit_core::cleanup::CleanupStyle) -> String {
        use opit_core::cleanup::styles::builtin_name;
        if builtin_name(&style.id, "en") == Some(style.name.as_str()) {
            builtin_name(&style.id, if self == Self::Tr { "tr" } else { "en" }).unwrap().into()
        } else {
            style.name.clone()
        }
    }

    pub fn tray_pause_hotkey(self) -> &'static str {
        self.pick("Pause shortcut", "Kısayolu duraklat")
    }

    pub fn tray_quit(self) -> &'static str {
        self.pick("Quit", "Çıkış")
    }

    pub fn tray_tooltip(self, recording: bool) -> &'static str {
        if recording {
            self.pick("Opit Speech to Text — recording", "Opit Speech to Text — kayıtta")
        } else {
            "Opit Speech to Text"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signs of broken encoding rather than legitimate punctuation: a `?` replacing a letter (it is
    /// followed by a letter, a digit or another `?`), U+FFFD, and UTF-8 read as Windows-1252 or
    /// Latin-1 (a lead byte shown as Â/Ã/Ä/Å followed by a continuation byte, or "â€").
    fn corrupted(text: &str) -> bool {
        // Windows-1252 renderings of the continuation bytes 0x80..=0x9F.
        const CP1252: &str = "€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ";
        let continuation = |c: char| ('\u{80}'..='\u{bf}').contains(&c) || CP1252.contains(c);
        let chars: Vec<char> = text.chars().collect();
        text.contains('\u{fffd}')
            || text.contains("â€")
            || chars.windows(2).any(|pair| match pair {
                ['?', next] => next.is_alphanumeric() || *next == '?',
                [lead, next] => "ÂÃÄÅ".contains(*lead) && continuation(*next),
                _ => false,
            })
    }

    /// Every message `Lang` produces, with each variant of its arguments, in one language.
    fn messages(lang: Lang) -> Vec<(&'static str, String)> {
        use opit_core::cleanup::CleanupError;
        use opit_core::provider::retry::RetryWait;
        let mut out: Vec<(&'static str, String)> = vec![
            ("loading_local", lang.loading_local().into()),
            ("application_list_failed", lang.application_list_failed().into()),
            ("ask_cleanup_failed", lang.ask_cleanup_failed().into()),
            ("ask_invalid", lang.ask_invalid().into()),
            ("ask_detection_failed", lang.ask_detection_failed().into()),
            ("tray_ask", lang.tray_ask().into()),
            ("edit_label", lang.edit_label().into()),
            ("translate_label", lang.translate_label().into()),
            ("voice_preparing", lang.voice_preparing().into()),
            ("voice_connection_required", lang.voice_connection_required().into()),
            ("voice_failed", lang.voice_failed().into()),
            ("selection_failed", lang.selection_failed().into()),
            ("selection_too_long", lang.selection_too_long().into()),
            ("new_conversation", lang.new_conversation().into()),
            ("asking", lang.asking("Claude", 12)),
            ("file_media_filter", lang.file_media_filter().into()),
            ("shortcut_collision", lang.shortcut_collision().into()),
            ("pause_escape_reserved", lang.pause_escape_reserved().into()),
            ("queue_full", lang.queue_full().into()),
            ("recording_paused", lang.recording_paused().into()),
            ("with_waiting", lang.with_waiting(lang.listening(), 2)),
            ("listening", lang.listening().into()),
            ("listening_default_mic", lang.listening_default_mic().into()),
            ("cleaning", lang.cleaning().into()),
            ("cleaning_skippable", lang.cleaning_skippable().into()),
            ("answer_truncated", lang.answer_truncated().into()),
            ("transcribing", lang.transcribing().into()),
            ("pasted", lang.pasted(1.25)),
            ("clipboard_only", lang.clipboard_only().into()),
            ("no_speech", lang.no_speech(-52.0)),
            ("too_short", lang.too_short().into()),
            ("empty", lang.empty().into()),
            ("cancelled", lang.cancelled().into()),
            ("retry", lang.retry().into()),
            ("open_settings", lang.open_settings().into()),
            ("config_reset", lang.config_reset().into()),
            ("user_rules_broken", lang.user_rules_broken().into()),
            ("update_installing", lang.update_installing().into()),
            ("hotkey_failed", lang.hotkey_failed().into()),
            ("tray_open", lang.tray_open().into()),
            ("tray_profile", lang.tray_profile().into()),
            ("tray_style", lang.tray_style().into()),
            ("tray_pause_hotkey", lang.tray_pause_hotkey().into()),
            ("tray_quit", lang.tray_quit().into()),
        ];
        for mut style in opit_core::cleanup::styles::builtin_styles() {
            out.push(("style_name", lang.style_name(&style)));
            style.name = "My style".into();
            out.push(("style_name", lang.style_name(&style)));
        }
        for flag in [false, true] {
            out.push(("tray_recording_pause", lang.tray_recording_pause(flag).into()));
            out.push(("paste_failed", lang.paste_failed(flag).into()));
            out.push(("tray_tooltip", lang.tray_tooltip(flag).into()));
        }
        for format in [
            "read",
            "container",
            "audio track",
            "audio codec",
            "packet",
            "sample rate",
            "sample rate change",
            "channels",
            "empty audio",
            "other",
        ] {
            for container in ["media", "WAV"] {
                out.push(("file_decode", lang.file_decode(container, format)));
            }
        }
        for error in [
            CleanupError::MissingKey,
            CleanupError::InvalidKey,
            CleanupError::Network,
            CleanupError::Timeout,
            CleanupError::Rejected,
            CleanupError::BadResponse,
            CleanupError::Truncated,
        ] {
            for clipboard_only in [false, true] {
                out.push(("cleanup_failed", lang.cleanup_failed(error, clipboard_only)));
            }
        }
        for kind in ["rate_limited", "server", "timeout", "network"] {
            out.push(("retry_wait", lang.retry_wait(RetryWait { kind, seconds: 5, attempt: 1, total: 3 })));
        }
        for kind in ErrorKind::ALL {
            out.push(("error", lang.error(kind).into()));
        }
        out
    }

    #[test]
    fn corruption_check_catches_mojibake_but_accepts_legitimate_text() {
        for bad in [
            "Yerel (?evrimd???)",
            "ba?ar?s?z",
            "Ã§alÄ±ÅŸÄ±yor",
            "Ä°ptal",
            "deÄŸil",
            "Ã‡alÄ±ÅŸma",
            "Ãœcret",
            "Ã–nizleme",
            "Åžimdi",
            "YÃ¼kleniyorâ€¦",
            "bozuk \u{fffd} metin",
        ] {
            assert!(corrupted(bad), "{bad}");
        }
        for good in
            ["Delete 3 items? This cannot be undone.", "Silinsin mi?", "Âdem’in kâr payı", "hâlâ…", "\"quoted\""]
        {
            assert!(!corrupted(good), "{good}");
        }
    }

    #[test]
    fn native_strings_preserve_utf8_and_ask_wording() {
        // Covers what users see, including formatted overlay messages, in both languages.
        let covered: std::collections::BTreeSet<&str> = messages(Lang::En).into_iter().map(|(name, _)| name).collect();
        let source = include_str!("i18n.rs").split("#[cfg(test)]").next().unwrap();
        let methods: std::collections::BTreeSet<&str> = source
            .lines()
            .filter_map(|line| line.strip_prefix("    pub fn "))
            .filter_map(|rest| rest.split('(').next())
            .collect();
        assert_eq!(covered, methods, "every Lang message is checked");
        for lang in [Lang::En, Lang::Tr] {
            for (name, text) in messages(lang) {
                assert!(!text.trim().is_empty(), "{name} is empty");
                assert!(!corrupted(&text), "{name} ({lang:?}) looks like corrupted text");
            }
        }
        assert_eq!(Lang::Tr.ask_cleanup_failed(), "İstek düzenlenemedi. Algılanan istek ajana gönderildi.");
        assert_eq!(Lang::Tr.ask_invalid(), "Sorma ayarları veya çalışma dizini geçersiz.");
        assert_eq!(Lang::Tr.ask_detection_failed(), "Ajan algılama başarısız.");
        assert_eq!(Lang::Tr.new_conversation(), "Yeni konuşma");
        assert_eq!(Lang::Tr.asking("agent", 3), "agent yanıtlıyor… 3 sn");
    }

    #[test]
    fn queue_count_precedes_labels_that_the_overlay_may_truncate() {
        assert_eq!(Lang::En.with_waiting("Listening", 0), "Listening");
        assert!(Lang::En.with_waiting(Lang::En.listening_default_mic(), 2).starts_with("2 waiting"));
        assert!(Lang::Tr.with_waiting(Lang::Tr.recording_paused(), 3).starts_with("3 bekliyor"));
    }

    #[test]
    fn config_language_wins_over_the_system() {
        assert_eq!(resolve(Some("en"), Some("tr-TR")), Lang::En);
        assert_eq!(resolve(Some("tr"), Some("en-US")), Lang::Tr);
        assert_eq!(resolve(None, Some("tr-TR")), Lang::Tr);
        assert_eq!(resolve(None, Some("de-DE")), Lang::En);
        assert_eq!(resolve(None, None), Lang::En);
    }

    #[test]
    fn pasted_uses_the_local_decimal_separator() {
        assert_eq!(Lang::En.pasted(0.94), "Pasted (0.9 s)");
        assert_eq!(Lang::Tr.pasted(0.94), "Yapıştırıldı (0,9 sn)");
    }

    #[test]
    fn paste_failed_mentions_history_only_when_it_is_on() {
        assert!(Lang::En.paste_failed(true).contains("History"));
        assert!(!Lang::En.paste_failed(false).contains("History"));
        assert!(Lang::Tr.paste_failed(true).contains("Geçmiş"));
        assert!(!Lang::Tr.paste_failed(false).contains("Geçmiş"));
    }

    #[test]
    fn every_error_kind_has_both_languages() {
        for kind in ErrorKind::ALL {
            assert!(!Lang::En.error(kind).is_empty());
            assert_ne!(Lang::En.error(kind), Lang::Tr.error(kind), "{kind:?}");
        }
    }
}
