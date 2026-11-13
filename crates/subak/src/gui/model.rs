//! The window's side of the Host conversation (IPC v1, docs/contracts/IPC_PROTOCOL.md), as a
//! state machine without I/O: it is told what arrived (a connection, a response, an event, a
//! data frame, a disconnect, a user action) and answers with requests to send and what the
//! window must do. The window module does the pipe and the drawing; this module decides, and is
//! unit-tested on its own.
//!
//! **Tabs (Step 17).** The window shows the Host's Terminals as tabs, in the Host's order
//! (creation order, SESSION_MODEL §1) unless the user moved one, minus the ones the user hid in
//! this window. One tab is active: its pane is attached interactively (the others are not
//! attached, so they cost nothing); switching detaches the old pane and attaches the new one,
//! whose screen and history come back from the Host's snapshot (Step 13). The tabs follow every
//! client's changes through the `terminals` events: created, renamed, state changes, removed.
//!
//! Rules it keeps (ADR 0009, ADR 0020, docs/SESSION_MODEL.md §4, `SUBAK-FR-003`,
//! `SUBAK-FR-014`, `SUBAK-FR-015`):
//!
//! - **three distinct ends.** *Hiding a tab* (its `×`, "Hide tab") is a detach of this window:
//!   the Terminal keeps running and nothing is sent but `pane.detach`. *Closing a Terminal*
//!   (`Ctrl+W`, "Close Terminal") is `terminal.kill` of exactly that Terminal **by id**, first
//!   without `force`: the Host refuses a busy Terminal (`busy`), the window then asks, and only
//!   the user's explicit confirmation sends `force` — again by id, so a rename or another
//!   Terminal can never be hit. *Removing a stored Terminal* (stopped or failed; `Ctrl+W` on it)
//!   is `terminal.remove` without `force`, which the Host refuses for anything running;
//! - closing the window is a detach (`pane.detach`), never anything else;
//! - it only creates a Terminal on an explicit `terminal.new` (`Ctrl+T`, `+`), never by opening;
//! - when the Host goes away it keeps the last screen, retries with a growing delay without
//!   starting a Host, and reattaches to the same Terminal (by id) when a Host serves it again.
//!
//! **Internal windows and panes (Step 21, ADR 0054).** A tab is a Terminal; the window shows its
//! active window's active pane, which is the Host's selection, the same for every client. When it
//! changes (`window.selected`, `pane.selected`; `subak window select`, another client, or the
//! shown pane's program ending while others run) the window detaches that pane and attaches the
//! new active one; nothing ends. Which window and pane is shown is in the title
//! ([`TermInfo::place`]). Keys to switch them are Step 23's prefix table.
//!
//! **Splits (Step 22, ADR 0056).** When the shown window has several visible panes, the window
//! draws them all at the places of the Host's layout ([`TermInfo::layout`]): the active pane is
//! the interactive attachment (keys, selection, search), every other visible pane a read-only
//! view attached for drawing only ([`Model::views`]). A zoomed window shows its zoomed pane alone,
//! and panes nobody sees are not attached. The window asks for the whole grid; the Host gives each
//! pane its share. Splitting, moving focus, resizing, zooming and closing a pane are requests to
//! the Host ([`Model::pane_op`]); the layout comes back with the next list.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

/// A Terminal as the tabs and the title show it (`terminal.list` with `detail: "full"`, or the
/// `terminal` of `terminal.created` and `terminal.create`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermInfo {
    pub id: String,
    pub name: String,
    /// The Host's state; locally also `exited` (its program ended, shown last) and `removed`
    /// (no longer in the Host, shown last).
    pub state: String,
    pub pid: Option<u32>,
    pub program: String,
    /// The pane's history limit: the view keeps as many lines.
    pub history_limit: usize,
    /// Step 21: the shown window and pane as `index:name.pane`, with the counts, when the
    /// Terminal has more than one window or pane (`None` otherwise, so nothing changes for the
    /// usual single-pane Terminal).
    pub place: Option<String>,
    /// Step 25: tmux's window list (`0:cmd* 1:logs#`) with its flags (`*` shown, `-` last,
    /// `#` activity, `!` bell, `Z` zoomed), when the Terminal has more than one window or a
    /// window has an alert.
    pub status: Option<String>,
    /// Step 28: the Terminal's Claude Code state (hooks only; `None` without).
    pub claude: crate::gui::float::art::Claude,
    /// Step 21: the id of the Terminal's active pane (its active window's active pane).
    pub active_pane: Option<String>,
    /// Step 22: the active window's layout when it has more than one pane.
    pub layout: Option<WinLayout>,
    /// Step 23: the Terminal's internal windows `(id, index, name)` in index order, and the
    /// active one (the prefix keys switch, rename and kill them).
    pub windows: Vec<(String, u64, String)>,
    pub active_window: Option<String>,
}

/// Where one pane of the shown window is, in cells of the window's grid (Step 22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
    /// False while another pane of the window is zoomed.
    pub visible: bool,
}

impl Slot {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        col >= self.x && row >= self.y && col < self.x + self.cols && row < self.y + self.rows
    }
}

/// The shown window's layout (Step 22): its size and its panes' places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinLayout {
    pub window_id: String,
    pub size: (u16, u16),
    pub zoomed: Option<String>,
    pub slots: Vec<Slot>,
}

impl WinLayout {
    pub fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// More than one pane is drawn.
    pub fn split(&self) -> bool {
        self.slots.iter().filter(|s| s.visible).count() > 1
    }

    fn from_window(w: &Value) -> Option<Self> {
        let panes = w["panes"].as_array()?;
        if panes.len() < 2 {
            return None;
        }
        let n = |v: &Value| v.as_u64().and_then(|n| u16::try_from(n).ok());
        let slots = panes
            .iter()
            .map(|p| {
                let r = &p["rect"];
                Some(Slot {
                    id: p["id"].as_str()?.to_string(),
                    x: n(&r["x"])?,
                    y: n(&r["y"])?,
                    cols: n(&r["cols"])?,
                    rows: n(&r["rows"])?,
                    visible: p["visible"] != false,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            window_id: w["id"].as_str()?.to_string(),
            size: (n(&w["size"]["cols"])?, n(&w["size"]["rows"])?),
            zoomed: w["zoomed"].as_str().map(String::from),
            slots,
        })
    }
}

/// A read-only view of a visible pane other than the active one (Step 22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneViewState {
    pub pane_id: String,
    /// Its stream once attached.
    pub stream: Option<u32>,
    in_snapshot: bool,
    snapshot_size: Option<(u16, u16)>,
    /// The attach was refused: not asked again until the layout changes.
    failed: bool,
}

impl TermInfo {
    pub fn running(&self) -> bool {
        self.state == "running"
    }

    /// Kept by the Host without a process.
    pub fn stored(&self) -> bool {
        matches!(self.state.as_str(), "stopped" | "failed")
    }

    /// No longer in the Host (only the shown tab keeps it, with its last screen).
    pub fn gone(&self) -> bool {
        matches!(self.state.as_str(), "removed" | "exited")
    }

    fn from_full(t: &Value) -> Option<Self> {
        let windows = t["windows"].as_array().map_or(&[][..], Vec::as_slice);
        let window = windows
            .iter()
            .find(|w| w["id"] == t["active_window"])
            .unwrap_or(&t["windows"][0]);
        let pane = window["panes"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["id"] == window["active_pane"]))
            .unwrap_or(&window["panes"][0]);
        let panes: usize = windows
            .iter()
            .map(|w| w["panes"].as_array().map_or(0, Vec::len))
            .sum();
        let place = (windows.len() > 1 || panes > 1).then(|| {
            format!(
                "window {}:{} pane {} ({} window{}, {} pane{})",
                window["index"],
                window["name"].as_str().unwrap_or("?"),
                pane["index"],
                windows.len(),
                if windows.len() == 1 { "" } else { "s" },
                panes,
                if panes == 1 { "" } else { "s" },
            )
        });
        let status = window_list(t);
        let program = pane["executable"]
            .as_str()
            .or_else(|| pane["command"]["program"].as_str())
            .unwrap_or("");
        let program = program
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(program)
            .to_string();
        Some(Self {
            id: t["id"].as_str()?.to_string(),
            name: t["name"].as_str()?.to_string(),
            state: t["state"].as_str().unwrap_or("unknown").to_string(),
            pid: pane["pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
            program,
            history_limit: pane["history"]["limit_lines"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(2000),
            place,
            status,
            claude: t["claude_state"]
                .as_str()
                .and_then(crate::gui::float::art::Claude::parse)
                .unwrap_or(crate::gui::float::art::Claude::None),
            active_pane: pane["id"].as_str().map(String::from),
            layout: WinLayout::from_window(window),
            windows: windows
                .iter()
                .filter_map(|w| {
                    Some((
                        w["id"].as_str()?.to_string(),
                        w["index"].as_u64()?,
                        w["name"].as_str().unwrap_or("").to_string(),
                    ))
                })
                .collect(),
            active_window: window["id"].as_str().map(String::from),
        })
    }
}

/// Why the shown Terminal is no longer live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// Its program exited.
    Exited(Option<i64>),
    /// Another client detached this window (`subak detach`); the Terminal still runs.
    DetachedBy(Option<String>),
    /// It was removed (killed or removed by another client).
    Removed,
    /// It was stopped and is kept as `stopped`.
    Stopped,
}

/// What the window shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Connecting (and starting a Host on demand on the first connection).
    Connecting,
    /// Connected; no tab. A Terminal created later opens by itself.
    Empty { notice: Option<String> },
    /// The active tab's `pane.attach` is on its way (or waits until it runs).
    Attaching(TermInfo),
    /// The active tab's pane is shown live.
    Attached {
        term: TermInfo,
        stream: u32,
        pane_pid: u32,
        pane_id: String,
    },
    /// The active tab's pane was live and is not any more; its last screen stays.
    Ended { term: TermInfo, why: EndReason },
    /// The active tab's Terminal is kept by the Host without a process (stopped or failed):
    /// nothing to attach; `Ctrl+W` removes it.
    Stored(TermInfo),
    /// The connection to the Host is gone; reconnecting (the last screen stays).
    Disconnected {
        last: Option<TermInfo>,
        attempt: u32,
        reason: String,
    },
    /// The first connection failed (no Host could be reached or started).
    Failed {
        code: String,
        message: String,
        hint: Option<String>,
    },
}

impl Phase {
    /// A short name for logs and tests.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Empty { .. } => "empty",
            Self::Attaching(_) => "attaching",
            Self::Attached { .. } => "attached",
            Self::Ended { .. } => "ended",
            Self::Stored(_) => "stored",
            Self::Disconnected { .. } => "disconnected",
            Self::Failed { .. } => "failed",
        }
    }

    /// Whether the window shows a pane's screen (live or the last one) rather than a state screen.
    pub fn shows_pane(&self) -> bool {
        matches!(
            self,
            Self::Attached { .. } | Self::Ended { .. } | Self::Disconnected { last: Some(_), .. }
        )
    }
}

/// A Terminal the user asked to close while it is busy: the window asks before `force`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub id: String,
    pub name: String,
    /// The pane's program (redacted by the Host).
    pub program: String,
    /// Processes in its job.
    pub processes: u64,
    /// Step 22: the busy pane to end (`pane.kill` with `force`) instead of the whole Terminal.
    pub pane: Option<String>,
    /// #98 T1 (`CORE-14`): asked because the tab is pinned.
    pub pinned: bool,
    /// #98 T1 (`CORE-17`): confirming restarts the Terminal (`terminal.duplicate` with
    /// `restart` and `force`) instead of ending it.
    pub restart: bool,
}

/// What the window must do after the model handled something.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send this control message (a request with its id).
    Send(Value),
    /// The phase changed: redraw, retitle, report it (tests).
    Phase,
    /// The tabs changed (list, order, names, states, the active one): redraw the tab bar.
    Tabs,
    /// Start a fresh view of `cols × rows` keeping `history` lines: a snapshot begins.
    ResetView {
        cols: u16,
        rows: u16,
        history: usize,
    },
    /// Remember this Terminal as the one shown last (preferences).
    Remember(Box<TermInfo>),
    /// Try to connect again after this delay (never starting a Host).
    Reconnect(Duration),
    /// Connect again now, starting a Host on demand (the user retried a failed start).
    Connect,
    /// Tell the user something (a refused or failed action) in a banner.
    Notice(String),
    /// Ask the user before ending this busy Terminal.
    Confirm(Confirm),
    /// The rename the user asked for finished (`Err`: why it was refused).
    Renamed(Result<(), String>),
    /// Step 22: the read-only pane views changed (added, attached, gone).
    Views,
    /// Step 24: the answer of a command-prompt query (`list-panes`, `show-buffer`, …) to show.
    Show { method: &'static str, result: Value },
    /// #98 T3 (`CORE-79`): a command or a Terminal's program finished (the window decides
    /// whether that is worth a notification).
    Finished(Finished),
}

/// #98 T3: what finished, for [`Effect::Finished`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub terminal_id: String,
    pub name: String,
    /// A shell-integration command (OSC 133) rather than the Terminal's program.
    pub command: bool,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<u64>,
}

/// What a data frame is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Not the shown pane's stream (or nothing is attached): drop it.
    Ignore,
    /// Feed it to the view.
    Feed,
    /// A snapshot begins: reset the view first (see [`Effect::ResetView`]), then feed it.
    Reset {
        cols: u16,
        rows: u16,
        history: usize,
    },
}

/// What the user asked for on the command line or what is remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    /// Nothing: the last one, else the first running tab.
    Any,
    /// `subak-gui <target>`: a name or an id.
    Target(String),
    /// Reopen this Terminal (remembered from the last run, or the one shown before a disconnect).
    Last { id: String, name: String },
}

