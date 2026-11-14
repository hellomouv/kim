//! Headless UI tests for Kim Manager's launcher (ADR 0090; egui_kittest: no window, no GPU). The
//! UI is driven through its accessibility tree, like a screen-reader user would, against a
//! sandboxed backend: three tiles (Subak, Noeul, Ddalgak), their right-click menus, the install
//! and uninstall confirmations, Information, and Settings behind the gear.
#![cfg(windows)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kim::catalog::signature::{self, TrustedKey};
use kim::paths::Paths;
use kim::platform::Platform;
use kim_manager::{Backend, Banner, KimManagerApp, Prefs, ThemePref, TileState};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

const SEED: [u8; 32] = [21u8; 32];
const CMD: &str = r"C:\Windows\System32\cmd.exe";

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("kim-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keys() -> Vec<TrustedKey> {
    let public: &'static str = Box::leak(signature::public_key_hex(&SEED).into_boxed_str());
    let id: &'static str =
        Box::leak(signature::key_id(&signature::unhex::<32>(public).unwrap()).into_boxed_str());
    let mut k = vec![TrustedKey {
        key_id: id,
        public_key_hex: public,
        label: "ui test key",
    }];
    k.extend_from_slice(signature::TRUSTED_KEYS);
    k
}

/// Serves files over loopback HTTP (`/<name>`); returns the base URL.
fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            match files.iter().find(|(p, _)| format!("/{p}") == path) {
                Some((_, body)) => {
                    let _ = write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(body);
                }
                None => {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                }
            }
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// A package: the CLI entrypoint is a copy of cmd.exe (it answers `--version` with stdin
/// closed); the other programs are copies of cmd.exe too (Kim never starts them in tests).
fn package(files: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (name, from) in files {
        zip.start_file(*name, o).unwrap();
        zip.write_all(&std::fs::read(from).unwrap()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The programs of each tool's package, as `cargo xtask package` lays them out.
fn programs(id: &str) -> Vec<(&'static str, &'static str)> {
    match id {
        "subak" => vec![("subak.exe", CMD), ("subak-gui.exe", CMD)],
        "noeul" => vec![("noeul.exe", CMD), ("noeulw.exe", CMD)],
        _ => vec![
            ("ddalgak.exe", CMD),
            ("ddalgak-gui.exe", CMD),
            ("ddalgak-editor.exe", CMD),
        ],
    }
}

/// The bundled catalog's real tool entries, with a test release (`version`) for each tool in
/// `released`, served over loopback and test-signed into the sandbox cache.
fn backend_with(sb: &Sandbox, released: &[(&str, &str, Vec<u8>)]) -> Backend {
    let paths = Paths::under(&sb.0);
    let files: Vec<(String, Vec<u8>)> = released
        .iter()
        .map(|(id, v, body)| (format!("{id}-{v}.zip"), body.clone()))
        .collect();
    let base = serve(files);
    let mut doc: serde_json::Value =
        serde_json::from_slice(kim::catalog::store::BUNDLED_CATALOG).unwrap();
    doc["catalog_version"] = json!(500);
    doc["expires_at"] = json!("2099-01-01T00:00:00Z");
    for t in doc["tools"].as_array_mut().unwrap() {
        if let Some((id, v, body)) = released.iter().find(|(id, _, _)| t["id"] == *id) {
            let entry = programs(id)[0].0;
            t["releases"] = json!([{"version": v, "channel": "stable",
                "published_at": "2026-11-01T00:00:00Z", "release_notes": "test release",
                "assets": [{"os": "windows", "arch": "x86_64", "url": format!("{base}/{id}-{v}.zip"),
                            "size": body.len(), "sha256": signature::hex(&Sha256::digest(body)),
                            "kind": "zip", "entrypoint": entry}]}]);
        }
    }
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    let sig = serde_json::to_vec(&signature::sign(&bytes, &SEED)).unwrap();
    let dir = paths.catalog_cache_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("catalog.json"), bytes).unwrap();
    std::fs::write(dir.join("catalog.json.sig"), sig).unwrap();
    Backend {
        paths,
        keys: keys(),
        downloader: Arc::new(kim::fetch::DefaultFetcher::default()),
        settings_path: Some(sb.0.join("settings.json")),
        platform: Platform::current(),
        catalog_source: "http://127.0.0.1:9/never-used.json".into(),
    }
}

/// All three tools released at `version`.
fn all_released(sb: &Sandbox, version: &str) -> Backend {
    let r: Vec<(&str, &str, Vec<u8>)> = ["subak", "noeul", "ddalgak"]
        .iter()
        .map(|id| (*id, version, package(&programs(id))))
        .collect();
    backend_with(sb, &r)
}

type Started = Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>;

/// The app with a launcher that records what it would start (nothing is started).
fn app(be: Backend) -> (KimManagerApp, Started) {
    let started: Started = Arc::default();
    let mut app = KimManagerApp::new(be, None);
    let rec = started.clone();
    app.set_launcher(Arc::new(move |p: &Path, a: &[&str]| {
        rec.lock().unwrap().push((
            p.to_path_buf(),
            a.iter().map(|s| (*s).to_string()).collect(),
        ));
        Ok(())
    }));
    (app, started)
}

fn harness(app: KimManagerApp) -> Harness<'static, KimManagerApp> {
    sized(app, 368.0, 224.0)
}

fn sized(app: KimManagerApp, w: f32, h: f32) -> Harness<'static, KimManagerApp> {
    Harness::builder()
        .with_size(egui::vec2(w, h))
        .build_ui_state(|ui, app: &mut KimManagerApp| app.ui(ui), app)
}

