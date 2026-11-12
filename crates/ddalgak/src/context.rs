//! Step 77 (DDALGAK-FR-022, DDALGAK-SEC-011, ADR 0078): rule conditions on the focused app
//! (docs/DDALGAK_RULE_MODEL.md §7.2) — compiled from `when` and evaluated at trigger time.
//!
//! The engine asks a [`Foreground`] only when a binding whose trigger matched has a condition,
//! and at most once per event. Windows answers from the context watcher's cache of the foreground
//! process path (recomputed when the foreground window changed since), and reads the window
//! title only when a matched condition needs it; the title is compared and dropped, never logged
//! or stored. **Unknown is never a match**: a foreground window whose process cannot be read
//! (an elevated process, a protected one, the secure desktop, no window) matches no `apps`, and
//! a rule with `except_apps` does not apply either, because Ddalgak cannot tell that the window
//! is not one of them. Fail safe means the rule stays quiet and the key reaches the app.

use serde_json::Value;

/// The focused window as conditions see it.
pub trait Foreground: Send + Sync + std::fmt::Debug {
    /// The foreground process's executable path, lowercase, or `None` when it cannot be known
    /// (elevated or protected process, secure desktop, no window).
    fn app(&self) -> Option<String>;
    /// The foreground window's title, lowercase, or `None` when it cannot be known. Called only
    /// for a matched rule with a `window_title` condition.
    fn title(&self) -> Option<String>;
    /// #100: the foreground window's class name, lowercase (`None`: unknown, as for the title).
    fn class(&self) -> Option<String> {
        None
    }
    /// #100: the local wall-clock time now (`None`: unknown; a time condition then fails).
    fn local_time(&self) -> Option<crate::time::LocalTime> {
        None
    }
}

/// Nothing is known (platforms without watchers, tests): every condition fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoForeground;

impl Foreground for NoForeground {
    fn app(&self) -> Option<String> {
        None
    }
    fn title(&self) -> Option<String> {
        None
    }
}

/// A fixed foreground (tests, the playground).
#[derive(Clone, Debug, Default)]
pub struct FixedForeground {
    pub app: Option<String>,
    pub title: Option<String>,
    /// #100.
    pub class: Option<String>,
    pub local: Option<crate::time::LocalTime>,
}

impl Foreground for FixedForeground {
    fn app(&self) -> Option<String> {
        self.app.as_ref().map(|a| a.to_lowercase())
    }
    fn title(&self) -> Option<String> {
        self.title.as_ref().map(|t| t.to_lowercase())
    }
    fn class(&self) -> Option<String> {
        self.class.as_ref().map(|c| c.to_lowercase())
    }
    fn local_time(&self) -> Option<crate::time::LocalTime> {
        self.local
    }
}

/// One `apps` / `except_apps` entry, lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppMatch {
    /// The file name (`Code.exe`).
    Exe(String),
    /// The full path, with forward slashes turned into backslashes.
    Path(String),
}

impl AppMatch {
    fn matches(&self, path: &str) -> bool {
        match self {
            AppMatch::Exe(e) => file_name(path) == e,
            AppMatch::Path(p) => path == p,
        }
    }
}

/// A `window_title` condition, lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleMatch {
    Contains(String),
    Equals(String),
}

/// #100: `when.time`: from and to (minutes after midnight; `to` before `from` crosses midnight)
/// on the chosen weekdays (Monday first; for a range crossing midnight, the day it starts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeWindow {
    pub from: u32,
    pub to: u32,
    pub days: [bool; 7],
}

impl TimeWindow {
    pub fn holds(&self, t: &crate::time::LocalTime) -> bool {
        let now = u32::from(t.hour) * 60 + u32::from(t.minute);
        let today = crate::actions::weekday(t.year, t.month, t.day);
        let yesterday = (today + 6) % 7;
        if self.from <= self.to {
            self.days[today] && now >= self.from && now < self.to
        } else {
            (self.days[today] && now >= self.from) || (self.days[yesterday] && now < self.to)
        }
    }
}

