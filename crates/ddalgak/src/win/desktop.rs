//! Step 79 (ADR 0080): the Windows side of window and notification steps, and other Kim tools.
//!
//! **Windows.** The target is the foreground window, or the first visible, unowned top-level
//! window of the named program (`EnumWindows`; processes compared by image path, never by
//! title). A window of a process with a higher integrity level (UIPI would ignore Ddalgak anyway)
//! or with the session locked is never touched. Minimize, maximize and restore use
//! `ShowWindowAsync` and moves `SetWindowPos` with `SWP_ASYNCWINDOWPOS`, so a hung program can
//! never stall the worker. Bringing a window to the front may be refused by Windows' foreground
//! rules (`focus_refused`).
//!
//! **Notifications** are a balloon of Ddalgak's own tray icon (`NIF_INFO`); without a tray icon
//! (the debug test host) they are `unsupported_platform` unless recorded for a test.
//!
//! **Tools** run `kim tools invoke <capability> --params <json> --json --no-input` of the
//! installed Kim (`%LOCALAPPDATA%\Programs\mouv\kim\current.json`), hidden, at most 30 s, with no
//! consent flags: a capability that needs the user's consent is refused by Kim. No Kim: nothing is
//! started and the step is `tool_missing`; Kim is never installed by Ddalgak.

use crate::actions::{Desktop, NotifySpec, ToolSpec, Tools, WindowSpec};
use crate::context::AppMatch;
use crate::model::WindowOp;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Shell::{
    NIF_INFO, NIIF_INFO, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowThreadProcessId,
    HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindowVisible, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindowAsync,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The app's tray icon (window and icon ID), when there is one.
#[derive(Debug, Default)]
pub struct WinDesktop {
    pub tray: Mutex<Option<(isize, u32)>>,
}

fn image(hwnd: HWND) -> Option<(u32, String)> {
    let mut pid = 0u32;
    // SAFETY: `pid` is writable.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    super::process_image(pid)
        .ok()
        .map(|p| (pid, p.to_string_lossy().to_lowercase()))
}

fn matches(m: &AppMatch, path: &str) -> bool {
    match m {
        AppMatch::Exe(e) => path.rsplit('\\').next() == Some(e.as_str()),
        AppMatch::Path(p) => path == p,
    }
}

/// The first visible, unowned top-level window of the program.
fn find(app: &AppMatch) -> Option<HWND> {
    struct Search<'a> {
        app: &'a AppMatch,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, l: LPARAM) -> i32 {
        // SAFETY: `l` is the `Search` passed below, alive for the whole enumeration.
        let s = unsafe { &mut *(l as *mut Search<'_>) };
        // SAFETY: plain queries of a window handle from the enumeration.
        let visible = unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() };
        if visible && image(hwnd).is_some_and(|(_, p)| matches(s.app, &p)) {
            s.found = Some(hwnd);
            return 0;
        }
        1
    }
    let mut s = Search { app, found: None };
    // SAFETY: the callback only reads window properties; `s` outlives the call.
    unsafe { EnumWindows(Some(each), (&raw mut s) as LPARAM) };
    s.found
}

/// #102: the programs with a visible, unowned top-level window on this desktop (lowercase file
/// names, sorted, without duplicates), for the editor's picker. Only process image paths are
/// read: never a window title or any window content. Ddalgak's own windows and windows of
/// elevated processes (which no rule may touch) are left out.
pub fn running_programs() -> Vec<String> {
    struct Found {
        own: u32,
        names: std::collections::BTreeSet<String>,
    }
    unsafe extern "system" fn each(hwnd: HWND, l: LPARAM) -> i32 {
        // SAFETY: `l` is the `Found` passed below, alive for the whole enumeration.
        let f = unsafe { &mut *(l as *mut Found) };
        // SAFETY: plain queries of a window handle from the enumeration.
        let visible = unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() };
        if visible
            && !super::context::is_elevated_window(hwnd, f.own)
            && let Some((_, path)) = image(hwnd)
            && let Some(name) = path.rsplit('\\').next()
            && !name.is_empty()
        {
            f.names.insert(name.to_string());
        }
        1
    }
    let mut f = Found {
        own: super::own_integrity().unwrap_or(0x2000),
        names: std::collections::BTreeSet::new(),
    };
    // SAFETY: the callback only reads window properties; `f` outlives the call.
    unsafe { EnumWindows(Some(each), (&raw mut f) as LPARAM) };
    f.names.into_iter().collect()
}

/// #102: one capability an installed Kim tool provides (interop v1 `provides`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub id: String,
    pub tool: String,
    pub effect: String,
    pub summary: String,
}

