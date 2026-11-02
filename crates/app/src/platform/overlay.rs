//! Layout and animation policy shared by the native recording overlays.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::{RecordingTiming, Tone};
use opit_core::config::OverlayPosition;

pub(crate) const SAMPLE_MS: u32 = 100;
pub(crate) const WAVE_BARS: usize = 25; // 2.5 seconds at 10 samples per second.
pub(crate) const ALPHA: u8 = 235;

/// Logical (96-dpi) layout constants.
pub(crate) const WIDTH: i32 = 420;
pub(crate) const HEIGHT: i32 = 88;
pub(crate) const HEIGHT_WITH_BUTTON: i32 = 104;
pub(crate) const MARGIN: i32 = 24;
pub(crate) const PAD: i32 = 16;
pub(crate) const CORNER: i32 = 16;
pub(crate) const DOT: i32 = 10;
pub(crate) const ROW: i32 = 22;

pub(crate) type Rgb = (u8, u8, u8);
pub(crate) const BG: Rgb = (30, 31, 36);
pub(crate) const FG: Rgb = (236, 236, 240);
pub(crate) const TRACK: Rgb = (62, 63, 72);

/// Native backends supply light-mode preference and optional system high-contrast colours.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub high_contrast: bool,
    pub light: bool,
}

impl Palette {
    pub fn select(light: bool, contrast: Option<(Rgb, Rgb)>) -> Self {
        if let Some((bg, fg)) = contrast {
            return Self { bg, fg, high_contrast: true, light: false };
        }
        Self {
            bg: if light { (243, 243, 243) } else { BG },
            fg: if light { (27, 27, 27) } else { FG },
            high_contrast: false,
            light,
        }
    }

    pub fn accent(self, tone: Tone) -> Rgb {
        if self.high_contrast {
            return self.fg;
        }
        if !self.light {
            return tone_color(tone);
        }
        match tone {
            Tone::Neutral => (0, 99, 177),
            Tone::Busy => (102, 61, 170),
            Tone::Success => (15, 123, 15),
            Tone::Warning => (140, 80, 0),
            Tone::Error => (196, 43, 28),
        }
    }
}

pub(crate) fn tone_color(tone: Tone) -> Rgb {
    match tone {
        Tone::Neutral => (138, 180, 248),
        Tone::Busy => (178, 146, 255),
        Tone::Success => (80, 200, 120),
        Tone::Warning => (245, 182, 66),
        Tone::Error => (242, 95, 92),
    }
}

/// The clock advances only during active capture, including between view updates.
pub(crate) fn recording_elapsed(timing: &RecordingTiming, now: Instant) -> Duration {
    timing.elapsed + if timing.paused { Duration::ZERO } else { now.saturating_duration_since(timing.updated_at) }
}

#[derive(Default)]
pub(crate) struct Waveform {
    session: Option<u64>,
    samples: VecDeque<f32>,
}

impl Waveform {
    pub fn set_recording(&mut self, recording: Option<&RecordingTiming>) {
        let session = recording.map(|r| r.session);
        if self.session != session {
            self.samples.clear();
        }
        self.session = session;
    }