/// A compiled `when`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cond {
    pub apps: Vec<AppMatch>,
    pub except_apps: Vec<AppMatch>,
    pub title: Option<TitleMatch>,
    /// #100.
    pub class: Option<TitleMatch>,
    pub time: Option<TimeWindow>,
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn norm_path(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

/// What one evaluation found out, so a second rule on the same event asks nothing again.
#[derive(Debug, Default)]
pub struct Probe {
    app: Option<Option<String>>,
    title: Option<Option<String>>,
    class: Option<Option<String>>,
}

impl Probe {
    fn app(&mut self, fg: &dyn Foreground) -> Option<&str> {
        self.app.get_or_insert_with(|| fg.app()).as_deref()
    }
    fn title(&mut self, fg: &dyn Foreground) -> Option<&str> {
        self.title.get_or_insert_with(|| fg.title()).as_deref()
    }
    fn class(&mut self, fg: &dyn Foreground) -> Option<&str> {
        self.class.get_or_insert_with(|| fg.class()).as_deref()
    }
}

/// `window_title` / `window_class`: exactly one of `contains`, `equals` (1–200 characters).
fn text_match(v: &Value, name: &str) -> Result<TitleMatch, String> {
    let t = v
        .as_object()
        .ok_or(format!("`when.{name}` must be an object"))?;
    if t.len() != 1 {
        return Err(format!(
            "`when.{name}` takes exactly one of `contains`, `equals`"
        ));
    }
    let (op, s) = t.iter().next().expect("one entry");
    let s = s
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 200)
        .ok_or(format!("`when.{name}` needs a text of 1–200 characters"))?;
    match op.as_str() {
        "contains" => Ok(TitleMatch::Contains(s.to_lowercase())),
        "equals" => Ok(TitleMatch::Equals(s.to_lowercase())),
        other => Err(format!("unknown field `when.{name}.{other}`")),
    }
}

fn hhmm(v: Option<&Value>, name: &str) -> Result<u32, String> {
    let s = v
        .and_then(Value::as_str)
        .ok_or(format!("`when.time.{name}` is HH:MM"))?;
    let (h, m) = s
        .split_once(':')
        .ok_or(format!("`when.time.{name}` is HH:MM"))?;
    match (h.parse::<u32>(), m.parse::<u32>()) {
        (Ok(h), Ok(m)) if h < 24 && m < 60 && h_len_ok(s) => Ok(h * 60 + m),
        _ => Err(format!("`when.time.{name}` is HH:MM")),
    }
}

fn h_len_ok(s: &str) -> bool {
    s.len() == 5
}

fn time_window(v: &Value) -> Result<TimeWindow, String> {
    let o = v.as_object().ok_or("`when.time` must be an object")?;
    if let Some(k) = o
        .keys()
        .find(|k| !["from", "to", "days"].contains(&k.as_str()))
    {
        return Err(format!("unknown field `when.time.{k}`"));
    }
    let from = hhmm(o.get("from"), "from")?;
    let to = hhmm(o.get("to"), "to")?;
    if from == to {
        return Err("`when.time` needs `from` and `to` to differ".into());
    }
    let mut days = [true; 7];
    if let Some(list) = o.get("days") {
        let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
        let list = list
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or("`when.time.days` lists 1–7 days")?;
        days = [false; 7];
        for d in list {
            let i = d
                .as_str()
                .and_then(|d| names.iter().position(|n| *n == d))
                .ok_or("`when.time.days` are mon, tue, wed, thu, fri, sat, sun")?;
            days[i] = true;
        }
    }
    Ok(TimeWindow { from, to, days })
}