/// Reads the `provides` lists of `kim tools describe --json` documents (other fields ignored).
pub fn capabilities_of(describe: &serde_json::Value) -> Vec<Capability> {
    let tool = describe["data"]["tool"]["id"]
        .as_str()
        .unwrap_or("")
        .to_string();
    describe["data"]["capabilities"]["provides"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(Capability {
                        id: p["id"].as_str()?.to_string(),
                        tool: tool.clone(),
                        effect: p["effect"].as_str().unwrap_or("").to_string(),
                        summary: p["summary"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// #102: asks the installed Kim (hidden, at most 10 s per call) which capabilities its installed
/// tools provide: `kim tools list --json`, then `kim tools describe <id> --json` for each. Reads
/// manifests only; nothing is installed or started. `None`: no Kim.
pub fn discover_capabilities() -> Option<Vec<Capability>> {
    let kim = kim_exe()?;
    discover_with(|| std::process::Command::new(&kim))
}

/// [`discover_capabilities`] with the Kim command made by `kim` (tests give it a sandbox).
pub fn discover_with(kim: impl Fn() -> std::process::Command) -> Option<Vec<Capability>> {
    let run = |args: &[&str]| -> Option<serde_json::Value> {
        let mut child = kim()
            .args(args)
            .args(["--json", "--no-input"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut out, &mut buf);
            buf
        });
        let until = Instant::now() + Duration::from_secs(10);
        while child.try_wait().ok()?.is_none() {
            if Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        serde_json::from_slice(&reader.join().ok()?).ok()
    };
    let list = run(&["tools", "list"])?;
    let mut caps = Vec::new();
    for t in list["data"]["tools"].as_array().into_iter().flatten() {
        if t["status"] != "installed" {
            continue;
        }
        let Some(id) = t["id"].as_str() else { continue };
        if let Some(d) = run(&["tools", "describe", id]) {
            caps.extend(capabilities_of(&d));
        }
    }
    caps.sort_by(|a, b| a.id.cmp(&b.id));
    Some(caps)
}

impl Desktop for WinDesktop {
    fn window(&self, w: &WindowSpec) -> &'static str {
        if !super::input_desktop_available() {
            return "skipped_secure_desktop";
        }
        let hwnd = match &w.app {
            // SAFETY: no preconditions.
            None => unsafe { GetForegroundWindow() },
            Some(app) => match find(app) {
                Some(h) => h,
                None => return "window_not_found",
            },
        };
        if hwnd.is_null() || image(hwnd).is_none() {
            return "window_not_found";
        }
        let own = super::own_integrity().unwrap_or(0x2000);
        if super::context::is_elevated_window(hwnd, own) {
            return "skipped_elevated_target";
        }
        // SAFETY: asynchronous window calls on a window of another process; none blocks.
        let ok = unsafe {
            match w.op {
                // The return value is the previous visibility, not success.
                WindowOp::Minimize => {
                    ShowWindowAsync(hwnd, SW_MINIMIZE);
                    true
                }
                WindowOp::Maximize => {
                    ShowWindowAsync(hwnd, SW_MAXIMIZE);
                    true
                }
                WindowOp::Restore => {
                    ShowWindowAsync(hwnd, SW_RESTORE);
                    true
                }
                WindowOp::Activate => {
                    if IsIconic(hwnd) != 0 {
                        ShowWindowAsync(hwnd, SW_RESTORE);
                    }
                    if SetForegroundWindow(hwnd) == 0 {
                        return "focus_refused";
                    }
                    true
                }
                WindowOp::Move => {
                    let (x, y) = w.pos.unwrap_or((0, 0));
                    let (cx, cy) = w.size.map_or((0, 0), |(a, b)| {
                        (
                            i32::try_from(a).unwrap_or(50),
                            i32::try_from(b).unwrap_or(50),
                        )
                    });
                    let mut flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
                    if w.pos.is_none() {
                        flags |= SWP_NOMOVE;
                    }
                    if w.size.is_none() {
                        flags |= SWP_NOSIZE;
                    }
                    SetWindowPos(hwnd, std::ptr::null_mut(), x, y, cx, cy, flags) != 0
                }
                WindowOp::Topmost => {
                    let after = if w.on { HWND_TOPMOST } else { HWND_NOTOPMOST };
                    SetWindowPos(
                        hwnd,
                        after,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                    ) != 0
                }
            }
        };
        if ok { "ok" } else { "window_failed" }
    }

    fn notify(&self, n: &NotifySpec) -> &'static str {
        let Some((hwnd, id)) = self.tray.lock().ok().and_then(|g| *g) else {
            return "unsupported_platform";
        };
        let mut d = NOTIFYICONDATAW {
            cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
            hWnd: hwnd as HWND,
            uID: id,
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_INFO,
            ..Default::default()
        };
        let put = |dst: &mut [u16], t: &str| {
            let w: Vec<u16> = t.encode_utf16().take(dst.len() - 1).collect();
            dst[..w.len()].copy_from_slice(&w);
            dst[w.len()] = 0;
        };
        put(&mut d.szInfoTitle, &n.title);
        put(&mut d.szInfo, if n.body.is_empty() { " " } else { &n.body });
        // SAFETY: `d` is a fully initialized NOTIFYICONDATAW for the app's own icon.
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const d) } != 0 {
            "ok"
        } else {
            "notify_failed"
        }
    }
}

/// The debug test host: window and notification steps are written to `<root>\desktop.jsonl`
/// (the operation and target only) and never touch a real window or the tray.
#[derive(Debug)]
pub struct RecordingDesktop {
    pub file: Option<PathBuf>,
}

impl RecordingDesktop {
    fn write(&self, line: &serde_json::Value) -> &'static str {
        let Some(f) = &self.file else {
            return "unsupported_platform";
        };
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)
        {
            Ok(mut h) => {
                let _ = writeln!(h, "{line}");
                "ok"
            }
            Err(_) => "window_failed",
        }
    }
}

