//! #101 (follow-up of Step 78, ADR 0086): recording the mouse for a macro.
//!
//! Only during an explicit recording the editor started (with its banner): a low-level mouse
//! hook on its own thread **observes** (it never blocks or changes an event) and keeps button
//! presses and wheel turns with their screen points. Left out: input other programs injected,
//! anything on Ddalgak's own windows (so pressing *Stop* is not recorded), windows of elevated
//! processes (no rule may act on them) and the secure desktop (Windows never delivers it).
//! Moves without a click are not kept; the point of each click is. Nothing is written; the
//! editor turns the events into steps the user reviews before saving.

use super::context::is_elevated_window;
use crate::mouse::RecEvent;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GA_ROOT, GetAncestor, GetMessageW, GetWindowThreadProcessId, HC_ACTION,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN, WindowFromPoint, XBUTTON1,
};

/// Where the hook callback puts what it saw (one recording at a time).
struct Sink {
    events: Arc<Mutex<Vec<RecEvent>>>,
    own_pid: u32,
    own_il: u32,
    started: Instant,
}

fn sink() -> &'static Mutex<Option<Sink>> {
    static S: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// A running recording; [`MouseRecording::stop`] (or dropping it) removes the hook.
#[derive(Debug)]
pub struct MouseRecording {
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
    events: Arc<Mutex<Vec<RecEvent>>>,
}

/// Whether the window under `pt` belongs to this process or an elevated one (both skipped).
fn skip_point(pt: POINT, own_pid: u32, own_il: u32) -> bool {
    // SAFETY: plain window queries with a point and the handles they return.
    let root = unsafe { GetAncestor(WindowFromPoint(pt), GA_ROOT) };
    if root.is_null() {
        return true;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is writable.
    unsafe { GetWindowThreadProcessId(root, &raw mut pid) };
    pid == own_pid || is_elevated_window(root, own_il)
}

unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, `lparam` points to an MSLLHOOKSTRUCT valid for this call.
        let ms = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let msg = u32::try_from(wparam).unwrap_or(0);
        let physical = ms.flags & LLMHF_INJECTED == 0;
        let hi = i16::from_ne_bytes(((ms.mouseData >> 16) as u16).to_ne_bytes());
        let kind = match msg {
            WM_LBUTTONDOWN => Some(("left", 0)),
            WM_RBUTTONDOWN => Some(("right", 0)),
            WM_MBUTTONDOWN => Some(("middle", 0)),
            WM_XBUTTONDOWN if (ms.mouseData >> 16) as u16 == XBUTTON1 => Some(("x1", 0)),
            WM_XBUTTONDOWN => Some(("x2", 0)),
            // A positive wheel delta turns away from the user: up (vertical) or right.
            WM_MOUSEWHEEL => Some((if hi > 0 { "up" } else { "down" }, i32::from(hi))),
            WM_MOUSEHWHEEL => Some((if hi > 0 { "right" } else { "left" }, i32::from(hi))),
            _ => None,
        };
        if physical
            && let Some((what, delta)) = kind
            && let Ok(g) = sink().lock()
            && let Some(s) = g.as_ref()
            && !skip_point(ms.pt, s.own_pid, s.own_il)
            && let Ok(mut ev) = s.events.lock()
            && ev.len() < 1000
        {
            let at_ms = u64::try_from(s.started.elapsed().as_millis()).unwrap_or(u64::MAX);
            ev.push(if delta == 0 {
                RecEvent::Click {
                    button: what,
                    x: ms.pt.x,
                    y: ms.pt.y,
                    at_ms,
                }
            } else {
                RecEvent::Wheel {
                    direction: what,
                    delta: delta.abs(),
                    at_ms,
                }
            });
        }
    }
    // SAFETY: passes every event on unchanged; the hook never consumes or alters input.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

impl MouseRecording {
    /// Installs the observing hook on its own thread.
    pub fn start() -> io::Result<MouseRecording> {
        let events = Arc::new(Mutex::new(Vec::new()));
        {
            let mut g = sink()
                .lock()
                .map_err(|_| io::Error::other("recorder state poisoned"))?;
            if g.is_some() {
                return Err(io::Error::other("a mouse recording is already running"));
            }
            *g = Some(Sink {
                events: Arc::clone(&events),
                own_pid: std::process::id(),
                own_il: super::own_integrity().unwrap_or(0x2000),
                started: Instant::now(),
            });
        }
        let (tx, rx) = std::sync::mpsc::channel::<io::Result<u32>>();
        let join = std::thread::Builder::new()
            .name("ddalgak-mouse-record".into())
            .spawn(move || {
                // SAFETY: a module handle of this process and a callback that lives as long.
                let hook = unsafe {
                    SetWindowsHookExW(
                        WH_MOUSE_LL,
                        Some(proc),
                        GetModuleHandleW(std::ptr::null()),
                        0,
                    )
                };
                if hook.is_null() {
                    let _ = tx.send(Err(io::Error::last_os_error()));
                    return;
                }
                // SAFETY: no preconditions.
                let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));
                let mut msg = MSG::default();
                // SAFETY: a plain message loop of this thread; WM_QUIT ends it.
                while unsafe {
                    GetMessageW(
                        &raw mut msg,
                        std::ptr::null_mut::<std::ffi::c_void>() as HWND,
                        0,
                        0,
                    )
                } > 0
                {}
                // SAFETY: the hook this thread installed.
                unsafe { UnhookWindowsHookEx(hook) };
            })?;
        match rx.recv() {
            Ok(Ok(thread)) => Ok(MouseRecording {
                thread,
                join: Some(join),
                events,
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                *sink().lock().map_err(|_| io::Error::other("poisoned"))? = None;
                Err(e)
            }
            Err(_) => {
                *sink().lock().map_err(|_| io::Error::other("poisoned"))? = None;
                Err(io::Error::other("the recording thread ended"))
            }
        }
    }

    /// How many presses and wheel turns were kept so far.
    pub fn len(&self) -> usize {
        self.events.lock().map_or(0, |e| e.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes the hook and returns what was recorded.
    pub fn stop(mut self) -> Vec<RecEvent> {
        self.end();
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    fn end(&mut self) {
        if let Some(j) = self.join.take() {
            // SAFETY: posts WM_QUIT to the recording thread's own queue.
            unsafe { PostThreadMessageW(self.thread, WM_QUIT, 0, 0) };
            let _ = j.join();
            if let Ok(mut g) = sink().lock() {
                *g = None;
            }
        }
    }
}

impl Drop for MouseRecording {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hook installs and is removed again (no input is sent; a real click by the person at
    /// the computer may be kept, so the content is not asserted); one recording at a time.
    #[test]
    fn a_recording_installs_its_hook_once_and_removes_it() {
        let r = MouseRecording::start().expect("the hook installs");
        assert!(MouseRecording::start().is_err(), "one recording at a time");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(r.stop().len() <= 1000);
        let again = MouseRecording::start().expect("a new recording after stop");
        drop(again);
    }
}
