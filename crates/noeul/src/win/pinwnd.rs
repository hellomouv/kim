//! Win32 pin windows: borderless, always-on-top, layered tool windows (NOEUL-FR-060,
//! NOEUL-FR-061, NOEUL-FR-062, NOEUL-FR-063, NOEUL-FR-026; ADR 0023 §2, ADR 0029, ADR 0044).
//!
//! - **Never on the taskbar or in Alt+Tab** (`WS_EX_TOOLWINDOW`, no `WS_EX_APPWINDOW`).
//! - **Retained picture.** The window's surface is painted once per change
//!   ([`transform::paint`]: the picture scaled to its rectangle — at 100 % one image pixel per
//!   device pixel — inside a one-pixel outline and the soft pin shadow, plus the click-through
//!   badge) into a temporary premultiplied DIB section and handed to `UpdateLayeredWindow`, which
//!   keeps its own copy; the DIB is freed at once. Opacity is the constant alpha of the same call
//!   with no new surface (nothing is painted again). No `WM_PAINT`, no timer, no GPU context: an
//!   idle or hidden pin costs no CPU (NOEUL-NFR-001, NOEUL-NFR-007), and its memory is measured
//!   in ADR 0029 (NOEUL-NFR-002).
//! - **Focus only when the user asks.** Created with `SW_SHOWNOACTIVATE` unless the user made it
//!   (then it takes focus so `Esc` and the arrows work at once); a click activates it. Hiding or
//!   closing the focused pin first hands focus back to the window the user was in before the
//!   pin took it (never to another pin), so the next key reaches that program, not a pin.
//!   Showing, moving, zooming and transforming never activate anything. A window created for the
//!   debug-build test hook ([`Style::OFF_SCREEN_TEST`]) is never topmost and can never be
//!   activated.
//! - **Z-order.** A new pin, and a pin shown again, go to the top of the topmost band once;
//!   pins never fight other topmost windows for it (no timer, no re-assertion). The capture
//!   overlay, created later, lies above them.
//! - **Input stays Noeul's own**: keys reach the pin only while it has focus; they are matched
//!   against the `pin` context and never forwarded anywhere (NOEUL-SEC-012). The IME is switched
//!   off for the window so letter shortcuts work in Hangul mode (NOEUL-NFR-014).
//! - **Mouse.** Dragging the picture moves it (the system move loop, `HTCAPTION`); dragging its
//!   outline, shadow or the band just inside its edge resizes it with the aspect ratio kept (the
//!   window's own capture and mouse messages, no system sizing loop); the wheel zooms around the
//!   pointer, `Ctrl`+wheel changes the opacity; a middle-click returns to 100 % and opaque; a
//!   right-click (or `Shift+F10`, or the menu key) opens its context menu; `Shift`+double-click
//!   toggles thumbnail mode; dropping it with `Ctrl` held onto another pin joins that pin's group.
//!   A plain double-click closes the pin when `[pins] double_click = "close"` (the default since
//!   ADR 0094; `"none"` does nothing, NOEUL-FR-026). For the guard time a new pin was given (one
//!   double-click time after the double-click that made it), the rest of that gesture cannot act
//!   on it: its release, a double-click, the middle and right buttons and the menu are ignored —
//!   but a left press starts a drag at once (ADR 0094: the old guard swallowed every press, so a
//!   pin could not be dragged for half a second after it appeared).
//! - **Click-through** (NOEUL-FR-063): `WS_EX_TRANSPARENT` on the layered window, so the system
//!   hit-tests through it to the window below; the pin keeps keyboard focus if it had it (its
//!   `Ctrl+Shift+T` turns it back) and shows the badge.
//! - **Displays.** The rectangle `WM_DPICHANGED` suggests is ignored: a pin keeps its pixel size
//!   on every monitor (NOEUL-FR-061, NOEUL-FR-091). The message itself is reported to the core
//!   ([`PinEvent::DpiChanged`], Step 58), which looks at the displays once they are quiet and
//!   redraws the pin's outline, shadow and resize band for its monitor's scale.
//!
//! The window procedure only queues [`PinEvent`]s and calls the registered waker; the instance
//! core handles them later, never re-entrantly. The context menu is shown by [`track_menu`],
//! which the runtime calls with the core released.

use crate::app::{PinEvent, PinMenuItem};
use crate::frame::{Frame, Rect};
use crate::hotkeys::chord_of;
use crate::pins::{PinId, PinWindow, PinWindows, WindowSpec};
use crate::transform::{self, Edge, View};
use crate::win::wide;
use crate::zoom;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP,
    HDC, ReleaseDC, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::MK_CONTROL;
use windows_sys::Win32::UI::Input::Ime::ImmAssociateContextEx;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, DragQueryPoint, HDROP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_DBLCLKS, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, EnumWindows, GWL_EXSTYLE, GWLP_USERDATA, GetForegroundWindow, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTLEFT,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_SIZEALL,
    IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IsIconic, IsWindow, IsWindowVisible,
    KillTimer, LoadCursorW, MA_ACTIVATE, MA_NOACTIVATE, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterClassExW, SM_CXDRAG, SM_CYDRAG, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, ULW_ALPHA, UpdateLayeredWindow, WA_INACTIVE,
    WM_ACTIVATE, WM_CAPTURECHANGED, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DPICHANGED,
    WM_DROPFILES, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVING, WM_NCDESTROY,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMBUTTONDBLCLK,
    WM_NCMBUTTONDOWN, WM_NCMBUTTONUP, WM_NCRBUTTONDBLCLK, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP,
    WM_NULL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: &str = "NoeulPin";

/// A border drag in progress.
#[derive(Debug, Clone, Copy)]
struct Drag {
    edge: Edge,
    /// The picture's rectangle when it began.
    start: Rect,
    /// The pointer (screen) when it began.
    from: (i32, i32),
    /// Whether this window holds the mouse capture for it.
    captured: bool,
}

