//! Kim Manager GUI (egui + glow, ADR 0014). A thin, explicit front-end over the `kim` library:
//! every install / update / uninstall starts only from a button the user pressed, runs on a
//! worker thread with visible progress and a Cancel button, and never touches a running program.
//! The window can be closed at any time; nothing keeps running in the background afterwards.
//! Keyboard shortcuts (F5 refresh, F1 shortcut list, and any the user binds with
//! `kim hotkeys set`) are read from the window's own key events only ([`shortcuts`]).

pub mod shortcuts;
pub mod theme;

use eframe::egui;
use kim::catalog::details::{self, Details};
use kim::catalog::signature::TrustedKey;
use kim::catalog::status::{self, Availability, ToolStatus, ToolView};
use kim::catalog::store::{self, Loaded};
use kim::catalog::{Channel, EntryStatus};
use kim::error::KimError;
use kim::fetch::{DefaultFetcher, Downloader};
use kim::install::{self, Event, Phase, PlanOptions};
use kim::paths::Paths;
use kim::platform::Platform;
use kim::settings::Settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Everything the UI needs from the outside world; tests inject temp paths, test keys and a
/// loopback downloader.
#[derive(Clone)]
pub struct Backend {
    pub paths: Paths,
    pub keys: Vec<TrustedKey>,
    pub downloader: Arc<dyn Downloader + Send + Sync>,
    pub settings_path: Option<PathBuf>,
    pub platform: Platform,
    /// Source for the explicit "Refresh catalog" action.
    pub catalog_source: String,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl Backend {
    /// The real per-user environment.
    pub fn real() -> Result<Self, KimError> {
        Ok(Self {
            paths: Paths::from_env()?,
            keys: kim::catalog::signature::TRUSTED_KEYS.to_vec(),
            downloader: Arc::new(DefaultFetcher::default()),
            settings_path: Settings::path().ok(),
            platform: Platform::current(),
            catalog_source: store::DEFAULT_SOURCE.into(),
        })
    }
}

/// ADR 0089: starts a program of an installed tool: `(program, arguments)`. Tests record the
/// call instead of starting anything.
pub type Launcher = Arc<dyn Fn(&Path, &[&str]) -> std::io::Result<()> + Send + Sync>;

/// One program the details panel offers to start for an installed tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launch {
    /// The button.
    pub label: &'static str,
    /// The program in the installed version's folder.
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// ADR 0090: programs of the same version started first (a tool's background part, which
    /// keeps a single instance of itself; a second start leaves the running one alone).
    pub also: &'static [&'static str],
    /// ADR 0090: what the status line says after the start (empty: "Started …").
    pub hint: &'static str,
}

/// ADR 0089/0090: what each installed tool's tile and menu start (its window, or its
/// notification-area app). Only programs of the installed version are started, only when the
/// user clicks, and Kim Manager never stops them. The first entry is the tile's left-click.
pub fn launches(tool: &str) -> &'static [Launch] {
    const SUBAK: &[Launch] = &[Launch {
        label: "Open Subak",
        program: "subak-gui.exe",
        args: &[],
        also: &[],
        hint: "",
    }];
    const NOEUL: &[Launch] = &[Launch {
        label: "Start Noeul",
        program: "noeulw.exe",
        args: &[],
        also: &[],
        hint: "Noeul runs in the notification area: F1 captures a region, F3 pins the clipboard.",
    }];
    const DDALGAK: &[Launch] = &[
        Launch {
            label: "Open Ddalgak",
            program: "ddalgak-editor.exe",
            args: &[],
            also: &["ddalgak-gui.exe"],
            hint: "Ddalgak runs in the notification area (its engine stays off until you turn it on); the rule editor is open.",
        },
        Launch {
            label: "Start in the notification area only",
            program: "ddalgak-gui.exe",
            args: &[],
            also: &[],
            hint: "Ddalgak runs in the notification area; its engine stays off until you turn it on.",
        },
    ];
    match tool {
        "subak" => SUBAK,
        "noeul" => NOEUL,
        "ddalgak" => DDALGAK,
        _ => &[],
    }
}

/// The real [`Launcher`]: a detached start in the program's own folder, without a console.
pub fn real_launcher() -> Launcher {
    Arc::new(|program: &Path, args: &[&str]| {
        let mut c = std::process::Command::new(program);
        c.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = program.parent() {
            c.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: GUI programs show their own windows
        }
        c.spawn().map(|_| ())
    })
}

/// Messages from worker threads to the UI.
#[derive(Debug)]
enum Msg {
    Progress(Event),
    Finished(Result<String, KimError>),
    Refreshed(Result<String, KimError>),
    /// ADR 0090: a result for the status line (e.g. Noeul's settings window).
    Notice(Banner),
}

#[derive(Debug)]
struct Running {
    tool: String,
    verb: &'static str,
    phase: Phase,
    done: u64,
    total: u64,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Banner {
    Info(String),
    Success(String),
    Error { message: String, code: String },
}

#[derive(Debug, Default)]
struct ConfirmUninstall {
    tool: String,
    purge: bool,
    understood: bool,
}

pub struct KimManagerApp {
    backend: Backend,
    loaded: Option<Loaded>,
    views: Vec<ToolView>,
    /// Metadata and Kim's own facts per tool, computed on (re)load, never per frame.
    details: HashMap<String, Details>,
    load_error: Option<String>,
    selected: Option<String>,
    channel: Channel,
    settings: Settings,
    running: Option<Running>,
    refreshing: bool,
    confirm: Option<ConfirmUninstall>,
    banner: Option<Banner>,
    shortcuts: shortcuts::Shortcuts,
    show_shortcuts: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: Option<egui::Context>,
    /// ADR 0089: a local mirror (`--catalog file://…`) used instead of the cache this session.
    mirror: Option<String>,
    launcher: Launcher,
    // ADR 0090: the launcher.
    install_prompt: Option<String>,
    /// *Reinstall…* was chosen for this tool: the install dialog installs the same version again.
    reinstall: Option<String>,
    info: Option<String>,
    show_settings: bool,
    settings_tab: SettingsTab,
    /// The action whose shortcut is being changed (Settings → Shortcuts).
    capture: Option<String>,
    textures: HashMap<(String, bool), egui::TextureHandle>,
    prefs: Prefs,
    light: Option<bool>,
    launcher_size: Option<egui::Vec2>,
    /// ADR 0091: the size chosen for this screen (Settings → General → Size: Auto) and the
    /// zoom last applied to the window (`None` in tests: no real window to scale).
    auto_scale: f32,
    zoom: Option<f32>,
    /// The real window (Windows: its HWND), where it was before it grew to stay on the screen
    /// (pixels; it moves back when Settings or Information closes), and how many more frames
    /// to check that it is within the screen's work area after a resize.
    hwnd: Option<isize>,
    moved_from: Option<(i32, i32)>,
    screen_check: u8,
    /// After a new size: the size in points to give the window once the zoom is in effect.
    resize_to: Option<egui::Vec2>,
    /// The banner on the status line and when it was first shown (it fades).
    banner_at: Option<(String, f64)>,
}

impl std::fmt::Debug for KimManagerApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimManagerApp")
            .field("selected", &self.selected)
            .field("banner", &self.banner)
            .finish_non_exhaustive()
    }
}

impl KimManagerApp {
    pub fn new(backend: Backend, ctx: Option<&egui::Context>) -> Self {
        let (tx, rx) = channel();
        let settings = backend
            .settings_path
            .as_deref()
            .map(|p| Settings::load_from(p).0)
            .unwrap_or_default();
        let shortcuts = shortcuts::Shortcuts::load(&backend.paths, kim_hotkeys::no_hotkeys_env());
        let mut app = Self {
            shortcuts,
            show_shortcuts: false,
            backend,
            loaded: None,
            views: Vec::new(),
            details: HashMap::new(),
            load_error: None,
            selected: None,
            channel: Channel::Stable,
            settings,
            running: None,
            refreshing: false,
            confirm: None,
            banner: None,
            tx,
            rx,
            ctx: ctx.cloned(),
            mirror: None,
            launcher: real_launcher(),
            install_prompt: None,
            reinstall: None,
            info: None,
            show_settings: false,
            settings_tab: SettingsTab::default(),
            capture: None,
            textures: HashMap::new(),
            prefs: Prefs::default(),
            light: None,
            launcher_size: None,
            auto_scale: 1.0,
            zoom: None,
            hwnd: None,
            moved_from: None,
            screen_check: 0,
            resize_to: None,
            banner_at: None,
        };
        app.prefs = app
            .prefs_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Some(ctx) = ctx {
            theme::apply(ctx);
        }
        app.reload();
        // Launch-time check only if the user enabled it (off by default, KIM-SEC-002). It only
        // refreshes the catalog to *show* updates; it never downloads or installs a tool.
        if app.settings.check_updates_on_launch && !app.settings.offline_mode {
            app.refresh_catalog();
        }
        app
    }

    /// ADR 0089: use the local mirror at `url` (a `file://` catalog of packages built on this
    /// PC) for this session: it is verified like a refresh and never written to the cache.
    pub fn use_mirror(&mut self, url: &str) {
        self.mirror = Some(url.to_string());
        self.backend.catalog_source = url.to_string();
        self.reload();
        if let Some(l) = &self.loaded
            && l.source == store::Source::Mirror
        {
            self.banner = Some(Banner::Info(format!(
                "Using the local packages built on this PC (catalog v{}).",
                l.catalog.catalog_version
            )));
        }
    }

    /// Replaces how programs are started (tests).
    pub fn set_launcher(&mut self, launcher: Launcher) {
        self.launcher = launcher;
    }