impl Cond {
    /// Compiles `when`; an unknown field, matcher or an empty condition is an error (the rule
    /// stays inactive, never broader).
    pub fn compile(when: &Value) -> Result<Cond, String> {
        let o = when.as_object().ok_or("`when` must be an object")?;
        let mut c = Cond::default();
        for (k, v) in o {
            match k.as_str() {
                "apps" => c.apps = apps(v, "apps")?,
                "except_apps" => c.except_apps = apps(v, "except_apps")?,
                "window_title" => {
                    let t = v
                        .as_object()
                        .ok_or("`when.window_title` must be an object")?;
                    if t.len() != 1 {
                        return Err(
                            "`when.window_title` takes exactly one of `contains`, `equals`".into(),
                        );
                    }
                    let (op, s) = t.iter().next().expect("one entry");
                    let s = s
                        .as_str()
                        .filter(|s| !s.is_empty() && s.chars().count() <= 200)
                        .ok_or("`when.window_title` needs a text of 1–200 characters")?;
                    c.title = Some(match op.as_str() {
                        "contains" => TitleMatch::Contains(s.to_lowercase()),
                        "equals" => TitleMatch::Equals(s.to_lowercase()),
                        other => return Err(format!("unknown field `when.window_title.{other}`")),
                    });
                }
                "window_class" => c.class = Some(text_match(v, "window_class")?),
                "time" => c.time = Some(time_window(v)?),
                other => return Err(format!("unknown condition `when.{other}`")),
            }
        }
        if c.apps.is_empty()
            && c.except_apps.is_empty()
            && c.title.is_none()
            && c.class.is_none()
            && c.time.is_none()
        {
            return Err("`when` is empty".into());
        }
        Ok(c)
    }

    /// Whether the condition holds for the foreground window. Unknown never matches.
    pub fn holds(&self, fg: &dyn Foreground, probe: &mut Probe) -> bool {
        if !self.apps.is_empty() || !self.except_apps.is_empty() {
            let Some(path) = probe.app(fg) else {
                return false;
            };
            if !self.apps.is_empty() && !self.apps.iter().any(|m| m.matches(path)) {
                return false;
            }
            if self.except_apps.iter().any(|m| m.matches(path)) {
                return false;
            }
        }
        if let Some(t) = &self.title {
            let Some(title) = probe.title(fg) else {
                return false;
            };
            let ok = match t {
                TitleMatch::Contains(s) => title.contains(s.as_str()),
                TitleMatch::Equals(s) => title == s,
            };
            if !ok {
                return false;
            }
        }
        if let Some(t) = &self.class {
            let Some(class) = probe.class(fg) else {
                return false;
            };
            let ok = match t {
                TitleMatch::Contains(s) => class.contains(s.as_str()),
                TitleMatch::Equals(s) => class == s,
            };
            if !ok {
                return false;
            }
        }
        if let Some(w) = &self.time
            && !fg.local_time().is_some_and(|t| w.holds(&t))
        {
            return false;
        }
        true
    }

    /// The precedence ranks of §7.1 step 6.2–6.3: rules with `apps`, then with a title, first.
    pub fn rank(c: Option<&Cond>) -> (bool, bool) {
        (
            !c.is_some_and(|c| !c.apps.is_empty()),
            !c.is_some_and(|c| c.title.is_some() || c.class.is_some()),
        )
    }
}

