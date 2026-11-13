//! Step 61 — the settings window's model (NOEUL-FR-101, NOEUL-FR-103, NOEUL-FR-111,
//! NOEUL-NFR-012, ADR 0075): portable, no window code.
//!
//! The window shows every Noeul action with its keys, lets the user record new keys, disable or
//! reset one, reset all, import and export the shortcuts, choose what a double-click and a
//! middle-click inside a selection do (double-click **pins** by default — Noeul's signature,
//! not Snipaste's copy), and switch the tray icon, the disk history and the pin backup.
//!
//! **Every change is a `noeul` command** (`hotkeys set|disable|reset|import|export`, `config
//! set`): the window runs the same command the CLI documents, so a GUI change and a CLI change
//! are one code path with one set of checks — conflicts, reserved keys, validation, atomic writes
//! with a backup (KIM-FR-013). The model only builds those commands and reads their answers.
//!
//! **Recording keys never triggers them**: while the window records, the instance ignores its
//! global hotkeys ([`crate::app::App::set_recording`]); the recorder takes every key, including
//! `F1`, `F3` and `Tab`. `Escape` alone cancels; modifiers alone wait for a key.

use serde_json::Value;

/// One action row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    /// Changed by the user (`source` other than `default`).
    pub changed: bool,
    /// Can the user bind it (some navigation actions are fixed).
    pub bindable: bool,
}

impl Row {
    /// The list entry and its accessible name: "Capture a region — F1", "… — no keys (changed)".
    pub fn text(&self) -> String {
        let keys = if self.keys.is_empty() {
            "no keys".to_string()
        } else {
            self.keys.join(", ")
        };
        format!(
            "{} — {keys}{}",
            self.title,
            if self.changed { " (changed)" } else { "" }
        )
    }
}

/// What a double-click and a middle-click inside a confirmed selection may do.
pub const DOUBLE_CLICK: [&str; 4] = ["pin", "copy", "save", "none"];
pub const MIDDLE_CLICK: [&str; 3] = ["pin", "copy", "none"];

/// The switches of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    TrayIcon,
    HistoryDisk,
    PinBackup,
}

impl Toggle {
    pub const ALL: [Toggle; 3] = [Toggle::TrayIcon, Toggle::HistoryDisk, Toggle::PinBackup];

    pub fn label(self) -> &'static str {
        match self {
            Toggle::TrayIcon => "Show the tray icon (from the next start)",
            Toggle::HistoryDisk => "Keep the capture history on disk",
            Toggle::PinBackup => "Back up pins to restore them after a restart",
        }
    }
}

/// What a key pressed while recording did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A modifier alone, or a key that cannot be bound: keep recording.
    Waiting,
    /// `Escape` alone: recording ended, nothing changes.
    Cancelled,
    /// The command that binds the recorded keys.
    Bind(Vec<String>),
}

/// The window's state.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    /// The action being recorded.
    pub recording: Option<String>,
    pub double_click: String,
    pub middle_click: String,
    pub tray_icon: bool,
    pub history_disk: bool,
    pub pin_backup: bool,
    /// The status line (results and problems in words; read by screen readers).
    pub status: String,
    /// A refused binding the user may force (`--replace` or `--allow-conflict`).
    pub pending: Option<Vec<String>>,
    /// #99 (`PIN-15`): the running instance's pin groups (empty when Noeul does not run).
    pub groups: Vec<GroupRow>,
    pub group_selected: Option<usize>,
}

/// #99 (`PIN-15`): one pin group as the settings window lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupRow {
    pub name: String,
    pub pins: u64,
    pub hidden: u64,
    pub current: bool,
}

impl GroupRow {
    /// `default — 3 pins (1 hidden), current`.
    pub fn text(&self) -> String {
        let mut t = format!(
            "{} \u{2014} {} pin{}",
            self.name,
            self.pins,
            if self.pins == 1 { "" } else { "s" }
        );
        if self.hidden > 0 {
            t.push_str(&format!(" ({} hidden)", self.hidden));
        }
        if self.current {
            t.push_str(", current");
        }
        t
    }
}

/// #99 (`PIN-15`): what a group button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupAction {
    Switch,
    Show,
    Hide,
    Close,
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

