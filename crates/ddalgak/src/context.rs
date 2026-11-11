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
}

impl Foreground for FixedForeground {
    fn app(&self) -> Option<String> {
        self.app.as_ref().map(|a| a.to_lowercase())
    }
    fn title(&self) -> Option<String> {
        self.title.as_ref().map(|t| t.to_lowercase())
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

/// A compiled `when`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cond {
    pub apps: Vec<AppMatch>,
    pub except_apps: Vec<AppMatch>,
    pub title: Option<TitleMatch>,
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
}

impl Probe {
    fn app(&mut self, fg: &dyn Foreground) -> Option<&str> {
        self.app.get_or_insert_with(|| fg.app()).as_deref()
    }
    fn title(&mut self, fg: &dyn Foreground) -> Option<&str> {
        self.title.get_or_insert_with(|| fg.title()).as_deref()
    }
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
                other => return Err(format!("unknown condition `when.{other}`")),
            }
        }
        if c.apps.is_empty() && c.except_apps.is_empty() && c.title.is_none() {
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
        true
    }

    /// The precedence ranks of §7.1 step 6.2–6.3: rules with `apps`, then with a title, first.
    pub fn rank(c: Option<&Cond>) -> (bool, bool) {
        (
            !c.is_some_and(|c| !c.apps.is_empty()),
            !c.is_some_and(|c| c.title.is_some()),
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
        };
        let t = std::time::Instant::now();
        for _ in 0..10_000 {
            assert!(c.holds(&f, &mut Probe::default()));
        }
        assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    }
}