    /// Called by the native 100 ms timer; paused recordings retain their history.
    pub fn tick(&mut self, recording: Option<&RecordingTiming>, level: Option<f32>) {
        if recording.is_some_and(|r| !r.paused) {
            if self.samples.len() == WAVE_BARS {
                self.samples.pop_front();
            }
            self.samples.push_back(level.unwrap_or(0.0));
        }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &f32> {
        self.samples.iter()
    }

    pub fn clear(&mut self) {
        self.session = None;
        self.samples.clear();
    }
}

pub(crate) fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn bar_height(level: f32, max: i32) -> i32 {
    if max <= 0 {
        return 0;
    }
    let level = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
    ((max as f32 * level).round() as i32).clamp(1, max)
}

/// Fits at most two lines using actual font metrics, never cutting a word or UTF-16 unit.
pub(crate) fn fit_status(value: &str, width: i32, measure: impl Fn(&str) -> i32) -> String {
    if width <= 0 {
        return String::new();
    }
    let mut lines = vec![String::new()];
    for word in value.split_whitespace() {
        let first_line = lines.len() == 1;
        let last = lines.last_mut().unwrap();
        let candidate = if last.is_empty() { word.to_string() } else { format!("{last} {word}") };
        if measure(&candidate) <= width {
            *last = candidate;
        } else if first_line && !last.is_empty() && measure(word) <= width {
            lines.push(word.to_string());
        } else {
            let last = lines.last_mut().unwrap();
            while !last.is_empty() && measure(&format!("{last}…")) > width {
                *last = last.rsplit_once(' ').map_or("", |(prefix, _)| prefix).to_string();
            }
            if measure("…") <= width {
                last.push('…');
            }
            break;
        }
    }
    lines.join("\n")
}

/// Keep the most recent words visible using actual font metrics, including long tokens.
pub(crate) fn fit_live(value: &str, width: i32, measure: impl Fn(&str) -> i32) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut tail = normalized.as_str();
    let mut shortened = tail.starts_with('…');
    while !tail.is_empty() {
        let candidate = if shortened && !tail.starts_with('…') { format!("…{tail}") } else { tail.into() };
        let fitted = fit_status(&candidate, width, &measure);
        if fitted.replace('\n', " ") == candidate {
            return fitted;
        }
        tail = &tail[tail.chars().next().unwrap().len_utf8()..];
        shortened = true;
    }
    String::new()
}

/// Integer rectangle in physical pixels (left/top inclusive, right/bottom exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Scales a 96-dpi length to `dpi`, rounding to nearest.
pub fn scale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

/// Overlay height in physical pixels for a view.
pub fn overlay_height(has_button: bool, dpi: u32) -> i32 {
    scale(if has_button { HEIGHT_WITH_BUTTON } else { HEIGHT }, dpi)
}

/// Where the overlay goes inside the monitor work area `work` (physical px). The overlay is
/// `WIDTH` logical px wide, `height_px` tall, `MARGIN` logical px from the chosen edge, and
/// is shrunk/clamped so it never leaves the work area.
pub fn overlay_rect(work: RectI, position: OverlayPosition, dpi: u32, height_px: i32) -> RectI {
    let w = scale(WIDTH, dpi).min(work.width()).max(0);
    let h = height_px.min(work.height()).max(0);
    let margin = scale(MARGIN, dpi);
    let center_x = work.left + (work.width() - w) / 2;
    let center_y = work.top + (work.height() - h) / 2;
    let (x, y) = match position {
        OverlayPosition::RightCenter => (work.right - margin - w, center_y),
        OverlayPosition::LeftCenter => (work.left + margin, center_y),
        OverlayPosition::TopCenter => (center_x, work.top + margin),
        OverlayPosition::BottomCenter => (center_x, work.bottom - margin - h),
    };
    let x = x.clamp(work.left, work.right - w);
    let y = y.clamp(work.top, work.bottom - h);
    RectI { left: x, top: y, right: x + w, bottom: y + h }
}