/// What the runtime knows about one live pin window.
#[derive(Debug, Clone, Copy)]
struct Slot {
    id: PinId,
    /// The window the user was in before this pin took focus (never a pin); 0 when unknown.
    prev: isize,
    /// Presses are swallowed until then (the rest of the gesture that created the pin).
    guard_until: Option<Instant>,
    /// How often the window was activated (tests check that nothing activated it).
    activations: u32,
    /// What the window shows now: the picture's rectangle, its margin and grab band.
    view: View,
    /// A border drag in progress.
    drag: Option<Drag>,
    /// Real pins take the mouse capture while their border is dragged; the test hook's
    /// windows (never activated, off-screen) do not.
    capture: bool,
    /// #99 (`PIN-08`): where (screen) the right button went down on the picture.
    right_from: Option<(i32, i32)>,
    /// ADR 0094: the window was presented at least once (a first present shows no zoom).
    presented: bool,
    /// ADR 0094: the zoom read-out is shown now (the surface without it is in [`BADGES`]).
    badge: bool,
}

thread_local! {
    static EVENTS: RefCell<VecDeque<PinEvent>> = const { RefCell::new(VecDeque::new()) };
    static WAKER: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    static SLOTS: RefCell<BTreeMap<isize, Slot>> = const { RefCell::new(BTreeMap::new()) };
    /// The debug-build test hook: no key state is read ([`ignore_key_state`]).
    static IGNORE_KEYS: Cell<bool> = const { Cell::new(false) };
    /// #99 (`PIN-17`): the edges a pin being moved may snap to (visible top-level windows,
    /// other pins included), read once when the move begins.
    static SNAP_TO: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
}

/// ADR 0094: the zoom read-out's one-shot timer (the window's own; no timer while hidden).
const BADGE_TIMER: usize = 0x4E5A;

thread_local! {
    /// Lays the zoom read-out's text out (Segoe UI with fallback), made on first use.
    static TYPE: RefCell<Option<crate::win::typeset::GdiType>> = const { RefCell::new(None) };
    /// ADR 0094: while a pin shows its zoom read-out, its surface without it (to put back).
    static BADGES: RefCell<BTreeMap<isize, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
}

/// How close (device pixels) an edge must come to snap while `Shift` is held.
const SNAP_PX: i64 = 12;

/// The rectangles of the visible, not minimized top-level windows other than `skip` (only their
/// position: no title or content is read).
fn snap_targets(skip: HWND) -> Vec<Rect> {
    unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> i32 {
        // SAFETY: `data` is the `&mut Vec<usize>` passed below, alive during EnumWindows.
        unsafe { (*(data as *mut Vec<usize>)).push(h as usize) };
        1
    }
    let mut handles: Vec<usize> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, passed by pointer for the call.
    unsafe { EnumWindows(Some(collect), (&raw mut handles) as LPARAM) };
    handles
        .into_iter()
        .map(|h| h as HWND)
        .filter(|h| *h != skip)
        // SAFETY: plain queries of window handles from the enumeration.
        .filter(|h| unsafe { IsWindowVisible(*h) != 0 && IsIconic(*h) == 0 })
        .filter_map(|h| {
            // Another pin: its picture, without the outline and shadow margin.
            if let Some(s) = slot(h) {
                let p = s.view.rect;
                return Some(RECT {
                    left: p.x,
                    top: p.y,
                    right: p.x.saturating_add(i32::try_from(p.width).unwrap_or(0)),
                    bottom: p.y.saturating_add(i32::try_from(p.height).unwrap_or(0)),
                });
            }
            // Another window: its visible frame (Windows 10 and later draw an invisible resize
            // border outside it), else its window rectangle.
            let mut r = RECT::default();
            // SAFETY: `r` is a writable RECT of the size passed.
            let dwm = unsafe {
                DwmGetWindowAttribute(
                    h,
                    DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                    (&raw mut r).cast(),
                    u32::try_from(size_of::<RECT>()).unwrap_or(16),
                )
            };
            if dwm == 0 {
                return Some(r);
            }
            // SAFETY: `r` is writable.
            (unsafe { GetWindowRect(h, &raw mut r) } != 0).then_some(r)
        })
        .filter(|r| r.right > r.left && r.bottom > r.top)
        .map(|r| {
            Rect::new(
                r.left,
                r.top,
                u32::try_from(r.right - r.left).unwrap_or(1),
                u32::try_from(r.bottom - r.top).unwrap_or(1),
            )
        })
        .take(512)
        .collect()
}

/// Sets what runs after a pin window queued an event (the instance's pump). Same thread.
pub fn set_waker(f: Option<Box<dyn Fn()>>) {
    WAKER.with(|w| *w.borrow_mut() = f);
}

/// Takes the queued pin events (in order).
pub fn take_events() -> Vec<PinEvent> {
    EVENTS.with(|e| e.borrow_mut().drain(..).collect())
}

fn push(e: PinEvent) {
    EVENTS.with(|q| q.borrow_mut().push_back(e));
    // A waker that is already running (re-entrant dispatch) is not called again: the outer
    // pump drains the queue when it finishes.
    WAKER.with(|w| {
        if let Ok(w) = w.try_borrow()
            && let Some(f) = w.as_ref()
        {
            f();
        }
    });
}

fn slot(hwnd: HWND) -> Option<Slot> {
    SLOTS.with(|s| s.try_borrow().ok()?.get(&(hwnd as isize)).copied())
}

fn with_slot(hwnd: HWND, f: impl FnOnce(&mut Slot)) {
    SLOTS.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut()
            && let Some(slot) = s.get_mut(&(hwnd as isize))
        {
            f(slot);
        }
    });
}

/// The window of an open pin on this thread.
pub fn hwnd_of(id: PinId) -> Option<HWND> {
    SLOTS.with(|s| {
        s.borrow()
            .iter()
            .find(|(_, slot)| slot.id == id)
            .map(|(h, _)| *h as HWND)
    })
}

/// How often the pin window was activated since it was created.
pub fn activations(hwnd: HWND) -> Option<u32> {
    slot(hwnd).map(|s| s.activations)
}

/// What the pin window shows now (its last view), for tests and measurements.
pub fn view_of(hwnd: HWND) -> Option<View> {
    slot(hwnd).map(|s| s.view)
}

