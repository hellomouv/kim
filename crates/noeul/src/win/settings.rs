//! The settings window (Step 61, ADR 0075; NOEUL-FR-101, NOEUL-FR-103, NOEUL-NFR-012, NOEUL-NFR-013).
//!
//! Standard Win32 controls — a list box of the actions with their keys, push buttons, combo
//! boxes, check boxes, a read-only edit field that records keys and a status line — so the
//! keyboard (`Tab`, arrows, `Alt`+mnemonics through `IsDialogMessage`), screen readers (each
//! control has its text as its accessible name), Windows High Contrast (system colours) and the
//! user's text size work as everywhere in Windows. The system message font at the monitor's DPI;
//! the layout scales with the DPI and follows `WM_DPICHANGED`. In dark mode (and not High
//! Contrast) the title bar and the controls are dark.
//!
//! Every change runs the documented `noeul` command on a worker thread ([`crate::prefs`]): the
//! command may talk to this very instance over IPC (`hotkeys.reload`), which must not wait for a
//! UI thread that waits for it. While keys are recorded, [`recording`] is true and the instance
//! ignores its global hotkeys, so `F1` or `F3` can be recorded without capturing or pasting.
//!
//! The window exists only while the user has it open (no hidden window, no timer). Under the
//! debug-build test hook it lies far off-screen and is never activated; tests post messages to it
//! (`WM_COMMAND` for the buttons, [`WM_TEST_KEY`] for a recorded chord) and read it back with
//! [`test_state`].

use crate::prefs::{GroupAction, Key, Model, Toggle};
use serde_json::Value;
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontIndirectW, CreateSolidBrush, DeleteObject, FillRect, GetSysColorBrush,
    HBRUSH, HDC, HFONT, SetBkColor, SetTextColor,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, SetWindowTheme};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL,
    CB_SETCURSEL, CBS_DROPDOWNLIST, CallWindowProcW, CreateWindowExW, DLGC_WANTALLKEYS,
    DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, ES_READONLY, GWLP_WNDPROC, GetClientRect,
    GetDlgItem, IsDialogMessageW, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
    LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, MSG, NONCLIENTMETRICSW, PostMessageW, RegisterClassExW,
    SPI_GETNONCLIENTMETRICS, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETDLGCODE, WM_KEYDOWN,
    WM_SETFONT, WM_SETTINGCHANGE, WM_SYSCHAR, WM_SYSCOLORCHANGE, WM_SYSKEYDOWN, WM_THEMECHANGED,
    WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

const CLASS: &str = "NoeulSettings";
/// A command finished on the worker thread.
const WM_DONE: u32 = WM_APP + 0x60;
/// Test hook only: a recorded chord — `wParam` the virtual key, `lParam` bit 0 Ctrl, bit 1 Alt,
/// bit 2 Shift (posted messages carry no key state).
pub const WM_TEST_KEY: u32 = WM_APP + 0x61;

/// Control IDs (tests post `WM_COMMAND` with them).
pub mod id {
    pub const LIST: u16 = 100;
    pub const CHANGE: u16 = 101;
    pub const DISABLE: u16 = 102;
    pub const RESET: u16 = 103;
    pub const RESET_ALL: u16 = 104;
    pub const IMPORT: u16 = 105;
    pub const EXPORT: u16 = 106;
    pub const FORCE: u16 = 107;
    pub const RECORDER: u16 = 108;
    pub const DOUBLE: u16 = 110;
    pub const MIDDLE: u16 = 111;
    pub const TRAY: u16 = 120;
    pub const HISTORY_DISK: u16 = 121;
    pub const PIN_BACKUP: u16 = 122;
    pub const STATUS: u16 = 130;
    pub const CLOSE: u16 = 131;
    pub const LABEL_KEYS: u16 = 140;
    pub const LABEL_DOUBLE: u16 = 141;
    pub const LABEL_MIDDLE: u16 = 142;
    /// #99 (`PIN-15`): the pin groups of the running instance.
    pub const LABEL_GROUPS: u16 = 143;
    pub const GROUPS: u16 = 150;
    pub const GROUP_SWITCH: u16 = 151;
    pub const GROUP_SHOW: u16 = 152;
    pub const GROUP_HIDE: u16 = 153;
    pub const GROUP_CLOSE: u16 = 154;
}