impl Desktop for RecordingDesktop {
    fn window(&self, w: &WindowSpec) -> &'static str {
        let app = w.app.as_ref().map(|a| match a {
            AppMatch::Exe(e) | AppMatch::Path(e) => e.clone(),
        });
        self.write(&serde_json::json!({ "window": w.op.as_str(), "app": app, "pos": w.pos, "size": w.size }))
    }
    fn notify(&self, n: &NotifySpec) -> &'static str {
        self.write(&serde_json::json!({ "notify": n.title.chars().count(), "body": n.body.chars().count() }))
    }
}

/// #102: local commands, run hidden (`CREATE_NO_WINDOW`), never through a shell, with the exact
/// argument list; waited for at most the step's timeout, then ended. Output is discarded.
#[derive(Debug, Default)]
pub struct WinCommands;

impl crate::actions::Commands for WinCommands {
    fn run(&self, c: &crate::actions::CommandSpec) -> &'static str {
        if !c.program.is_file() {
            return "program_not_found";
        }
        let mut cmd = std::process::Command::new(&c.program);
        cmd.args(&c.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if let Some(d) = &c.working_dir {
            cmd.current_dir(d);
        }
        let Ok(mut child) = cmd.spawn() else {
            return "launch_failed";
        };
        let until = Instant::now() + Duration::from_millis(u64::from(c.timeout_ms));
        loop {
            match child.try_wait() {
                Ok(Some(s)) => return if s.success() { "ok" } else { "command_failed" },
                Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return "timeout";
                }
            }
        }
    }
}

/// The debug test host: commands are recorded in `<root>\commands.jsonl` (the program's file
/// name and the argument count) and never run.
#[derive(Debug)]
pub struct RecordingCommands {
    pub file: Option<PathBuf>,
}

impl crate::actions::Commands for RecordingCommands {
    fn run(&self, c: &crate::actions::CommandSpec) -> &'static str {
        let Some(f) = &self.file else {
            return "unsupported_platform";
        };
        use std::io::Write;
        let name = c
            .program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let line = serde_json::json!({ "program": name, "arguments": c.args.len(), "timeout_ms": c.timeout_ms });
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)
        {
            Ok(mut h) => {
                let _ = writeln!(h, "{line}");
                "ok"
            }
            Err(_) => "launch_failed",
        }
    }
}

/// `kim tools invoke` of the installed Kim.
#[derive(Debug, Default)]
pub struct KimTools;

/// The installed `kim.exe` (`KIM_EXE` overrides it for tests).
pub fn kim_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KIM_EXE").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("Programs")
        .join("mouv")
        .join("kim");
    let rec: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("current.json")).ok()?).ok()?;
    let exe = dir.join(rec["entrypoint"].as_str()?.replace('/', "\\"));
    exe.is_file().then_some(exe)
}