    /// ADR 0089: starts `program` of the installed `tool` (the user pressed its button).
    pub fn open(&mut self, tool: &str, launch: &Launch) {
        let Some(version) = self
            .views
            .iter()
            .find(|v| v.id == tool)
            .and_then(|v| v.installed_version.clone())
        else {
            return;
        };
        let path = self
            .backend
            .paths
            .tool_dir(tool)
            .join("versions")
            .join(&version)
            .join(launch.program);
        let name = self
            .views
            .iter()
            .find(|v| v.id == tool)
            .map_or(tool.to_string(), |v| v.name.clone());
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let missing = std::iter::once(launch.program)
            .chain(launch.also.iter().copied())
            .find(|p| !dir.join(p).is_file());
        self.banner = Some(if let Some(m) = missing {
            Banner::Error {
                message: format!("{name} {version} has no {m}; repair or reinstall it"),
                code: "not_found".into(),
            }
        } else {
            let started = launch
                .also
                .iter()
                .try_for_each(|p| (self.launcher)(&dir.join(p), &[]))
                .and_then(|()| (self.launcher)(&path, launch.args));
            match started {
                Ok(()) if !launch.hint.is_empty() => Banner::Success(launch.hint.to_string()),
                Ok(()) => Banner::Success(format!("Started {name}.")),
                Err(e) => Banner::Error {
                    message: format!("cannot start {}: {e}", path.display()),
                    code: "io".into(),
                },
            }
        });
    }

    pub fn banner(&self) -> Option<&Banner> {
        self.banner.as_ref()
    }

    /// What the banner's Dismiss button does.
    pub fn dismiss_banner(&mut self) {
        self.banner = None;
    }

    /// Starts with every shortcut off (`--no-hotkeys`), without touching `hotkeys.json`.
    pub fn disable_hotkeys(&mut self) {
        self.shortcuts = shortcuts::Shortcuts::load(&self.backend.paths, true);
    }

    /// Whether the keyboard shortcut list is open (F1 by default).
    pub fn shortcuts_visible(&self) -> bool {
        self.show_shortcuts
    }

    /// Whether an uninstall confirmation is open.
    pub fn confirming(&self) -> Option<&str> {
        self.confirm.as_ref().map(|c| c.tool.as_str())
    }

