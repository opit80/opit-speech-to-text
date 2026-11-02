//! Linux status panel. GTK objects and all drawing stay on Tauri's GTK main thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{cairo, gdk, glib, pango};
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use opit_core::config::OverlayPosition;

use crate::platform::overlay::*;
use crate::platform::{Overlay, OverlaySink, OverlayView};

#[derive(Default)]
struct Pending {
    view: Option<Option<OverlayView>>,
    position: Option<OverlayPosition>,
}

/// Only queued values cross threads. A main-thread timer coalesces frequent microphone updates.
pub struct LinuxOverlay {
    pending: Arc<Mutex<Pending>>,
    available: Arc<AtomicBool>,
    notification: Mutex<Option<(String, Instant)>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Theme {
    light: bool,
    high_contrast: bool,
}

fn detect_theme(prefer_dark: bool, name: Option<&str>) -> Theme {
    let name = name.unwrap_or_default().trim().to_ascii_lowercase();
    let compact: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let high_contrast = compact.contains("highcontrast");
    let dark = prefer_dark || name.ends_with("-dark") || (high_contrast && compact.contains("inverse"));
    Theme { light: !name.is_empty() && !dark, high_contrast }
}

fn palette(area: &gtk::DrawingArea) -> Palette {
    let settings = area.settings();
    let name = settings.as_ref().and_then(|s| s.gtk_theme_name());
    let theme =
        detect_theme(settings.as_ref().is_some_and(|s| s.is_gtk_application_prefer_dark_theme()), name.as_deref());
    let contrast = theme.high_contrast.then(|| {
        let context = area.style_context();
        let rgb = |color: gdk::RGBA| {
            let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            (channel(color.red()), channel(color.green()), channel(color.blue()))
        };
        let fallback = if theme.light { ((255, 255, 255), (0, 0, 0)) } else { ((0, 0, 0), (255, 255, 255)) };
        (
            context.lookup_color("theme_bg_color").map_or(fallback.0, rgb),
            context.lookup_color("theme_fg_color").map_or(fallback.1, rgb),
        )
    });
    Palette::select(theme.light, contrast)
}

fn layer_edges(position: OverlayPosition) -> [bool; 4] {
    // Top, right, bottom, left. An unanchored axis is centered by layer-shell.
    match position {
        OverlayPosition::TopCenter => [true, false, false, false],
        OverlayPosition::RightCenter => [false, true, false, false],
        OverlayPosition::BottomCenter => [false, false, true, false],
        OverlayPosition::LeftCenter => [false, false, false, true],
    }
}

fn place(window: &gtk::Window, layer: bool, position: OverlayPosition) {
    if layer {
        for (edge, anchored) in
            [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left].into_iter().zip(layer_edges(position))
        {
            window.set_anchor(edge, anchored);
            window.set_layer_shell_margin(edge, MARGIN);
        }
    } else if let Some(display) = gdk::Display::default() {
        let monitor = display
            .default_seat()
            .and_then(|seat| seat.pointer())
            .and_then(|pointer| {
                let (_, x, y) = pointer.position();
                display.monitor_at_point(x, y)
            })
            .or_else(|| display.primary_monitor())
            .or_else(|| display.monitor(0));
        if let Some(monitor) = monitor {
            let work = monitor.workarea();
            let (width, height) = window.size();
            let rect = overlay_rect(
                RectI {
                    left: work.x(),
                    top: work.y(),
                    right: work.x() + work.width(),
                    bottom: work.y() + work.height(),
                },
                position,
                96, // GTK coordinates are already logical pixels.
                height,
            );
            // Preserve actual allocated width if the compositor constrained the panel.
            let x = match position {
                OverlayPosition::LeftCenter => rect.left,
                OverlayPosition::RightCenter => rect.right - width,
                _ => work.x() + (work.width() - width) / 2,
            };
            window.move_(x, rect.top);
        }
    }
}

fn button_rect(width: i32, height: i32, label_width: i32) -> RectI {
    let right = (width - PAD).max(0);
    let left = (right - label_width.max(0)).max(PAD + DOT + 10).min(right);
    let bottom = (height - 12).max(0);
    RectI { left, top: (bottom - ROW).max(0), right, bottom }
}

#[derive(Debug, PartialEq, Eq)]
struct MeterLayout {
    top: i32,
    right: i32,
    timer_right: i32,
}

fn meter_layout(width: i32, height: i32, button: Option<RectI>) -> MeterLayout {
    let text_left = PAD + DOT + 10;
    // Linux keeps its existing Cancel link during capture. Reserve its measured width
    // beside the timer rather than hiding the recording footer when a button is present.
    let timer_right = button.map_or(width - PAD, |r| r.left - 12).max(text_left);
    MeterLayout {
        top: button.map_or(height - 28, |r| r.top + (ROW - 18) / 2),
        right: (timer_right - 70).max(text_left),
        timer_right,
    }
}

fn contains(rect: RectI, x: f64, y: f64) -> bool {
    x >= f64::from(rect.left) && x < f64::from(rect.right) && y >= f64::from(rect.top) && y < f64::from(rect.bottom)
}

/// Only the drawn link accepts input. Every other pixel passes through to the target app.
fn set_clickable(window: &gtk::Window, rect: Option<RectI>) {
    let Some(surface) = window.window() else { return };
    let region = rect.map_or_else(cairo::Region::create, |r| {
        cairo::Region::create_rectangle(&cairo::RectangleInt::new(r.left, r.top, r.width(), r.height()))
    });
    surface.input_shape_combine_region(&region, 0, 0);
    surface.set_pass_through(rect.is_none());
    surface.set_cursor(rect.and_then(|_| gdk::Cursor::from_name(&surface.display(), "pointer")).as_ref());
}

fn layout(area: &gtk::DrawingArea, link: bool, color: Rgb) -> pango::Layout {
    let layout = area.create_pango_layout(None);
    let mut font =
        area.pango_context().font_description().unwrap_or_else(|| pango::FontDescription::from_string("Sans"));
    font.set_absolute_size(f64::from(if link { 14 } else { 15 }) * f64::from(pango::SCALE));
    font.set_weight(if link { pango::Weight::Semibold } else { pango::Weight::Normal });
    layout.set_font_description(Some(&font));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrColor::new_foreground(
        u16::from(color.0) * 257,
        u16::from(color.1) * 257,
        u16::from(color.2) * 257,
    ));
    if link {
        attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
    }
    layout.set_attributes(Some(&attrs));
    layout
}