#[cfg(test)]
mod tests {
    use super::*;
    const WORK: RectI = RectI { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn waveform_scrolls_pauses_and_resets_only_at_session_boundaries() {
        let mut timing =
            RecordingTiming { session: 1, elapsed: Duration::ZERO, updated_at: Instant::now(), paused: false };
        let mut wave = Waveform::default();
        wave.set_recording(Some(&timing));
        for i in 0..30 {
            wave.tick(Some(&timing), Some(i as f32 / 30.0));
        }
        assert_eq!(wave.iter().copied().collect::<Vec<_>>(), (5..30).map(|i| i as f32 / 30.0).collect::<Vec<_>>());
        timing.paused = true;
        wave.set_recording(Some(&timing));
        wave.tick(Some(&timing), Some(1.0));
        assert_eq!(wave.iter().len(), WAVE_BARS);
        assert_eq!(wave.iter().next(), Some(&(5.0 / 30.0)));
        timing.paused = false;
        wave.tick(Some(&timing), None);
        assert_eq!(wave.iter().last(), Some(&0.0));
        timing.session = 2;
        wave.set_recording(Some(&timing));
        assert_eq!(wave.iter().len(), 0);
        wave.tick(Some(&timing), Some(0.5));
        wave.set_recording(None);
        assert_eq!(wave.iter().len(), 0);
        wave.tick(None, Some(1.0));
        assert_eq!(wave.iter().len(), 0);
        wave.set_recording(Some(&timing));
        wave.tick(Some(&timing), Some(0.5));
        wave.clear();
        assert_eq!(wave.iter().len(), 0);
    }

    #[test]
    fn palettes_preserve_windows_colours_and_high_contrast_overrides_every_tone() {
        let dark = Palette::select(false, None);
        let light = Palette::select(true, None);
        let contrast = Palette::select(true, Some(((1, 2, 3), (253, 254, 255))));
        assert_eq!((dark.bg, dark.fg), (BG, FG));
        assert_eq!((light.bg, light.fg), ((243, 243, 243), (27, 27, 27)));
        assert!(contrast.high_contrast);
        assert_eq!(contrast.bg, (1, 2, 3));
        for (tone, expected_dark, expected_light) in [
            (Tone::Neutral, (138, 180, 248), (0, 99, 177)),
            (Tone::Busy, (178, 146, 255), (102, 61, 170)),
            (Tone::Success, (80, 200, 120), (15, 123, 15)),
            (Tone::Warning, (245, 182, 66), (140, 80, 0)),
            (Tone::Error, (242, 95, 92), (196, 43, 28)),
        ] {
            assert_eq!(dark.accent(tone), expected_dark);
            assert_eq!(light.accent(tone), expected_light);
            assert_eq!(contrast.accent(tone), contrast.fg);
        }
    }

    #[test]
    fn recording_clock_tolerates_updates_ahead_of_the_paint_snapshot() {
        let timing =
            RecordingTiming { session: 1, elapsed: Duration::from_secs(9), updated_at: Instant::now(), paused: false };
        assert_eq!(timing.elapsed_at(timing.updated_at - Duration::from_millis(1)), timing.elapsed);
    }

    #[test]
    fn elapsed_clock_excludes_pauses_and_formats_minutes() {
        use crate::platform::RecordingTiming;
        let now = Instant::now();
        let mut timing =
            RecordingTiming { session: 1, elapsed: Duration::from_millis(59_750), updated_at: now, paused: false };
        assert_eq!(format_elapsed(timing.elapsed_at(now + Duration::from_millis(250))), "1:00");
        timing.paused = true;
        assert_eq!(format_elapsed(timing.elapsed_at(now + Duration::from_secs(50))), "0:59");
        timing.elapsed = Duration::from_secs(60);
        timing.updated_at = now + Duration::from_secs(50);
        timing.paused = false;
        assert_eq!(format_elapsed(timing.elapsed_at(now + Duration::from_secs(52))), "1:02");
        assert_eq!(format_elapsed(Duration::ZERO), "0:00");
        assert_eq!(format_elapsed(Duration::from_secs(600)), "10:00");
    }

    #[test]
    fn bars_are_bounded_and_readable_at_all_dpi_scales() {
        for dpi in [96, 120, 144, 192, 288] {
            let max = scale(18, dpi);
            assert_eq!(bar_height(f32::NAN, max), 1);
            assert_eq!(bar_height(f32::INFINITY, max), 1);
            assert_eq!(bar_height(-1.0, max), 1);
            assert_eq!(bar_height(2.0, max), max);
            assert_eq!(bar_height(0.5, max), (max as f32 * 0.5).round() as i32);
        }
        assert_eq!(bar_height(1.0, 0), 0);
    }

    #[test]
    fn stage_text_wraps_and_ellipsizes_at_whole_words() {
        let measure = |s: &str| s.chars().count() as i32;
        assert_eq!(fit_status("Çevriliyor…", 20, measure), "Çevriliyor…");
        assert_eq!(
            fit_status("Yeniden deneniyor 120 saniye sonra", 20, measure),
            "Yeniden deneniyor\n120 saniye sonra"
        );
        assert_eq!(
            fit_status("Temizleniyor ve sırada bekleyen başka kayıtlar var", 17, measure),
            "Temizleniyor ve\nsırada bekleyen…"
        );
        assert_eq!(fit_status("Çokuzunbirkayıtadı", 5, measure), "…");
        assert_eq!(fit_status("bir", 0, measure), "");
    }

    #[test]
    fn live_preview_keeps_the_latest_words_and_long_unicode_tokens() {
        let measure = |s: &str| s.chars().count() as i32;
        let value = fit_live("earlier words followed by the latest words", 16, measure);
        assert!(value.starts_with('…'));
        assert!(value.ends_with("latest words"));
        assert!(value.lines().count() <= 2);
        let token = fit_live(&"ğ".repeat(60), 10, measure);
        assert!(token.starts_with('…'));
        assert!(token.ends_with('ğ'));
        assert!(token.chars().count() <= 10);
        assert_eq!(fit_live("ğüş", 20, measure), "ğüş");
    }

    #[test]
    fn scale_rounds() {
        assert_eq!(scale(260, 96), 260);
        assert_eq!(scale(260, 144), 390);
        assert_eq!(scale(24, 120), 30);
        assert_eq!(scale(10, 120), 13); // 12.5 rounds up
        assert_eq!(overlay_height(false, 96), 88);
        assert_eq!(overlay_height(true, 192), 208);
    }

    #[test]
    fn right_center_at_96_dpi() {
        let r = overlay_rect(WORK, OverlayPosition::RightCenter, 96, 56);
        assert_eq!(
            r,
            RectI { left: 1920 - 24 - 420, top: (1040 - 56) / 2, right: 1920 - 24, bottom: (1040 - 56) / 2 + 56 }
        );
    }

    #[test]
    fn all_positions_at_150_percent() {
        let h = overlay_height(false, 144); // 132
        let (w, m) = (630, 36);
        let r = overlay_rect(WORK, OverlayPosition::LeftCenter, 144, h);
        assert_eq!((r.left, r.top, r.width(), r.height()), (m, (1040 - h) / 2, w, h));
        let r = overlay_rect(WORK, OverlayPosition::TopCenter, 144, h);
        assert_eq!((r.left, r.top), ((1920 - w) / 2, m));
        let r = overlay_rect(WORK, OverlayPosition::BottomCenter, 144, h);
        assert_eq!((r.left, r.bottom), ((1920 - w) / 2, 1040 - m));
        let r = overlay_rect(WORK, OverlayPosition::RightCenter, 144, h);
        assert_eq!(r.right, 1920 - m);
    }

    #[test]
    fn work_area_offset_and_secondary_monitor() {
        // Secondary monitor left of the primary with a taskbar on top.
        let work = RectI { left: -2560, top: 48, right: 0, bottom: 1440 };
        let r = overlay_rect(work, OverlayPosition::TopCenter, 96, 56);
        assert_eq!((r.left, r.top), (-2560 + (2560 - 420) / 2, 48 + 24));
        let r = overlay_rect(work, OverlayPosition::RightCenter, 96, 56);
        assert_eq!(r.right, -24);
        assert_eq!(r.top, 48 + (1392 - 56) / 2);
    }

    #[test]
    fn tiny_work_area_is_clamped() {
        let work = RectI { left: 100, top: 100, right: 300, bottom: 140 };
        for pos in [
            OverlayPosition::RightCenter,
            OverlayPosition::LeftCenter,
            OverlayPosition::TopCenter,
            OverlayPosition::BottomCenter,
        ] {
            let r = overlay_rect(work, pos, 96, 56);
            assert!(
                r.left >= work.left && r.right <= work.right && r.top >= work.top && r.bottom <= work.bottom,
                "{pos:?} {r:?}"
            );
            assert_eq!((r.width(), r.height()), (200, 40));
        }
    }
}