    /// Runs one shortcut action exactly like its button or menu entry: the same checks and the
    /// same confirmation (a shortcut never skips consent, KIM-SEC-001).
    fn run_action(&mut self, action: &str) {
        let busy = self.is_busy();
        let view = self
            .selected
            .as_ref()
            .and_then(|id| self.views.iter().find(|v| &v.id == id))
            .filter(|v| v.id != "kim")
            .cloned();
        match action {
            "help.show" => self.show_shortcuts = !self.show_shortcuts,
            _ if self.show_shortcuts || busy => {}
            "catalog.refresh" => self.refresh_catalog(),
            "tool.install" | "tool.update" => {
                let want = if action == "tool.install" {
                    ToolStatus::Available
                } else {
                    ToolStatus::UpdateAvailable
                };
                if let Some(v) = view.filter(|v| v.status == want) {
                    self.start_install(&v.id);
                }
            }
            "tool.uninstall" => {
                if let Some(v) = view.filter(|v| {
                    matches!(
                        v.status,
                        ToolStatus::Installed | ToolStatus::UpdateAvailable | ToolStatus::Damaged
                    )
                }) {
                    self.confirm = Some(ConfirmUninstall {
                        tool: v.id,
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        if events
            .iter()
            .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        {
            // `kim hotkeys set` may have changed the file while the window was in the background,
            // and a terminal may have installed a tool or changed its shortcuts (an event, no
            // polling; reading is cheap and changes nothing).
            self.shortcuts.reload();
            if !self.is_busy() {
                self.reload();
            }
        }
        let actions = self.shortcuts.handle(&events);
        if self.confirm.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        for a in actions {
            self.run_action(&a);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.running.is_some() || self.refreshing
    }

    pub fn views(&self) -> &[ToolView] {
        &self.views
    }

    /// Selects a tool in the list, as clicking its row does.
    pub fn select(&mut self, id: &str) {
        self.selected = Some(id.to_string());
        self.banner = None;
    }

    /// What the details panel shows about `id` besides its status.
    pub fn tool_details(&self, id: &str) -> Option<&Details> {
        self.details.get(id)
    }

    fn reload(&mut self) {
        let loaded = store::load(
            &self.backend.paths,
            &self.backend.keys,
            kim::time::now_utc(),
        )
        .map(|l| match &self.mirror {
            // ADR 0089: the local mirror replaces the cache for this session; if it cannot be
            // used, the trusted catalog is shown with the reason.
            Some(url) => match store::mirror(
                &self.backend.paths,
                &self.backend.keys,
                kim::time::now_utc(),
                url,
                &DefaultFetcher::default(),
            ) {
                Ok(m) => m,
                Err(e) => {
                    self.banner = Some(Banner::Error {
                        message: format!("the local packages cannot be used: {}", e.message),
                        code: e.code.as_str().into(),
                    });
                    l
                }
            },
            None => l,
        });
        match loaded {
            Ok(l) => {
                self.views = status::evaluate(
                    &l.catalog,
                    &self.backend.paths,
                    &self.backend.platform,
                    self.channel,
                );
                self.details = l
                    .catalog
                    .tools
                    .iter()
                    .map(|t| (t.id.clone(), details::details(&self.backend.paths, t)))
                    .collect();
                if self.selected.is_none() {
                    self.selected = self
                        .views
                        .iter()
                        .find(|v| v.id != "kim")
                        .map(|v| v.id.clone());
                }
                self.load_error = None;
                self.loaded = Some(l);
            }
            Err(e) => self.load_error = Some(e.to_string()),
        }
    }

    fn refresh_catalog(&mut self) {
        if self.mirror.is_some() {
            // A local mirror is read again from disk, never through the network or the cache.
            self.reload();
            if let Some(l) = &self.loaded
                && l.source == store::Source::Mirror
            {
                self.banner = Some(Banner::Info(format!(
                    "Read the local packages again (catalog v{}).",
                    l.catalog.catalog_version
                )));
            }
            return;
        }
        if let Err(e) = self.settings.ensure_online() {
            self.banner = Some(Banner::Error {
                message: e.message,
                code: e.code.as_str().into(),
            });
            return;
        }
        self.refreshing = true;
        self.banner = Some(Banner::Info("Refreshing the catalog…".into()));
        let b = self.backend.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = store::refresh(
                &b.paths,
                &b.keys,
                kim::time::now_utc(),
                &b.catalog_source,
                &DefaultFetcher::default(),
            )
            .map(|o| {
                if o.updated {
                    format!("Catalog updated to v{}.", o.fetched_version)
                } else {
                    format!("Catalog is up to date (v{}).", o.fetched_version)
                }
            });
            let _ = tx.send(Msg::Refreshed(r));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Starts an explicit install or update of `tool` (the user pressed the button).
    pub fn start_install(&mut self, tool: &str) {
        let Some(loaded) = &self.loaded else { return };
        let local = loaded.source == store::Source::Mirror;
        if !local && let Err(e) = self.settings.ensure_online() {
            self.banner = Some(Banner::Error {
                message: e.message,
                code: e.code.as_str().into(),
            });
            return;
        }
        // ADR 0090: a repair (the record is fine but the program the tile starts is missing)
        // installs the same version again, verified like any install.
        let opts = PlanOptions {
            channel: Some(self.channel),
            reinstall: self.tile_state(tool) == TileState::Damaged
                || self.reinstall.as_deref() == Some(tool),
            ..PlanOptions::default()
        };
        self.reinstall = None;
        let plan = match install::plan_install(
            loaded,
            &self.backend.paths,
            &self.backend.platform,
            tool,
            &opts,
        ) {
            Ok(p) => p,
            Err(e) => {
                self.banner = Some(Banner::Error {
                    message: e.message,
                    code: e.code.as_str().into(),
                });
                return;
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let verb = if plan.from.is_some() {
            "Updating"
        } else {
            "Installing"
        };
        self.running = Some(Running {
            tool: tool.into(),
            verb,
            phase: Phase::Locking,
            done: 0,
            total: plan.asset.size,
            cancel: cancel.clone(),
        });
        self.banner = None;
        let name = self.tool_name(tool);
        let b = self.backend.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let txp = tx.clone();
            let ctxp = ctx.clone();
            let result = install::execute(
                &plan,
                &b.paths,
                b.downloader.as_ref(),
                &mut |e| {
                    let _ = txp.send(Msg::Progress(e));
                    if let Some(c) = &ctxp {
                        c.request_repaint();
                    }
                },
                &cancel,
            )
            .map(|o| {
                let mut s = format!("{name} {} is installed.", o.to);
                if !o.running.is_empty() {
                    s.push_str(" The running copy was not stopped; the new version is used next time it starts.");
                }
                s
            });
            let _ = tx.send(Msg::Finished(result));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    pub fn cancel(&mut self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn start_uninstall(&mut self, tool: &str, purge: bool) {
        let plan = match install::plan_uninstall(&self.backend.paths, tool, purge) {
            // How the tool runs (e.g. a tray utility) gives the refusal for a running tool a
            // precise hint; Kim never stops it.
            Ok(p) => match &self.loaded {
                Some(l) => p.with_catalog(&l.catalog),
                None => p,
            },
            Err(e) => {
                self.banner = Some(Banner::Error {
                    message: e.message,
                    code: e.code.as_str().into(),
                });
                return;
            }
        };
        self.running = Some(Running {
            tool: tool.into(),
            verb: "Uninstalling",
            phase: Phase::Activating,
            done: 0,
            total: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let name = self.tool_name(tool);
        let b = self.backend.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = install::execute_uninstall(&plan, &b.paths).map(|o| {
                if o.kept_data.is_empty() {
                    format!("{name} was uninstalled.")
                } else {
                    format!("{name} was uninstalled. Your settings and data were kept.")
                }
            });
            let _ = tx.send(Msg::Finished(result));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Progress(Event::Phase { phase }) => {
                    if let Some(r) = &mut self.running {
                        r.phase = phase;
                    }
                }
                Msg::Progress(Event::Progress { done, total }) => {
                    if let Some(r) = &mut self.running {
                        r.done = done;
                        r.total = total;
                    }
                }
                Msg::Finished(result) => {
                    self.running = None;
                    self.banner = Some(match result {
                        Ok(m) => Banner::Success(m),
                        Err(e) => Banner::Error {
                            message: match &e.hint {
                                Some(h) => format!("{}. {h}.", e.message),
                                None => e.message,
                            },
                            code: e.code.as_str().into(),
                        },
                    });
                    self.reload();
                }
                Msg::Notice(b) => self.set_banner(b),
                Msg::Refreshed(result) => {
                    self.refreshing = false;
                    self.banner = Some(match result {
                        Ok(m) => Banner::Info(m),
                        Err(e) => Banner::Error {
                            message: e.message,
                            code: e.code.as_str().into(),
                        },
                    });
                    self.reload();
                }
            }
        }
    }

    /// Blocks until background work finishes (tests).
    pub fn wait_idle(&mut self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        while self.is_busy() {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.poll();
        }
        true
    }

    /// `manager.json` next to Kim's settings file: the window's own look (ADR 0090).
    fn prefs_path(&self) -> Option<PathBuf> {
        self.backend
            .settings_path
            .as_ref()
            .map(|p| p.with_file_name("manager.json"))
    }

    /// The theme choice.
    pub fn prefs(&self) -> Prefs {
        self.prefs
    }

    fn save_prefs(&mut self) {
        let Some(p) = self.prefs_path() else { return };
        let text = serde_json::to_string_pretty(&self.prefs).unwrap_or_default()
            + "
";
        let r = p
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&p, text));
        if let Err(e) = r {
            self.set_banner(Banner::Error {
                message: format!("cannot save {}: {e}", p.display()),
                code: "io".into(),
            });
        }
    }

    fn save_settings(&mut self) {
        if let Some(p) = &self.backend.settings_path
            && let Err(e) = self.settings.save_to(p)
        {
            self.banner = Some(Banner::Error {
                message: e.message,
                code: e.code.as_str().into(),
            });
        }
    }

    // --- UI (ADR 0090: the launcher) --------------------------------------------------------

    /// What the launcher shows for `id`, from the installed inventory and the catalog: installed
    /// only when Kim's install record is valid **and** the program the tile starts is on disk.
    pub fn tile_state(&self, id: &str) -> TileState {
        let Some(v) = self.views.iter().find(|v| v.id == id) else {
            return TileState::Unavailable;
        };
        let program_present = || {
            let (Some(version), Some(first)) = (&v.installed_version, launches(id).first()) else {
                return false;
            };
            self.backend
                .paths
                .tool_dir(id)
                .join("versions")
                .join(version)
                .join(first.program)
                .is_file()
        };
        match v.status {
            ToolStatus::Installed if program_present() => TileState::Installed,
            ToolStatus::UpdateAvailable if program_present() => TileState::Update,
            ToolStatus::Installed | ToolStatus::UpdateAvailable | ToolStatus::Damaged => {
                TileState::Damaged
            }
            ToolStatus::Available => TileState::NotInstalled,
            ToolStatus::Incompatible | ToolStatus::Unreleased => TileState::Unavailable,
        }
    }

    /// The tile's accessible name: the tool and its state in words.
    pub fn tile_label(&self, id: &str) -> String {
        let name = self.tool_name(id);
        let v = self.views.iter().find(|v| v.id == id);
        let installed = v
            .and_then(|v| v.installed_version.clone())
            .unwrap_or_default();
        match self.tile_state(id) {
            TileState::Installed => format!("{name}, installed {installed}"),
            TileState::Update => format!(
                "{name}, installed {installed}, update {} available",
                v.and_then(|v| v.latest_compatible.clone())
                    .unwrap_or_default()
            ),
            TileState::NotInstalled => format!("{name}, not installed"),
            TileState::Damaged => format!("{name}, needs repair"),
            TileState::Unavailable => format!("{name}, not available"),
        }
    }

    fn tool_name(&self, id: &str) -> String {
        self.views
            .iter()
            .find(|v| v.id == id)
            .map_or_else(|| id.to_string(), |v| v.name.clone())
    }

    /// Whether the Settings panel is open.
    pub fn settings_visible(&self) -> bool {
        self.show_settings
    }

    /// The tool whose install confirmation is open.
    pub fn installing_prompt(&self) -> Option<&str> {
        self.install_prompt.as_deref()
    }

    /// The tool whose information is open.
    pub fn info_visible(&self) -> Option<&str> {
        self.info.as_deref()
    }

    /// Opens or closes Settings (the gear does this).
    pub fn toggle_settings(&mut self, _ctx: Option<&egui::Context>) {
        self.show_settings = !self.show_settings;
        self.capture = None;
    }

    /// A compact window grows while Settings or Information is open and returns to its size
    /// afterwards.
    fn fit_window(&mut self, ctx: &egui::Context) {
        let big = self.show_settings || self.info.is_some();
        if big == self.launcher_size.is_some() {
            return;
        }
        if big {
            let now = ctx.content_rect().size();
            self.launcher_size = Some(now);
            let want = egui::vec2(now.x.max(580.0), now.y.max(460.0));
            if want != now {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
                self.screen_check = 10;
            }
        } else if let Some(size) = self.launcher_size.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            #[cfg(windows)]
            if let (Some(h), Some(p)) = (self.hwnd, self.moved_from.take()) {
                screen::move_to(h, p);
            }
        }
    }

    /// ADR 0091: for a few frames after the window grew, a window that now reaches past the
    /// bottom or right edge of its screen's work area moves up or left into it (and back when it
    /// shrinks again). Read from Windows: egui's own idea of the position lags a zoom change.
    fn keep_on_screen(&mut self, ctx: &egui::Context) {
        if self.screen_check == 0 || self.hwnd.is_none() {
            self.screen_check = 0;
            return;
        }
        self.screen_check -= 1;
        ctx.request_repaint();
        #[cfg(windows)]
        if let Some(h) = self.hwnd
            && let Some(from) = screen::fit(h)
        {
            self.moved_from.get_or_insert(from);
        }
    }

    /// A left-click on a tile: start an installed tool at once; otherwise ask before installing.
    pub fn activate(&mut self, id: &str) {
        self.selected = Some(id.to_string());
        match self.tile_state(id) {
            TileState::Installed | TileState::Update => {
                if let Some(first) = launches(id).first() {
                    self.open(id, first);
                }
            }
            TileState::NotInstalled | TileState::Damaged | TileState::Unavailable => {
                self.install_prompt = Some(id.to_string());
            }
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let light = match self.prefs.theme {
            ThemePref::Light => true,
            ThemePref::Dark => false,
            ThemePref::System => ctx.input(|i| i.raw.system_theme) == Some(egui::Theme::Light),
        };
        if self.light != Some(light) {
            self.light = Some(light);
            theme::set_light(ctx, light);
        }
    }

    /// ADR 0091: the window shows everything `zoom` times larger (it was created at that size;
    /// `auto` is what *Size: Auto* means on this screen).
    pub fn set_window_scale(&mut self, ctx: &egui::Context, auto: f32, zoom: f32) {
        self.auto_scale = auto;
        self.zoom = Some(zoom);
        ctx.set_zoom_factor(zoom);
    }

    /// The zoom Settings → General → Size asks for.
    pub fn wanted_scale(&self) -> f32 {
        self.prefs.scale_or(self.auto_scale)
    }

    /// A new size from Settings: everything grows or shrinks, the window with it.
    fn apply_scale(&mut self, ctx: &egui::Context) {
        let Some(zoom) = self.zoom else { return };
        // The window keeps its size in points: sent once the new zoom is in effect (a size
        // sent in the same frame would still be converted with the old one).
        if let Some(points) = self.resize_to {
            let native = ctx.input(|i| i.viewport().native_pixels_per_point.unwrap_or(1.0));
            if (ctx.pixels_per_point() - native * zoom).abs() < 0.01 {
                self.resize_to = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                    MIN_SIZE[0],
                    MIN_SIZE[1],
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(points));
                self.screen_check = 10;
            } else {
                ctx.request_repaint();
            }
        }
        let want = self.wanted_scale();
        if (want - zoom).abs() < f32::EPSILON {
            return;
        }
        if self.resize_to.is_none() {
            self.resize_to = Some(ctx.content_rect().size());
        }
        self.zoom = Some(want);
        ctx.set_zoom_factor(want);
        ctx.request_repaint();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.apply_scale(&ctx);
        self.fit_window(&ctx);
        self.keep_on_screen(&ctx);
        if self.capture.is_some() {
            self.capture_key(&ctx);
        } else {
            self.handle_shortcuts(&ctx);
        }
        let c = theme::Colors::get();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c.bg_base))
            .show(ui, |ui| {
                if self.show_settings {
                    self.settings_panel(ui);
                } else {
                    self.launcher(ui);
                }
            });
        if self.confirm.is_some() {
            self.confirm_dialog(&ctx);
        }
        if self.install_prompt.is_some() {
            self.install_dialog(&ctx);
        }
        if self.info.is_some() {
            self.info_dialog(&ctx);
        }
        if self.show_shortcuts {
            self.shortcuts_dialog(&ctx);
        }
    }

    /// The whole main view: three tiles from the upper-left corner, the gear in the
    /// lower-right corner and, only while there is something to say, one quiet line of text.
    fn launcher(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        let full = ui.max_rect();
        const MARGIN: f32 = 24.0;
        const TILE: egui::Vec2 = egui::vec2(88.0, 100.0);
        const GAP: f32 = 8.0;
        if let Some(e) = &self.load_error {
            ui.painter().text(
                full.left_top() + egui::vec2(MARGIN, MARGIN),
                egui::Align2::LEFT_TOP,
                format!("Kim cannot read its catalog: {e}"),
                egui::FontId::proportional(12.0),
                c.danger,
            );
        }
        let mut clicked = None;
        let mut menu_action = None;
        for (i, id) in LAUNCHER_TOOLS.iter().enumerate() {
            if !self.views.iter().any(|v| v.id == *id) {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let min = full.left_top() + egui::vec2(MARGIN + i as f32 * (TILE.x + GAP), MARGIN);
            let rect = egui::Rect::from_min_size(min, TILE);
            let resp = ui.interact(rect, egui::Id::unique(("tile", id)), egui::Sense::click());
            let label = self.tile_label(id);
            let state = self.tile_state(id);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            self.paint_tile(ui, id, rect, &resp, state);
            if resp.clicked() {
                clicked = Some(id.to_string());
            }
            if resp.secondary_clicked() {
                self.selected = Some(id.to_string());
            }
            let resp = resp.on_hover_text(match state {
                TileState::Installed | TileState::Update => {
                    format!("Open {}", self.tool_name(id))
                }
                TileState::NotInstalled => format!("Install {}…", self.tool_name(id)),
                TileState::Damaged => format!("Repair {}…", self.tool_name(id)),
                TileState::Unavailable => format!("{} is not available yet", self.tool_name(id)),
            });
            resp.context_menu(|ui| {
                if let Some(a) = self.tool_menu(ui, id, state) {
                    menu_action = Some((id.to_string(), a));
                    ui.close();
                }
            });
        }
        if let Some(id) = clicked {
            self.activate(&id);
        }
        if let Some((id, a)) = menu_action {
            self.menu(&id, a);
        }

        // The gear, anchored to the lower-right corner.
        let gear = egui::Rect::from_center_size(
            // Never over the tiles, even in a window squeezed below its minimum size.
            egui::pos2(
                full.right() - 22.0,
                (full.bottom() - 22.0).max(full.top() + MARGIN + TILE.y + 16.0),
            ),
            egui::vec2(28.0, 28.0),
        );
        let g = ui.interact(
            gear,
            egui::Id::unique("settings-gear"),
            egui::Sense::click(),
        );
        g.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Settings"));
        let col = if g.hovered() || g.has_focus() {
            c.text_primary
        } else {
            c.text_muted
        };
        if g.hovered() {
            ui.painter().circle_filled(gear.center(), 14.0, c.bg_hover);
        }
        paint_gear(ui.painter(), gear.center(), 7.0, col);
        if g.on_hover_text("Settings").clicked() {
            self.toggle_settings(Some(&ui.ctx().clone()));
        }

        // One quiet line for progress and results, lower left; it fades after a while.
        let line = egui::Rect::from_min_max(
            egui::pos2(
                full.left() + MARGIN,
                (full.bottom() - 52.0).max(full.top() + MARGIN + TILE.y),
            ),
            egui::pos2(gear.left() - 8.0, full.bottom() - 10.0),
        );
        self.status_line(ui, line);
    }

    fn texture(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        gray: bool,
    ) -> Option<egui::TextureHandle> {
        let key = (id.to_string(), gray);
        if let Some(t) = self.textures.get(&key) {
            return Some(t.clone());
        }
        let png = icon_png(id)?;
        let icon = eframe::icon_data::from_png_bytes(png).ok()?;
        let mut rgba = icon.rgba;
        if gray {
            desaturate(&mut rgba);
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &rgba,
        );
        let t = ctx.load_texture(
            format!("tile-{id}-{gray}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(key, t.clone());
        Some(t)
    }

    fn paint_tile(
        &mut self,
        ui: &egui::Ui,
        id: &str,
        rect: egui::Rect,
        resp: &egui::Response,
        state: TileState,
    ) {
        let c = theme::Colors::get();
        let installed = matches!(state, TileState::Installed | TileState::Update);
        let painter = ui.painter();
        if resp.hovered()
            || resp.has_focus()
            || self.selected.as_deref() == Some(id) && resp.context_menu_opened()
        {
            painter.rect_filled(rect, 12.0, c.bg_hover.gamma_multiply(0.7));
        }
        if resp.has_focus() {
            painter.rect_stroke(
                rect,
                12.0,
                egui::Stroke::new(1.5, c.focus),
                egui::StrokeKind::Inside,
            );
        }
        let pressed = resp.is_pointer_button_down_on();
        let size = if pressed { 54.0 } else { 58.0 };
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 12.0 + 29.0),
            egui::vec2(size, size),
        );
        let tex = self.texture(ui.ctx(), id, !installed);
        let painter = ui.painter();
        if let Some(t) = tex {
            let tint = if installed {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_white_alpha(150)
            };
            painter.image(
                t.id(),
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        // Not installed: a small download mark on the corner; an update: an accent dot.
        let corner = icon_rect.right_bottom() - egui::vec2(4.0, 4.0);
        match state {
            TileState::NotInstalled | TileState::Damaged => {
                let fill = if state == TileState::Damaged {
                    c.warning
                } else {
                    c.bg_overlay
                };
                painter.circle_filled(corner, 9.0, fill);
                painter.circle_stroke(corner, 9.0, egui::Stroke::new(1.0, c.border_strong));
                let ink = if state == TileState::Damaged {
                    c.bg_base
                } else {
                    c.text_secondary
                };
                let s = egui::Stroke::new(1.6, ink);
                painter.line_segment(
                    [corner - egui::vec2(0.0, 4.5), corner + egui::vec2(0.0, 3.0)],
                    s,
                );
                painter.line_segment(
                    [
                        corner + egui::vec2(-3.2, 0.0),
                        corner + egui::vec2(0.0, 3.2),
                    ],
                    s,
                );
                painter.line_segment(
                    [corner + egui::vec2(3.2, 0.0), corner + egui::vec2(0.0, 3.2)],
                    s,
                );
            }
            TileState::Update => {
                painter.circle_filled(icon_rect.right_top() + egui::vec2(-4.0, 4.0), 5.0, c.accent);
            }
            TileState::Installed | TileState::Unavailable => {}
        }
        // Busy with this tool: a thin progress line under the icon.
        if let Some(r) = self.running.as_ref().filter(|r| r.tool == id) {
            let bar = egui::Rect::from_min_size(
                egui::pos2(icon_rect.left() + 6.0, icon_rect.bottom() + 3.0),
                egui::vec2(icon_rect.width() - 12.0, 3.0),
            );
            painter.rect_filled(bar, 1.5, c.bg_overlay);
            #[allow(clippy::cast_precision_loss)]
            let frac = if r.total == 0 {
                0.15
            } else {
                (r.done as f32 / r.total as f32).clamp(0.05, 1.0)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac, bar.height())),
                1.5,
                c.accent,
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let name_color = if installed {
            c.text_primary
        } else {
            c.text_muted
        };
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 18.0),
            egui::Align2::CENTER_CENTER,
            self.tool_name(id),
            egui::FontId::proportional(12.0),
            name_color,
        );
    }

    /// The tile's right-click menu: only what fits the tool's state; unavailable entries say
    /// why. Returns the chosen action.
    fn tool_menu(&mut self, ui: &mut egui::Ui, id: &str, state: TileState) -> Option<MenuAction> {
        let c = theme::Colors::get();
        let busy = self.is_busy();
        let busy_why = "Kim is busy with another install or uninstall";
        let mut pick = None;
        ui.set_min_width(190.0);
        ui.label(
            egui::RichText::new(self.tool_name(id))
                .size(12.0)
                .strong()
                .color(c.text_secondary),
        );
        ui.separator();
        match state {
            TileState::Installed | TileState::Update => {
                for (k, l) in launches(id).iter().enumerate() {
                    if ui.button(l.label).clicked() {
                        pick = Some(MenuAction::Open(k));
                    }
                }
                match configure(id) {
                    Configure::Program(_) => {
                        if ui.button("Settings…").clicked() {
                            pick = Some(MenuAction::Configure);
                        }
                    }
                    Configure::Note(why) => {
                        ui.add_enabled(false, egui::Button::new("Settings…"))
                            .on_disabled_hover_text(why);
                    }
                }
                if ui.button("Open installation folder").clicked() {
                    pick = Some(MenuAction::Folder);
                }
                ui.separator();
                if state == TileState::Update {
                    let v = self
                        .views
                        .iter()
                        .find(|v| v.id == id)
                        .and_then(|v| v.latest_compatible.clone())
                        .unwrap_or_default();
                    if ui
                        .add_enabled(!busy, egui::Button::new(format!("Update to {v}…")))
                        .on_disabled_hover_text(busy_why)
                        .clicked()
                    {
                        pick = Some(MenuAction::Install);
                    }
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Check for updates"))
                    .on_disabled_hover_text(busy_why)
                    .on_hover_text("Reads the catalog again; never downloads or installs anything")
                    .clicked()
                {
                    pick = Some(MenuAction::CheckUpdates);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Reinstall…"))
                    .on_disabled_hover_text(busy_why)
                    .on_hover_text(
                        "Installs this version again from the catalog, verified like any \
                         install; your settings and data stay",
                    )
                    .clicked()
                {
                    pick = Some(MenuAction::Reinstall);
                }
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(egui::RichText::new("Uninstall…").color(c.danger)),
                    )
                    .on_disabled_hover_text(busy_why)
                    .clicked()
                {
                    pick = Some(MenuAction::Uninstall);
                }
            }
            TileState::NotInstalled | TileState::Damaged => {
                let label = if state == TileState::Damaged {
                    "Repair…"
                } else {
                    "Install…"
                };
                if ui
                    .add_enabled(!busy, egui::Button::new(label))
                    .on_disabled_hover_text(busy_why)
                    .clicked()
                {
                    pick = Some(MenuAction::Install);
                }
                if state == TileState::Damaged
                    && ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(egui::RichText::new("Uninstall…").color(c.danger)),
                        )
                        .on_disabled_hover_text(busy_why)
                        .clicked()
                {
                    pick = Some(MenuAction::Uninstall);
                }
            }
            TileState::Unavailable => {
                ui.add_enabled(false, egui::Button::new("Install…"))
                    .on_disabled_hover_text(self.unavailable_reason(id));
            }
        }
        ui.separator();
        if ui.button("Information").clicked() {
            pick = Some(MenuAction::Info);
        }
        pick
    }

    fn menu(&mut self, id: &str, a: MenuAction) {
        self.selected = Some(id.to_string());
        match a {
            MenuAction::Open(k) => {
                if let Some(l) = launches(id).get(k) {
                    self.open(id, l);
                }
            }
            MenuAction::Configure if id == "noeul" => self.configure_noeul(),
            MenuAction::Configure => {
                if let Configure::Program(l) = configure(id) {
                    self.open(id, &l);
                }
            }
            MenuAction::Folder => self.open_folder(id),
            MenuAction::CheckUpdates => self.refresh_catalog(),
            MenuAction::Install => self.install_prompt = Some(id.to_string()),
            MenuAction::Reinstall => {
                self.reinstall = Some(id.to_string());
                self.install_prompt = Some(id.to_string());
            }
            MenuAction::Uninstall => {
                self.confirm = Some(ConfirmUninstall {
                    tool: id.to_string(),
                    ..Default::default()
                });
            }
            MenuAction::Info => self.info = Some(id.to_string()),
        }
    }

    /// Noeul's settings window belongs to the running Noeul: start it if needed (it hands over
    /// to a running one), then ask it to open the window; the answer goes to the status line.
    fn configure_noeul(&mut self) {
        let Some(version) = self
            .views
            .iter()
            .find(|v| v.id == "noeul")
            .and_then(|v| v.installed_version.clone())
        else {
            return;
        };
        let exe = self
            .backend
            .paths
            .tool_dir("noeul")
            .join("versions")
            .join(version)
            .join("noeul.exe");
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let run = |args: &[&str]| {
                let mut c = std::process::Command::new(&exe);
                c.args(args).stdin(std::process::Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    c.creation_flags(0x0800_0000);
                }
                c.output()
            };
            let result = run(&["start", "--quiet", "--no-input"])
                .and_then(|_| run(&["settings", "--json", "--no-input"]));
            let banner = match result {
                Ok(o) => {
                    let doc: serde_json::Value =
                        serde_json::from_slice(&o.stdout).unwrap_or_default();
                    if doc["ok"] == true {
                        Banner::Success("Noeul's settings are open.".into())
                    } else {
                        Banner::Error {
                            message: doc["error"]["message"]
                                .as_str()
                                .unwrap_or("Noeul did not open its settings")
                                .to_string(),
                            code: doc["error"]["code"].as_str().unwrap_or("error").to_string(),
                        }
                    }
                }
                Err(e) => Banner::Error {
                    message: format!("cannot start Noeul: {e}"),
                    code: "io".into(),
                },
            };
            let _ = tx.send(Msg::Notice(banner));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Opens the installed version's folder in Explorer (the user asked for it).
    fn open_folder(&mut self, id: &str) {
        let Some(version) = self
            .views
            .iter()
            .find(|v| v.id == id)
            .and_then(|v| v.installed_version.clone())
        else {
            return;
        };
        let dir = self
            .backend
            .paths
            .tool_dir(id)
            .join("versions")
            .join(version);
        let explorer =
            Path::new(&std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                .join("explorer.exe");
        let arg = dir.display().to_string();
        if let Err(e) = (self.launcher)(&explorer, &[arg.as_str()]) {
            self.set_banner(Banner::Error {
                message: format!("cannot open {}: {e}", dir.display()),
                code: "io".into(),
            });
        }
    }

    fn unavailable_reason(&self, id: &str) -> String {
        let Some(v) = self.views.iter().find(|v| v.id == id) else {
            return "This tool is not in the catalog.".into();
        };
        if let Some(r) = &v.incompatible_reason {
            return format!("Not installable on this PC: {r}");
        }
        match v.availability {
            Availability::Planned => format!(
                "No signed release of {} has been published yet. On a development PC, \
                 `cargo xtask local-mirror` builds installable packages next to kim.exe.",
                v.name
            ),
            Availability::Preview => {
                "Only preview releases exist; choose the Preview channel in Settings.".into()
            }
            Availability::Available => "No release on this channel.".into(),
        }
    }

    /// One line for what Kim is doing or just did; results fade after a few seconds, errors
    /// stay until clicked.
    fn status_line(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let c = theme::Colors::get();
        let now = ui.input(|i| i.time);
        let (text, color) = if let Some(r) = &self.running {
            (
                format!("{} {}…", r.verb, self.tool_name(&r.tool)),
                c.text_secondary,
            )
        } else if self.refreshing {
            ("Reading the catalog…".to_string(), c.text_secondary)
        } else if let Some(b) = &self.banner {
            let key = format!("{b:?}");
            if self.banner_at.as_ref().is_none_or(|(k, _)| *k != key) {
                self.banner_at = Some((key, now));
            }
            let shown_at = self.banner_at.as_ref().map_or(now, |(_, t)| *t);
            let error = matches!(b, Banner::Error { .. });
            if !error && now - shown_at > 6.0 {
                self.banner = None;
                self.banner_at = None;
                return;
            }
            if !error {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
            match b {
                Banner::Info(m) => (m.clone(), c.text_secondary),
                Banner::Success(m) => (m.clone(), c.success),
                Banner::Error { message, .. } => (message.clone(), c.danger),
            }
        } else {
            return;
        };
        let resp = ui.interact(rect, egui::Id::unique("status-line"), egui::Sense::click());
        let galley = ui.painter().layout(
            text.clone(),
            egui::FontId::proportional(11.0),
            color,
            rect.width(),
        );
        let pos = egui::pos2(
            rect.left(),
            rect.bottom() - galley.size().y.min(rect.height()),
        );
        ui.painter().with_clip_rect(rect).galley(pos, galley, color);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        if resp.on_hover_text(text).clicked() {
            self.banner = None;
            self.banner_at = None;
        }
    }

    fn set_banner(&mut self, b: Banner) {
        self.banner = Some(b);
        self.banner_at = None;
    }

    /// The small install confirmation: what will be installed, from where, and what the tool may
    /// do on this computer (disclosed before the button, `KIM-SEC-006`).
    fn install_dialog(&mut self, ctx: &egui::Context) {
        let c = theme::Colors::get();
        let Some(id) = self.install_prompt.clone() else {
            return;
        };
        let state = self.tile_state(&id);
        let again = self.reinstall.as_deref() == Some(id.as_str());
        let view = self.views.iter().find(|v| v.id == id).cloned();
        let name = self.tool_name(&id);
        let local = self
            .loaded
            .as_ref()
            .is_some_and(|l| l.source == store::Source::Mirror);
        let mut close = false;
        let mut go = false;
        let r = egui::Modal::new(egui::Id::unique("install-dialog")).show(ctx, |ui| {
            ui.set_max_width(dialog_width(ctx));
            ui.horizontal(|ui| {
                if let Some(t) = self.texture(ctx, &id, false) {
                    ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(36.0, 36.0)));
                }
                ui.vertical(|ui| {
                    let title = match state {
                        _ if again => format!("Reinstall {name}?"),
                        TileState::Damaged => format!("Repair {name}?"),
                        TileState::Update => format!("Update {name}?"),
                        _ => format!("Install {name}?"),
                    };
                    ui.label(egui::RichText::new(title).size(14.0).strong());
                    if let Some(v) = &view {
                        ui.label(
                            egui::RichText::new(&v.summary)
                                .size(11.0)
                                .color(c.text_secondary),
                        );
                    }
                });
            });
            ui.add_space(6.0);
            let installable = again
                || matches!(
                    state,
                    TileState::NotInstalled | TileState::Damaged | TileState::Update
                );
            if installable {
                if let Some(v) = view.as_ref().and_then(|v| v.latest_compatible.clone()) {
                    let from = if local {
                        "a package built on this PC (local preview, not a published release)"
                    } else {
                        "the signed catalog"
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "Version {v} from {from}. For your user only, never as administrator."
                        ))
                        .size(11.0)
                        .color(c.text_muted),
                    );
                }
                if let Some(d) = self.details.get(&id).filter(|d| !d.permissions.is_empty()) {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("On this computer it")
                            .size(11.0)
                            .color(c.text_secondary),
                    );
                    // Scrolls inside the dialog so its buttons stay in a small window.
                    let room = (ctx.content_rect().height() - 150.0).max(36.0);
                    egui::ScrollArea::vertical()
                        .max_height(room)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for p in &d.permissions {
                                let label = p.label.map_or_else(|| p.id.clone(), str::to_string);
                                let opt = if p.opt_in {
                                    " (only after you turn it on)"
                                } else {
                                    ""
                                };
                                ui.label(
                                    egui::RichText::new(format!("· {label}{opt}"))
                                        .size(11.0)
                                        .color(c.text_muted),
                                );
                            }
                        });
                }
            } else {
                ui.label(
                    egui::RichText::new(self.unavailable_reason(&id))
                        .size(11.0)
                        .color(c.text_muted),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if installable {
                    let blocked = if self.is_busy() {
                        Some("Kim is busy with another install or uninstall")
                    } else if self.settings.offline_mode && !local {
                        Some("Offline mode is on (Settings)")
                    } else {
                        None
                    };
                    let label = match state {
                        _ if again => "Reinstall",
                        TileState::Damaged => "Repair",
                        TileState::Update => "Update",
                        _ => "Install",
                    };
                    let b = egui::Button::new(
                        egui::RichText::new(label).color(c.text_on_accent).strong(),
                    )
                    .fill(c.accent);
                    let resp = ui.add_enabled(blocked.is_none(), b);
                    if let Some(why) = blocked {
                        resp.on_disabled_hover_text(why);
                    } else if resp.clicked() {
                        go = true;
                    }
                }
            });
        });
        if r.should_close() {
            close = true;
        }
        if go {
            self.install_prompt = None;
            self.start_install(&id);
        } else if close {
            self.install_prompt = None;
            self.reinstall = None;
        }
    }

    /// Information: version, release stage and what the catalog and Kim know about the tool.
    fn info_dialog(&mut self, ctx: &egui::Context) {
        let c = theme::Colors::get();
        let Some(id) = self.info.clone() else { return };
        let view = self.views.iter().find(|v| v.id == id).cloned();
        let info = self.details.get(&id).cloned();
        let tool = self
            .loaded
            .as_ref()
            .and_then(|l| l.catalog.tool(&id))
            .cloned();
        let mut close = false;
        let max_h = (ctx.content_rect().height() - 60.0).max(160.0);
        let width = (ctx.content_rect().width() - 48.0).clamp(220.0, 520.0);
        let r = egui::Modal::new(egui::Id::unique("info-dialog")).show(ctx, |ui| {
            // A fixed width; long commands and paths wrap instead of widening the dialog.
            ui.set_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            let name = self.tool_name(&id);
            ui.label(egui::RichText::new(&name).size(14.0).strong());
            egui::ScrollArea::vertical()
                .max_height(max_h - 70.0)
                .show(ui, |ui| {
                    if let (Some(v), Some(t)) = (&view, &tool) {
                        ui.label(
                            egui::RichText::new(&t.description)
                                .size(11.0)
                                .color(c.text_secondary),
                        );
                        ui.add_space(4.0);
                        egui::Grid::new("info-meta")
                            .num_columns(2)
                            .spacing([16.0, 3.0])
                            .show(ui, |ui| {
                                let row = |ui: &mut egui::Ui, k: &str, val: String| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(k).size(11.0).color(c.text_muted),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                    ui.label(egui::RichText::new(val).size(11.0));
                                    ui.end_row();
                                };
                                row(ui, "Status", status_label(v));
                                row(
                                    ui,
                                    "Installed",
                                    v.installed_version.clone().unwrap_or_else(|| "—".into()),
                                );
                                row(
                                    ui,
                                    "Newest",
                                    v.latest_compatible
                                        .clone()
                                        .or(v.latest.clone())
                                        .unwrap_or_else(|| "—".into()),
                                );
                                row(ui, "Release", release_label(v.availability).into());
                                row(ui, "Support", support_label(t.support).into());
                                row(ui, "License", t.license.clone());
                            });
                    }
                    if let Some(d) = &info {
                        tool_sections(ui, &name, d);
                    }
                });
            ui.add_space(6.0);
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if close || r.should_close() {
            self.info = None;
        }
    }

    /// The Settings panel (the gear): Kim's own settings, in four short sections.
    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Settings").size(15.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() {
                            self.toggle_settings(Some(&ui.ctx().clone()));
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (SettingsTab::General, "General"),
                        (SettingsTab::Shortcuts, "Shortcuts"),
                        (SettingsTab::Catalog, "Catalog"),
                        (SettingsTab::About, "About"),
                    ] {
                        if ui
                            .selectable_label(self.settings_tab == tab, label)
                            .clicked()
                        {
                            self.settings_tab = tab;
                            self.capture = None;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.settings_tab {
                            SettingsTab::General => self.settings_general(ui),
                            SettingsTab::Shortcuts => self.settings_shortcuts(ui),
                            SettingsTab::Catalog => self.settings_catalog(ui),
                            SettingsTab::About => self.settings_about(ui),
                        }
                        if let Some(b) = &self.banner {
                            ui.add_space(8.0);
                            let (t, col) = match b {
                                Banner::Info(m) => (m.clone(), c.text_secondary),
                                Banner::Success(m) => (m.clone(), c.success),
                                Banner::Error { message, .. } => (message.clone(), c.danger),
                            };
                            ui.label(egui::RichText::new(t).size(11.0).color(col));
                        }
                    });
            });
    }

