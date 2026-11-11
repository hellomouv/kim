//! Step 80 (ADR 0081, docs/DDALGAK_AHK_MIGRATION.md): converting a well-defined, declarative
//! subset of AutoHotkey v2 scripts into a Ddalgak rules document.
//!
//! **This reads text; it never runs a script, a command or an expression.** What it understands
//! becomes rules of one profile; everything else is reported with its line and column as
//! *requires manual migration* — never approximated. The document then goes through the profile
//! import of Step 75 (`ddalgak profile import file.ahk`, the editor's *Import profile…*), so every
//! imported rule arrives turned off and without consent, with its permissions, exact targets and
//! conflicts shown before anything is written.
//!
//! Understood (AutoHotkey v2): one-line hotkeys `keys::action` with the modifiers `^ ! + < >`,
//! the prefixes `~` (pass through), `$` and `*` (with a warning) and the suffix ` up`; remaps
//! `a::b`; `return`/`{}` blocks; hotkey blocks `{ … }` whose lines are `Send`, `SendText`,
//! `SendInput`, `SendEvent`, `Run`, `Sleep`, `TrayTip`, `WinMinimize`/`WinMaximize`/`WinRestore`
//! /`WinActivate` and `A_Clipboard := "…"` (several lines become a macro); hotstrings
//! `:options:abbreviation::replacement` with `*`, `?`, `C`, `O`, `B0`, `R`/`T`; `#HotIf
//! WinActive("ahk_exe x.exe")` (and `#HotIf` to end it); `#Requires`, `#SingleInstance`, `#Warn`,
//! `#NoTrayIcon` (nothing to do). `Win` (`#`) shortcuts, custom combinations (`a & b`),
//! functions, variables, expressions, `DllCall`, file, registry and network commands, loops,
//! `#Include` and other `#HotIf` expressions are reported for manual migration.

use serde_json::{Value, json};

/// How a line was handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Became (part of) a rule.
    Converted,
    /// Became a rule, with a difference the user should know.
    Warning,
    /// Not converted: needs manual migration.
    Manual,
    /// Nothing to convert (a directive Ddalgak does not need, a comment block).
    Ignored,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Converted => "converted",
            Level::Warning => "warning",
            Level::Manual => "manual",
            Level::Ignored => "ignored",
        }
    }
}

/// One finding, at a 1-based line and column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub level: Level,
    pub code: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn to_json(&self) -> Value {
        json!({ "line": self.line, "column": self.column, "level": self.level.as_str(),
                "code": self.code, "message": self.message })
    }
}

/// The result of a conversion.
#[derive(Clone, Debug)]
pub struct Conversion {
    /// A `ddalgak.rules` document with one profile (`ahk`).
    pub document: Value,
    pub rules: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl Conversion {
    /// The coverage report (counts and every diagnostic).
    pub fn report(&self, source: &str) -> Value {
        let n = |l: Level| self.diagnostics.iter().filter(|d| d.level == l).count();
        json!({
            "source": source,
            "rules": self.rules,
            "converted": n(Level::Converted),
            "warnings": n(Level::Warning),
            "manual": n(Level::Manual),
            "ignored": n(Level::Ignored),
            "diagnostics": self.diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
        })
    }
}

/// An AutoHotkey key name as Ddalgak's (`None`: not convertible).
fn key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "esc" | "escape" => "Escape",
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "space" => "Space",
        "bs" | "backspace" => "Backspace",
        "del" | "delete" => "Delete",
        "ins" | "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pgup" => "PageUp",
        "pgdn" => "PageDown",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "capslock" => "CapsLock",
        "numlock" => "NumLock",
        "scrolllock" => "ScrollLock",
        "printscreen" => "PrintScreen",
        "pause" => "Pause",
        "appskey" => "ContextMenu",
        "mbutton" => "MouseMiddle",
        "xbutton1" => "MouseX1",
        "xbutton2" => "MouseX2",
        "lctrl" | "lcontrol" => "LCtrl",
        "rctrl" | "rcontrol" => "RCtrl",
        "lalt" => "LAlt",
        "ralt" => "RAlt",
        "lshift" => "LShift",
        "rshift" => "RShift",
        "ctrl" | "control" => "LCtrl",
        "alt" => "LAlt",
        "shift" => "LShift",
        "lwin" | "rwin" | "win" | "lbutton" | "rbutton" => return None,
        "numpadenter" => "NumpadEnter",
        "numpadadd" => "NumpadAdd",
        "numpadsub" => "NumpadSubtract",
        "numpadmult" => "NumpadMultiply",
        "numpaddiv" => "NumpadDivide",
        "numpaddot" => "NumpadDecimal",
        "media_play_pause" => "MediaPlayPause",
        "media_next" => "MediaNext",
        "media_prev" => "MediaPrevious",
        "media_stop" => "MediaStop",
        "volume_up" => "VolumeUp",
        "volume_down" => "VolumeDown",
        "volume_mute" => "VolumeMute",
        _ => {
            if lower.len() == 1 && lower.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Some(lower.to_ascii_uppercase());
            }
            if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
                && (1..=24).contains(&n)
            {
                return Some(format!("F{n}"));
            }
            if let Some(n) = lower
                .strip_prefix("numpad")
                .and_then(|n| n.parse::<u8>().ok())
                && n <= 9
            {
                return Some(format!("Numpad{n}"));
            }
            return None;
        }
    };
    Some(mapped.to_string())
}

