//! The rule form: one rule as the editor edits it (docs/DDALGAK_GUI_JOURNEYS.md J2–J4,
//! `DDALGAK-FR-050`).
//!
//! A rule is a sentence: *When I press* the trigger *do* one effect. The form holds plain field
//! values; the validation, summaries and previews are the engine's own data models of Steps
//! 68–72 (`remap`, `text::EditorDraft`, `launch::ProgramDraft`/`UrlDraft`, `mouse::MouseDraft`,
//! `modes`), so the editor never re-invents a check. [`RuleForm::build`] turns the fields into a
//! [`Rule`] (canonical keys, defaults left out, every field the form does not show kept from the
//! rule it started from); the document checks of the rule store then run on the whole document
//! before anything is written (`crate::store`).
//!
//! Nothing here performs an effect: [`RuleForm::simulate`] describes what the rule would do in
//! the playground's simulate mode (the mouse on a simulated pointer); it never sends a key,
//! types, opens a program or website, or moves the pointer.

use ddalgak::edit::{canonical_output, canonical_trigger};
use ddalgak::inject::Button;
use ddalgak::launch::{ProgramDraft, UrlDraft};
use ddalgak::model::{
    Action, AppMatcher, Condition, EngineOp, Grant, ModeOp, Permission, Profile, Rule, TitleMatch,
    Trigger, TriggerOn,
};
use ddalgak::modes::{ModeAction, ModeSet, ModeState};
use ddalgak::mouse::{
    self, Anchor, Grid, HoldChoice, Layout, MouseAction, MouseDraft, Point, PointSpec, ScrollDir,
};
use ddalgak::text::{EditorDraft, FieldProblem};
use ddalgak::validate;
use serde_json::{Map, Value, json};

/// The effect tiles (GUI_JOURNEYS J2 step 2). The issue's seven action categories are key remap
/// (`Remap`, with `Block`), text typing (`TypeText`, with `SendKeys`), program open, website
/// open, pointer move/scroll/click (`Mouse`), mode set/toggle and Ddalgak enable/disable
/// (`Engine`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Effect {
    Remap,
    SendKeys,
    TypeText,
    OpenProgram,
    OpenUrl,
    Mouse,
    Mode,
    Engine,
    Block,
    /// Step 79.
    Window,
    Clipboard,
    Notify,
}

impl Effect {
    /// Step 79: the effects under "More" (with `Block`).
    pub const MORE: [Effect; 4] = [
        Effect::Window,
        Effect::Clipboard,
        Effect::Notify,
        Effect::Block,
    ];

    /// The eight tiles in display order; `Block` is under "More".
    pub const TILES: [Effect; 8] = [
        Effect::Remap,
        Effect::SendKeys,
        Effect::TypeText,
        Effect::OpenProgram,
        Effect::OpenUrl,
        Effect::Mouse,
        Effect::Mode,
        Effect::Engine,
    ];

    /// The label key part (`tile.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Effect::Remap => "remap",
            Effect::SendKeys => "send_keys",
            Effect::TypeText => "type_text",
            Effect::OpenProgram => "open_program",
            Effect::OpenUrl => "open_url",
            Effect::Mouse => "mouse",
            Effect::Mode => "mode",
            Effect::Engine => "engine",
            Effect::Block => "block",
            Effect::Window => "window",
            Effect::Clipboard => "clipboard",
            Effect::Notify => "notify",
        }
    }

    /// A small glyph next to the title; the title always says it in words too.
    pub fn icon(self) -> &'static str {
        match self {
            Effect::Remap => "⌨",
            Effect::SendKeys => "⌘",
            Effect::TypeText => "Aa",
            Effect::OpenProgram => "▢",
            Effect::OpenUrl => "◎",
            Effect::Mouse => "➚",
            Effect::Mode => "◧",
            Effect::Engine => "Ⅱ",
            Effect::Block => "⊘",
            Effect::Window => "▣",
            Effect::Clipboard => "⎘",
            Effect::Notify => "✉",
        }
    }

    /// The effect a step belongs to (`delay` and steps of later versions have none).
    pub fn of(step: &Action) -> Option<Effect> {
        Some(match step {
            Action::Remap(_) => Effect::Remap,
            Action::Block(_) => Effect::Block,
            Action::SendKeys(_) => Effect::SendKeys,
            Action::TypeText(_) => Effect::TypeText,
            Action::OpenProgram(_) => Effect::OpenProgram,
            Action::OpenUrl(_) => Effect::OpenUrl,
            Action::MouseMove(_) | Action::MouseScroll(_) | Action::MouseClick(_) => Effect::Mouse,
            Action::Mode(_) => Effect::Mode,
            Action::Engine(_) => Effect::Engine,
            Action::Window(_) => Effect::Window,
            Action::Clipboard(_) => Effect::Clipboard,
            Action::Notify(_) => Effect::Notify,
            Action::Delay(_) | Action::Tool(_) | Action::Future { .. } => return None,
        })
    }

    /// Whether a typed abbreviation (hotstring) can trigger it (§3.4: typing steps only).
    pub fn allows_hotstring(self) -> bool {
        matches!(self, Effect::TypeText | Effect::SendKeys)
    }
}

/// Where the four-step panel is (GUI_JOURNEYS J2).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    #[default]
    Keys,
    Effect,
    Details,
    Try,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Keys, Stage::Effect, Stage::Details, Stage::Try];

    pub fn key(self) -> &'static str {
        match self {
            Stage::Keys => "form.step.keys",
            Stage::Effect => "form.step.effect",
            Stage::Details => "form.step.details",
            Stage::Try => "form.step.try",
        }
    }
}

/// The trigger fields.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TriggerFields {
    /// *When I type* an abbreviation instead of *when I press* keys.
    pub hotstring: bool,
    pub keys: String,
    pub on: TriggerOn,
    pub hold_ms: u32,
    pub pass_through: bool,
    pub abbreviation: String,
    /// Step 79: *On a schedule* instead of keys.
    pub scheduled: bool,
    /// Every this many minutes (`None`: at a time).
    pub every: Option<u32>,
    /// `HH:MM` and the weekdays (Monday first) for *at a time*.
    pub at: String,
    pub days: [bool; 7],
}

impl Default for TriggerFields {
    fn default() -> TriggerFields {
        TriggerFields {
            hotstring: false,
            keys: String::new(),
            on: TriggerOn::Press,
            hold_ms: 500,
            pass_through: false,
            abbreviation: String::new(),
            scheduled: false,
            every: Some(60),
            at: "09:00".into(),
            days: [true; 7],
        }
    }
}

/// "Act as another key".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RemapFields {
    pub to: String,
    pub tap: String,
    pub tap_ms: u32,
}

impl Default for RemapFields {
    fn default() -> RemapFields {
        RemapFields {
            to: String::new(),
            tap: String::new(),
            tap_ms: 200,
        }
    }
}

/// "Press a shortcut".
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct KeysFields {
    pub keys: String,
    pub step_delay_ms: u32,
}

/// "Type text".
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TextFields {
    pub text: String,
    pub variables: bool,
    pub paste: bool,
    pub key_delay_ms: u32,
}