    fn settings_general(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        ui.label(egui::RichText::new("Appearance").strong());
        ui.horizontal(|ui| {
            let before = self.prefs.theme;
            ui.selectable_value(&mut self.prefs.theme, ThemePref::System, "System");
            ui.selectable_value(&mut self.prefs.theme, ThemePref::Dark, "Dark");
            ui.selectable_value(&mut self.prefs.theme, ThemePref::Light, "Light");
            if before != self.prefs.theme {
                self.save_prefs();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Size");
            let before = self.prefs.scale;
            let auto = format!("Auto ({}%)", (self.auto_scale * 100.0).round());
            ui.selectable_value(&mut self.prefs.scale, None, auto)
                .on_hover_text("Fits this screen: larger on a big screen.");
            for p in SCALES {
                ui.selectable_value(&mut self.prefs.scale, Some(p), format!("{p}%"));
            }
            if before != self.prefs.scale {
                self.save_prefs();
            }
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Installing and updating").strong());
        let mut changed = false;
        changed |= ui
            .checkbox(&mut self.settings.offline_mode, "Offline mode")
            .on_hover_text(
                "Refuse every network operation. Packages built on this PC still install.",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.settings.check_updates_on_launch,
                "Check for updates when Kim starts",
            )
            .on_hover_text("Only shows available updates. Never downloads or installs anything.")
            .changed();
        if changed {
            self.save_settings();
        }
        ui.horizontal(|ui| {
            ui.label("Channel");
            let before = self.channel;
            ui.selectable_value(&mut self.channel, Channel::Stable, "Stable");
            ui.selectable_value(&mut self.channel, Channel::Preview, "Preview");
            if before != self.channel {
                self.reload();
            }
        });
        ui.label(
            egui::RichText::new(
                "Kim never installs, updates or removes a tool by itself: only when you choose to.",
            )
            .size(11.0)
            .color(c.text_muted),
        );
    }

    fn settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        if self.shortcuts.is_off() {
            ui.colored_label(
                c.warning,
                "All shortcuts are off for this window (--no-hotkeys or KIM_NO_HOTKEYS).",
            );
        }
        if let Some(p) = &self.shortcuts.problem {
            ui.colored_label(c.warning, p);
        }
        let cli = kim_cli();
        let mut act = None;
        egui::Grid::new("settings-shortcuts")
            .num_columns(3)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for r in self.shortcuts.rows() {
                    ui.label(egui::RichText::new(&r.title).size(12.0));
                    let capturing = self.capture.as_deref() == Some(r.action.as_str());
                    let keys = if capturing {
                        "Press keys… (Esc cancels)".to_string()
                    } else {
                        r.keys.clone()
                    };
                    ui.label(egui::RichText::new(keys).monospace().size(12.0));
                    if shortcuts::is_bindable(&r.action) {
                        ui.horizontal(|ui| {
                            let why = "Changing shortcuts needs kim.exe next to this window";
                            if ui
                                .add_enabled(
                                    cli.is_some() && !capturing,
                                    egui::Button::new("Change").small(),
                                )
                                .on_disabled_hover_text(why)
                                .clicked()
                            {
                                act = Some((r.action.clone(), "change"));
                            }
                            if ui
                                .add_enabled(cli.is_some(), egui::Button::new("Off").small())
                                .on_disabled_hover_text(why)
                                .on_hover_text("Turn this shortcut off")
                                .clicked()
                            {
                                act = Some((r.action.clone(), "disable"));
                            }
                            if ui
                                .add_enabled(cli.is_some(), egui::Button::new("Reset").small())
                                .on_disabled_hover_text(why)
                                .on_hover_text("Back to Kim's default")
                                .clicked()
                            {
                                act = Some((r.action.clone(), "reset"));
                            }
                        });
                    } else {
                        ui.label(egui::RichText::new(&r.state).size(11.0).color(c.text_muted));
                    }
                    ui.end_row();
                }
            });
        match act {
            Some((action, "change")) => self.capture = Some(action),
            Some((action, verb)) => self.hotkeys_cli(&["hotkeys", verb, &action]),
            None => {}
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Shortcuts work only while this window has focus. Saved in hotkeys.json; `kim hotkeys …` changes the same file.")
                .size(11.0)
                .color(c.text_muted),
        );
    }

