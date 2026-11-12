//! Step 79 (DDALGAK-FR-072, DDALGAK-FR-073, DDALGAK-FR-076, ADR 0080): window, clipboard,
//! notification and cross-tool steps, and schedules.
//!
//! The specs here are what the loader compiles and the worker carries out through narrow
//! platform interfaces ([`Desktop`], [`Tools`], [`crate::run::Clipboard`]), so every rule of the
//! step is testable without touching a real window, the real clipboard or another program:
//!
//! - **Windows** (`window-control` consent): the active window, or a visible window of a named
//!   program. A window of an elevated process or on the secure desktop is never touched
//!   (`skipped_elevated_target`, `skipped_secure_desktop`); no matching window is
//!   `window_not_found`. Nothing ever reads or reports a window title.
//! - **Clipboard** (`clipboard` consent): put a text (marked so clipboard history and cloud sync
//!   skip it), empty it, or change its text (plain, trimmed, upper or lower case). The content is
//!   never logged or recorded; a clipboard another program holds open is `clipboard_busy`.
//! - **Notifications**: a balloon from Ddalgak's own tray icon; nothing leaves the computer.
//! - **Tools**: `kim tools invoke <capability>` of an installed Kim, with no consent flags, so a
//!   capability that needs the user's consent is refused by Kim (`confirmation_required`). A
//!   missing Kim or provider is `tool_missing`; nothing is ever installed.
//! - **Schedules**: every N minutes from the engine start, or at a local time on chosen days,
//!   only while the engine runs. A time missed while the computer slept, the session was locked
//!   or the engine was paused or off is skipped, never caught up.

use crate::run::Clipboard;
use serde_json::Value;
use std::time::{Duration, SystemTime};

/// A compiled `window` step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSpec {
    pub op: crate::model::WindowOp,
    /// The program (lowercase file name or full path); `None`: the active window.
    pub app: Option<crate::context::AppMatch>,
    pub pos: Option<(i32, i32)>,
    pub size: Option<(u32, u32)>,
    pub on: bool,
}

/// A compiled `clipboard` step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipSpec {
    Set(String),
    Clear,
    Transform(crate::model::ClipTransform),
}

/// A compiled `notify` step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifySpec {
    pub title: String,
    pub body: String,
}

/// A compiled `tool` step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    pub capability: String,
    pub args: Value,
}

/// #102: a compiled `run_command` step (the path is expanded and absolute).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<std::path::PathBuf>,
    pub timeout_ms: u32,
}

/// Runs local commands (#102).
pub trait Commands: Send + Sync + std::fmt::Debug {
    /// Runs the program hidden and waits (at most its timeout); the outcome code: `ok` for exit
    /// code 0, `command_failed` otherwise, `program_not_found`, `timeout` (the process is ended).
    fn run(&self, c: &CommandSpec) -> &'static str;
}

/// Runs nothing (tests, platforms without commands).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCommands;

impl Commands for NoCommands {
    fn run(&self, _: &CommandSpec) -> &'static str {
        "unsupported_platform"
    }
}

/// Records every command and answers `ok` (tests).
#[derive(Debug, Default)]
pub struct FakeCommands {
    pub runs: std::sync::Mutex<Vec<CommandSpec>>,
}

impl Commands for FakeCommands {
    fn run(&self, c: &CommandSpec) -> &'static str {
        if let Ok(mut v) = self.runs.lock() {
            v.push(c.clone());
        }
        "ok"
    }
}

/// Windows and notifications.
pub trait Desktop: Send + Sync + std::fmt::Debug {
    /// Carries out a window step; the outcome code.
    fn window(&self, w: &WindowSpec) -> &'static str;
    /// Shows a notification; the outcome code.
    fn notify(&self, n: &NotifySpec) -> &'static str;
}

/// Touches nothing (tests, platforms without windows): `unsupported_platform`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDesktop;