fn link_rect(area: &gtk::DrawingArea, view: &OverlayView, width: i32, height: i32) -> Option<RectI> {
    let button = view.button.as_ref()?;
    let text = layout(area, true, (0, 0, 0));
    text.set_text(&button.label);
    Some(button_rect(width, height, text.pixel_size().0))
}

fn source(cr: &cairo::Context, color: Rgb) {
    cr.set_source_rgb(f64::from(color.0) / 255.0, f64::from(color.1) / 255.0, f64::from(color.2) / 255.0);
}

fn rounded_panel(cr: &cairo::Context, width: f64, height: f64, radius: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(width - radius, radius, radius, -FRAC_PI_2, 0.0);
    cr.arc(width - radius, height - radius, radius, 0.0, FRAC_PI_2);
    cr.arc(radius, height - radius, radius, FRAC_PI_2, PI);
    cr.arc(radius, radius, radius, PI, PI + FRAC_PI_2);
    cr.close_path();
}

fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, state: &Panel) {
    let Some(view) = &state.view else { return };
    let palette = palette(area);
    let accent = palette.accent(view.tone);
    let (w, h) = (area.allocated_width(), area.allocated_height());
    let rounded = area.screen().is_some_and(|s| s.is_composited() && s.rgba_visual().is_some());
    let _ = cr.save();
    cr.set_operator(cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);
    cr.push_group();
    if rounded {
        rounded_panel(cr, f64::from(w), f64::from(h), f64::from(CORNER));
    } else {
        cr.rectangle(0.0, 0.0, f64::from(w), f64::from(h));
    }
    cr.clip();
    source(cr, palette.bg);
    let _ = cr.paint();
    let text_left = PAD + DOT + 10;
    source(cr, accent);
    cr.arc(
        f64::from(PAD) + f64::from(DOT) / 2.0,
        f64::from(10 + ROW / 2),
        f64::from(DOT) / 2.0,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.fill();

    let text = layout(area, false, palette.fg);
    let measure = |value: &str| {
        text.set_text(value);
        text.pixel_size().0
    };
    let width = (w - PAD - text_left).max(0);
    let fitted = view
        .live_text
        .as_ref()
        .map_or_else(|| fit_status(&view.text, width, measure), |live| fit_live(live, width, measure));
    // Draw each measured line on the same 22 px grid as the GDI panel.
    let _ = cr.save();
    cr.rectangle(f64::from(text_left), 10.0, f64::from(width), f64::from(ROW * 2));
    cr.clip();
    for (line, value) in fitted.lines().enumerate() {
        text.set_text(value);
        gtk::render_layout(&area.style_context(), cr, f64::from(text_left), 10.0 + line as f64 * f64::from(ROW), &text);
    }
    let _ = cr.restore();

    let link = link_rect(area, view, w, h);
    if let Some(button) = &view.button {
        let text = layout(area, true, accent);
        // Long labels stay inside the same measured rectangle used for input.
        text.set_text(&button.label);
        let rect = button_rect(w, h, text.pixel_size().0);
        text.set_width(rect.width() * pango::SCALE);
        text.set_ellipsize(pango::EllipsizeMode::End);
        text.set_single_paragraph_mode(true);
        let y = rect.top + (rect.height() - text.pixel_size().1) / 2;
        gtk::render_layout(&area.style_context(), cr, f64::from(rect.left), f64::from(y), &text);
    }
    if view.level.is_some() {
        let MeterLayout { top, right, timer_right } = meter_layout(w, h, link);
        let step = (right - text_left).max(0) / WAVE_BARS as i32;
        let count = state.waveform.iter().len();
        for (index, level) in state.waveform.iter().enumerate() {
            let height = bar_height(*level, 18);
            let left = text_left + (WAVE_BARS - count + index) as i32 * step;
            let color = if *level > 0.0 || palette.high_contrast {
                accent
            } else if palette.light {
                (95, 95, 95)
            } else {
                TRACK
            };
            if step > 0 {
                source(cr, color);
                cr.rectangle(
                    f64::from(left),
                    f64::from(top + (18 - height) / 2),
                    f64::from((step - 2).max(1)),
                    f64::from(height),
                );
                let _ = cr.fill();
            }
        }
        if let Some(recording) = &view.recording {
            text.set_text(&format_elapsed(recording.elapsed_at(Instant::now())));
            let available = (timer_right - right - 8).max(0);
            if available > 0 {
                text.set_width(available * pango::SCALE);
                text.set_alignment(pango::Alignment::Right);
                text.set_ellipsize(pango::EllipsizeMode::End);
                let th = text.pixel_size().1;
                gtk::render_layout(
                    &area.style_context(),
                    cr,
                    f64::from(right + 8),
                    f64::from(top - 2) + f64::from(ROW + 2 - th) / 2.0,
                    &text,
                );
            }
        }
    }
    let _ = cr.pop_group_to_source();
    let _ = cr.paint_with_alpha(if palette.high_contrast || !rounded { 1.0 } else { f64::from(ALPHA) / 255.0 });
    let _ = cr.restore();
}

#[derive(Default)]
struct Panel {
    view: Option<OverlayView>,
    waveform: Waveform,
    hide_at: Option<Instant>,
    button: Option<RectI>,
    pressed: bool,
    sample_timer: Option<glib::SourceId>,
    clock_timer: Option<glib::SourceId>,
}

impl Panel {
    fn stop_timers(&mut self) {
        if let Some(timer) = self.sample_timer.take() {
            timer.remove();
        }
        if let Some(timer) = self.clock_timer.take() {
            timer.remove();
        }
    }

    fn hide(&mut self, window: &gtk::Window) {
        self.stop_timers();
        self.view = None;
        self.waveform.clear();
        self.hide_at = None;
        self.button = None;
        self.pressed = false;
        window.hide();
        set_clickable(window, None);
    }
}

fn sync_timers(state: &Rc<RefCell<Panel>>, area: &gtk::DrawingArea) {
    let active = state.borrow().view.as_ref().and_then(|v| v.recording.as_ref()).is_some_and(|r| !r.paused);
    if !active {
        state.borrow_mut().stop_timers();
        return;
    }
    if state.borrow().sample_timer.is_some() {
        return;
    }
    let weak = Rc::downgrade(state);
    let canvas = area.downgrade();
    let sample = glib::timeout_add_local(Duration::from_millis(u64::from(SAMPLE_MS)), move || {
        let (Some(state), Some(area)) = (weak.upgrade(), canvas.upgrade()) else { return glib::ControlFlow::Break };
        let mut state = state.borrow_mut();
        let Panel { view, waveform, .. } = &mut *state;
        if let Some(view) = view {
            waveform.tick(view.recording.as_ref(), view.level);
        }
        drop(state);
        area.queue_draw();
        glib::ControlFlow::Continue
    });
    let canvas = area.downgrade();
    let clock = glib::timeout_add_local(Duration::from_secs(1), move || {
        let Some(area) = canvas.upgrade() else { return glib::ControlFlow::Break };
        area.queue_draw();
        glib::ControlFlow::Continue
    });
    let mut state = state.borrow_mut();
    state.sample_timer = Some(sample);
    state.clock_timer = Some(clock);
}

impl LinuxOverlay {
    /// Called on the GTK main thread during platform factory setup. Does not initialize GTK or
    /// open another display. Unsupported compositors get notifications and the existing tray.
    pub fn new(position: OverlayPosition, sink: OverlaySink) -> Self {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let available = Arc::new(AtomicBool::new(false));
        let result = Self { pending: pending.clone(), available: available.clone(), notification: Mutex::new(None) };
        if !gtk::is_initialized_main_thread() {
            return result;
        }
        let Some(display) = gdk::Display::default() else { return result };
        let display_type = display.type_().name();
        let wayland = display_type.contains("Wayland");
        let x11 = display_type.contains("X11");
        let layer = wayland && gtk_layer_shell::is_supported();
        if !layer && !x11 {
            return result;
        }

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Opit Speech to Text");
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_accept_focus(false);
        window.set_focus_on_map(false);
        window.set_skip_taskbar_hint(true);
        window.set_skip_pager_hint(true);
        window.set_keep_above(true);
        window.set_app_paintable(true);
        if let Some(visual) = WidgetExt::screen(&window).and_then(|s| s.rgba_visual()) {
            window.set_visual(Some(&visual));
        }
        let css = gtk::CssProvider::new();
        let _ = css.load_from_data(b"window { background-color: transparent; background-image: none; box-shadow: none; } * { text-shadow: none; }");
        window.style_context().add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        window.set_default_size(WIDTH, HEIGHT);
        if layer {
            window.init_layer_shell();
            window.set_namespace("opit-speech-to-text-overlay");
            window.set_layer(Layer::Overlay);
            window.set_keyboard_mode(KeyboardMode::None);
            window.set_exclusive_zone(0);
        } else {
            window.set_type_hint(gdk::WindowTypeHint::Dock);
        }
        let area = gtk::DrawingArea::new();
        area.set_size_request(WIDTH, HEIGHT);
        area.add_events(gdk::EventMask::BUTTON_PRESS_MASK | gdk::EventMask::BUTTON_RELEASE_MASK);
        window.add(&area);
        let state = Rc::new(RefCell::new(Panel::default()));
        let paint = state.clone();
        area.connect_draw(move |area, cr| {
            draw(area, cr, &paint.borrow());
            glib::Propagation::Stop
        });
        let press = state.clone();
        area.connect_button_press_event(move |_, event| {
            if event.button() == 1 {
                let (x, y) = event.position();
                let mut state = press.borrow_mut();
                state.pressed = state.button.is_some_and(|r| contains(r, x, y));
            }
            glib::Propagation::Stop
        });
        let release = state.clone();
        let weak_window = window.downgrade();
        area.connect_button_release_event(move |_, event| {
            if event.button() != 1 {
                return glib::Propagation::Stop;
            }
            let action = {
                let mut state = release.borrow_mut();
                let pressed = std::mem::take(&mut state.pressed);
                let (x, y) = event.position();
                if pressed && state.button.is_some_and(|r| contains(r, x, y)) {
                    state.view.as_ref().and_then(|v| v.button.as_ref()).map(|b| b.action)
                } else {
                    None
                }
            };
            if let Some(action) = action {
                if let Some(window) = weak_window.upgrade() {
                    release.borrow_mut().hide(&window);
                }
                sink(action);
            }
            glib::Propagation::Stop
        });
        let allocate = state.clone();
        let weak_window = window.downgrade();
        area.connect_size_allocate(move |area, allocation| {
            let Ok(mut state) = allocate.try_borrow_mut() else { return };
            state.button =
                state.view.as_ref().and_then(|view| link_rect(area, view, allocation.width(), allocation.height()));
            let rect = state.button;
            drop(state);
            if let Some(window) = weak_window.upgrade() {
                set_clickable(&window, rect);
            }
        });
        // GTK style updates include live theme, font and high-contrast changes.
        let restyle = state.clone();
        let weak_window = window.downgrade();
        area.connect_style_updated(move |area| {
            // Font measurement can synchronously resolve a pending GTK style update.
            let Ok(mut state) = restyle.try_borrow_mut() else { return };
            state.button = state
                .view
                .as_ref()
                .and_then(|view| link_rect(area, view, area.allocated_width(), area.allocated_height()));
            let rect = state.button;
            drop(state);
            if let Some(window) = weak_window.upgrade() {
                set_clickable(&window, rect);
            }
            area.queue_draw();
        });
        if let Some(screen) = area.screen() {
            let canvas = area.downgrade();
            screen.connect_composited_changed(move |_| {
                if let Some(area) = canvas.upgrade() {
                    area.queue_draw();
                }
            });
        }
        let alive = available.clone();
        let destroy = state.clone();
        window.connect_destroy(move |_| {
            destroy.borrow_mut().stop_timers();
            alive.store(false, Ordering::SeqCst);
        });
        window.realize();
        if window.window().is_none() {
            return result;
        }
        set_clickable(&window, None);
        available.store(true, Ordering::SeqCst);
        let weak_pending = Arc::downgrade(&pending);
        let mut position = position;
        let mut last_size = window.size();
        glib::timeout_add_local(Duration::from_millis(40), move || {
            let Some(pending) = weak_pending.upgrade().filter(|_| available.load(Ordering::SeqCst)) else {
                state.borrow_mut().hide(&window);
                available.store(false, Ordering::SeqCst);
                window.close();
                return glib::ControlFlow::Break;
            };
            let next = std::mem::take(&mut *pending.lock().unwrap());
            if let Some(next_position) = next.position {
                position = next_position;
                if state.borrow().view.is_some() {
                    place(&window, layer, position);
                }
            }
            if let Some(view) = next.view {
                if let Some(view) = view {
                    let height = overlay_height(view.button.is_some(), 96);
                    {
                        let mut state = state.borrow_mut();
                        let session = view.recording.as_ref().map(|r| r.session);
                        let previous = state.view.as_ref().and_then(|v| v.recording.as_ref()).map(|r| r.session);
                        if session != previous {
                            state.stop_timers();
                        }
                        state.waveform.set_recording(view.recording.as_ref());
                        state.hide_at = view.hide_after.map(|delay| Instant::now() + delay);
                        // Retargeting an action cannot complete an old pointer gesture.
                        if state.view.as_ref().and_then(|v| v.button.as_ref()) != view.button.as_ref() {
                            state.pressed = false;
                        }
                        state.button = link_rect(&area, &view, area.allocated_width(), area.allocated_height());
                        state.view = Some(view);
                    }
                    area.set_size_request(WIDTH, height);
                    window.resize(WIDTH, height);
                    window.show_all();
                    place(&window, layer, position);
                    set_clickable(&window, state.borrow().button);
                    sync_timers(&state, &area);
                    area.queue_draw();
                } else {
                    state.borrow_mut().hide(&window);
                }
            }
            if state.borrow().view.is_some() {
                if window.size() != last_size {
                    last_size = window.size();
                    place(&window, layer, position);
                }
                let expired = state.borrow().hide_at.is_some_and(|deadline| Instant::now() >= deadline);
                if expired {
                    state.borrow_mut().hide(&window);
                }
            }
            glib::ControlFlow::Continue
        });
        result
    }

    pub fn available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }
}