/// "Open a program".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProgramFields {
    pub path: String,
    pub app_id: String,
    /// One argument per line.
    pub args: String,
    pub working_dir: String,
    pub window: String,
    pub activate: bool,
}

impl Default for ProgramFields {
    fn default() -> ProgramFields {
        ProgramFields {
            path: String::new(),
            app_id: String::new(),
            args: String::new(),
            working_dir: String::new(),
            window: "normal".into(),
            activate: false,
        }
    }
}

/// Step 79: "Window".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WindowFields {
    pub op: ddalgak::model::WindowOp,
    /// A program's file name or full path (empty: the active window).
    pub app: String,
    pub x: String,
    pub y: String,
    pub width: String,
    pub height: String,
    pub on: bool,
}

impl Default for WindowFields {
    fn default() -> WindowFields {
        WindowFields {
            op: ddalgak::model::WindowOp::Maximize,
            app: String::new(),
            x: String::new(),
            y: String::new(),
            width: String::new(),
            height: String::new(),
            on: true,
        }
    }
}

/// Step 79: "Clipboard".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClipFields {
    pub op: ddalgak::model::ClipboardOp,
    pub text: String,
    pub to: ddalgak::model::ClipTransform,
}

impl Default for ClipFields {
    fn default() -> ClipFields {
        ClipFields {
            op: ddalgak::model::ClipboardOp::Set,
            text: String::new(),
            to: ddalgak::model::ClipTransform::Plain,
        }
    }
}

/// Step 79: "Notification".
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NotifyFields {
    pub title: String,
    pub body: String,
}

/// "Switch mode".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModeFields {
    pub op: ModeOp,
    pub mode: String,
    pub modes: Vec<String>,
}

impl Default for ModeFields {
    fn default() -> ModeFields {
        ModeFields {
            op: ModeOp::Set,
            mode: String::new(),
            modes: Vec::new(),
        }
    }
}

/// Every value the form edits. `PartialEq` decides whether the form has unsaved changes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fields {
    pub title: String,
    /// The title follows the effect's summary until the user types one.
    pub title_auto: bool,
    pub description: String,
    pub enabled: bool,
    pub trigger: TriggerFields,
    pub effect: Option<Effect>,
    pub remap: RemapFields,
    pub keys: KeysFields,
    pub text: TextFields,
    pub program: ProgramFields,
    pub url: String,
    pub mouse: MouseAction,
    pub mode: ModeFields,
    pub engine: EngineOp,
    /// Step 79.
    pub window: WindowFields,
    pub clip: ClipFields,
    pub notify: NotifyFields,
    /// Only in these modes (empty: every mode).
    pub modes: Vec<String>,
    /// Only in these apps (file names or full paths, one per line; Step 77).
    pub apps: String,
    /// Step 77: not in these apps (same form).
    pub except_apps: String,
    /// Step 77: only when the window title contains (or, with `title_exact`, is) this text.
    pub window_title: String,
    pub title_exact: bool,
    pub priority: i64,
    /// Step 78 (ADR 0079): a macro's steps in order (empty: the rule is the one effect above).
    pub macro_steps: Vec<Value>,
    /// Where *Add to macro* puts the step being edited (after *Edit* took it out).
    pub macro_at: Option<usize>,
    /// The steps run this many times (1–100).
    pub times: u32,
    /// `on_error: continue`.
    pub continue_on_error: bool,
}

impl Default for Fields {
    fn default() -> Fields {
        Fields {
            title: String::new(),
            title_auto: true,
            description: String::new(),
            enabled: true,
            trigger: TriggerFields::default(),
            effect: None,
            remap: RemapFields::default(),
            keys: KeysFields::default(),
            text: TextFields::default(),
            program: ProgramFields::default(),
            url: String::new(),
            mouse: MouseAction::MoveBy {
                dx: 0,
                dy: 0,
                grid: None,
                hold: HoldChoice::Once,
            },
            mode: ModeFields::default(),
            engine: EngineOp::Pause,
            window: WindowFields::default(),
            clip: ClipFields::default(),
            notify: NotifyFields::default(),
            modes: Vec::new(),
            apps: String::new(),
            except_apps: String::new(),
            window_title: String::new(),
            title_exact: false,
            priority: 0,
            macro_steps: Vec::new(),
            macro_at: None,
            times: 1,
            continue_on_error: false,
        }
    }
}

/// One rule being created or edited.
#[derive(Clone, Debug)]
pub struct RuleForm {
    /// `None`: a new rule.
    pub id: Option<String>,
    pub profile: String,
    /// The rule as it was when the form opened (fields the form does not show come from it).
    pub base: Option<Rule>,
    pub fields: Fields,
    /// The fields as opened (unsaved changes = `fields != pristine`).
    pub pristine: Fields,
    pub stage: Stage,
    /// Why this rule cannot be edited in the form (several steps, a step of a later version, a
    /// rule that does not fit the types); the detail view says so and offers the CLI.
    pub unsupported: Option<String>,
}

/// What the simulate mode says a rule would do (GUI_JOURNEYS J2 step 4, J5). Nothing was done.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Simulation {
    pub fires: bool,
    pub lines: Vec<String>,
    /// The text as it would be typed now, shown in the editor's own field only.
    pub typed: Option<String>,
    /// The simulated pointer's start and end (physical pixels), for a mouse rule.
    pub pointer: Option<(Point, Point)>,
}

/// The app matchers of a condition as lines: file names and full paths as stored.
fn app_lines(list: Option<&Vec<AppMatcher>>) -> String {
    list.map(|a| {
        a.iter()
            .filter_map(|m| m.exe.clone().or_else(|| m.path.clone()))
            .collect::<Vec<_>>()
            .join("\n")
    })
    .unwrap_or_default()
}

/// A line is a full path when it has a folder (`\` or a drive); otherwise a file name.
fn is_path_line(a: &str) -> bool {
    a.contains('\\') || a.contains(':') || a.starts_with('%')
}

fn app_line_ok(a: &str) -> bool {
    a.chars().count() <= 260
        && a.to_lowercase().ends_with(".exe")
        && !a.contains(['"', '*', '?', '<', '>', '|'])
        && (is_path_line(a) || !a.contains(['/', ':']))
}

/// The lines of an apps field as matchers (`None` when empty).
fn matchers(text: &str) -> Option<Vec<AppMatcher>> {
    let list = lines(text);
    (!list.is_empty()).then(|| {
        list.into_iter()
            .map(|a| {
                let path = is_path_line(&a);
                AppMatcher {
                    exe: (!path).then(|| a.clone()),
                    path: path.then_some(a),
                    extra: Map::new(),
                }
            })
            .collect()
    })
}

fn problem(path: &'static str, message: impl Into<String>) -> FieldProblem {
    FieldProblem {
        path,
        message: message.into(),
    }
}

/// Lines of a multi-line field, trimmed, empty lines left out.
pub fn lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// The words for a permission (the consent dialog and the permission chips).
pub fn permission_key(p: Permission) -> String {
    format!("perm.{}", p.as_str())
}