impl Model {
    /// From `noeul hotkeys list --json` and `noeul config show --json` (their `data`).
    pub fn from_docs(hotkeys: &Value, config: &Value) -> Model {
        let rows = hotkeys["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| Row {
                id: a["id"].as_str().unwrap_or("").to_string(),
                title: a["title"].as_str().unwrap_or("").to_string(),
                keys: a["bindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["active"] != false)
                    .filter_map(|b| b["keys"].as_str().map(String::from))
                    .collect(),
                changed: a["source"].as_str().is_some_and(|s| s != "default"),
                bindable: a["bindable"] != false,
            })
            .collect();
        let s = &config["settings"];
        Model {
            rows,
            selected: Some(0),
            double_click: s["gestures"]["double_click"]
                .as_str()
                .unwrap_or("pin")
                .into(),
            middle_click: s["gestures"]["middle_click"]
                .as_str()
                .unwrap_or("pin")
                .into(),
            tray_icon: s["tray_icon"] != false,
            history_disk: s["history"]["disk"] == "on",
            pin_backup: s["pins"]["backup"] == "on",
            ..Model::default()
        }
    }

    /// #99 (`PIN-15`): the groups from `noeul pin group list --json` (its `data`; nothing when
    /// the instance does not run).
    pub fn set_groups(&mut self, list: &Value) {
        let current = list["current"].as_str().unwrap_or("");
        let name = self
            .group_selected
            .and_then(|i| self.groups.get(i))
            .map(|g| g.name.clone());
        self.groups = list["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|g| {
                let name = g["name"].as_str().unwrap_or("").to_string();
                GroupRow {
                    current: name == current,
                    name,
                    pins: g["pins"].as_u64().unwrap_or(0),
                    hidden: g["hidden"].as_u64().unwrap_or(0),
                }
            })
            .collect();
        self.group_selected = name
            .and_then(|n| self.groups.iter().position(|g| g.name == n))
            .or_else(|| (!self.groups.is_empty()).then_some(0));
    }

    /// #99 (`PIN-15`): the command for a group button on the selected group.
    pub fn group_command(&self, a: GroupAction) -> Option<Vec<String>> {
        let g = self.group_selected.and_then(|i| self.groups.get(i))?;
        let n = g.name.as_str();
        Some(match a {
            GroupAction::Switch => args(&["pin", "group", "switch", n]),
            GroupAction::Show => args(&["pin", "show", "--group", n]),
            GroupAction::Hide => args(&["pin", "hide", "--group", n]),
            GroupAction::Close => args(&["pin", "close", "--group", n, "--yes"]),
        })
    }

    /// Keeps the selection on the same action after the rows are read again.
    pub fn refresh(&mut self, fresh: Model) {
        let id = self.selected_row().map(|r| r.id.clone());
        let status = std::mem::take(&mut self.status);
        let pending = self.pending.take();
        let (groups, group_selected) =
            (std::mem::take(&mut self.groups), self.group_selected.take());
        *self = fresh;
        self.status = status;
        self.pending = pending;
        self.groups = groups;
        self.group_selected = group_selected;
        if let Some(id) = id {
            self.selected = self.rows.iter().position(|r| r.id == id).or(Some(0));
        }
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.selected.and_then(|i| self.rows.get(i))
    }

    /// `Change…`: start recording the selected action's new keys.
    pub fn start_recording(&mut self) -> bool {
        let Some((id, title)) = self
            .selected_row()
            .filter(|r| r.bindable)
            .map(|r| (r.id.clone(), r.title.clone()))
        else {
            self.status = "This action's keys cannot be changed.".into();
            return false;
        };
        self.recording = Some(id);
        self.pending = None;
        self.status =
            format!("Press the new keys for \"{title}\" (Esc cancels). They will not run now.");
        true
    }

    /// A key pressed while recording: `name` is the logical key name (`None`: a modifier or a
    /// key a binding cannot name).
    pub fn key(&mut self, name: Option<&str>, ctrl: bool, alt: bool, shift: bool) -> Key {
        let Some(action) = self.recording.clone() else {
            return Key::Waiting;
        };
        let Some(name) = name else {
            return Key::Waiting;
        };
        if name == "Escape" && !ctrl && !alt && !shift {
            self.recording = None;
            self.status = "Recording cancelled; nothing changed.".into();
            return Key::Cancelled;
        }
        let Some(chord) = crate::hotkeys::chord_of(name, ctrl, alt, shift) else {
            self.status = format!("{name} cannot be bound; press another key.");
            return Key::Waiting;
        };
        self.recording = None;
        Key::Bind(vec![
            "hotkeys".into(),
            "set".into(),
            action,
            chord.to_string(),
        ])
    }

    pub fn disable(&self) -> Option<Vec<String>> {
        let r = self.selected_row()?;
        Some(args(&["hotkeys", "disable", &r.id]))
    }

    pub fn reset(&self) -> Option<Vec<String>> {
        let r = self.selected_row()?;
        Some(args(&["hotkeys", "reset", &r.id]))
    }

    /// `Reset all…` (the window asked first): every shortcut back to its default, a backup kept.
    pub fn reset_all() -> Vec<String> {
        args(&["hotkeys", "reset", "--all", "--yes"])
    }

    pub fn import(path: &str) -> Vec<String> {
        args(&["hotkeys", "import", path])
    }

    pub fn export() -> Vec<String> {
        args(&["hotkeys", "export"])
    }

    /// The file the window's Export writes: the hotkeys document of `noeul hotkeys export --json`
    /// (its `data.document`), which `noeul hotkeys import` reads back. The whole `data` object
    /// (with `path` and `tool` around the document) was written before, and Import refused it
    /// (found by the real-desktop check of #66).
    pub fn export_text(reply: &Value) -> String {
        let data = &reply["data"];
        let doc = data.get("document").unwrap_or(data);
        serde_json::to_string_pretty(doc).unwrap_or_default()
    }

    pub fn gesture(double: bool, value: &str) -> Vec<String> {
        let key = if double {
            "gestures.double_click"
        } else {
            "gestures.middle_click"
        };
        args(&["config", "set", key, value])
    }

    pub fn toggle(t: Toggle, on: bool) -> Vec<String> {
        match t {
            Toggle::TrayIcon => args(&[
                "config",
                "set",
                "general.tray_icon",
                if on { "true" } else { "false" },
            ]),
            Toggle::HistoryDisk => args(&[
                "config",
                "set",
                "history.disk",
                if on { "on" } else { "off" },
            ]),
            Toggle::PinBackup => args(&[
                "config",
                "set",
                "pins.backup",
                if on { "on" } else { "off" },
            ]),
        }
    }

    /// `Use anyway`: the refused binding again, taking the keys from the other action
    /// (`--replace`) or accepting a conflict with another tool (`--allow-conflict`).
    pub fn force(&mut self) -> Option<Vec<String>> {
        self.pending.take()
    }

    /// Reads a command's answer (`--json` envelope) into the status line.
    pub fn answer(&mut self, cmd: &[String], doc: &Value) {
        self.pending = None;
        if doc["ok"] == true {
            self.status = match cmd.get(..2).map(|c| [c[0].as_str(), c[1].as_str()]) {
                Some(["hotkeys", "set"]) => {
                    format!("{} now runs on {}.", title_of(self, &cmd[2]), cmd[3])
                }
                Some(["hotkeys", "disable"]) => {
                    format!("{} has no keys now.", title_of(self, &cmd[2]))
                }
                Some(["hotkeys", "reset"]) if cmd.get(2).map(String::as_str) == Some("--all") => {
                    "Every shortcut is back to its default (a backup of your file was kept).".into()
                }
                Some(["hotkeys", "reset"]) => {
                    format!("{} is back to its default keys.", title_of(self, &cmd[2]))
                }
                Some(["hotkeys", "import"]) => {
                    "The shortcuts were imported (a backup was kept).".into()
                }
                Some(["config", "set"]) => "Saved.".into(),
                Some(["pin", "group"]) => format!(
                    "Switched to the group {}: its pins show, the others are hidden.",
                    cmd.get(3).map_or("", String::as_str)
                ),
                Some(["pin", "show" | "hide" | "close"]) => format!(
                    "The pins of the group {} were {}.",
                    cmd.get(3).map_or("", String::as_str),
                    match cmd[1].as_str() {
                        "show" => "shown",
                        "hide" => "hidden",
                        _ => "closed",
                    }
                ),
                _ => "Done.".into(),
            };
            // A default it shadows, or a warning, in the CLI's own words.
            if let Some(m) = doc["data"]["conflicts"]
                .as_array()
                .and_then(|c| c.first())
                .and_then(|c| c["message"].as_str())
            {
                self.status.push(' ');
                self.status.push_str(m);
            }
            return;
        }
        let e = &doc["error"];
        let message = e["message"].as_str().unwrap_or("The change was refused.");
        let hint = e["hint"].as_str().unwrap_or("");
        self.status = message.to_string();
        if cmd.first().map(String::as_str) == Some("hotkeys")
            && cmd.get(1).map(String::as_str) == Some("set")
        {
            let flag = if hint.contains("--replace") {
                Some("--replace")
            } else if hint.contains("--allow-conflict") {
                Some("--allow-conflict")
            } else {
                None
            };
            if let Some(flag) = flag {
                let mut again = cmd.to_vec();
                again.push(flag.to_string());
                self.pending = Some(again);
                self.status.push_str(if flag == "--replace" {
                    " Choose \"Use anyway\" to move the keys to this action."
                } else {
                    " Choose \"Use anyway\" to keep both (the other tool may get them first)."
                });
            }
        }
    }
}