fn wait(h: &mut Harness<'static, KimManagerApp>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)), "finishes");
    h.run();
}

fn programs_entries(paths: &Paths) -> Vec<String> {
    std::fs::read_dir(&paths.programs)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}

/// ADR 0090: the whole main view is three tiles in the order Subak, Noeul, Ddalgak from the
/// upper-left corner and the gear in the lower-right corner (also after a resize); no sidebar,
/// toolbar, tabs, table or details pane. Opening Kim installs nothing (KIM-FR-002).
#[test]
fn the_launcher_is_three_tiles_in_order_and_a_gear() {
    let sb = Sandbox::new("layout");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let (app, _) = app(be);
    let mut h = harness(app);
    h.run();
    let tiles: Vec<egui::Rect> = [
        "Subak, not installed",
        "Noeul, not installed",
        "Ddalgak, not installed",
    ]
    .iter()
    .map(|l| h.get_by_label(l).rect())
    .collect();
    assert!(tiles[0].left() < tiles[1].left() && tiles[1].left() < tiles[2].left());
    assert!(
        tiles.iter().all(|r| (r.top() - tiles[0].top()).abs() < 0.5),
        "one row"
    );
    assert!(
        tiles[0].left() < 40.0 && tiles[0].top() < 40.0,
        "from the upper-left corner"
    );
    let gear = h.get_by_label("Settings").rect();
    assert!(
        gear.right() > 330.0 && gear.bottom() > 190.0,
        "lower-right: {gear:?}"
    );
    for gone in [
        "Refresh catalog",
        "Channel",
        "Offline mode",
        "Uninstall…",
        "Features",
        "Kim",
    ] {
        assert!(
            h.query_by_label(gone).is_none(),
            "`{gone}` is not on the main view"
        );
    }
    assert!(
        programs_entries(&paths).is_empty(),
        "opening Kim installs nothing"
    );
    assert!(
        !h.state().is_busy(),
        "no network activity at launch by default"
    );
    // Resized: the gear follows the corner, the tiles stay where they are.
    h.set_size(egui::vec2(640.0, 400.0));
    h.run();
    let gear = h.get_by_label("Settings").rect();
    assert!(gear.right() > 600.0 && gear.bottom() > 365.0, "{gear:?}");
    assert_eq!(h.get_by_label("Subak, not installed").rect(), tiles[0]);
}

/// ADR 0090, KIM-SEC-001: a click on a tool that is not installed asks first; Cancel installs
/// nothing; Install installs it; the tile turns into an installed one and a click starts the
/// installed program at once, without a question.
#[test]
fn a_click_asks_before_installing_and_then_launches() {
    let sb = Sandbox::new("install");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let (app, started) = app(be);
    let mut h = harness(app);
    h.run();
    assert_eq!(h.state().tile_state("subak"), TileState::NotInstalled);
    h.get_by_label("Subak, not installed").click();
    h.run();
    assert_eq!(h.state().installing_prompt(), Some("subak"));
    assert!(h.query_by_label("Install Subak?").is_some());
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().installing_prompt().is_none());
    assert!(
        !paths.install_record("subak").exists(),
        "Cancel installs nothing"
    );

    h.get_by_label("Subak, not installed").click();
    h.run();
    h.get_by_label("Install").click();
    wait(&mut h);
    assert!(paths.install_record("subak").exists());
    assert_eq!(programs_entries(&paths), ["subak"], "only Subak");
    assert_eq!(h.state().tile_state("subak"), TileState::Installed);
    assert!(
        started.lock().unwrap().is_empty(),
        "installing starts nothing"
    );
    h.get_by_label("Subak, installed 1.0.0").click();
    h.run();
    let s = started.lock().unwrap().clone();
    assert_eq!(s.len(), 1);
    assert_eq!(
        s[0].0,
        paths
            .tool_dir("subak")
            .join("versions")
            .join("1.0.0")
            .join("subak-gui.exe")
    );
    assert!(
        h.state().installing_prompt().is_none(),
        "no confirmation to launch"
    );
}