impl Overlay for LinuxOverlay {
    fn available(&self) -> bool {
        self.available()
    }

    fn show(&self, view: OverlayView) {
        if self.available() {
            self.pending.lock().unwrap().view = Some(Some(view));
        } else {
            // Status only, never the private live preview. Repeated meter updates do not spam.
            let mut previous = self.notification.lock().unwrap();
            if previous
                .as_ref()
                .is_some_and(|(text, when)| text == &view.text || when.elapsed() < Duration::from_secs(1))
            {
                return;
            }
            *previous = Some((view.text.clone(), Instant::now()));
            let mut command = std::process::Command::new("notify-send");
            command.args(["--app-name=Opit Speech to Text", "--", "Opit Speech to Text", &view.text]);
            super::run_detached(command);
        }
    }

    fn hide(&self) {
        self.pending.lock().unwrap().view = Some(None);
    }
    fn set_position(&self, position: OverlayPosition) {
        self.pending.lock().unwrap().position = Some(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_footer_keeps_the_linux_cancel_link_clear_of_timer_and_waveform() {
        assert_eq!(meter_layout(WIDTH, HEIGHT, None), MeterLayout { top: 60, right: 334, timer_right: 404 });
        let button = button_rect(WIDTH, HEIGHT_WITH_BUTTON, 80);
        let meter = meter_layout(WIDTH, HEIGHT_WITH_BUTTON, Some(button));
        assert_eq!(meter, MeterLayout { top: 72, right: 242, timer_right: 312 });
        assert!(meter.right > PAD + DOT + 10);
        assert!(meter.timer_right < button.left);
        assert!(meter.top >= button.top && meter.top + 18 <= button.bottom);
        let long = button_rect(WIDTH, HEIGHT_WITH_BUTTON, 900);
        let meter = meter_layout(WIDTH, HEIGHT_WITH_BUTTON, Some(long));
        assert_eq!(meter.right, PAD + DOT + 10);
        assert_eq!(meter.timer_right, meter.right);
    }

    #[test]
    fn gtk_theme_detection_handles_light_dark_contrast_and_missing_settings() {
        for (prefer_dark, name, light, high_contrast) in [
            (false, None, false, false),
            (false, Some(""), false, false),
            (false, Some("Adwaita"), true, false),
            (true, Some("Adwaita"), false, false),
            (false, Some("Breeze-Dark"), false, false),
            (false, Some(" Adwaita-dark "), false, false),
            (false, Some("HighContrast"), true, true),
            (false, Some("HighContrastInverse"), false, true),
            (false, Some("High-Contrast-dark"), false, true),
        ] {
            assert_eq!(detect_theme(prefer_dark, name), Theme { light, high_contrast });
        }
    }

    #[test]
    fn link_hit_region_tracks_the_drawn_label_and_excludes_all_other_pixels() {
        let r = button_rect(WIDTH, HEIGHT_WITH_BUTTON, 80);
        assert_eq!(r, RectI { left: 324, top: 70, right: 404, bottom: 92 });
        assert!(contains(r, 324.0, 70.0));
        assert!(contains(r, 403.9, 91.9));
        for (x, y) in [(323.9, 70.0), (404.0, 70.0), (350.0, 69.9), (350.0, 92.0), (20.0, 20.0)] {
            assert!(!contains(r, x, y));
        }
        assert!(!contains(r, f64::NAN, 80.0));
        let long = button_rect(WIDTH, HEIGHT_WITH_BUTTON, 900);
        assert_eq!(long.left, PAD + DOT + 10);
        assert_eq!(long.right, WIDTH - PAD);
        assert_eq!(button_rect(10, 10, 100).width(), 0);
        assert_eq!(button_rect(WIDTH, HEIGHT_WITH_BUTTON, 0).width(), 0);
    }

    #[test]
    fn layer_positions_anchor_one_edge_and_center_the_other_axis() {
        assert_eq!(layer_edges(OverlayPosition::TopCenter), [true, false, false, false]);
        assert_eq!(layer_edges(OverlayPosition::RightCenter), [false, true, false, false]);
        assert_eq!(layer_edges(OverlayPosition::BottomCenter), [false, false, true, false]);
        assert_eq!(layer_edges(OverlayPosition::LeftCenter), [false, false, false, true]);
    }
}