// A few requests are in flight at a time; boxing the Terminal of `Attach` would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
enum Pending {
    List,
    Subscribe,
    Attach(Option<TermInfo>),
    Resize,
    Detach,
    Create,
    /// #98 T1: `terminal.duplicate` of Terminal `id` (at tab `index`; `restart`: it ends).
    Duplicate {
        id: String,
        name: String,
        restart: bool,
    },
    Kill {
        id: String,
        name: String,
    },
    Remove {
        id: String,
        name: String,
    },
    Rename,
    /// Step 22: a read-only view of this pane.
    View(String),
    /// Step 22: a layout request (`pane.split`, `pane.select`, …) from a key or a click, and
    /// its target pane.
    PaneOp(&'static str, String),
    /// Step 24: a command-prompt query whose answer is shown.
    Query(&'static str),
}

/// Retry delays after a disconnect: 0.5, 1, 2, 4, 8 s, then every 10 s.
pub fn reconnect_delay(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1 << attempt.min(5));
    Duration::from_millis(ms.min(10_000))
}

/// The model.
#[derive(Debug)]
pub struct Model {
    pub phase: Phase,
    /// The tabs, in display order.
    tabs: Vec<TermInfo>,
    /// Terminals hidden in this window (they keep running and are listed by `subak ls`).
    hidden: Vec<TermInfo>,
    /// Terminals this window is closing (`Ctrl+W`): their removal closes the tab quietly.
    closing: Vec<String>,
    /// A starting Terminal the active tab waits for (attached once it runs).
    awaiting: Option<String>,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    want: Want,
    /// The grid the window shows (what attach and resize ask for).
    grid: (u16, u16),
    /// Inside a run of snapshot frames.
    in_snapshot: bool,
    /// The size of the snapshot that follows an attach or a resync.
    snapshot_size: Option<(u16, u16)>,
    /// The Host's reason when it announced that it stops.
    host_stopping: Option<String>,
    /// Whether a connection is up.
    connected: bool,
    /// Terminals hidden in this window's last run (`gui.json`, Step 18): they go to `hidden`
    /// instead of the tabs when the first list arrives.
    pre_hidden: Vec<String>,
    /// Step 22: the read-only views of the shown window's other visible panes.
    views: Vec<PaneViewState>,
    /// #98 T3 (`CORE-80`): Terminals not shown that had output or a bell since they were
    /// last shown.
    alerts: HashMap<String, Alert>,
    /// #98 T3 (`CORE-78`): the progress each Terminal's program reports (OSC 9;4).
    progress: HashMap<String, subak_term::screen::Progress>,
    /// #98 T1 (`CORE-15`): Terminals in the order this window last showed them (newest first).
    mru: Vec<String>,
    /// #98 T1 (`CORE-14`): pinned Terminals (ids): first in the bar, closing one asks.
    pinned: Vec<String>,
    /// #98 T1 (`CORE-10`): the next/previous tab keys wrap around (`tabs.cycle`).
    wrap: bool,
    /// #98 T1: the tab now shown came from a walk through the MRU order (it is not moved).
    mru_walk: bool,
}

/// #98 T3: what happened in a Terminal that is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alert {
    Output,
    Bell,
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(String::from)
}

impl Model {
    pub fn new(want: Want, grid: (u16, u16)) -> Self {
        Self {
            phase: Phase::Connecting,
            tabs: Vec::new(),
            hidden: Vec::new(),
            closing: Vec::new(),
            awaiting: None,
            next_id: 1,
            pending: HashMap::new(),
            want,
            grid,
            in_snapshot: false,
            snapshot_size: None,
            host_stopping: None,
            connected: false,
            pre_hidden: Vec::new(),
            views: Vec::new(),
            alerts: HashMap::new(),
            progress: HashMap::new(),
            mru: Vec::new(),
            pinned: Vec::new(),
            wrap: true,
            mru_walk: false,
        }
    }

    /// #98 T1: the pinned Terminals of the last run (`gui.json`).
    pub fn with_pinned(mut self, ids: Vec<String>) -> Self {
        self.pinned = ids;
        self
    }

    /// #98 T1: whether Terminal `id` is pinned.
    pub fn pinned(&self, id: &str) -> bool {
        self.pinned.iter().any(|p| p == id)
    }

    /// #98 T1: the pinned ids that are still tabs (what `gui.json` keeps).
    pub fn pinned_ids(&self) -> Vec<String> {
        self.pinned
            .iter()
            .filter(|p| {
                self.tabs
                    .iter()
                    .chain(self.hidden.iter())
                    .any(|t| &t.id == *p)
            })
            .cloned()
            .collect()
    }

    /// #98 T1 (`tabs.cycle`): whether next/previous wrap around.
    pub fn set_wrap(&mut self, on: bool) {
        self.wrap = on;
    }

    /// #98 T1 (`CORE-14`): pins or unpins the shown tab; pinned tabs come first (in the order
    /// they were pinned), the others keep their order.
    pub fn toggle_pin(&mut self) -> Vec<Effect> {
        let Some(id) = self.terminal().map(|t| t.id.clone()) else {
            return Vec::new();
        };
        if self.pinned(&id) {
            self.pinned.retain(|p| *p != id);
        } else {
            self.pinned.push(id);
        }
        self.sort_pinned();
        vec![Effect::Tabs]
    }

    /// Pinned tabs first, by pin order; the rest keep their order.
    fn sort_pinned(&mut self) {
        let pin = |id: &str| self.pinned.iter().position(|p| p == id);
        let mut tabs = std::mem::take(&mut self.tabs);
        tabs.sort_by_key(|t| pin(&t.id).unwrap_or(usize::MAX));
        self.tabs = tabs;
    }

    /// #98 T1 (`CORE-15`): `delta` 1 shows the most recently shown other tab (`tab.next-mru`
    /// pressed again goes further back), -1 goes the other way (`tab.previous-mru`).
    pub fn cycle_mru(&mut self, delta: isize) -> Vec<Effect> {
        let order: Vec<String> = self
            .mru
            .iter()
            .filter(|id| self.tabs.iter().any(|t| &t.id == *id))
            .cloned()
            .chain(
                self.tabs
                    .iter()
                    .filter(|t| !self.mru.contains(&t.id))
                    .map(|t| t.id.clone()),
            )
            .collect();
        let n = order.len();
        if n < 2 || !self.connected {
            return Vec::new();
        }
        let cur = self
            .terminal()
            .and_then(|t| order.iter().position(|o| *o == t.id))
            .unwrap_or(0);
        let next = (cur.cast_signed() + delta).rem_euclid(n.cast_signed()) as usize;
        let id = order[next].clone();
        // Walking the order must not reorder it until the user settles (`settle_mru`).
        self.mru_walk = true;
        match self.tabs.iter().position(|t| t.id == id) {
            Some(i) => self.select(i),
            None => Vec::new(),
        }
    }