impl Desktop for NoDesktop {
    fn window(&self, _: &WindowSpec) -> &'static str {
        "unsupported_platform"
    }
    fn notify(&self, _: &NotifySpec) -> &'static str {
        "unsupported_platform"
    }
}

/// Records every call and answers `ok` (tests; the debug test host records into a file).
#[derive(Debug, Default)]
pub struct FakeDesktop {
    pub windows: std::sync::Mutex<Vec<WindowSpec>>,
    pub notes: std::sync::Mutex<Vec<NotifySpec>>,
}

impl Desktop for FakeDesktop {
    fn window(&self, w: &WindowSpec) -> &'static str {
        if let Ok(mut v) = self.windows.lock() {
            v.push(w.clone());
        }
        "ok"
    }
    fn notify(&self, n: &NotifySpec) -> &'static str {
        if let Ok(mut v) = self.notes.lock() {
            v.push(n.clone());
        }
        "ok"
    }
}

/// Other Kim tools.
pub trait Tools: Send + Sync + std::fmt::Debug {
    fn invoke(&self, t: &ToolSpec) -> &'static str;
}

/// No Kim here: every tool step is `tool_missing`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoTools;

impl Tools for NoTools {
    fn invoke(&self, _: &ToolSpec) -> &'static str {
        "tool_missing"
    }
}

/// Carries out a clipboard step. The content is never returned, logged or recorded.
pub fn clipboard(c: &mut dyn Clipboard, spec: &ClipSpec) -> &'static str {
    let mut tries = 0;
    while !c.open() {
        tries += 1;
        if tries >= 10 {
            return "clipboard_busy";
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let code = match spec {
        ClipSpec::Set(t) => {
            if c.set_text(t) {
                "ok"
            } else {
                "clipboard_busy"
            }
        }
        ClipSpec::Clear => {
            if c.set_text("") {
                "ok"
            } else {
                "clipboard_busy"
            }
        }
        ClipSpec::Transform(to) => match c.text() {
            None => "clipboard_no_text",
            Some(t) => {
                use crate::model::ClipTransform as T;
                let new = match to {
                    T::Plain => t,
                    T::Trim => t.trim().to_string(),
                    T::Upper => t.to_uppercase(),
                    T::Lower => t.to_lowercase(),
                };
                if c.set_text(&new) {
                    "ok"
                } else {
                    "clipboard_busy"
                }
            }
        },
    };
    c.close();
    code
}

/// When a `trigger.schedule` fires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    Every(u32),
    /// Minutes after midnight, and the weekdays (0 = Monday) it runs on.
    At {
        minute: u32,
        days: [bool; 7],
    },
}

/// A compiled `trigger.schedule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleSpec {
    pub when: When,
    /// #102 (`missed: run_once`): a time missed while the computer slept runs once when it wakes
    /// (default: skipped as `skipped_missed`). Times passed while paused or off never run.
    pub catch_up: bool,
}

/// #101: a compiled `set_var` step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetVarSpec {
    Set { name: String, value: String },
    Add { name: String, add: i64 },
}

/// At most this many variables in one rule, and characters in one value (#101).
pub const MAX_VARS: usize = 16;
pub const MAX_VAR_CHARS: usize = 1000;