/// A parsed hotkey left side.
#[derive(Debug, Default)]
struct Keys {
    keys: String,
    pass_through: bool,
    release: bool,
    wildcard: bool,
    win: bool,
    /// The bare key, for remaps (no modifiers).
    plain: bool,
}

fn parse_keys(spec: &str) -> Result<Keys, String> {
    let mut k = Keys::default();
    if spec.contains(" & ") {
        return Err("custom combinations (`a & b`) have no Ddalgak equivalent".into());
    }
    let mut s = spec.trim();
    if let Some(rest) = s.strip_suffix(" up").or_else(|| s.strip_suffix(" Up")) {
        k.release = true;
        s = rest.trim_end();
    }
    let mut mods: Vec<&str> = Vec::new();
    let mut side: Option<char> = None;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() || (i < chars.len() && "~$*".contains(chars[i]) && chars.len() > 1) {
        let c = chars[i];
        match c {
            '~' => k.pass_through = true,
            '$' => {}
            '*' => k.wildcard = true,
            '<' => side = Some('L'),
            '>' => side = Some('R'),
            '^' | '!' | '+' | '#' => {
                if c == '#' {
                    k.win = true;
                }
                let m = match (c, side.take()) {
                    ('^', Some('L')) => "LCtrl",
                    ('^', Some('R')) => "RCtrl",
                    ('^', _) => "Ctrl",
                    ('!', Some('L')) => "LAlt",
                    ('!', Some('R')) => "RAlt",
                    ('!', _) => "Alt",
                    ('+', Some('L')) => "LShift",
                    ('+', Some('R')) => "RShift",
                    ('+', _) => "Shift",
                    _ => "Win",
                };
                mods.push(m);
            }
            _ => break,
        }
        i += 1;
    }
    let name: String = chars[i..].iter().collect();
    let name = key(&name).ok_or_else(|| format!("the key `{name}` has no Ddalgak name"))?;
    k.plain = mods.is_empty();
    // Ddalgak's canonical order: Ctrl, Alt, Shift (side-specific names are kept as written).
    let rank = |m: &str| match m.trim_start_matches(['L', 'R']) {
        "Ctrl" => 0,
        "Alt" => 1,
        "Shift" => 2,
        _ => 3,
    };
    mods.sort_by_key(|m| rank(m));
    mods.dedup();
    let mut parts: Vec<String> = mods.iter().map(|m| (*m).to_string()).collect();
    parts.push(name);
    k.keys = parts.join("+");
    Ok(k)
}