    /// While a shortcut is being changed: the next key press with its modifiers becomes it.
    fn capture_key(&mut self, ctx: &egui::Context) {
        let Some(action) = self.capture.clone() else {
            return;
        };
        // Capture stops when the window loses focus (HOTKEY_CONTRACT §11.2).
        if ctx.input(|i| !i.focused) {
            self.capture = None;
            return;
        }
        let mut done = None;
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if *key == egui::Key::Escape && modifiers.is_none() {
                        done = Some(None);
                    } else if let Some(t) = shortcuts::chord_text(*key, *modifiers) {
                        done = Some(Some(t));
                    }
                    false
                }
                _ => true,
            });
        });
        match done {
            Some(Some(keys)) => {
                self.capture = None;
                self.hotkeys_cli(&["hotkeys", "set", &action, &keys]);
            }
            Some(None) => self.capture = None,
            None => {}
        }
    }

    /// Runs `kim hotkeys …` (the same checks and file as the CLI), headless, then re-reads the
    /// shortcuts.
    fn hotkeys_cli(&mut self, args: &[&str]) {
        let Some(cli) = kim_cli() else { return };
        let mut cmd = std::process::Command::new(cli);
        cmd.args(args)
            .args(["--json", "--no-input"])
            .stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output();
        let doc: Option<serde_json::Value> = out
            .as_ref()
            .ok()
            .and_then(|o| serde_json::from_slice(&o.stdout).ok());
        match doc {
            Some(d) if d["ok"] == true => {
                self.shortcuts.reload();
                self.set_banner(Banner::Success("Shortcut saved.".into()));
            }
            Some(d) => self.set_banner(Banner::Error {
                message: d["error"]["message"]
                    .as_str()
                    .unwrap_or("the shortcut was not changed")
                    .to_string(),
                code: d["error"]["code"].as_str().unwrap_or("error").to_string(),
            }),
            None => self.set_banner(Banner::Error {
                message: "kim.exe did not answer; the shortcut was not changed".into(),
                code: "io".into(),
            }),
        }
    }

    fn settings_catalog(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        if let Some(l) = &self.loaded {
            let src = match l.source {
                store::Source::Bundled => "built into Kim",
                store::Source::Cache => "downloaded earlier",
                store::Source::Mirror => "packages built on this PC (local preview)",
            };
            egui::Grid::new("catalog-meta")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.label(egui::RichText::new(k).size(11.0).color(c.text_muted));
                        ui.label(egui::RichText::new(v).size(12.0));
                        ui.end_row();
                    };
                    row(
                        ui,
                        "Catalog",
                        format!("v{} · {src}", l.catalog.catalog_version),
                    );
                    row(ui, "Signed by", l.signer.key_id.clone());
                    row(ui, "Expires", l.catalog.expires_at.clone());
                    row(ui, "Source", kim::redact::url(&self.backend.catalog_source));
                });
            if l.stale {
                ui.colored_label(
                    c.warning,
                    "The catalog has expired; refresh it when online.",
                );
            }
        }
        ui.add_space(6.0);
        let local = self.mirror.is_some();
        let blocked = if self.is_busy() {
            Some("Kim is busy")
        } else if self.settings.offline_mode && !local {
            Some("Offline mode is on")
        } else {
            None
        };
        let resp = ui.add_enabled(blocked.is_none(), egui::Button::new("Refresh now"));
        if let Some(why) = blocked {
            resp.on_disabled_hover_text(why);
        } else if resp
            .on_hover_text("Reads the signed catalog again. Never installs anything.")
            .clicked()
        {
            self.refresh_catalog();
        }
    }

    fn settings_about(&mut self, ui: &mut egui::Ui) {
        let c = theme::Colors::get();
        ui.horizontal(|ui| {
            if let Some(t) = self.texture(ui.ctx(), "kim", false) {
                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(40.0, 40.0)));
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(format!("Kim {}", kim::KIM_VERSION)).strong());
                ui.label(
                    egui::RichText::new("A tiny home for Subak, Noeul and Ddalgak.")
                        .size(11.0)
                        .color(c.text_secondary),
                );
            });
        });
        ui.add_space(6.0);
        for (k, v) in [
            (
                "Programs",
                self.backend.paths.programs.display().to_string(),
            ),
            (
                "Kim's data",
                self.backend.paths.kim_data.display().to_string(),
            ),
        ] {
            ui.label(
                egui::RichText::new(format!("{k}: {v}"))
                    .size(11.0)
                    .color(c.text_muted),
            );
        }
        ui.label(
            egui::RichText::new("Local-first: no account, no telemetry, no automatic installs or updates. MIT OR Apache-2.0.")
                .size(11.0)
                .color(c.text_muted),
        );
    }
}