/// ADR 0090: a tile looks installed only while the program it starts is on disk; a removed
/// program makes it a grey tile that offers a repair.
#[test]
fn a_missing_program_is_never_shown_as_installed() {
    let sb = Sandbox::new("missing");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let (app, started) = app(be);
    let mut h = harness(app);
    h.run();
    h.state_mut().start_install("noeul");
    wait(&mut h);
    assert_eq!(h.state().tile_state("noeul"), TileState::Installed);
    std::fs::remove_file(
        paths
            .tool_dir("noeul")
            .join("versions")
            .join("1.0.0")
            .join("noeulw.exe"),
    )
    .unwrap();
    h.run();
    assert_eq!(h.state().tile_state("noeul"), TileState::Damaged);
    h.get_by_label("Noeul, needs repair").click();
    h.run();
    assert!(h.query_by_label("Repair Noeul?").is_some());
    assert!(started.lock().unwrap().is_empty(), "nothing started");
    // Regression (real desktop): Repair reinstalls the same version and brings the program back.
    h.get_by_label("Repair").click();
    wait(&mut h);
    assert!(
        matches!(h.state().banner(), Some(Banner::Success(_))),
        "{:?}",
        h.state().banner()
    );
    assert_eq!(h.state().tile_state("noeul"), TileState::Installed);
}

/// ADR 0091: *Reinstall…* (right-click on an installed tool) installs the same version again
/// after the dialog, verified like any install: a changed program file is back, the data stay.
/// Cancel changes nothing and a later plain Install is not a reinstall.
#[test]
fn reinstall_puts_the_same_version_back_after_asking() {
    let sb = Sandbox::new("reinstall");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let (app, _) = app(be);
    let mut h = harness(app);
    h.run();
    h.state_mut().start_install("noeul");
    wait(&mut h);
    let program = paths
        .tool_dir("noeul")
        .join("versions")
        .join("1.0.0")
        .join("noeulw.exe");
    let original = std::fs::read(&program).unwrap();
    std::fs::write(&program, b"changed").unwrap();
    let data = paths
        .tool_dir("noeul")
        .join("..")
        .join("noeul-user-data.txt");
    std::fs::write(&data, b"mine").unwrap();
    h.get_by_label("Noeul, installed 1.0.0").click_secondary();
    h.run();
    h.get_by_label("Reinstall…").click();
    h.run();
    assert!(h.query_by_label("Reinstall Noeul?").is_some());
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().installing_prompt(), None);
    assert_eq!(
        std::fs::read(&program).unwrap(),
        b"changed",
        "Cancel changes nothing"
    );
    h.get_by_label("Noeul, installed 1.0.0").click_secondary();
    h.run();
    h.get_by_label("Reinstall…").click();
    h.run();
    h.get_by_label("Reinstall").click();
    wait(&mut h);
    assert!(
        matches!(h.state().banner(), Some(Banner::Success(_))),
        "{:?}",
        h.state().banner()
    );
    assert_eq!(
        std::fs::read(&program).unwrap(),
        original,
        "the program is back"
    );
    assert_eq!(std::fs::read(&data).unwrap(), b"mine", "data kept");
    assert_eq!(h.state().tile_state("noeul"), TileState::Installed);
}

/// ADR 0090: the right-click menu shows only what fits: not installed → Install…, Information;
/// installed → Open (Ddalgak: and its editor), Settings…, the folder, updates, Uninstall…,
/// Information. The install prompt fits the compact window.
#[test]
fn right_click_menus_fit_the_state() {
    let sb = Sandbox::new("menus");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let (app, started) = app(be);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Ddalgak, not installed").click_secondary();
    h.run();
    for item in ["Install…", "Information"] {
        assert!(h.query_by_label(item).is_some(), "not installed: {item}");
    }
    for item in ["Open Ddalgak", "Uninstall…", "Open installation folder"] {
        assert!(h.query_by_label(item).is_none(), "not installed: no {item}");
    }
    h.get_by_label("Install…").click();
    h.run();
    assert_eq!(h.state().installing_prompt(), Some("ddalgak"));
    // Regression: Ddalgak's long permission list scrolls; the button stays inside the window.
    let button = h.get_by_label("Install").rect();
    assert!(button.bottom() <= 224.0, "Install is visible: {button:?}");
    h.get_by_label("Install").click();
    wait(&mut h);

    h.get_by_label("Ddalgak, installed 1.0.0").click_secondary();
    h.run();
    for item in [
        "Open Ddalgak",
        "Start in the notification area only",
        "Settings…",
        "Open installation folder",
        "Check for updates",
        "Reinstall…",
        "Uninstall…",
        "Information",
    ] {
        assert!(h.query_by_label(item).is_some(), "installed: {item}");
    }
    h.get_by_label("Open Ddalgak").click();
    h.run();
    let v = paths.tool_dir("ddalgak").join("versions").join("1.0.0");
    let s: Vec<PathBuf> = started
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.0.clone())
        .collect();
    assert_eq!(
        s,
        [v.join("ddalgak-gui.exe"), v.join("ddalgak-editor.exe")],
        "the engine's tray app first, then the editor"
    );
    h.get_by_label("Ddalgak, installed 1.0.0").click_secondary();
    h.run();
    h.get_by_label("Open installation folder").click();
    h.run();
    let last = started.lock().unwrap().last().unwrap().clone();
    assert!(last.0.ends_with("explorer.exe"), "{last:?}");
    assert_eq!(last.1, [v.display().to_string()]);

    h.state_mut().start_install("subak");
    wait(&mut h);
    h.get_by_label("Subak, installed 1.0.0").click_secondary();
    h.run();
    assert!(h.query_by_label("Open Subak").is_some());
    h.get_by_label("Information").click();
    h.run();
    assert_eq!(h.state().info_visible(), Some("subak"));
    assert!(h.query_by_label_contains("Program files: ").is_some());
    // Regression (real desktop): the window grows for Information (and back afterwards);
    // long lines wrap or shorten, so the dialog stays inside it.
    let width = h.ctx.content_rect().width();
    assert!(width >= 580.0, "grown: {width}");
    for node in [
        h.get_by_label_contains("Program files: "),
        h.get_by_label_contains("Sessions live in a per-user background Host"),
    ] {
        let r = node.rect();
        assert!(r.left() >= 0.0 && r.right() <= width, "{r:?}");
    }
    h.key_press(egui::Key::Escape);
    h.run();
    h.run();
    assert_eq!(
        h.ctx.content_rect().width(),
        368.0,
        "back to the launcher's size"
    );
}