/// One piece of an AutoHotkey `Send` string.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Chord(String),
    Text(String),
}

/// Parses `Send` syntax (`^c`, `{Enter}`, `{Tab 3}`, literal text, `{Text}`/`{Raw}`).
fn parse_send(s: &str, raw: bool) -> Result<Vec<Piece>, String> {
    let mut out: Vec<Piece> = Vec::new();
    let push_text = |out: &mut Vec<Piece>, c: char| match out.last_mut() {
        Some(Piece::Text(t)) => t.push(c),
        _ => out.push(Piece::Text(c.to_string())),
    };
    if raw {
        return Ok(vec![Piece::Text(s.to_string())]);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut mods: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '^' | '!' | '+' | '#' => {
                if c == '#' {
                    return Err("`#` (Win) in Send: Ddalgak does not send Win shortcuts from a typed string".into());
                }
                mods.push(match c {
                    '^' => "Ctrl",
                    '!' => "Alt",
                    _ => "Shift",
                });
                i += 1;
            }
            '{' => {
                let end = chars[i + 1..]
                    .iter()
                    .position(|c| *c == '}')
                    .map(|p| p + i + 1);
                // `{}}` is a literal `}`.
                let end = match end {
                    Some(e) if e == i + 1 && chars.get(i + 2) == Some(&'}') => i + 2,
                    Some(e) => e,
                    None => return Err("an unclosed `{` in Send".into()),
                };
                let inner: String = chars[i + 1..end].iter().collect();
                i = end + 1;
                let lower = inner.to_ascii_lowercase();
                if lower == "text" || lower == "raw" {
                    let rest: String = chars[i..].iter().collect();
                    if !mods.is_empty() {
                        return Err("modifiers before `{Text}`".into());
                    }
                    out.push(Piece::Text(rest));
                    return Ok(out);
                }
                if inner.chars().count() == 1 && mods.is_empty() {
                    push_text(&mut out, inner.chars().next().unwrap_or(' '));
                    continue;
                }
                let (name, count) = match inner.rsplit_once(' ') {
                    Some((n, c)) if c.parse::<u32>().is_ok() => {
                        (n.to_string(), c.parse::<u32>().unwrap_or(1))
                    }
                    Some((_, c))
                        if matches!(
                            c.to_ascii_lowercase().as_str(),
                            "down" | "up" | "downtemp"
                        ) =>
                    {
                        return Err(format!(
                            "`{{{inner}}}` holds a key down; Ddalgak sends whole key presses"
                        ));
                    }
                    _ => (inner.clone(), 1),
                };
                if count > 20 {
                    return Err(format!("`{{{inner}}}` repeats a key more than 20 times"));
                }
                let k =
                    key(&name).ok_or_else(|| format!("the key `{name}` has no Ddalgak name"))?;
                let chord = mods
                    .drain(..)
                    .map(str::to_string)
                    .chain([k])
                    .collect::<Vec<_>>()
                    .join("+");
                for _ in 0..count {
                    out.push(Piece::Chord(chord.clone()));
                }
            }
            _ => {
                i += 1;
                if mods.is_empty() {
                    push_text(&mut out, c);
                } else {
                    let k = key(&c.to_string())
                        .ok_or_else(|| format!("`{c}` with modifiers has no Ddalgak key"))?;
                    let chord = mods
                        .drain(..)
                        .map(str::to_string)
                        .chain([k])
                        .collect::<Vec<_>>()
                        .join("+");
                    out.push(Piece::Chord(chord));
                }
            }
        }
    }
    if !mods.is_empty() {
        return Err("modifiers at the end of a Send string".into());
    }
    Ok(out)
}

