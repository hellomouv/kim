//! `noeul-settings` — Noeul's settings window (ADR 0093).
//!
//! Started by Noeul when the user opens Settings (the tray menu, `noeul settings`, Kim's
//! *Settings…*), or by the user. One window per user session: a second start brings the open
//! one to the front. It runs `noeul.exe` from its own folder for every change and never starts
//! Noeul by itself.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

const HELP: &str = "\
noeul-settings — Noeul settings

USAGE:
    noeul-settings [--lang en|ko] [--endpoint <name>]
    noeul-settings --help | --version

Opens Noeul's settings window. Every change runs the documented `noeul` command (config set,
hotkeys …) with noeul.exe from this folder; `--endpoint` names the instance's pipe when it is
not the default one.
";

#[cfg(windows)]
#[allow(unsafe_code)]
mod single {
    //! One settings window per user session: a named mutex marks the owner, a named event asks
    //! it to show itself.

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, RECT, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, SPI_GETWORKAREA, SystemParametersInfoW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The owner's event (kept for the process's life).
    pub struct Owner(pub HANDLE);
    // SAFETY: kernel handles are process-wide.
    unsafe impl Send for Owner {}

    /// `None` when another window owns the session (it was asked to show itself).
    pub fn claim(tag: &str) -> Option<Owner> {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let base = format!("Local\\mouv-noeul-settings-{user}-{tag}");
        let m = wide(&base);
        let e = wide(&format!("{base}-show"));
        // SAFETY: NUL-terminated names; default security; an auto-reset event.
        unsafe {
            let mutex = CreateMutexW(std::ptr::null(), 0, m.as_ptr());
            let existed = GetLastError() == ERROR_ALREADY_EXISTS;
            let event = CreateEventW(std::ptr::null(), 0, 0, e.as_ptr());
            if existed {
                AllowSetForegroundWindow(ASFW_ANY);
                SetEvent(event);
                CloseHandle(event);
                CloseHandle(mutex);
                return None;
            }
            Some(Owner(event))
        }
    }

    /// Calls `show` whenever a second start asks.
    pub fn on_show(owner: &Owner, show: impl Fn() + Send + 'static) {
        let event = owner.0 as usize;
        let _ = std::thread::Builder::new()
            .name("noeul-settings-show".into())
            .spawn(move || {
                // SAFETY: the event lives as long as the process.
                while unsafe { WaitForSingleObject(event as HANDLE, INFINITE) } == WAIT_OBJECT_0 {
                    show();
                }
            });
    }

    /// The primary work area in points (read before the window exists).
    pub fn work_area() -> Option<(f32, f32)> {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: SPI_GETWORKAREA writes one RECT.
        let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut r).cast(), 0) };
        (ok != 0 && r.right > r.left)
            .then(|| ((r.right - r.left) as f32, (r.bottom - r.top) as f32))
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lang = None;
    let mut endpoint = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("noeul-settings {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--lang" => {
                i += 1;
                match args.get(i).and_then(|l| kim_ui::Lang::parse(l)) {
                    Some(l) => lang = Some(l),
                    None => {
                        eprintln!("noeul-settings: error: --lang takes en or ko (usage)");
                        return ExitCode::from(2);
                    }
                }
            }
            "--endpoint" => {
                i += 1;
                endpoint = args.get(i).cloned();
            }
            other => {
                eprintln!("noeul-settings: error: unknown option `{other}` (usage)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    run(lang, endpoint)
}

#[cfg(windows)]
fn run(lang: Option<kim_ui::Lang>, endpoint: Option<String>) -> ExitCode {
    use eframe::egui;
    let Some(owner) = single::claim(endpoint.as_deref().unwrap_or("default")) else {
        return ExitCode::SUCCESS;
    };
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("noeul.exe")))
        .filter(|p| p.is_file());
    let Some(exe) = exe else {
        eprintln!("noeul-settings: error: noeul.exe is not next to this program (not_found)");
        return ExitCode::from(1);
    };
    let zoom = single::work_area().map_or(1.0, |(w, h)| noeul_settings::window_scale(w, h));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Noeul settings")
            .with_app_id("mouv.noeul.settings")
            .with_inner_size([680.0 * zoom, 560.0 * zoom])
            .with_min_inner_size([480.0 * zoom, 360.0 * zoom]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let runner = std::sync::Arc::new(noeul_settings::NoeulCli { exe, endpoint });
    let result = eframe::run_native(
        "noeul-settings",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_zoom_factor(zoom);
            let ctx = cc.egui_ctx.clone();
            single::on_show(&owner, move || {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            });
            let mut app = noeul_settings::SettingsApp::new(runner);
            if let Some(l) = lang {
                app.set_lang(l);
            }
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("noeul-settings: error: {e} (io)");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
fn run(_lang: Option<kim_ui::Lang>, _endpoint: Option<String>) -> ExitCode {
    eprintln!("noeul-settings: error: Windows only (unsupported)");
    ExitCode::from(1)
}