fn is_pin(hwnd: HWND) -> bool {
    slot(hwnd).is_some()
}

/// The window to give focus back to when `hwnd` (about to be activated) takes it from
/// `from`: `from` itself, or, when `from` is another pin, the window that pin got it from.
fn origin_of(from: HWND) -> isize {
    if from.is_null() {
        return 0;
    }
    match slot(from) {
        Some(s) => s.prev,
        None => from as isize,
    }
}

fn guarded(hwnd: HWND) -> bool {
    slot(hwnd)
        .and_then(|s| s.guard_until)
        .is_some_and(|t| Instant::now() < t)
}

/// The logical key name of a virtual key, as the hotkey contract spells it (§5.2).
pub fn vk_name(vk: u32) -> Option<String> {
    Some(match vk {
        0x41..=0x5A | 0x30..=0x39 => char::from(u8::try_from(vk).ok()?).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x1B => "Escape".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x20 => "Space".into(),
        0x08 => "Backspace".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x26 => "Up".into(),
        0x28 => "Down".into(),
        0x25 => "Left".into(),
        0x27 => "Right".into(),
        0xBB => "Plus".into(),
        0xBD => "Minus".into(),
        0xBC => "Comma".into(),
        0xBE => "Period".into(),
        0x6A => "NumpadMultiply".into(),
        0x6B => "NumpadAdd".into(),
        0x6D => "NumpadSubtract".into(),
        0x6E => "NumpadDecimal".into(),
        0x6F => "NumpadDivide".into(),
        _ => return None,
    })
}