impl SetVarSpec {
    pub fn compile(v: &Value) -> Result<SetVarSpec, String> {
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
        if !crate::text::is_var_name(name) {
            return Err(
                "`name` is a lowercase letter, then letters, digits or `_` (at most 32)".into(),
            );
        }
        match (v.get("value"), v.get("add")) {
            (Some(val), None) => {
                let val = val
                    .as_str()
                    .filter(|t| t.chars().count() <= MAX_VAR_CHARS && !t.contains('\0'))
                    .ok_or("`value` is a text of at most 1000 characters")?;
                Ok(SetVarSpec::Set {
                    name: name.into(),
                    value: val.into(),
                })
            }
            (None, Some(a)) => {
                let add = a
                    .as_i64()
                    .filter(|n| n.unsigned_abs() <= 1_000_000)
                    .ok_or("`add` is a whole number of at most ±1000000")?;
                Ok(SetVarSpec::Add {
                    name: name.into(),
                    add,
                })
            }
            _ => Err("a `set_var` has exactly one of `value` and `add`".into()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            SetVarSpec::Set { name, .. } | SetVarSpec::Add { name, .. } => name,
        }
    }

    /// Applies the step to the run's variables (an `add` to a text that is no number starts at 0).
    pub fn apply(&self, vars: &mut std::collections::BTreeMap<String, String>) {
        match self {
            SetVarSpec::Set { name, value } => {
                vars.insert(name.clone(), value.clone());
            }
            SetVarSpec::Add { name, add } => {
                let n = vars
                    .get(name)
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(0);
                vars.insert(name.clone(), n.saturating_add(*add).to_string());
            }
        }
    }
}

/// #101: an `if` step's condition: the foreground (as a rule's `when`) and a variable's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfCond {
    pub window: Option<crate::context::Cond>,
    /// (`name`, `equals`).
    pub var: Option<(String, String)>,
}

impl IfCond {
    pub fn compile(when: &Value) -> Result<IfCond, String> {
        let mut o = when
            .as_object()
            .cloned()
            .ok_or("`when` must be an object")?;
        let var = match o.remove("var") {
            None => None,
            Some(v) => {
                let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
                let equals = v.get("equals").and_then(Value::as_str);
                let keys_ok = v
                    .as_object()
                    .is_some_and(|m| m.keys().all(|k| k == "name" || k == "equals"));
                match (crate::text::is_var_name(name), equals, keys_ok) {
                    (true, Some(e), true) if e.chars().count() <= MAX_VAR_CHARS => {
                        Some((name.to_string(), e.to_string()))
                    }
                    _ => return Err("`when.var` is {\"name\": …, \"equals\": \"text\"}".into()),
                }
            }
        };
        let window = if o.is_empty() {
            None
        } else {
            Some(crate::context::Cond::compile(&Value::Object(o))?)
        };
        if window.is_none() && var.is_none() {
            return Err("`when` is empty".into());
        }
        Ok(IfCond { window, var })
    }

    /// Whether it holds now (an unknown foreground never matches, as in `when`).
    pub fn holds(
        &self,
        fg: &dyn crate::context::Foreground,
        vars: &std::collections::BTreeMap<String, String>,
    ) -> bool {
        if let Some((name, equals)) = &self.var
            && vars.get(name).map_or("", String::as_str) != equals.as_str()
        {
            return false;
        }
        match &self.window {
            Some(c) => c.holds(fg, &mut crate::context::Probe::default()),
            None => true,
        }
    }
}

/// #101: a compiled `if`: its condition and how many stages its `then` branch spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSpec {
    pub cond: IfCond,
    pub skip: u16,
}

/// #102: a compiled `timer` step: starts the rule `rule` (same profile) once, `after` from now,
/// while the engine runs. Arming it again restarts the countdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimerSpec {
    pub rule: String,
    pub after: Duration,
}

impl TimerSpec {
    pub fn compile(v: &Value) -> Result<TimerSpec, String> {
        let rule = v
            .get("rule")
            .and_then(Value::as_str)
            .filter(|r| !r.is_empty())
            .ok_or("`rule` names the rule to start")?;
        let ms = v
            .get("after_ms")
            .and_then(Value::as_u64)
            .filter(|n| (1..=86_400_000).contains(n))
            .ok_or("`after_ms` is 1–86400000 (24 hours)")?;
        Ok(TimerSpec {
            rule: rule.to_string(),
            after: Duration::from_millis(ms),
        })
    }
}

/// The weekday of a civil date, 0 = Monday (Sakamoto's method).
pub fn weekday(y: i32, m: u8, d: u8) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let sunday0 = (y + y / 4 - y / 100 + y / 400 + T[usize::from(m - 1)] + i32::from(d)) % 7;
    usize::try_from((sunday0 + 6) % 7).unwrap_or(0)
}