impl Tools for KimTools {
    fn invoke(&self, t: &ToolSpec) -> &'static str {
        let Some(kim) = kim_exe() else {
            return "tool_missing";
        };
        let child = std::process::Command::new(kim)
            .args([
                "tools",
                "invoke",
                &t.capability,
                "--params",
                &t.args.to_string(),
                "--json",
                "--no-input",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let Ok(mut child) = child else {
            return "tool_missing";
        };
        let until = Instant::now() + Duration::from_secs(30);
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let Some(status) = status else {
            return "timeout";
        };
        let mut out = String::new();
        if let Some(o) = child.stdout.take() {
            use std::io::Read;
            let _ = o.take(1 << 20).read_to_string(&mut out);
        }
        let code = serde_json::from_str::<serde_json::Value>(&out)
            .ok()
            .and_then(|v| v["error"]["code"].as_str().map(String::from));
        match (status.success(), code.as_deref()) {
            (true, _) => "ok",
            (_, Some("capability_missing" | "not_installed")) => "tool_missing",
            (_, Some("tool_incompatible")) => "tool_incompatible",
            (_, Some("confirmation_required")) => "consent_missing",
            (_, Some("timeout")) => "timeout",
            _ => "tool_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No real window is touched: a program with no window is `window_not_found`, a balloon
    /// without a tray icon is refused, the test host records, a missing Kim is `tool_missing`.
    #[test]
    fn nothing_real_is_touched_and_absence_fails_gracefully() {
        let d = WinDesktop::default();
        let w = WindowSpec {
            op: WindowOp::Minimize,
            app: Some(AppMatch::Exe("no-such-program-kim-e2e.exe".into())),
            pos: None,
            size: None,
            on: true,
        };
        if super::super::input_desktop_available() {
            assert_eq!(d.window(&w), "window_not_found");
        }
        let n = NotifySpec {
            title: "t".into(),
            body: String::new(),
        };
        assert_eq!(d.notify(&n), "unsupported_platform");
        let dir = std::env::temp_dir().join(format!("ddalgak-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = RecordingDesktop {
            file: Some(dir.join("desktop.jsonl")),
        };
        assert_eq!((r.window(&w), r.notify(&n)), ("ok", "ok"));
        let lines = std::fs::read_to_string(dir.join("desktop.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: no other thread of this test reads or writes the environment meanwhile.
        unsafe { std::env::set_var("KIM_EXE", dir.join("missing-kim.exe")) };
        let t = ToolSpec {
            capability: "pin.list".into(),
            args: serde_json::json!({}),
        };
        assert_eq!(KimTools.invoke(&t), "tool_missing");
        // SAFETY: as above.
        unsafe { std::env::remove_var("KIM_EXE") };
    }

    /// #102: the real runner, hidden: exit code 0 is `ok`, another is `command_failed`, a missing
    /// program is `program_not_found`, an overrun is ended (`timeout`). Uses `where.exe` and
    /// `ping.exe` from System32: no window, no network beyond the loopback.
    #[test]
    fn win_commands_report_outcomes_and_end_overruns() {
        use crate::actions::{CommandSpec, Commands};
        let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into()))
            .join("System32");
        let spec = |exe: &str, args: &[&str], timeout_ms| CommandSpec {
            program: sys.join(exe),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            working_dir: None,
            timeout_ms,
        };
        let c = WinCommands;
        assert_eq!(c.run(&spec("where.exe", &["/q", "cmd.exe"], 10_000)), "ok");
        assert_eq!(
            c.run(&spec(
                "where.exe",
                &["/q", "no-such-program-kim.exe"],
                10_000
            )),
            "command_failed"
        );
        assert_eq!(
            c.run(&spec("no-such-program-kim.exe", &[], 1_000)),
            "program_not_found"
        );
        let t = Instant::now();
        assert_eq!(
            c.run(&spec("ping.exe", &["-n", "30", "127.0.0.1"], 300)),
            "timeout"
        );
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    /// #102: the picker lists file names only, sorted and unique, never Ddalgak itself.
    #[test]
    fn running_programs_are_sorted_unique_file_names() {
        let list = running_programs();
        let mut sorted = list.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(list, sorted);
        assert!(
            list.iter()
                .all(|n| !n.contains('\\') && n == &n.to_lowercase()),
            "{list:?}"
        );
        let own = std::env::current_exe().unwrap();
        let own = own.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(!list.contains(&own), "{list:?}");
    }

    /// #102: `provides` entries of a describe document become capabilities; others are ignored.
    #[test]
    fn capabilities_are_read_from_describe_documents() {
        let d = serde_json::json!({"data": {"tool": {"id": "noeul"}, "capabilities": {"provides": [
            {"id": "pin.list", "version": "1.0.0", "effect": "read", "summary": "Open pins"},
            {"version": "1.0.0"}
        ]}}});
        assert_eq!(
            capabilities_of(&d),
            [Capability {
                id: "pin.list".into(),
                tool: "noeul".into(),
                effect: "read".into(),
                summary: "Open pins".into()
            }]
        );
        assert!(capabilities_of(&serde_json::json!({"data": {}})).is_empty());
        let none = discover_with(|| std::process::Command::new(r"C:\no-such-kim\kim.exe"));
        assert_eq!(none, None);
    }
}