/// Whether a key is down for the message being processed. Under the debug-build test hook
/// ([`ignore_key_state`]) the pins are never activated, so they never have the keyboard: any key
/// state their thread sees can only be the user's real keyboard leaking in (a modifier held
/// elsewhere while a test runs), and it reads as up.
fn down(vk: u16) -> bool {
    // SAFETY: plain key-state query for this thread's input.
    !IGNORE_KEYS.get() && unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// The debug-build test hook's instance reads no key state for its pins (see [`down`]): the
/// messages its tests post carry everything. Same thread as the pins.
pub fn ignore_key_state(ignore: bool) {
    IGNORE_KEYS.set(ignore);
}

fn pin_of(hwnd: HWND) -> Option<PinId> {
    // SAFETY: reads the value stored by `create` (0 before it is set).
    let v = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    u64::try_from(v).ok().filter(|v| *v > 0).map(PinId)
}

/// Screen coordinates packed in a mouse message's `lParam` (or client coordinates, for client
/// messages).
fn point_of(lp: LPARAM) -> (i32, i32) {
    let x = i32::from((lp & 0xFFFF) as u16 as i16);
    let y = i32::from(((lp >> 16) & 0xFFFF) as u16 as i16);
    (x, y)
}

/// The hit-test code of a border.
fn ht_of(e: Edge) -> u32 {
    match e {
        Edge::Left => HTLEFT,
        Edge::Right => HTRIGHT,
        Edge::Top => HTTOP,
        Edge::Bottom => HTBOTTOM,
        Edge::TopLeft => HTTOPLEFT,
        Edge::TopRight => HTTOPRIGHT,
        Edge::BottomLeft => HTBOTTOMLEFT,
        Edge::BottomRight => HTBOTTOMRIGHT,
    }
}

/// The border of a hit-test code.
fn edge_of(ht: u32) -> Option<Edge> {
    Some(match ht {
        HTLEFT => Edge::Left,
        HTRIGHT => Edge::Right,
        HTTOP => Edge::Top,
        HTBOTTOM => Edge::Bottom,
        HTTOPLEFT => Edge::TopLeft,
        HTTOPRIGHT => Edge::TopRight,
        HTBOTTOMLEFT => Edge::BottomLeft,
        HTBOTTOMRIGHT => Edge::BottomRight,
        _ => return None,
    })
}

/// The window's top-left corner on screen (client coordinates start there: no frame).
/// The paths of a drop (at most [`MAX_DROPPED`]) and its point in client coordinates.
fn dropped_files(drop: HDROP) -> (Vec<std::path::PathBuf>, (i32, i32)) {
    let mut files = Vec::new();
    // SAFETY: a drop handle from WM_DROPFILES; 0xFFFFFFFF asks the count.
    let count = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
    for i in 0..count.min(MAX_DROPPED) {
        // SAFETY: the length (in characters, without the NUL) of file `i`.
        let len = unsafe { DragQueryFileW(drop, i, std::ptr::null_mut(), 0) } as usize;
        if len == 0 || len > 32_767 {
            continue;
        }
        let mut buf = vec![0u16; len + 1];
        // SAFETY: `buf` holds `len + 1` characters.
        let n = unsafe {
            DragQueryFileW(
                drop,
                i,
                buf.as_mut_ptr(),
                u32::try_from(len + 1).unwrap_or(0),
            )
        } as usize;
        files.push(std::path::PathBuf::from(String::from_utf16_lossy(
            &buf[..n.min(len)],
        )));
    }
    let mut pt = POINT::default();
    // SAFETY: `pt` is writable; the drop handle is valid until DragFinish.
    unsafe { DragQueryPoint(drop, &raw mut pt) };
    (files, (pt.x, pt.y))
}

/// The most paths of one drop that are looked at (the core pins at most `paste::MAX_FILES`).
const MAX_DROPPED: u32 = 256;

fn origin(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT::default();
    // SAFETY: our window; `r` is writable.
    unsafe { GetWindowRect(hwnd, &raw mut r) };
    (r.left, r.top)
}

/// A border drag moved to the screen point `at`: the core resizes from the drag's start.
fn drag_to(hwnd: HWND, at: (i32, i32)) {
    if let (Some(id), Some(d)) = (pin_of(hwnd), slot(hwnd).and_then(|s| s.drag)) {
        let delta = (at.0.saturating_sub(d.from.0), at.1.saturating_sub(d.from.1));
        push(PinEvent::Resize(id, d.edge, d.start, delta));
    }
}

fn end_drag(hwnd: HWND) {
    let mut release = false;
    with_slot(hwnd, |s| {
        release = s.drag.is_some_and(|d| d.captured);
        s.drag = None;
    });
    if release {
        // SAFETY: gives back the capture this window took for the drag.
        unsafe { ReleaseCapture() };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // The border band resizes, the rest moves (drag anywhere).
        WM_NCHITTEST => {
            let edge = slot(hwnd).and_then(|s| {
                let (out, inside) = s.view.band();
                transform::edge_at(s.view.rect, out, inside, point_of(lp))
            });
            return edge.map_or(HTCAPTION, ht_of) as LRESULT;
        }
        WM_SETCURSOR => {
            let cursor = match (lp & 0xFFFF) as u32 {
                HTLEFT | HTRIGHT => IDC_SIZEWE,
                HTTOP | HTBOTTOM => IDC_SIZENS,
                HTTOPLEFT | HTBOTTOMRIGHT => IDC_SIZENWSE,
                HTTOPRIGHT | HTBOTTOMLEFT => IDC_SIZENESW,
                _ => std::ptr::null(),
            };
            if !cursor.is_null() {
                // SAFETY: a system cursor, shown while the pointer is over the border.
                unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), cursor)) };
                return 1;
            }
        }
        // ADR 0094: during the guard a press is not eaten (a drag starts at once) and does not
        // activate the pin (it already has the focus it was created with).
        WM_MOUSEACTIVATE => {
            if guarded(hwnd) {
                return MA_NOACTIVATE as LRESULT;
            }
            // SAFETY: plain query; the window that has focus before this click activates us.
            let before = unsafe { GetForegroundWindow() };
            if before != hwnd {
                let prev = origin_of(before);
                with_slot(hwnd, |s| s.prev = prev);
            }
            return MA_ACTIVATE as LRESULT;
        }
        WM_ACTIVATE => {
            if (wp & 0xFFFF) as u32 != WA_INACTIVE {
                let from = lp as HWND;
                with_slot(hwnd, |s| s.activations += 1);
                // The window being deactivated (null when it belongs to another thread).
                if !from.is_null() && from != hwnd {
                    let prev = origin_of(from);
                    with_slot(hwnd, |s| s.prev = prev);
                }
            }
        }
        // The rest of the creating gesture: its release, the other buttons and a double-click
        // never act on the new pin. A left press (below) moves it at once.
        WM_NCRBUTTONDOWN | WM_NCMBUTTONDOWN | WM_NCLBUTTONUP | WM_NCRBUTTONUP | WM_LBUTTONDOWN
        | WM_RBUTTONDOWN | WM_MBUTTONDOWN
            if guarded(hwnd) =>
        {
            return 0;
        }
        // A double-click during the guard is the third and fourth press of a quick click series:
        // a plain press (a drag may start), never a close or a thumbnail.
        WM_NCLBUTTONDBLCLK if guarded(hwnd) => {
            // SAFETY: the default handling of our own window's press (the move loop).
            return unsafe { DefWindowProcW(hwnd, WM_NCLBUTTONDOWN, wp, lp) };
        }
        // #99 (`PIN-20`): `Ctrl`+press on the picture: the system move loop runs as usual (a
        // `Ctrl`+drag onto another pin still joins its group); when it ends with the pin where it
        // was, it was a `Ctrl`+click, which selects the pin. (A click without movement never
        // sends `WM_EXITSIZEMOVE`, so this is the only place that sees it.)
        WM_NCLBUTTONDOWN if wp as u32 == HTCAPTION && down(VK_CONTROL) => {
            let before = origin(hwnd);
            // SAFETY: the default handling of our own window's message (the modal move loop).
            let r = unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            if origin(hwnd) == before
                && let Some(id) = pin_of(hwnd)
            {
                push(PinEvent::CtrlClick(id));
            }
            return r;
        }
        // A press on the border starts a resize (this window's own capture and mouse messages).
        WM_NCLBUTTONDOWN if edge_of(wp as u32).is_some() => {
            let (Some(edge), Some(s)) = (edge_of(wp as u32), slot(hwnd)) else {
                return 0;
            };
            let captured = s.capture && s.view.resizable;
            with_slot(hwnd, |slot| {
                slot.drag = Some(Drag {
                    edge,
                    start: slot.view.rect,
                    from: point_of(lp),
                    captured,
                });
            });
            if captured {
                // SAFETY: our window takes the mouse until the button is released.
                unsafe { SetCapture(hwnd) };
            }
            return 0;
        }
        WM_MOUSEMOVE | WM_LBUTTONUP if slot(hwnd).is_some_and(|s| s.drag.is_some()) => {
            let (cx, cy) = point_of(lp);
            let (ox, oy) = origin(hwnd);
            drag_to(hwnd, (ox.saturating_add(cx), oy.saturating_add(cy)));
            if msg == WM_LBUTTONUP {
                end_drag(hwnd);
            }
            return 0;
        }
        WM_CAPTURECHANGED if lp as HWND != hwnd => {
            with_slot(hwnd, |s| {
                s.drag = None;
                s.right_from = None;
            });
        }
        // `Shift`+double-click toggles thumbnail mode; a plain double-click asks the core to
        // close the pin (ADR 0094, `[pins] double_click`); never maximize.
        WM_NCLBUTTONDBLCLK => {
            if let Some(id) = pin_of(hwnd)
                && !down(VK_CONTROL)
                && !down(VK_MENU)
            {
                push(if down(VK_SHIFT) {
                    PinEvent::ShiftDoubleClick(id)
                } else {
                    PinEvent::DoubleClick(id)
                });
            }
            return 0;
        }
        WM_NCRBUTTONDBLCLK | WM_NCMBUTTONDBLCLK => return 0,
        // #99 (`PIN-08`): a right-press on the picture: a drag beyond the system's drag
        // rectangle makes a thumbnail of the swept size, a click opens the menu as before.
        WM_NCRBUTTONDOWN if wp as u32 == HTCAPTION && slot(hwnd).is_some() => {
            let captured = slot(hwnd).is_some_and(|s| s.capture);
            with_slot(hwnd, |s| s.right_from = Some(point_of(lp)));
            if captured {
                // SAFETY: our window takes the mouse until the button is released.
                unsafe { SetCapture(hwnd) };
            }
            return 0;
        }
        WM_RBUTTONUP if slot(hwnd).is_some_and(|s| s.right_from.is_some()) => {
            let from = slot(hwnd).and_then(|s| s.right_from);
            with_slot(hwnd, |s| s.right_from = None);
            // SAFETY: we hold the capture taken on the press.
            unsafe { ReleaseCapture() };
            let (cx, cy) = point_of(lp);
            let (ox, oy) = origin(hwnd);
            let at = (ox.saturating_add(cx), oy.saturating_add(cy));
            if let (Some(id), Some((fx, fy))) = (pin_of(hwnd), from) {
                let (dx, dy) = (at.0.abs_diff(fx), at.1.abs_diff(fy));
                // SAFETY: plain metric queries.
                let (tx, ty) =
                    unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
                let beyond = dx > tx.unsigned_abs() / 2 || dy > ty.unsigned_abs() / 2;
                push(if beyond {
                    PinEvent::RightDrag(id, (dx.max(1), dy.max(1)))
                } else {
                    PinEvent::Menu(id, Some(at))
                });
            }
            return 0;
        }
        WM_NCMBUTTONUP => {
            if let Some(id) = pin_of(hwnd)
                && !guarded(hwnd)
            {
                push(PinEvent::MiddleClick(id));
            }
            return 0;
        }
        WM_CONTEXTMENU => {
            if let Some(id) = pin_of(hwnd)
                && !guarded(hwnd)
            {
                // -1 (both words 0xFFFF) means the keyboard asked (the menu key).
                let at = (lp as u32 != u32::MAX).then(|| point_of(lp));
                push(PinEvent::Menu(id, at));
            }
            return 0;
        }
        WM_MOUSEWHEEL => {
            if let Some(id) = pin_of(hwnd) {
                let delta = i32::from(((wp >> 16) & 0xFFFF) as u16 as i16);
                let ctrl = (wp & 0xFFFF) as u32 & MK_CONTROL != 0;
                push(PinEvent::Wheel(id, delta, point_of(lp), ctrl));
            }
            return 0;
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = u32::try_from(wp).unwrap_or(0);
            let repeat = (lp >> 30) & 1 == 1;
            let (ctrl, alt, shift) = (down(VK_CONTROL), down(VK_MENU), down(VK_SHIFT));
            if let (Some(id), Some(name)) = (pin_of(hwnd), vk_name(vk)) {
                // Shift+F10 opens the context menu (UX spec §5).
                if name == "F10" && shift && !ctrl && !alt {
                    if !repeat {
                        push(PinEvent::Menu(id, None));
                    }
                    return 0;
                }
                // Alt+F4 stays the system's (reserved `window.close`): it arrives as WM_CLOSE.
                if let Some(chord) = chord_of(&name, ctrl, alt, shift)
                    && !(msg == WM_SYSKEYDOWN && name == "F4")
                {
                    push(PinEvent::Key(id, chord, repeat));
                    return 0;
                }
            }
        }
        // #99 (`PIN-17`): with `Shift` held a moved pin snaps to nearby window and pin edges.
        WM_ENTERSIZEMOVE => {
            let targets = snap_targets(hwnd);
            SNAP_TO.with(|s| *s.borrow_mut() = targets);
        }
        WM_MOVING if down(VK_SHIFT) => {
            // SAFETY: for WM_MOVING `lp` points to the proposed window RECT, writable.
            let r = unsafe { &mut *(lp as *mut RECT) };
            let m = i32::try_from(slot(hwnd).map_or(0, |s| s.view.margin())).unwrap_or(0);
            let pic = Rect::new(
                r.left + m,
                r.top + m,
                u32::try_from(r.right - r.left - 2 * m).unwrap_or(1),
                u32::try_from(r.bottom - r.top - 2 * m).unwrap_or(1),
            );
            let (dx, dy) = SNAP_TO.with(|s| transform::snap(pic, &s.borrow(), SNAP_PX));
            r.left += dx;
            r.right += dx;
            r.top += dy;
            r.bottom += dy;
            return 1;
        }
        WM_EXITSIZEMOVE => {
            SNAP_TO.with(|s| s.borrow_mut().clear());
            let (x, y) = origin(hwnd);
            let m = slot(hwnd).map_or(0, |s| s.view.margin());
            let m = i32::try_from(m).unwrap_or(0);
            if let Some(id) = pin_of(hwnd) {
                let (x, y) = (x.saturating_add(m), y.saturating_add(m));
                push(if down(VK_CONTROL) {
                    PinEvent::Dropped(id, x, y)
                } else {
                    PinEvent::Moved(id, x, y)
                });
            }
            return 0;
        }
        // One image pixel per device pixel on every monitor: the suggested size is ignored, and
        // the core redraws the outline for the new scale once the displays are quiet (Step 58).
        WM_TIMER if wp == BADGE_TIMER => {
            hide_badge(hwnd);
            return 0;
        }
        WM_DPICHANGED => {
            if let Some(id) = pin_of(hwnd) {
                push(PinEvent::DpiChanged(id));
            }
            return 0;
        }
        WM_CLOSE => {
            // Alt+F4 or the system: the core closes the pin (and destroys this window).
            if let Some(id) = pin_of(hwnd) {
                push(PinEvent::Closed(id));
            }
            return 0;
        }
        // Files dropped on the pin (Step 55, NOEUL-FR-073): their paths and the drop point; the
        // core pins the image files among them. The drop's memory is freed here.
        WM_DROPFILES => {
            let drop = wp as HDROP;
            let (files, at) = dropped_files(drop);
            // SAFETY: the HDROP of this message, freed once (the sender gave it to us).
            unsafe { DragFinish(drop) };
            if let Some(id) = pin_of(hwnd)
                && !files.is_empty()
            {
                let (ox, oy) = origin(hwnd);
                push(PinEvent::Files(
                    id,
                    files,
                    (ox.saturating_add(at.0), oy.saturating_add(at.1)),
                ));
            }
            return 0;
        }
        WM_DESTROY => return 0,
        WM_NCDESTROY => {
            SLOTS.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    s.remove(&(hwnd as isize));
                }
            });
        }
        _ => {}
    }
    // SAFETY: default processing of a message of our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn class() -> Result<*const u16, String> {
    static ATOM: OnceLock<Result<usize, String>> = OnceLock::new();
    let name = ATOM.get_or_init(|| {
        let name = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).expect("small"),
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            // SAFETY: the module handle of this executable.
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            // SAFETY: a system cursor.
            hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_SIZEALL) },
            lpszClassName: name.as_ptr(),
            ..Default::default()
        };
        // SAFETY: `wc` is fully initialized; the class name is copied by the system.
        if unsafe { RegisterClassExW(&raw const wc) } == 0 {
            return Err(format!(
                "RegisterClassExW failed ({})",
                std::io::Error::last_os_error()
            ));
        }
        // Keep the name alive for CreateWindowExW (leaked once per process).
        Ok(Box::leak(name.into_boxed_slice()).as_ptr() as usize)
    });
    name.clone().map(|p| p as *const u16)
}