fn apps(v: &Value, name: &str) -> Result<Vec<AppMatch>, String> {
    let list = v
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 32)
        .ok_or(format!("`when.{name}` needs 1–32 entries"))?;
    list.iter()
        .map(|m| {
            let o = m.as_object().filter(|o| o.len() == 1).ok_or(format!(
                "each `when.{name}` entry takes exactly one of `exe`, `path`"
            ))?;
            let (k, s) = o.iter().next().expect("one entry");
            let s = s
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or(format!("`when.{name}[].{k}` must be a non-empty text"))?;
            match k.as_str() {
                "exe" => Ok(AppMatch::Exe(s.to_lowercase())),
                // Known-folder tokens (`%LOCALAPPDATA%\…`) as in launch paths (§5.5).
                "path" => crate::launch::expand(s, &crate::launch::process_var)
                    .map(|p| AppMatch::Path(norm_path(&p.to_string_lossy())))
                    .ok_or(format!(
                        "`when.{name}[].path` uses a folder this user does not have"
                    )),
                other => Err(format!("unknown field `when.{name}[].{other}`")),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Counting {
        app: Option<&'static str>,
        title: Option<&'static str>,
        calls: AtomicUsize,
    }

    impl Foreground for Counting {
        fn app(&self) -> Option<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.app.map(str::to_lowercase)
        }
        fn title(&self) -> Option<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.title.map(str::to_lowercase)
        }
    }

    fn fg(app: Option<&'static str>, title: Option<&'static str>) -> Counting {
        Counting {
            app,
            title,
            calls: AtomicUsize::new(0),
        }
    }

    fn holds(when: &Value, f: &Counting) -> bool {
        Cond::compile(when).unwrap().holds(f, &mut Probe::default())
    }

    /// DDALGAK-FR-022: exe names and paths ignore case; any entry matches; except_apps wins.
    #[test]
    fn apps_match_by_file_name_or_path_ignoring_case() {
        let code = fg(
            Some(r"C:\Users\x\AppData\Local\Programs\Microsoft VS Code\Code.exe"),
            None,
        );
        assert!(holds(
            &json!({"apps": [{"exe": "notepad.exe"}, {"exe": "code.EXE"}]}),
            &code
        ));
        assert!(holds(
            &json!({"apps": [{"path": "c:/users/x/appdata/local/programs/microsoft vs code/code.exe"}]}),
            &code
        ));
        assert!(
            !holds(&json!({"apps": [{"exe": "Code"}]}), &code),
            "the whole file name"
        );
        assert!(!holds(
            &json!({"except_apps": [{"exe": "Code.exe"}]}),
            &code
        ));
        assert!(holds(
            &json!({"except_apps": [{"exe": "Excel.exe"}]}),
            &code
        ));
    }

    /// Unknown is never a match: not for apps, not for except_apps, not for titles.
    #[test]
    fn an_unknown_foreground_matches_nothing() {
        let none = fg(None, None);
        for w in [
            json!({"apps": [{"exe": "Code.exe"}]}),
            json!({"except_apps": [{"exe": "Code.exe"}]}),
            json!({"window_title": {"contains": "x"}}),
        ] {
            assert!(!holds(&w, &none), "{w}");
        }
    }

    #[test]
    fn titles_contain_or_equal_ignoring_case_and_are_read_only_when_needed() {
        let f = fg(
            Some(r"C:\Apps\Code.exe"),
            Some("main.rs — Kim — Visual Studio Code"),
        );
        assert!(holds(
            &json!({"window_title": {"contains": "visual studio"}}),
            &f
        ));
        assert!(!holds(
            &json!({"window_title": {"equals": "visual studio"}}),
            &f
        ));
        let f = fg(Some(r"C:\Apps\Excel.exe"), Some("t"));
        assert!(!holds(
            &json!({"apps": [{"exe": "Code.exe"}], "window_title": {"contains": "t"}}),
            &f
        ));
        assert_eq!(
            f.calls.load(Ordering::SeqCst),
            1,
            "no title read when the app already failed"
        );
        // One probe per event: a second rule asks nothing again.
        let f = fg(Some(r"C:\Apps\Code.exe"), Some("t"));
        let mut p = Probe::default();
        let a = Cond::compile(&json!({"apps": [{"exe": "Code.exe"}]})).unwrap();
        let b =
            Cond::compile(&json!({"apps": [{"exe": "x.exe"}], "window_title": {"contains": "t"}}))
                .unwrap();
        assert!(a.holds(&f, &mut p));
        assert!(!b.holds(&f, &mut p));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_or_empty_conditions_are_refused() {
        for w in [
            json!({}),
            json!({"window_class": "x"}),
            json!({"apps": []}),
            json!({"apps": [{"exe": "a.exe", "path": "b"}]}),
            json!({"apps": [{"name": "a"}]}),
            json!({"window_title": {"regex": "a.*"}}),
            json!({"window_title": {"contains": ""}}),
        ] {
            assert!(Cond::compile(&w).is_err(), "{w}");
        }
    }

    /// Matching stays cheap: 32 matchers (the schema's most) against 10 000 events in well under a second.
    #[test]
    fn matching_is_cheap() {
        let apps: Vec<Value> = (0..32)
            .map(|i| json!({"exe": format!("tool{i}.exe")}))
            .collect();
        let c = Cond::compile(&json!({"apps": apps, "window_title": {"contains": "kim"}})).unwrap();
        let f = FixedForeground {
            app: Some(r"C:\Apps\tool31.exe".into()),
            title: Some("Kim".into()),
            ..FixedForeground::default()
        };
        let t = std::time::Instant::now();
        for _ in 0..10_000 {
            assert!(c.holds(&f, &mut Probe::default()));
        }
        assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    }

    /// #100: the window class matches case-insensitively like the title; a time window holds
    /// from `from` up to `to` on its days, across midnight on the day it starts; an unknown class
    /// or clock never matches; malformed conditions are errors.
    #[test]
    fn window_class_and_time_conditions() {
        use crate::time::LocalTime;
        let at = |y, m, d, h, mi| LocalTime {
            year: y,
            month: m,
            day: d,
            hour: h,
            minute: mi,
            second: 0,
        };
        // 2026-11-12 is a Thursday.
        let fg = |class: Option<&str>, local: Option<LocalTime>| FixedForeground {
            app: Some(r"C:\Apps\code.exe".into()),
            title: None,
            class: class.map(str::to_string),
            local,
        };
        let c = Cond::compile(&json!({"window_class": {"equals": "Chrome_WidgetWin_1"}})).unwrap();
        assert!(c.holds(&fg(Some("chrome_widgetwin_1"), None), &mut Probe::default()));
        assert!(!c.holds(&fg(Some("Notepad"), None), &mut Probe::default()));
        assert!(!c.holds(&fg(None, None), &mut Probe::default()));
        let work = Cond::compile(&json!({"time": {"from": "09:00", "to": "17:30", "days": ["mon", "tue", "wed", "thu", "fri"]}})).unwrap();
        let holds = |c: &Cond, t| c.holds(&fg(None, Some(t)), &mut Probe::default());
        assert!(holds(&work, at(2026, 11, 12, 9, 0)));
        assert!(!holds(&work, at(2026, 11, 12, 17, 30)));
        assert!(!holds(&work, at(2026, 11, 14, 10, 0)), "Saturday");
        let night =
            Cond::compile(&json!({"time": {"from": "22:00", "to": "06:00", "days": ["fri"]}}))
                .unwrap();
        assert!(holds(&night, at(2026, 11, 13, 23, 0)), "Friday night");
        assert!(
            holds(&night, at(2026, 11, 14, 5, 59)),
            "into Saturday morning"
        );
        assert!(!holds(&night, at(2026, 11, 12, 23, 0)), "Thursday");
        assert!(
            !night.holds(&fg(None, None), &mut Probe::default()),
            "no clock, no match"
        );
        for bad in [
            json!({"time": {"from": "9:00", "to": "17:00"}}),
            json!({"time": {"from": "09:00", "to": "09:00"}}),
            json!({"time": {"from": "09:00"}}),
            json!({"time": {"from": "09:00", "to": "10:00", "days": ["mo"]}}),
            json!({"window_class": {"starts": "x"}}),
        ] {
            assert!(Cond::compile(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            Cond::rank(Some(&c)),
            (true, false),
            "a class ranks like a title"
        );
    }
}
