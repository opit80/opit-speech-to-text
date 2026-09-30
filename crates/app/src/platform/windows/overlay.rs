//! Status overlay: a small layered, topmost, non-activating popup drawn with GDI on its own
//! UI thread. Other threads hand it state through a mutex and a coalesced `WM_APP + 1`.

use std::cell::RefCell;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use opit_core::config::OverlayPosition;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint, FW_NORMAL,
    FW_SEMIBOLD, FillRect, GetMonitorInfoW, GetStockObject, HDC, HFONT, HGDIOBJ, InvalidateRect,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, RoundRect,
    SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWL_EXSTYLE, GetClientRect, GetForegroundWindow, GetMessageW,
    GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, IDC_HAND, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::call_guarded;
use crate::platform::{Overlay, OverlaySink, OverlayView, Tone};

/// Posted by other threads when `Pending` has something new.
const WM_APP_UPDATE: u32 = WM_APP + 1;
const HIDE_TIMER: usize = 1;
const CLASS_NAME: PCWSTR = w!("OpitOverlayWindow");
const ALPHA: u8 = 235;

/// Logical (96-dpi) layout constants.
const WIDTH: i32 = 260;
const HEIGHT: i32 = 56;
const HEIGHT_WITH_BUTTON: i32 = 76;
const MARGIN: i32 = 24;
const PAD: i32 = 16;
const CORNER: i32 = 16;
const DOT: i32 = 10;
const ROW: i32 = 22;
const BAR: i32 = 4;