/// KIM-SEC-005: uninstall is behind a confirmation from the menu; keeping it changes nothing;
/// uninstalling keeps the user's data by default, the tile turns grey again, and a reinstall
/// finds the same data.
#[test]
fn uninstall_needs_confirmation_and_keeps_data() {
    let sb = Sandbox::new("uninstall");
    let be = all_released(&sb, "1.0.0");
    let paths = be.paths.clone();
    let data = paths.user_data_dirs("subak")[0].clone();
    let (app, _) = app(be);
    let mut h = harness(app);
    h.run();
    h.state_mut().start_install("subak");
    wait(&mut h);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("config.toml"), "config_version = 1\n").unwrap();

    h.get_by_label("Subak, installed 1.0.0").click_secondary();
    h.run();
    h.get_by_label("Uninstall…").click();
    h.run();
    assert_eq!(h.state().confirming(), Some("subak"));
    // Regression (real desktop): the confirmation fits the compact window and names the tool.
    assert!(h.query_by_label("Uninstall Subak?").is_some());
    for b in ["Keep installed", "Uninstall"] {
        let r = h.get_by_label(b).rect();
        assert!(
            r.left() >= 0.0 && r.right() <= 368.0 && r.bottom() <= 224.0,
            "{b}: {r:?}"
        );
    }
    h.get_by_label("Keep installed").click();
    h.run();
    assert!(paths.install_record("subak").exists());

    h.get_by_label("Subak, installed 1.0.0").click_secondary();
    h.run();
    h.get_by_label("Uninstall…").click();
    h.run();
    h.get_by_label("Uninstall").click();
    wait(&mut h);
    assert!(!paths.install_record("subak").exists());
    assert!(data.join("config.toml").exists(), "user data is kept");
    assert_eq!(h.state().tile_state("subak"), TileState::NotInstalled);
    h.state_mut().start_install("subak");
    wait(&mut h);
    assert_eq!(h.state().tile_state("subak"), TileState::Installed);
    assert!(data.join("config.toml").exists());
}

/// ADR 0090: Settings behind the gear: offline mode and update checks (Kim's settings.json,
/// off by default), the theme (manager.json, read again by a new window), the catalog and
/// About; the launcher is back unchanged after Done. Offline mode blocks installs that need
/// the network (KIM-SEC-002).
#[test]
fn settings_open_from_the_gear_and_persist() {
    let sb = Sandbox::new("settings");
    let be = all_released(&sb, "1.0.0");
    let again = be.clone();
    let (first, _) = app(be);
    let mut h = sized(first, 560.0, 460.0);
    h.run();
    h.get_by_label("Settings").click();
    h.run();
    assert!(h.state().settings_visible());
    h.get_by_label("Offline mode").click();
    h.run();
    h.get_by_label("Light").click();
    h.run();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sb.0.join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["offline_mode"], true);
    assert_eq!(saved["check_updates_on_launch"], false);
    let prefs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sb.0.join("manager.json")).unwrap()).unwrap();
    assert_eq!(prefs["theme"], "light");
    h.get_by_label("About").click();
    h.run();
    assert!(
        h.query_by_label_contains("Kim 0.").is_some(),
        "About shows the version"
    );
    h.get_by_label("Catalog").click();
    h.run();
    assert!(h.query_by_label("Refresh now").is_some());
    h.get_by_label("Done").click();
    h.run();
    assert!(!h.state().settings_visible());
    assert!(h.query_by_label("Subak, not installed").is_some());
    h.state_mut().start_install("subak");
    h.run();
    assert!(
        matches!(h.state().banner(), Some(Banner::Error { code, .. }) if code == "offline_mode"),
        "{:?}",
        h.state().banner()
    );
    let (next, _) = app(again);
    assert_eq!(
        next.prefs().theme,
        ThemePref::Light,
        "kept for the next start"
    );
}