/// The steps of Send pieces: text as `type_text`, chords as `send_keys` (at most 8 each).
fn send_steps(pieces: Vec<Piece>) -> Vec<Value> {
    let mut steps: Vec<Value> = Vec::new();
    let mut chords: Vec<String> = Vec::new();
    let flush = |steps: &mut Vec<Value>, chords: &mut Vec<String>| {
        for group in chords.chunks(8) {
            steps.push(json!({ "type": "send_keys", "keys": group.join(" ") }));
        }
        chords.clear();
    };
    for p in pieces {
        match p {
            Piece::Chord(c) => chords.push(c),
            Piece::Text(t) => {
                flush(&mut steps, &mut chords);
                steps.push(json!({ "type": "type_text", "text": t }));
            }
        }
    }
    flush(&mut steps, &mut chords);
    steps
}

/// The first quoted AutoHotkey string of an argument list (`"…"` or `'…'`, with `` `n ``, `` `t ``,
/// ``` `` ``` and doubled quotes).
fn quoted(args: &str) -> Option<String> {
    let a = args.trim().trim_start_matches('(').trim_start();
    let q = a.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let mut out = String::new();
    let mut it = a[1..].chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '`' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => {}
                o => out.push(o),
            },
            c if c == q => {
                if it.peek() == Some(&q) {
                    it.next();
                    out.push(q);
                } else {
                    return Some(out);
                }
            }
            c => out.push(c),
        }
    }
    None
}

/// The window target of `Win…` commands: `"A"` (the active window) or `"ahk_exe x.exe"`.
fn win_target(args: &str) -> Result<Option<Value>, String> {
    match quoted(args).as_deref() {
        Some("A") => Ok(None),
        Some(t) if t.to_ascii_lowercase().starts_with("ahk_exe ") => {
            Ok(Some(json!({ "exe": t[8..].trim() })))
        }
        Some(_) => Err("a window by title or class: Ddalgak finds windows by program only".into()),
        None if args.trim().is_empty() => {
            Err("a window command without a target (the last found window)".into())
        }
        None => Err("a window target that is an expression".into()),
    }
}

