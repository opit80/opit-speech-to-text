# Opit Speech to Text v1 — Tasarım Brief'i

> Tarih: 2026-09-30 · Durum: **onaylandı** · Ad: **Opit Speech to Text** — kısa ad / repo / binary / veri klasörü: `opit-speech-to-text`; uygulama kimliği: `io.github.opit80.opit-speech-to-text`

## 1. Amaç

Windows için açık kaynak, kurulumlu, cilalı bir **sesli dikte** uygulaması: kısayola bas → konuş → metin imlecin olduğu yere yapıştırılır. Transkripsiyon **kullanıcının kendi bulut API anahtarıyla (BYOK)** yapılır; hiçbir kişisel sunucuya bağımlılık yoktur. Hedef: **en doğru cümle**, düşük gecikme, günlük kullanımda hissedilmeyen bir tray uygulaması.

**Başarı ölçütleri**
- Kısayoldan yapıştırmaya uçtan uca süre, 5 sn'lik konuşmada Groq ile medyan **< 1,5 sn**.
- Tray'de beklerken **< 40 MB RAM**, kurulum dosyası **< 15 MB**.
- Türkçe teknik dikte test setinde, kural katmanı açıkken WER ve terim isabeti kural katmanı kapalıya göre **daha iyi** (ölçülür, §9).
- Yeni kullanıcı kurulumdan ilk başarılı dikteye **< 2 dk** (sihirbaz).

**Kapsam dışı (v1)**: LLM ile metin düzeltme adımı, gerçek zamanlı/streaming transkripsiyon, macOS/Linux, OpenAI-uyumlu olmayan sağlayıcılar (Deepgram vb.), uygulamaya göre profil değiştirme, telemetri. Mimari bunların sonradan eklenmesini engellemez.

## 2. Kilitli kararlar