/// A tool built for another system is grey, says why, and offers no Install button.
#[test]
fn incompatible_tools_say_why_and_offer_no_install() {
    let sb = Sandbox::new("noinstall");
    let mut be = all_released(&sb, "1.0.0");
    be.platform = Platform {
        os: "linux".into(),
        arch: "x86_64".into(),
        os_build: None,
    };
    let (app, _) = app(be);
    let mut h = sized(app, 560.0, 460.0);
    h.run();
    h.get_by_label("Subak, not available").click();
    h.run();
    assert!(
        h.query_by_label_contains("Not installable on this PC")
            .is_some()
    );
    assert!(h.query_by_label("Install").is_none());
}

/// KIM-FR-002, KIM-FR-050, KIM-FR-051, KIM-SEC-006: with the bundled catalog (no release) every
/// tile is grey and not available; Information still shows what the tool is and may do
/// (features, shortcuts, permissions, entry points, manual-only updates); nothing is created.
#[test]
fn the_bundled_catalog_shows_unreleased_tools_and_their_information() {
    let sb = Sandbox::new("bundled");
    let be = Backend {
        paths: Paths::under(&sb.0),
        keys: signature::TRUSTED_KEYS.to_vec(),
        downloader: Arc::new(kim::fetch::DefaultFetcher::default()),
        settings_path: Some(sb.0.join("settings.json")),
        platform: Platform::current(),
        catalog_source: "http://127.0.0.1:9/never-used.json".into(),
    };
    let paths = be.paths.clone();
    let (app, _) = app(be);
    let mut h = sized(app, 560.0, 1400.0);
    h.run();
    for t in [
        "Subak, not available",
        "Noeul, not available",
        "Ddalgak, not available",
    ] {
        assert!(h.query_by_label(t).is_some(), "{t}");
    }
    h.get_by_label("Noeul, not available").click_secondary();
    h.run();
    h.get_by_label("Information").click();
    h.run();
    for text in [
        "Planned · no release has been published yet",
        "In development",
        "Capture a region",
        "F1",
        "Registers system-wide shortcuts",
        "Captures the screen",
        "Can start when you sign in (only after you turn it on)",
        "noeul capture --region <x,y,w,h> --output <file> --json",
        "Manual only: Kim never installs, updates or removes Noeul by itself.",
    ] {
        assert!(
            h.query_by_label(text).is_some(),
            "noeul information: `{text}`"
        );
    }
    assert!(programs_entries(&paths).is_empty() && !paths.programs.exists());
}

