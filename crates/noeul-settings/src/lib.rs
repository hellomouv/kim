//! Noeul's settings window (ADR 0093; NOEUL-FR-101, NOEUL-FR-103, NOEUL-FR-111,
//! NOEUL-NFR-012): the Kim design system (`kim-ui`) in a program of its own, started by Noeul
//! when the user opens Settings. Noeul itself links no GUI framework (ADR 0023); this window
//! exists only while it is open.
//!
//! **Every change is the documented `noeul` command** (`config set`, `hotkeys set|disable|
//! reset|import|export`, `pin group …`), run on a worker thread through [`Runner`]: the window
//! and the CLI share one code path with one set of checks, and `config set` makes the running
//! instance read the file again. The settings model is Noeul's own ([`noeul::prefs::Model`]).
//!
//! Sections: **General** (the tray icon and its clicks, the language), **Capture** (detection,
//! magnifier, pointer, the double-click and middle-click inside a selection, keyboard help,
//! quick-save folder), **Pins** (double-click a pin, the backup, pin groups), **Shortcuts**
//! (every action's keys: click a field and press keys, or pick keys; Off, Reset, Reset all,
//! Import, Export), **Appearance** (this window's theme, pin outline and shadow) and
//! **Advanced** (history, privacy, output, paste, colour picker). While keys are recorded the
//! instance ignores its global hotkeys (`noeul hotkeys pause`), so `F1` can be recorded.

use eframe::egui;
use kim_ui::keys::{KeyPicker, Picked};
use kim_ui::{Button, Lang, LangPref, Theme, ThemePref, Tone};
use noeul::prefs::{Key, Model};
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Runs one `noeul` command and returns its `--json` document; asks the running instance to
/// pause its global hotkeys while keys are recorded.
pub trait Runner: Send + Sync {
    fn run(&self, args: &[String]) -> Value;
    /// `hotkeys.pause` over Noeul's IPC: `seconds` > 0 releases the hotkeys for that long, 0
    /// registers them again (nothing when Noeul does not run).
    fn pause_hotkeys(&self, seconds: u64);
}

/// The real runner: `noeul.exe` next to this program, headless (`--json --no-input`, no console
/// window), with `--endpoint` when the instance answers on another pipe.
#[derive(Debug, Clone)]
pub struct NoeulCli {
    pub exe: std::path::PathBuf,
    pub endpoint: Option<String>,
}

impl Runner for NoeulCli {
    fn run(&self, args: &[String]) -> Value {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.args(args)
            .args(["--json", "--no-input"])
            .stdin(std::process::Stdio::null());
        if let Some(e) = &self.endpoint {
            cmd.args(["--endpoint", e]);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.output() {
            Ok(o) => serde_json::from_slice(&o.stdout).unwrap_or_else(|_| {
                json!({"ok": false, "error": {"code": "io", "message": "noeul.exe gave no answer"}})
            }),
            Err(e) => json!({"ok": false, "error": {"code": "io", "message": e.to_string()}}),
        }
    }

    #[cfg(windows)]
    fn pause_hotkeys(&self, seconds: u64) {
        let endpoint = match &self.endpoint {
            Some(e) => e.clone(),
            None => match noeul::win::pipe::default_endpoint() {
                Ok(e) => e,
                Err(_) => return,
            },
        };
        // The client verifies that the pipe is served by this user's noeul.exe first.
        if let Ok(mut s) = noeul::win::client::open(&endpoint) {
            let _ = s.request("hotkeys.pause", json!({ "seconds": seconds }));
        }
    }

    #[cfg(not(windows))]
    fn pause_hotkeys(&self, _seconds: u64) {}
}

/// The sections, in the order of the rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Capture,
    Pins,
    Shortcuts,
    Appearance,
    Advanced,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::General,
        Section::Capture,
        Section::Pins,
        Section::Shortcuts,
        Section::Appearance,
        Section::Advanced,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Capture => "Capture",
            Section::Pins => "Pins",
            Section::Shortcuts => "Shortcuts",
            Section::Appearance => "Appearance",
            Section::Advanced => "Advanced",
        }
    }
}

/// How a setting is edited.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// A switch writing `on` / `off` (or `true` / `false`).
    Switch { on: &'static str, off: &'static str },
    /// One of a few values: `(value, label)`.
    Choice(&'static [(&'static str, &'static str)]),
    /// A whole number within a range.
    Number { min: i64, max: i64 },
    /// An absolute folder path (empty: the default).
    Folder,
}

/// One setting of `config.toml`.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    /// The `noeul config set` key.
    pub key: &'static str,
    /// Where `config show` reports the value.
    pub path: &'static [&'static str],
    /// The English name (titles stay English, ADR 0092).
    pub title: &'static str,
    pub en: &'static str,
    pub ko: &'static str,
    pub kind: Kind,
}

const ON_OFF: Kind = Kind::Switch {
    on: "on",
    off: "off",
};

