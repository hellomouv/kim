//! `kim-manager` — the optional Kim Manager window. Started only by the user; closing it ends
//! the process (no tray icon, no background updater).
//!
//! Options: `--no-hotkeys` (start without any shortcut, HOTKEY_CONTRACT §8.3) and
//! `--catalog <file://…/catalog.json>` (ADR 0089: use a local mirror of packages built on this
//! PC for this session; a double-click on `kim.exe` passes the one next to it).

#![windows_subsystem = "windows"]

use eframe::egui;
use kim_manager::{Backend, KimManagerApp};

/// The Kim icon (the laver sheet) for the window's title bar and taskbar button.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/kim-128.png");

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let catalog = args
        .iter()
        .position(|a| a == "--catalog")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Kim")
        .with_inner_size([900.0, 560.0])
        .with_min_inner_size([720.0, 480.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "kim-manager",
        options,
        Box::new(move |cc| {
            let backend = Backend::real().map_err(|e| e.to_string())?;
            let mut app = KimManagerApp::new(backend, Some(&cc.egui_ctx));
            // Emergency start without any shortcut (HOTKEY_CONTRACT §8.3, KIM-SEC-023).
            if args.iter().any(|a| a == "--no-hotkeys") {
                app.disable_hotkeys();
            }
            if let Some(url) = &catalog {
                app.use_mirror(url);
            }
            Ok(Box::new(app))
        }),
    )
}