/// ADR 0090: the launcher's tiles, in this order.
pub const LAUNCHER_TOOLS: [&str; 3] = ["subak", "noeul", "ddalgak"];

/// What a tile shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    /// Installed and its program is on disk: full colour, a click starts it.
    Installed,
    /// Installed, a newer compatible release exists (an accent dot).
    Update,
    /// Installable: grey, a click asks before installing.
    NotInstalled,
    /// Kim's record exists but the program is missing or broken: grey with a warning mark.
    Damaged,
    /// Nothing to install here (no release, or not for this PC): grey; the menu says why.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Open(usize),
    Configure,
    Folder,
    CheckUpdates,
    Install,
    Reinstall,
    Uninstall,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    General,
    Shortcuts,
    Catalog,
    About,
}

/// The theme choice (Settings → General), kept in `manager.json` next to Kim's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

/// Kim Manager's own preferences (ADR 0090): only how the window looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    /// ADR 0091: the size in percent (`None`: Auto, fitted to the screen).
    pub scale: Option<u16>,
}

impl Prefs {
    /// The zoom to use: the chosen percentage (kept within 100–300 %) or `auto`.
    pub fn scale_or(&self, auto: f32) -> f32 {
        self.scale
            .map_or(auto, |p| f32::from(p.clamp(100, 300)) / 100.0)
    }