/// KIM-FR-001, KIM-FR-034, KIM-FR-052, KIM-SEC-006: a released background utility discloses
/// what it may do in the install prompt before the button; installing it installs nothing
/// else; while its process runs uninstall is refused with a tray-specific hint and the process
/// is never stopped; afterwards uninstall works and keeps the user's files.
#[test]
fn a_background_utility_installs_alone_and_is_never_stopped() {
    use std::os::windows::process::CommandExt;
    let sb = Sandbox::new("noeul-release");
    let mut files = programs("noeul");
    files.push(("noeul-tray.exe", r"C:\Windows\System32\PING.EXE"));
    let be = backend_with(&sb, &[("noeul", "0.1.0", package(&files))]);
    let paths = be.paths.clone();
    let (app, _) = app(be);
    let mut h = sized(app, 560.0, 640.0);
    h.run();
    assert!(h.query_by_label("Ddalgak, not available").is_some());
    h.get_by_label("Noeul, not installed").click();
    h.run();
    assert!(
        h.query_by_label_contains("Captures the screen").is_some(),
        "disclosed first"
    );
    h.get_by_label("Install").click();
    wait(&mut h);
    assert_eq!(programs_entries(&paths), ["noeul"]);
    let layer = paths.roaming_data.join("noeul").join("hotkeys.json");
    std::fs::create_dir_all(layer.parent().unwrap()).unwrap();
    std::fs::write(&layer, "{}").unwrap();
    let tray = paths
        .tool_dir("noeul")
        .join("versions")
        .join("0.1.0")
        .join("noeul-tray.exe");
    let mut child = std::process::Command::new(&tray)
        .args(["-n", "60", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    h.get_by_label("Noeul, installed 0.1.0").click_secondary();
    h.run();
    h.get_by_label("Uninstall…").click();
    h.run();
    h.get_by_label("Uninstall").click();
    wait(&mut h);
    let refused = h.state().banner().cloned();
    let still_running = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        matches!(&refused, Some(Banner::Error { code, message })
            if code == "tool_running" && message.contains("quit it from its tray icon")),
        "{refused:?}"
    );
    assert!(
        still_running,
        "Kim never stops a running background utility"
    );
    assert!(paths.install_record("noeul").exists());
    h.state_mut().dismiss_banner();
    h.get_by_label("Noeul, installed 0.1.0").click_secondary();
    h.run();
    h.get_by_label("Uninstall…").click();
    h.run();
    h.get_by_label("Uninstall").click();
    wait(&mut h);
    assert!(!paths.install_record("noeul").exists());
    assert!(layer.exists(), "user data is kept (KIM-SEC-005)");
}

/// Kim Manager's shortcuts (HOTKEY_CONTRACT §13.2; KIM-FR-041, KIM-FR-043, KIM-FR-044,
/// KIM-SEC-001): F1 lists them, F5 refreshes, a binding changed in hotkeys.json applies when the
/// window regains focus, a destructive shortcut opens the same confirmation as the menu, and
/// the tool-wide switch turns them off. Focus backend only.
#[test]
fn keyboard_shortcuts_follow_hotkeys_json_and_never_skip_confirmation() {
    let sb = Sandbox::new("shortcuts");
    let mut be = all_released(&sb, "1.0.0");
    be.catalog_source = "ftp://never-used.invalid/catalog.json".into();
    let layer = be.paths.roaming_data.join("kim").join("hotkeys.json");
    let paths = be.paths.clone();
    let (app, _) = app(be);
    let mut h = sized(app, 560.0, 460.0);
    h.run();
    h.key_press(egui::Key::F5);
    wait(&mut h);
    assert!(
        matches!(h.state().banner(), Some(Banner::Error { code, .. }) if code == "unsupported_source"),
        "{:?}",
        h.state().banner()
    );
    h.state_mut().dismiss_banner();
    h.key_press(egui::Key::F1);
    h.run();
    assert!(h.state().shortcuts_visible());
    assert!(h.query_by_label("Refresh the catalog").is_some());
    h.key_press(egui::Key::F1);
    h.run();
    assert!(!h.state().shortcuts_visible());

    std::fs::create_dir_all(layer.parent().unwrap()).unwrap();
    std::fs::write(
        &layer,
        json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "kim",
               "overrides": [{"action": "catalog.refresh", "bindings": ["Ctrl+R"]},
                             {"action": "tool.uninstall", "bindings": ["Ctrl+Shift+U"]}]})
        .to_string(),
    )
    .unwrap();
    h.event(egui::Event::WindowFocused(true));
    h.run();
    h.key_press(egui::Key::F5);
    h.run();
    assert!(
        h.state().banner().is_none() && !h.state().is_busy(),
        "F5 no longer bound"
    );
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::R);
    wait(&mut h);
    assert!(
        matches!(h.state().banner(), Some(Banner::Error { .. })),
        "Ctrl+R refreshed"
    );
    h.state_mut().dismiss_banner();

    h.state_mut().start_install("subak");
    wait(&mut h);
    h.state_mut().select("subak");
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::U);
    h.run();
    assert_eq!(h.state().confirming(), Some("subak"));
    h.get_by_label("Keep installed").click();
    h.run();
    assert!(
        paths.install_record("subak").exists(),
        "never skips the confirmation"
    );

    std::fs::write(
        &layer,
        json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "kim", "enabled": false})
            .to_string(),
    )
    .unwrap();
    h.event(egui::Event::WindowFocused(true));
    h.run();
    h.key_press(egui::Key::F1);
    h.run();
    assert!(!h.state().shortcuts_visible(), "all shortcuts are off");
}

/// ADR 0090: Settings → Shortcuts lists Kim's actions with Change/Reset (they run `kim hotkeys`
/// next to the window; without one the buttons are disabled and say why).
#[test]
fn settings_list_the_shortcuts_with_change_buttons() {
    let sb = Sandbox::new("editor");
    let (app, _) = app(all_released(&sb, "1.0.0"));
    let mut h = sized(app, 560.0, 480.0);
    h.run();
    h.get_by_label("Settings").click();
    h.run();
    h.get_by_label("Shortcuts").click();
    h.run();
    assert!(h.query_by_label("Refresh the catalog").is_some());
    assert!(h.query_by_label("F5").is_some());
    assert!(h.query_all_by_label("Change").count() >= 5);
}

/// `--no-hotkeys` starts with every shortcut off without touching hotkeys.json (KIM-SEC-023).
#[test]
fn no_hotkeys_start_flag_turns_every_shortcut_off() {
    let sb = Sandbox::new("nohotkeys");
    let be = all_released(&sb, "1.0.0");
    let layer = be.paths.roaming_data.join("kim").join("hotkeys.json");
    let (mut app, _) = app(be);
    app.disable_hotkeys();
    let mut h = harness(app);
    h.run();
    h.key_press(egui::Key::F1);
    h.run();
    h.key_press(egui::Key::F5);
    h.run();
    assert!(!h.state().shortcuts_visible() && !h.state().is_busy());
    assert!(!layer.exists(), "the file is not touched");
}