/// Every setting the window offers, by section and group.
pub fn groups(s: Section) -> Vec<(&'static str, Vec<Setting>)> {
    match s {
        Section::General => vec![(
            "Notification area",
            vec![
                Setting {
                    key: "general.tray_icon",
                    path: &["tray_icon"],
                    title: "Tray icon",
                    en: "Shows Noeul in the notification area (from the next start).",
                    ko: "알림 영역에 Noeul 아이콘을 표시합니다 (다음 시작부터).",
                    kind: Kind::Switch {
                        on: "true",
                        off: "false",
                    },
                },
                Setting {
                    key: "general.tray_left_click",
                    path: &["tray_clicks", "left"],
                    title: "Left click on the icon",
                    en: "What a click on the tray icon does.",
                    ko: "트레이 아이콘을 클릭했을 때 할 일입니다.",
                    kind: Kind::Choice(TRAY),
                },
                Setting {
                    key: "general.tray_middle_click",
                    path: &["tray_clicks", "middle"],
                    title: "Middle click on the icon",
                    en: "What a middle click on the tray icon does.",
                    ko: "트레이 아이콘을 가운데 클릭했을 때 할 일입니다.",
                    kind: Kind::Choice(TRAY),
                },
            ],
        )],
        Section::Capture => vec![
            (
                "Selecting",
                vec![
                    Setting {
                        key: "capture.detection",
                        path: &["detection"],
                        title: "Detect",
                        en: "What the overlay outlines under the pointer before you drag.",
                        ko: "드래그하기 전에 포인터 아래에서 자동으로 잡을 대상입니다.",
                        kind: Kind::Choice(&[
                            ("window", "Windows"),
                            ("element", "Elements"),
                            ("off", "Off"),
                        ]),
                    },
                    Setting {
                        key: "capture.magnifier",
                        path: &["magnifier"],
                        title: "Magnifier",
                        en: "Shows the pixels around the pointer while you select.",
                        ko: "선택하는 동안 포인터 주변 픽셀을 확대해 보여 줍니다.",
                        kind: ON_OFF,
                    },
                    Setting {
                        key: "capture.pointer",
                        path: &["pointer"],
                        title: "Include the pointer",
                        en: "Key-started captures include the mouse pointer.",
                        ko: "단축키로 시작한 캡처에 마우스 포인터를 함께 담습니다.",
                        kind: ON_OFF,
                    },
                ],
            ),
            (
                "Inside a selection",
                vec![
                    Setting {
                        key: "gestures.double_click",
                        path: &["gestures", "double_click"],
                        title: "Double-click",
                        en: "Pin is Noeul's default: the selection floats on top at once.",
                        ko: "기본값은 Pin입니다. 선택 영역이 바로 화면 위에 고정됩니다.",
                        kind: Kind::Choice(&[
                            ("pin", "Pin"),
                            ("copy", "Copy"),
                            ("save", "Save"),
                            ("none", "Nothing"),
                        ]),
                    },
                    Setting {
                        key: "gestures.middle_click",
                        path: &["gestures", "middle_click"],
                        title: "Middle click",
                        en: "A middle click inside the selection.",
                        ko: "선택 영역 안을 가운데 클릭했을 때 할 일입니다.",
                        kind: Kind::Choice(&[
                            ("pin", "Pin"),
                            ("copy", "Copy"),
                            ("none", "Nothing"),
                        ]),
                    },
                ],
            ),
            (
                "Help and saving",
                vec![
                    Setting {
                        key: "capture.keyboard_help",
                        path: &["keyboard_help"],
                        title: "Keyboard help",
                        en: "The overlay's list of keys.",
                        ko: "캡처 화면에 표시하는 키 안내입니다.",
                        kind: Kind::Choice(&[
                            ("first-run", "First time"),
                            ("always", "Always"),
                            ("never", "Never"),
                        ]),
                    },
                    Setting {
                        key: "capture.quick_save_folder",
                        path: &["quick_save_folder"],
                        title: "Quick save folder",
                        en: "Where quick save writes. Empty: Pictures\\Noeul.",
                        ko: "빠른 저장 위치입니다. 비우면 사진\\Noeul 폴더를 씁니다.",
                        kind: Kind::Folder,
                    },
                ],
            ),
        ],
        Section::Pins => vec![(
            "Pins",
            vec![
                Setting {
                    key: "pins.double_click",
                    path: &["pins", "double_click"],
                    title: "Double-click a pin",
                    en: "Close closes only that pin. A double-click inside a selection still makes a pin.",
                    ko: "Close는 그 고정 이미지만 닫습니다. 선택 영역 안 더블클릭은 그대로 고정 이미지를 만듭니다.",
                    kind: Kind::Choice(&[("close", "Close"), ("none", "Nothing")]),
                },
                Setting {
                    key: "pins.backup",
                    path: &["pins", "backup"],
                    title: "Back up pins",
                    en: "Restores your pins after a restart.",
                    ko: "다시 시작한 뒤 고정 이미지를 복원합니다.",
                    kind: ON_OFF,
                },
            ],
        )],
        Section::Appearance => vec![(
            "Pins",
            vec![
                Setting {
                    key: "pins.border",
                    path: &["pins", "border"],
                    title: "Outline",
                    en: "A thin outline around every pin.",
                    ko: "고정 이미지마다 얇은 테두리를 그립니다.",
                    kind: ON_OFF,
                },
                Setting {
                    key: "pins.shadow",
                    path: &["pins", "shadow"],
                    title: "Shadow",
                    en: "A soft shadow under every pin.",
                    ko: "고정 이미지 아래에 부드러운 그림자를 그립니다.",
                    kind: ON_OFF,
                },
            ],
        )],
        Section::Advanced => vec![
            (
                "History",
                vec![
                    Setting {
                        key: "history.count",
                        path: &["history", "count"],
                        title: "Captures kept",
                        en: "How many recent captures the history keeps (1–100).",
                        ko: "기록에 남길 최근 캡처 수입니다 (1–100).",
                        kind: Kind::Number { min: 1, max: 100 },
                    },
                    Setting {
                        key: "history.memory_mb",
                        path: &["history", "memory_mb"],
                        title: "Memory limit (MB)",
                        en: "The most memory the history may use (16–2048).",
                        ko: "기록이 쓸 수 있는 최대 메모리입니다 (16–2048).",
                        kind: Kind::Number { min: 16, max: 2048 },
                    },
                    Setting {
                        key: "history.keep_days",
                        path: &["history", "keep_days"],
                        title: "Days kept",
                        en: "How long captures stay in the history (1–365).",
                        ko: "캡처를 기록에 보관하는 기간입니다 (1–365일).",
                        kind: Kind::Number { min: 1, max: 365 },
                    },
                    Setting {
                        key: "history.disk",
                        path: &["history", "disk"],
                        title: "Keep on disk",
                        en: "The history survives a restart (stored in your profile).",
                        ko: "기록을 디스크에 저장해 다시 시작해도 남깁니다.",
                        kind: ON_OFF,
                    },
                    Setting {
                        key: "history.keep_cancelled",
                        path: &["history", "keep_cancelled"],
                        title: "Keep cancelled captures",
                        en: "A capture you cancel with Esc is kept too.",
                        ko: "Esc로 취소한 캡처도 기록에 남깁니다.",
                        kind: ON_OFF,
                    },
                    Setting {
                        key: "history.auto_save",
                        path: &["history", "auto_save"],
                        title: "Save every capture",
                        en: "Every capture is also written to the quick save folder.",
                        ko: "모든 캡처를 빠른 저장 폴더에도 저장합니다.",
                        kind: ON_OFF,
                    },
                ],
            ),
            (
                "Privacy",
                vec![
                    Setting {
                        key: "privacy.cli",
                        path: &["privacy", "cli"],
                        title: "Captures from the command line",
                        en: "Notify shows a notice each time; Deny refuses them.",
                        ko: "Notify는 매번 알림을 띄우고, Deny는 거부합니다.",
                        kind: Kind::Choice(&[
                            ("notify", "Notify"),
                            ("allow", "Allow"),
                            ("deny", "Deny"),
                        ]),
                    },
                    Setting {
                        key: "privacy.mcp_captures",
                        path: &["privacy", "mcp_captures"],
                        title: "Captures by AI tools (MCP)",
                        en: "Off by default: tools must ask you.",
                        ko: "기본값은 꺼짐입니다. AI 도구가 캡처하려면 허락이 필요합니다.",
                        kind: ON_OFF,
                    },
                ],
            ),
            (
                "Files and clipboard",
                vec![
                    Setting {
                        key: "output.format",
                        path: &["output", "format"],
                        title: "File format",
                        en: "The format of saved captures.",
                        ko: "저장하는 캡처의 파일 형식입니다.",
                        kind: Kind::Choice(&[("png", "PNG"), ("jpeg", "JPEG")]),
                    },
                    Setting {
                        key: "output.jpeg_quality",
                        path: &["output", "jpeg_quality"],
                        title: "JPEG quality",
                        en: "1–100; higher is sharper and larger.",
                        ko: "1–100. 높을수록 선명하고 파일이 커집니다.",
                        kind: Kind::Number { min: 1, max: 100 },
                    },
                    Setting {
                        key: "output.copy_as_file",
                        path: &["output", "copy_as_file"],
                        title: "Copy as a file too",
                        en: "Copy also puts a PNG file on the clipboard.",
                        ko: "복사할 때 PNG 파일도 클립보드에 넣습니다.",
                        kind: ON_OFF,
                    },
                    Setting {
                        key: "paste.paths",
                        path: &["paste", "paths"],
                        title: "Paste image paths",
                        en: "A copied path to an image file pins the file.",
                        ko: "복사한 이미지 파일 경로를 붙여 넣으면 그 파일을 고정합니다.",
                        kind: ON_OFF,
                    },
                    Setting {
                        key: "paste.file_cards",
                        path: &["paste", "file_cards"],
                        title: "File cards",
                        en: "Copied non-image files become a card of their names.",
                        ko: "이미지가 아닌 파일은 이름 카드로 고정합니다.",
                        kind: ON_OFF,
                    },
                ],
            ),
            (
                "Colour picker",
                vec![
                    Setting {
                        key: "color.format",
                        path: &["color", "format"],
                        title: "Colour format",
                        en: "How the colour picker writes colours.",
                        ko: "색 선택기가 색을 적는 형식입니다.",
                        kind: Kind::Choice(&[("hex", "HEX"), ("rgb", "RGB"), ("hsl", "HSL")]),
                    },
                    Setting {
                        key: "color.hex_case",
                        path: &["color", "hex_case"],
                        title: "HEX letters",
                        en: "Upper or lower case in HEX colours.",
                        ko: "HEX 색 코드의 대소문자입니다.",
                        kind: Kind::Choice(&[("upper", "ABC"), ("lower", "abc")]),
                    },
                    Setting {
                        key: "color.alpha",
                        path: &["color", "alpha"],
                        title: "Include alpha",
                        en: "Adds the opacity to copied colours.",
                        ko: "복사한 색에 불투명도를 포함합니다.",
                        kind: ON_OFF,
                    },
                ],
            ),
        ],
        Section::Shortcuts => Vec::new(),
    }
}