/// #99 (`PIN-15`): the instance's pipe name when it is not the default one (`--endpoint`), so
/// the group commands reach this instance.
static ENDPOINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The instance tells the window which pipe it answers on (its name under `\\.\pipe\`).
pub fn set_endpoint(pipe_path: &str) {
    let name = pipe_path.rsplit('\\').next().unwrap_or(pipe_path);
    let _ = ENDPOINT.set(name.to_string());
}

/// Arguments for a command that talks to the running instance.
fn to_instance(mut args: Vec<String>) -> Vec<String> {
    if let Some(e) = ENDPOINT.get() {
        args.push("--endpoint".into());
        args.push(e.clone());
    }
    args
}

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Whether the window records keys now (the instance then ignores its global hotkeys).
pub fn recording() -> bool {
    RECORDING.load(Ordering::SeqCst)
}

/// The answer of the command running on the worker thread: its arguments and its `--json` reply.
type Done = Arc<Mutex<Option<(Vec<String>, Value)>>>;

struct State {
    hwnd: HWND,
    model: Model,
    interactive: bool,
    font: HFONT,
    dark: Option<HBRUSH>,
    /// A command runs (input waits; the status says so).
    busy: bool,
    /// `Reset all…` was pressed once: the next press confirms.
    confirm_reset: bool,
    done: Done,
    recorder_proc: isize,
    /// ADR 0091: the window is drawn this many percent larger on a large screen (100 in tests).
    zoom: i32,
}

/// ADR 0091: the zoom in percent for a work area of `w` × `h` pixels at `dpi`: as large as keeps
/// the 600 × 740 window within 90 % of it, in steps of 25 %, between 100 % and 200 %.
pub fn zoom_for(w: i32, h: i32, dpi: i32) -> i32 {
    let (bw, bh) = (600 * dpi / 96, 740 * dpi / 96);
    let fit = (w as f32 * 0.9 / bw as f32).min(h as f32 * 0.9 / bh as f32);
    (((fit * 4.0).floor() * 25.0) as i32).clamp(100, 200)
}

/// The zoom for the monitor `hwnd` is on.
fn screen_zoom(hwnd: HWND) -> i32 {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
    };
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        // SAFETY: plain data.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: our own window; a correctly sized out-structure.
    let ok = unsafe {
        let m = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        GetMonitorInfoW(m, &raw mut mi)
    };
    if ok == 0 {
        return 100;
    }
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let w = mi.rcWork;
    zoom_for(w.right - w.left, w.bottom - w.top, dpi)
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn item(hwnd: HWND, id: u16) -> HWND {
    // SAFETY: a child of our own window.
    unsafe { GetDlgItem(hwnd, i32::from(id)) }
}

fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    // SAFETY: NUL-terminated text; our own control.
    unsafe { SetWindowTextW(h, w.as_ptr()) };
}

/// `noeul <args> --json` with this process's environment (the same profile), no window.
fn run_noeul(args: &[String]) -> Value {
    let exe = std::env::current_exe().unwrap_or_else(|_| "noeul.exe".into());
    std::process::Command::new(exe)
        .args(args)
        .arg("--json")
        .env("KIM_NO_INPUT", "1")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_else(
            || serde_json::json!({"ok": false, "error": {"message": "noeul could not be run"}}),
        )
}

/// ADR 0093: the settings window is `noeul-settings.exe`, a program of its own next to
/// `noeul.exe` (the Kim design system in egui; Noeul links no GUI framework, ADR 0023). It
/// is started for the user's Settings (with this instance's `--endpoint` when it is not the
/// default); the window below remains for the test hook and when the program is missing.
pub fn open(interactive: bool) -> Result<(), String> {
    if interactive && let Some(exe) = companion() {
        let mut cmd = std::process::Command::new(exe);
        if let Some(e) = ENDPOINT.get() {
            cmd.args(["--endpoint", e]);
        }
        // CREATE_NO_WINDOW: no console; the program shows its own window and ends with it.
        cmd.creation_flags(0x0800_0000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // Let the new process bring its window to the front (the user asked for it).
        // SAFETY: a plain call; failure only means Windows keeps the focus where it is.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX) };
        return match cmd.spawn() {
            Ok(_) => Ok(()),
            Err(_) => open_window(interactive),
        };
    }
    open_window(interactive)
}

/// `noeul-settings.exe` next to this program, when it is there.
fn companion() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("noeul-settings.exe");
    p.is_file().then_some(p)
}

/// The built-in settings window (Step 61): the test hook's, and the fallback.
fn open_window(interactive: bool) -> Result<(), String> {
    if let Some(h) = STATE.with(|s| s.borrow().as_ref().map(|s| s.hwnd)) {
        if interactive {
            // SAFETY: our own window.
            unsafe { SetForegroundWindow(h) };
        }
        return Ok(());
    }
    let class = wide(CLASS);
    // SAFETY: registering our class (a second registration fails harmlessly) and creating our
    // window; strings outlive the calls.
    let hwnd = unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: inst,
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&raw const wc);
        let title = wide("Noeul settings");
        let (x, y) = if interactive {
            (200, 120)
        } else {
            (-20_000, -20_000)
        };
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            600,
            740,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(format!(
            "CreateWindowExW failed ({})",
            std::io::Error::last_os_error()
        ));
    }
    let dark = interactive && dark_mode();
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            hwnd,
            model: Model::default(),
            interactive,
            font: std::ptr::null_mut(),
            // SAFETY: a plain brush, deleted with the window.
            dark: dark.then(|| unsafe { CreateSolidBrush(0x0020_2020) }),
            busy: false,
            confirm_reset: false,
            done: Arc::new(Mutex::new(None)),
            recorder_proc: 0,
            zoom: if interactive { screen_zoom(hwnd) } else { 100 },
        });
    });
    build(hwnd, dark);
    layout(hwnd);
    // SAFETY: our own window.
    unsafe {
        ShowWindow(
            hwnd,
            if interactive {
                SW_SHOW
            } else {
                SW_SHOWNOACTIVATE
            },
        )
    };
    if interactive {
        // SAFETY: as above.
        unsafe { SetForegroundWindow(hwnd) };
    }
    reload(hwnd, "Loading…");
    Ok(())
}

/// Whether Windows apps use the dark mode and High Contrast is off.
pub fn dark_mode() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let (key, name) = (
        wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        wide("AppsUseLightTheme"),
    );
    let mut v: u32 = 1;
    let mut n = 4u32;
    // SAFETY: a DWORD out-parameter of the documented size.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut v).cast(),
            &raw mut n,
        )
    };
    wants_dark(true, !(r == 0 && v == 0), high_contrast())
}

pub fn high_contrast() -> bool {
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    // SAFETY: the documented structure with its size set.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, (&raw mut hc).cast(), 0) };
    ok != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
}

fn child(hwnd: HWND, class: &str, text: &str, style: u32, ex: u32, id: u16) -> HWND {
    let (c, t) = (wide(class), wide(text));
    // SAFETY: a child control of our window with its ID as the menu handle.
    unsafe {
        CreateWindowExW(
            ex,
            c.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            10,
            10,
            hwnd,
            usize::from(id) as _,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

fn build(hwnd: HWND, dark: bool) {
    use id::*;
    child(
        hwnd,
        "STATIC",
        "&Shortcuts (select one, then Change, Disable or Reset):",
        0,
        0,
        LABEL_KEYS,
    );
    child(
        hwnd,
        "LISTBOX",
        "Shortcuts",
        WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32,
        WS_EX_CLIENTEDGE,
        LIST,
    );
    for (i, t) in [
        (CHANGE, "C&hange…"),
        (DISABLE, "&Disable"),
        (RESET, "&Reset"),
        (RESET_ALL, "Reset &all…"),
        (IMPORT, "&Import…"),
        (EXPORT, "E&xport…"),
        (FORCE, "&Use anyway"),
    ] {
        child(hwnd, "BUTTON", t, WS_TABSTOP | BS_PUSHBUTTON as u32, 0, i);
    }
    let rec = child(
        hwnd,
        "EDIT",
        "",
        WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        RECORDER,
    );
    // The recorder takes every key while it records.
    // SAFETY: subclassing our own edit control; the old procedure is kept and called.
    let old = unsafe { SetWindowLongPtrW(rec, GWLP_WNDPROC, recorder_proc as *const () as isize) };
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.recorder_proc = old;
        }
    });
    child(
        hwnd,
        "STATIC",
        "Double-click &inside a selection:",
        0,
        0,
        LABEL_DOUBLE,
    );
    child(
        hwnd,
        "COMBOBOX",
        "",
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        0,
        DOUBLE,
    );
    child(
        hwnd,
        "STATIC",
        "&Middle-click inside a selection:",
        0,
        0,
        LABEL_MIDDLE,
    );
    child(
        hwnd,
        "COMBOBOX",
        "",
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        0,
        MIDDLE,
    );
    for (i, t) in [
        (TRAY, Toggle::TrayIcon),
        (HISTORY_DISK, Toggle::HistoryDisk),
        (PIN_BACKUP, Toggle::PinBackup),
    ] {
        child(
            hwnd,
            "BUTTON",
            t.label(),
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            i,
        );
    }
    child(
        hwnd,
        "STATIC",
        "Pin &groups (select one, then Switch, Show, Hide or Close its pins):",
        0,
        0,
        LABEL_GROUPS,
    );
    child(
        hwnd,
        "LISTBOX",
        "Pin groups",
        WS_TABSTOP | WS_VSCROLL | WS_BORDER | LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32,
        WS_EX_CLIENTEDGE,
        GROUPS,
    );
    for (i, t) in [
        (GROUP_SWITCH, "S&witch to"),
        (GROUP_SHOW, "Sh&ow pins"),
        (GROUP_HIDE, "Hi&de pins"),
        (GROUP_CLOSE, "Clo&se pins"),
    ] {
        child(hwnd, "BUTTON", t, WS_TABSTOP | BS_PUSHBUTTON as u32, 0, i);
    }
    child(hwnd, "STATIC", "Loading…", 0, 0, STATUS);
    child(
        hwnd,
        "BUTTON",
        "Close",
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        CLOSE,
    );
    let names = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    for (combo, values) in [
        (DOUBLE, names(&crate::prefs::DOUBLE_CLICK)),
        (MIDDLE, names(&crate::prefs::MIDDLE_CLICK)),
    ] {
        for v in values {
            let w = wide(match v.as_str() {
                "pin" => "Pin it (Noeul default)",
                "copy" => "Copy it",
                "save" => "Save it",
                _ => "Nothing",
            });
            // SAFETY: NUL-terminated text to our own combo box.
            unsafe { SendMessageW(item(hwnd, combo), CB_ADDSTRING, 0, w.as_ptr() as isize) };
        }
    }
    if dark {
        set_dark_controls(hwnd, true);
    }
}

/// The caption and the themed controls in dark mode, or back to the default look.
fn set_dark_controls(hwnd: HWND, dark: bool) {
    use id::*;
    let on: i32 = i32::from(dark);
    // SAFETY: DWMWA_USE_IMMERSIVE_DARK_MODE (20) with a BOOL of the documented size.
    unsafe { DwmSetWindowAttribute(hwnd, 20, (&raw const on).cast(), 4) };
    let theme = wide(if dark {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    });
    for i in [
        LIST, CHANGE, DISABLE, RESET, RESET_ALL, IMPORT, EXPORT, FORCE, DOUBLE, MIDDLE, CLOSE,
    ] {
        // SAFETY: our own control; a theme name.
        unsafe { SetWindowTheme(item(hwnd, i), theme.as_ptr(), std::ptr::null()) };
    }
}

/// Whether the window should be dark: an interactive window while Windows apps use the dark
/// mode and High Contrast is off.
pub fn wants_dark(interactive: bool, apps_use_light: bool, high_contrast: bool) -> bool {
    interactive && !apps_use_light && !high_contrast
}

/// Follows a change of Windows' app mode or High Contrast while the window is open (it was
/// decided once at creation, so the window stayed dark after switching to light; found by the
/// real-desktop check of #66): the caption, the controls and the colours, then a repaint.
fn follow_theme(hwnd: HWND) {
    let Some((interactive, was_dark)) = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| (st.interactive, st.dark.is_some()))
    }) else {
        return;
    };
    let dark = interactive && dark_mode();
    if dark == was_dark {
        return;
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if dark {
                // SAFETY: a plain brush, deleted with the window or the next switch.
                st.dark = Some(unsafe { CreateSolidBrush(0x0020_2020) });
            } else if let Some(b) = st.dark.take() {
                // SAFETY: our brush, no longer selected anywhere.
                unsafe { DeleteObject(b) };
            }
        }
    });
    set_dark_controls(hwnd, dark);
    use windows_sys::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, WM_NCACTIVATE};
    // SAFETY: repaints our own window and its controls; the default WM_NCACTIVATE handling draws
    // the caption once in the other state and back (Windows 10 keeps the old caption colour
    // after the DWM attribute changes until the frame is activated again).
    unsafe {
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
        );
        let active = usize::from(GetForegroundWindow() == hwnd);
        DefWindowProcW(hwnd, WM_NCACTIVATE, 1 - active, 0);
        DefWindowProcW(hwnd, WM_NCACTIVATE, active, 0);
    }
}

/// Places the controls for the window's DPI and sets the message font at that DPI.
fn layout(hwnd: HWND) {
    use id::*;
    let zoom = STATE.with(|s| s.borrow().as_ref().map_or(100, |st| st.zoom));
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32 * zoom / 100;
    let px = |v: i32| v * dpi / 96;
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        // SAFETY: plain data.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the documented structure with its size set.
    let font = unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            (&raw mut ncm).cast(),
            0,
            dpi as u32,
        );
        CreateFontIndirectW(&raw const ncm.lfMessageFont)
    };
    let old = STATE.with(|s| {
        s.borrow_mut()
            .as_mut()
            .map(|st| std::mem::replace(&mut st.font, font))
    });
    let place = |i: u16, x: i32, y: i32, w: i32, h: i32| {
        let c = item(hwnd, i);
        // SAFETY: our own control and font.
        unsafe {
            SetWindowPos(
                c,
                std::ptr::null_mut(),
                px(x),
                px(y),
                px(w),
                px(h),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            SendMessageW(c, WM_SETFONT, font as usize, 1);
        }
    };
    place(LABEL_KEYS, 12, 10, 560, 18);
    place(LIST, 12, 30, 440, 250);
    for (k, i) in [CHANGE, DISABLE, RESET, RESET_ALL, IMPORT, EXPORT, FORCE]
        .into_iter()
        .enumerate()
    {
        place(i, 462, 30 + 34 * k as i32, 110, 28);
    }
    place(RECORDER, 12, 286, 560, 24);
    place(LABEL_DOUBLE, 12, 322, 250, 18);
    place(DOUBLE, 270, 318, 200, 200);
    place(LABEL_MIDDLE, 12, 352, 250, 18);
    place(MIDDLE, 270, 348, 200, 200);
    place(TRAY, 12, 382, 560, 22);
    place(HISTORY_DISK, 12, 406, 560, 22);
    place(PIN_BACKUP, 12, 430, 560, 22);
    place(LABEL_GROUPS, 12, 458, 560, 18);
    place(GROUPS, 12, 478, 440, 130);
    for (k, i) in [GROUP_SWITCH, GROUP_SHOW, GROUP_HIDE, GROUP_CLOSE]
        .into_iter()
        .enumerate()
    {
        place(i, 462, 478 + 34 * k as i32, 110, 28);
    }
    place(STATUS, 12, 616, 560, 40);
    place(CLOSE, 472, 658, 100, 28);
    if zoom != 100 {
        // SAFETY: our own window; only its size changes.
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                px(600),
                px(740),
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
            );
        }
    }
    if let Some(old) = old.filter(|f| !f.is_null()) {
        // SAFETY: the previous font, no longer selected anywhere.
        unsafe { DeleteObject(old) };
    }
}

/// Runs `args` on a worker thread; the answer arrives as `WM_DONE`.
fn start(hwnd: HWND, args: Vec<String>, status: &str) {
    let done = STATE.with(|s| {
        let mut b = s.borrow_mut();
        let st = b.as_mut()?;
        if st.busy {
            return None;
        }
        st.busy = true;
        // A reload after a change keeps the change's answer on the status line.
        if !status.is_empty() {
            st.model.status = status.to_string();
        }
        Some(Arc::clone(&st.done))
    });
    let Some(done) = done else {
        return;
    };
    show(hwnd);
    let target = hwnd as usize;
    std::thread::spawn(move || {
        let v = if args.first().map(String::as_str) == Some("reload") {
            let hk = run_noeul(&["hotkeys".into(), "list".into()]);
            let cfg = run_noeul(&["config".into(), "show".into()]);
            let groups = run_noeul(&to_instance(vec![
                "pin".into(),
                "group".into(),
                "list".into(),
            ]));
            serde_json::json!({"ok": true, "hotkeys": hk["data"], "config": cfg["data"],
                "groups": groups["data"]})
        } else {
            run_noeul(&args)
        };
        if let Ok(mut d) = done.lock() {
            *d = Some((args, v));
        }
        // SAFETY: posting to our window (a closed window drops it).
        unsafe { PostMessageW(target as HWND, WM_DONE, 0, 0) };
    });
}

fn reload(hwnd: HWND, status: &str) {
    start(hwnd, vec!["reload".into()], status);
}

/// Puts the model on the controls. The state is copied out first: setting a control's text or
/// state makes it send notifications (`EN_CHANGE`…) that re-enter this window synchronously.
fn show(hwnd: HWND) {
    use id::*;
    let Some((m, busy)) = STATE.with(|s| s.borrow().as_ref().map(|st| (st.model.clone(), st.busy)))
    else {
        return;
    };
    let list = item(hwnd, LIST);
    let groups = item(hwnd, GROUPS);
    // SAFETY: our own controls; NUL-terminated strings.
    unsafe {
        SendMessageW(groups, LB_RESETCONTENT, 0, 0);
        for g in &m.groups {
            let w = wide(&g.text());
            SendMessageW(groups, LB_ADDSTRING, 0, w.as_ptr() as isize);
        }
        if let Some(i) = m.group_selected {
            SendMessageW(groups, LB_SETCURSEL, i, 0);
        }
        for i in [GROUP_SWITCH, GROUP_SHOW, GROUP_HIDE, GROUP_CLOSE] {
            EnableWindow(
                item(hwnd, i),
                i32::from(!busy && m.group_selected.is_some()),
            );
        }
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        for r in &m.rows {
            let w = wide(&r.text());
            SendMessageW(list, LB_ADDSTRING, 0, w.as_ptr() as isize);
        }
        SendMessageW(list, LB_SETCURSEL, m.selected.unwrap_or(0), 0);
        let pos = |v: &[&str], x: &str| v.iter().position(|y| *y == x).unwrap_or(0);
        SendMessageW(
            item(hwnd, DOUBLE),
            CB_SETCURSEL,
            pos(&crate::prefs::DOUBLE_CLICK, &m.double_click),
            0,
        );
        SendMessageW(
            item(hwnd, MIDDLE),
            CB_SETCURSEL,
            pos(&crate::prefs::MIDDLE_CLICK, &m.middle_click),
            0,
        );
        for (i, on) in [
            (TRAY, m.tray_icon),
            (HISTORY_DISK, m.history_disk),
            (PIN_BACKUP, m.pin_backup),
        ] {
            SendMessageW(
                item(hwnd, i),
                BM_SETCHECK,
                if on { BST_CHECKED as usize } else { 0 },
                0,
            );
        }
        EnableWindow(item(hwnd, FORCE), i32::from(m.pending.is_some() && !busy));
        for i in [
            CHANGE,
            DISABLE,
            RESET,
            RESET_ALL,
            IMPORT,
            EXPORT,
            DOUBLE,
            MIDDLE,
            TRAY,
            HISTORY_DISK,
            PIN_BACKUP,
        ] {
            EnableWindow(item(hwnd, i), i32::from(!busy));
        }
    }
    set_text(item(hwnd, STATUS), &m.status);
    set_text(
        item(hwnd, RECORDER),
        &if m.recording.is_some() {
            "Recording: press the new keys (Esc cancels)".to_string()
        } else {
            m.selected_row()
                .map(|r| format!("Selected: {}", r.text()))
                .unwrap_or_default()
        },
    );
}

fn with_model<T>(f: impl FnOnce(&mut Model) -> T) -> Option<T> {
    STATE.with(|s| s.borrow_mut().as_mut().map(|st| f(&mut st.model)))
}

fn command(hwnd: HWND, ctl: u16, code: u16) {
    use id::*;
    let handled = matches!(
        (ctl, code),
        (LIST | DOUBLE | MIDDLE | GROUPS, 1)
            | (GROUP_SWITCH | GROUP_SHOW | GROUP_HIDE | GROUP_CLOSE, 0)
            | (
                CHANGE
                    | DISABLE
                    | RESET
                    | RESET_ALL
                    | IMPORT
                    | EXPORT
                    | FORCE
                    | TRAY
                    | HISTORY_DISK
                    | PIN_BACKUP
                    | CLOSE,
                0
            )
    );
    if !handled {
        return;
    }
    let busy = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.busy));
    if busy && ctl != CLOSE {
        return;
    }
    if ctl != RESET_ALL {
        STATE.with(|s| {
            if let Some(st) = s.borrow_mut().as_mut() {
                st.confirm_reset = false;
            }
        });
    }
    match (ctl, code) {
        (LIST, 1) => {
            // LBN_SELCHANGE
            // SAFETY: our own list box.
            let i = unsafe { SendMessageW(item(hwnd, LIST), LB_GETCURSEL, 0, 0) };
            with_model(|m| m.selected = usize::try_from(i).ok());
            show(hwnd);
        }
        (CHANGE, 0) => {
            if with_model(Model::start_recording) == Some(true) {
                RECORDING.store(true, Ordering::SeqCst);
                let interactive =
                    STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.interactive));
                if interactive {
                    // SAFETY: our own control.
                    unsafe { SetFocus(item(hwnd, RECORDER)) };
                }
            }
            show(hwnd);
        }
        (DISABLE, 0) => {
            if let Some(c) = with_model(|m| m.disable()).flatten() {
                start(hwnd, c, "Saving…");
            }
        }
        (RESET, 0) => {
            if let Some(c) = with_model(|m| m.reset()).flatten() {
                start(hwnd, c, "Saving…");
            }
        }
        (RESET_ALL, 0) => {
            let confirmed = STATE.with(|s| {
                let mut b = s.borrow_mut();
                let st = b.as_mut()?;
                let was = st.confirm_reset;
                st.confirm_reset = !was;
                if !was {
                    st.model.status =
                        "Reset every shortcut to its default? Press \"Reset all\" again to confirm (a backup is kept).".into();
                }
                Some(was)
            });
            if confirmed == Some(true) {
                start(hwnd, Model::reset_all(), "Resetting…");
            } else {
                show(hwnd);
            }
        }
        (FORCE, 0) => {
            if let Some(c) = with_model(Model::force).flatten() {
                start(hwnd, c, "Saving…");
            }
        }
        (IMPORT, 0) | (EXPORT, 0) => file_dialog(hwnd, ctl == IMPORT),
        (DOUBLE, 1) | (MIDDLE, 1) => {
            // CBN_SELCHANGE
            // SAFETY: our own combo box.
            let i = unsafe { SendMessageW(item(hwnd, ctl), CB_GETCURSEL, 0, 0) };
            let values: &[&str] = if ctl == DOUBLE {
                &crate::prefs::DOUBLE_CLICK
            } else {
                &crate::prefs::MIDDLE_CLICK
            };
            if let Some(v) = usize::try_from(i).ok().and_then(|i| values.get(i)) {
                start(hwnd, Model::gesture(ctl == DOUBLE, v), "Saving…");
            }
        }
        (TRAY, 0) | (HISTORY_DISK, 0) | (PIN_BACKUP, 0) => {
            // SAFETY: our own check box.
            let on =
                unsafe { SendMessageW(item(hwnd, ctl), BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
            let t = match ctl {
                TRAY => Toggle::TrayIcon,
                HISTORY_DISK => Toggle::HistoryDisk,
                _ => Toggle::PinBackup,
            };
            start(hwnd, Model::toggle(t, on), "Saving…");
        }
        (GROUPS, 1) => {
            // LBN_SELCHANGE
            // SAFETY: our own list box.
            let i = unsafe { SendMessageW(item(hwnd, GROUPS), LB_GETCURSEL, 0, 0) };
            with_model(|m| m.group_selected = usize::try_from(i).ok());
            show(hwnd);
        }
        (GROUP_SWITCH | GROUP_SHOW | GROUP_HIDE | GROUP_CLOSE, 0) => {
            let a = match ctl {
                GROUP_SWITCH => GroupAction::Switch,
                GROUP_SHOW => GroupAction::Show,
                GROUP_HIDE => GroupAction::Hide,
                _ => GroupAction::Close,
            };
            if let Some(c) = with_model(|m| m.group_command(a)).flatten() {
                start(hwnd, to_instance(c), "Working…");
            }
        }
        (CLOSE, 0) => {
            // SAFETY: our own window.
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        }
        _ => {}
    }
}

/// Import or export through the common file dialog (interactive only: the test hook never shows
/// a dialog; import and export are the CLI's `hotkeys import|export`).
fn file_dialog(hwnd: HWND, import: bool) {
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let interactive = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.interactive));
    if !interactive {
        with_model(|m| {
            m.status =
                "Import and export need the window on screen; use `noeul hotkeys import|export`."
                    .into();
        });
        show(hwnd);
        return;
    }
    let mut buf = [0u16; 1024];
    let filter: Vec<u16> = "Kim shortcuts (*.json)\0*.json\0All files\0*.*\0\0"
        .encode_utf16()
        .collect();
    let def = wide("json");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: buf.as_mut_ptr(),
        nMaxFile: buf.len() as u32,
        lpstrDefExt: def.as_ptr(),
        Flags: if import {
            OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST
        } else {
            OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST
        },
        // SAFETY: plain data.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the documented structure; `buf` and the strings outlive the modal call.
    let ok = unsafe {
        if import {
            GetOpenFileNameW(&raw mut ofn)
        } else {
            GetSaveFileNameW(&raw mut ofn)
        }
    };
    if ok == 0 {
        return;
    }
    let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..n]);
    if import {
        start(hwnd, Model::import(&path), "Importing…");
    } else {
        // Export: the CLI prints the document; the window writes it where the user chose.
        let v = run_noeul(&Model::export());
        let text = Model::export_text(&v);
        let msg = match std::fs::write(&path, text) {
            Ok(()) => "The shortcuts were exported.".to_string(),
            Err(e) => format!("The file could not be written: {e}"),
        };
        with_model(|m| m.status = msg);
        show(hwnd);
    }
}

fn finished(hwnd: HWND) {
    let got = STATE.with(|s| {
        let mut b = s.borrow_mut();
        let st = b.as_mut()?;
        st.busy = false;
        st.done.lock().ok()?.take()
    });
    let Some((args, v)) = got else {
        return;
    };
    if args.first().map(String::as_str) == Some("reload") {
        let fresh = Model::from_docs(&v["hotkeys"], &v["config"]);
        with_model(|m| {
            let first = m.rows.is_empty();
            m.refresh(fresh);
            m.set_groups(&v["groups"]);
            if first {
                m.status = format!(
                    "{} actions. Select one and press Change to record new keys.",
                    m.rows.len()
                );
            }
        });
        show(hwnd);
        return;
    }
    with_model(|m| m.answer(&args, &v));
    // Read the result back from the files (the CLI's own view of them).
    reload(hwnd, "");
}

fn recorded(hwnd: HWND, vk: u32, ctrl: bool, alt: bool, shift: bool) {
    let name = crate::win::pinwnd::vk_name(vk);
    let k = with_model(|m| m.key(name.as_deref(), ctrl, alt, shift));
    match k {
        Some(Key::Bind(c)) => {
            RECORDING.store(false, Ordering::SeqCst);
            start(hwnd, c, "Saving…");
        }
        Some(Key::Cancelled) => {
            RECORDING.store(false, Ordering::SeqCst);
            show(hwnd);
        }
        _ => show(hwnd),
    }
}

unsafe extern "system" fn recorder_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let recording = recording();
    match msg {
        WM_GETDLGCODE if recording => return DLGC_WANTALLKEYS as LRESULT,
        WM_KEYDOWN | WM_SYSKEYDOWN if recording => {
            // SAFETY: plain key-state queries for this thread's input.
            let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
            // SAFETY: the recorder's parent is our window.
            let parent = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetParent(h) };
            recorded(
                parent,
                wp as u32,
                down(VK_CONTROL),
                down(VK_MENU),
                down(VK_SHIFT),
            );
            return 0;
        }
        WM_CHAR | WM_SYSCHAR if recording => return 0,
        _ => {}
    }
    let old = STATE.with(|s| s.borrow().as_ref().map_or(0, |st| st.recorder_proc));
    // SAFETY: the edit control's own procedure, kept when it was subclassed.
    unsafe {
        let p: WNDPROC = std::mem::transmute::<isize, WNDPROC>(old);
        CallWindowProcW(p, h, msg, wp, lp)
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            command(hwnd, (wp & 0xFFFF) as u16, ((wp >> 16) & 0xFFFF) as u16);
            0
        }
        WM_ERASEBKGND => match STATE.with(|s| s.borrow().as_ref().and_then(|st| st.dark)) {
            // The dark background behind the controls (the class brush is the light face colour;
            // found by the real-desktop check of ADR 0091).
            Some(brush) => {
                let mut r = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                // SAFETY: our own window and the DC Windows passed for this erase.
                unsafe {
                    GetClientRect(hwnd, &raw mut r);
                    FillRect(wp as HDC, &raw const r, brush);
                }
                1
            }
            // SAFETY: the default handling of our own window's message.
            None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        },
        WM_DONE => {
            finished(hwnd);
            0
        }
        WM_TEST_KEY => {
            let interactive = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.interactive));
            if !interactive && recording() {
                recorded(hwnd, wp as u32, lp & 1 != 0, lp & 2 != 0, lp & 4 != 0);
            }
            0
        }
        WM_DPICHANGED => {
            // SAFETY: lParam points at the suggested RECT (documented).
            let r = unsafe { &*(lp as *const RECT) };
            // SAFETY: our own window.
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            layout(hwnd);
            0
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            follow_theme(hwnd);
            // SAFETY: the default handling of our own window's message.
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN => {
            match STATE.with(|s| s.borrow().as_ref().and_then(|st| st.dark)) {
                Some(brush) => {
                    let dc = wp as HDC;
                    // SAFETY: the control's DC for this paint.
                    unsafe {
                        SetTextColor(dc, 0x00F0_F0F0);
                        SetBkColor(dc, 0x0020_2020);
                    }
                    brush as LRESULT
                }
                // SAFETY: default colours (light mode, High Contrast).
                None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
            }
        }
        WM_CLOSE => {
            // SAFETY: our own window.
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            RECORDING.store(false, Ordering::SeqCst);
            if let Some(st) = STATE.with(|s| s.borrow_mut().take()) {
                // SAFETY: our font and brush, no longer used.
                unsafe {
                    if !st.font.is_null() {
                        DeleteObject(st.font);
                    }
                    if let Some(b) = st.dark {
                        DeleteObject(b);
                    }
                }
            }
            0
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// For the message loop: keyboard navigation (`Tab`, arrows, mnemonics) inside the window.
pub fn dialog_message(msg: &MSG) -> bool {
    let Some(h) = STATE.with(|s| s.borrow().as_ref().map(|st| st.hwnd)) else {
        return false;
    };
    // While recording, the recorder gets every key itself.
    if recording() {
        return false;
    }
    // SAFETY: our own window; a message just retrieved.
    unsafe { IsDialogMessageW(h, msg) != 0 }
}

/// Test hook: the window, its rows, selection, status and whether it records or is busy.
pub fn test_state() -> Value {
    STATE.with(|s| {
        s.borrow().as_ref().map_or(Value::Null, |st| {
            serde_json::json!({
                "hwnd": st.hwnd as usize,
                "rows": st.model.rows.iter().map(|r| serde_json::json!({"id": r.id, "text": r.text()})).collect::<Vec<_>>(),
                "selected": st.model.selected,
                "status": st.model.status,
                "recording": recording(),
                "busy": st.busy,
                "pending": st.model.pending.is_some(),
                "double_click": st.model.double_click,
                "history_disk": st.model.history_disk,
                "pin_backup": st.model.pin_backup,
            })
        })
    })
}

pub fn is_open() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

#[cfg(test)]
mod theme_tests {
    use super::wants_dark;

    #[test]
    fn the_window_is_larger_on_a_large_screen() {
        // ADR 0091: 5120 × 1410 at 100 % → 150 %; 1920 × 1040 → 125 %; small screens 100 %.
        assert_eq!(super::zoom_for(5120, 1410, 96), 150);
        assert_eq!(super::zoom_for(1920, 1040, 96), 125);
        assert_eq!(super::zoom_for(1366, 728, 96), 100);
        assert_eq!(super::zoom_for(3840, 2100, 144), 150);
        assert_eq!(super::zoom_for(7680, 4280, 96), 200);
    }

    /// #66: the window is dark only when interactive, in the dark app mode and without High
    /// Contrast; `follow_theme` re-evaluates this on every theme change.
    #[test]
    fn the_window_is_dark_only_in_the_dark_app_mode_without_high_contrast() {
        assert!(wants_dark(true, false, false));
        assert!(!wants_dark(true, true, false), "light app mode");
        assert!(
            !wants_dark(true, false, true),
            "High Contrast uses the system colours"
        );
        assert!(
            !wants_dark(false, false, false),
            "test windows stay default"
        );
    }
}