    /// #98 T1 (`CORE-15`): `tab.last`: the tab shown before this one.
    pub fn last_tab(&mut self) -> Vec<Effect> {
        let cur = self.terminal().map(|t| t.id.clone());
        let prev = self
            .mru
            .iter()
            .find(|id| Some(*id) != cur.as_ref() && self.tabs.iter().any(|t| &t.id == *id))
            .cloned();
        match prev.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
            Some(i) => self.select(i),
            None => Vec::new(),
        }
    }

    /// #98 T1: the MRU order as shown (tests, the dump).
    pub fn mru(&self) -> &[String] {
        &self.mru
    }

    /// #98 T3: the progress Terminal `id` reports, if any.
    pub fn progress(&self, id: &str) -> Option<subak_term::screen::Progress> {
        self.progress.get(id).copied()
    }

    /// The tab name of Terminal `id` (its id when it is not a tab).
    fn name_of(&self, id: &str) -> String {
        self.tabs
            .iter()
            .chain(self.hidden.iter())
            .find(|t| t.id == id)
            .map_or_else(|| id.to_string(), |t| t.name.clone())
    }

    /// #98 T3: the alert of Terminal `id` (never the shown one: seeing it clears it).
    pub fn alert(&self, id: &str) -> Option<Alert> {
        if self.terminal().is_some_and(|t| t.id == id) {
            return None;
        }
        self.alerts.get(id).copied()
    }

    /// #98 T3: the shown Terminal was seen: its alert goes.
    pub fn seen(&mut self) {
        if let Some(id) = self.terminal().map(|t| t.id.clone()) {
            self.alerts.remove(&id);
        }
    }

    /// Starts with these Terminals hidden (by id; the ones the window hid in its last run).
    pub fn with_hidden(mut self, ids: Vec<String>) -> Self {
        self.pre_hidden = ids;
        self
    }

    /// The ids of the hidden Terminals (persisted in `gui.json`).
    pub fn hidden_ids(&self) -> Vec<String> {
        self.hidden.iter().map(|t| t.id.clone()).collect()
    }

    /// Another Subak client (the Terminal manager, Step 18) asks this window to show the
    /// Terminal `id`: its tab is shown (a hidden one comes back first). A Terminal this window
    /// does not know yet (created a moment ago) is attached by id; its tab follows the answer.
    pub fn open_id(&mut self, id: &str) -> Vec<Effect> {
        if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
            return self.select(i);
        }
        if self.hidden.iter().any(|t| t.id == id) {
            return self.unhide(id);
        }
        if !self.connected {
            self.want = Want::Target(id.to_string());
            return Vec::new();
        }
        let mut out = Vec::new();
        let t = TermInfo {
            id: id.to_string(),
            name: id.to_string(),
            state: "running".into(),
            pid: None,
            program: String::new(),
            history_limit: 2000,
            place: None,
            status: None,
            claude: crate::gui::float::art::Claude::None,
            active_pane: None,
            layout: None,
            windows: Vec::new(),
            active_window: None,
        };
        self.show(t, &mut out);
        out.push(self.request("terminal.list", json!({"detail": "full"}), Pending::List));
        out
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    /// Inside a run of snapshot frames (or waiting for the snapshot of an attach or resync): the
    /// view keeps the snapshot's size until it is complete.
    pub fn in_snapshot(&self) -> bool {
        self.in_snapshot || self.snapshot_size.is_some()
    }

    /// The shown Terminal (the active tab), live or not.
    pub fn terminal(&self) -> Option<&TermInfo> {
        match &self.phase {
            Phase::Attached { term, .. }
            | Phase::Ended { term, .. }
            | Phase::Attaching(term)
            | Phase::Stored(term) => Some(term),
            Phase::Disconnected { last, .. } => last.as_ref(),
            _ => None,
        }
    }

    /// The tabs, in display order.
    pub fn tabs(&self) -> &[TermInfo] {
        &self.tabs
    }

    /// The Terminals hidden in this window.
    pub fn hidden(&self) -> &[TermInfo] {
        &self.hidden
    }

    /// The active tab's index.
    pub fn active(&self) -> Option<usize> {
        let id = &self.terminal()?.id;
        self.tabs.iter().position(|t| &t.id == id)
    }

    /// The stream to send input to, while the pane is live.
    pub fn input_stream(&self) -> Option<u32> {
        match self.phase {
            Phase::Attached { stream, .. } => Some(stream),
            _ => None,
        }
    }

    fn request(&mut self, method: &str, params: Value, p: Pending) -> Effect {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, p);
        let mut msg = json!({"type": "request", "id": id, "method": method});
        msg["params"] = params;
        Effect::Send(msg)
    }

    fn set(&mut self, phase: Phase, out: &mut Vec<Effect>) {
        if self.phase != phase {
            self.phase = phase;
            out.push(Effect::Phase);
            out.push(Effect::Tabs);
        }
    }

    /// A connection is up (handshake done): subscribe, then list.
    pub fn on_connected(&mut self) -> Vec<Effect> {
        self.connected = true;
        self.pending.clear();
        self.closing.clear();
        self.awaiting = None;
        self.host_stopping = None;
        self.in_snapshot = false;
        let mut out = vec![
            self.request(
                "events.subscribe",
                json!({"topics": ["terminals", "panes", "host"]}),
                Pending::Subscribe,
            ),
            self.request("terminal.list", json!({"detail": "full"}), Pending::List),
        ];
        if !self.phase.shows_pane() {
            self.set(Phase::Connecting, &mut out);
        }
        out.push(Effect::Tabs);
        out
    }

    /// A connection attempt failed. The first connection shows the error and waits for the user;
    /// a reconnection tries again later.
    pub fn on_connect_failed(
        &mut self,
        code: &str,
        message: &str,
        hint: Option<&str>,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        match &self.phase {
            Phase::Disconnected {
                last,
                attempt,
                reason,
            } => {
                let next = Phase::Disconnected {
                    last: last.clone(),
                    attempt: attempt + 1,
                    reason: reason.clone(),
                };
                out.push(Effect::Reconnect(reconnect_delay(attempt + 1)));
                self.set(next, &mut out);
            }
            _ => self.set(
                Phase::Failed {
                    code: code.to_string(),
                    message: message.to_string(),
                    hint: hint.map(String::from),
                },
                &mut out,
            ),
        }
        out
    }

    /// The connection ended (the Host stopped or crashed, or the pipe broke).
    pub fn on_disconnected(&mut self) -> Vec<Effect> {
        self.connected = false;
        self.views.clear();
        self.pending.clear();
        self.closing.clear();
        self.awaiting = None;
        self.in_snapshot = false;
        let last = self.terminal().cloned();
        if let Some(t) = &last {
            self.want = Want::Last {
                id: t.id.clone(),
                name: t.name.clone(),
            };
        }
        let reason = match self.host_stopping.take().as_deref() {
            Some("idle") => "the Host exited (idle)".to_string(),
            Some(_) => "the Host was stopped".to_string(),
            None => "the connection to the Host was lost".to_string(),
        };
        let mut out = vec![Effect::Reconnect(reconnect_delay(0))];
        self.set(
            Phase::Disconnected {
                last,
                attempt: 0,
                reason,
            },
            &mut out,
        );
        out
    }

    /// The user pressed Enter (or clicked the action) on a state screen or banner.
    pub fn activate(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        match self.phase.clone() {
            Phase::Failed { .. } => {
                out.push(Effect::Connect);
                self.set(Phase::Connecting, &mut out);
            }
            Phase::Disconnected { .. } => out.push(Effect::Reconnect(Duration::ZERO)),
            Phase::Ended {
                term,
                why: EndReason::DetachedBy(_),
            } if self.connected => {
                out.push(self.attach(Some(term.clone()), &term.id));
                self.set(Phase::Attaching(term), &mut out);
            }
            Phase::Ended { term, .. } if term.gone() && self.connected => {
                self.close_tab(&term.id, &mut out);
            }
            Phase::Empty { .. } if self.connected => {
                self.want = Want::Any;
                out.push(self.request("terminal.list", json!({"detail": "full"}), Pending::List));
            }
            _ => {}
        }
        out
    }

    fn attach(&mut self, term: Option<TermInfo>, target: &str) -> Effect {
        let (cols, rows) = self.grid;
        self.in_snapshot = false;
        self.snapshot_size = None;
        self.request(
            "pane.attach",
            json!({"target": target, "interactive": true, "size": {"cols": cols, "rows": rows}}),
            Pending::Attach(term),
        )
    }

    /// Leaves the active tab (detaching its pane) and shows `t`.
    fn show(&mut self, t: TermInfo, out: &mut Vec<Effect>) {
        if let Phase::Attached { stream, .. } = self.phase {
            out.push(self.request("pane.detach", json!({"stream_id": stream}), Pending::Detach));
        }
        // A tab whose Terminal is gone disappears once it is left.
        if let Some(cur) = self.terminal().cloned()
            && cur.gone()
            && cur.id != t.id
        {
            self.tabs.retain(|x| x.id != cur.id);
        }
        self.awaiting = None;
        if t.running() {
            out.push(self.attach(Some(t.clone()), &t.id));
            self.set(Phase::Attaching(t), out);
        } else if t.stored() || t.gone() {
            self.set(Phase::Stored(t), out);
        } else {
            // Starting (or stopping): attach once it runs.
            self.awaiting = Some(t.id.clone());
            self.set(Phase::Attaching(t), out);
        }
        out.push(Effect::Tabs);
    }

    /// Shows tab `i`.
    pub fn select(&mut self, i: usize) -> Vec<Effect> {
        let mut out = Vec::new();
        if !self.connected {
            return out;
        }
        if let Some(t) = self.tabs.get(i).cloned()
            && self.active() != Some(i)
        {
            self.show(t, &mut out);
        }
        out
    }

    /// Shows the next (`delta` 1) or previous (`-1`) tab, wrapping around.
    pub fn cycle(&mut self, delta: isize) -> Vec<Effect> {
        let n = self.tabs.len();
        if n == 0 || !self.connected {
            return Vec::new();
        }
        let cur = self.active().unwrap_or(0).cast_signed();
        let to = cur + delta;
        // #98 T1 (`tabs.cycle` off): the ends stop instead of wrapping around.
        if !self.wrap && !(0..n.cast_signed()).contains(&to) {
            return Vec::new();
        }
        let next = to.rem_euclid(n.cast_signed()) as usize;
        self.select(next)
    }

    /// Moves tab `i` by `delta` places (the order is this window's; the Host keeps its own).
    pub fn move_tab(&mut self, i: usize, delta: isize) -> Vec<Effect> {
        let n = self.tabs.len();
        let to = i.cast_signed() + delta;
        if i >= n || to < 0 || to >= n.cast_signed() {
            return Vec::new();
        }
        let t = self.tabs.remove(i);
        self.tabs.insert(to as usize, t);
        vec![Effect::Tabs]
    }

    /// `terminal.new` (`Ctrl+T`, `+`): a Terminal with the Host's defaults (its default shell
    /// in the user's profile folder), at this window's size; shown once it runs.
    #[cfg(test)]
    pub fn create(&mut self) -> Vec<Effect> {
        self.create_with(None)
    }

    /// Creates a Terminal with extra `terminal.create` parameters (Step 20: a shell profile's
    /// explicit `command`, `cwd` and `env`; the Host never reads profiles itself).
    pub fn create_with(&mut self, extra: Option<&Value>) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Notice(
                "Not connected to the Subak Host; no Terminal was created".into(),
            )];
        }
        let (cols, rows) = self.grid;
        let mut params = json!({"size": {"cols": cols, "rows": rows}});
        if let Some(Value::Object(m)) = extra {
            for (k, v) in m {
                if !v.is_null() {
                    params[k.as_str()] = v.clone();
                }
            }
        }
        vec![self.request("terminal.create", params, Pending::Create)]
    }

    /// Hides tab `i`: a detach of this window only. The Terminal keeps running (ADR 0020 §7).
    pub fn hide(&mut self, i: usize) -> Vec<Effect> {
        let mut out = Vec::new();
        let Some(t) = self.tabs.get(i).cloned() else {
            return out;
        };
        let was_active = self.active() == Some(i);
        self.tabs.remove(i);
        if !t.gone() {
            self.hidden.push(t);
        }
        if was_active {
            if let Phase::Attached { stream, .. } = self.phase {
                out.push(self.request(
                    "pane.detach",
                    json!({"stream_id": stream}),
                    Pending::Detach,
                ));
            }
            self.after_close(i, &mut out);
        }
        out.push(Effect::Tabs);
        out
    }

    /// Shows a hidden Terminal again as the last tab, and opens it.
    pub fn unhide(&mut self, id: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Some(k) = self.hidden.iter().position(|t| t.id == id) {
            let t = self.hidden.remove(k);
            self.tabs.push(t.clone());
            if self.connected {
                self.show(t, &mut out);
            }
            out.push(Effect::Tabs);
        }
        out
    }

    /// After the active tab at `i` was taken away: show the tab now at `i` (the right
    /// neighbour), else the one before it, else nothing.
    fn after_close(&mut self, i: usize, out: &mut Vec<Effect>) {
        self.awaiting = None;
        let next = self
            .tabs
            .get(i)
            .or_else(|| i.checked_sub(1).and_then(|j| self.tabs.get(j)))
            .cloned();
        match next {
            Some(t) if self.connected => {
                // The old phase's pane is no longer ours: no detach for it.
                self.phase = Phase::Empty { notice: None };
                self.show(t, out);
            }
            _ => self.set(Phase::Empty { notice: None }, out),
        }
    }

    /// Takes the tab of `id` away (closed or removed by this window).
    fn close_tab(&mut self, id: &str, out: &mut Vec<Effect>) {
        self.closing.retain(|c| c != id);
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let was_active = self.active() == Some(i);
        self.tabs.remove(i);
        if was_active {
            self.after_close(i, out);
        }
        out.push(Effect::Tabs);
    }

    /// `terminal.close` (`Ctrl+W`, "Close Terminal") on the active tab: kill a live Terminal
    /// (confirmed when busy), remove a stored one, close the tab of one that is gone.
    pub fn close_active(&mut self) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Notice(
                "Not connected to the Subak Host; nothing was closed".into(),
            )];
        }
        let Some(t) = self.terminal().cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if t.gone() {
            self.close_tab(&t.id, &mut out);
        } else if self.pinned(&t.id) && !t.stored() {
            // #98 T1 (`CORE-14`): a pinned Terminal asks before it ends.
            out.push(Effect::Confirm(Confirm {
                id: t.id.clone(),
                name: t.name.clone(),
                program: t.program.clone(),
                processes: 0,
                pane: None,
                pinned: true,
                restart: false,
            }));
        } else if t.stored() {
            self.closing.push(t.id.clone());
            out.push(self.request(
                "terminal.remove",
                json!({"target": t.id}),
                Pending::Remove {
                    id: t.id.clone(),
                    name: t.name.clone(),
                },
            ));
        } else {
            out.push(self.kill(&t, false));
        }
        out
    }

    fn kill(&mut self, t: &TermInfo, force: bool) -> Effect {
        self.closing.push(t.id.clone());
        self.request(
            "terminal.kill",
            json!({"target": t.id, "force": force}),
            Pending::Kill {
                id: t.id.clone(),
                name: t.name.clone(),
            },
        )
    }

    /// #98 T1 (`CORE-12`, `CORE-17`): duplicates the shown Terminal (a new one running the same
    /// program in the same folder), or restarts it (`restart`; `force` once the user agreed).
    pub fn duplicate(&mut self, restart: bool, force: bool) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Notice(
                "Not connected to the Subak Host; nothing was started".into(),
            )];
        }
        let Some(t) = self.terminal().cloned() else {
            return Vec::new();
        };
        if t.gone() || t.stored() {
            return vec![Effect::Notice(format!(
                "`{}` does not run; nothing to {}",
                t.name,
                if restart { "restart" } else { "duplicate" }
            ))];
        }
        vec![self.request(
            "terminal.duplicate",
            json!({"target": t.id, "restart": restart, "force": force}),
            Pending::Duplicate {
                id: t.id.clone(),
                name: t.name,
                restart,
            },
        )]
    }

    /// The user confirmed ending the busy Terminal `id`: `terminal.kill` with `force`, by id.
    pub fn confirm_close(&mut self, id: &str) -> Vec<Effect> {
        let Some(t) = self.tabs.iter().find(|t| t.id == id).cloned() else {
            return vec![Effect::Notice(
                "That Terminal is no longer open; nothing was ended".into(),
            )];
        };
        if !self.connected || t.gone() || t.stored() {
            return vec![Effect::Notice(format!(
                "`{}` no longer runs; nothing was ended",
                t.name
            ))];
        }
        vec![self.kill(&t, true)]
    }

    /// `terminal.rename` of `id` (by id, so a concurrent rename cannot redirect it).
    pub fn rename(&mut self, id: &str, name: &str) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Renamed(Err(
                "not connected to the Subak Host".into()
            ))];
        }
        vec![self.request(
            "terminal.rename",
            json!({"target": id, "name": name}),
            Pending::Rename,
        )]
    }

    /// The window's grid changed (a resize, a DPI or font change). A live pane is resized to it;
    /// the view is resized by the window at once.
    pub fn on_grid(&mut self, cols: u16, rows: u16) -> Vec<Effect> {
        if self.grid == (cols, rows) {
            return Vec::new();
        }
        self.grid = (cols, rows);
        match self.phase {
            Phase::Attached { stream, .. } => vec![self.request(
                "pane.resize",
                json!({"cols": cols, "rows": rows, "stream_id": stream}),
                Pending::Resize,
            )],
            _ => Vec::new(),
        }
    }

    /// Closing the window: detach (every Terminal keeps running).
    pub fn on_close(&mut self) -> Vec<Effect> {
        match self.phase {
            Phase::Attached { .. } | Phase::Attaching(_) if self.connected => {
                vec![self.request("pane.detach", json!({}), Pending::Detach)]
            }
            _ => Vec::new(),
        }
    }

    /// A data frame arrived.
    pub fn on_data(&mut self, stream: u32, snapshot: bool) -> Frame {
        let Phase::Attached { stream: mine, .. } = self.phase else {
            return Frame::Ignore;
        };
        if stream != mine {
            return Frame::Ignore;
        }
        let begins = snapshot && !self.in_snapshot;
        self.in_snapshot = snapshot;
        if begins {
            let (cols, rows) = self.snapshot_size.take().unwrap_or(self.grid);
            let history = self.terminal().map_or(2000, |t| t.history_limit);
            return Frame::Reset {
                cols,
                rows,
                history,
            };
        }
        Frame::Feed
    }

    /// A control message (response or event) arrived.
    pub fn on_control(&mut self, m: &Value) -> Vec<Effect> {
        let mut out = match m["type"].as_str() {
            Some("response") => {
                let Some(p) = m["id"].as_u64().and_then(|id| self.pending.remove(&id)) else {
                    return Vec::new();
                };
                if m["ok"] == true {
                    self.on_result(p, &m["result"])
                } else {
                    self.on_error(p, &m["error"])
                }
            }
            Some("event") => self.on_event(m["event"].as_str().unwrap_or(""), &m["data"]),
            _ => Vec::new(),
        };
        self.sync_views(&mut out);
        out
    }

    /// Step 22: the shown window's layout while it shows more than one pane live.
    pub fn layout(&self) -> Option<&WinLayout> {
        match &self.phase {
            Phase::Attached { term, .. } => term.layout.as_ref().filter(|l| l.split()),
            _ => None,
        }
    }

    /// Step 22: where the active (interactive) pane is drawn, when the window is split.
    pub fn active_slot(&self) -> Option<&Slot> {
        let Phase::Attached { pane_id, .. } = &self.phase else {
            return None;
        };
        self.layout()?.slot(pane_id)
    }

    /// Step 22: the read-only views of the other visible panes.
    pub fn views(&self) -> &[PaneViewState] {
        &self.views
    }

    /// Step 22: a data frame of a read-only view: its pane and what to do with it.
    pub fn view_data(&mut self, stream: u32, snapshot: bool) -> Option<(String, Frame)> {
        let v = self.views.iter_mut().find(|v| v.stream == Some(stream))?;
        let begins = snapshot && !v.in_snapshot;
        v.in_snapshot = snapshot;
        if begins {
            let (cols, rows) = v.snapshot_size.take().unwrap_or((80, 24));
            return Some((
                v.pane_id.clone(),
                Frame::Reset {
                    cols,
                    rows,
                    history: 0,
                },
            ));
        }
        Some((v.pane_id.clone(), Frame::Feed))
    }

    /// Step 22: attaches a read-only view of each visible pane other than the active one, and
    /// detaches views nobody sees any more (another tab, a zoom, a pane gone).
    fn sync_views(&mut self, out: &mut Vec<Effect>) {
        let want: Vec<String> = match (&self.phase, self.connected) {
            (Phase::Attached { pane_id, .. }, true) => self
                .layout()
                .map(|l| {
                    l.slots
                        .iter()
                        .filter(|s| s.visible && s.id != *pane_id)
                        .map(|s| s.id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        // The pane the window's own (interactive) attachment is on or goes to: the Host keeps
        // one attachment per stream and connection, so its view is forgotten, never detached
        // (that would end the interactive attachment that replaced it).
        let main = match &self.phase {
            Phase::Attached { pane_id, .. } => Some(pane_id.clone()),
            Phase::Attaching(t) => t.active_pane.clone(),
            _ => None,
        };
        let mut changed = false;
        let gone: Vec<PaneViewState> = self
            .views
            .iter()
            .filter(|v| !want.contains(&v.pane_id))
            .cloned()
            .collect();
        for v in gone {
            self.views.retain(|x| x.pane_id != v.pane_id);
            if let Some(s) = v.stream
                && main.as_deref() != Some(v.pane_id.as_str())
            {
                out.push(self.request("pane.detach", json!({"stream_id": s}), Pending::Detach));
            }
            changed = true;
        }
        for id in want {
            if self.views.iter().any(|v| v.pane_id == id) {
                continue;
            }
            out.push(self.request(
                "pane.attach",
                json!({"target": id, "interactive": false, "history_lines": 0}),
                Pending::View(id.clone()),
            ));
            self.views.push(PaneViewState {
                pane_id: id,
                stream: None,
                in_snapshot: false,
                snapshot_size: None,
                failed: false,
            });
            changed = true;
        }
        if changed {
            out.push(Effect::Views);
        }
    }

    /// Step 24: a request of the command prompt as planned (`params` complete); a query's answer
    /// comes back as [`Effect::Show`].
    pub fn host_op(&mut self, method: &'static str, params: Value, show: bool) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Notice(
                "Not connected to the Subak Host; nothing was run".into(),
            )];
        }
        let target = params["target"].as_str().unwrap_or("").to_string();
        let p = if show {
            Pending::Query(method)
        } else {
            Pending::PaneOp(method, target)
        };
        vec![self.request(method, params, p)]
    }

    /// Step 22: a layout request for the active pane (or `target`), from a key or a click:
    /// `pane.split`, `pane.select`, `pane.adjust`, `pane.zoom`, `pane.kill`. A refusal is a
    /// notice; the result arrives as the next layout.
    pub fn pane_op(&mut self, method: &'static str, mut params: Value) -> Vec<Effect> {
        if !self.connected {
            return vec![Effect::Notice(
                "Not connected to the Subak Host; nothing was changed".into(),
            )];
        }
        let Phase::Attached { pane_id, .. } = &self.phase else {
            return Vec::new();
        };
        if params["target"].is_null() {
            params["target"] = json!(pane_id);
        }
        let target = params["target"].as_str().unwrap_or("").to_string();
        vec![self.request(method, params, Pending::PaneOp(method, target))]
    }

    fn on_result(&mut self, p: Pending, r: &Value) -> Vec<Effect> {
        let mut out = Vec::new();
        match p {
            Pending::List => {
                let list: Vec<TermInfo> = r["terminals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(TermInfo::from_full)
                    .collect();
                self.merge(&list);
                self.sort_pinned();
                self.decide(&mut out);
                self.follow(&mut out);
                out.push(Effect::Tabs);
            }
            Pending::Attach(known) => {
                let (Some(stream), Some(tid)) = (
                    r["stream_id"].as_u64().and_then(|s| u32::try_from(s).ok()),
                    r["terminal_id"].as_str(),
                ) else {
                    return out;
                };
                // The user moved on while this attach was on its way: let it go.
                let wanted = match &self.phase {
                    Phase::Attaching(t) => t.id == tid || known.is_none(),
                    _ => false,
                };
                if !wanted {
                    out.push(self.request(
                        "pane.detach",
                        json!({"stream_id": stream}),
                        Pending::Detach,
                    ));
                    return out;
                }
                let mut term = known
                    .or_else(|| self.tabs.iter().find(|t| t.id == tid).cloned())
                    .unwrap_or_else(|| TermInfo {
                        id: tid.to_string(),
                        name: tid.to_string(),
                        state: "running".into(),
                        pid: None,
                        program: String::new(),
                        history_limit: 2000,
                        place: None,
                        status: None,
                        claude: crate::gui::float::art::Claude::None,
                        active_pane: None,
                        layout: None,
                        windows: Vec::new(),
                        active_window: None,
                    });
                term.id = tid.to_string();
                if let Some(t) = self.tabs.iter().find(|t| t.id == tid) {
                    term.name.clone_from(&t.name);
                }
                term.state = "running".into();
                let pane_pid = r["pane"]["pid"]
                    .as_u64()
                    .and_then(|p| u32::try_from(p).ok())
                    .unwrap_or(0);
                term.pid = Some(pane_pid);
                if !self.tabs.iter().any(|t| t.id == tid) {
                    self.hidden.retain(|h| h.id != tid);
                    self.tabs.push(term.clone());
                }
                let size = &r["snapshot"]["size"];
                self.snapshot_size = match (size["cols"].as_u64(), size["rows"].as_u64()) {
                    // A snapshot of no frames would never begin: nothing to wait for.
                    _ if r["snapshot"]["frames"].as_u64() == Some(0) => None,
                    (Some(c), Some(r)) => Some((
                        u16::try_from(c).unwrap_or(self.grid.0),
                        u16::try_from(r).unwrap_or(self.grid.1),
                    )),
                    _ => None,
                };
                self.in_snapshot = false;
                self.want = Want::Last {
                    id: term.id.clone(),
                    name: term.name.clone(),
                };
                // #98 T1 (`CORE-15`): the most recently shown first (a walk through the order
                // keeps it as it is).
                if !std::mem::take(&mut self.mru_walk) {
                    self.mru.retain(|m| *m != term.id);
                    self.mru.insert(0, term.id.clone());
                }
                out.push(Effect::Remember(Box::new(term.clone())));
                // The snapshot frames follow; until then the view starts fresh at their size.
                let (cols, rows) = match (size["cols"].as_u64(), size["rows"].as_u64()) {
                    (Some(c), Some(r)) => (
                        u16::try_from(c).unwrap_or(self.grid.0),
                        u16::try_from(r).unwrap_or(self.grid.1),
                    ),
                    _ => self.grid,
                };
                out.push(Effect::ResetView {
                    cols,
                    rows,
                    history: term.history_limit,
                });
                let phase = Phase::Attached {
                    term,
                    stream,
                    pane_pid,
                    pane_id: r["pane_id"].as_str().unwrap_or("").to_string(),
                };
                self.set(phase, &mut out);
                // The window may have changed size while the attach was on its way.
                if (cols, rows) != self.grid {
                    let g = self.grid;
                    self.grid = (cols, rows);
                    out.extend(self.on_grid(g.0, g.1));
                }
            }
            Pending::Duplicate { id, restart, .. } => {
                if let Some(t) = TermInfo::from_full(&r["terminal"]) {
                    // A restart takes the old tab's place; a copy goes right after it.
                    let at = self.tabs.iter().position(|x| x.id == id);
                    self.tabs
                        .retain(|x| x.id != t.id && !(restart && x.id == id));
                    let at = at.map_or(self.tabs.len(), |i| {
                        (if restart { i } else { i + 1 }).min(self.tabs.len())
                    });
                    self.tabs.insert(at, t.clone());
                    if restart {
                        for p in &mut self.pinned {
                            if *p == id {
                                p.clone_from(&t.id);
                            }
                        }
                    }
                    self.show(t, &mut out);
                }
            }
            Pending::Create => {
                if let Some(t) = TermInfo::from_full(&r["terminal"]) {
                    match self.tabs.iter_mut().find(|x| x.id == t.id) {
                        Some(x) => *x = t.clone(),
                        None => self.tabs.push(t.clone()),
                    }
                    self.show(t, &mut out);
                }
            }
            Pending::Kill { id, .. } | Pending::Remove { id, .. } => {
                self.close_tab(&id, &mut out);
            }
            Pending::Rename => out.push(Effect::Renamed(Ok(()))),
            Pending::View(id) => {
                let stream = r["stream_id"].as_u64().and_then(|s| u32::try_from(s).ok());
                let size = &r["snapshot"]["size"];
                let snap = match (size["cols"].as_u64(), size["rows"].as_u64()) {
                    (Some(c), Some(r)) => u16::try_from(c).ok().zip(u16::try_from(r).ok()),
                    _ => None,
                };
                match self.views.iter_mut().find(|v| v.pane_id == id) {
                    Some(v) => {
                        v.stream = stream;
                        v.snapshot_size = snap;
                        out.push(Effect::Views);
                    }
                    // Not wanted any more: let it go.
                    None => {
                        if let Some(s) = stream {
                            out.push(self.request(
                                "pane.detach",
                                json!({"stream_id": s}),
                                Pending::Detach,
                            ));
                        }
                    }
                }
            }
            Pending::Query(method) => out.push(Effect::Show {
                method,
                result: r.clone(),
            }),
            Pending::PaneOp(..) | Pending::Subscribe | Pending::Resize | Pending::Detach => {}
        }
        out
    }

    fn on_error(&mut self, p: Pending, e: &Value) -> Vec<Effect> {
        let mut out = Vec::new();
        let code = e["code"].as_str().unwrap_or("internal_error");
        let message = e["message"].as_str().unwrap_or("request failed");
        match p {
            Pending::Attach(term) => {
                // The Terminal is gone or not running any more.
                let what = term.as_ref().map_or_else(
                    || match &self.want {
                        Want::Target(t) => format!("`{t}`"),
                        _ => "the Terminal".into(),
                    },
                    |t| format!("`{}`", t.name),
                );
                let notice = match code {
                    "target_not_found" => format!("No Terminal {what} exists"),
                    "invalid_state" => format!("{what} is not running"),
                    _ => format!("Cannot open {what}: {message} ({code})"),
                };
                self.want = Want::Any;
                out.push(self.request("terminal.list", json!({"detail": "full"}), Pending::List));
                self.set(
                    Phase::Empty {
                        notice: Some(notice),
                    },
                    &mut out,
                );
            }
            Pending::List => self.set(
                Phase::Failed {
                    code: code.to_string(),
                    message: message.to_string(),
                    hint: None,
                },
                &mut out,
            ),
            Pending::Create => out.push(Effect::Notice(format!(
                "No Terminal was created: {message} ({code})"
            ))),
            // #98 T1: a busy Terminal asks before it restarts.
            Pending::Duplicate {
                id,
                name,
                restart: true,
            } if code == "busy" => out.push(Effect::Confirm(Confirm {
                id,
                name,
                program: e["details"]["program"].as_str().unwrap_or("").to_string(),
                processes: e["details"]["processes"].as_u64().unwrap_or(0),
                pane: None,
                pinned: false,
                restart: true,
            })),
            Pending::Duplicate { name, restart, .. } => out.push(Effect::Notice(format!(
                "`{name}` was not {}: {message} ({code})",
                if restart { "restarted" } else { "duplicated" }
            ))),
            Pending::Kill { id, name } => {
                self.closing.retain(|c| *c != id);
                match code {
                    // The Host's guard (ADR 0020 §3): ask the user before `force`.
                    "busy" => out.push(Effect::Confirm(Confirm {
                        id,
                        name,
                        program: e["details"]["program"].as_str().unwrap_or("").to_string(),
                        processes: e["details"]["processes"].as_u64().unwrap_or(0),
                        pane: None,
                        pinned: false,
                        restart: false,
                    })),
                    // Already gone: its tab goes.
                    "target_not_found" => self.close_tab(&id, &mut out),
                    _ => out.push(Effect::Notice(format!(
                        "`{name}` was not closed: {message} ({code})"
                    ))),
                }
            }
            Pending::Remove { id, name } => {
                self.closing.retain(|c| *c != id);
                match code {
                    "target_not_found" => self.close_tab(&id, &mut out),
                    _ => out.push(Effect::Notice(format!(
                        "`{name}` was not removed: {message} ({code})"
                    ))),
                }
            }
            Pending::Rename => out.push(Effect::Renamed(Err(format!("{message} ({code})")))),
            Pending::View(id) => {
                if let Some(v) = self.views.iter_mut().find(|v| v.pane_id == id) {
                    v.failed = true;
                }
            }
            // A busy pane: the window asks before `force`, as for a busy Terminal (ADR 0042).
            Pending::PaneOp("pane.kill" | "window.kill", target) if code == "busy" => {
                let pane = &e["details"]["panes"][0];
                let tname = self.terminal().map_or("", |t| t.name.as_str());
                let name = if target.starts_with("w-") {
                    format!("this window of `{tname}`")
                } else {
                    format!(
                        "pane {} of `{tname}`",
                        pane["index"]
                            .as_u64()
                            .map_or_else(|| "?".into(), |i| i.to_string()),
                    )
                };
                out.push(Effect::Confirm(Confirm {
                    id: self.terminal().map_or(String::new(), |t| t.id.clone()),
                    name,
                    program: pane["program"].as_str().unwrap_or("").to_string(),
                    processes: e["details"]["processes"].as_u64().unwrap_or(0),
                    pane: Some(target),
                    pinned: false,
                    restart: false,
                }));
            }
            Pending::Query(method) => {
                out.push(Effect::Notice(format!("{method}: {message} ({code})")));
            }
            Pending::PaneOp(method, _) => {
                let what = match method {
                    "pane.split" => "The pane was not split",
                    "pane.kill" => "The pane was not closed",
                    "pane.adjust" => "The pane was not resized",
                    "pane.zoom" => "The pane was not zoomed",
                    "window.create" => "No window was created",
                    "window.select" => "The window was not selected",
                    "window.rename" => "The window was not renamed",
                    "window.kill" => "The window was not closed",
                    "buffer.set" => "The buffer was not saved",
                    "buffer.paste" => "Nothing was pasted",
                    "pane.break" => "The pane was not moved to a window of its own",
                    "pane.join" => "The pane was not joined",
                    "pane.send_keys" => "No keys were sent",
                    _ => "The pane was not changed",
                };
                let busy = if code == "busy" {
                    " (it runs a program; end it there, or use `subak pane kill`)"
                } else {
                    ""
                };
                out.push(Effect::Notice(format!("{what}: {message} ({code}){busy}")));
            }
            Pending::Subscribe | Pending::Resize | Pending::Detach => {}
        }
        out
    }

    /// Step 21: shows the shown Terminal's active pane. When the Host's selection moved to another
    /// pane (another window or pane was selected, or the shown pane's program ended while other
    /// panes run), the old pane is detached and the active one attached; nothing ends.
    fn follow(&mut self, out: &mut Vec<Effect>) {
        let Some(shown) = self.terminal().map(|t| t.id.clone()) else {
            return;
        };
        let Some(t) = self.tabs.iter().find(|x| x.id == shown).cloned() else {
            return;
        };
        // The shown Terminal's place stays current (the title names the window and pane).
        if let Phase::Attached { term, .. } | Phase::Ended { term, .. } | Phase::Attaching(term) =
            &mut self.phase
            && (term.place != t.place
                || term.status != t.status
                || term.claude != t.claude
                || term.active_pane != t.active_pane
                || term.layout != t.layout
                || term.windows != t.windows)
        {
            term.place.clone_from(&t.place);
            term.status.clone_from(&t.status);
            term.claude = t.claude;
            term.active_pane.clone_from(&t.active_pane);
            term.layout.clone_from(&t.layout);
            term.windows.clone_from(&t.windows);
            term.active_window.clone_from(&t.active_window);
            out.push(Effect::Phase);
        }
        if !t.running() {
            return;
        }
        let Some(want) = t.active_pane.clone() else {
            return;
        };
        match self.phase.clone() {
            Phase::Attached {
                pane_id, stream, ..
            } if pane_id != want => {
                out.push(self.request(
                    "pane.detach",
                    json!({"stream_id": stream}),
                    Pending::Detach,
                ));
                out.push(self.attach(Some(t.clone()), &t.id));
                self.set(Phase::Attaching(t), out);
            }
            Phase::Ended {
                term,
                why: EndReason::Exited(_),
            } if term.id == t.id => {
                out.push(self.attach(Some(t.clone()), &t.id));
                self.set(Phase::Attaching(t), out);
            }
            _ => {}
        }
    }

    /// Takes a Terminal list as the truth for the tabs: known tabs keep their place, new ones
    /// are appended in the Host's order, missing ones go (but the shown one stays with its last
    /// screen, marked removed). Hidden ones stay hidden.
    fn merge(&mut self, list: &[TermInfo]) {
        let shown = self.terminal().map(|t| t.id.clone());
        self.hidden.retain(|h| list.iter().any(|t| t.id == h.id));
        for h in &mut self.hidden {
            if let Some(t) = list.iter().find(|t| t.id == h.id) {
                *h = t.clone();
            }
        }
        let mut tabs: Vec<TermInfo> = Vec::new();
        for old in &self.tabs {
            match list.iter().find(|t| t.id == old.id) {
                Some(t) => tabs.push(t.clone()),
                None if Some(&old.id) == shown.as_ref() && self.phase.shows_pane() => {
                    let mut gone = old.clone();
                    gone.state = "removed".into();
                    tabs.push(gone);
                }
                None => {}
            }
        }
        for t in list {
            if tabs.iter().any(|x| x.id == t.id) || self.hidden.iter().any(|h| h.id == t.id) {
                continue;
            }
            if self.pre_hidden.contains(&t.id) {
                self.hidden.push(t.clone());
            } else {
                tabs.push(t.clone());
            }
        }
        self.pre_hidden.clear();
        self.tabs = tabs;
    }

    /// Picks the tab to show after a list, when none is shown.
    fn decide(&mut self, out: &mut Vec<Effect>) {
        let notice = match &self.phase {
            Phase::Empty { notice } => notice.clone(),
            _ => None,
        };
        // A live, opening or stored tab stays as it is.
        if matches!(
            self.phase,
            Phase::Attached { .. } | Phase::Attaching(_) | Phase::Stored(_)
        ) {
            return;
        }
        if let Phase::Ended { term, .. } = &self.phase
            && self.tabs.iter().any(|t| t.id == term.id)
        {
            return;
        }
        match self.want.clone() {
            Want::Target(t) => {
                self.want = Want::Any;
                let known = self
                    .tabs
                    .iter()
                    .chain(self.hidden.iter())
                    .find(|x| x.id == t || x.name.to_lowercase() == t.to_lowercase())
                    .cloned();
                if let Some(k) = &known {
                    self.hidden.retain(|h| h.id != k.id);
                    if !self.tabs.iter().any(|x| x.id == k.id) {
                        self.tabs.push(k.clone());
                    }
                }
                let shown = known.unwrap_or_else(|| TermInfo {
                    id: t.clone(),
                    name: t.clone(),
                    state: "unknown".into(),
                    pid: None,
                    program: String::new(),
                    history_limit: 2000,
                    place: None,
                    status: None,
                    claude: crate::gui::float::art::Claude::None,
                    active_pane: None,
                    layout: None,
                    windows: Vec::new(),
                    active_window: None,
                });
                if shown.stored() {
                    self.set(Phase::Stored(shown), out);
                    return;
                }
                // The Host resolves the name (NFC, case) and says if it does not run.
                out.push(self.attach(Some(shown.clone()), &t));
                self.set(Phase::Attaching(shown), out);
                return;
            }
            Want::Last { id, name } => {
                let hit = self
                    .tabs
                    .iter()
                    .filter(|x| x.running())
                    .find(|x| x.id == id)
                    .or_else(|| {
                        self.tabs
                            .iter()
                            .filter(|x| x.running())
                            .find(|x| x.name == name)
                    })
                    .cloned();
                self.want = Want::Any;
                if let Some(t) = hit {
                    self.show(t, out);
                    return;
                }
                if matches!(self.phase, Phase::Disconnected { .. } | Phase::Connecting)
                    && self.terminal().is_some()
                {
                    // It ended while the Host was away (or with it).
                    self.pick(Some(format!("`{name}` is no longer running")), out);
                    return;
                }
            }
            Want::Any => {}
        }
        self.pick(notice, out);
    }

    /// Shows the first running tab, else the first tab, else the empty screen (`notice` goes
    /// with it).
    fn pick(&mut self, notice: Option<String>, out: &mut Vec<Effect>) {
        // The last screen of a Terminal that is gone is not kept across a reconnect.
        self.tabs.retain(|t| !t.gone());
        let first = self
            .tabs
            .iter()
            .find(|t| t.running())
            .or_else(|| self.tabs.first())
            .cloned();
        match first {
            Some(t) => {
                self.phase = Phase::Empty { notice: None };
                self.show(t, out);
                if let Some(n) = notice {
                    out.push(Effect::Notice(n));
                }
            }
            None => self.set(Phase::Empty { notice }, out),
        }
    }

    /// Applies a field change to the Terminal `id` wherever this model keeps it.
    fn update(&mut self, id: &str, f: impl Fn(&mut TermInfo)) {
        for t in self.tabs.iter_mut().chain(self.hidden.iter_mut()) {
            if t.id == id {
                f(t);
            }
        }
        match &mut self.phase {
            Phase::Attached { term, .. }
            | Phase::Ended { term, .. }
            | Phase::Attaching(term)
            | Phase::Stored(term) => {
                if term.id == id {
                    f(term);
                }
            }
            Phase::Disconnected {
                last: Some(term), ..
            } if term.id == id => f(term),
            _ => {}
        }
    }

    fn on_event(&mut self, name: &str, d: &Value) -> Vec<Effect> {
        let mut out = Vec::new();
        let ours_pane = |m: &Model| match &m.phase {
            Phase::Attached { pane_id, .. } => d["pane_id"].as_str() == Some(pane_id.as_str()),
            _ => false,
        };
        let tid = d["terminal_id"]
            .as_str()
            .or_else(|| d["terminal"]["id"].as_str())
            .unwrap_or("")
            .to_string();
        let shown = self.terminal().is_some_and(|t| t.id == tid);
        match name {
            "pane.exited" if ours_pane(self) && self.closing.contains(&tid) => {
                // This window is closing it (`Ctrl+W`): no "ended" banner; its removal follows.
                self.update(&tid, |t| t.state = "exited".into());
            }
            "pane.exited" if ours_pane(self) => {
                self.update(&tid, |t| t.state = "exited".into());
                let term = self.terminal().cloned().expect("attached");
                self.set(
                    Phase::Ended {
                        term,
                        why: EndReason::Exited(d["exit_code"].as_i64()),
                    },
                    &mut out,
                );
            }
            "pane.detached" if ours_pane(self) => {
                let term = self.terminal().cloned().expect("attached");
                let by = d["by"]["kind"].as_str().map(|k| {
                    format!(
                        "{k} client{}",
                        d["by"]["pid"]
                            .as_u64()
                            .map_or(String::new(), |p| format!(" (pid {p})"))
                    )
                });
                self.set(
                    Phase::Ended {
                        term,
                        why: EndReason::DetachedBy(by),
                    },
                    &mut out,
                );
            }
            "pane.resync" if ours_pane(self) => {
                // A snapshot at the pane's current size follows (the window's grid).
                self.in_snapshot = false;
                self.snapshot_size = Some(self.grid);
            }
            "terminal.created" => {
                if let Some(t) = TermInfo::from_full(&d["terminal"])
                    && !self.tabs.iter().any(|x| x.id == t.id)
                    && !self.hidden.iter().any(|x| x.id == t.id)
                {
                    // Another client's (or our own) new Terminal gets a tab; the shown tab
                    // stays (keys never go to a Terminal the user did not choose).
                    self.tabs.push(t);
                    out.push(Effect::Tabs);
                }
                if matches!(self.phase, Phase::Empty { .. }) {
                    out.push(self.request(
                        "terminal.list",
                        json!({"detail": "full"}),
                        Pending::List,
                    ));
                }
            }
            "terminal.renamed" => {
                if let Some(n) = text(&d["name"]) {
                    self.update(&tid, |t| t.name.clone_from(&n));
                    if shown && let Phase::Attached { term, .. } = &self.phase {
                        out.push(Effect::Remember(Box::new(term.clone())));
                    }
                    out.push(Effect::Phase);
                    out.push(Effect::Tabs);
                }
            }
            // Step 28: a Terminal's Claude state changed: its tab's mark (and the title).
            "terminal.claude_changed" => {
                let c = d["claude_state"]
                    .as_str()
                    .and_then(crate::gui::float::art::Claude::parse)
                    .unwrap_or(crate::gui::float::art::Claude::None);
                self.update(&tid, |t| t.claude = c);
                if let Phase::Attached { term, .. }
                | Phase::Ended { term, .. }
                | Phase::Attaching(term) = &mut self.phase
                    && term.id == tid
                {
                    term.claude = c;
                }
                out.push(Effect::Tabs);
                out.push(Effect::Phase);
            }
            "terminal.state_changed" => {
                let state = d["state"].as_str().unwrap_or("unknown").to_string();
                let keep_exit = |t: &TermInfo| t.state == "exited" && state == "stopping";
                self.update(&tid, |t| {
                    if !keep_exit(t) {
                        t.state.clone_from(&state);
                    }
                });
                if shown {
                    match (&self.phase, state.as_str()) {
                        (Phase::Attached { term, .. }, "stopped") => {
                            let term = term.clone();
                            self.set(
                                Phase::Ended {
                                    term,
                                    why: EndReason::Stopped,
                                },
                                &mut out,
                            );
                        }
                        (Phase::Attaching(t), "running")
                            if self.awaiting.as_deref() == Some(tid.as_str()) =>
                        {
                            let t = t.clone();
                            self.awaiting = None;
                            out.push(self.attach(Some(t.clone()), &t.id));
                        }
                        (Phase::Attaching(t), "stopped" | "failed")
                            if self.awaiting.as_deref() == Some(tid.as_str()) =>
                        {
                            let t = t.clone();
                            self.awaiting = None;
                            self.set(Phase::Stored(t), &mut out);
                        }
                        _ => out.push(Effect::Phase),
                    }
                }
                out.push(Effect::Tabs);
                if matches!(self.phase, Phase::Empty { .. }) {
                    out.push(self.request(
                        "terminal.list",
                        json!({"detail": "full"}),
                        Pending::List,
                    ));
                }
            }
            "terminal.removed" => {
                self.hidden.retain(|h| h.id != tid);
                if self.closing.contains(&tid) {
                    // Closed by this window (`Ctrl+W`): the tab goes, no banner.
                    self.close_tab(&tid, &mut out);
                } else if shown {
                    match self.phase.clone() {
                        Phase::Attached { term, .. } => {
                            self.update(&tid, |t| t.state = "removed".into());
                            let mut term = term;
                            term.state = "removed".into();
                            self.set(
                                Phase::Ended {
                                    term,
                                    why: EndReason::Removed,
                                },
                                &mut out,
                            );
                        }
                        Phase::Ended { .. } => {
                            self.update(&tid, |t| t.state = "removed".into());
                            out.push(Effect::Phase);
                        }
                        _ => {
                            let name = self.terminal().map(|t| t.name.clone()).unwrap_or_default();
                            self.close_tab(&tid, &mut out);
                            out.push(Effect::Notice(format!(
                                "`{name}` was removed by another client"
                            )));
                        }
                    }
                } else {
                    self.tabs.retain(|t| t.id != tid);
                }
                out.push(Effect::Tabs);
            }
            // Step 21: a window or pane appeared, went, was renamed or selected: the list says
            // what the tabs and the title show, and which pane to show (see `follow`).
            "window.created" | "window.renamed" | "window.removed" | "window.selected"
            | "pane.created" | "pane.removed" | "pane.selected" | "window.layout"
            | "window.alert" | "window.options" => {
                if self
                    .tabs
                    .iter()
                    .chain(self.hidden.iter())
                    .any(|t| t.id == tid)
                {
                    out.push(self.request(
                        "terminal.list",
                        json!({"detail": "full"}),
                        Pending::List,
                    ));
                }
            }
            // #98 T3: output or a bell in a Terminal that is not shown marks its tab.
            "terminal.activity" if !shown && !tid.is_empty() => {
                let kind = if d["kind"] == "bell" {
                    Alert::Bell
                } else {
                    Alert::Output
                };
                let e = self.alerts.entry(tid).or_insert(kind);
                if kind > *e {
                    *e = kind;
                }
                out.push(Effect::Tabs);
            }
            // #98 T3 (`CORE-78`): a program's progress (OSC 9;4), for the tab and the taskbar.
            "terminal.progress" if !tid.is_empty() => {
                let state = d["state"]
                    .as_str()
                    .and_then(subak_term::screen::ProgressState::parse);
                let value = d["value"].as_u64().map_or(0, |v| v.min(100) as u8);
                let next = state.map(|state| subak_term::screen::Progress { state, value });
                if self.progress.get(&tid).copied() != next {
                    match next {
                        Some(p) => self.progress.insert(tid, p),
                        None => self.progress.remove(&tid),
                    };
                    out.push(Effect::Tabs);
                }
            }
            // #98 T3 (`CORE-79`): a shell-integration command finished.
            "terminal.command_done" if !tid.is_empty() => {
                out.push(Effect::Finished(Finished {
                    name: self.name_of(&tid),
                    terminal_id: tid,
                    command: true,
                    exit_code: d["exit_code"].as_i64(),
                    duration_ms: d["duration_ms"].as_u64(),
                }));
            }
            // #98 T3 (`CORE-79`): another Terminal's program ended (the shown one ends above).
            "pane.exited" if !tid.is_empty() => {
                self.progress.remove(&tid);
                out.push(Effect::Finished(Finished {
                    name: self.name_of(&tid),
                    terminal_id: tid,
                    command: false,
                    exit_code: d["exit_code"].as_i64(),
                    duration_ms: None,
                }));
            }
            "host.stopping" => self.host_stopping = text(&d["reason"]),
            _ => {}
        }
        out
    }
}

/// Step 25: tmux's status-line window list of a Terminal object (`0:cmd* 1:logs-#`): `*` the
/// active window, `-` the last one, `#` activity, `!` a bell, `Z` a zoomed pane. `None` for a
/// single window without alerts, so nothing changes for the usual Terminal.
pub fn window_list(t: &Value) -> Option<String> {
    let windows = t["windows"].as_array()?;
    let alert = windows
        .iter()
        .any(|w| w["activity"] == true || w["bell"] == true);
    if windows.len() < 2 && !alert {
        return None;
    }
    let last = t["last_window"].as_str();
    Some(
        windows
            .iter()
            .map(|w| {
                let mut f = String::new();
                if w["id"] == t["active_window"] {
                    f.push('*');
                } else if last.is_some() && w["id"].as_str() == last {
                    f.push('-');
                }
                if w["activity"] == true {
                    f.push('#');
                }
                if w["bell"] == true {
                    f.push('!');
                }
                if !w["zoomed"].is_null() && w.get("zoomed").is_some() {
                    f.push('Z');
                }
                format!("{}:{}{f}", w["index"], w["name"].as_str().unwrap_or("?"))
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(id: &str, name: &str, state: &str, pid: u32) -> Value {
        json!({
            "id": id, "name": name, "state": state,
            "windows": [{"panes": [{
                "pid": pid, "executable": "C:\\Windows\\System32\\cmd.exe",
                "command": {"program": "cmd.exe", "args": []},
                "history": {"limit_lines": 5000}
            }]}]
        })
    }

    fn sent(effects: &[Effect]) -> Vec<(u64, String, Value)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Send(m) => Some((
                    m["id"].as_u64().unwrap(),
                    m["method"].as_str().unwrap().to_string(),
                    m["params"].clone(),
                )),
                _ => None,
            })
            .collect()
    }

    fn methods(effects: &[Effect]) -> Vec<String> {
        sent(effects).into_iter().map(|s| s.1).collect()
    }

    fn respond(m: &mut Model, id: u64, result: Value) -> Vec<Effect> {
        let mut msg = json!({"type": "response", "id": id, "ok": true});
        msg["result"] = result;
        m.on_control(&msg)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn fail(m: &mut Model, id: u64, code: &str, details: Value) -> Vec<Effect> {
        m.on_control(&json!({"type": "response", "id": id, "ok": false,
            "error": {"code": code, "message": format!("{code} happened"), "details": details}}))
    }

    /// Step 28: a tab's Claude state comes with the list and follows `terminal.claude_changed`.
    #[test]
    fn claude_states_reach_the_tabs_from_the_list_and_events() {
        let mut m = Model::new(Want::Any, (80, 24));
        let mut a = term("t-a", "agent", "running", 10);
        a["claude_state"] = json!("in-progress");
        connect_with(&mut m, json!([a, term("t-b", "b", "running", 11)]));
        use crate::gui::float::art::Claude;
        assert_eq!(m.tabs()[0].claude, Claude::InProgress);
        assert_eq!(m.tabs()[1].claude, Claude::None);
        let fx = event(
            &mut m,
            "terminal.claude_changed",
            json!({"terminal_id": "t-b", "claude_state": "need-input", "previous": "none"}),
        );
        assert!(fx.iter().any(|e| matches!(e, Effect::Tabs)), "{fx:?}");
        assert_eq!(m.tabs()[1].claude, Claude::NeedInput);
        event(
            &mut m,
            "terminal.claude_changed",
            json!({"terminal_id": "t-a", "claude_state": "unknown", "previous": "in-progress"}),
        );
        assert_eq!(
            m.tabs()[0].claude,
            Claude::Unknown,
            "a stale signal is shown as unknown"
        );
    }

    #[allow(clippy::needless_pass_by_value)]
    fn event(m: &mut Model, name: &str, data: Value) -> Vec<Effect> {
        m.on_control(&json!({"type": "event", "seq": 1, "event": name, "data": data}))
    }

    fn id_of(effects: &[Effect], method: &str) -> u64 {
        sent(effects)
            .into_iter()
            .find(|(_, m, _)| m == method)
            .unwrap_or_else(|| panic!("no {method} in {effects:?}"))
            .0
    }

    fn attach_result(stream: u32, tid: &str, pid: u32) -> Value {
        json!({
            "stream_id": stream, "terminal_id": tid, "pane_id": format!("p-{tid}"),
            "pane": {"id": format!("p-{tid}"), "pid": pid, "size": {"cols": 100, "rows": 30}},
            "snapshot": {"size": {"cols": 100, "rows": 30}, "bytes": 10, "frames": 1}
        })
    }

    /// Connects and answers the list; returns the effects of the list response.
    fn connect_with(m: &mut Model, terminals: Value) -> Vec<Effect> {
        let fx = m.on_connected();
        assert_eq!(methods(&fx), ["events.subscribe", "terminal.list"]);
        let mut result = json!({});
        result["terminals"] = terminals;
        respond(m, id_of(&fx, "terminal.list"), result)
    }

    fn names(m: &Model) -> Vec<String> {
        m.tabs().iter().map(|t| t.name.clone()).collect()
    }

    /// Answers the pending attach in `fx` (stream = the Terminal's pid).
    fn attach_from(m: &mut Model, fx: &[Effect]) -> Vec<Effect> {
        let (id, _, p) = sent(fx)
            .into_iter()
            .find(|(_, method, _)| method == "pane.attach")
            .expect("an attach");
        let tid = p["target"].as_str().unwrap().to_string();
        let pid = m
            .tabs()
            .iter()
            .find(|t| t.id == tid)
            .and_then(|t| t.pid)
            .unwrap_or(1);
        respond(m, id, attach_result(pid, &tid, pid))
    }

    /// A Terminal with two internal windows (Step 21): `p-a` in window 0, `p-b` and `p-c` in
    /// window 1; `active` is the active window's index.
    fn multi(active: usize, b_or_c: &str) -> Value {
        let pane = |id: &str, pid: u32| {
            json!({"id": id, "index": 0, "pid": pid, "command": {"program": "cmd.exe", "args": []},
                   "history": {"limit_lines": 2000}})
        };
        json!({
            "id": "t-1", "name": "work", "state": "running",
            "active_window": if active == 0 { "w-0" } else { "w-1" },
            "windows": [
                {"id": "w-0", "index": 0, "name": "cmd", "active_pane": "p-a", "panes": [pane("p-a", 10)]},
                {"id": "w-1", "index": 1, "name": "build", "active_pane": b_or_c,
                 "panes": [pane("p-b", 11), pane("p-c", 12)]},
            ]
        })
    }

    /// Step 21: the window shows the Terminal's active pane and follows the Host's selection
    /// (detach the old pane, attach the new one; nothing ends), also when the shown pane's
    /// program ends while others run; the title names the window and pane.
    #[test]
    fn the_window_follows_the_hosts_window_and_pane_selection() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(&mut m, json!([multi(1, "p-b")]));
        let t = m.tabs()[0].clone();
        assert_eq!(t.active_pane.as_deref(), Some("p-b"));
        assert_eq!(
            t.place.as_deref(),
            Some("window 1:build pane 0 (2 windows, 3 panes)")
        );
        let (id, _, p) = sent(&fx)
            .into_iter()
            .find(|(_, method, _)| method == "pane.attach")
            .unwrap();
        assert_eq!(
            p["target"], "t-1",
            "a Terminal target: the Host gives its active pane"
        );
        let mut r = attach_result(11, "t-1", 11);
        r["pane_id"] = json!("p-b");
        respond(&mut m, id, r);
        assert_eq!(m.phase.name(), "attached");
        // Another client selects window 0: a list, then a detach of p-b and an attach.
        let fx = event(
            &mut m,
            "window.selected",
            json!({"terminal_id": "t-1", "window_id": "w-0", "pane_id": "p-a"}),
        );
        assert_eq!(methods(&fx), ["terminal.list"]);
        let fx = respond(
            &mut m,
            id_of(&fx, "terminal.list"),
            json!({"terminals": [multi(0, "p-b")]}),
        );
        assert_eq!(methods(&fx), ["pane.detach", "pane.attach"]);
        for (_, method, _) in sent(&fx) {
            assert!(!method.starts_with("terminal.") && !method.ends_with(".kill"));
        }
        let mut r = attach_result(10, "t-1", 10);
        r["pane_id"] = json!("p-a");
        respond(&mut m, id_of(&fx, "pane.attach"), r);
        assert!(matches!(&m.phase, Phase::Attached { pane_id, .. } if pane_id == "p-a"));
        // An event of another Terminal changes nothing here.
        let fx = event(
            &mut m,
            "pane.selected",
            json!({"terminal_id": "t-9", "window_id": "w-9", "pane_id": "p-9"}),
        );
        assert!(fx.is_empty(), "{fx:?}");
        // The same selection again: a list, no reattach.
        let fx = event(
            &mut m,
            "pane.created",
            json!({"terminal_id": "t-1", "window_id": "w-1", "pane": {}}),
        );
        let fx = respond(
            &mut m,
            id_of(&fx, "terminal.list"),
            json!({"terminals": [multi(0, "p-c")]}),
        );
        assert!(!methods(&fx).contains(&"pane.attach".to_string()));
        // The shown pane's program ends while others run: the next active pane is shown.
        event(
            &mut m,
            "pane.exited",
            json!({"terminal_id": "t-1", "pane_id": "p-a", "exit_code": 0}),
        );
        assert_eq!(m.phase.name(), "ended");
        let fx = event(
            &mut m,
            "pane.removed",
            json!({"terminal_id": "t-1", "window_id": "w-0", "pane_id": "p-a", "reason": "exited"}),
        );
        let mut after = multi(1, "p-c");
        after["windows"].as_array_mut().unwrap().remove(0);
        let fx = respond(
            &mut m,
            id_of(&fx, "terminal.list"),
            json!({"terminals": [after]}),
        );
        assert_eq!(methods(&fx), ["pane.attach"]);
        assert_eq!(m.phase.name(), "attaching");
        assert_eq!(
            m.terminal().unwrap().place.as_deref(),
            Some("window 1:build pane 0 (1 window, 2 panes)")
        );
    }

    #[test]
    fn no_sessions_one_session_and_many_sessions_become_tabs() {
        // None: the empty screen; a Terminal created by any client opens by itself.
        let mut m = Model::new(Want::Any, (80, 24));
        connect_with(&mut m, json!([]));
        assert_eq!(m.phase, Phase::Empty { notice: None });
        assert!(m.tabs().is_empty());
        let fx = event(
            &mut m,
            "terminal.created",
            json!({"terminal": term("t-n", "new", "starting", 0)}),
        );
        assert_eq!(names(&m), ["new"]);
        let fx = respond(
            &mut m,
            id_of(&fx, "terminal.list"),
            json!({"terminals": [term("t-n", "new", "running", 9)]}),
        );
        assert_eq!(sent(&fx)[0].1, "pane.attach");
        // One: it opens interactively at the window's size.
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 11)]));
        let s = sent(&fx);
        assert_eq!(s.len(), 1);
        assert_eq!(
            s[0].2,
            json!({"target": "t-a", "interactive": true, "size": {"cols": 100, "rows": 30}})
        );
        let fx = respond(&mut m, s[0].0, attach_result(7, "t-a", 11));
        assert!(fx.contains(&Effect::ResetView {
            cols: 100,
            rows: 30,
            history: 5000
        }));
        assert!(matches!(&fx[0], Effect::Remember(t) if t.name == "build"));
        assert_eq!(m.input_stream(), Some(7));
        assert_eq!(m.active(), Some(0));
        // Many: every Terminal is a tab in the Host's order (stored ones too); the first running
        // one opens; nothing is created, stopped, killed or removed by opening.
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-s", "old", "stopped", 0),
                term("t-a", "one", "running", 1),
                term("t-b", "two", "running", 2)
            ]),
        );
        assert_eq!(names(&m), ["old", "one", "two"]);
        assert_eq!(sent(&fx)[0].2["target"], "t-a");
        for method in methods(&fx) {
            assert!(!method.starts_with("terminal."), "{method}");
        }
    }

    #[test]
    fn ctrl_tab_cycles_tabs_detaching_one_pane_and_attaching_the_next() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "one", "running", 1),
                term("t-s", "kept", "stopped", 0),
                term("t-b", "two", "running", 2)
            ]),
        );
        attach_from(&mut m, &fx);
        assert_eq!(m.active(), Some(0));
        // Next: the stored tab shows its state screen; only a detach is sent.
        let fx = m.cycle(1);
        assert_eq!(methods(&fx), ["pane.detach"]);
        assert_eq!(sent(&fx)[0].2, json!({"stream_id": 1}));
        assert_eq!(m.phase.name(), "stored");
        assert_eq!(m.input_stream(), None, "no input to a stored Terminal");
        // Next again: attach `two`.
        let fx = m.cycle(1);
        assert_eq!(methods(&fx), ["pane.attach"]);
        attach_from(&mut m, &fx);
        assert_eq!(m.input_stream(), Some(2));
        // Next wraps around to `one`; previous goes back.
        let fx = m.cycle(1);
        assert_eq!(methods(&fx), ["pane.detach", "pane.attach"]);
        attach_from(&mut m, &fx);
        assert_eq!(m.active(), Some(0));
        m.cycle(-1);
        assert_eq!(m.terminal().unwrap().id, "t-b");
        // A late attach answer for a tab the user already left is detached at once.
        let mut m2 = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m2,
            json!([
                term("t-a", "one", "running", 1),
                term("t-b", "two", "running", 2)
            ]),
        );
        let first = id_of(&fx, "pane.attach");
        let fx2 = m2.cycle(1);
        let fx = respond(&mut m2, first, attach_result(1, "t-a", 1));
        assert_eq!(methods(&fx), ["pane.detach"]);
        assert_eq!(m2.terminal().unwrap().id, "t-b");
        attach_from(&mut m2, &fx2);
        assert_eq!(m2.input_stream(), Some(2));
        // One tab: cycling stays; no tabs: nothing.
        let mut one = Model::new(Want::Any, (80, 24));
        let fx = connect_with(&mut one, json!([term("t-a", "one", "running", 1)]));
        attach_from(&mut one, &fx);
        assert!(one.cycle(1).is_empty());
        let mut none = Model::new(Want::Any, (80, 24));
        connect_with(&mut none, json!([]));
        assert!(none.cycle(1).is_empty());
    }

    #[test]
    fn ctrl_t_creates_at_the_window_size_and_shows_the_new_terminal() {
        let mut m = Model::new(Want::Any, (90, 25));
        let fx = connect_with(&mut m, json!([term("t-a", "one", "running", 1)]));
        attach_from(&mut m, &fx);
        let fx = m.create();
        let (cid, method, params) = sent(&fx)[0].clone();
        assert_eq!(method, "terminal.create");
        assert_eq!(params, json!({"size": {"cols": 90, "rows": 25}}));
        // The created event arrives first: a tab, but the shown tab stays.
        event(
            &mut m,
            "terminal.created",
            json!({"terminal": term("t-n", "2", "starting", 0)}),
        );
        assert_eq!(names(&m), ["one", "2"]);
        assert_eq!(m.terminal().unwrap().id, "t-a");
        event(
            &mut m,
            "terminal.state_changed",
            json!({"terminal_id": "t-n", "state": "running", "previous_state": "starting"}),
        );
        // Then the answer: show it (detach `one`, attach `2`).
        let fx = respond(
            &mut m,
            cid,
            json!({"terminal_id": "t-n", "window_id": "w-1", "pane_id": "p-1", "name": "2",
                "terminal": term("t-n", "2", "running", 5)}),
        );
        assert_eq!(methods(&fx), ["pane.detach", "pane.attach"]);
        attach_from(&mut m, &fx);
        assert_eq!(m.active(), Some(1));
        // Repeated create and close leave exactly the tabs that exist.
        for k in 0..5u32 {
            let fx = m.create();
            let id = format!("t-r{k}");
            let fx = respond(
                &mut m,
                id_of(&fx, "terminal.create"),
                json!({"terminal": term(&id, &format!("r{k}"), "running", 100 + k)}),
            );
            attach_from(&mut m, &fx);
            let fx = m.close_active();
            let kill = id_of(&fx, "terminal.kill");
            let fx = event(
                &mut m,
                "terminal.removed",
                json!({"terminal_id": id, "reason": "killed"}),
            );
            // The neighbour (`2`) opens again.
            attach_from(&mut m, &fx);
            assert!(respond(&mut m, kill, json!({"removed": true})).is_empty());
            assert_eq!(m.terminal().unwrap().name, "2");
        }
        assert_eq!(names(&m), ["one", "2"]);
        assert_eq!(m.input_stream(), Some(5));
        // Not connected: nothing is created, the user is told.
        let mut off = Model::new(Want::Any, (80, 24));
        assert!(matches!(&off.create()[0], Effect::Notice(_)));
    }

    #[test]
    fn ctrl_w_kills_by_id_and_asks_before_ending_a_busy_terminal() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "claude", "running", 1),
                term("t-b", "other", "running", 2)
            ]),
        );
        attach_from(&mut m, &fx);
        // Ctrl+W: kill by id, without force first.
        let fx = m.close_active();
        let (kill, method, params) = sent(&fx)[0].clone();
        assert_eq!(method, "terminal.kill");
        assert_eq!(params, json!({"target": "t-a", "force": false}));
        // The Host refuses: busy. The window asks; nothing else happens.
        let fx = fail(
            &mut m,
            kill,
            "busy",
            json!({"terminal_id": "t-a", "program": "claude.exe", "processes": 3, "busy": true}),
        );
        assert_eq!(
            fx,
            [Effect::Confirm(Confirm {
                id: "t-a".into(),
                name: "claude".into(),
                program: "claude.exe".into(),
                processes: 3,
                pane: None,
                pinned: false,
                restart: false,
            })]
        );
        assert_eq!(names(&m), ["claude", "other"]);
        // Meanwhile another client renames it: the confirmed kill still targets the same id.
        event(
            &mut m,
            "terminal.renamed",
            json!({"terminal_id": "t-a", "name": "renamed", "previous_name": "claude"}),
        );
        let fx = m.confirm_close("t-a");
        assert_eq!(sent(&fx)[0].2, json!({"target": "t-a", "force": true}));
        let kill = sent(&fx)[0].0;
        // The removal closes the tab quietly and shows the neighbour; `other` is untouched.
        let fx = event(
            &mut m,
            "terminal.removed",
            json!({"terminal_id": "t-a", "reason": "killed"}),
        );
        assert!(!fx.iter().any(|e| matches!(e, Effect::Notice(_))));
        assert_eq!(names(&m), ["other"]);
        assert_eq!(
            methods(&fx),
            ["pane.attach"],
            "the old pane is gone: no detach"
        );
        assert_eq!(sent(&fx)[0].2["target"], "t-b");
        respond(&mut m, kill, json!({"removed": true}));
        assert_eq!(names(&m), ["other"]);
        // An idle shell is killed without a question (the Host decides busy, ADR 0020 §3).
        attach_from(&mut m, &fx);
        let fx = m.close_active();
        let kill = id_of(&fx, "terminal.kill");
        let fx = respond(&mut m, kill, json!({"removed": true, "terminal_id": "t-b"}));
        assert!(fx.iter().all(|e| !matches!(e, Effect::Confirm(_))));
        assert!(m.tabs().is_empty());
        assert_eq!(m.phase, Phase::Empty { notice: None });
        // Confirming a Terminal that is gone meanwhile ends nothing.
        assert!(matches!(&m.confirm_close("t-a")[0], Effect::Notice(_)));
    }

    #[test]
    fn hiding_a_tab_is_a_detach_and_removing_a_stored_terminal_is_separate() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "one", "running", 1),
                term("t-b", "two", "running", 2),
                term("t-s", "kept", "stopped", 0)
            ]),
        );
        attach_from(&mut m, &fx);
        // × / "Hide tab": only a detach; the Terminal stays in the Host and in `hidden`.
        let fx = m.hide(0);
        assert_eq!(methods(&fx), ["pane.detach", "pane.attach"]);
        assert_eq!(names(&m), ["two", "kept"]);
        assert_eq!(m.hidden()[0].id, "t-a");
        for method in methods(&fx) {
            assert!(!method.starts_with("terminal."));
        }
        // A list (another client's change) keeps it hidden.
        let fx = event(
            &mut m,
            "terminal.created",
            json!({"terminal": term("t-c", "three", "running", 3)}),
        );
        assert!(sent(&fx).is_empty());
        assert_eq!(names(&m), ["two", "kept", "three"]);
        // Unhide: back as the last tab, opened.
        let fx = m.unhide("t-a");
        assert!(methods(&fx).contains(&"pane.attach".to_string()));
        assert_eq!(names(&m), ["two", "kept", "three", "one"]);
        // Ctrl+W on the stored Terminal removes its record (never with force).
        let fx = m.select(1);
        assert_eq!(m.phase.name(), "stored");
        let _ = fx;
        let fx = m.close_active();
        let (rm, method, params) = sent(&fx)[0].clone();
        assert_eq!(method, "terminal.remove");
        assert_eq!(params, json!({"target": "t-s"}));
        respond(&mut m, rm, json!({"removed": true}));
        assert_eq!(names(&m), ["two", "three", "one"]);
        // Moving tabs is this window's order only.
        assert_eq!(m.move_tab(0, 1), [Effect::Tabs]);
        assert_eq!(names(&m), ["three", "two", "one"]);
        assert!(m.move_tab(0, -1).is_empty());
    }

    #[test]
    fn tabs_follow_other_clients_create_rename_stop_and_kill() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "one", "running", 1),
                term("t-b", "two", "running", 2)
            ]),
        );
        attach_from(&mut m, &fx);
        // `subak new three`.
        event(
            &mut m,
            "terminal.created",
            json!({"terminal": term("t-c", "three", "starting", 0)}),
        );
        assert_eq!(names(&m), ["one", "two", "three"]);
        assert_eq!(m.tabs()[2].state, "starting");
        event(
            &mut m,
            "terminal.state_changed",
            json!({"terminal_id": "t-c", "state": "running", "previous_state": "starting"}),
        );
        assert!(m.tabs()[2].running());
        // `subak rename two deux` and of the shown one (remembered under the new name).
        event(
            &mut m,
            "terminal.renamed",
            json!({"terminal_id": "t-b", "name": "deux", "previous_name": "two"}),
        );
        let fx = event(
            &mut m,
            "terminal.renamed",
            json!({"terminal_id": "t-a", "name": "eins", "previous_name": "one"}),
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Remember(t) if t.name == "eins"))
        );
        assert_eq!(names(&m), ["eins", "deux", "three"]);
        assert_eq!(m.terminal().unwrap().name, "eins");
        // `subak stop deux`: its tab shows stopped.
        event(
            &mut m,
            "terminal.state_changed",
            json!({"terminal_id": "t-b", "state": "stopped", "previous_state": "stopping"}),
        );
        assert!(m.tabs()[1].stored());
        // `subak kill three` (not shown): its tab goes silently.
        event(
            &mut m,
            "terminal.removed",
            json!({"terminal_id": "t-c", "reason": "killed"}),
        );
        assert_eq!(names(&m), ["eins", "deux"]);
        // `subak kill eins` (shown): its last screen stays with a banner; Enter closes the tab.
        event(
            &mut m,
            "terminal.removed",
            json!({"terminal_id": "t-a", "reason": "killed"}),
        );
        assert!(matches!(
            &m.phase,
            Phase::Ended {
                why: EndReason::Removed,
                ..
            }
        ));
        assert!(m.phase.shows_pane());
        assert!(m.tabs()[0].gone());
        let fx = m.activate();
        assert!(sent(&fx).is_empty(), "a stored tab needs no attach");
        assert_eq!(names(&m), ["deux"]);
        assert_eq!(m.phase.name(), "stored");
    }

    #[test]
    fn a_named_or_remembered_terminal_wins_and_a_missing_one_falls_back() {
        let both = json!([
            term("t-a", "one", "running", 1),
            term("t-b", "Two", "running", 2)
        ]);
        // Named on the command line (case is ignored, as by the Host).
        let mut m = Model::new(Want::Target("two".into()), (80, 24));
        let fx = connect_with(&mut m, both.clone());
        assert_eq!(sent(&fx)[0].2["target"], "two");
        // Remembered by id from the last run.
        let mut m = Model::new(
            Want::Last {
                id: "t-b".into(),
                name: "renamed".into(),
            },
            (80, 24),
        );
        let fx = connect_with(&mut m, both.clone());
        assert_eq!(sent(&fx)[0].2["target"], "t-b");
        // A name the Host does not know: the attach fails, the first tab opens with a notice.
        let mut m = Model::new(Want::Target("nope".into()), (80, 24));
        let fx = connect_with(&mut m, both.clone());
        let fx = fail(&mut m, sent(&fx)[0].0, "target_not_found", json!({}));
        assert_eq!(m.phase.name(), "empty");
        let fx = respond(
            &mut m,
            id_of(&fx, "terminal.list"),
            json!({"terminals": both}),
        );
        assert_eq!(sent(&fx)[0].2["target"], "t-a");
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Notice(n) if n.contains("`nope`"))),
            "{fx:?}"
        );
    }

    #[test]
    fn snapshots_reset_the_view_and_other_streams_are_ignored() {
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 7)]));
        attach_from(&mut m, &fx);
        assert_eq!(
            m.on_data(7, true),
            Frame::Reset {
                cols: 100,
                rows: 30,
                history: 5000
            }
        );
        assert_eq!(m.on_data(7, true), Frame::Feed);
        assert_eq!(m.on_data(7, false), Frame::Feed);
        assert_eq!(m.on_data(8, false), Frame::Ignore);
        m.on_grid(90, 20);
        event(
            &mut m,
            "pane.resync",
            json!({"pane_id": "p-t-a", "terminal_id": "t-a", "stream_id": 7, "seq": 99}),
        );
        assert_eq!(
            m.on_data(7, true),
            Frame::Reset {
                cols: 90,
                rows: 20,
                history: 5000
            }
        );
    }

    #[test]
    fn resizes_go_to_a_live_pane_only_and_closing_the_window_detaches() {
        let mut m = Model::new(Want::Any, (100, 30));
        assert!(m.on_grid(90, 25).is_empty(), "nothing attached");
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 7)]));
        attach_from(&mut m, &fx);
        let s = sent(&m.on_grid(120, 40));
        assert_eq!(s[0].1, "pane.resize");
        assert_eq!(s[0].2, json!({"cols": 120, "rows": 40, "stream_id": 7}));
        assert!(m.on_grid(120, 40).is_empty(), "unchanged");
        let s = sent(&m.on_close());
        assert_eq!(s[0].1, "pane.detach");
    }

    #[test]
    fn the_pane_ending_or_being_detached_keeps_its_screen() {
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 7)]));
        attach_from(&mut m, &fx);
        event(
            &mut m,
            "pane.detached",
            json!({"pane_id": "p-t-a", "terminal_id": "t-a", "stream_id": 7,
                "by": {"client_id": 4, "pid": 4242, "kind": "cli"}}),
        );
        assert!(matches!(
            &m.phase,
            Phase::Ended { why: EndReason::DetachedBy(Some(by)), .. } if by == "cli client (pid 4242)"
        ));
        assert!(m.phase.shows_pane());
        assert_eq!(m.input_stream(), None, "no input after a detach");
        // Enter reattaches the same Terminal.
        let s = sent(&m.activate());
        assert_eq!(s[0].2["target"], "t-a");
        respond(&mut m, s[0].0, attach_result(8, "t-a", 7));
        event(
            &mut m,
            "pane.exited",
            json!({"pane_id": "p-t-a", "terminal_id": "t-a", "pid": 7, "exit_code": 0}),
        );
        assert!(matches!(
            m.phase,
            Phase::Ended {
                why: EndReason::Exited(Some(0)),
                ..
            }
        ));
        assert!(m.tabs()[0].gone());
    }

    #[test]
    fn a_lost_host_is_retried_with_backoff_and_the_same_terminal_reattached() {
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 11)]));
        attach_from(&mut m, &fx);
        event(&mut m, "host.stopping", json!({"reason": "stop_requested"}));
        let fx = m.on_disconnected();
        assert_eq!(fx[0], Effect::Reconnect(Duration::from_millis(500)));
        let Phase::Disconnected { last, reason, .. } = &m.phase else {
            panic!()
        };
        assert_eq!(last.as_ref().unwrap().id, "t-a");
        assert_eq!(reason, "the Host was stopped");
        assert!(m.phase.shows_pane(), "the last screen stays");
        assert!(
            m.close_active()
                .iter()
                .all(|e| matches!(e, Effect::Notice(_)))
        );
        let mut delays = Vec::new();
        for _ in 0..7 {
            for e in m.on_connect_failed("host_not_running", "no Host", None) {
                if let Effect::Reconnect(d) = e {
                    delays.push(d.as_millis());
                }
            }
        }
        assert_eq!(delays, [1000, 2000, 4000, 8000, 10000, 10000, 10000]);
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 11)]));
        let s = sent(&fx);
        assert_eq!(s[0].1, "pane.attach");
        assert_eq!(s[0].2["target"], "t-a");
        // If it is gone, another tab opens and the window says so.
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = connect_with(&mut m, json!([term("t-a", "build", "running", 11)]));
        attach_from(&mut m, &fx);
        m.on_disconnected();
        let fx = connect_with(&mut m, json!([term("t-x", "other", "running", 3)]));
        assert_eq!(names(&m), ["other"]);
        assert_eq!(sent(&fx)[0].2["target"], "t-x");
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Notice(n) if n == "`build` is no longer running")),
            "{fx:?}"
        );
    }

    #[test]
    fn a_failed_first_connection_waits_for_the_user() {
        let mut m = Model::new(Want::Any, (100, 30));
        let fx = m.on_connect_failed(
            "host_identity_mismatch",
            "not a Subak Host",
            Some("use --endpoint"),
        );
        assert_eq!(fx, [Effect::Phase, Effect::Tabs]);
        assert_eq!(m.phase.name(), "failed");
        assert_eq!(m.activate(), [Effect::Connect, Effect::Phase, Effect::Tabs]);
    }

    #[test]
    fn renames_go_by_id_and_report_refusals() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(&mut m, json!([term("t-a", "one", "running", 1)]));
        attach_from(&mut m, &fx);
        let fx = m.rename("t-a", "빌드");
        let (id, method, params) = sent(&fx)[0].clone();
        assert_eq!(method, "terminal.rename");
        assert_eq!(params, json!({"target": "t-a", "name": "빌드"}));
        assert_eq!(
            respond(&mut m, id, json!({"changed": true})),
            [Effect::Renamed(Ok(()))]
        );
        let fx = m.rename("t-a", "taken");
        let fx = fail(&mut m, sent(&fx)[0].0, "name_conflict", json!({}));
        assert!(matches!(&fx[0], Effect::Renamed(Err(e)) if e.contains("name_conflict")));
    }

    /// #98 T3 (`CORE-80`, `CORE-81`): output or a bell in a Terminal that is not shown marks
    /// its tab (a bell wins over output); the shown Terminal never gets a mark and seeing a
    /// Terminal clears its own.
    #[test]
    fn background_output_and_bells_mark_the_tabs_until_seen() {
        let mut m = Model::new(Want::Any, (80, 24));
        connect_with(
            &mut m,
            json!([
                term("t-a", "a", "running", 10),
                term("t-b", "b", "running", 11)
            ]),
        );
        let shown = m.terminal().unwrap().id.clone();
        let other = if shown == "t-a" { "t-b" } else { "t-a" };
        let fx = event(
            &mut m,
            "terminal.activity",
            json!({"terminal_id": shown, "pane_id": "p", "kind": "bell"}),
        );
        assert!(
            fx.is_empty() && m.alert(&shown).is_none(),
            "the shown one: nothing"
        );
        let fx = event(
            &mut m,
            "terminal.activity",
            json!({"terminal_id": other, "pane_id": "p", "kind": "output"}),
        );
        assert!(fx.iter().any(|e| matches!(e, Effect::Tabs)), "{fx:?}");
        assert_eq!(m.alert(other), Some(Alert::Output));
        event(
            &mut m,
            "terminal.activity",
            json!({"terminal_id": other, "pane_id": "p", "kind": "bell"}),
        );
        event(
            &mut m,
            "terminal.activity",
            json!({"terminal_id": other, "pane_id": "p", "kind": "output"}),
        );
        assert_eq!(m.alert(other), Some(Alert::Bell), "a bell stays a bell");
        m.seen();
        assert_eq!(
            m.alert(other),
            Some(Alert::Bell),
            "seeing the shown one clears only it"
        );
    }

    /// #98 T3 (`CORE-78`, `CORE-79`): progress events set and clear a Terminal's progress;
    /// finished commands and other Terminals' ended programs become `Effect::Finished`.
    #[test]
    fn progress_and_finished_commands_reach_the_window() {
        use subak_term::screen::{Progress, ProgressState};
        let mut m = Model::new(Want::Any, (80, 24));
        connect_with(
            &mut m,
            json!([
                term("t-a", "a", "running", 10),
                term("t-b", "build", "running", 11)
            ]),
        );
        let fx = event(
            &mut m,
            "terminal.progress",
            json!({"terminal_id": "t-b", "pane_id": "p", "state": "normal", "value": 42}),
        );
        assert!(fx.iter().any(|e| matches!(e, Effect::Tabs)), "{fx:?}");
        assert_eq!(
            m.progress("t-b"),
            Some(Progress {
                state: ProgressState::Normal,
                value: 42
            })
        );
        let fx = event(
            &mut m,
            "terminal.progress",
            json!({"terminal_id": "t-b", "pane_id": "p", "state": "normal", "value": 42}),
        );
        assert!(fx.is_empty(), "no change: no redraw");
        event(
            &mut m,
            "terminal.progress",
            json!({"terminal_id": "t-b", "pane_id": "p", "state": "none", "value": 0}),
        );
        assert_eq!(m.progress("t-b"), None);
        let fx = event(
            &mut m,
            "terminal.command_done",
            json!({"terminal_id": "t-b", "pane_id": "p", "exit_code": 3, "duration_ms": 1500}),
        );
        assert_eq!(
            fx,
            [Effect::Finished(Finished {
                terminal_id: "t-b".into(),
                name: "build".into(),
                command: true,
                exit_code: Some(3),
                duration_ms: Some(1500),
            })]
        );
        let fx = event(
            &mut m,
            "pane.exited",
            json!({"terminal_id": "t-b", "pane_id": "p-b", "pid": 11, "exit_code": 0}),
        );
        assert!(
            matches!(&fx[..], [Effect::Finished(f)] if !f.command && f.exit_code == Some(0) && f.name == "build"),
            "{fx:?}"
        );
    }

    /// The id of the shown Terminal.
    fn shown_id(m: &Model) -> String {
        m.terminal().unwrap().id.clone()
    }

    /// #98 T1 (`CORE-15`, `CORE-10`, `CORE-14`): the MRU order follows what was shown, a walk
    /// through it does not reorder it, `tab.last` goes back; `tabs.cycle` off stops at the
    /// ends; pinned tabs come first and closing one asks.
    #[test]
    fn mru_switching_wrap_and_pinned_tabs() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "a", "running", 10),
                term("t-b", "b", "running", 11),
                term("t-c", "c", "running", 12)
            ]),
        );
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-a");
        let fx = m.select(2);
        attach_from(&mut m, &fx);
        let fx = m.select(1);
        attach_from(&mut m, &fx);
        assert_eq!(m.mru(), ["t-b", "t-c", "t-a"]);
        let fx = m.last_tab();
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-c", "the one before");
        assert_eq!(m.mru(), ["t-c", "t-b", "t-a"]);
        let fx = m.cycle_mru(1);
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-b");
        let fx = m.cycle_mru(1);
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-a", "further back");
        assert_eq!(m.mru(), ["t-c", "t-b", "t-a"], "a walk keeps the order");
        let fx = m.cycle_mru(-1);
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-b");
        // Wrap off: the ends stop.
        m.set_wrap(false);
        let fx = m.select(2);
        attach_from(&mut m, &fx);
        assert!(m.cycle(1).is_empty(), "no wrap at the end");
        assert_eq!(shown_id(&m), "t-c");
        m.set_wrap(true);
        let fx = m.cycle(1);
        attach_from(&mut m, &fx);
        assert_eq!(shown_id(&m), "t-a", "wraps");
        let fx = m.select(2);
        attach_from(&mut m, &fx);
        // Pin the third tab: it comes first; closing it asks.
        m.select(2);
        m.toggle_pin();
        assert_eq!(m.tabs()[0].id, "t-c");
        assert!(m.pinned("t-c"));
        assert_eq!(m.pinned_ids(), ["t-c"]);
        let fx = m.close_active();
        assert!(
            matches!(&fx[..], [Effect::Confirm(c)] if c.pinned && c.id == "t-c"),
            "{fx:?}"
        );
        m.toggle_pin();
        assert!(!m.pinned("t-c"));
        let fx = m.close_active();
        assert_eq!(
            methods(&fx),
            ["terminal.kill"],
            "unpinned: closes as before"
        );
    }

    /// #98 T1 (`CORE-12`, `CORE-17`): a copy goes right after its tab; a restart takes the old
    /// tab's place (and its pin); a busy restart asks, and agreeing sends it with `force`.
    #[test]
    fn duplicates_follow_their_tab_and_restarts_take_its_place() {
        let mut m = Model::new(Want::Any, (80, 24));
        let fx = connect_with(
            &mut m,
            json!([
                term("t-a", "a", "running", 10),
                term("t-b", "b", "running", 11)
            ]),
        );
        attach_from(&mut m, &fx);
        let fx = m.duplicate(false, false);
        let (id, method, params) = sent(&fx).remove(0);
        assert_eq!(method, "terminal.duplicate");
        assert_eq!(
            params,
            json!({"target": "t-a", "restart": false, "force": false})
        );
        respond(
            &mut m,
            id,
            json!({"terminal": term("t-c", "a-2", "running", 12)}),
        );
        assert_eq!(names(&m), ["a", "a-2", "b"]);
        // Restart `a` (pinned): busy first, then forced; the new one takes its place and pin.
        let fx = m.select(0);
        attach_from(&mut m, &fx);
        m.toggle_pin();
        let fx = m.duplicate(true, false);
        let (id, _, _) = sent(&fx).remove(0);
        let fx = fail(
            &mut m,
            id,
            "busy",
            json!({"program": "x.exe", "processes": 2}),
        );
        assert!(
            matches!(&fx[..], [Effect::Confirm(c)] if c.restart && c.id == "t-a"),
            "{fx:?}"
        );
        let fx = m.duplicate(true, true);
        let (id, _, params) = sent(&fx).remove(0);
        assert_eq!(params["force"], true);
        respond(
            &mut m,
            id,
            json!({"terminal": term("t-d", "a", "running", 13)}),
        );
        assert_eq!(
            m.tabs().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t-d", "t-c", "t-b"]
        );
        assert!(m.pinned("t-d"), "the pin moved to the new Terminal");
    }
}