/// The file that keeps this window's theme (next to Noeul's `config.toml`).
pub const UI_FILE: &str = "settings-window.json";

const TRAY: &[(&str, &str)] = &[
    ("capture", "Capture"),
    ("pin_clipboard", "Pin clipboard"),
    ("settings", "Settings"),
    ("none", "Nothing"),
];

/// A command's answer, back from the worker thread.
type Answer = (Vec<String>, Value);

/// The window's state.
pub struct SettingsApp {
    runner: Arc<dyn Runner>,
    model: Model,
    config: Value,
    section: Section,
    theme: ThemePref,
    applied: Option<Theme>,
    themed: bool,
    notice: Option<(Tone, String)>,
    tx: Sender<Answer>,
    rx: Receiver<Answer>,
    pending: usize,
    ctx: Arc<Mutex<Option<egui::Context>>>,
    /// The action being recorded, and since when (the pause is renewed while it lasts).
    recording: Option<(String, Instant)>,
    picker: Option<(String, KeyPicker)>,
    /// Edits of number and folder fields, by key, until they are applied.
    edits: std::collections::HashMap<&'static str, String>,
    confirm_reset_all: bool,
    lang_override: Option<Lang>,
    /// `settings-window.json` in Noeul's config folder: this window's own theme.
    ui_file: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for SettingsApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsApp")
            .field("section", &self.section)
            .finish_non_exhaustive()
    }
}