fn engine_op_str(op: EngineOp) -> &'static str {
    match op {
        EngineOp::Pause => "pause",
        EngineOp::Resume => "resume",
        EngineOp::TogglePause => "toggle_pause",
        EngineOp::Disable => "disable",
    }
}

/// The engine step's sentence ("Pauses all rules").
pub fn engine_summary(op: EngineOp) -> &'static str {
    match op {
        EngineOp::Pause => "Pauses all rules",
        EngineOp::Resume => "Resumes all rules",
        EngineOp::TogglePause => "Pauses all rules, or resumes them when paused",
        EngineOp::Disable => "Turns Ddalgak off (only you can turn it on again)",
    }
}

fn mode_op_str(op: ModeOp) -> &'static str {
    match op {
        ModeOp::Set => "set",
        ModeOp::Toggle => "toggle",
        ModeOp::Cycle => "cycle",
        ModeOp::Hold => "hold",
        ModeOp::Previous => "previous",
        ModeOp::Default => "default",
    }
}

/// The send-keys sentence ("Presses Ctrl+K, then Ctrl+V").
pub fn send_keys_summary(keys: &str) -> String {
    let chords: Vec<&str> = keys.split_whitespace().collect();
    match chords.as_slice() {
        [] => "Presses nothing yet".into(),
        [one] => format!("Presses {one}"),
        [first @ .., last] => format!("Presses {}, then {last}", first.join(", then ")),
    }
}

fn point_of(v: &Value) -> Option<PointSpec> {
    Some(PointSpec {
        x: i32::try_from(v["x"].as_i64()?).ok()?,
        y: i32::try_from(v["y"].as_i64()?).ok()?,
        anchor: v["relative_to"]
            .as_str()
            .map_or(Some(Anchor::VirtualScreen), Anchor::parse)?,
    })
}

/// The mouse form's value of a stored mouse step (`None` when it uses something the form cannot
/// show).
pub fn mouse_action_of(s: &Value) -> Option<MouseAction> {
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d);
    Some(match s["type"].as_str()? {
        "mouse_move" if s["mode"] == "absolute" => MouseAction::MoveTo(point_of(&s["to"])?),
        "mouse_move" => {
            let grid = if s["unit"] == "grid" {
                Some(Grid {
                    columns: u32::try_from(s["grid"]["columns"].as_u64()?).ok()?,
                    rows: u32::try_from(s["grid"]["rows"].as_u64()?).ok()?,
                })
            } else {
                None
            };
            let w = &s["while_held"];
            let hold = if w.is_null() {
                HoldChoice::Once
            } else if let Some(speed) = w["speed"].as_u64() {
                HoldChoice::Continuous {
                    speed: u32::try_from(speed).ok()?,
                    acceleration: u32::try_from(w["acceleration"].as_u64().unwrap_or(0)).ok()?,
                }
            } else {
                HoldChoice::Repeat {
                    repeat_ms: u32::try_from(w["repeat_ms"].as_u64()?).ok()?,
                    delay_ms: u32::try_from(
                        w["delay_ms"]
                            .as_u64()
                            .unwrap_or(u64::from(mouse::DEFAULT_DELAY_MS)),
                    )
                    .ok()?,
                }
            };
            MouseAction::MoveBy {
                dx: i32::try_from(int("dx", 0)).ok()?,
                dy: i32::try_from(int("dy", 0)).ok()?,
                grid,
                hold,
            }
        }
        "mouse_scroll" => MouseAction::Scroll {
            dir: ScrollDir::parse(s["direction"].as_str()?)?,
            notches: u32::try_from(int("notches", 1)).ok()?,
        },
        "mouse_click" => MouseAction::Click {
            button: mouse::button(s["button"].as_str().unwrap_or("left"))?,
            op: match s["op"].as_str().unwrap_or("click") {
                "click" => "click",
                "double_click" => "double_click",
                "hold" => "hold",
                _ => return None,
            },
            at: if s["at"].is_null() {
                None
            } else {
                Some(point_of(&s["at"])?)
            },
        },
        _ => return None,
    })
}

/// The default mouse action for a segment of the Mouse form (Move by, Move to, Scroll, Click).
pub fn mouse_default(kind: usize) -> MouseAction {
    match kind {
        1 => MouseAction::MoveTo(PointSpec {
            x: 0,
            y: 0,
            anchor: Anchor::PrimaryMonitor,
        }),
        2 => MouseAction::Scroll {
            dir: ScrollDir::Down,
            notches: 1,
        },
        3 => MouseAction::Click {
            button: Button::Left,
            op: "click",
            at: None,
        },
        _ => MouseAction::MoveBy {
            dx: 0,
            dy: 0,
            grid: None,
            hold: HoldChoice::Once,
        },
    }
}

/// The segment of a mouse action (0 move by, 1 move to, 2 scroll, 3 click).
pub fn mouse_kind(a: &MouseAction) -> usize {
    match a {
        MouseAction::MoveBy { .. } => 0,
        MouseAction::MoveTo(_) => 1,
        MouseAction::Scroll { .. } => 2,
        MouseAction::Click { .. } => 3,
    }
}

impl RuleForm {
    /// A new, empty rule in `profile` (J2).
    pub fn new(profile: &str) -> RuleForm {
        let fields = Fields::default();
        RuleForm {
            id: None,
            profile: profile.to_string(),
            base: None,
            pristine: fields.clone(),
            fields,
            stage: Stage::Keys,
            unsupported: None,
        }
    }

    /// A new rule from a starter suggestion (J1): pre-filled, still a draft until saved.
    pub fn starter(profile: &str, effect: Effect) -> RuleForm {
        let mut f = RuleForm::new(profile);
        f.fields.effect = Some(effect);
        match effect {
            Effect::Remap => {
                f.fields.trigger.keys = "CapsLock".into();
                f.fields.remap.to = "LCtrl".into();
                f.fields.remap.tap = "Escape".into();
            }
            Effect::TypeText => {
                f.fields.trigger.keys = "Ctrl+Shift+F2".into();
                f.fields.text.text = "감사합니다.".into();
            }
            Effect::OpenProgram => {
                f.fields.trigger.keys = "Ctrl+Alt+Shift+F14".into();
                f.fields.program.path = r"%SystemRoot%\System32\notepad.exe".into();
            }
            _ => {}
        }
        f.stage = Stage::Details;
        f.refresh_title(None);
        f
    }