impl ScheduleSpec {
    pub fn compile(v: &Value) -> Result<ScheduleSpec, String> {
        let o = v.as_object().ok_or("`schedule` must be an object")?;
        if let Some(k) = o
            .keys()
            .find(|k| !["every_minutes", "at", "days", "missed"].contains(&k.as_str()))
        {
            return Err(format!("unknown field `schedule.{k}`"));
        }
        let catch_up = match o.get("missed").map(|m| m.as_str()) {
            None | Some(Some("skip")) => false,
            Some(Some("run_once")) => true,
            Some(_) => return Err("`missed` is skip or run_once".into()),
        };
        if let Some(n) = o.get("every_minutes") {
            let n = n
                .as_u64()
                .filter(|n| (1..=1440).contains(n))
                .ok_or("`every_minutes` is 1–1440")?;
            if o.contains_key("at") || o.contains_key("days") {
                return Err("`every_minutes` takes no `at` or `days`".into());
            }
            return Ok(ScheduleSpec {
                when: When::Every(u32::try_from(n).unwrap_or(1)),
                catch_up,
            });
        }
        let at = o
            .get("at")
            .and_then(Value::as_str)
            .ok_or("a schedule needs `every_minutes` or `at`")?;
        let (h, m) = at.split_once(':').ok_or("`at` is HH:MM")?;
        let (h, m) = (h.parse::<u32>().ok(), m.parse::<u32>().ok());
        let (Some(h), Some(m)) = (h.filter(|h| *h < 24), m.filter(|m| *m < 60)) else {
            return Err("`at` is HH:MM".into());
        };
        let mut days = [true; 7];
        if let Some(list) = o.get("days") {
            let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
            let list = list
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("`days` lists 1–7 days")?;
            days = [false; 7];
            for d in list {
                let i = d
                    .as_str()
                    .and_then(|d| names.iter().position(|n| *n == d))
                    .ok_or("`days` are mon, tue, wed, thu, fri, sat, sun")?;
                days[i] = true;
            }
        }
        Ok(ScheduleSpec {
            when: When::At {
                minute: h * 60 + m,
                days,
            },
            catch_up,
        })
    }

    /// When it fires next after `now` (whose local time is `local`).
    pub fn next(&self, now: SystemTime, local: &crate::time::LocalTime) -> SystemTime {
        match &self.when {
            When::Every(n) => now + Duration::from_secs(u64::from(*n) * 60),
            When::At { minute, days } => {
                let now_min = u32::from(local.hour) * 60 + u32::from(local.minute);
                let today = weekday(local.year, local.month, local.day);
                let into_minute = u64::from(local.second);
                for ahead in 0..8u32 {
                    let day = (today + ahead as usize) % 7;
                    if !days[day] || (ahead == 0 && *minute <= now_min) {
                        continue;
                    }
                    let mins = u64::from(ahead * 1440 + minute) - u64::from(now_min);
                    return now
                        + Duration::from_secs(mins * 60)
                            .saturating_sub(Duration::from_secs(into_minute));
                }
                now + Duration::from_secs(7 * 86_400)
            }
        }
    }
}