impl SettingsApp {
    /// Reads the settings and the shortcuts once (two quick commands) and opens on General.
    pub fn new(runner: Arc<dyn Runner>) -> SettingsApp {
        let (tx, rx) = channel();
        let mut app = SettingsApp {
            runner,
            model: Model::default(),
            config: Value::Null,
            section: Section::General,
            theme: ThemePref::System,
            applied: None,
            themed: false,
            notice: None,
            tx,
            rx,
            pending: 0,
            ctx: Arc::new(Mutex::new(None)),
            recording: None,
            picker: None,
            edits: Default::default(),
            confirm_reset_all: false,
            lang_override: None,
            ui_file: None,
        };
        app.reload();
        app
    }

    /// Forces the language (tests; `--lang`).
    pub fn set_lang(&mut self, l: Lang) {
        self.lang_override = Some(l);
    }

    /// The theme of this window (Appearance → Theme).
    pub fn set_theme(&mut self, t: ThemePref) {
        self.theme = t;
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn status(&self) -> Option<&(Tone, String)> {
        self.notice.as_ref()
    }

    pub fn config(&self) -> &Value {
        &self.config
    }

    /// The key picker's chord so far, while it is open (tests).
    pub fn picked(&self) -> Option<Option<String>> {
        self.picker.as_ref().map(|(_, p)| p.chord())
    }

    pub fn recording(&self) -> Option<&str> {
        self.recording.as_ref().map(|r| r.0.as_str())
    }

    /// The language of descriptions: Noeul's `general.language`, or Windows' language.
    pub fn lang(&self) -> Lang {
        if let Some(l) = self.lang_override {
            return l;
        }
        let v = self.value(&["language"]);
        LangPref::parse(&v).unwrap_or_default().resolve()
    }

    fn reload(&mut self) {
        let hk = self.runner.run(&["hotkeys".into(), "list".into()]);
        let cfg = self.runner.run(&["config".into(), "show".into()]);
        let groups = self
            .runner
            .run(&["pin".into(), "group".into(), "list".into()]);
        let fresh = Model::from_docs(&hk["data"], &cfg["data"]);
        self.model.refresh(fresh);
        if groups["ok"] == true {
            self.model.set_groups(&groups["data"]);
        } else {
            self.model.groups.clear();
        }
        if self.ui_file.is_none()
            && let Some(dir) = cfg["data"]["paths"]["config_dir"].as_str()
        {
            let f = std::path::Path::new(dir).join(UI_FILE);
            if let Some(t) = std::fs::read(&f)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v["theme"].as_str().and_then(ThemePref::parse))
            {
                self.theme = t;
            }
            self.ui_file = Some(f);
        }
        if cfg["ok"] == true {
            self.config = cfg["data"]["settings"].clone();
        } else {
            let m = cfg["error"]["message"]
                .as_str()
                .unwrap_or("noeul.exe could not be asked")
                .to_string();
            self.notice = Some((Tone::Error, m));
        }
    }