/// ADR 0089/0090: a double-click on `kim.exe` opens the window with the local mirror next to it:
/// Subak installs from the package on disk (no network, nothing cached) and its tile then
/// starts the installed `subak-gui.exe`.
#[test]
fn a_local_mirror_installs_a_tool_that_the_tile_then_starts() {
    let sb = Sandbox::new("mirror");
    let paths = Paths::under(sb.0.join("user"));
    let mirror = sb.0.join("kim-local");
    std::fs::create_dir_all(&mirror).unwrap();
    let body = package(&programs("subak"));
    let zip_path = mirror.join("subak-0.0.1-windows-x86_64.zip");
    std::fs::write(&zip_path, &body).unwrap();
    let bundled: serde_json::Value =
        serde_json::from_slice(kim::catalog::store::BUNDLED_CATALOG).unwrap();
    let mut doc = bundled.clone();
    doc["catalog_version"] = json!(bundled["catalog_version"].as_u64().unwrap() + 1);
    for t in doc["tools"].as_array_mut().unwrap() {
        if t["id"] == "subak" {
            t["releases"] = json!([{"version": "0.0.1", "channel": "stable",
                "published_at": "2026-11-14T00:00:00Z", "release_notes": "built on this PC",
                "assets": [{"os": "windows", "arch": "x86_64", "url": kim::start::file_url(&zip_path),
                            "size": body.len(), "sha256": signature::hex(&Sha256::digest(&body)),
                            "kind": "zip", "entrypoint": "subak.exe"}]}]);
        }
    }
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    let catalog = mirror.join("catalog.json");
    std::fs::write(&catalog, &bytes).unwrap();
    std::fs::write(
        mirror.join("catalog.json.sig"),
        serde_json::to_vec(&signature::sign(&bytes, &SEED)).unwrap(),
    )
    .unwrap();
    let be = Backend {
        paths: paths.clone(),
        keys: keys(),
        downloader: Arc::new(kim::fetch::DefaultFetcher::default()),
        settings_path: Some(sb.0.join("settings.json")),
        platform: Platform::current(),
        catalog_source: "http://127.0.0.1:9/never-used.json".into(),
    };
    let (mut app, started) = app(be);
    assert_eq!(
        app.tile_state("subak"),
        TileState::Unavailable,
        "before: planned"
    );
    app.use_mirror(&kim::start::file_url(&catalog));
    assert!(
        matches!(app.banner(), Some(Banner::Info(m)) if m.contains("built on this PC")),
        "{:?}",
        app.banner()
    );
    let mut h = sized(app, 560.0, 460.0);
    h.run();
    h.get_by_label("Subak, not installed").click();
    h.run();
    assert!(
        h.query_by_label_contains("local preview, not a published release")
            .is_some()
    );
    h.get_by_label("Install").click();
    wait(&mut h);
    assert!(paths.install_record("subak").exists());
    assert!(
        !paths.catalog_cache_dir().join("catalog.json").exists(),
        "never cached"
    );
    h.get_by_label("Subak, installed 0.0.1").click();
    h.run();
    let want = paths
        .tool_dir("subak")
        .join("versions")
        .join("0.0.1")
        .join("subak-gui.exe");
    assert_eq!(started.lock().unwrap()[0].0, want);
    println!(
        "EVIDENCE local mirror: Subak 0.0.1 installed from {} and its tile started subak-gui.exe",
        zip_path.display()
    );
}

/// ADR 0089/0090: what each tile starts, and the tile order.
#[test]
fn open_starts_each_tools_own_window_program() {
    let p = |t: &str| {
        kim_manager::launches(t)
            .iter()
            .map(|l| l.program)
            .collect::<Vec<_>>()
    };
    assert_eq!(p("subak"), ["subak-gui.exe"]);
    assert_eq!(p("noeul"), ["noeulw.exe"]);
    assert_eq!(p("ddalgak"), ["ddalgak-editor.exe", "ddalgak-gui.exe"]);
    assert_eq!(
        kim_manager::launches("ddalgak")[0].also,
        ["ddalgak-gui.exe"]
    );
    assert_eq!(kim_manager::LAUNCHER_TOOLS, ["subak", "noeul", "ddalgak"]);
}

/// ADR 0090: a tile that is not installed is grey (no colour left in the icon).
#[test]
fn not_installed_icons_are_grey() {
    let mut px = vec![229, 56, 59, 255, 76, 176, 108, 128];
    kim_manager::desaturate(&mut px);
    assert!(px[0] == px[1] && px[1] == px[2] && px[4] == px[5] && px[5] == px[6]);
    assert_eq!((px[3], px[7]), (255, 128), "alpha unchanged");
}