    /// The form for an existing rule (J7 "Edit opens the same panel at step 3").
    pub fn edit(profile: &str, rule: &Rule) -> RuleForm {
        let mut f = Fields {
            title: rule.title.clone(),
            title_auto: false,
            description: rule.description.clone().unwrap_or_default(),
            enabled: rule.enabled,
            modes: rule.modes.clone().unwrap_or_default(),
            apps: app_lines(rule.when.as_ref().and_then(|w| w.apps.as_ref())),
            except_apps: app_lines(rule.when.as_ref().and_then(|w| w.except_apps.as_ref())),
            window_title: rule
                .when
                .as_ref()
                .and_then(|w| w.window_title.as_ref())
                .and_then(|t| t.contains.clone().or_else(|| t.equals.clone()))
                .unwrap_or_default(),
            title_exact: rule
                .when
                .as_ref()
                .and_then(|w| w.window_title.as_ref())
                .is_some_and(|t| t.equals.is_some()),
            priority: rule.priority(),
            ..Fields::default()
        };
        let t = &rule.trigger;
        if let Some(s) = &t.schedule {
            f.trigger.scheduled = true;
            f.trigger.every = s.every_minutes;
            f.trigger.at = s.at.clone().unwrap_or_else(|| "09:00".into());
            if let Some(d) = &s.days {
                let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
                f.trigger.days = names.map(|n| d.iter().any(|x| x == n));
            }
        } else if let Some(h) = &t.hotstring {
            f.trigger.hotstring = true;
            f.trigger.abbreviation = h.abbreviation.clone();
        } else {
            f.trigger.keys = t.keys_text().to_string();
            f.trigger.on = t.on();
            f.trigger.hold_ms = t.hold_ms.unwrap_or(500);
            f.trigger.pass_through = t.pass_through == Some(true);
        }
        let mut unsupported = None;
        f.times = rule.times.unwrap_or(1);
        f.continue_on_error = rule.on_error == Some(ddalgak::model::OnError::Continue);
        let is_macro = rule.steps.len() > 1
            || f.times > 1
            || rule.steps.iter().any(|s| matches!(s, Action::Delay(_)));
        match rule.steps.as_slice() {
            _ if is_macro => {
                if let Some(s) = rule
                    .steps
                    .iter()
                    .find(|s| matches!(s, Action::Future { .. }))
                {
                    unsupported = Some(format!("its step `{}` has no form yet", s.kind()));
                }
                f.macro_steps = rule.steps.iter().map(Action::to_value).collect();
            }
            [step] => {
                f.effect = Effect::of(step);
                let v = step.to_value();
                match step {
                    Action::Remap(r) => {
                        f.remap = RemapFields {
                            to: r.to.clone(),
                            tap: r.tap.clone().unwrap_or_default(),
                            tap_ms: r.tap_ms.unwrap_or(200),
                        };
                    }
                    Action::SendKeys(k) => {
                        f.keys = KeysFields {
                            keys: k.keys.clone(),
                            step_delay_ms: k.step_delay_ms.unwrap_or(0),
                        };
                    }
                    Action::TypeText(t) => {
                        f.text = TextFields {
                            text: t.text.clone(),
                            variables: t.variables == Some(true),
                            paste: v["method"] == "paste",
                            key_delay_ms: t.key_delay_ms.unwrap_or(0),
                        };
                    }
                    Action::OpenProgram(p) => {
                        f.program = ProgramFields {
                            path: p.path.clone().unwrap_or_default(),
                            app_id: p.app_id.clone().unwrap_or_default(),
                            args: p.args.clone().unwrap_or_default().join("\n"),
                            working_dir: p.working_dir.clone().unwrap_or_default(),
                            window: v["window"].as_str().unwrap_or("normal").to_string(),
                            activate: v["if_running"] == "activate",
                        };
                    }
                    Action::OpenUrl(u) => f.url = u.url.clone(),
                    Action::MouseMove(_) | Action::MouseScroll(_) | Action::MouseClick(_) => {
                        match mouse_action_of(&v) {
                            Some(a) => f.mouse = a,
                            None => {
                                unsupported = Some(
                                    "its mouse step uses options the form cannot show".to_string(),
                                );
                            }
                        }
                    }
                    Action::Mode(m) => {
                        f.mode = ModeFields {
                            op: m.op,
                            mode: m.mode.clone().unwrap_or_default(),
                            modes: m.modes.clone().unwrap_or_default(),
                        };
                    }
                    Action::Engine(e) => f.engine = e.op,
                    Action::Block(_) => {}
                    Action::Window(w) => {
                        let n = |v: Option<i32>| v.map(|v| v.to_string()).unwrap_or_default();
                        let u = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
                        f.window = WindowFields {
                            op: w.op,
                            app: w
                                .app
                                .as_ref()
                                .and_then(|m| m.exe.clone().or_else(|| m.path.clone()))
                                .unwrap_or_default(),
                            x: n(w.x),
                            y: n(w.y),
                            width: u(w.width),
                            height: u(w.height),
                            on: w.on != Some(false),
                        };
                    }
                    Action::Clipboard(c) => {
                        f.clip = ClipFields {
                            op: c.op,
                            text: c.text.clone().unwrap_or_default(),
                            to: c.to.unwrap_or(ddalgak::model::ClipTransform::Plain),
                        };
                    }
                    Action::Notify(n) => {
                        f.notify = NotifyFields {
                            title: n.title.clone(),
                            body: n.body.clone().unwrap_or_default(),
                        };
                    }
                    Action::Delay(_) | Action::Tool(_) | Action::Future { .. } => {
                        unsupported = Some(format!("its step `{}` has no form yet", step.kind()));
                    }
                }
            }
            _ => {
                unsupported = Some("it has no steps".into());
            }
        }
        if let Some(e) = &rule.when
            && (e.except_apps.is_some() || e.window_title.is_some() || !e.extra.is_empty())
            && unsupported.is_none()
        {
            // Kept as they are; the form edits `apps` only.
        }
        RuleForm {
            id: Some(rule.id.clone()),
            profile: profile.to_string(),
            base: Some(rule.clone()),
            pristine: f.clone(),
            fields: f,
            stage: Stage::Details,
            unsupported,
        }
    }

    /// Whether the form has changes that are not saved.
    pub fn dirty(&self) -> bool {
        self.fields != self.pristine
    }

    /// The trigger as the summary sentences show it.
    pub fn trigger_text(&self) -> String {
        let t = &self.fields.trigger;
        if t.hotstring {
            format!("Typing {}", t.abbreviation.trim())
        } else {
            canonical_trigger(&t.keys).unwrap_or_else(|_| t.keys.trim().to_string())
        }
    }

    fn title_for_drafts(&self) -> String {
        let t = self.fields.title.trim();
        if t.is_empty() {
            "untitled".into()
        } else {
            t.to_string()
        }
    }

    /// The type-text draft of Step 69.
    pub fn text_draft(&self) -> EditorDraft {
        let t = &self.fields.text;
        EditorDraft {
            title: self.title_for_drafts(),
            text: t.text.clone(),
            variables: t.variables,
            method: if t.paste { "paste" } else { "unicode" }.into(),
            key_delay_ms: t.key_delay_ms,
            saved: None,
        }
    }