| Konu | Karar | Gerekçe |
|---|---|---|
| Framework | **Tauri 2** | Küçük kurulum, düşük RAM, WebView2 Win10/11'de hazır |
| Mantık | **Rust çekirdek**; UI yalnız görünüm | Kritik zincir (kısayol→yapıştır) tek, hep ayakta süreçte; WebView'e bağlı değil |
| UI | **Svelte 5 + Vite + TypeScript** | Hafif, Tauri ile yaygın |
| Pencere | Kapat → tray'e düş, **WebView yok edilir** | Tray'de düşük RAM |
| Sağlayıcı | **BYOK**, tek **OpenAI-uyumlu adaptör** + profiller | Groq/OpenAI/özel sunucu aynı protokol |
| Doğruluk | **prompt/keywords + deterministik kurallar** (LLM yok) | Hızlı, öngörülebilir, maliyetsiz |
| Veri | Ayarlar JSON, kurallar YAML, geçmiş **SQLite (rusqlite, gömülü, FTS5)** | Kurulum gerektirmez |
| Sırlar | API anahtarları **Windows Credential Manager** (`keyring`) | Düz dosyada anahtar yok |
| Geçmiş | Metin geçmişi **açık**, ses kaydı **varsayılan kapalı** | Gizlilik + kural geliştirme imkânı |
| Dil | UI **en + tr** (sistem diline göre); repo/kod/README **İngilizce**; transkripsiyon dili ayrı ayar (varsayılan `tr`) | Açık kaynak katkısı |
| Kurulum | NSIS, **currentUser** (admin gerekmez) | UAC yok |
| Güncelleme | `tauri-plugin-updater` + **GitHub Releases** (minisign imzalı manifest) | Sunucusuz |
| Lisans | **MIT** | En yaygın, sade |
| Kod imzalama | v1 imzasız (SmartScreen uyarısı README'de açıklanır); **SignPath Foundation** (ücretsiz OSS imzalama) başvurusu yapılır | Maliyetsiz |

## 3. Mimari

```
repo/
├─ crates/
│  ├─ core/        saf mantık — Tauri/OS bağımsız, Linux'ta da test edilir
│  │  ├─ pipeline.rs      Recording → Transcript zinciri
│  │  ├─ audio/           resample (16 kHz mono), seviye, sessizlik kapısı, WAV/FLAC kodlama
│  │  ├─ provider/        Transcriber trait + OpenAiCompatible, profiller, preset'ler
│  │  ├─ rules/           kural motoru, YAML paket yükleyici, prompt kurucu
│  │  ├─ history/         rusqlite + FTS5, ses dosyası saklama/temizlik
│  │  └─ config.rs        ayarlar şeması + göç (schema_version)
│  ├─ app/         Tauri uygulaması
│  │  ├─ main.rs          kurulum, tray, pencere yaşam döngüsü
│  │  ├─ controller.rs    dikte durum makinesi
│  │  ├─ commands.rs      UI'nin çağırdığı invoke API'si + event'ler
│  │  └─ platform/        trait'ler + windows/
│  │       Microphone(cpal) · Hotkey(WH_KEYBOARD_LL) · Paster(SendInput)
│  │       Overlay(Win32 layered) · SecretStore(keyring) · Sounds · Autostart
│  └─ eval/        geliştirici CLI'ı: klasördeki ses+referans metinle WER/terim isabeti ölçer
├─ ui/             Svelte 5
├─ rules/          varsayılan kural paketleri (repo ile gelir)
└─ docs/
```

**Sınırlar**
- `core::pipeline::run(recording, &Profile, &RuleSet) -> Result<Transcript, PipelineError>` — tek giriş noktası; UI ve platformdan habersiz.
- `provider::Transcriber` trait'i — ileride LLM düzeltme adımı veya başka sağlayıcı buraya takılır.
- `platform/` trait'leri — macOS/Linux = yeni klasör + trait uygulaması.
- `controller` tek sahip: `Idle → Recording → Transcribing → Pasting → Idle` (+ `Cancelled`, `Error`). Kısayol, tray ve UI tetiklerinin hepsi buradan geçer; aynı anda tek dikte.

## 4. Dikte akışı

1. **Tetik**: kısayol (varsayılan **sağ Ctrl + sağ Shift**, mod: **toggle**; alternatif **bas-konuş**), tray sol tık veya UI düğmesi. Toggle modunda kayıt sırasında aynı kombin ya da tek başına **Ctrl** durdurur (Ctrl ancak kombin bırakıldıktan sonra yeniden basılırsa sayılır); **Esc** iptal eder.
2. **Kayıt**: `cpal` ile seçili mikrofon, cihazın yerel örnekleme hızında; bellekte tutulur. Overlay "Dinleniyor" + seviye çubuğu, başlama sesi. Üst sınır **180 sn** (ayarlanabilir, en fazla 600); dolunca otomatik gönderilir.
3. **Durdurma → ön kontrol**: 16 kHz mono'ya resample. **Sessizlik kapısı**: 30 ms'lik çerçevelerde RMS eşiğini aşan çerçeve toplamı < 0,3 sn ise gönderilmez ("Konuşma algılanmadı"). **< 0,4 sn** kayıtlar da gönderilmez. (Sessizliğe halüsinasyonun ana önlemi budur.)
4. **Kodlama**: profilin `audio_format`'ına göre **FLAC** (Groq) veya **WAV** (OpenAI, özel sunucu).
5. **İstek**: `POST {base_url}/audio/transcriptions`, multipart: `file`, `model`, `language`, `temperature=0`, `response_format`, profil izin veriyorsa `prompt` / `keywords` (§5). Zaman aşımı: bağlantı 5 sn, toplam 30 sn + kayıt süresi/4.
6. **Yanıt işleme**: `verbose_json` destekleyen profilde `no_speech_prob > 0,6` **ve** `avg_logprob < −1,0` olan segmentler atılır. Sonra halüsinasyon filtresi ve kural motoru (§5).
7. **Yapıştırma**: pano içeriği yedeklenir → metin panoya → `SendInput` ile Ctrl+V → 400 ms sonra eski pano geri yüklenir (ayarla kapatılabilir). Son boşluk ayarı: metnin sonuna tek boşluk ekle (varsayılan açık).
8. **Kayıt**: geçmişe `raw_text` (API çıktısı) ve `text` (kurallardan sonra), profil, süreler yazılır; ses kaydı açıksa ses dosyası da. Overlay "Yapıştırıldı (0,9 sn)", bitiş sesi.

## 5. Doğruluk katmanı

Bulut API'lerinde `beam_size`/`hotwords` yok; doğruluk üç kademede sağlanır.

**5.1 Yönlendirme (istek öncesi)** — `PromptBuilder`
- Whisper prompt'u talimat değil **örnek metin** gibi çalışır. Kurulan prompt: kullanıcının kısa bağlam cümlesi (ör. "FiveM sunucusu ve yazılım geliştirme üzerine Türkçe konuşma.") + aktif paketlerin `terms` listesi, doğal bir cümle olarak ("Geçen terimler: Claude Code, FiveM, GitHub, …"), noktalamalı.
- **224 token bütçesi**: terimler öncelik sırasıyla eklenir (kişisel paket önce), bütçe dolunca kesilir; UI'de "prompt'a sığmayan terim" uyarısı.
- `keywords` destekleyen modelde (OpenAI `gpt-transcribe`) terimler ayrıca `keywords` alanına da gider.

**5.2 Filtre (istek sonrası)** — `HallucinationFilter`
- Pakette dil başına bilinen kalıplar ("altyazı m.k", "thanks for watching", "abone olmayı unutmayın" …). **Yalnızca çıktının tamamı** normalize edilmiş haliyle kalıba eşitse atılır; cümle içinde asla silinmez.

**5.3 Düzeltme (istek sonrası)** — `RuleEngine`, sırayla:
1. `corrections`: `kanonik: [yanlış varyantlar]` — tam kelime, büyük/küçük harf duyarsız, en uzun varyant önce; çıktı kanonik yazım.
2. `replacements`: `from → to`, sıralı; varsayılan tam kelime + harf duyarsız; opsiyonel `case_sensitive`, `regex: true`.
3. `terms` casing: terim listesindeki kelimeler kanonik yazıma zorlanır (`github → GitHub`).

Motorun iki kuralı (eski sistemdeki hataları önler):
- **Türkçe harf katlama**: eşleşmede `I/ı`, `İ/i` doğru katlanır (Rust `to_lowercase` Türkçe değil; özel fold fonksiyonu).
- **Büyük harf koruma**: eşleşme cümle başındaysa veya kaynak büyük harfle başlıyorsa ve hedef küçük harfle başlıyorsa hedefin ilk harfi büyütülür (`Hala → Hâlâ`, `hala → hâlâ`).
- Kesme işaretli ekler doğal çalışır: `cloud code'u → Claude Code'u` (kelime sınırı kesmeden önce biter).

**5.4 Kural paketleri** — YAML, `schema: 1`
```yaml
id: tr-tech
name: Turkish – software terms
language: tr
terms: [Claude Code, GitHub, TypeScript, API, endpoint, worktree, …]
corrections:
  Claude Code: [cloud code, clod code, claude kod, klad kod]
replacements:
  - { from: hala, to: hâlâ }
hallucinations: [altyazı m.k, abone olmayı unutmayın]
```
- **Repo ile gelen paketler**: `tr-core` (Türkçe imla: hâlâ, pekâlâ, sağ ol … + halüsinasyon kalıpları), `tr-tech` (yazılım terimleri), `fivem` (FiveM/QBCore/ox_* terimleri). Sihirbazda seçilir.
- **Kişisel paket**: `%APPDATA%\opit-speech-to-text\rules\user.yaml`; repoya girmez, UI'den düzenlenir, en yüksek önceliktedir.
- Mevcut eski kurallar bu paketlere **ayıklanarak** taşınır: genel terim/imla kuralları → repo paketleri; kişiye özel olanlar (isimler, argo, kişisel ifadeler) → yalnız kullanıcının kendi `user.yaml`'ı; çok geniş eşleşenler (`ettik→Et`, `MC→MCP` gibi) taşınmaz. Taşıma sonrası her kural §9'daki eval'le doğrulanır; yeni modelde etkisiz kalan kural silinir.

## 6. Sağlayıcı profilleri

```
Profile { id, name, base_url, model, api_key_ref?, language,
          audio_format: flac|wav, response_format: verbose_json|json,
          send_prompt: bool, send_keywords: bool, apply_rules: bool,
          fallback_profile_id? }
```
| Preset | base_url | Model (varsayılan) | Format | Prompt | verbose_json |
|---|---|---|---|---|---|
| **Groq** (varsayılan) | `https://api.groq.com/openai/v1` | `whisper-large-v3` | FLAC | ✓ | ✓ |
| OpenAI | `https://api.openai.com/v1` | `gpt-transcribe` | WAV | ✓ (+keywords) | ✗ |
| Özel (OpenAI-uyumlu) | kullanıcı girer | kullanıcı girer | WAV | ayar | ayar |

- Varsayılan model **Groq `whisper-large-v3`** (turbo'dan daha düşük WER; maliyet ~$0,11/saat). v1 yayını öncesi eval ile `whisper-large-v3-turbo` ve OpenAI `gpt-transcribe` ile karşılaştırılır; Türkçede belirgin fark çıkarsa varsayılan ona göre değişir.
- **Yedek profil**: ağ hatası / 5xx / 429 sonrası bir yeniden deneme (500 ms), yine başarısızsa `fallback_profile_id` ile tekrar. Örn. birincil = özel GPU sunucusu, yedek = Groq.
- `http://` adreslerine izin verilir, profil ekranında uyarı gösterilir. "Bağlantıyı test et": `GET {base_url}/models`, olmazsa 1 sn'lik sessiz WAV ile transkripsiyon denemesi.
- Özel sunucu kendi kurallarını uyguluyorsa kullanıcı `apply_rules` ve `send_prompt`'u kapatır.

## 7. Arayüz

- **İlk açılış sihirbazı** (5 adım): arayüz dili → sağlayıcı + anahtar (+ test) → mikrofon seçimi + seviye testi → kısayol kaydı → kural paketleri + "Windows ile başlat" (işaretli). Son adımda deneme diktesi.
- **Ana pencere** (her açılışta gösterilir; ayar ile "tray'de başla" seçilebilir): sol menü — Ana sayfa (son dikteler, durum, hızlı profil değiştirici), Geçmiş, Kurallar, Profiller, Ayarlar.
- **Geçmiş**: FTS5 arama, kopyala, sil, tümünü temizle; ses açıksa oynat. Bir diktede kelime seçip **"Düzeltme kuralı ekle"** → `user.yaml`'a eklenir, canlı önizleme (`rules_preview` invoke).
- **Kurallar**: paketleri aç/kapa, `user.yaml` düzenleyici (tablo görünümü + ham YAML), "metin üzerinde dene" kutusu, prompt bütçe göstergesi.
- **Tray**: sol tık = kayıt başlat/durdur; sağ menü: Aç, Profil ▸, Kısayolu duraklat, Çıkış.
- **Overlay**: Win32 katmanlı, tıklama geçirgen, her zaman üstte; konum ayarlanabilir (varsayılan sağ-orta). Durumlar: Dinleniyor (seviye), Çevriliyor, Yapıştırıldı, Hata. Tam ekran exclusive oyunlarda görünmeyebilir → **sesli geri bildirim** varsayılan açık.

## 8. Hata yönetimi

| Durum | Davranış |
|---|---|
| Mikrofon açılamadı / cihaz çıktı | Overlay hata + ses; varsayılan cihaza düşmeyi dener; ayarlarda uyarı |
| Konuşma yok / çok kısa | Gönderilmez; overlay bilgi |
| 401/403 | "API anahtarı geçersiz" + ayarlara kısayol; yeniden deneme yok |
| 413 | "Kayıt sağlayıcı sınırını aşıyor" hatası (600 sn WAV ≈ 19 MB < 25 MB olduğundan normalde oluşmaz) |
| 429 / 5xx / ağ | 1 yeniden deneme → yedek profil → hata; ses bellekte tutulur, overlay'de **"Tekrar dene"** (tray menüsünden de) |
| Boş metin | Yapıştırma yok; geçmişe "boş" olarak düşer |
| Yapıştırma başarısız (ör. yönetici yetkili pencere, UIPI) | Metin panoda kalır, overlay "Panoda — Ctrl+V ile yapıştır" |
| Kısayol çakışması / kanca kurulamadı | Sihirbaz/ayarlar uyarı; tray ile kullanım sürer |

Loglar: `tracing` → `%APPDATA%\opit-speech-to-text\logs\` (dönen dosya, 7 gün). **Transkript metni loglara yazılmaz**; API anahtarı asla. Telemetri yok.

## 9. Test ve doğrulama

- **core birim testleri**: kural motoru (Türkçe fold, büyük harf koruma, kesme ekleri, sıralama, regex), prompt kurucu (224 token bütçesi), sessizlik kapısı, halüsinasyon filtresi, config göçü.
- **Sağlayıcı entegrasyon testleri**: `wiremock` ile sahte OpenAI-uyumlu sunucu — başarılı, 401, 429→yedek, zaman aşımı, verbose_json segment filtresi.
- **Controller testleri**: platform trait'lerinin sahte uygulamalarıyla durum makinesi (iptal, çift tetik, kayıt sırasında hata).
- **eval CLI**: `eval --dir <wav+txt klasörü> --profile groq --rules on|off` → WER, terim isabeti, gecikme; sonuç Markdown tablo. Test verisi repoya girmez (kişisel ses); README'de kendi setini kurma rehberi.
- **Manuel yayın kontrol listesi**: `docs/RELEASE-CHECKLIST.md` (temiz Windows'ta kurulum, sihirbaz, kısayol, yapıştırma, tray, güncelleme).
- **CI** (GitHub Actions, `windows-latest`): `cargo fmt --check`, `clippy -D warnings`, `cargo test`, UI `svelte-check` + build. Tag'de release iş akışı: NSIS kurulum + updater manifesti → GitHub Release.

## 10. Veri konumları

`%APPDATA%\opit-speech-to-text\`: `config.json` · `rules\user.yaml` · `history.db` · `audio\` (yalnız ses kaydı açıksa; varsayılan saklama 30 gün) · `logs\`. Kaldırma sırasında "verilerimi de sil" seçeneği.

## 11. Açık konular

Yok — varsayılan model seçimi §6'daki eval'e bağlıdır; bu bir uygulama adımıdır, açık karar değildir.
