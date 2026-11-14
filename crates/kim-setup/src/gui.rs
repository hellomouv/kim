//! The installer's only window: a title, a status line and a progress bar, drawn with GDI.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_WINDOW, CreateSolidBrush, DT_LEFT, DT_SINGLELINE, DeleteObject, DrawTextW,
    EndPaint, FillRect, InvalidateRect, PAINTSTRUCT, SetBkMode, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetMessageW, GetSystemMetrics, IDC_ARROW, IDYES, LoadCursorW, LoadIconW,
    MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO, MSG, MessageBoxW, PostMessageW,
    PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, ShowWindow,
    TranslateMessage, WM_APP, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_CAPTION, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU,
};

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn message(text: &str, error: bool) {
    let (t, c) = (w(text), w("Kim Setup"));
    // SAFETY: NUL-terminated strings; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            if error {
                MB_ICONERROR
            } else {
                MB_ICONINFORMATION
            },
        );
    }
}

pub fn confirm(text: &str) -> bool {
    let (t, c) = (w(text), w("Kim Setup"));
    // SAFETY: as above.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

static PERCENT: AtomicU32 = AtomicU32::new(0);
static STATUS: Mutex<String> = Mutex::new(String::new());
const WM_PROGRESS: u32 = WM_APP + 1;
const WM_DONE: u32 = WM_APP + 2;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            // SAFETY: a paint cycle on our own window.
            unsafe {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &raw mut ps);
                SetBkMode(dc, TRANSPARENT as i32);
                let mut title = RECT {
                    left: 24,
                    top: 20,
                    right: 440,
                    bottom: 44,
                };
                let t = w("Installing Kim Manager");
                DrawTextW(dc, t.as_ptr(), -1, &raw mut title, DT_LEFT | DT_SINGLELINE);
                let status = STATUS.lock().map(|s| s.clone()).unwrap_or_default();
                let mut line = RECT {
                    left: 24,
                    top: 50,
                    right: 440,
                    bottom: 72,
                };
                let s = w(&status);
                DrawTextW(dc, s.as_ptr(), -1, &raw mut line, DT_LEFT | DT_SINGLELINE);
                let track = RECT {
                    left: 24,
                    top: 86,
                    right: 424,
                    bottom: 98,
                };
                let bg = CreateSolidBrush(0x00E0_E0E0);
                FillRect(dc, &raw const track, bg);
                DeleteObject(bg);
                let pct = i32::try_from(PERCENT.load(Ordering::Relaxed).min(100)).unwrap_or(0);
                let bar = RECT {
                    left: 24,
                    top: 86,
                    right: 24 + 400 * pct / 100,
                    bottom: 98,
                };
                let fg = CreateSolidBrush(0x0031_8A3A);
                FillRect(dc, &raw const bar, fg);
                DeleteObject(fg);
                EndPaint(hwnd, &raw const ps);
            }
            0
        }
        WM_PROGRESS => {
            // SAFETY: our own window.
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 1) };
            0
        }
        WM_DONE => {
            // SAFETY: our own window.
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Runs `work` on a worker thread while the window shows its progress; returns its result.
pub fn run_with_progress<T: Send + 'static>(
    work: impl FnOnce(&mut dyn FnMut(u32, &str)) -> T + Send + 'static,
) -> T {
    let class = w("KimSetupWindow");
    // SAFETY: registering and creating our own window class and window; all strings outlive
    // the calls; the handles are used only on this thread (the worker only posts messages).
    let hwnd = unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc),
            hInstance: inst,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            // ADR 0089: the Kim icon linked into Kim-Setup.exe (resource 1, kim-winres).
            hIcon: LoadIconW(inst, 1 as _),
            hbrBackground: (COLOR_WINDOW + 1) as usize as _,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&raw const wc);
        let (cw, ch) = (464, 160);
        let (x, y) = (
            (GetSystemMetrics(SM_CXSCREEN) - cw) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - ch) / 2,
        );
        let title = w("Kim Setup");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            if x > 0 { x } else { CW_USEDEFAULT },
            if y > 0 { y } else { CW_USEDEFAULT },
            cw,
            ch,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        );
        ShowWindow(hwnd, SW_SHOW);
        hwnd
    };
    let target = hwnd as usize;
    let result: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let slot = result.clone();
    let worker = std::thread::spawn(move || {
        let mut report = |pct: u32, status: &str| {
            PERCENT.store(pct, Ordering::Relaxed);
            if let Ok(mut s) = STATUS.lock() {
                *s = status.to_string();
            }
            // SAFETY: posting to the window created above (a stale handle is harmless).
            unsafe { PostMessageW(target as HWND, WM_PROGRESS, 0, 0) };
        };
        let r = work(&mut report);
        if let Ok(mut s) = slot.lock() {
            *s = Some(r);
        }
        // SAFETY: as above.
        unsafe { PostMessageW(target as HWND, WM_DONE, 0, 0) };
    });
    // SAFETY: the standard message loop of this thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
    }
    let _ = worker.join();
    let r = result.lock().ok().and_then(|mut s| s.take());
    r.expect("the installer finished")
}