/// A temporary premultiplied BGRA DIB section of one window size.
pub(crate) struct Surface {
    pub(crate) dc: HDC,
    bitmap: HBITMAP,
    old: *mut c_void,
    bits: *mut u8,
    len: usize,
}

impl Surface {
    pub(crate) fn new(w: u32, h: u32) -> Result<Surface, String> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).expect("small"),
                biWidth: i32::try_from(w).map_err(|_| "too wide")?,
                biHeight: -i32::try_from(h).map_err(|_| "too high")?,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: a memory DC compatible with the screen; deleted in Drop.
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if dc.is_null() {
            return Err("CreateCompatibleDC failed".into());
        }
        let mut bits: *mut c_void = std::ptr::null_mut();
        // SAFETY: `info` describes a top-down 32-bit DIB of `w`×`h`.
        let bitmap = unsafe {
            CreateDIBSection(
                dc,
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                std::ptr::null_mut(),
                0,
            )
        };
        if bitmap.is_null() || bits.is_null() {
            // SAFETY: the DC created above.
            unsafe { DeleteDC(dc) };
            return Err("CreateDIBSection failed (out of memory?)".into());
        }
        // SAFETY: selects our bitmap; restored in Drop.
        let old = unsafe { SelectObject(dc, bitmap) };
        Ok(Surface {
            dc,
            bitmap,
            old,
            bits: bits.cast::<u8>(),
            len: usize::try_from(u64::from(w) * u64::from(h) * 4).map_err(|_| "too large")?,
        })
    }

    pub(crate) fn pixels(&mut self) -> &mut [u8] {
        // SAFETY: the DIB holds `len` bytes at `bits` until Drop; `&mut self` makes this the
        // only view.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: objects created in `new`, released once in reverse order.
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

fn blend(opacity: u32) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: transform::alpha(opacity),
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

/// Lets the mouse through the window or not (`WS_EX_TRANSPARENT`, NOEUL-FR-063).
fn set_transparent(hwnd: HWND, on: bool) {
    // SAFETY: reads and writes our own window's extended style.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let want = if on {
            ex | WS_EX_TRANSPARENT
        } else {
            ex & !WS_EX_TRANSPARENT
        };
        if want != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want as isize);
        }
    }
}

