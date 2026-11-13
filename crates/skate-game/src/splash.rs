//! Startup splash (Windows, own thread and window) with the loading steps; not retail; `SKATE_NO_SPLASH=1` off.
//! Details: docs/hails-additions/08-setup-performance.md#startup-splash
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

/// What the splash shows: the current step and item, and the last finished lines (oldest first).
struct Text {
    stage: String,
    detail: String,
    history: Vec<String>,
    started: Option<Instant>,
    /// A warning that stays until the splash closes (out-of-date game files), drawn under the status line.
    warning: String,
}

static TEXT: Mutex<Text> = Mutex::new(Text { stage: String::new(), detail: String::new(), history: Vec::new(), started: None, warning: String::new() });
/// The splash window (0 = none, or closed).
static WINDOW: AtomicIsize = AtomicIsize::new(0);
const HISTORY_LINES: usize = 4;

fn push_history(t: &mut Text, line: String) {
    if !line.is_empty() {
        t.history.push(line);
        let excess = t.history.len().saturating_sub(HISTORY_LINES);
        t.history.drain(..excess);
    }
}

/// A new step ("Loading physics"); the previous step (and its last item) move to the finished lines.
pub(crate) fn stage(label: &str) {
    if let Ok(mut t) = TEXT.lock() {
        if t.stage == label {
            return;
        }
        let previous = std::mem::take(&mut t.detail);
        let previous = if previous.is_empty() { std::mem::take(&mut t.stage) } else { format!("{}: {previous}", std::mem::take(&mut t.stage)) };
        push_history(&mut t, previous);
        t.stage = label.to_owned();
    }
}

/// The item the current step reads now; the previous item moves to the finished lines.
pub(crate) fn detail(text: String) {
    if let Ok(mut t) = TEXT.lock() {
        let previous = std::mem::replace(&mut t.detail, text);
        if !previous.is_empty() {
            push_history(&mut t, previous);
        }
    }
}

/// A warning shown under the status until the splash closes (the start-up asset check's out-of-date features).
pub(crate) fn warning(text: String) {
    if let Ok(mut t) = TEXT.lock() {
        t.warning = text;
    }
}

/// The splash is up (not closed yet).
pub(crate) fn active() -> bool {
    WINDOW.load(Ordering::Relaxed) != 0
}

/// The line under the board.
fn line(t: &Text) -> String {
    let secs = t.started.map_or(0, |s| s.elapsed().as_secs());
    match (t.stage.is_empty(), t.detail.is_empty()) {
        (true, _) => format!("Starting  ({secs} s)"),
        (false, true) => format!("{}  ({secs} s)", t.stage),
        (false, false) => format!("{}: {}  ({secs} s)", t.stage, t.detail),
    }
}

// Win32 (user32 / gdi32 / kernel32).
type Hwnd = *mut c_void;
type Hdc = *mut c_void;
type Hgdi = *mut c_void;
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: [i32; 2],
    private: u32,
}
#[repr(C)]
struct PaintStruct {
    hdc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    proc_: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: *const c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: *mut c_void,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: Hwnd, menu: *mut c_void, instance: *const c_void, param: *mut c_void) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, ms: u32, func: *mut c_void) -> usize;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn BeginPaint(hwnd: Hwnd, ps: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, ps: *const PaintStruct) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn FillRect(hdc: Hdc, rect: *const Rect, brush: Hgdi) -> i32;
    fn DrawTextW(hdc: Hdc, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetThreadDpiAwarenessContext(context: isize) -> isize;
    fn GetDpiForSystem() -> u32;
    fn LoadCursorW(instance: *const c_void, name: usize) -> *mut c_void;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Hgdi;
    fn CreatePen(style: i32, width: i32, color: u32) -> Hgdi;
    fn DeleteObject(obj: Hgdi) -> i32;
    fn SelectObject(hdc: Hdc, obj: Hgdi) -> Hgdi;
    fn Ellipse(hdc: Hdc, l: i32, t: i32, r: i32, b: i32) -> i32;
    fn SetTextColor(hdc: Hdc, color: u32) -> u32;
    fn SetBkMode(hdc: Hdc, mode: i32) -> i32;
    fn CreateFontW(h: i32, w: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32, strike: u32, charset: u32, out: u32, clip: u32, quality: u32, pitch: u32, face: *const u16) -> Hgdi;
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn CreateCompatibleBitmap(hdc: Hdc, w: i32, h: i32) -> Hgdi;
    fn BitBlt(dst: Hdc, x: i32, y: i32, w: i32, h: i32, src: Hdc, sx: i32, sy: i32, rop: u32) -> i32;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn Polygon(hdc: Hdc, points: *const [i32; 2], count: i32) -> i32;
    fn StretchBlt(dst: Hdc, x: i32, y: i32, w: i32, h: i32, src: Hdc, sx: i32, sy: i32, sw: i32, sh: i32, rop: u32) -> i32;
    fn SetStretchBltMode(hdc: Hdc, mode: i32) -> i32;
    fn SetBrushOrgEx(hdc: Hdc, x: i32, y: i32, old: *mut [i32; 2]) -> i32;
    #[cfg(test)]
    fn GetDIBits(hdc: Hdc, bitmap: Hgdi, start: u32, lines: u32, bits: *mut c_void, info: *mut c_void, usage: u32) -> i32;
}
#[cfg(test)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *const c_void;
}