    /// The program draft of Step 70 (the environment is kept from the stored rule).
    pub fn program_draft(&self) -> ProgramDraft {
        let p = &self.fields.program;
        let env = self
            .base
            .as_ref()
            .and_then(|r| match r.steps.first() {
                Some(Action::OpenProgram(o)) => o.env.clone(),
                _ => None,
            })
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.name, e.value))
            .collect();
        ProgramDraft {
            title: self.title_for_drafts(),
            path: p.path.trim().to_string(),
            app_id: p.app_id.trim().to_string(),
            args: p
                .args
                .lines()
                .map(str::to_string)
                .filter(|a| !a.is_empty())
                .collect(),
            working_dir: p.working_dir.trim().to_string(),
            window: p.window.clone(),
            activate: p.activate,
            env,
            saved: None,
        }
    }

    /// The website draft of Step 70.
    pub fn url_draft(&self) -> UrlDraft {
        UrlDraft {
            title: self.title_for_drafts(),
            input: self.fields.url.clone(),
            saved: None,
        }
    }

    /// The mouse draft of Step 71.
    pub fn mouse_draft(&self) -> MouseDraft {
        MouseDraft::new(
            &self.title_for_drafts(),
            &self.trigger_text(),
            self.fields.mouse,
        )
    }

    /// The step the effect saves as (canonical JSON), or the field problems.
    pub fn step(&self, profile: Option<&Profile>) -> Result<Value, Vec<FieldProblem>> {
        let f = &self.fields;
        let mut out = Vec::new();
        let Some(effect) = f.effect else {
            return Err(vec![problem("effect", "choose what the rule does")]);
        };
        let keep = |p: FieldProblem| p.path != "title";
        let step = match effect {
            Effect::Remap => {
                let to = canonical_output(&f.remap.to, 1)
                    .map_err(|e| problem("steps[0].to", e))
                    .map_err(|e| out.push(e))
                    .ok();
                let tap = if f.remap.tap.trim().is_empty() {
                    None
                } else {
                    canonical_output(&f.remap.tap, 1)
                        .map_err(|e| out.push(problem("steps[0].tap", e)))
                        .ok()
                };
                if !(100..=1000).contains(&f.remap.tap_ms) {
                    out.push(problem("steps[0].tap_ms", "100–1000 ms"));
                }
                if f.trigger.pass_through && !f.remap.tap.trim().is_empty() {
                    out.push(problem(
                        "trigger.pass_through",
                        "the original key cannot also go through when a quick tap sends another key",
                    ));
                }
                let mut s = json!({ "type": "remap", "to": to.unwrap_or_default() });
                if let Some(t) = tap {
                    s["tap"] = json!(t);
                    if f.remap.tap_ms != 200 {
                        s["tap_ms"] = json!(f.remap.tap_ms);
                    }
                }
                s
            }
            Effect::Block => json!({ "type": "block" }),
            Effect::SendKeys => {
                let keys = canonical_output(&f.keys.keys, 8)
                    .map_err(|e| out.push(problem("steps[0].keys", e)))
                    .unwrap_or_default();
                if f.keys.step_delay_ms > 1000 {
                    out.push(problem("steps[0].step_delay_ms", "0–1000 ms"));
                }
                let mut s = json!({ "type": "send_keys", "keys": keys });
                if f.keys.step_delay_ms > 0 {
                    s["step_delay_ms"] = json!(f.keys.step_delay_ms);
                }
                s
            }
            Effect::TypeText => {
                let d = self.text_draft();
                out.extend(d.problems().into_iter().filter(|p| keep(p.clone())));
                d.step()
            }
            Effect::OpenProgram => {
                let d = self.program_draft();
                out.extend(d.problems().into_iter().filter(|p| keep(p.clone())));
                d.step()
            }
            Effect::OpenUrl => {
                let d = self.url_draft();
                out.extend(d.problems().into_iter().filter(|p| keep(p.clone())));
                d.step()
                    .unwrap_or_else(|| json!({ "type": "open_url", "url": "" }))
            }
            Effect::Mouse => {
                let d = self.mouse_draft();
                out.extend(d.problems().into_iter().filter(|p| keep(p.clone())));
                d.step()
            }
            Effect::Mode => {
                let m = &f.mode;
                let known = |id: &str| profile.is_none_or(|p| p.modes.iter().any(|x| x.id == id));
                let mut s = json!({ "type": "mode", "op": mode_op_str(m.op) });
                match m.op {
                    ModeOp::Set | ModeOp::Toggle | ModeOp::Hold => {
                        if m.mode.is_empty() {
                            out.push(problem("steps[0].mode", "choose a mode"));
                        } else if !known(&m.mode) {
                            out.push(problem("steps[0].mode", "that mode is not in the profile"));
                        }
                        s["mode"] = json!(m.mode);
                    }
                    ModeOp::Cycle => {
                        if m.modes.len() < 2 {
                            out.push(problem("steps[0].modes", "choose at least two modes"));
                        }
                        s["modes"] = json!(m.modes);
                    }
                    ModeOp::Previous | ModeOp::Default => {}
                }
                s
            }
            Effect::Engine => json!({ "type": "engine", "op": engine_op_str(f.engine) }),
            Effect::Window => {
                use ddalgak::model::WindowOp as W;
                let w = &f.window;
                let mut s = json!({ "type": "window", "op": w.op.as_str() });
                let app = w.app.trim();
                if !app.is_empty() {
                    if !app_line_ok(app) {
                        out.push(problem(
                            "steps[0].app",
                            "a program's file name like Code.exe, or its full path",
                        ));
                    } else if is_path_line(app) {
                        s["app"] = json!({ "path": app });
                    } else {
                        s["app"] = json!({ "exe": app });
                    }
                } else if w.op == W::Activate {
                    out.push(problem(
                        "steps[0].app",
                        "name the program to bring to the front",
                    ));
                }
                if w.op == W::Move {
                    let num = |t: &str, lo: i64, hi: i64| {
                        t.trim()
                            .parse::<i64>()
                            .ok()
                            .filter(|v| (lo..=hi).contains(v))
                    };
                    let (x, y) = (num(&w.x, -32768, 32767), num(&w.y, -32768, 32767));
                    let (wd, ht) = (num(&w.width, 50, 32767), num(&w.height, 50, 32767));
                    if let (Some(x), Some(y)) = (x, y) {
                        s["x"] = json!(x);
                        s["y"] = json!(y);
                    }
                    if let (Some(a), Some(b)) = (wd, ht) {
                        s["width"] = json!(a);
                        s["height"] = json!(b);
                    }
                    if s.get("x").is_none() && s.get("width").is_none() {
                        out.push(problem(
                            "steps[0].x",
                            "a position (x and y) or a size (width and height, at least 50)",
                        ));
                    }
                }
                if w.op == W::Topmost {
                    s["on"] = json!(w.on);
                }
                s
            }
            Effect::Clipboard => {
                use ddalgak::model::ClipboardOp as C;
                let c = &f.clip;
                let mut s = json!({ "type": "clipboard", "op": c.op.as_str() });
                match c.op {
                    C::Set => {
                        let n = c.text.chars().count();
                        if !(1..=20_000).contains(&n) {
                            out.push(problem("steps[0].text", "1–20000 characters"));
                        }
                        s["text"] = json!(c.text);
                    }
                    C::Transform => s["to"] = json!(c.to.as_str()),
                    C::Clear => {}
                }
                s
            }
            Effect::Notify => {
                let n = &f.notify;
                let t = n.title.trim().chars().count();
                if !(1..=63).contains(&t) {
                    out.push(problem("steps[0].title", "a title of 1–63 characters"));
                }
                if n.body.chars().count() > 255 {
                    out.push(problem("steps[0].body", "at most 255 characters"));
                }
                let mut s = json!({ "type": "notify", "title": n.title.trim() });
                if !n.body.trim().is_empty() {
                    s["body"] = json!(n.body.trim());
                }
                s
            }
        };
        if out.is_empty() { Ok(step) } else { Err(out) }
    }

    /// Every field problem, in display order (nothing is written while there is one).
    pub fn problems(&self, profile: Option<&Profile>) -> Vec<FieldProblem> {
        let f = &self.fields;
        let mut out = Vec::new();
        if self.unsupported.is_some() {
            out.push(problem("", "this rule cannot be edited in the form"));
            return out;
        }
        let title = f.title.trim().chars().count();
        if !(1..=80).contains(&title) {
            out.push(problem("title", "the name needs 1–80 characters"));
        }
        if f.description.chars().count() > 500 {
            out.push(problem("description", "at most 500 characters"));
        }
        let t = &f.trigger;
        if t.scheduled {
            // Step 79: the schedule's own checks (the engine's, from the saved form).
            let sched = match t.every {
                Some(n) => serde_json::json!({ "every_minutes": n }),
                None => serde_json::json!({ "at": t.at.trim() }),
            };
            if let Err(e) = ddalgak::actions::ScheduleSpec::compile(&sched) {
                out.push(problem("trigger.schedule", e));
            }
            if t.every.is_none() && !t.days.iter().any(|d| *d) {
                out.push(problem("trigger.schedule", "choose at least one day"));
            }
        } else if t.hotstring {
            let a = t.abbreviation.trim();
            let n = a.chars().count();
            if !(1..=40).contains(&n) || a.chars().any(char::is_whitespace) {
                out.push(problem(
                    "trigger.hotstring.abbreviation",
                    "1–40 characters without spaces (btw, ;sig)",
                ));
            }
            if f.effect.is_some_and(|e| !e.allows_hotstring()) {
                out.push(problem(
                    "trigger.hotstring.abbreviation",
                    "an abbreviation can only type text or press a shortcut",
                ));
            }
        } else {
            if t.keys.trim().is_empty() {
                out.push(problem("trigger.keys", "press the keys, or type them"));
            } else if let Err(e) = canonical_trigger(&t.keys) {
                out.push(problem("trigger.keys", e.to_string()));
            }
            if t.on == TriggerOn::Hold && !(200..=5000).contains(&t.hold_ms) {
                out.push(problem("trigger.hold_ms", "200–5000 ms"));
            }
        }
        if self.is_macro() {
            // Step 78: the engine's own check of the macro (every step as it would run).
            let set = profile
                .map(ModeSet::from_profile)
                .unwrap_or_else(ModeSet::single);
            if let Err(e) = ddalgak::rules::check_macro(&f.macro_steps, u64::from(f.times), &set) {
                out.push(problem("steps", e));
            }
            if t.hotstring {
                out.push(problem(
                    "trigger.hotstring.abbreviation",
                    "an abbreviation runs one step, not a macro",
                ));
            }
        } else if let Err(p) = self.step(profile) {
            out.extend(p);
        }
        if let Some(p) = profile {
            for m in &f.modes {
                if !p.modes.iter().any(|x| &x.id == m) {
                    out.push(problem("modes", format!("no mode `{m}` in the profile")));
                }
            }
        }
        for (field, text) in [("when.apps", &f.apps), ("when.except_apps", &f.except_apps)] {
            let list = lines(text);
            if list.len() > 32 {
                out.push(problem(field, "at most 32 programs"));
            }
            if list.iter().any(|a| !app_line_ok(a)) {
                out.push(problem(
                    field,
                    "a program's file name like Code.exe, or its full path (one per line)",
                ));
            }
        }
        if f.window_title.trim().chars().count() > 200 {
            out.push(problem("when.window_title", "at most 200 characters"));
        }
        if !(-1000..=1000).contains(&f.priority) {
            out.push(problem("priority", "−1000…1000"));
        }
        out
    }

    /// The rule the form saves as; `id` is used for a new rule. Consent is not granted here.
    pub fn build(&self, id: &str, profile: Option<&Profile>) -> Result<Rule, Vec<FieldProblem>> {
        let problems = self.problems(profile);
        if !problems.is_empty() {
            return Err(problems);
        }
        let f = &self.fields;
        let values = if self.is_macro() {
            f.macro_steps.clone()
        } else {
            vec![self.step(profile)?]
        };
        let actions = values
            .into_iter()
            .map(serde_json::from_value::<Action>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                vec![problem(
                    "",
                    format!("the step does not fit the rule format: {e}"),
                )]
            })?;
        let mut rule = self.base.clone().unwrap_or_else(|| {
            Rule::new(String::new(), String::new(), Trigger::keys(""), Vec::new())
        });
        if self.base.is_none() {
            rule.id = id.to_string();
        }
        rule.title = f.title.trim().to_string();
        rule.description = Some(f.description.trim().to_string()).filter(|d| !d.is_empty());
        rule.enabled = f.enabled;
        let base_trigger = self.base.as_ref().map(|r| r.trigger.clone());
        rule.trigger = if f.trigger.scheduled {
            // Step 79: a schedule replaces the keys.
            let tf = &f.trigger;
            let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
            let days: Vec<String> = names
                .iter()
                .zip(tf.days)
                .filter(|(_, on)| *on)
                .map(|(n, _)| (*n).to_string())
                .collect();
            let mut t = Trigger::keys("");
            t.keys = None;
            t.schedule = Some(ddalgak::model::Schedule {
                every_minutes: tf.every,
                at: tf.every.is_none().then(|| tf.at.trim().to_string()),
                days: (tf.every.is_none() && days.len() < 7).then_some(days),
                extra: Map::new(),
            });
            t
        } else if f.trigger.hotstring {
            let mut t = Trigger::hotstring(f.trigger.abbreviation.trim());
            if let Some(b) = &base_trigger {
                if let Some(h) = &b.hotstring {
                    let mut h = h.clone();
                    h.abbreviation = f.trigger.abbreviation.trim().to_string();
                    t.hotstring = Some(h);
                }
                t.injected = b.injected;
            }
            t
        } else {
            let keys = canonical_trigger(&f.trigger.keys)
                .map_err(|e| vec![problem("trigger.keys", e.to_string())])?;
            let mut t = Trigger::keys(keys);
            t.on = (f.trigger.on != TriggerOn::Press).then_some(f.trigger.on);
            t.hold_ms = (f.trigger.on == TriggerOn::Hold).then_some(f.trigger.hold_ms);
            t.pass_through = f.trigger.pass_through.then_some(true);
            if let Some(b) = &base_trigger
                && b.hotstring.is_none()
            {
                t.injected = b.injected;
                t.repeat = b.repeat.filter(|_| f.trigger.on == TriggerOn::Press);
                t.extra = b.extra.clone();
            }
            t
        };
        rule.steps = actions;
        rule.times = (self.is_macro() && f.times > 1).then_some(f.times);
        rule.on_error = (self.is_macro() && f.continue_on_error)
            .then_some(ddalgak::model::OnError::Continue)
            .or(rule.on_error.filter(|_| !self.is_macro()));
        rule.modes = (!f.modes.is_empty()).then(|| f.modes.clone());
        let mut when = rule.when.clone().unwrap_or_default();
        when.apps = matchers(&f.apps);
        when.except_apps = matchers(&f.except_apps);
        let title = f.window_title.trim();
        when.window_title = (!title.is_empty()).then(|| TitleMatch {
            contains: (!f.title_exact).then(|| title.to_string()),
            equals: f.title_exact.then(|| title.to_string()),
            extra: Map::new(),
        });
        rule.when = (when != Condition::default()).then_some(when);
        rule.priority = (f.priority != 0).then_some(f.priority);
        Ok(rule)
    }

    /// Sets the title from the summary while the user has not typed one.
    pub fn refresh_title(&mut self, profile: Option<&Profile>) {
        if !self.fields.title_auto {
            return;
        }
        let s = match self.fields.effect {
            Some(Effect::TypeText) => "Type a phrase".to_string(),
            Some(Effect::OpenProgram) => {
                let d = self.program_draft();
                let p = d.preview(&ddalgak::launch::process_var);
                if p.program.is_empty() {
                    "Open a program".into()
                } else {
                    format!("Open {}", p.program)
                }
            }
            Some(_) => self.summary(profile),
            None => String::new(),
        };
        self.fields.title = s.chars().take(80).collect();
    }

    /// The live summary sentence (J4): "CapsLock acts as Left Ctrl; a quick tap is Escape".
    pub fn summary(&self, profile: Option<&Profile>) -> String {
        let f = &self.fields;
        let trigger = self.trigger_text();
        if self.is_macro() {
            let n = f.macro_steps.len();
            let times = if f.times > 1 {
                format!(", {} times", f.times)
            } else {
                String::new()
            };
            return format!(
                "Runs {n} step{} in order{times}",
                if n == 1 { "" } else { "s" }
            );
        }
        match f.effect {
            None => String::new(),
            Some(Effect::Remap) => {
                let to = canonical_output(&f.remap.to, 1).unwrap_or_else(|_| f.remap.to.clone());
                let tap = canonical_output(&f.remap.tap, 1).ok();
                ddalgak::remap::summary(&trigger, &to, tap.as_deref(), f.trigger.pass_through)
            }
            Some(Effect::Block) => ddalgak::remap::block_summary(&trigger),
            Some(Effect::SendKeys) => send_keys_summary(
                &canonical_output(&f.keys.keys, 8).unwrap_or_else(|_| f.keys.keys.clone()),
            ),
            Some(Effect::TypeText) => {
                let d = self.text_draft();
                ddalgak::text::summary(&d.text, d.variables, &d.method)
            }
            Some(Effect::OpenProgram) => self.program_draft().summary(),
            Some(Effect::OpenUrl) => self.url_draft().summary(),
            Some(Effect::Mouse) => self.mouse_draft().summary(),
            Some(Effect::Mode) => {
                let (Some(p), Ok(step)) = (profile, self.step(profile)) else {
                    return "Switches the mode".into();
                };
                let set = ModeSet::from_profile(p);
                ModeAction::compile(&step, &set).map_or_else(
                    |e| format!("Not ready: {e}"),
                    |a| ddalgak::modes::summary(&set, &a, &trigger),
                )
            }
            Some(Effect::Engine) => engine_summary(f.engine).into(),
            Some(Effect::Window) => {
                let target = if f.window.app.trim().is_empty() {
                    "the active window".to_string()
                } else {
                    ddalgak::launch::program_name(f.window.app.trim()).to_string()
                };
                format!(
                    "{} {target}",
                    match f.window.op.as_str() {
                        "activate" => "Brings to the front:",
                        "minimize" => "Minimizes",
                        "maximize" => "Maximizes",
                        "restore" => "Restores",
                        "move" => "Moves",
                        _ => "Keeps on top:",
                    }
                )
            }
            Some(Effect::Clipboard) => match f.clip.op.as_str() {
                "set" => format!(
                    "Puts {} characters on the clipboard",
                    f.clip.text.chars().count()
                ),
                "clear" => "Empties the clipboard".into(),
                _ => format!("Changes the clipboard text ({})", f.clip.to.as_str()),
            },
            Some(Effect::Notify) => "Shows a notification".into(),
        }
    }

    /// Step 78: the rule is a macro (its steps are [`Fields::macro_steps`]).
    pub fn is_macro(&self) -> bool {
        !self.fields.macro_steps.is_empty()
    }

    /// *Add to macro*: the effect being edited becomes the next step (or goes back where *Edit*
    /// took it from); the effect fields are free for the next step.
    pub fn add_to_macro(&mut self, profile: Option<&Profile>) -> Result<(), Vec<FieldProblem>> {
        let step = self.step(profile)?;
        let at = self
            .fields
            .macro_at
            .take()
            .map_or(self.fields.macro_steps.len(), |i| {
                i.min(self.fields.macro_steps.len())
            });
        self.fields.macro_steps.insert(at, step);
        self.fields.effect = None;
        Ok(())
    }

    /// Adds a wait step.
    pub fn add_delay(&mut self, ms: u32) {
        self.fields
            .macro_steps
            .push(serde_json::json!({ "type": "delay", "ms": ms.clamp(1, 10_000) }));
    }

    /// Moves step `i` one place up or down.
    pub fn move_step(&mut self, i: usize, up: bool) {
        let n = self.fields.macro_steps.len();
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < n)
        };
        if let Some(j) = j.filter(|_| i < n) {
            self.fields.macro_steps.swap(i, j);
        }
    }

    pub fn remove_step(&mut self, i: usize) {
        if i < self.fields.macro_steps.len() {
            self.fields.macro_steps.remove(i);
        }
    }

    /// *Edit* step `i`: it goes into the effect form; *Add to macro* puts it back at `i`.
    /// A delay is edited in place in the list and is not taken out.
    pub fn edit_step(&mut self, i: usize) {
        let Some(v) = self.fields.macro_steps.get(i).cloned() else {
            return;
        };
        let Ok(action) = serde_json::from_value::<Action>(v) else {
            return;
        };
        if matches!(action, Action::Delay(_) | Action::Future { .. }) {
            return;
        }
        let tmp = Rule::new(
            "step".into(),
            "step".into(),
            Trigger::keys("F13"),
            vec![action],
        );
        let one = RuleForm::edit(&self.profile, &tmp);
        if one.unsupported.is_some() {
            return;
        }
        let o = one.fields;
        let f = &mut self.fields;
        f.effect = o.effect;
        f.remap = o.remap;
        f.keys = o.keys;
        f.text = o.text;
        f.program = o.program;
        f.url = o.url;
        f.mouse = o.mouse;
        f.mode = o.mode;
        f.engine = o.engine;
        f.macro_steps.remove(i);
        f.macro_at = Some(i);
    }

    /// One sentence per macro step (DDALGAK-SEC-011: text as counts, URLs as hosts).
    pub fn step_summaries(&self, profile: Option<&Profile>) -> Vec<String> {
        self.fields
            .macro_steps
            .iter()
            .map(|v| {
                if v["type"] == "delay" {
                    return format!("Waits {} ms", v["ms"]);
                }
                let Ok(action) = serde_json::from_value::<Action>(v.clone()) else {
                    return format!("`{}` step", v["type"].as_str().unwrap_or("?"));
                };
                let tmp = Rule::new(
                    "step".into(),
                    "step".into(),
                    Trigger::keys("F13"),
                    vec![action],
                );
                let one = RuleForm::edit(&self.profile, &tmp);
                if one.unsupported.is_some() {
                    return format!("`{}` step", v["type"].as_str().unwrap_or("?"));
                }
                one.summary(profile)
            })
            .collect()
    }

    /// The permissions the effect needs (§6.1).
    pub fn permissions(&self, profile: Option<&Profile>) -> Vec<Permission> {
        self.build("draft", profile)
            .map(|r| r.permissions_needed())
            .unwrap_or_default()
    }

    /// What the rule would do, in simulate mode (nothing is sent, typed, opened or moved).
    pub fn simulate(
        &self,
        profile: Option<&Profile>,
        layout: &Layout,
        cursor: Point,
    ) -> Simulation {
        let problems = self.problems(profile);
        if let Some(p) = problems.first() {
            return Simulation {
                fires: false,
                lines: vec![format!("✕ Not ready: {}", p.message)],
                ..Simulation::default()
            };
        }
        let trigger = self.trigger_text();
        if self.is_macro() {
            // Step 78: the steps in order, as they would run (nothing is sent).
            let mut lines = vec![format!("✓ {trigger} would fire: {}", self.summary(profile))];
            for (i, s) in self.step_summaries(profile).iter().enumerate() {
                lines.push(format!("{}. {s}", i + 1));
            }
            return Simulation {
                fires: true,
                lines,
                ..Simulation::default()
            };
        }
        let mut sim = Simulation {
            fires: true,
            lines: vec![format!("✓ {trigger} would fire: {}", self.summary(profile))],
            ..Simulation::default()
        };
        let step = self.step(profile).unwrap_or(Value::Null);
        match self.fields.effect {
            Some(Effect::Remap) => {
                let p = ddalgak::remap::preview(
                    &trigger,
                    step["to"].as_str().unwrap_or_default(),
                    step["tap"].as_str(),
                    self.fields.trigger.pass_through,
                );
                sim.lines.push(format!(
                    "While held: {}",
                    p["holds"].as_str().unwrap_or("?")
                ));
                if let Some(t) = p["tap"].as_str() {
                    sim.lines.push(format!("A quick tap alone: {t}"));
                }
            }
            Some(Effect::TypeText) => {
                let d = self.text_draft();
                let now = local_now();
                let pv = d.preview(&now);
                sim.lines.push(format!(
                    "Would type {} characters on {} lines into the focused field",
                    pv.characters, pv.lines
                ));
                sim.typed = Some(pv.rendered);
            }
            Some(Effect::OpenProgram) => {
                let p = self.program_draft().preview(&ddalgak::launch::process_var);
                sim.lines.push(format!(
                    "Would open {} ({}); nothing was opened",
                    if p.program.is_empty() {
                        "?"
                    } else {
                        &p.program
                    },
                    if p.found {
                        "found"
                    } else {
                        "not found on this PC"
                    }
                ));
            }
            Some(Effect::OpenUrl) => {
                if let Some(pv) = self.url_draft().preview() {
                    sim.lines.push(format!(
                        "Would open {} in your browser; nothing was opened",
                        pv["host"].as_str().unwrap_or("?")
                    ));
                }
            }
            Some(Effect::Mouse) => {
                if let Ok(c) = mouse::compile_step(&step) {
                    let mut pg = mouse::Playground::new(layout.clone(), cursor, None);
                    let from = pg.pointer().cursor();
                    let _ = pg.press(&c);
                    let _ = pg.advance(250);
                    let to = pg.pointer().cursor();
                    let _ = pg.release();
                    sim.pointer = Some((from, to));
                    sim.lines.push(format!(
                        "The simulated pointer would go from ({}, {}) to ({}, {}); the real pointer did not move",
                        from.x, from.y, to.x, to.y
                    ));
                }
            }
            Some(Effect::Mode) => {
                if let Some(p) = profile {
                    let set = ModeSet::from_profile(p);
                    if let Ok(a) = ModeAction::compile(&step, &set) {
                        let pv = ddalgak::modes::preview(&set, &ModeState::new(&set), &a);
                        sim.lines.push(format!(
                            "The mode would change from {} to {} (simulation: the engine's mode is unchanged)",
                            pv["from"].as_str().unwrap_or("?"),
                            pv["to"].as_str().unwrap_or("?")
                        ));
                    }
                }
            }
            Some(Effect::Engine) => sim
                .lines
                .push("Simulation: the engine's state is unchanged".into()),
            Some(Effect::Window | Effect::Clipboard | Effect::Notify) => sim
                .lines
                .push("Simulation: no window, clipboard or notification was touched".into()),
            Some(Effect::SendKeys) => sim.lines.push(format!(
                "Would press {} chord(s); nothing was sent",
                step["keys"]
                    .as_str()
                    .unwrap_or_default()
                    .split_whitespace()
                    .count()
            )),
            Some(Effect::Block) => sim.lines.push("The key would reach no program".into()),
            None => {}
        }
        if !self.fields.trigger.pass_through && self.fields.effect != Some(Effect::Block) {
            sim.lines.push("Key reaches the app: no".into());
        }
        sim
    }
}