    /// The value `config show` reports at `path`, as text.
    pub fn value(&self, path: &[&str]) -> String {
        let mut v = &self.config;
        for p in path {
            v = &v[*p];
        }
        match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        }
    }

    /// Runs a command on a worker thread; its answer reloads the window.
    fn start(&mut self, args: Vec<String>) {
        let runner = self.runner.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.pending += 1;
        std::thread::spawn(move || {
            let doc = runner.run(&args);
            let _ = tx.send((args, doc));
            if let Ok(c) = ctx.lock()
                && let Some(c) = c.as_ref()
            {
                c.request_repaint();
            }
        });
    }

    /// Waits until every started command answered (tests).
    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while self.pending > 0 && Instant::now() < end {
            if let Ok(a) = self.rx.recv_timeout(Duration::from_millis(20)) {
                self.answered(a);
            }
        }
        self.pending == 0
    }

    fn poll(&mut self) {
        while let Ok(a) = self.rx.try_recv() {
            self.answered(a);
        }
    }

    fn answered(&mut self, (args, doc): Answer) {
        self.pending = self.pending.saturating_sub(1);
        self.model.answer(&args, &doc);
        let lang = self.lang();
        self.notice = Some(if doc["ok"] == true {
            let saved = lang.pick("Saved.", "저장했습니다.").to_string();
            let text = if self.model.status == "Saved." || self.model.status == "Done." {
                saved
            } else {
                self.model.status.clone()
            };
            (Tone::Success, text)
        } else {
            (Tone::Error, self.model.status.clone())
        });
        if args.get(1).map(String::as_str) == Some("export") && doc["ok"] == true {
            self.export_done(&doc);
        }
        self.reload();
    }

    fn set(&mut self, key: &str, value: &str) {
        self.start(vec![
            "config".into(),
            "set".into(),
            key.into(),
            value.into(),
        ]);
    }

    /// Draws one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Ok(mut c) = self.ctx.lock()
            && c.is_none()
        {
            *c = Some(ctx.clone());
        }
        if !self.themed {
            kim_ui::apply(&ctx, kim_ui::Options::default());
            self.themed = true;
        }
        let want = self.theme.resolve();
        if self.applied != Some(want) {
            kim_ui::set_theme(&ctx, want);
            self.applied = Some(want);
        }
        self.poll();
        self.record_keys(&ctx);
        let c = kim_ui::Colors::get(&ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.bg_base))
            .show(ui, |ui| self.layout(ui));
        if self.picker.is_some() {
            self.picker_dialog(&ctx);
        }
        if self.confirm_reset_all {
            self.reset_all_dialog(&ctx);
        }
    }

    fn layout(&mut self, ui: &mut egui::Ui) {
        use kim_design::{size, space};
        let c = kim_ui::Colors::get(ui.ctx());
        let full = ui.max_rect();
        let nav = egui::Rect::from_min_size(
            full.min,
            egui::vec2(size::SETTINGS_NAV_WIDTH, full.height()),
        );
        ui.painter().rect_filled(nav, 0.0, c.bg_raised);
        let mut rail = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(nav.shrink2(egui::vec2(space::S, space::L)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        rail.horizontal(|ui| {
            ui.add_space(space::XS);
            kim_ui::title(ui, "Noeul");
        });
        rail.add_space(space::M);
        rail.spacing_mut().item_spacing.y = 2.0;
        for s in Section::ALL {
            if kim_ui::nav_item(&mut rail, self.section == s, s.title()).clicked() {
                self.section = s;
                self.stop_recording();
            }
        }
        let body = egui::Rect::from_min_max(egui::pos2(nav.right(), full.top()), full.max);
        let mut page = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body.shrink2(egui::vec2(space::L + 4.0, space::L)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut page, |ui| {
                ui.set_width(ui.available_width() - space::XS);
                kim_ui::title(ui, self.section.title());
                ui.add_space(space::S);
                match self.section {
                    Section::Shortcuts => self.shortcuts(ui),
                    Section::Pins => {
                        self.settings_groups(ui);
                        self.pin_groups(ui);
                    }
                    Section::Appearance => {
                        self.window_look(ui);
                        self.settings_groups(ui);
                    }
                    Section::Advanced => {
                        self.settings_groups(ui);
                        self.advanced_actions(ui);
                    }
                    _ => self.settings_groups(ui),
                }
                if let Some((tone, text)) = self.notice.clone()
                    && kim_ui::notice(ui, tone, &text, None, Some("Dismiss"))
                {
                    self.notice = None;
                }
            });
    }

    fn settings_groups(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        for (title, items) in groups(self.section) {
            kim_ui::setting_group(ui, title, |ui| {
                for (i, s) in items.iter().enumerate() {
                    if i > 0 {
                        kim_ui::row_divider(ui);
                    }
                    self.setting(ui, s, lang);
                }
            });
        }
    }

    fn setting(&mut self, ui: &mut egui::Ui, s: &Setting, lang: Lang) {
        let current = self.value(s.path);
        let desc = lang.pick(s.en, s.ko);
        match s.kind {
            Kind::Switch { on, off } => {
                let mut v = current == on || current == "true";
                if ui
                    .add(kim_ui::SwitchRow::new(&mut v, s.title).description(desc))
                    .changed()
                {
                    self.set(s.key, if v { on } else { off });
                }
            }
            Kind::Choice(options) => {
                let before = options.iter().position(|(v, _)| *v == current);
                let mut idx = before.unwrap_or(usize::MAX);
                let changed = kim_ui::setting_row(ui, s.title, Some(desc), |ui| {
                    if options.len() <= 3 {
                        let refs: Vec<(usize, &str)> = options
                            .iter()
                            .enumerate()
                            .map(|(i, (_, l))| (i, *l))
                            .collect();
                        kim_ui::segmented(ui, &mut idx, &refs)
                    } else {
                        let opts: Vec<(usize, String)> = options
                            .iter()
                            .enumerate()
                            .map(|(i, (_, l))| (i, (*l).to_string()))
                            .collect();
                        kim_ui::dropdown(ui, s.key, s.title, &mut idx, &opts)
                    }
                });
                if changed && let Some((value, _)) = options.get(idx) {
                    self.set(s.key, value);
                }
            }
            Kind::Number { min, max } => {
                let mut text = self
                    .edits
                    .get(s.key)
                    .cloned()
                    .unwrap_or_else(|| current.clone());
                let mut apply = None;
                kim_ui::setting_row(ui, s.title, Some(desc), |ui| {
                    let r = kim_ui::text_field(ui, &mut text, s.title, "", 72.0);
                    if r.lost_focus()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter)) && r.has_focus()
                    {
                        apply = Some(text.clone());
                    }
                });
                match text.trim().parse::<i64>() {
                    Ok(n) if (min..=max).contains(&n) => {}
                    _ if text.trim().is_empty() => {}
                    _ => {
                        kim_ui::field_error(
                            ui,
                            &format!(
                                "{} {min}–{max}",
                                lang.pick(
                                    "Enter a whole number from",
                                    "다음 범위의 정수를 입력하세요:"
                                )
                            ),
                        );
                    }
                }
                self.edits.insert(s.key, text.clone());
                if let Some(t) = apply {
                    self.edits.remove(s.key);
                    if let Ok(n) = t.trim().parse::<i64>()
                        && (min..=max).contains(&n)
                        && t.trim() != current
                    {
                        self.set(s.key, &n.to_string());
                    }
                }
            }
            Kind::Folder => {
                let mut text = self
                    .edits
                    .get(s.key)
                    .cloned()
                    .unwrap_or_else(|| current.clone());
                let mut apply = false;
                kim_ui::setting_stack(ui, s.title, Some(desc), |ui| {
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 80.0).max(120.0);
                        let r = kim_ui::text_field(ui, &mut text, s.title, "Pictures\\Noeul", w);
                        if r.lost_focus() && text != current {
                            apply = true;
                        }
                        if ui.add(Button::secondary("Apply").compact()).clicked() {
                            apply = true;
                        }
                    });
                });
                let bad =
                    !text.trim().is_empty() && !std::path::Path::new(text.trim()).is_absolute();
                if bad {
                    kim_ui::field_error(
                        ui,
                        lang.pick(
                            "Use a full path, like D:\\Captures.",
                            "D:\\Captures처럼 전체 경로를 쓰세요.",
                        ),
                    );
                }
                self.edits.insert(s.key, text.clone());
                if apply && !bad && !text.trim().is_empty() && text != current {
                    self.edits.remove(s.key);
                    self.set(s.key, text.trim());
                }
            }
        }
    }

    fn window_look(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let lang_value = self.value(&["language"]);
        kim_ui::setting_group(ui, "Window", |ui| {
            kim_ui::setting_row(
                ui,
                "Theme",
                Some(lang.pick(
                    "This window follows Windows, or is always dark or light.",
                    "이 창의 테마입니다. Windows 설정을 따르거나 항상 어둡게 또는 밝게 표시합니다.",
                )),
                |ui| {
                    if kim_ui::segmented(
                        ui,
                        &mut self.theme,
                        &[
                            (ThemePref::System, "System"),
                            (ThemePref::Dark, "Dark"),
                            (ThemePref::Light, "Light"),
                        ],
                    ) && let Some(f) = &self.ui_file
                    {
                        let doc = json!({ "theme": self.theme.as_str() });
                        let _ = std::fs::create_dir_all(f.parent().unwrap_or(f));
                        let _ = std::fs::write(f, doc.to_string());
                    }
                },
            );
            kim_ui::row_divider(ui);
            let mut v = if lang_value.is_empty() {
                "auto".to_string()
            } else {
                lang_value.clone()
            };
            let system = format!("System ({})", Lang::system().native_name());
            let mut pick = v.as_str();
            let changed = kim_ui::setting_row(
                ui,
                "Language",
                Some(lang.pick(
                    "Descriptions, menus and messages; names stay English.",
                    "설명, 메뉴, 안내 메시지의 언어입니다. 이름은 영어로 표시됩니다.",
                )),
                |ui| {
                    kim_ui::segmented(
                        ui,
                        &mut pick,
                        &[
                            ("auto", system.as_str()),
                            ("en", "English"),
                            ("ko", "한국어"),
                        ],
                    )
                },
            );
            if changed {
                v = pick.to_string();
                self.set("general.language", &v);
            }
        });
    }

    fn pin_groups(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        if self.model.groups.is_empty() {
            ui.label(kim_ui::caption(
                ui,
                lang.pick(
                    "Pin groups appear here while Noeul runs.",
                    "Noeul이 실행 중일 때 고정 이미지 그룹이 여기에 표시됩니다.",
                ),
            ));
            return;
        }
        let mut cmd = None;
        let groups = self.model.groups.clone();
        kim_ui::setting_group(ui, "Groups", |ui| {
            for (i, g) in groups.iter().enumerate() {
                if i > 0 {
                    kim_ui::row_divider(ui);
                }
                let mut d = match lang {
                    Lang::Ko => format!("고정 이미지 {}개", g.pins),
                    Lang::En => format!("{} pins", g.pins),
                };
                if g.hidden > 0 {
                    d.push_str(&format!(", {} {}", g.hidden, lang.pick("hidden", "숨김")));
                }
                if g.current {
                    d.push_str(lang.pick(", current", ", 현재 그룹"));
                }
                kim_ui::setting_row(ui, &g.name, Some(&d), |ui| {
                    for (label, a) in [
                        ("Close", noeul::prefs::GroupAction::Close),
                        ("Hide", noeul::prefs::GroupAction::Hide),
                        ("Show", noeul::prefs::GroupAction::Show),
                        ("Switch", noeul::prefs::GroupAction::Switch),
                    ] {
                        let b = if label == "Close" {
                            Button::danger(label).compact()
                        } else {
                            Button::ghost(label).compact()
                        };
                        if ui.add(b).clicked() {
                            cmd = Some((i, a));
                        }
                    }
                });
            }
        });
        if let Some((i, a)) = cmd {
            self.model.group_selected = Some(i);
            if let Some(args) = self.model.group_command(a) {
                self.start(args);
            }
        }
    }

    fn advanced_actions(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let path = self.value(&["path"]);
        ui.label(kim_ui::caption(
            ui,
            format!("{} {path}", lang.pick("Settings file:", "설정 파일:")),
        ));
    }

    // --- Shortcuts -------------------------------------------------------------------------

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let rows = self.model.rows.clone();
        let recording = self.recording().map(str::to_string);
        let mut act: Option<(usize, &'static str)> = None;
        ui.label(kim_ui::caption(
            ui,
            lang.pick(
                "Click a field and press the new keys, or use Pick to choose keys such as PrintScreen. They do not run while you record.",
                "칸을 클릭한 뒤 새 키를 누르거나, Pick으로 PrintScreen 같은 키를 고르세요. 녹화하는 동안에는 키가 실행되지 않습니다.",
            ),
        ));
        ui.add_space(kim_design::space::S);
        let press = lang
            .pick("Press keys… (Esc cancels)", "키를 누르세요… (Esc 취소)")
            .to_string();
        let none = lang.pick("Off", "꺼짐").to_string();
        kim_ui::setting_group(ui, "Actions", |ui| {
            for (i, r) in rows.iter().enumerate() {
                if i > 0 {
                    kim_ui::row_divider(ui);
                }
                let is_rec = recording.as_deref() == Some(r.id.as_str());
                let desc = if r.changed {
                    Some(lang.pick("Changed", "변경됨"))
                } else if !r.bindable {
                    Some(lang.pick("Fixed", "고정"))
                } else {
                    None
                };
                kim_ui::setting_row(ui, &r.title, desc, |ui| {
                    if r.bindable {
                        if ui.add(Button::ghost("Reset").compact()).clicked() {
                            act = Some((i, "reset"));
                        }
                        if ui.add(Button::ghost("Off").compact()).clicked() {
                            act = Some((i, "disable"));
                        }
                        if ui.add(Button::ghost("Pick").compact()).clicked() {
                            act = Some((i, "pick"));
                        }
                    }
                    let field = kim_ui::shortcut_field(
                        ui,
                        &r.title,
                        &r.keys.join(", "),
                        is_rec,
                        &none,
                        &press,
                        140.0,
                    );
                    if r.bindable && field.clicked() && !is_rec {
                        act = Some((i, "record"));
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            if ui.add(Button::secondary("Import…")).clicked() {
                self.import();
            }
            if ui.add(Button::secondary("Export…")).clicked() {
                self.start(Model::export());
            }
            if ui.add(Button::danger("Reset all…")).clicked() {
                self.confirm_reset_all = true;
            }
        });
        if let Some(force) = self.model.pending.clone()
            && ui.add(Button::secondary("Use anyway")).clicked()
        {
            self.model.pending = None;
            self.start(force);
        }
        if let Some((i, what)) = act {
            self.model.selected = Some(i);
            match what {
                "record" => {
                    if self.model.start_recording() {
                        let id = self.model.recording.clone().unwrap_or_default();
                        self.recording = Some((id, Instant::now()));
                        self.pause_hotkeys();
                    }
                }
                "pick" => {
                    let id = rows[i].id.clone();
                    self.stop_recording();
                    self.picker = Some((id, KeyPicker::default()));
                }
                "disable" => {
                    if let Some(c) = self.model.disable() {
                        self.start(c);
                    }
                }
                _ => {
                    if let Some(c) = self.model.reset() {
                        self.start(c);
                    }
                }
            }
        }
    }

    /// Asks the instance to pause (`seconds` > 0) or resume (0) its hotkeys, off the UI thread.
    fn pause(&mut self, seconds: u64) {
        let runner = self.runner.clone();
        std::thread::spawn(move || runner.pause_hotkeys(seconds));
    }

    fn pause_hotkeys(&mut self) {
        self.pause(30);
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            self.model.recording = None;
            self.pause(0);
        }
    }

    /// While recording, every key goes to the recorder (none reaches the window's shortcuts).
    fn record_keys(&mut self, ctx: &egui::Context) {
        let Some((_, since)) = self.recording.clone() else {
            return;
        };
        if ctx.input(|i| !i.focused) {
            self.stop_recording();
            return;
        }
        // The pause lasts 30 s; a long recording renews it.
        if since.elapsed() > Duration::from_secs(20) {
            if let Some(r) = self.recording.as_mut() {
                r.1 = Instant::now();
            }
            self.pause_hotkeys();
        }
        let mut result = None;
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if result.is_none() {
                        let name = kim_ui::keys::key_name(*key);
                        result = Some(self.model.key(
                            name,
                            modifiers.ctrl,
                            modifiers.alt,
                            modifiers.shift,
                        ));
                    }
                    false
                }
                _ => true,
            });
        });
        match result {
            Some(Key::Bind(cmd)) => {
                self.recording = None;
                self.start(cmd);
                self.pause(0);
            }
            Some(Key::Cancelled) => {
                self.recording = None;
                self.pause(0);
                self.notice = Some((Tone::Info, self.model.status.clone()));
            }
            _ => {}
        }
    }

    fn picker_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let Some((id, mut picker)) = self.picker.take() else {
            return;
        };
        let title = self
            .model
            .rows
            .iter()
            .find(|r| r.id == id)
            .map_or_else(|| id.clone(), |r| r.title.clone());
        let mut picked = Picked::None;
        let r = kim_ui::dialog(
            ctx,
            "noeul-key-picker",
            &format!("Pick keys: {title}"),
            |ui| {
                picked = picker.ui(ui, kim_hotkeys_keys(), false, lang);
            },
        );
        match picked {
            Picked::Confirm(chord) => {
                self.start(vec!["hotkeys".into(), "set".into(), id, chord]);
            }
            Picked::Cancel => {}
            Picked::None if r.should_close() => {}
            Picked::None => self.picker = Some((id, picker)),
        }
    }

    fn reset_all_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let mut go = false;
        let mut close = false;
        let r = kim_ui::dialog(ctx, "noeul-reset-all", "Reset all shortcuts?", |ui| {
            ui.label(kim_ui::secondary(
                ui,
                lang.pick(
                    "Every shortcut goes back to Noeul's default. A backup of your file is kept.",
                    "모든 단축키를 Noeul 기본값으로 되돌립니다. 기존 파일은 백업해 둡니다.",
                ),
            ));
            kim_ui::dialog_actions(ui, |ui| {
                if ui.add(Button::danger("Reset all")).clicked() {
                    go = true;
                }
                if ui.add(Button::secondary("Cancel")).clicked() {
                    close = true;
                }
            });
        });
        if go {
            self.start(Model::reset_all());
        }
        if go || close || r.should_close() {
            self.confirm_reset_all = false;
        }
    }

    fn import(&mut self) {
        if let Some(path) = files::pick(true) {
            self.start(Model::import(&path));
        }
    }

    fn export_done(&mut self, doc: &Value) {
        if let Some(path) = files::pick(false) {
            let text = Model::export_text(doc);
            let lang = self.lang();
            self.notice = Some(match std::fs::write(&path, text) {
                Ok(()) => (
                    Tone::Success,
                    format!("{} {path}", lang.pick("Exported to", "내보냈습니다:")),
                ),
                Err(e) => (Tone::Error, e.to_string()),
            });
        }
    }
}