/// Paints `frame` at `view`'s size with its outline, shadow and badge and hands it to the layered
/// window, which moves and resizes to the view's window (no activation, no z-order change), with
/// its opacity and click-through. The temporary surface is freed on return.
fn upload(hwnd: HWND, frame: &Frame, view: &View) -> Result<(), String> {
    let win = view.window();
    if !zoom::allowed(view.rect.width, view.rect.height)
        || u64::from(win.width) * u64::from(win.height) * 4 > zoom::MAX_BYTES + (64 << 20)
    {
        return Err(format!(
            "a {}×{} pin window is too large",
            view.rect.width, view.rect.height
        ));
    }
    let mut surface = Surface::new(win.width, win.height)?;
    transform::paint(frame, view, surface.pixels());
    // ADR 0094: after the zoom changed (not on the first present), the read-out shows for
    // `HINT_MS`; it stays through other changes while its timer runs, never at 100 %.
    let before = slot(hwnd).map(|s| (s.view.zoom, s.presented, s.badge));
    let changed = matches!(before, Some((z, true, _)) if z != view.zoom);
    let showing = before.is_some_and(|b| b.2);
    let mut clean = None;
    if let Some(text) = transform::zoom_label(view.zoom).filter(|_| changed || showing) {
        let size = (12 * view.dpi).div_ceil(crate::capture::BASE_DPI).max(9);
        let glyphs = TYPE.with(|t| {
            use crate::annotation::Typesetter;
            t.borrow_mut()
                .get_or_insert_with(crate::win::typeset::GdiType::new)
                .typeset(&text, size)
        });
        let plain = surface.pixels().to_vec();
        if transform::paint_zoom_badge(surface.pixels(), view, &glyphs) {
            clean = Some(plain);
        }
    }
    let blend = blend(view.opacity);
    let pos = POINT { x: win.x, y: win.y };
    let size = SIZE {
        cx: i32::try_from(win.width).map_err(|_| "too wide")?,
        cy: i32::try_from(win.height).map_err(|_| "too high")?,
    };
    let src = POINT { x: 0, y: 0 };
    // SAFETY: the screen DC is released right after; the surface DC holds the premultiplied
    // picture of exactly `size`.
    let ok = unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let ok = UpdateLayeredWindow(
            hwnd,
            screen,
            &raw const pos,
            &raw const size,
            surface.dc,
            &raw const src,
            0,
            &raw const blend,
            ULW_ALPHA,
        );
        ReleaseDC(std::ptr::null_mut(), screen);
        ok
    };
    if ok == 0 {
        return Err(format!(
            "UpdateLayeredWindow failed ({})",
            std::io::Error::last_os_error()
        ));
    }
    set_transparent(hwnd, view.click_through);
    let badge = clean.is_some();
    BADGES.with(|b| match clean {
        Some(c) => {
            b.borrow_mut().insert(hwnd as isize, c);
        }
        None => {
            b.borrow_mut().remove(&(hwnd as isize));
        }
    });
    with_slot(hwnd, |s| {
        s.view = *view;
        s.presented = true;
        s.badge = badge;
        if !view.resizable {
            s.drag = None;
        }
    });
    let ms = kim_design::motion::HINT_MS;
    // SAFETY: the window's own one-shot timer; (re)armed on a zoom change, killed otherwise.
    unsafe {
        if badge && changed {
            SetTimer(hwnd, BADGE_TIMER, ms, None);
        } else if !badge {
            KillTimer(hwnd, BADGE_TIMER);
        }
    }
    Ok(())
}