fn title_of(m: &Model, id: &str) -> String {
    m.rows
        .iter()
        .find(|r| r.id == id)
        .map_or_else(|| id.to_string(), |r| format!("\"{}\"", r.title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> Model {
        let hk = json!({"actions": [
            {"id": "capture.region", "title": "Capture a region", "source": "default", "bindable": true,
             "bindings": [{"keys": "F1", "active": true}]},
            {"id": "paste.float", "title": "Pin the clipboard", "source": "user", "bindable": true,
             "bindings": [{"keys": "Shift+F3", "active": true}]},
            {"id": "nav.cancel", "title": "Cancel", "source": "default", "bindable": false, "bindings": []}
        ]});
        let cfg = json!({"settings": {"tray_icon": true, "gestures": {"double_click": "pin", "middle_click": "pin"},
            "history": {"disk": "off"}, "pins": {"backup": "on"}}});
        Model::from_docs(&hk, &cfg)
    }

    /// NOEUL-FR-101: every action with its keys in words; double-click pins by default.
    #[test]
    fn rows_and_settings_come_from_the_cli_documents() {
        let m = model();
        assert_eq!(m.rows[0].text(), "Capture a region — F1");
        assert_eq!(m.rows[1].text(), "Pin the clipboard — Shift+F3 (changed)");
        assert_eq!(m.rows[2].text(), "Cancel — no keys");
        assert_eq!(
            (m.double_click.as_str(), m.middle_click.as_str()),
            ("pin", "pin")
        );
        assert!(m.tray_icon && !m.history_disk && m.pin_backup);
    }

    /// Recording takes any key (F1 too) without running it, Esc cancels, modifiers wait.
    #[test]
    fn recording_binds_a_chord_cancels_on_escape_and_waits_on_modifiers() {
        let mut m = model();
        assert!(m.start_recording());
        assert!(m.status.contains("Capture a region"));
        assert_eq!(
            m.key(None, true, false, false),
            Key::Waiting,
            "Ctrl alone waits"
        );
        assert_eq!(
            m.key(Some("F2"), true, false, true),
            Key::Bind(args(&["hotkeys", "set", "capture.region", "Ctrl+Shift+F2"]))
        );
        assert_eq!(m.recording, None);
        assert!(m.start_recording());
        assert_eq!(m.key(Some("Escape"), false, false, false), Key::Cancelled);
        assert!(m.status.contains("nothing changed"));
        // Escape with a modifier is a chord like any other.
        assert!(m.start_recording());
        assert!(matches!(
            m.key(Some("Escape"), false, false, true),
            Key::Bind(_)
        ));
        m.selected = Some(2);
        assert!(!m.start_recording(), "fixed actions cannot be recorded");
    }

    /// A conflict is said in words and can be forced with the flag the CLI's hint names.
    #[test]
    fn a_refused_binding_is_explained_and_can_be_forced() {
        let mut m = model();
        let cmd = args(&["hotkeys", "set", "capture.region", "Shift+F3"]);
        m.answer(&cmd, &json!({"ok": false, "error": {"code": "hotkey_conflict",
            "message": "Shift+F3 is used by paste.float", "hint": "re-run with --replace to move it"}}));
        assert!(m.status.starts_with("Shift+F3 is used by paste.float"));
        assert!(m.status.contains("Use anyway"));
        let again = m.force().unwrap();
        assert_eq!(again.last().map(String::as_str), Some("--replace"));
        assert!(m.force().is_none());
        m.answer(&again, &json!({"ok": true, "data": {}}));
        assert_eq!(m.status, "\"Capture a region\" now runs on Shift+F3.");
        m.answer(
            &again,
            &json!({"ok": true, "data": {"conflicts": [{"severity": "info",
            "message": "Your binding replaces the default F3."}]}}),
        );
        assert!(
            m.status
                .ends_with("now runs on Shift+F3. Your binding replaces the default F3."),
            "{}",
            m.status
        );
        m.answer(&args(&["hotkeys", "set", "x", "F9"]), &json!({"ok": false,
            "error": {"message": "F9 conflicts with Subak", "hint": "--allow-conflict accepts it"}}));
        assert_eq!(
            m.pending.as_ref().unwrap().last().map(String::as_str),
            Some("--allow-conflict")
        );
    }

    #[test]
    fn every_other_change_is_the_documented_command() {
        let mut m = model();
        m.selected = Some(1);
        assert_eq!(
            m.disable().unwrap(),
            args(&["hotkeys", "disable", "paste.float"])
        );
        assert_eq!(
            m.reset().unwrap(),
            args(&["hotkeys", "reset", "paste.float"])
        );
        assert_eq!(
            Model::reset_all(),
            args(&["hotkeys", "reset", "--all", "--yes"])
        );
        assert_eq!(
            Model::gesture(true, "copy"),
            args(&["config", "set", "gestures.double_click", "copy"])
        );
        assert_eq!(
            Model::toggle(Toggle::PinBackup, false),
            args(&["config", "set", "pins.backup", "off"])
        );
        assert_eq!(
            Model::toggle(Toggle::TrayIcon, true),
            args(&["config", "set", "general.tray_icon", "true"])
        );
        // The selection follows its action across a refresh.
        let mut fresh = model();
        fresh.rows.swap(0, 1);
        m.refresh(fresh);
        assert_eq!(m.selected_row().unwrap().id, "paste.float");
    }

    /// #99 (`PIN-15`): the groups list parses, keeps the selection by name and builds commands.
    #[test]
    fn the_groups_are_listed_and_their_buttons_build_commands() {
        let mut m = Model::default();
        assert_eq!(
            m.group_command(GroupAction::Switch),
            None,
            "nothing to act on"
        );
        let list = serde_json::json!({"current": "work", "groups": [
            {"name": "default", "pins": 3, "hidden": 1},
            {"name": "work", "pins": 1, "hidden": 0}]});
        m.set_groups(&list);
        assert_eq!(m.groups.len(), 2);
        assert_eq!(m.groups[0].text(), "default \u{2014} 3 pins (1 hidden)");
        assert_eq!(m.groups[1].text(), "work \u{2014} 1 pin, current");
        m.group_selected = Some(1);
        m.set_groups(&list);
        assert_eq!(
            m.group_selected,
            Some(1),
            "the selection stays on the same group"
        );
        assert_eq!(
            m.group_command(GroupAction::Close).unwrap(),
            ["pin", "close", "--group", "work", "--yes"]
        );
        assert_eq!(
            m.group_command(GroupAction::Switch).unwrap(),
            ["pin", "group", "switch", "work"]
        );
        m.set_groups(&Value::Null);
        assert!(
            m.groups.is_empty() && m.group_selected.is_none(),
            "Noeul not running"
        );
    }
}
