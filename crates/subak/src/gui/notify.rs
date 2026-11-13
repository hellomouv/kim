//! #98 T3 (`CORE-79`): notifications when a long command or a Terminal's program finishes.
//!
//! Opt-in only (`terminal.notify_command_seconds`, `terminal.notify_exit`; both off by default)
//! and only while the user is not looking: the Terminal's tab is not shown or the window is not
//! in front. The decision and the words are pure functions; the notice itself is a Windows
//! notification from a notification-area icon that exists only while it shows (no icon stays
//! behind, nothing leaves the PC). The off-screen test window never shows one: it reports the
//! notice as a test event instead.

use crate::config::TerminalPrefs;
use crate::gui::model::Finished;

/// What a notification says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub text: String,
}

/// Whether `f` is worth a notification: the setting asks for it and the user is not looking
/// at that Terminal (`shown`: its tab is shown; `focused`: the window is in front).
pub fn wanted(f: &Finished, prefs: &TerminalPrefs, shown: bool, focused: bool) -> bool {
    if shown && focused {
        return false;
    }
    if f.command {
        let min = u64::from(prefs.notify_command_seconds) * 1000;
        prefs.notify_command_seconds > 0 && f.duration_ms.is_some_and(|ms| ms >= min)
    } else {
        prefs.notify_exit
    }
}

/// The notification's words: the Terminal's name, what finished, how long it ran, its code.
pub fn notice(f: &Finished) -> Notice {
    let code = f
        .exit_code
        .map_or(String::new(), |c| format!(" (exit code {c})"));
    let text = if f.command {
        match f.duration_ms {
            Some(ms) => format!("A command finished after {}{code}.", duration(ms)),
            None => format!("A command finished{code}."),
        }
    } else {
        format!("Its program ended{code}.")
    };
    Notice {
        title: format!("Subak \u{2014} {}", f.name),
        text,
    }
}

/// `5 s`, `1 min 05 s`, `2 h 03 min`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, (s % 3600) / 60),
    }
}

/// The Windows notification: a notification-area icon of this window with the notice as its
/// balloon (Windows 10 shows it as a toast), removed again by [`hide`].
#[cfg(windows)]
pub fn show(hwnd: windows_sys::Win32::Foundation::HWND, n: &Notice) -> bool {
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME, NIM_ADD, NIM_MODIFY,
        NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{ICON_SMALL, SendMessageW, WM_GETICON};
    // SAFETY: asks our own window for its small icon (owned by the window, not destroyed here).
    let icon = unsafe { SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0) };
    let mut d = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: NOTICE_ID,
        uFlags: NIF_ICON | NIF_TIP | NIF_INFO,
        hIcon: icon as _,
        dwInfoFlags: NIIF_INFO | NIIF_RESPECT_QUIET_TIME,
        ..NOTIFYICONDATAW::default()
    };
    let put = |dst: &mut [u16], s: &str| {
        for (i, u) in s.encode_utf16().take(dst.len() - 1).enumerate() {
            dst[i] = u;
        }
    };
    put(&mut d.szTip, "Subak");
    put(&mut d.szInfoTitle, &n.title);
    put(&mut d.szInfo, &n.text);
    // SAFETY: a correctly sized NOTIFYICONDATAW for our own window; a second notice while
    // the first still shows replaces it.
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &raw const d) != 0
            || Shell_NotifyIconW(NIM_MODIFY, &raw const d) != 0
    }
}

/// Removes the notification's icon (nothing happens when none shows).
#[cfg(windows)]
pub fn hide(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::Shell::{NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
    let d = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: NOTICE_ID,
        ..NOTIFYICONDATAW::default()
    };
    // SAFETY: removes our own window's icon, if it is there.
    unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const d) };
}

/// The icon id of the notification (the floating UI's tray icon lives in another window).
#[cfg(windows)]
const NOTICE_ID: u32 = 0x5342;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn finished(command: bool, ms: Option<u64>, exit: Option<i64>) -> Finished {
        Finished {
            terminal_id: "t-1".into(),
            name: "build".into(),
            command,
            exit_code: exit,
            duration_ms: ms,
        }
    }

    /// #98 T3 (`CORE-79`): nothing without opt-in; with it, only long commands and only while
    /// the user is not looking at that Terminal.
    #[test]
    fn notifications_need_opt_in_and_an_unwatched_terminal() {
        let mut p = Settings::default().terminal;
        let long = finished(true, Some(90_000), Some(0));
        let ended = finished(false, None, Some(1));
        for (shown, focused) in [(false, false), (false, true), (true, false)] {
            assert!(!wanted(&long, &p, shown, focused), "off by default");
            assert!(!wanted(&ended, &p, shown, focused), "off by default");
        }
        p.notify_command_seconds = 60;
        p.notify_exit = true;
        assert!(wanted(&long, &p, false, true), "another tab");
        assert!(wanted(&long, &p, true, false), "the window is behind");
        assert!(!wanted(&long, &p, true, true), "the user is watching");
        assert!(!wanted(
            &finished(true, Some(59_999), None),
            &p,
            false,
            false
        ));
        assert!(
            !wanted(&finished(true, None, None), &p, false, false),
            "no start mark"
        );
        assert!(wanted(&ended, &p, false, false));
        assert!(!wanted(&ended, &p, true, true));
    }

    #[test]
    fn the_notice_names_the_terminal_the_time_and_the_code() {
        assert_eq!(
            notice(&finished(true, Some(65_000), Some(2))),
            Notice {
                title: "Subak \u{2014} build".into(),
                text: "A command finished after 1 min 05 s (exit code 2).".into()
            }
        );
        assert_eq!(
            notice(&finished(false, None, None)).text,
            "Its program ended."
        );
        assert_eq!(duration(4_999), "4 s");
        assert_eq!(duration(7_380_000), "2 h 03 min");
    }
}