/// A due time more than this late was missed (the computer slept): skipped, or with
/// `missed: run_once` run once on waking (#102).
pub const MISSED: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::FakeClipboard;
    use crate::time::LocalTime;
    use serde_json::json;

    #[test]
    fn clipboard_steps_set_clear_and_transform_without_revealing_the_text() {
        let mut c = FakeClipboard::with_text("  Hello Kim  ");
        let now = |c: &mut FakeClipboard| {
            c.open();
            let t = c.text();
            c.close();
            t
        };
        assert_eq!(
            clipboard(
                &mut c,
                &ClipSpec::Transform(crate::model::ClipTransform::Trim)
            ),
            "ok"
        );
        assert_eq!(now(&mut c).as_deref(), Some("Hello Kim"));
        assert_eq!(
            clipboard(
                &mut c,
                &ClipSpec::Transform(crate::model::ClipTransform::Upper)
            ),
            "ok"
        );
        assert_eq!(now(&mut c).as_deref(), Some("HELLO KIM"));
        assert_eq!(clipboard(&mut c, &ClipSpec::Set("새 글".into())), "ok");
        assert_eq!(now(&mut c).as_deref(), Some("새 글"));
        assert_eq!(clipboard(&mut c, &ClipSpec::Clear), "ok");
        assert_eq!(now(&mut c).as_deref(), Some(""));
        assert!(!c.state().open, "closed after every step");
        assert_eq!(
            clipboard(&mut crate::run::NoClipboard, &ClipSpec::Clear),
            "clipboard_busy"
        );
    }

    #[test]
    fn weekdays_and_schedules() {
        assert_eq!(weekday(2026, 11, 12), 3, "a Thursday");
        assert_eq!(weekday(2024, 2, 29), 3);
        let local = |h, m, s| LocalTime {
            year: 2026,
            month: 11,
            day: 12,
            hour: h,
            minute: m,
            second: s,
        };
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let every = ScheduleSpec::compile(&json!({"every_minutes": 15})).unwrap();
        assert_eq!(
            every.next(t0, &local(9, 0, 0)),
            t0 + Duration::from_secs(900)
        );
        let at = ScheduleSpec::compile(&json!({"at": "09:30"})).unwrap();
        assert_eq!(
            at.next(t0, &local(9, 0, 10)),
            t0 + Duration::from_secs(30 * 60 - 10)
        );
        // Already past today: tomorrow.
        assert_eq!(
            at.next(t0, &local(10, 0, 0)),
            t0 + Duration::from_secs((24 * 60 - 30) * 60)
        );
        // Weekdays only, on a Friday evening: Monday.
        let wd = ScheduleSpec::compile(
            &json!({"at": "08:00", "days": ["mon", "tue", "wed", "thu", "fri"]}),
        )
        .unwrap();
        let fri = LocalTime {
            year: 2026,
            month: 11,
            day: 13,
            hour: 20,
            minute: 0,
            second: 0,
        };
        assert_eq!(
            wd.next(t0, &fri),
            t0 + Duration::from_secs((2 * 1440 + 12 * 60) * 60)
        );
        for bad in [
            json!({}),
            json!({"every_minutes": 0}),
            json!({"at": "24:00"}),
            json!({"at": "9"}),
            json!({"every_minutes": 5, "at": "09:00"}),
            json!({"at": "09:00", "days": ["xyz"]}),
            json!({"at": "09:00", "cron": "*"}),
        ] {
            assert!(ScheduleSpec::compile(&bad).is_err(), "{bad}");
        }
    }

    /// #102: `missed` and one-shot timers compile; bad values are refused.
    #[test]
    fn missed_option_and_timers_compile() {
        let s = ScheduleSpec::compile(&json!({"every_minutes": 5, "missed": "run_once"})).unwrap();
        assert!(s.catch_up);
        assert!(
            !ScheduleSpec::compile(&json!({"at": "09:00"}))
                .unwrap()
                .catch_up
        );
        assert!(
            !ScheduleSpec::compile(&json!({"at": "09:00", "missed": "skip"}))
                .unwrap()
                .catch_up
        );
        assert!(ScheduleSpec::compile(&json!({"at": "09:00", "missed": "later"})).is_err());
        let t = TimerSpec::compile(&json!({"rule": "later", "after_ms": 1500})).unwrap();
        assert_eq!(
            (t.rule.as_str(), t.after),
            ("later", Duration::from_millis(1500))
        );
        for bad in [
            json!({"rule": "", "after_ms": 10}),
            json!({"rule": "x", "after_ms": 0}),
            json!({"rule": "x", "after_ms": 86_400_001u64}),
            json!({"rule": "x"}),
        ] {
            assert!(TimerSpec::compile(&bad).is_err(), "{bad}");
        }
    }
}