    /// The preferences in `manager.json` next to `settings` (Kim's settings file); defaults
    /// when the file is missing or unreadable.
    pub fn load_beside(settings: &Path) -> Prefs {
        std::fs::read(settings.with_file_name("manager.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}

/// The launcher's size in points at 100 % (ADR 0090) and its minimum.
pub const BASE_SIZE: [f32; 2] = [368.0, 224.0];
pub const MIN_SIZE: [f32; 2] = [336.0, 196.0];
/// The sizes Settings → General offers besides Auto (percent).
pub const SCALES: [u16; 5] = [100, 150, 200, 250, 300];

/// ADR 0091: the zoom for a screen whose work area is `work` points: as large as fits the
/// largest view (Settings, 580 × 460 plus the title bar) on the screen with a small margin, in
/// steps of 25 %, between 100 % and 300 %.
pub fn auto_scale(work: egui::Vec2) -> f32 {
    let fit = (work.x * 0.95 / 580.0).min(work.y * 0.98 / 492.0);
    ((fit * 4.0).floor() / 4.0).clamp(1.0, 3.0)
}

/// ADR 0091: keeping the real window within its screen (Win32).
#[cfg(windows)]
#[allow(unsafe_code)]
mod screen {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };

    /// Moves the window up or left into its monitor's work area when it reaches past the
    /// bottom or right edge; returns where it was (pixels) when it moved.
    pub fn fit(hwnd: isize) -> Option<(i32, i32)> {
        let h = hwnd as HWND;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: r,
            rcWork: r,
            dwFlags: 0,
        };
        // SAFETY: our own window; correctly sized out-structures.
        let ok = unsafe {
            GetWindowRect(h, &raw mut r) != 0
                && GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &raw mut mi) != 0
        };
        if !ok {
            return None;
        }
        let w = mi.rcWork;
        let x = r.left.min(w.right - (r.right - r.left)).max(w.left);
        let y = r.top.min(w.bottom - (r.bottom - r.top)).max(w.top);
        if (x, y) == (r.left, r.top) {
            return None;
        }
        move_to(hwnd, (x, y));
        Some((r.left, r.top))
    }

    /// Moves the window's upper-left corner to `pos` (pixels), keeping its size.
    pub fn move_to(hwnd: isize, pos: (i32, i32)) {
        // SAFETY: our own window; only its position changes.
        unsafe {
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                pos.0,
                pos.1,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// The primary monitor's work area (without the taskbar) in points, read before the window
/// exists (the process is not DPI aware yet, so Windows answers in 96-DPI points).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn work_area() -> Option<egui::Vec2> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: SPI_GETWORKAREA writes one RECT.
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut r).cast(), 0) };
    (ok != 0 && r.right > r.left && r.bottom > r.top)
        .then(|| egui::vec2((r.right - r.left) as f32, (r.bottom - r.top) as f32))
}

/// How a tool is configured from its menu.
enum Configure {
    Program(Launch),
    Note(&'static str),
}

fn configure(id: &str) -> Configure {
    match id {
        "noeul" => Configure::Program(Launch {
            label: "Settings…",
            program: "noeul.exe",
            args: &["settings"],
            also: &[],
            hint: "Noeul's settings open in its own window (Noeul must be running).",
        }),
        "ddalgak" => Configure::Program(Launch {
            label: "Settings…",
            program: "ddalgak-editor.exe",
            args: &[],
            also: &[],
            hint: "",
        }),
        _ => Configure::Note(
            "Subak keeps its settings in its config file: `subak config show`, `subak config set <key> <value>`",
        ),
    }
}

/// A dialog's width: at most 340 points, and always inside a small window.
fn dialog_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 48.0).clamp(220.0, 340.0)
}

/// The `kim.exe` next to this window (for `kim hotkeys …`).
fn kim_cli() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.with_file_name("kim.exe");
    p.is_file().then_some(p)
}

/// The icon of a tile (256 px, from assets/).
fn icon_png(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "subak" => include_bytes!("../../../assets/subak/subak-256.png"),
        "noeul" => include_bytes!("../../../assets/noeul/noeul-256.png"),
        "ddalgak" => include_bytes!("../../../assets/ddalgak/ddalgak-256.png"),
        "kim" => include_bytes!("../../../assets/icon/kim-128.png"),
        _ => return None,
    })
}

/// Grey for a tool that is not installed: luminance, slightly lifted.
pub fn desaturate(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let l = (0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2]))
            .mul_add(0.85, 20.0)
            .clamp(0.0, 255.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let l = l as u8;
        px[0] = l;
        px[1] = l;
        px[2] = l;
    }
}