/// Every key name a shortcut may use (the picker's list).
fn kim_hotkeys_keys() -> &'static [&'static str] {
    kim_hotkeys::keys::NAMED_KEYS
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        SettingsApp::ui(self, ui);
    }
}

/// The Open and Save dialogs for Import and Export (`*.json`).
pub mod files {
    /// The chosen path, or `None` (cancelled, or no dialog in tests).
    #[cfg(windows)]
    #[allow(unsafe_code)]
    pub fn pick(open: bool) -> Option<String> {
        if std::env::var_os("NOEUL_SETTINGS_NO_DIALOGS").is_some() {
            return None;
        }
        use windows_sys::Win32::UI::Controls::Dialogs::{
            GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
        };
        let filter: Vec<u16> = "Shortcuts (*.json)\0*.json\0All files\0*.*\0\0"
            .encode_utf16()
            .collect();
        let ext: Vec<u16> = "json\0".encode_utf16().collect();
        let mut buf = vec![0u16; 1024];
        // SAFETY: OPENFILENAMEW is plain data; all-zero is its documented empty state.
        let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrDefExt = ext.as_ptr();
        ofn.Flags = OFN_PATHMUSTEXIST
            | if open {
                OFN_FILEMUSTEXIST
            } else {
                OFN_OVERWRITEPROMPT
            };
        // SAFETY: `ofn` points to buffers that live across the call; the dialog is modal.
        let ok = unsafe {
            if open {
                GetOpenFileNameW(&raw mut ofn)
            } else {
                GetSaveFileNameW(&raw mut ofn)
            }
        };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    #[cfg(not(windows))]
    pub fn pick(_open: bool) -> Option<String> {
        None
    }
}

/// ADR 0091: the window's zoom on a screen whose work area is `w` × `h` points (680 × 560
/// within 90 %, at most 200 %, in steps of 25 %).
pub fn window_scale(w: f32, h: f32) -> f32 {
    let z = (w * 0.9 / 680.0).min(h * 0.9 / 560.0);
    ((z * 4.0).floor() / 4.0).clamp(1.0, 2.0)
}