/// The steps of one command line, or why it needs manual migration.
fn command(line: &str) -> Result<(Vec<Value>, Option<String>), String> {
    let t = line.trim();
    let (name, args) = match t.find([' ', '(', ',']) {
        Some(i) => (&t[..i], t[i..].trim_start_matches(',').trim()),
        None => (t, ""),
    };
    let lower = name.to_ascii_lowercase();
    let warn = None;
    match lower.as_str() {
        "send" | "sendinput" | "sendevent" | "sendplay" => {
            let s = quoted(args).ok_or("Send with an expression instead of a string")?;
            Ok((send_steps(parse_send(&s, false)?), warn))
        }
        "sendtext" | "sendraw" => {
            let s = quoted(args).ok_or("SendText with an expression instead of a string")?;
            Ok((vec![json!({ "type": "type_text", "text": s })], warn))
        }
        "sleep" => {
            let ms: u32 = args
                .trim_matches(['(', ')', ' '])
                .parse()
                .map_err(|_| "Sleep with an expression")?;
            if !(1..=10_000).contains(&ms) {
                return Err("Sleep longer than 10 s (a Ddalgak wait is 1–10000 ms)".into());
            }
            Ok((vec![json!({ "type": "delay", "ms": ms })], warn))
        }
        "run" => {
            let target = quoted(args).ok_or("Run with an expression instead of a string")?;
            if args.trim_start_matches('(').trim().len() > target.len() + 2 && args.contains(',') {
                return Err("Run with a working folder or options; set them in the editor".into());
            }
            let low = target.to_ascii_lowercase();
            if low.starts_with("https://") || low.starts_with("http://") {
                return Ok((vec![json!({ "type": "open_url", "url": target })], warn));
            }
            let (prog, rest) = if let Some(r) = target.strip_prefix('"') {
                match r.split_once('"') {
                    Some((p, rest)) => (p.to_string(), rest.trim().to_string()),
                    None => return Err("an unclosed quote in Run".into()),
                }
            } else {
                match target.split_once(' ') {
                    Some((p, rest)) if p.to_ascii_lowercase().ends_with(".exe") => {
                        (p.to_string(), rest.trim().to_string())
                    }
                    _ => (target.clone(), String::new()),
                }
            };
            let pl = prog.to_ascii_lowercase();
            let path = if prog.contains(['\\', '/']) || prog.starts_with('%') {
                prog
            } else if [
                "notepad.exe",
                "calc.exe",
                "mspaint.exe",
                "explorer.exe",
                "charmap.exe",
            ]
            .contains(&pl.as_str())
            {
                format!("%SystemRoot%\\System32\\{prog}")
            } else {
                return Err(format!(
                    "Run \"{prog}\" by name: Ddalgak opens programs by their full path (choose it in the editor)"
                ));
            };
            let mut step = json!({ "type": "open_program", "path": path });
            let mut w = None;
            if !rest.is_empty() {
                step["args"] = json!(rest.split_whitespace().collect::<Vec<_>>());
                w = Some(
                    "arguments were split at spaces; check quoted arguments in the editor"
                        .to_string(),
                );
            }
            Ok((vec![step], w))
        }
        "traytip" => {
            let parts: Vec<&str> = args.split(',').collect();
            let text = quoted(parts.first().copied().unwrap_or("")).unwrap_or_default();
            let title = parts.get(1).and_then(|p| quoted(p)).unwrap_or_default();
            let (title, body) = if title.is_empty() {
                (text, String::new())
            } else {
                (title, text)
            };
            if title.is_empty() {
                return Err("TrayTip without a text".into());
            }
            let mut v =
                json!({ "type": "notify", "title": title.chars().take(63).collect::<String>() });
            if !body.is_empty() {
                v["body"] = json!(body.chars().take(255).collect::<String>());
            }
            Ok((vec![v], warn))
        }
        "winminimize" | "winmaximize" | "winrestore" | "winactivate" => {
            let op = &lower[3..];
            let app = win_target(args)?;
            if op == "activate" && app.is_none() {
                return Err("WinActivate of the active window does nothing".into());
            }
            let mut v = json!({ "type": "window", "op": op });
            if let Some(a) = app {
                v["app"] = a;
            }
            Ok((vec![v], warn))
        }
        "return" => Ok((Vec::new(), warn)),
        _ if lower == "a_clipboard" && args.starts_with(":=") => {
            let s = quoted(&args[2..]).ok_or("A_Clipboard set to an expression")?;
            Ok((
                vec![json!({ "type": "clipboard", "op": "set", "text": s })],
                warn,
            ))
        }
        "dllcall" | "fileappend" | "filedelete" | "filemove" | "filecopy" | "download"
        | "regwrite" | "regdelete" | "comobjcreate" | "runwait" | "exitapp" | "reload" => Err(
            format!("`{name}` has side effects Ddalgak does not imitate"),
        ),
        _ => Err(format!("`{name}` is not in the convertible subset")),
    }
}

/// Strips a `;` comment that starts the line or follows whitespace (outside quotes).
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ';') if prev_space => return &line[..i],
            _ => {}
        }
        prev_space = c.is_whitespace();
    }
    line
}

/// The position of the hotkey `::` outside quotes.
fn hotkey_split(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let b = line.as_bytes();
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ':') if b.get(i + 1) == Some(&b':') => return Some(i),
            _ => {}
        }
    }
    None
}

struct Ctx {
    rules: Vec<Value>,
    diags: Vec<Diagnostic>,
    when: Option<Value>,
    /// The current `#HotIf` could not be converted: its hotkeys need manual migration.
    when_unsupported: bool,
}

impl Ctx {
    fn diag(
        &mut self,
        line: usize,
        column: usize,
        level: Level,
        code: &'static str,
        message: impl Into<String>,
    ) {
        self.diags.push(Diagnostic {
            line,
            column,
            level,
            code,
            message: message.into(),
        });
    }

