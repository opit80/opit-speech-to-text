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

    pub fn transcribing(self) -> &'static str {
        self.pick("Transcribing…", "Çevriliyor…")
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

    pub fn no_speech(self) -> &'static str {
        self.pick("No speech detected", "Konuşma algılanmadı")
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
        }
    }

    pub fn tray_open(self) -> &'static str {
        self.pick("Open", "Aç")
    }

    pub fn tray_profile(self) -> &'static str {
        self.pick("Profile", "Profil")
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