const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_TIMER: u32 = 0x0113;
const WM_NCHITTEST: u32 = 0x0084;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_APPWINDOW: u32 = 0x0004_0000;
const WIDTH: i32 = 560;
const HEIGHT: i32 = 240;

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const BACKGROUND: u32 = rgb(14, 14, 17);
const GRIP: u32 = rgb(34, 34, 38);
const PLY: u32 = rgb(214, 176, 124);
const PLY_DARK: u32 = rgb(150, 104, 62);
const PLY_EDGE: u32 = rgb(92, 62, 36);
const METAL: u32 = rgb(176, 181, 191);
const METAL_DARK: u32 = rgb(96, 100, 110);
const WHEEL: u32 = rgb(240, 235, 220);
const WHEEL_EDGE: u32 = rgb(168, 162, 148);
const WHEEL_CORE: u32 = rgb(86, 146, 214);
const BEARING: u32 = rgb(52, 54, 60);
const SHADOW: u32 = rgb(7, 7, 9);
const TEXT_BRIGHT: u32 = rgb(217, 217, 217);
const TEXT_DIM: u32 = rgb(110, 110, 115);
const TITLE: u32 = rgb(160, 160, 165);
const WARNING: u32 = rgb(236, 172, 76);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Show the splash (once per process; nothing with `SKATE_NO_SPLASH=1`).
pub(crate) fn show() {
    if std::env::var("SKATE_NO_SPLASH").is_ok_and(|v| v == "1") || WINDOW.load(Ordering::Relaxed) != 0 {
        return;
    }
    if let Ok(mut t) = TEXT.lock() {
        t.started = Some(Instant::now());
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("splash".into()).spawn(move || {
        // SAFETY: plain Win32 window creation and a message loop owned by this thread; every pointer passed is to
        // a live local (class, strings, message) for the duration of the call.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("Skate3RustSplash");
            let wc = WndClassExW {
                size: std::mem::size_of::<WndClassExW>() as u32,
                style: 0,
                proc_: wnd_proc,
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: std::ptr::null_mut(),
                cursor: LoadCursorW(std::ptr::null(), 32512),
                background: std::ptr::null_mut(),
                menu_name: std::ptr::null(),
                class_name: class.as_ptr(),
                icon_small: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
            // Its own DPI context (per-monitor v2, -4), fixed before the window exists: the renderer makes the
            // process DPI aware later, and a splash created unaware was rescaled by Windows mid-start. Sized for
            // the screen's scale; paint draws the 560 x 240 layout and scales it to whatever the window is.
            SetThreadDpiAwarenessContext(-4);
            let dpi = GetDpiForSystem().max(96) as i32;
            let (ww, wh) = (WIDTH * dpi / 96, HEIGHT * dpi / 96);
            let (sw, sh) = (GetSystemMetrics(0), GetSystemMetrics(1));
            let title = wide("Skate 3 Rust Engine");
            let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP | WS_VISIBLE, (sw - ww) / 2, (sh - wh) / 2, ww, wh, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());
            let _ = ready_tx.send(());
            if hwnd.is_null() {
                return;
            }
            WINDOW.store(hwnd as isize, Ordering::Relaxed);
            SetTimer(hwnd, 1, 33, std::ptr::null_mut());
            let mut msg: Msg = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            WINDOW.store(0, Ordering::Relaxed);
        }
    });
    if spawned.is_ok() {
        // The window is up before the slow start begins (bounded: never hold the start for it).
        let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(2));
    }
}