/// ADR 0094: the zoom read-out's time is up: the surface without it goes back on screen.
fn hide_badge(hwnd: HWND) {
    // SAFETY: the window's own timer.
    unsafe { KillTimer(hwnd, BADGE_TIMER) };
    let Some(view) = slot(hwnd).map(|s| s.view) else {
        return;
    };
    let Some(pixels) = BADGES.with(|b| b.borrow_mut().remove(&(hwnd as isize))) else {
        return;
    };
    let win = view.window();
    let Ok(mut surface) = Surface::new(win.width, win.height) else {
        return;
    };
    if surface.pixels().len() != pixels.len() {
        return;
    }
    surface.pixels().copy_from_slice(&pixels);
    let blend = blend(view.opacity);
    let pos = POINT { x: win.x, y: win.y };
    let size = SIZE {
        cx: win.width as i32,
        cy: win.height as i32,
    };
    let src = POINT { x: 0, y: 0 };
    // SAFETY: as in `upload`: the screen DC is released right after.
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        UpdateLayeredWindow(
            hwnd,
            screen,
            &raw const pos,
            &raw const size,
            surface.dc,
            &raw const src,
            0,
            &raw const blend,
            ULW_ALPHA,
        );
        ReleaseDC(std::ptr::null_mut(), screen);
    }
    with_slot(hwnd, |s| s.badge = false);
}

/// ADR 0094 (tests): whether the zoom read-out shows on this pin window now.
pub fn badge_shown(hwnd: HWND) -> bool {
    slot(hwnd).is_some_and(|s| s.badge)
}

/// How pin windows sit on the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Above every normal window (`WS_EX_TOPMOST`).
    pub topmost: bool,
    /// May be activated (by the user's click or creation); `false` adds `WS_EX_NOACTIVATE`.
    pub activate: bool,
}

impl Style {
    /// Real pins.
    pub const INTERACTIVE: Style = Style {
        topmost: true,
        activate: true,
    };
    /// The debug-build test hook (ADR 0026 §8): the windows lie off-screen, never on top, never
    /// activated, and never take the mouse capture.
    pub const OFF_SCREEN_TEST: Style = Style {
        topmost: false,
        activate: false,
    };
}

/// One live pin window.
pub struct WinPin {
    hwnd: HWND,
    style: Style,
    /// #99: the user put this pin in the normal z-order (only for topmost-capable pins).
    below: bool,
}

impl std::fmt::Debug for WinPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinPin")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl WinPin {
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    fn margin(&self) -> i32 {
        i32::try_from(slot(self.hwnd).map_or(0, |s| s.view.margin())).unwrap_or(0)
    }

    /// When this pin has focus, gives it back to the window the user was in before (if that
    /// still exists and is visible), so hiding or closing a pin never moves focus to another pin.
    fn hand_back_focus(&self) {
        // SAFETY: plain queries and an activation this process may make while it has focus.
        unsafe {
            if self.hwnd.is_null() || GetForegroundWindow() != self.hwnd {
                return;
            }
            let prev = slot(self.hwnd).map_or(0, |s| s.prev) as HWND;
            if !prev.is_null() && IsWindow(prev) != 0 && IsWindowVisible(prev) != 0 && !is_pin(prev)
            {
                SetForegroundWindow(prev);
            }
        }
    }
}

impl PinWindow for WinPin {
    fn set_position(&mut self, x: i32, y: i32) {
        let m = self.margin();
        with_slot(self.hwnd, |s| {
            s.view.rect.x = x;
            s.view.rect.y = y;
        });
        // SAFETY: our window; moves it without activating, resizing or changing the z-order.
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                x.saturating_sub(m),
                y.saturating_sub(m),
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    fn present(&mut self, frame: &Frame, view: &View) -> Result<(), String> {
        upload(self.hwnd, frame, view)
    }

    fn set_opacity(&mut self, opacity: u32) {
        let blend = blend(opacity);
        // SAFETY: our layered window keeps its surface, position and size; only the constant
        // alpha changes (no source DC: nothing is painted again).
        let ok = unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &raw const blend,
                ULW_ALPHA,
            )
        } != 0;
        if ok {
            with_slot(self.hwnd, |s| s.view.opacity = opacity);
        }
    }

    fn set_topmost(&mut self, on: bool) {
        // Test windows (never topmost) stay as they are.
        if !self.style.topmost {
            return;
        }
        self.below = !on;
        let z = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: our window; only its z-order band changes, nothing is activated.
        unsafe {
            SetWindowPos(
                self.hwnd,
                z,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
    }

    fn set_visible(&mut self, visible: bool) {
        if visible {
            let top = if self.style.topmost && !self.below {
                HWND_TOPMOST
            } else {
                HWND_TOP
            };
            // SAFETY: our window; shown at the top of its band without activation.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    top,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            };
        } else {
            self.hand_back_focus();
            end_drag(self.hwnd);
            // SAFETY: our window; it is no longer the active one, so hiding moves no focus.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    fn focus(&mut self) -> bool {
        if !self.style.activate {
            return false;
        }
        // SAFETY: plain query, then our window; the OS may refuse (foreground lock), which is
        // reported.
        unsafe {
            let before = GetForegroundWindow();
            if before != self.hwnd {
                let prev = origin_of(before);
                with_slot(self.hwnd, |s| s.prev = prev);
            }
            SetForegroundWindow(self.hwnd) != 0
        }
    }

    fn rect(&self) -> Option<Rect> {
        let mut r = RECT::default();
        // SAFETY: our window; `r` is writable.
        if self.hwnd.is_null() || unsafe { GetWindowRect(self.hwnd, &raw mut r) } == 0 {
            return None;
        }
        let m = self.margin();
        let w = u32::try_from(r.right - r.left - 2 * m).ok()?;
        let h = u32::try_from(r.bottom - r.top - 2 * m).ok()?;
        Some(Rect::new(r.left + m, r.top + m, w, h))
    }

    fn destroy(&mut self) {
        self.hand_back_focus();
        end_drag(self.hwnd);
        // SAFETY: destroys our own window once; later calls see a dead handle and do nothing.
        unsafe {
            if !self.hwnd.is_null() && IsWindow(self.hwnd) != 0 {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(self.hwnd);
            }
        }
        SLOTS.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                s.remove(&(self.hwnd as isize));
            }
        });
        self.hwnd = std::ptr::null_mut();
    }
}