/// The local time for the text preview's variables (Windows: the system's local time).
pub fn local_now() -> ddalgak::time::LocalTime {
    #[cfg(windows)]
    {
        ddalgak::win::typing::local_time()
    }
    #[cfg(not(windows))]
    {
        ddalgak::time::LocalTime::utc(ddalgak::time::now_secs())
    }
}

/// Records consent given in the editor's dialog (`via: "gui"`, RULE_MODEL §6.3) for the
/// permissions the rule needs; a launch consent carries the fingerprint of the exact targets.
pub fn grant_gui(rule: &mut Rule, perms: &[Permission]) {
    let needed = rule.permissions_needed();
    for p in perms {
        if needed.contains(p) {
            let fingerprint = (*p == Permission::LaunchPrograms)
                .then(|| validate::launch_fingerprint(&rule.to_value()));
            rule.set_grant(
                *p,
                Some(Grant {
                    granted_at: ddalgak::time::now_rfc3339(),
                    via: "gui".into(),
                    fingerprint,
                    extra: Map::new(),
                }),
            );
        }
    }
}

/// The exact targets a launch consent names (the program path or app, the website).
pub fn launch_targets(rule: &Rule) -> Vec<String> {
    rule.steps
        .iter()
        .filter_map(|s| match s {
            Action::OpenProgram(p) => p.path.clone().or_else(|| p.app_id.clone()),
            Action::OpenUrl(u) => Some(u.url.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