/// Close the splash (any thread; nothing when it is not up).
pub(crate) fn close() {
    let hwnd = WINDOW.swap(0, Ordering::Relaxed);
    if hwnd != 0 {
        // SAFETY: posting a message to a window handle; harmless if the window is already gone.
        unsafe {
            PostMessageW(hwnd as Hwnd, WM_CLOSE, 0, 0);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: called by the system on the splash thread with a valid window handle.
    unsafe {
        match msg {
            WM_TIMER => {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                0
            }
            WM_ERASEBKGND => 1,
            // Drag it anywhere.
            WM_NCHITTEST => 2,
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// One frame drawn into a memory bitmap: the side-view board at 4x, scaled down with HALFTONE.
unsafe fn draw_board(dc: Hdc, x: i32, y: i32, len: i32) {
    const S: f32 = 4.0;
    const H: i32 = 40;
    const PAD: i32 = 6;
    let l = len as f32;
    // SAFETY: GDI calls on a DC and objects created here and released before return.
    unsafe {
        let (bw, bh) = (((len + 2 * PAD) as f32 * S) as i32, (H as f32 * S) as i32);
        let big = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, bw, bh);
        let old_bitmap = SelectObject(big, bitmap);
        let bg = CreateSolidBrush(BACKGROUND);
        FillRect(big, &Rect { left: 0, top: 0, right: bw, bottom: bh }, bg);
        DeleteObject(bg);
        let p = |px: f32, py: f32| [((px + PAD as f32) * S) as i32, (py * S) as i32];
        let fill = |color: u32, edge: u32, edge_px: f32, shape: &dyn Fn(Hdc)| {
            let brush = CreateSolidBrush(color);
            let pen = if edge_px > 0.0 { CreatePen(0, (edge_px * S) as i32, edge) } else { CreatePen(5, 0, 0) };
            let (ob, op) = (SelectObject(big, brush), SelectObject(big, pen));
            shape(big);
            SelectObject(big, ob);
            SelectObject(big, op);
            DeleteObject(brush);
            DeleteObject(pen);
        };
        let circle = |cx: f32, cy: f32, r: f32| {
            let (a, b) = (p(cx - r, cy - r), p(cx + r, cy + r));
            move |d: Hdc| {
                Ellipse(d, a[0], a[1], b[0], b[1]);
            }
        };

        // Shadow on the ground.
        let (a, b) = (p(10.0, 32.5), p(l - 10.0, 37.5));
        fill(SHADOW, 0, 0.0, &move |d: Hdc| {
            Ellipse(d, a[0], a[1], b[0], b[1]);
        });

        // The deck's top line: tail kick, flat middle, nose kick (the nose a little longer and higher).
        let top: Vec<(f32, f32)> = vec![
            (1.0, 3.0), (5.0, 3.6), (11.0, 5.6), (18.0, 8.6), (25.0, 11.0), (31.0, 12.0),
            (l - 33.0, 12.0), (l - 26.0, 10.8), (l - 18.0, 7.8), (l - 10.0, 4.4), (l - 4.0, 2.4), (l - 1.0, 2.0),
        ];
        let deck_at = |down: f32| -> Vec<[i32; 2]> { top.iter().map(|&(px, py)| p(px, py + down)).collect() };
        let outline = |upper: f32, lower: f32| -> Vec<[i32; 2]> {
            let mut v = deck_at(upper);
            let mut under = deck_at(lower);
            under.reverse();
            v.extend(under);
            v
        };
        // Maple edge (5 px thick) with a darker centre ply, then the griptape on top.
        let wood = outline(0.0, 5.0);
        fill(PLY, PLY_EDGE, 0.5, &move |d: Hdc| {
            Polygon(d, wood.as_ptr(), wood.len() as i32);
        });
        let ply = outline(2.6, 3.4);
        fill(PLY_DARK, 0, 0.0, &move |d: Hdc| {
            Polygon(d, ply.as_ptr(), ply.len() as i32);
        });
        let grip = outline(0.0, 1.4);
        fill(GRIP, 0, 0.0, &move |d: Hdc| {
            Polygon(d, grip.as_ptr(), grip.len() as i32);
        });

        // Trucks: baseplate under the flat of the deck, the hanger down to the axle.
        for ax in [40.0, l - 42.0] {
            let plate = [p(ax - 10.0, 17.0), p(ax + 10.0, 17.0), p(ax + 9.0, 19.5), p(ax - 9.0, 19.5)];
            fill(METAL, METAL_DARK, 0.5, &move |d: Hdc| {
                Polygon(d, plate.as_ptr(), 4);
            });
            let hanger = [p(ax - 6.0, 19.5), p(ax + 6.0, 19.5), p(ax + 3.5, 26.0), p(ax - 3.5, 26.0)];
            fill(METAL, METAL_DARK, 0.5, &move |d: Hdc| {
                Polygon(d, hanger.as_ptr(), 4);
            });
            // Wheel: solid urethane with a coloured core and the bearing.
            let (cx, cy) = (ax, 26.5);
            fill(WHEEL, WHEEL_EDGE, 0.6, &circle(cx, cy, 7.5));
            fill(WHEEL_CORE, 0, 0.0, &circle(cx, cy, 3.9));
            fill(BEARING, 0, 0.0, &circle(cx, cy, 1.7));
        }

        SetStretchBltMode(dc, 4); // HALFTONE
        SetBrushOrgEx(dc, 0, 0, std::ptr::null_mut());
        StretchBlt(dc, x - PAD, y, len + 2 * PAD, H, big, 0, 0, bw, bh, 0x00CC_0020);
        SelectObject(big, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(big);
    }
}

unsafe fn paint(hwnd: Hwnd) {
    // SAFETY: GDI calls on DCs and objects created here and released before return.
    unsafe {
        let mut ps: PaintStruct = std::mem::zeroed();
        let screen = BeginPaint(hwnd, &mut ps);
        let mut client = Rect::default();
        GetClientRect(hwnd, &mut client);
        let (w, h) = (WIDTH, HEIGHT);
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old_bitmap = SelectObject(dc, bitmap);

        let bg = CreateSolidBrush(BACKGROUND);
        FillRect(dc, &Rect { left: 0, top: 0, right: w, bottom: h }, bg);
        DeleteObject(bg);

        let (text, history, started, warning) = match TEXT.lock() {
            Ok(t) => (line(&t), t.history.clone(), t.started, t.warning.clone()),
            Err(_) => (String::new(), Vec::new(), None, String::new()),
        };
        let t = started.map_or(0.0, |s| s.elapsed().as_secs_f32());

        // The board, side view, rolling left to right and coming round again.
        const BOARD_W: i32 = 180;
        const SPEED: f32 = 170.0;
        let travelled = t * SPEED;
        let x = (travelled % (w + BOARD_W) as f32) as i32 - BOARD_W;
        draw_board(dc, x, 52, BOARD_W);

        SetBkMode(dc, 1);
        let face = wide("Segoe UI");
        let font = |size: i32, weight: i32| CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
        let draw = |s: &str, size: i32, weight: i32, color: u32, rect: Rect, format: u32| {
            let f = font(size, weight);
            let old = SelectObject(dc, f);
            SetTextColor(dc, color);
            let mut r = rect;
            let ws: Vec<u16> = s.encode_utf16().collect();
            DrawTextW(dc, ws.as_ptr(), ws.len() as i32, &mut r, format);
            SelectObject(dc, old);
            DeleteObject(f);
        };
        // DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS (0x8000) | DT_NOPREFIX (0x800)
        let one_line = 0x1 | 0x20 | 0x8000 | 0x800;
        draw("SKATE 3 RUST ENGINE", 13, 600, TITLE, Rect { left: 0, top: 16, right: w, bottom: 36 }, one_line);
        draw(&text, 16, 400, TEXT_BRIGHT, Rect { left: 16, top: 122, right: w - 16, bottom: 146 }, one_line);
        // With a warning the finished lines make room for it (two lines kept, the warning in two wrapped lines below).
        let kept = if warning.is_empty() { HISTORY_LINES } else { 2 };
        for (i, h_line) in history.iter().rev().take(kept).enumerate() {
            let top = 152 + i as i32 * 19;
            draw(h_line, 13, 400, TEXT_DIM, Rect { left: 16, top, right: w - 16, bottom: top + 18 }, one_line);
        }
        if !warning.is_empty() {
            // DT_CENTER | DT_WORDBREAK (0x10) | DT_END_ELLIPSIS | DT_NOPREFIX
            draw(&warning, 13, 600, WARNING, Rect { left: 16, top: 194, right: w - 16, bottom: 232 }, 0x1 | 0x10 | 0x8000 | 0x800);
        }

        let (cw, ch) = (client.right, client.bottom);
        if (cw, ch) == (w, h) {
            BitBlt(screen, 0, 0, w, h, dc, 0, 0, 0x00CC_0020);
        } else {
            // HALFTONE (4) needs the brush origin reset.
            SetStretchBltMode(screen, 4);
            SetBrushOrgEx(screen, 0, 0, std::ptr::null_mut());
            StretchBlt(screen, 0, 0, cw, ch, dc, 0, 0, w, h, 0x00CC_0020);
        }
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        EndPaint(hwnd, &ps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders the board alone to a 24-bit BMP (`SKATE_SPLASH_BOARD_BMP=<path>`) to look at it without starting the game.
    #[test]
    #[ignore = "writes an image for a visual check"]
    fn board_renders_to_a_bitmap() {
        let Some(path) = std::env::var_os("SKATE_SPLASH_BOARD_BMP") else { return };
        let (w, h) = (240, 60);
        // SAFETY: a memory DC and bitmap created and released here; GetDIBits copies into an owned buffer.
        let pixels = unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let screen = GetDC(std::ptr::null_mut());
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(dc, bitmap);
            let bg = CreateSolidBrush(BACKGROUND);
            FillRect(dc, &Rect { left: 0, top: 0, right: w, bottom: h }, bg);
            DeleteObject(bg);
            draw_board(dc, 30, 6, 180);
            SelectObject(dc, old);
            let mut info = [0u32; 10];
            info[0] = 40;
            info[1] = w as u32;
            info[2] = h as u32;
            info[3] = 1 | 24 << 16;
            let row = ((w * 3 + 3) & !3) as usize;
            let mut buf = vec![0u8; row * h as usize];
            GetDIBits(dc, bitmap, 0, h as u32, buf.as_mut_ptr().cast(), info.as_mut_ptr().cast(), 0);
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
            buf
        };
        let mut file = Vec::new();
        let size = 54 + pixels.len() as u32;
        file.extend_from_slice(b"BM");
        for v in [size, 0, 54, 40, w as u32, h as u32] {
            file.extend_from_slice(&v.to_le_bytes());
        }
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&24u16.to_le_bytes());
        file.extend_from_slice(&[0u8; 24]);
        file.extend_from_slice(&pixels);
        std::fs::write(path, file).unwrap();
    }

    #[test]
    fn steps_and_items_become_finished_lines() {
        let mut t = Text { stage: String::new(), detail: String::new(), history: Vec::new(), started: None, warning: String::new() };
        push_history(&mut t, String::new());
        assert!(t.history.is_empty());
        for i in 0..6 {
            push_history(&mut t, format!("line {i}"));
        }
        assert_eq!(t.history, ["line 2", "line 3", "line 4", "line 5"]);
        t.stage = "Loading sound".into();
        t.detail = "sound bank grind".into();
        assert!(line(&t).starts_with("Loading sound: sound bank grind  ("));
    }
}