/// The gear: a ring with eight teeth (a painted shape, never a font glyph).
fn paint_gear(p: &egui::Painter, center: egui::Pos2, r: f32, col: egui::Color32) {
    let s = egui::Stroke::new(2.0, col);
    for k in 0..8 {
        #[allow(clippy::cast_precision_loss)]
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let d = egui::vec2(a.cos(), a.sin());
        p.line_segment(
            [center + d * (r - 1.0), center + d * (r + 3.0)],
            egui::Stroke::new(2.6, col),
        );
    }
    p.circle_stroke(center, r - 1.0, s);
    p.circle_stroke(center, r * 0.35, egui::Stroke::new(1.6, col));
}

impl KimManagerApp {
    /// The shortcut list (§11.1): every action with its keys and state, fixed keys included.
    /// Changing shortcuts is done with `kim hotkeys …`; the in-window binding editor is a later
    /// step (docs/HOTKEY_CONTRACT.md §11.2).
    fn shortcuts_dialog(&mut self, ctx: &egui::Context) {
        let c = theme::Colors::get();
        let rows = self.shortcuts.rows();
        let mut close = false;
        let r = egui::Modal::new(egui::Id::unique("keyboard-shortcuts")).show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(220.0, 560.0));
            ui.label(
                egui::RichText::new("Keyboard shortcuts")
                    .size(15.0)
                    .strong(),
            );
            if self.shortcuts.is_off() {
                ui.colored_label(
                    c.warning,
                    "All shortcuts are off for this window (--no-hotkeys or KIM_NO_HOTKEYS).",
                );
            }
            if let Some(p) = &self.shortcuts.problem {
                ui.colored_label(c.warning, p);
            }
            egui::Grid::new("shortcut-rows")
                .num_columns(3)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for h in ["Action", "Keys", "State"] {
                        ui.label(egui::RichText::new(h).size(11.0).color(c.text_muted));
                    }
                    ui.end_row();
                    for r in &rows {
                        ui.label(egui::RichText::new(&r.title).color(c.text_primary));
                        ui.label(egui::RichText::new(&r.keys).monospace());
                        ui.label(egui::RichText::new(&r.state).color(c.text_secondary));
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Change a shortcut with `kim hotkeys set <action> <keys>`, turn one off with \
                     `kim hotkeys disable <action>`, restore it with `kim hotkeys reset <action>`.",
                )
                .size(11.0)
                .color(c.text_muted),
            );
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if close || r.should_close() {
            self.show_shortcuts = false;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let c = theme::Colors::get();
        let mut close = false;
        let mut go = None;
        let name = self
            .confirm
            .as_ref()
            .map(|c| self.tool_name(&c.tool))
            .unwrap_or_default();
        let width = dialog_width(ctx);
        if let Some(conf) = &mut self.confirm {
            egui::Modal::new(egui::Id::unique("confirm-uninstall")).show(ctx, |ui| {
                ui.set_max_width(width);
                ui.label(egui::RichText::new(format!("Uninstall {name}?")).size(14.0).strong());
                ui.label("Program files are removed. Your settings and data are kept unless you choose otherwise.");
                ui.checkbox(&mut conf.purge, "Also delete this tool's settings and data");
                if conf.purge {
                    ui.colored_label(c.danger, "Deleting data is permanent.");
                    ui.checkbox(&mut conf.understood, "I understand that the data cannot be recovered");
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep installed").clicked() {
                        close = true;
                    }
                    let ok = !conf.purge || conf.understood;
                    let label = if conf.purge { "Uninstall and delete data" } else { "Uninstall" };
                    if ui.add_enabled(ok, egui::Button::new(egui::RichText::new(label).color(c.danger))).clicked() {
                        go = Some((conf.tool.clone(), conf.purge));
                    }
                });
            });
        }
        if close {
            self.confirm = None;
        }
        if let Some((tool, purge)) = go {
            self.confirm = None;
            self.start_uninstall(&tool, purge);
        }
    }
}

impl eframe::App for KimManagerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.ctx.is_none() {
            self.ctx = Some(ui.ctx().clone());
        }
        #[cfg(windows)]
        if self.hwnd.is_none() {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = frame.window_handle()
                && let RawWindowHandle::Win32(w) = h.as_raw()
            {
                self.hwnd = Some(w.hwnd.get());
            }
        }
        #[cfg(not(windows))]
        let _ = frame;
        KimManagerApp::ui(self, ui);
    }
}

fn status_label(v: &ToolView) -> String {
    match v.status {
        ToolStatus::Installed => {
            format!("installed {}", v.installed_version.as_deref().unwrap_or(""))
        }
        ToolStatus::UpdateAvailable => {
            format!("update {}", v.latest_compatible.as_deref().unwrap_or(""))
        }
        ToolStatus::Damaged => "damaged".into(),
        // available / available (preview) / incompatible / planned / preview only
        ToolStatus::Available | ToolStatus::Incompatible | ToolStatus::Unreleased => {
            v.availability_label().unwrap_or("not released").into()
        }
    }
}

/// The release stage (CATALOG_FORMAT §6, `KIM-FR-051`), derived from the signed releases.
fn release_label(a: Availability) -> &'static str {
    match a {
        Availability::Planned => "Planned · no release has been published yet",
        Availability::Preview => "Preview · only preview releases are published",
        Availability::Available => "Available · stable releases are published",
    }
}

fn support_label(s: kim::catalog::Support) -> &'static str {
    match s {
        kim::catalog::Support::Stable => "Stable",
        kim::catalog::Support::Preview => "Preview",
        kim::catalog::Support::InDevelopment => "In development",
        kim::catalog::Support::Deprecated => "Deprecated",
    }
}

/// What the catalog declares about a tool and what Kim decides itself (CATALOG_FORMAT §2.1).
/// Shown before any button is pressed, so what a tool may do is disclosed ahead of an install
/// (`KIM-SEC-006`). Links are plain, selectable text: Kim Manager starts no browser.
fn tool_sections(ui: &mut egui::Ui, name: &str, d: &Details) {
    let c = theme::Colors::get();
    let heading = |ui: &mut egui::Ui, t: &str| {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(t)
                .size(13.0)
                .strong()
                .color(c.text_primary),
        );
    };
    let muted = |ui: &mut egui::Ui, t: String| {
        ui.label(egui::RichText::new(t).size(11.0).color(c.text_muted));
    };
    if !d.features.is_empty() {
        heading(ui, "Features");
        for f in &d.features {
            ui.label(egui::RichText::new(f).color(c.text_secondary));
        }
    }
    if let Some(s) = &d.shortcuts {
        heading(ui, "Keyboard shortcuts");
        // One wrapping row per action (a table would widen a small window).
        for a in &s.actions {
            let keys = if a.keys.is_empty() {
                "not set".to_string()
            } else {
                a.keys.join(", ")
            };
            let scope = match a.scope.as_str() {
                "global" => "system-wide while it runs".to_string(),
                "app" => format!("inside {name}"),
                other => other.to_string(),
            };
            let fixed = if a.bindable { "" } else { " · fixed" };
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(keys).monospace());
                ui.label(egui::RichText::new(&a.title).color(c.text_primary));
                ui.label(
                    egui::RichText::new(format!("{scope}{fixed}"))
                        .size(11.0)
                        .color(c.text_muted),
                );
            });
        }
        let id = d
            .manage
            .as_ref()
            .and_then(|m| m.install.strip_prefix("kim install "))
            .unwrap_or("kim")
            .to_string();
        muted(
            ui,
            match s.source {
                "installed" => format!(
                    "From the installed {name} and your changes. Change one with `{id} hotkeys set <action> <keys>`, turn all off with `{id} hotkeys disable --all`."
                ),
                _ => format!(
                    "{name}'s defaults ({} input). Every shortcut can be changed, turned off or reset once it is installed.",
                    s.backend
                ),
            },
        );
        if let Some(n) = &s.note {
            ui.colored_label(c.warning, n);
        }
    }
    if !d.permissions.is_empty() {
        heading(ui, "What it may do on your computer");
        for p in &d.permissions {
            let label = p.label.map_or_else(|| p.id.clone(), str::to_string);
            let opt_in = if p.opt_in {
                " (only after you turn it on)"
            } else {
                ""
            };
            ui.label(
                egui::RichText::new(format!("{label}{opt_in}"))
                    .strong()
                    .color(c.text_primary),
            );
            ui.label(egui::RichText::new(&p.detail).color(c.text_secondary));
        }
    }
    if !d.entry_points.is_empty() {
        heading(ui, "Command line and agents");
        for e in &d.entry_points {
            let planned = match e.status {
                EntryStatus::Planned => " · planned",
                EntryStatus::Available => "",
            };
            ui.label(egui::RichText::new(&e.command).monospace());
            muted(ui, format!("{} · {}{planned}", e.kind, e.summary));
        }
    }
    if !d.links.is_empty() {
        heading(ui, "Documentation");
        for l in &d.links {
            ui.label(egui::RichText::new(&l.title).color(c.text_secondary));
            muted(ui, l.url.clone());
        }
    }
    if let Some(i) = &d.install {
        heading(ui, "Install location");
        // Paths have no spaces to wrap at: shortened to the width, whole on hover.
        let path_line = |ui: &mut egui::Ui, text: String| {
            ui.add(egui::Label::new(egui::RichText::new(&text).color(c.text_secondary)).truncate())
                .on_hover_text(text);
        };
        path_line(ui, format!("Program files: {}", i.program_dir.display()));
        for dir in &i.user_data_dirs {
            path_line(
                ui,
                format!("Your data (kept on uninstall): {}", dir.display()),
            );
        }
        muted(ui, "For your user only · never as administrator".into());
    }
    if let Some(m) = &d.manage {
        heading(ui, "Updates");
        ui.label(
            egui::RichText::new(format!(
                "Manual only: Kim never installs, updates or removes {name} by itself."
            ))
            .color(c.text_secondary),
        );
        muted(
            ui,
            format!(
                "Same as the buttons: `{}` · `{}` · `{}`",
                m.install, m.update, m.uninstall
            ),
        );
    }
}