const BG: COLORREF = rgb(30, 31, 36);
const FG: COLORREF = rgb(236, 236, 240);
const TRACK: COLORREF = rgb(62, 63, 72);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn tone_color(tone: Tone) -> COLORREF {
    match tone {
        Tone::Neutral => rgb(138, 180, 248),
        Tone::Busy => rgb(178, 146, 255),
        Tone::Success => rgb(80, 200, 120),
        Tone::Warning => rgb(245, 182, 66),
        Tone::Error => rgb(242, 95, 92),
    }
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

impl From<RECT> for RectI {
    fn from(r: RECT) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
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

/// State handed from other threads to the window thread. Latest write wins.
#[derive(Default)]
struct Pending {
    /// Some(Some(view)) = show, Some(None) = hide.
    view: Option<Option<OverlayView>>,
    position: Option<OverlayPosition>,
    /// A WM_APP_UPDATE is already queued; do not post another (keeps level updates coalesced).
    posted: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Win32 overlay window. Cheap to call from any thread; all drawing happens on its own thread.
///
/// The sink is called on the overlay's UI thread when the button is clicked; it must not
/// block (use a non-blocking channel send).
pub struct WinOverlay {
    /// Raw HWND (HWND itself is not Send).
    hwnd: isize,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl WinOverlay {
    /// Starts the overlay thread and waits until its window exists. The overlay starts hidden.
    pub fn new(position: OverlayPosition, sink: OverlaySink) -> Result<WinOverlay, String> {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::sync_channel(1);
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("opit-overlay".into())
            .spawn(move || window_thread(position, sink, thread_shared, tx))
            .map_err(|e| format!("could not start the overlay thread: {e}"))?;
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(WinOverlay { hwnd, shared, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the overlay thread exited unexpectedly".into())
            }
        }
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    fn update(&self, f: impl FnOnce(&mut Pending)) {
        let mut pending = self.shared.lock();
        f(&mut pending);
        if !pending.posted {
            // SAFETY: plain FFI call; posting to a destroyed window just fails.
            pending.posted = unsafe { PostMessageW(Some(self.hwnd()), WM_APP_UPDATE, WPARAM(0), LPARAM(0)) }.is_ok();
        }
    }
}

impl Overlay for WinOverlay {
    fn show(&self, view: OverlayView) {
        self.update(|p| p.view = Some(Some(view)));
    }

    fn hide(&self) {
        self.update(|p| p.view = Some(None));
    }

    fn set_position(&self, position: OverlayPosition) {
        self.update(|p| p.position = Some(position));
    }
}

impl Drop for WinOverlay {
    fn drop(&mut self) {
        // WM_CLOSE -> DestroyWindow (DefWindowProc) -> WM_DESTROY -> PostQuitMessage.
        // SAFETY: plain FFI call.
        let posted = unsafe { PostMessageW(Some(self.hwnd()), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok();
        if let Some(thread) = self.thread.take()
            && (posted || thread.is_finished())
        {
            let _ = thread.join();
        }
    }
}

// ---------- window thread ----------

struct Fonts {
    dpi: u32,
    text: HFONT,
    link: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let make = |px: i32, weight: u32, underline: u32| {
            // SAFETY: plain FFI call with a static face name.
            unsafe {
                CreateFontW(
                    -scale(px, dpi),
                    0,
                    0,
                    0,
                    weight as i32,
                    0,
                    underline,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe UI"),
                )
            }
        };
        Self { dpi, text: make(15, FW_NORMAL.0, 0), link: make(14, FW_SEMIBOLD.0, 1) }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: we created both fonts and they are not selected into any DC anymore.
        unsafe {
            let _ = DeleteObject(self.text.into());
            let _ = DeleteObject(self.link.into());
        }
    }
}

struct WinState {
    shared: Arc<Shared>,
    sink: OverlaySink,
    /// The view on screen; None = hidden.
    view: Option<OverlayView>,
    position: OverlayPosition,
    dpi: u32,
    fonts: Option<Fonts>,
    clickable: bool,
}

thread_local! {
    static WIN: RefCell<Option<WinState>> = const { RefCell::new(None) };
}

/// Runs `f` on the window state. Returns None if there is no state or it is already borrowed
/// (re-entrant window messages), so a nested call can never panic.
fn with_state<R>(f: impl FnOnce(&mut WinState) -> R) -> Option<R> {
    WIN.with(|cell| cell.try_borrow_mut().ok().and_then(|mut guard| guard.as_mut().map(f)))
}

fn window_thread(
    position: OverlayPosition,
    sink: OverlaySink,
    shared: Arc<Shared>,
    ready: SyncSender<Result<isize, String>>,
) {
    // SAFETY: FFI calls with valid arguments; the class name and title are static strings.
    let hwnd = unsafe {
        // Physical-pixel coordinates regardless of the process manifest.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = match GetModuleHandleW(PCWSTR::null()) {
            Ok(m) => m.into(),
            Err(e) => {
                let _ = ready.send(Err(format!("GetModuleHandleW failed: {}", e.message())));
                return;
            }
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            let _ = ready.send(Err(format!("RegisterClassW failed: {}", windows::core::Error::from_thread())));
            return;
        }
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        let hwnd =
            match CreateWindowExW(ex, CLASS_NAME, w!("Opit"), WS_POPUP, 0, 0, 1, 1, None, None, Some(instance), None) {
                Ok(h) => h,
                Err(e) => {
                    let _ = ready.send(Err(format!("CreateWindowExW failed: {}", e.message())));
                    return;
                }
            };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), ALPHA, LWA_ALPHA);
        hwnd
    };
    WIN.set(Some(WinState { shared, sink, view: None, position, dpi: 96, fonts: None, clickable: false }));
    let _ = ready.send(Ok(hwnd.0 as isize));

    // SAFETY: standard message loop; `msg` outlives every call.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    WIN.set(None); // drops fonts
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_UPDATE => {
            apply_pending(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // WM_PAINT covers every pixel
        WM_TIMER if wparam.0 == HIDE_TIMER => {
            hide_now(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR if with_state(|s| s.clickable) == Some(true) => {
            // SAFETY: plain FFI calls with a system cursor id.
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let clicked = with_state(|s| {
                let action = s.view.as_ref()?.button.as_ref()?.action;
                Some((action, Arc::clone(&s.sink)))
            })
            .flatten();
            if let Some((action, sink)) = clicked {
                hide_now(hwnd);
                call_guarded(|| sink(action));
            }
            LRESULT(0)
        }
        // We size ourselves for the target monitor; ignore the suggested rect.
        WM_DPICHANGED => LRESULT(0),
        WM_DESTROY => {
            // SAFETY: plain FFI call on the window's own thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default handling with the unchanged arguments.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Drains `Pending` and applies it. No state borrow is held across calls that can re-enter
/// the window procedure (SetWindowPos, ShowWindow, SetWindowLongPtrW, SetWindowRgn).
fn apply_pending(hwnd: HWND) {
    let Some(shared) = with_state(|s| Arc::clone(&s.shared)) else {
        return;
    };
    let (view, position) = {
        let mut p = shared.lock();
        p.posted = false;
        (p.view.take(), p.position.take())
    };
    let moved = position.and_then(|pos| {
        with_state(|s| {
            let changed = s.position != pos;
            s.position = pos;
            changed
        })
    }) == Some(true);
    match view {
        Some(Some(view)) => show_view(hwnd, view, moved),
        Some(None) => hide_now(hwnd),
        None if moved && with_state(|s| s.view.is_some()) == Some(true) => {
            let has_button = with_state(|s| s.view.as_ref().is_some_and(|v| v.button.is_some())).unwrap_or(false);
            place(hwnd, has_button);
        }
        None => {}
    }
}

fn show_view(hwnd: HWND, view: OverlayView, force_place: bool) {
    let has_button = view.button.is_some();
    let hide_after = view.hide_after;
    let Some(previous) = with_state(|s| s.view.replace(view)) else {
        return;
    };
    // SAFETY: timer calls on our own window.
    unsafe {
        match hide_after {
            Some(d) => {
                let ms = u32::try_from(d.as_millis()).unwrap_or(u32::MAX).max(1);
                SetTimer(Some(hwnd), HIDE_TIMER, ms, None);
            }
            None => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
            }
        }
    }
    set_clickable(hwnd, has_button);
    let height_changed = previous.as_ref().is_some_and(|p| p.button.is_some() != has_button);
    if previous.is_none() || force_place || height_changed {
        place(hwnd, has_button);
    }
    // SAFETY: plain FFI call on our own window.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn hide_now(hwnd: HWND) {
    with_state(|s| s.view = None);
    // SAFETY: plain FFI calls on our own window.
    unsafe {
        let _ = KillTimer(Some(hwnd), HIDE_TIMER);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Toggles WS_EX_TRANSPARENT: click-through unless the view has a button.
fn set_clickable(hwnd: HWND, clickable: bool) {
    if with_state(|s| std::mem::replace(&mut s.clickable, clickable)) == Some(clickable) {
        return;
    }
    // SAFETY: reading/writing our own window's extended style.
    unsafe {
        let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let ex = if clickable { WINDOW_EX_STYLE(ex.0 & !WS_EX_TRANSPARENT.0) } else { ex | WS_EX_TRANSPARENT };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex.0 as isize);
    }
}

/// Sizes and positions the window on the foreground window's monitor, then shows it.
fn place(hwnd: HWND, has_button: bool) {
    // SAFETY: FFI calls with valid out-pointers; the region is owned by the system after
    // SetWindowRgn succeeds.
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            dpi_x = 96;
        }
        let Some(position) = with_state(|s| {
            s.dpi = dpi_x;
            s.position
        }) else {
            return;
        };
        let r = overlay_rect(info.rcWork.into(), position, dpi_x, overlay_height(has_button, dpi_x));
        let (w, h) = (r.width(), r.height());
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, w, h, SWP_NOACTIVATE | SWP_FRAMECHANGED);
        let corner = scale(CORNER, dpi_x);
        let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, corner, corner);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: BeginPaint/EndPaint pair on our own window; always called so WM_PAINT is validated.
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    let snapshot = with_state(|s| {
        let view = s.view.clone()?;
        if s.fonts.as_ref().is_none_or(|f| f.dpi != s.dpi) {
            s.fonts = Some(Fonts::new(s.dpi));
        }
        let fonts = s.fonts.as_ref()?;
        Some((view, s.dpi, fonts.text, fonts.link))
    })
    .flatten();
    if let Some((view, dpi, text_font, link_font)) = snapshot {
        let mut rc = RECT::default();
        // SAFETY: valid out-pointer.
        if unsafe { GetClientRect(hwnd, &mut rc) }.is_ok() && rc.right > 0 && rc.bottom > 0 {
            // SAFETY: `hdc` is the paint DC for this window.
            unsafe { draw_buffered(hdc, rc.right, rc.bottom, &view, dpi, text_font, link_font) };
        }
    }
    // SAFETY: pairs with BeginPaint above.
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Renders into an off-screen bitmap and blits it in one go (no flicker).
///
/// # Safety
/// `hdc` must be a valid device context.
unsafe fn draw_buffered(hdc: HDC, w: i32, h: i32, view: &OverlayView, dpi: u32, text_font: HFONT, link_font: HFONT) {
    // SAFETY: every GDI object created here is deselected and deleted before returning.
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let bitmap = CreateCompatibleBitmap(hdc, w, h);
        let old_bitmap = SelectObject(mem, bitmap.into());
        draw(mem, w, h, view, dpi, text_font, link_font);
        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
    }
}

/// Fills `rect` with `color`.
unsafe fn fill(dc: HDC, rect: &RECT, color: COLORREF) {
    // SAFETY: brush created and deleted here.
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

/// Draws a filled rounded rectangle (or ellipse when `round` == the height) without outline.
unsafe fn round_rect(dc: HDC, r: RECT, round: i32, color: COLORREF, ellipse: bool) {
    // SAFETY: brush created, selected, deselected and deleted here; NULL_PEN is a stock object.
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(dc, brush.into());
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        // NULL_PEN draws nothing but shrinks the fill by one pixel; compensate.
        if ellipse {
            let _ = Ellipse(dc, r.left, r.top, r.right + 1, r.bottom + 1);
        } else {
            let _ = RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, round, round);
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe fn text(dc: HDC, s: &str, mut r: RECT, font: HFONT, color: COLORREF, align: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: font is a valid HFONT; the previous font is restored.
    unsafe {
        let old: HGDIOBJ = SelectObject(dc, font.into());
        SetTextColor(dc, color);
        DrawTextW(dc, &mut wide, &mut r, align | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
        SelectObject(dc, old);
    }
}

unsafe fn draw(dc: HDC, w: i32, h: i32, view: &OverlayView, dpi: u32, text_font: HFONT, link_font: HFONT) {
    let s = |v: i32| scale(v, dpi);
    let accent = tone_color(view.tone);
    let pad = s(PAD);
    let row = s(ROW);
    // Text row: top-aligned when a second element (button or level bar) sits below it.
    let row_top = if view.button.is_some() {
        s(12)
    } else if view.level.is_some() {
        s(9)
    } else {
        (h - row) / 2
    };
    let dot = s(DOT);
    let text_left = pad + dot + s(10);
    // SAFETY: `dc` is the memory DC set up by draw_buffered.
    unsafe {
        fill(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, BG);
        SetBkMode(dc, TRANSPARENT);

        let dot_top = row_top + (row - dot) / 2;
        round_rect(dc, RECT { left: pad, top: dot_top, right: pad + dot, bottom: dot_top + dot }, dot, accent, true);

        let text_rect = RECT { left: text_left, top: row_top, right: w - pad, bottom: row_top + row };
        text(dc, &view.text, text_rect, text_font, FG, DT_LEFT);

        if let Some(button) = &view.button {
            let bottom = h - s(12);
            let r = RECT { left: text_left, top: bottom - row, right: w - pad, bottom };
            text(dc, &button.label, r, link_font, accent, DT_RIGHT);
        } else if let Some(level) = view.level {
            let bar = s(BAR);
            let top = h - s(15);
            let track = RECT { left: text_left, top, right: w - pad, bottom: top + bar };
            round_rect(dc, track, bar, TRACK, false);
            let span = (track.right - track.left) as f32;
            let filled = (span * level.clamp(0.0, 1.0)).round() as i32;
            if filled > 0 {
                let fill_rect = RECT { right: track.left + filled.max(bar), ..track };
                round_rect(dc, fill_rect, bar, accent, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::platform::{OverlayAction, OverlayButton};

    const WORK: RectI = RectI { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn scale_rounds() {
        assert_eq!(scale(260, 96), 260);
        assert_eq!(scale(260, 144), 390);
        assert_eq!(scale(24, 120), 30);
        assert_eq!(scale(10, 120), 13); // 12.5 rounds up
        assert_eq!(overlay_height(false, 96), 56);
        assert_eq!(overlay_height(true, 192), 152);
    }

    #[test]
    fn right_center_at_96_dpi() {
        let r = overlay_rect(WORK, OverlayPosition::RightCenter, 96, 56);
        assert_eq!(
            r,
            RectI { left: 1920 - 24 - 260, top: (1040 - 56) / 2, right: 1920 - 24, bottom: (1040 - 56) / 2 + 56 }
        );
    }

    #[test]
    fn all_positions_at_150_percent() {
        let h = overlay_height(false, 144); // 84
        let (w, m) = (390, 36);
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
        assert_eq!((r.left, r.top), (-2560 + (2560 - 260) / 2, 48 + 24));
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

    /// Creates the overlay, animates a Listening level for 1.5 s, shows an Error with a button
    /// for 1.5 s, hides and drops it.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_smoke() {
        let clicks = Arc::new(Mutex::new(Vec::new()));
        let clicks2 = Arc::clone(&clicks);
        let overlay =
            WinOverlay::new(OverlayPosition::RightCenter, Arc::new(move |a| clicks2.lock().unwrap().push(a))).unwrap();
        for i in 0..30 {
            let level = ((i as f32) * 0.45).sin().abs();
            overlay.show(OverlayView {
                tone: Tone::Neutral,
                text: "Listening…".into(),
                level: Some(level),
                button: None,
                hide_after: None,
            });
            thread::sleep(Duration::from_millis(50));
        }
        overlay.show(OverlayView {
            tone: Tone::Error,
            text: "The API key was rejected by the provider".into(),
            level: None,
            button: Some(OverlayButton { action: OverlayAction::OpenSettings, label: "Open settings".into() }),
            hide_after: Some(Duration::from_secs(5)),
        });
        thread::sleep(Duration::from_millis(1500));
        overlay.set_position(OverlayPosition::BottomCenter);
        thread::sleep(Duration::from_millis(300));
        overlay.hide();
        thread::sleep(Duration::from_millis(100));
        drop(overlay);
        println!("clicks: {:?}", clicks.lock().unwrap());
    }

    /// hide_after hides on the window thread by itself.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_auto_hide() {
        let overlay = WinOverlay::new(OverlayPosition::TopCenter, Arc::new(|_| {})).unwrap();
        overlay.show(OverlayView {
            tone: Tone::Success,
            text: "Pasted".into(),
            level: None,
            button: None,
            hide_after: Some(Duration::from_millis(300)),
        });
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay));
        thread::sleep(Duration::from_millis(500));
        assert!(!visible(&overlay));
    }

    fn visible(o: &WinOverlay) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        // SAFETY: plain FFI call.
        unsafe { IsWindowVisible(o.hwnd()) }.as_bool()
    }

    fn click_through(o: &WinOverlay) -> bool {
        // SAFETY: plain FFI call.
        let ex = unsafe { GetWindowLongPtrW(o.hwnd(), GWL_EXSTYLE) } as u32;
        ex & WS_EX_TRANSPARENT.0 != 0
    }

    /// A button makes the overlay clickable; a click (posted, so the real mouse is untouched)
    /// fires the action and hides; a view without a button is click-through again.
    #[test]
    #[ignore = "opens a real window on the desktop"]
    fn overlay_button_click() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let overlay = WinOverlay::new(
            OverlayPosition::BottomCenter,
            Arc::new(move |a| {
                let _ = tx.lock().unwrap().send(a);
            }),
        )
        .unwrap();
        let view = |button: Option<OverlayButton>| OverlayView {
            tone: Tone::Warning,
            text: "Nothing was heard".into(),
            level: None,
            button,
            hide_after: None,
        };
        thread::sleep(Duration::from_millis(50));
        assert!(click_through(&overlay));
        overlay.show(view(Some(OverlayButton { action: OverlayAction::Retry, label: "Retry".into() })));
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay) && !click_through(&overlay));
        // SAFETY: plain FFI call.
        unsafe { PostMessageW(Some(overlay.hwnd()), WM_LBUTTONUP, WPARAM(0), LPARAM(0)) }.unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(OverlayAction::Retry));
        thread::sleep(Duration::from_millis(50));
        assert!(!visible(&overlay));
        overlay.show(view(None));
        thread::sleep(Duration::from_millis(150));
        assert!(visible(&overlay) && click_through(&overlay));
        // A click without a button does nothing.
        // SAFETY: plain FFI call.
        unsafe { PostMessageW(Some(overlay.hwnd()), WM_LBUTTONUP, WPARAM(0), LPARAM(0)) }.unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(visible(&overlay));
        assert!(rx.try_recv().is_err());
    }
}