/// ADR 0089/0090: the programs carry their icons (Explorer and the taskbar).
#[test]
fn the_programs_carry_their_icons() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let res =
        kim_winres::resources(&std::fs::read(env!("CARGO_BIN_EXE_kim-manager")).unwrap()).unwrap();
    let images =
        kim_winres::parse_ico(&std::fs::read(root.join("assets/icon/kim.ico")).unwrap()).unwrap();
    assert_eq!(
        res.iter().filter(|r| r.0 == kim_winres::RT_ICON).count(),
        images.len()
    );
    let v = &res
        .iter()
        .find(|r| r.0 == kim_winres::RT_VERSION)
        .unwrap()
        .2;
    assert_eq!(
        kim_winres::version_string(v, "FileDescription").as_deref(),
        Some("Kim Manager")
    );
    for name in ["noeul", "ddalgak"] {
        let ico = std::fs::read(root.join(format!("assets/{name}/{name}.ico"))).unwrap();
        assert_eq!(
            kim_winres::parse_ico(&ico).unwrap().len(),
            9,
            "{name}.ico sizes"
        );
    }
}

/// ADR 0090: at 150 % and 200 % display scaling the launcher keeps its layout in points: the
/// tiles in one row from the upper-left corner, the gear in the lower-right corner.
#[test]
fn the_layout_holds_at_other_display_scales() {
    for scale in [1.5_f32, 2.0] {
        let sb = Sandbox::new(&format!("dpi-{scale}"));
        let (app, _) = app(all_released(&sb, "1.0.0"));
        let mut h = Harness::builder()
            .with_size(egui::vec2(368.0, 224.0))
            .with_pixels_per_point(scale)
            .build_ui_state(|ui, app: &mut KimManagerApp| app.ui(ui), app);
        h.run();
        let s = h.get_by_label("Subak, not installed").rect();
        let d = h.get_by_label("Ddalgak, not installed").rect();
        let g = h.get_by_label("Settings").rect();
        assert!(s.left() < 40.0 && s.top() < 40.0, "{scale}: {s:?}");
        assert!(
            (d.top() - s.top()).abs() < 0.5 && d.right() < g.left(),
            "{scale}"
        );
        assert!(g.right() > 330.0 && g.bottom() > 190.0, "{scale}: {g:?}");
    }
}

/// ADR 0091: Size (Settings → General) follows the screen by default (Auto) and can be fixed;
/// the choice zooms the whole window at once, is saved in manager.json and read again by the
/// next start (`Prefs::load_beside`, what kim-manager.exe reads before its window opens).
#[test]
fn the_size_follows_the_screen_or_the_choice_and_persists() {
    // Auto: an ultrawide 5120 × 1410 → 275 %, 1920 × 1040 → 200 %, a small screen 100 %.
    assert!((kim_manager::auto_scale(egui::vec2(5120.0, 1410.0)) - 2.75).abs() < 1e-6);
    assert!((kim_manager::auto_scale(egui::vec2(1920.0, 1040.0)) - 2.0).abs() < 1e-6);
    assert!((kim_manager::auto_scale(egui::vec2(1024.0, 600.0)) - 1.0).abs() < 1e-6);
    assert!((kim_manager::auto_scale(egui::vec2(9000.0, 9000.0)) - 3.0).abs() < 1e-6);
    let sb = Sandbox::new("size");
    let be = all_released(&sb, "1.0.0");
    let again = be.clone();
    let (first, _) = app(be);
    let mut h = sized(first, 580.0, 460.0);
    let ctx = h.ctx.clone();
    h.state_mut().set_window_scale(&ctx, 2.75, 2.75);
    h.run();
    assert!((h.ctx.zoom_factor() - 2.75).abs() < 1e-6);
    h.get_by_label("Settings").click();
    h.run();
    assert!(
        h.query_by_label("Auto (275%)").is_some(),
        "Auto names its size"
    );
    h.get_by_label("150%").click();
    h.run();
    assert!((h.ctx.zoom_factor() - 1.5).abs() < 1e-6, "applied at once");
    assert!((h.state().wanted_scale() - 1.5).abs() < 1e-6);
    let prefs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sb.0.join("manager.json")).unwrap()).unwrap();
    assert_eq!(prefs["scale"], 150);
    let loaded = Prefs::load_beside(&sb.0.join("settings.json"));
    assert_eq!(loaded.scale, Some(150));
    assert!((loaded.scale_or(2.75) - 1.5).abs() < 1e-6);
    let (next, _) = app(again);
    assert_eq!(next.prefs().scale, Some(150), "kept for the next start");
    h.get_by_label("Auto (275%)").click();
    // The window takes its size once the new zoom is in effect (a few frames).
    h.run_steps(8);
    assert!((h.ctx.zoom_factor() - 2.75).abs() < 1e-6, "Auto again");
    let prefs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sb.0.join("manager.json")).unwrap()).unwrap();
    assert!(prefs["scale"].is_null());
    // Out-of-range values in a hand-edited file stay within 100–300 %.
    let odd = Prefs {
        scale: Some(900),
        ..Prefs::default()
    };
    assert!((odd.scale_or(1.0) - 3.0).abs() < 1e-6);
}