    #[allow(clippy::needless_pass_by_value)]
    fn push(
        &mut self,
        line: usize,
        keys: &str,
        trigger: Value,
        steps: Vec<Value>,
        warn: Option<String>,
    ) {
        let id = format!("ahk-{line}");
        let what = steps
            .first()
            .and_then(|s| s["type"].as_str())
            .unwrap_or("rule")
            .replace('_', " ");
        let mut title = format!("AHK line {line}: {keys} → {what}");
        if steps.len() > 1 {
            title = format!("AHK line {line}: {keys} → {} steps", steps.len());
        }
        let mut rule = json!({ "id": id, "title": title.chars().take(80).collect::<String>(),
            "enabled": false, "trigger": trigger, "steps": steps });
        if let Some(w) = &self.when {
            rule["when"] = w.clone();
        }
        self.rules.push(rule);
        match warn {
            Some(w) => self.diag(line, 1, Level::Warning, "converted_with_warning", w),
            None => self.diag(
                line,
                1,
                Level::Converted,
                "converted",
                format!("became rule ahk-{line}"),
            ),
        }
    }
}

/// Converts an AutoHotkey v2 script (its text only; nothing is run).
pub fn convert(src: &str, title: &str) -> Conversion {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut c = Ctx {
        rules: Vec::new(),
        diags: Vec::new(),
        when: None,
        when_unsupported: false,
    };
    let mut i = 0;
    let mut in_comment = false;
    while i < lines.len() {
        let n = i + 1;
        let raw = lines[i];
        i += 1;
        let trimmed = raw.trim_start();
        let col = raw.len() - trimmed.len() + 1;
        if in_comment {
            if trimmed.trim_end().ends_with("*/") || trimmed.starts_with("*/") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") {
            in_comment = !trimmed.trim_end().ends_with("*/");
            continue;
        }
        let line = strip_comment(trimmed).trim_end();
        if line.is_empty() {
            continue;
        }
        // Directives.
        // Directives: a line starting with `#` that is not a `#` (Win) hotkey.
        if let Some(d) = line
            .strip_prefix('#')
            .filter(|_| hotkey_split(line).is_none())
        {
            let (name, rest) = d.split_once(char::is_whitespace).unwrap_or((d, ""));
            match name.to_ascii_lowercase().as_str() {
                "requires" => {
                    if !rest.to_ascii_lowercase().contains("v2") {
                        c.diag(
                            n,
                            col,
                            Level::Manual,
                            "not_v2",
                            "the script requires an AutoHotkey version other than v2",
                        );
                    } else {
                        c.diag(
                            n,
                            col,
                            Level::Ignored,
                            "directive",
                            "#Requires: nothing to convert",
                        );
                    }
                }
                "singleinstance" | "warn" | "notrayicon" | "usehook" | "installkeybdhook" => {
                    c.diag(
                        n,
                        col,
                        Level::Ignored,
                        "directive",
                        format!("#{name}: Ddalgak needs nothing for it"),
                    );
                }
                "hotif" => {
                    let e = rest.trim();
                    if e.is_empty() {
                        c.when = None;
                        c.when_unsupported = false;
                        c.diag(
                            n,
                            col,
                            Level::Ignored,
                            "directive",
                            "#HotIf ends the condition",
                        );
                    } else if let Some(q) = e.strip_prefix("WinActive").and_then(quoted)
                        && q.to_ascii_lowercase().starts_with("ahk_exe ")
                        && e.trim_end().ends_with(')')
                        && e.matches('"').count() + e.matches('\'').count() == 2
                    {
                        c.when = Some(json!({ "apps": [ { "exe": q[8..].trim() } ] }));
                        c.when_unsupported = false;
                        c.diag(
                            n,
                            col,
                            Level::Converted,
                            "condition",
                            format!("the following hotkeys apply only in {}", q[8..].trim()),
                        );
                    } else {
                        c.when = None;
                        c.when_unsupported = true;
                        c.diag(n, col, Level::Manual, "condition_unsupported", "only #HotIf WinActive(\"ahk_exe name.exe\") converts; the hotkeys below it need manual migration");
                    }
                }
                _ => c.diag(
                    n,
                    col,
                    Level::Manual,
                    "directive_unsupported",
                    format!("#{name} is not converted"),
                ),
            }
            continue;
        }
        // Hotstrings: `:options:abbreviation::replacement`.
        if let Some(body) = line.strip_prefix(':') {
            let Some(second) = body.find(':').map(|p| p + 1) else {
                c.diag(n, col, Level::Manual, "syntax", "not a hotstring");
                continue;
            };
            let opts = &line[1..second];
            let rest = &line[second + 1..];
            let Some(sep) = rest.find("::") else {
                c.diag(n, col, Level::Manual, "syntax", "a hotstring without `::`");
                continue;
            };
            let abbr = &rest[..sep];
            let repl = &rest[sep + 2..];
            if c.when_unsupported {
                c.diag(
                    n,
                    col,
                    Level::Manual,
                    "condition_unsupported",
                    "under a #HotIf that does not convert",
                );
                continue;
            }
            let o = opts.to_ascii_uppercase();
            if o.contains('X') || repl.trim().is_empty() {
                c.diag(
                    n,
                    col,
                    Level::Manual,
                    "hotstring_code",
                    "a hotstring that runs code needs manual migration",
                );
                continue;
            }
            let raw = o.contains('R') || o.contains('T');
            let mut h = json!({ "abbreviation": abbr });
            if o.contains('*') {
                h["immediate"] = json!(true);
            }
            if o.contains('?') {
                h["inside_words"] = json!(true);
            }
            if o.contains("C1") {
                h["case"] = json!("insensitive");
            } else if o.contains('C') {
                h["case"] = json!("sensitive");
            }
            if o.contains('O') {
                h["omit_end"] = json!(true);
            }
            if o.contains("B0") {
                h["erase"] = json!(false);
            }
            let pieces = match parse_send(repl, raw) {
                Ok(p) => p,
                Err(e) => {
                    c.diag(
                        n,
                        col + second + 1 + sep + 2,
                        Level::Manual,
                        "send_unsupported",
                        e,
                    );
                    continue;
                }
            };
            // A hotstring runs one typing step: text with Enter and Tab as characters.
            let mut text = String::new();
            let mut ok = true;
            for p in pieces {
                match p {
                    Piece::Text(t) => text.push_str(&t),
                    Piece::Chord(k) if k == "Enter" => text.push('\n'),
                    Piece::Chord(k) if k == "Tab" => text.push('\t'),
                    Piece::Chord(_) => ok = false,
                }
            }
            if !ok || text.is_empty() {
                c.diag(n, col, Level::Manual, "hotstring_keys", "a hotstring replacement with shortcuts needs manual migration (a hotstring types text)");
                continue;
            }
            c.push(
                n,
                &format!("::{abbr}"),
                json!({ "hotstring": h }),
                vec![json!({ "type": "type_text", "text": text })],
                None,
            );
            continue;
        }
        // Hotkeys.
        let Some(at) = hotkey_split(line) else {
            c.diag(
                n,
                col,
                Level::Manual,
                "code",
                "code outside a hotkey (variables, functions, expressions) needs manual migration",
            );
            continue;
        };
        let spec = &line[..at];
        let action = line[at + 2..].trim();
        if c.when_unsupported {
            c.diag(
                n,
                col,
                Level::Manual,
                "condition_unsupported",
                "under a #HotIf that does not convert",
            );
            skip_block(&lines, &mut i, action);
            continue;
        }
        let k = match parse_keys(spec) {
            Ok(k) => k,
            Err(e) => {
                c.diag(n, col, Level::Manual, "keys_unsupported", e);
                skip_block(&lines, &mut i, action);
                continue;
            }
        };
        if k.win {
            c.diag(
                n,
                col,
                Level::Manual,
                "win_shortcut",
                "Win shortcuts belong to Windows; Ddalgak does not take them (choose another key)",
            );
            skip_block(&lines, &mut i, action);
            continue;
        }
        let mut trigger = json!({ "keys": k.keys });
        if k.pass_through {
            trigger["pass_through"] = json!(true);
        }
        if k.release {
            trigger["on"] = json!("release");
        }
        let mut warn = k
            .wildcard
            .then(|| "`*` (any modifiers): the Ddalgak rule matches its exact keys".to_string());
        // A remap: `a::b` (a single key name on the right). `x::return` is AutoHotkey's way to
        // disable a key (a block), although `Return` is also a key name.
        if !action.is_empty()
            && !action.eq_ignore_ascii_case("return")
            && !action.contains([' ', '(', '"', '\'', '{'])
            && let Some(to) = key(action)
        {
            if !k.plain || k.release {
                c.diag(
                    n,
                    col,
                    Level::Manual,
                    "remap_unsupported",
                    "a remap from a key with modifiers or ` up`",
                );
                continue;
            }
            c.push(
                n,
                &k.keys,
                trigger,
                vec![json!({ "type": "remap", "to": to })],
                warn,
            );
            continue;
        }
        // The action lines: the rest of the line, or a `{ … }` block.
        let mut body: Vec<(usize, String)> = Vec::new();
        let opened = action == "{"
            || (action.is_empty()
                && lines
                    .get(i)
                    .is_some_and(|l| strip_comment(l.trim()).trim() == "{"));
        if opened {
            if action.is_empty() {
                i += 1;
            }
            let mut closed = false;
            while i < lines.len() {
                let l = strip_comment(lines[i].trim()).trim().to_string();
                i += 1;
                if l == "}" {
                    closed = true;
                    break;
                }
                if !l.is_empty() {
                    body.push((i, l));
                }
            }
            if !closed {
                c.diag(
                    n,
                    col,
                    Level::Manual,
                    "syntax",
                    "a hotkey block without its closing `}`",
                );
                continue;
            }
        } else if action == "{}" || action.eq_ignore_ascii_case("return") {
            c.push(n, &k.keys, trigger, vec![json!({ "type": "block" })], warn);
            continue;
        } else if let Some(inner) = action.strip_prefix('{').and_then(|a| a.strip_suffix('}')) {
            body.push((n, inner.trim().to_string()));
        } else {
            body.push((n, action.to_string()));
        }
        let mut steps = Vec::new();
        let mut failed = None;
        for (ln, l) in &body {
            match command(l) {
                Ok((s, w)) => {
                    steps.extend(s);
                    if w.is_some() {
                        warn = w;
                    }
                }
                Err(e) => {
                    failed = Some((*ln, e));
                    break;
                }
            }
        }
        if let Some((ln, e)) = failed {
            c.diag(
                ln,
                1,
                Level::Manual,
                "command_unsupported",
                format!("{e}; the hotkey on line {n} needs manual migration"),
            );
            continue;
        }
        if steps.is_empty() {
            steps.push(json!({ "type": "block" }));
        }
        if steps.len() > 32 {
            c.diag(n, col, Level::Manual, "too_long", "more than 32 steps");
            continue;
        }
        c.push(n, &k.keys, trigger, steps, warn);
    }
    let rules = c.rules.len();
    let document = json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1,
        "active_profile": "ahk",
        "profiles": [ { "id": "ahk", "title": title.chars().take(80).collect::<String>(),
            "description": "Imported from an AutoHotkey v2 script; every rule arrives turned off.",
            "default_mode": "base", "modes": [ { "id": "base", "title": "Base" } ],
            "rules": c.rules } ]
    });
    Conversion {
        document,
        rules,
        diagnostics: c.diags,
    }
}

/// Skips a `{ … }` block that belongs to a hotkey that is not converted.
fn skip_block(lines: &[&str], i: &mut usize, action: &str) {
    let opened =
        action == "{" || (action.is_empty() && lines.get(*i).is_some_and(|l| l.trim() == "{"));
    if !opened {
        return;
    }
    while *i < lines.len() {
        let l = strip_comment(lines[*i].trim()).trim().to_string();
        *i += 1;
        if l == "}" {
            return;
        }
    }
}

/// Whether a file is an AutoHotkey script (by its extension).
pub fn is_ahk(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ahk") || e.eq_ignore_ascii_case("ah2"))
}

#[cfg(test)]
mod tests;