impl Drop for WinPin {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// Creates Win32 pin windows on the calling (UI) thread.
#[derive(Debug, Clone, Copy)]
pub struct WinPins {
    pub style: Style,
}

/// Creates one pin window showing `frame` as `spec` says. Exposed for tests and measurements,
/// which place it off-screen.
pub fn create(id: PinId, frame: &Frame, spec: &WindowSpec, style: Style) -> Result<WinPin, String> {
    let class = class()?;
    let view = spec.view;
    let r = view.window();
    if !zoom::allowed(view.rect.width, view.rect.height) {
        return Err(format!(
            "a {}×{} pin window is too large",
            view.rect.width, view.rect.height
        ));
    }
    let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW;
    if style.topmost {
        ex |= WS_EX_TOPMOST;
    }
    if !style.activate {
        ex |= WS_EX_NOACTIVATE;
    }
    let title = wide(format!("Noeul {id}"));
    // SAFETY: registered class; plain values; no parent, menu or creation data.
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class,
            title.as_ptr(),
            WS_POPUP,
            r.x,
            r.y,
            i32::try_from(r.width).map_err(|_| "too wide")?,
            i32::try_from(r.height).map_err(|_| "too high")?,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(format!(
            "CreateWindowExW failed ({})",
            std::io::Error::last_os_error()
        ));
    }
    SLOTS.with(|s| {
        s.borrow_mut().insert(
            hwnd as isize,
            Slot {
                id,
                prev: 0,
                guard_until: spec.guard.map(|g| Instant::now() + g),
                activations: 0,
                view,
                drag: None,
                right_from: None,
                capture: style.activate,
                presented: false,
                badge: false,
            },
        );
    });
    let mut pin = WinPin {
        hwnd,
        style,
        below: false,
    };
    // SAFETY: our window; the id is read back by the window procedure.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, isize::try_from(id.0).unwrap_or(0));
        // No input method context: letter keys are shortcuts, also in Hangul mode.
        ImmAssociateContextEx(hwnd, std::ptr::null_mut(), 0);
        // Image files can be dropped on a pin (Step 55, NOEUL-FR-073).
        DragAcceptFiles(hwnd, 1);
    }
    if let Err(e) = upload(hwnd, frame, &view) {
        pin.destroy();
        return Err(e);
    }
    let focus = spec.focus && style.activate;
    // SAFETY: shows our window; activation only when the user created it.
    unsafe {
        ShowWindow(hwnd, if focus { SW_SHOW } else { SW_SHOWNOACTIVATE });
    }
    if focus {
        pin.focus();
    }
    Ok(pin)
}

impl PinWindows for WinPins {
    fn create(
        &mut self,
        id: PinId,
        frame: &Frame,
        spec: &WindowSpec,
    ) -> Result<Box<dyn PinWindow>, String> {
        create(id, frame, spec, self.style).map(|p| Box::new(p) as Box<dyn PinWindow>)
    }
}

/// Shows the context menu of pin `id` (modal; call it while the instance core is not borrowed)
/// at `at`, or at the pin's top-left corner when the keyboard asked, and returns the chosen
/// action. `None` when the menu was dismissed or the pin is gone.
pub fn track_menu(
    id: PinId,
    items: &[PinMenuItem],
    at: Option<(i32, i32)>,
) -> Option<&'static str> {
    track_menu_on(hwnd_of(id)?, items, at)
}

/// Shows a popup menu owned by `hwnd`, one of Noeul's windows (a pin, the capture overlay), at
/// `at` or near the window's top-left corner, and returns the chosen action (modal; call it
/// while the instance core is not borrowed).
pub(crate) fn track_menu_on(
    hwnd: HWND,
    items: &[PinMenuItem],
    at: Option<(i32, i32)>,
) -> Option<&'static str> {
    // SAFETY: a liveness check on a handle the caller got from one of our windows.
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return None;
    }
    // SAFETY: a new popup menu, destroyed below.
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return None;
    }
    for (i, item) in items.iter().enumerate() {
        if item.action.is_none() {
            // SAFETY: valid menu handle.
            unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null()) };
            continue;
        }
        let label = wide(&item.label);
        let mut flags = if item.enabled {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        if item.checked {
            flags |= windows_sys::Win32::UI::WindowsAndMessaging::MF_CHECKED;
        }
        // SAFETY: valid menu; the label is copied.
        unsafe { AppendMenuW(menu, flags, i + 1, label.as_ptr()) };
    }
    let (x, y) = at.unwrap_or_else(|| {
        let mut r = RECT::default();
        // SAFETY: our window; `r` is writable.
        unsafe { GetWindowRect(hwnd, &raw mut r) };
        (r.left.saturating_add(8), r.top.saturating_add(8))
    });
    // SAFETY: the documented popup-menu pattern: the owner is foreground while the menu is open
    // (the pin already is after the click or key that asked), and a WM_NULL afterwards lets the
    // menu close correctly.
    let cmd = unsafe {
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            x,
            y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd
    };
    let index = usize::try_from(cmd).ok()?.checked_sub(1)?;
    items.get(index).and_then(|i| i.action)
}

#[cfg(test)]
mod tests;
