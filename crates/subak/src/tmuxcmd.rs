//! tmux command lines (Step 24, ADR 0058): the parser shared by the window's command prompt
//! (`Ctrl+B :`) and `subak command`, and the key names of `send-keys`.
//!
//! **Syntax** (tmux's, the parts Subak supports): words are separated by spaces and tabs;
//! `'…'` keeps everything literally; `"…"` keeps spaces and reads `\\`, `\"`, `\$`, `\n`, `\t`
//! and `\e`; a `\` outside quotes escapes the next character; an unquoted `;` separates
//! commands and `\;` is a literal semicolon; `#` starts a comment at a word's start. Flags are
//! single letters after `-` (`-hd` = `-h -d`); a flag that takes a value reads the rest of its
//! word or the next word (`-tfoo`, `-t foo`); `--` ends the flags.
//!
//! **Targets** are Subak's (`<terminal>[:<window>[.<pane>]]` or an id, SESSION_MODEL §3); tmux's
//! relative forms (`{last}`, `:+`, `!`, `%id` …) are refused with a hint, never guessed. Without
//! `-t` a command acts on its context: the window's shown pane, or, for `subak command`, the pane
//! it runs in (`SUBAK_PANE_ID`) or `--target`.
//!
//! Nothing here talks to the Host: [`parse`] gives typed commands; the window and the CLI turn
//! them into IPC requests.

use std::fmt;

/// A parse error, with the word it is about. Stable `kind`s for the CLI's `details`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl Error {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: None,
        }
    }

    fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Splits a command line into commands of words (quotes, escapes, `;`, comments).
pub fn split(line: &str) -> Result<Vec<Vec<String>>, Error> {
    let mut commands: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\r' | '\n' => end_word(&mut word, &mut in_word, &mut words),
            '#' if !in_word => break,
            ';' => {
                end_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(n) => word.push(n),
                    None => return Err(Error::new("syntax", "a `\\` at the end of the line")),
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(n) => word.push(n),
                        None => {
                            return Err(Error::new("syntax", "a `'` quote is not closed")
                                .hint("close it with another `'`"));
                        }
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => word.push('\n'),
                            Some('t') => word.push('\t'),
                            Some('e') => word.push('\x1b'),
                            Some(n @ ('\\' | '"' | '$')) => word.push(n),
                            Some(n) => {
                                word.push('\\');
                                word.push(n);
                            }
                            None => {
                                return Err(Error::new("syntax", "a `\"` quote is not closed"));
                            }
                        },
                        Some(n) => word.push(n),
                        None => {
                            return Err(Error::new("syntax", "a `\"` quote is not closed")
                                .hint("close it with another `\"`"));
                        }
                    }
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    if !words.is_empty() {
        commands.push(words);
    }
    Ok(commands)
}

/// A new pane's or window's size in a split (`-l N` or `-l N%`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Cells(u16),
    Percent(u8),
}

/// A direction of `select-pane`, `resize-pane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// A command Subak runs. `target` is `-t` (or `-s` where tmux names the source), as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    NewSession {
        name: Option<String>,
        command: Vec<String>,
    },
    KillSession {
        target: Option<String>,
    },
    RenameSession {
        target: Option<String>,
        name: String,
    },
    ListSessions,
    NewWindow {
        target: Option<String>,
        name: Option<String>,
        detached: bool,
        command: Vec<String>,
    },
    KillWindow {
        target: Option<String>,
    },
    RenameWindow {
        target: Option<String>,
        name: String,
    },
    SelectWindow {
        target: Option<String>,
    },
    NextWindow {
        target: Option<String>,
    },
    PreviousWindow {
        target: Option<String>,
    },
    ListWindows {
        target: Option<String>,
    },
    SplitWindow {
        target: Option<String>,
        horizontal: bool,
        before: bool,
        detached: bool,
        size: Option<Size>,
        command: Vec<String>,
    },
    KillPane {
        target: Option<String>,
    },
    SelectPane {
        target: Option<String>,
        side: Option<Side>,
    },
    ListPanes {
        target: Option<String>,
    },
    ResizePane {
        target: Option<String>,
        side: Option<Side>,
        cells: u16,
        zoom: bool,
    },
    SwapPane {
        target: Option<String>,
        source: Option<String>,
        /// `-U` (previous) or `-D` (next) when no source is given.
        next: bool,
    },
    RotateWindow {
        target: Option<String>,
        down: bool,
    },
    SelectLayout {
        target: Option<String>,
        layout: String,
    },
    BreakPane {
        source: Option<String>,
        name: Option<String>,
        detached: bool,
    },
    JoinPane {
        source: Option<String>,
        target: Option<String>,
        horizontal: bool,
        before: bool,
        detached: bool,
        size: Option<Size>,
    },
    SendKeys {
        target: Option<String>,
        literal: bool,
        keys: Vec<String>,
    },
    CapturePane {
        target: Option<String>,
    },
    DetachClient,
    CopyMode,
    PasteBuffer {
        target: Option<String>,
        buffer: Option<String>,
        delete: bool,
    },
    SetBuffer {
        buffer: Option<String>,
        append: bool,
        data: String,
    },
    DeleteBuffer {
        buffer: Option<String>,
    },
    ListBuffers,
    ShowBuffer {
        buffer: Option<String>,
    },
    // Step 25.
    SetWindowOption {
        target: Option<String>,
        option: String,
        value: Option<bool>,
    },
    ShowWindowOptions {
        target: Option<String>,
    },
    LastWindow {
        target: Option<String>,
    },
    LastPane {
        target: Option<String>,
    },
    NextLayout {
        target: Option<String>,
        forward: bool,
    },
    DisplayMessage {
        target: Option<String>,
        print: bool,
        message: String,
    },
    SetHook {
        name: String,
        command: Option<String>,
    },
    ShowHooks,
    ListCommands,
}

impl Command {
    /// The canonical (long) tmux name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NewSession { .. } => "new-session",
            Self::KillSession { .. } => "kill-session",
            Self::RenameSession { .. } => "rename-session",
            Self::ListSessions => "list-sessions",
            Self::NewWindow { .. } => "new-window",
            Self::KillWindow { .. } => "kill-window",
            Self::RenameWindow { .. } => "rename-window",
            Self::SelectWindow { .. } => "select-window",
            Self::NextWindow { .. } => "next-window",
            Self::PreviousWindow { .. } => "previous-window",
            Self::ListWindows { .. } => "list-windows",
            Self::SplitWindow { .. } => "split-window",
            Self::KillPane { .. } => "kill-pane",
            Self::SelectPane { .. } => "select-pane",
            Self::ListPanes { .. } => "list-panes",
            Self::ResizePane { .. } => "resize-pane",
            Self::SwapPane { .. } => "swap-pane",
            Self::RotateWindow { .. } => "rotate-window",
            Self::SelectLayout { .. } => "select-layout",
            Self::BreakPane { .. } => "break-pane",
            Self::JoinPane { .. } => "join-pane",
            Self::SendKeys { .. } => "send-keys",
            Self::CapturePane { .. } => "capture-pane",
            Self::DetachClient => "detach-client",
            Self::CopyMode => "copy-mode",
            Self::PasteBuffer { .. } => "paste-buffer",
            Self::SetBuffer { .. } => "set-buffer",
            Self::DeleteBuffer { .. } => "delete-buffer",
            Self::ListBuffers => "list-buffers",
            Self::ShowBuffer { .. } => "show-buffer",
            Self::SetWindowOption { .. } => "set-window-option",
            Self::ShowWindowOptions { .. } => "show-window-options",
            Self::LastWindow { .. } => "last-window",
            Self::LastPane { .. } => "last-pane",
            Self::NextLayout { forward: true, .. } => "next-layout",
            Self::NextLayout { .. } => "previous-layout",
            Self::DisplayMessage { .. } => "display-message",
            Self::SetHook { .. } => "set-hook",
            Self::ShowHooks => "show-hooks",
            Self::ListCommands => "list-commands",
        }
    }

    /// Ends processes (the CLI asks, or needs `--yes`; a hook may not contain one).
    pub fn destructive(&self) -> bool {
        matches!(
            self,
            Self::KillSession { .. } | Self::KillWindow { .. } | Self::KillPane { .. }
        )
    }
}

/// Every command name Subak accepts, with its aliases (for help and errors).
pub const COMMANDS: &[(&str, &[&str])] = &[
    ("new-session", &["new"]),
    ("kill-session", &[]),
    ("rename-session", &["rename"]),
    ("list-sessions", &["ls"]),
    ("new-window", &["neww"]),
    ("kill-window", &["killw"]),
    ("rename-window", &["renamew"]),
    ("select-window", &["selectw"]),
    ("next-window", &["next"]),
    ("previous-window", &["prev"]),
    ("list-windows", &["lsw"]),
    ("split-window", &["splitw"]),
    ("kill-pane", &["killp"]),
    ("select-pane", &["selectp"]),
    ("list-panes", &["lsp"]),
    ("resize-pane", &["resizep"]),
    ("swap-pane", &["swapp"]),
    ("rotate-window", &["rotatew"]),
    ("select-layout", &["selectl"]),
    ("break-pane", &["breakp"]),
    ("join-pane", &["joinp", "move-pane", "movep"]),
    ("send-keys", &["send"]),
    ("capture-pane", &["capturep"]),
    ("detach-client", &["detach"]),
    ("copy-mode", &[]),
    ("paste-buffer", &["pasteb"]),
    ("set-buffer", &["setb"]),
    ("delete-buffer", &["deleteb"]),
    ("list-buffers", &["lsb"]),
    ("show-buffer", &["showb"]),
    // Step 25.
    ("set-window-option", &["setw", "set-option", "set"]),
    ("show-window-options", &["showw", "show-options", "show"]),
    ("last-window", &["last"]),
    ("last-pane", &["lastp"]),
    ("next-layout", &["nextl"]),
    ("previous-layout", &["prevl"]),
    ("display-message", &["display"]),
    ("set-hook", &[]),
    ("show-hooks", &[]),
    ("list-commands", &["lscm"]),
];

/// The canonical name of `word`, or `None`.
pub fn canonical(word: &str) -> Option<&'static str> {
    COMMANDS
        .iter()
        .find(|(n, a)| *n == word || a.contains(&word))
        .map(|(n, _)| *n)
}

/// Flags of one command: which letters it takes, and which of those take a value.
struct Spec {
    bools: &'static str,
    values: &'static str,
    /// Positional words: none, one required, one optional, or the rest (a command, keys, data).
    rest: Rest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rest {
    None,
    One,
    Optional,
    Many,
}

fn spec(name: &str) -> Spec {
    let s = |bools, values, rest| Spec {
        bools,
        values,
        rest,
    };
    match name {
        "new-session" => s("d", "s", Rest::Many),
        "kill-session" => s("", "t", Rest::None),
        "rename-session" => s("", "t", Rest::One),
        "list-sessions" => s("", "", Rest::None),
        "new-window" => s("d", "nt", Rest::Many),
        "kill-window" => s("", "t", Rest::None),
        "rename-window" => s("", "t", Rest::One),
        "select-window" => s("", "t", Rest::None),
        "next-window" | "previous-window" => s("", "t", Rest::None),
        "list-windows" | "list-panes" => s("", "t", Rest::None),
        "split-window" => s("hvbd", "lt", Rest::Many),
        "kill-pane" => s("", "t", Rest::None),
        "select-pane" => s("LRUD", "t", Rest::None),
        "resize-pane" => s("LRUDZ", "t", Rest::Optional),
        "swap-pane" => s("UD", "st", Rest::None),
        "rotate-window" => s("UD", "t", Rest::None),
        "select-layout" => s("Enp", "t", Rest::Optional),
        "break-pane" => s("d", "sn", Rest::None),
        "join-pane" => s("hvbd", "lst", Rest::None),
        "send-keys" => s("l", "t", Rest::Many),
        "capture-pane" => s("p", "t", Rest::None),
        "detach-client" | "copy-mode" | "list-buffers" => s("", "", Rest::None),
        "paste-buffer" => s("d", "bt", Rest::None),
        "set-buffer" => s("a", "b", Rest::One),
        "delete-buffer" | "show-buffer" => s("", "b", Rest::None),
        "set-window-option" => s("wgqu", "t", Rest::Many),
        "show-window-options" => s("wgv", "t", Rest::Optional),
        "last-window" | "last-pane" | "next-layout" | "previous-layout" => s("", "t", Rest::None),
        "display-message" => s("p", "t", Rest::Optional),
        "set-hook" => s("gu", "t", Rest::Many),
        "show-hooks" => s("g", "t", Rest::None),
        "list-commands" => s("", "", Rest::None),
        _ => s("", "", Rest::None),
    }
}

/// Flags of tmux that Subak does not have yet, with what to use instead.
fn unsupported(name: &str, flag: char) -> Option<&'static str> {
    Some(match (name, flag) {
        ("split-window", 'f') => "full-width splits are not supported yet (Step 25)",
        ("split-window" | "new-window", 'c') => "use `subak pane split --cwd` / `window new --cwd`",
        ("split-window" | "new-window", 'e') => "use `subak pane split --env` / `window new --env`",
        ("select-pane", 'T') => "pane titles are Step 25",
        ("select-pane" | "swap-pane" | "rotate-window" | "split-window", 'Z') => {
            "keeping the zoom while navigating is Step 25; the pane is unzoomed"
        }
        ("resize-pane", 'x' | 'y') => "absolute sizes are not supported; use -L/-R/-U/-D [cells]",
        ("resize-pane", 'M') => "drag a border with the mouse in the window",
        ("send-keys", 'X') => "copy-mode commands are keys in the window's copy mode",
        ("send-keys", 'M') => "mouse events are not sent this way",
        ("send-keys", 'H' | 'N' | 'R') => "use literal keys (-l) or key names",
        ("capture-pane", 'S' | 'E' | 'e' | 'J') => "use `subak capture --start/--end/--vt/--join`",
        _ => return None,
    })
}

/// A size of `-l`: `N` cells or `N%`.
fn size(v: &str) -> Result<Size, Error> {
    let bad = || {
        Error::new("bad_value", format!("invalid size `{v}`"))
            .hint("a number of cells (1–1000) or a percentage (1%–99%)")
    };
    if let Some(p) = v.strip_suffix('%') {
        return p
            .parse::<u8>()
            .ok()
            .filter(|p| (1..=99).contains(p))
            .map(Size::Percent)
            .ok_or_else(bad);
    }
    v.parse::<u16>()
        .ok()
        .filter(|n| (1..=1000).contains(n))
        .map(Size::Cells)
        .ok_or_else(bad)
}

/// A target as Subak reads it; tmux's relative and special forms are refused (never guessed).
fn target(v: &str) -> Result<String, Error> {
    // Step 25: tmux's tokens work after a Terminal (`work:{last}`, `work:+1`, `work:1.{left-of}`,
    // `work:.!`); a target never starts with one, and tmux's own ids (`%3`, `@1`, `$0`), `=` and
    // `~` are refused: Subak never guesses which Terminal is meant.
    let special = v.starts_with(['{', '!', '%', '@', '$', '=', '~', '+', '-', '.', ':', '^']);
    if v.is_empty() || special {
        return Err(Error::new(
            "bad_target",
            format!("the target `{v}` is not one Subak reads"),
        )
        .hint("name a Terminal first: `work`, `work:1`, `work:1.0`, `work:{last}`, `work:.{left-of}`, or an id (t-/w-/p-…)"));
    }
    Ok(v.to_string())
}

/// Parses a command line into commands.
pub fn parse(line: &str) -> Result<Vec<Command>, Error> {
    let words = split(line)?;
    if words.is_empty() {
        return Err(Error::new("empty", "no command").hint("try `split-window -h` or `list-panes`"));
    }
    words.iter().map(|w| parse_one(w)).collect()
}

fn parse_one(words: &[String]) -> Result<Command, Error> {
    let first = words[0].as_str();
    let name = canonical(first).ok_or_else(|| {
        Error::new("unknown_command", format!("unknown command `{first}`")).hint(format!(
            "commands: {}",
            COMMANDS.iter().map(|c| c.0).collect::<Vec<_>>().join(", ")
        ))
    })?;
    let sp = spec(name);
    let mut flags: Vec<(char, Option<String>)> = Vec::new();
    let mut i = 1;
    while i < words.len() {
        let w = &words[i];
        if w == "--" {
            i += 1;
            break;
        }
        if !w.starts_with('-') || w.len() < 2 {
            break;
        }
        let mut letters = w[1..].chars();
        while let Some(f) = letters.next() {
            if sp.values.contains(f) {
                let rest: String = letters.by_ref().collect();
                let v = if rest.is_empty() {
                    i += 1;
                    words.get(i).cloned().ok_or_else(|| {
                        Error::new("missing_value", format!("{name} -{f} needs a value"))
                    })?
                } else {
                    rest
                };
                flags.push((f, Some(v)));
                break;
            } else if sp.bools.contains(f) {
                flags.push((f, None));
            } else if let Some(h) = unsupported(name, f) {
                return Err(Error::new(
                    "unsupported_flag",
                    format!("{name} -{f} is not supported"),
                )
                .hint(h));
            } else {
                return Err(
                    Error::new("unknown_flag", format!("{name} has no flag -{f}")).hint(format!(
                        "{name} takes {}",
                        sp.bools
                            .chars()
                            .chain(sp.values.chars())
                            .map(|c| format!("-{c}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )),
                );
            }
        }
        i += 1;
    }
    let rest: Vec<String> = words[i..].to_vec();
    match sp.rest {
        Rest::None if !rest.is_empty() => {
            return Err(Error::new(
                "extra_argument",
                format!("{name} takes no argument (`{}`)", rest[0]),
            ));
        }
        Rest::One if rest.len() != 1 => {
            return Err(Error::new(
                "missing_argument",
                format!("{name} takes exactly one argument"),
            ));
        }
        Rest::Optional if rest.len() > 1 => {
            return Err(Error::new(
                "extra_argument",
                format!("{name} takes at most one argument"),
            ));
        }
        _ => {}
    }
    let has = |c: char| flags.iter().any(|(f, _)| *f == c);
    let val = |c: char| {
        flags
            .iter()
            .rev()
            .find(|(f, _)| *f == c)
            .and_then(|(_, v)| v.clone())
    };
    let t = |c: char| val(c).map(|v| target(&v)).transpose();
    let one = || rest.first().cloned().unwrap_or_default();
    let side = |bools: &[(char, Side)]| -> Result<Option<Side>, Error> {
        let given: Vec<Side> = bools
            .iter()
            .filter(|(c, _)| has(*c))
            .map(|(_, s)| *s)
            .collect();
        match given.as_slice() {
            [] => Ok(None),
            [s] => Ok(Some(*s)),
            _ => Err(Error::new(
                "conflicting_flags",
                format!("{name}: give one direction"),
            )),
        }
    };
    let dirs = [
        ('L', Side::Left),
        ('R', Side::Right),
        ('U', Side::Up),
        ('D', Side::Down),
    ];
    Ok(match name {
        "new-session" => Command::NewSession {
            name: val('s'),
            command: rest,
        },
        "kill-session" => Command::KillSession { target: t('t')? },
        "rename-session" => Command::RenameSession {
            target: t('t')?,
            name: one(),
        },
        "list-sessions" => Command::ListSessions,
        "new-window" => Command::NewWindow {
            target: t('t')?,
            name: val('n'),
            detached: has('d'),
            command: rest,
        },
        "kill-window" => Command::KillWindow { target: t('t')? },
        "rename-window" => Command::RenameWindow {
            target: t('t')?,
            name: one(),
        },
        "select-window" => Command::SelectWindow { target: t('t')? },
        "next-window" => Command::NextWindow { target: t('t')? },
        "previous-window" => Command::PreviousWindow { target: t('t')? },
        "list-windows" => Command::ListWindows { target: t('t')? },
        "list-panes" => Command::ListPanes { target: t('t')? },
        "split-window" => {
            if has('h') && has('v') {
                return Err(Error::new(
                    "conflicting_flags",
                    "split-window: give -h or -v, not both",
                ));
            }
            Command::SplitWindow {
                target: t('t')?,
                horizontal: has('h'),
                before: has('b'),
                detached: has('d'),
                size: val('l').map(|v| size(&v)).transpose()?,
                command: rest,
            }
        }
        "kill-pane" => Command::KillPane { target: t('t')? },
        "select-pane" => Command::SelectPane {
            target: t('t')?,
            side: side(&dirs)?,
        },
        "resize-pane" => {
            let s = side(&dirs)?;
            let cells = match rest.first() {
                None => 1,
                Some(n) => n
                    .parse::<u16>()
                    .ok()
                    .filter(|n| (1..=1000).contains(n))
                    .ok_or_else(|| {
                        Error::new("bad_value", format!("invalid number of cells `{n}`"))
                    })?,
            };
            if s.is_none() && !has('Z') {
                return Err(Error::new(
                    "missing_argument",
                    "resize-pane needs -L, -R, -U, -D or -Z",
                ));
            }
            Command::ResizePane {
                target: t('t')?,
                side: s,
                cells,
                zoom: has('Z'),
            }
        }
        "swap-pane" => {
            if has('U') && has('D') {
                return Err(Error::new("conflicting_flags", "swap-pane: give -U or -D"));
            }
            let source = t('s')?;
            if source.is_none() && !has('U') && !has('D') {
                return Err(Error::new(
                    "missing_argument",
                    "swap-pane needs -s <pane>, -U or -D",
                ));
            }
            Command::SwapPane {
                target: t('t')?,
                source,
                next: has('D'),
            }
        }
        "rotate-window" => Command::RotateWindow {
            target: t('t')?,
            down: has('D'),
        },
        "select-layout" => {
            let l = if has('E') {
                "spread".to_string()
            } else if has('n') {
                "next".to_string()
            } else if has('p') {
                "previous".to_string()
            } else {
                one()
            };
            if !matches!(
                l.as_str(),
                "even-horizontal"
                    | "even-vertical"
                    | "main-horizontal"
                    | "main-horizontal-mirrored"
                    | "main-vertical"
                    | "main-vertical-mirrored"
                    | "tiled"
                    | "spread"
                    | "next"
                    | "previous"
            ) {
                return Err(Error::new("bad_value", format!("unknown layout `{l}`")).hint(
                    "even-horizontal, even-vertical, main-horizontal, main-vertical (-mirrored), tiled; -E spreads, -n/-p cycle (layout strings are not supported yet)",
                ));
            }
            Command::SelectLayout {
                target: t('t')?,
                layout: l,
            }
        }
        "set-window-option" => {
            if has('g') {
                return Err(Error::new(
                    "unsupported_flag",
                    "global options (-g) are not supported; set the option on a window",
                ));
            }
            let (option, value) = match rest.as_slice() {
                [o] if has('u') => (o.clone(), Some(false)),
                [o] => (o.clone(), None),
                [o, v] => (
                    o.clone(),
                    Some(match v.as_str() {
                        "on" | "1" | "yes" | "true" => true,
                        "off" | "0" | "no" | "false" => false,
                        _ => {
                            return Err(Error::new("bad_value", format!("invalid value `{v}`"))
                                .hint("on or off (omit it to toggle)"));
                        }
                    }),
                ),
                _ => {
                    return Err(Error::new(
                        "missing_argument",
                        "set-window-option takes an option and on/off",
                    ));
                }
            };
            if !matches!(
                option.as_str(),
                "synchronize-panes" | "monitor-activity" | "monitor-bell"
            ) {
                return Err(Error::new("bad_value", format!("unknown option `{option}`"))
                    .hint("synchronize-panes, monitor-activity, monitor-bell (more options are listed in TMUX_COMPATIBILITY §9)"));
            }
            Command::SetWindowOption {
                target: t('t')?,
                option,
                value,
            }
        }
        "show-window-options" => Command::ShowWindowOptions { target: t('t')? },
        "last-window" => Command::LastWindow { target: t('t')? },
        "last-pane" => Command::LastPane { target: t('t')? },
        "next-layout" => Command::NextLayout {
            target: t('t')?,
            forward: true,
        },
        "previous-layout" => Command::NextLayout {
            target: t('t')?,
            forward: false,
        },
        "display-message" => Command::DisplayMessage {
            target: t('t')?,
            print: has('p'),
            message: rest.first().cloned().unwrap_or_else(|| {
                "[#{session_name}] #{window_index}:#{window_name} pane #{pane_index}".into()
            }),
        },
        "set-hook" => {
            let name = rest
                .first()
                .cloned()
                .ok_or_else(|| Error::new("missing_argument", "set-hook needs a hook name"))?;
            let command = if has('u') {
                None
            } else {
                Some(rest[1..].join(" ")).filter(|c| !c.is_empty())
            };
            if !has('u') && command.is_none() {
                return Err(Error::new(
                    "missing_argument",
                    "set-hook needs a command (or -u to remove the hook)",
                ));
            }
            Command::SetHook { name, command }
        }
        "show-hooks" => Command::ShowHooks,
        "list-commands" => Command::ListCommands,
        "break-pane" => Command::BreakPane {
            source: t('s')?,
            name: val('n'),
            detached: has('d'),
        },
        "join-pane" => Command::JoinPane {
            source: t('s')?,
            target: t('t')?,
            horizontal: has('h'),
            before: has('b'),
            detached: has('d'),
            size: val('l').map(|v| size(&v)).transpose()?,
        },
        "send-keys" => Command::SendKeys {
            target: t('t')?,
            literal: has('l'),
            keys: rest,
        },
        "capture-pane" => Command::CapturePane { target: t('t')? },
        "detach-client" => Command::DetachClient,
        "copy-mode" => Command::CopyMode,
        "paste-buffer" => Command::PasteBuffer {
            target: t('t')?,
            buffer: val('b'),
            delete: has('d'),
        },
        "set-buffer" => Command::SetBuffer {
            buffer: val('b'),
            append: has('a'),
            data: one(),
        },
        "delete-buffer" => Command::DeleteBuffer { buffer: val('b') },
        "list-buffers" => Command::ListBuffers,
        "show-buffer" => Command::ShowBuffer { buffer: val('b') },
        _ => unreachable!("every canonical name has a case"),
    })
}

/// Where a command runs when it has no `-t`: the window's shown Terminal, its active window and
/// pane, or for `subak command` the pane it runs in (`SUBAK_PANE_ID`) or `--target`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub terminal: Option<String>,
    pub window: Option<String>,
    pub pane: Option<String>,
    /// Consent for commands that end processes (`--yes`, or the window's own confirmation).
    pub yes: bool,
}

/// What a command needs done.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// One IPC request.
    Request {
        method: &'static str,
        params: serde_json::Value,
    },
    /// Select the next (`+1`) or previous (`-1`) window of a Terminal (needs its window list).
    Relative { terminal: String, delta: i64 },
    /// Done by the window itself (copy mode, detach); refused by `subak command`.
    Local(&'static str),
    /// Step 25: a message (`display-message`) expanded with the formats of a Terminal's pane:
    /// the caller reads `terminal` (`terminal.info`) and expands `message` for `pane` (a target
    /// part or id; `None`: the active pane).
    Format {
        terminal: String,
        pane: Option<String>,
        message: String,
        print: bool,
    },
    /// Step 25: text the caller shows as it is (`list-commands`).
    Text(String),
}

fn need(v: Option<&String>, what: &str) -> Result<String, Error> {
    v.cloned().ok_or_else(|| {
        Error::new("no_target", format!("no {what} to act on"))
            .hint("give -t <target>, use `subak command --target <target>`, or run it inside a Subak pane")
    })
}

fn program(command: &[String]) -> serde_json::Value {
    match command.split_first() {
        Some((p, a)) => serde_json::json!({"program": p, "args": a}),
        None => serde_json::Value::Null,
    }
}

fn split_size(size: Option<Size>, params: &mut serde_json::Value) {
    match size {
        Some(Size::Cells(n)) => params["cells"] = serde_json::json!(n),
        Some(Size::Percent(n)) => params["percent"] = serde_json::json!(n),
        None => {}
    }
}

/// The plan of one command in a context.
pub fn plan(cmd: &Command, ctx: &Context) -> Result<Plan, Error> {
    use serde_json::json;
    let pane = |t: &Option<String>| need(t.as_ref().or(ctx.pane.as_ref()), "pane");
    let window = |t: &Option<String>| {
        need(
            t.as_ref().or(ctx.window.as_ref()).or(ctx.pane.as_ref()),
            "window",
        )
    };
    let terminal = |t: &Option<String>| need(t.as_ref().or(ctx.terminal.as_ref()), "Terminal");
    let req = |method: &'static str, params: serde_json::Value| Plan::Request { method, params };
    let consent = |what: &str| -> Result<(), Error> {
        if ctx.yes {
            Ok(())
        } else {
            Err(
                Error::new("confirmation_required", format!("{what} ends processes")).hint(
                    "add --yes to `subak command` (the window asks before it ends a busy pane)",
                ),
            )
        }
    };
    Ok(match cmd {
        Command::NewSession { name, command } => req(
            "terminal.create",
            json!({"name": name, "command": program(command)}),
        ),
        Command::KillSession { target } => {
            consent("kill-session")?;
            req(
                "terminal.kill",
                json!({"target": terminal(target)?, "force": true}),
            )
        }
        Command::RenameSession { target, name } => req(
            "terminal.rename",
            json!({"target": terminal(target)?, "name": name}),
        ),
        Command::ListSessions => req("terminal.list", json!({"detail": "full"})),
        Command::NewWindow {
            target,
            name,
            detached,
            command,
        } => req(
            "window.create",
            json!({"target": terminal(target)?, "name": name, "select": !detached,
                "command": program(command)}),
        ),
        Command::KillWindow { target } => {
            consent("kill-window")?;
            req(
                "window.kill",
                json!({"target": window(target)?, "force": true}),
            )
        }
        Command::RenameWindow { target, name } => req(
            "window.rename",
            json!({"target": window(target)?, "name": name}),
        ),
        Command::SelectWindow { target } => req(
            "window.select",
            json!({"target": need(target.as_ref(), "window (select-window needs -t)")?}),
        ),
        Command::NextWindow { target } => Plan::Relative {
            terminal: terminal(target)?,
            delta: 1,
        },
        Command::PreviousWindow { target } => Plan::Relative {
            terminal: terminal(target)?,
            delta: -1,
        },
        Command::ListWindows { target } => req("window.list", json!({"target": terminal(target)?})),
        Command::ListPanes { target } => req(
            "pane.list",
            json!({"target": target.clone().map_or_else(|| terminal(&None), Ok)?}),
        ),
        Command::SplitWindow {
            target,
            horizontal,
            before,
            detached,
            size,
            command,
        } => {
            let mut p = json!({"target": pane(target)?, "before": before, "select": !detached,
                "direction": if *horizontal { "horizontal" } else { "vertical" },
                "command": program(command)});
            split_size(*size, &mut p);
            req("pane.split", p)
        }
        Command::KillPane { target } => {
            consent("kill-pane")?;
            req("pane.kill", json!({"target": pane(target)?, "force": true}))
        }
        Command::SelectPane { target, side } => {
            let mut p = json!({"target": pane(target)?});
            if let Some(s) = side {
                p["direction"] = json!(s.name());
            }
            req("pane.select", p)
        }
        Command::ResizePane {
            target,
            side,
            cells,
            zoom,
        } => match side {
            Some(s) => req(
                "pane.adjust",
                json!({"target": pane(target)?, "direction": s.name(), "cells": cells}),
            ),
            None if *zoom => req("pane.zoom", json!({"target": pane(target)?})),
            None => unreachable!("the parser needs a side or -Z"),
        },
        Command::SwapPane {
            target,
            source,
            next,
        } => match source {
            Some(src) => req("pane.swap", json!({"target": src, "with": pane(target)?})),
            None => req(
                "pane.swap",
                json!({"target": pane(target)?,
                    "direction": if *next { "next" } else { "previous" }}),
            ),
        },
        Command::RotateWindow { target, down } => req(
            "window.rotate",
            json!({"target": window(target)?, "direction": if *down { "down" } else { "up" }}),
        ),
        Command::SelectLayout { target, layout } => req(
            "window.select_layout",
            json!({"target": window(target)?, "layout": layout}),
        ),
        Command::BreakPane {
            source,
            name,
            detached,
        } => req(
            "pane.break",
            json!({"target": pane(source)?, "name": name, "select": !detached}),
        ),
        Command::JoinPane {
            source,
            target,
            horizontal,
            before,
            detached,
            size,
        } => {
            let mut p = json!({"source": need(source.as_ref(), "pane to move (join-pane needs -s)")?,
                "target": pane(target)?, "before": before, "select": !detached,
                "direction": if *horizontal { "horizontal" } else { "vertical" }});
            split_size(*size, &mut p);
            req("pane.join", p)
        }
        Command::SendKeys {
            target,
            literal,
            keys,
        } => req(
            "pane.send_keys",
            json!({"target": pane(target)?, "keys": keys, "literal": literal}),
        ),
        Command::CapturePane { target } => req("pane.capture", json!({"target": pane(target)?})),
        Command::DetachClient => Plan::Local("detach-client"),
        Command::CopyMode => Plan::Local("copy-mode"),
        Command::PasteBuffer {
            target,
            buffer,
            delete,
        } => req(
            "buffer.paste",
            json!({"target": pane(target)?, "name": buffer, "delete": delete}),
        ),
        Command::SetBuffer {
            buffer,
            append,
            data,
        } => req(
            "buffer.set",
            json!({"name": buffer, "data": data, "append": append}),
        ),
        Command::DeleteBuffer { buffer } => req("buffer.delete", json!({"name": buffer})),
        Command::ListBuffers => req("buffer.list", json!({})),
        Command::ShowBuffer { buffer } => req("buffer.get", json!({"name": buffer})),
        Command::SetWindowOption {
            target,
            option,
            value,
        } => req(
            "window.set_option",
            json!({"target": window(target)?, "option": option, "value": value}),
        ),
        Command::ShowWindowOptions { target } => {
            req("window.options", json!({"target": window(target)?}))
        }
        Command::LastWindow { target } => req(
            "window.select",
            json!({"target": format!("{}:{{last}}", terminal(target)?)}),
        ),
        Command::LastPane { target } => req(
            "pane.select",
            json!({"target": format!("{}:.{{last}}", terminal(target)?)}),
        ),
        Command::NextLayout { target, forward } => req(
            "window.select_layout",
            json!({"target": window(target)?, "layout": if *forward { "next" } else { "previous" }}),
        ),
        Command::DisplayMessage {
            target,
            print,
            message,
        } => {
            let (term, pane) = match target {
                Some(t) if !t.starts_with("p-") && !t.starts_with("w-") => (
                    t.split(':').next().unwrap_or(t).to_string(),
                    t.split_once(':').map(|(_, r)| r.to_string()),
                ),
                Some(id) => (terminal(&None)?, Some(id.clone())),
                None => (
                    terminal(&None)?,
                    ctx.pane.clone().filter(|p| p.starts_with("p-")),
                ),
            };
            Plan::Format {
                terminal: term,
                pane,
                message: message.clone(),
                print: *print,
            }
        }
        Command::SetHook { name, command } => {
            req("hook.set", json!({"name": name, "command": command}))
        }
        Command::ShowHooks => req("hook.list", json!({})),
        Command::ListCommands => Plan::Text(
            COMMANDS
                .iter()
                .map(|(n, a)| {
                    if a.is_empty() {
                        (*n).to_string()
                    } else {
                        format!("{n} ({})", a.join(", "))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    })
}

/// Step 25: the format variables of one pane of a Terminal object (`terminal.info`): the pane
/// named by `pane` (`p-…`, or `window.pane`/`window` after the Terminal), else the active one.
pub fn vars(
    t: &serde_json::Value,
    pane: Option<&str>,
) -> std::collections::BTreeMap<String, String> {
    let windows = t["windows"].as_array().cloned().unwrap_or_default();
    let active_w = t["active_window"].as_str().unwrap_or("");
    let (wi, pi) = pick(&windows, active_w, pane);
    let w = &windows[wi];
    let panes = w["panes"].as_array().cloned().unwrap_or_default();
    let p = &panes[pi];
    let mut v = std::collections::BTreeMap::new();
    let s = |x: &serde_json::Value| match x {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };
    let flag = |b: bool| if b { "1" } else { "0" }.to_string();
    let w_active = w["id"].as_str() == Some(active_w);
    let mut flags = String::new();
    if w_active {
        flags.push('*');
    } else if t["last_window"].is_string() && w["id"] == t["last_window"] {
        flags.push('-');
    }
    if w["activity"] == true {
        flags.push('#');
    }
    if w["bell"] == true {
        flags.push('!');
    }
    if !w["zoomed"].is_null() {
        flags.push('Z');
    }
    let program = p["executable"]
        .as_str()
        .or_else(|| p["command"]["program"].as_str())
        .unwrap_or("");
    let program = program.rsplit(['\\', '/']).next().unwrap_or(program);
    for (k, val) in [
        ("session_name", s(&t["name"])),
        (
            "session_id",
            s(if t["id"].is_null() {
                &t["terminal_id"]
            } else {
                &t["id"]
            }),
        ),
        ("session_windows", windows.len().to_string()),
        ("window_index", s(&w["index"])),
        ("window_name", s(&w["name"])),
        ("window_id", s(&w["id"])),
        ("window_panes", panes.len().to_string()),
        ("window_active", flag(w_active)),
        ("window_flags", flags),
        ("window_zoomed_flag", flag(!w["zoomed"].is_null())),
        ("window_activity_flag", flag(w["activity"] == true)),
        ("window_bell_flag", flag(w["bell"] == true)),
        ("window_width", s(&w["size"]["cols"])),
        ("window_height", s(&w["size"]["rows"])),
        ("pane_index", s(&p["index"])),
        ("pane_id", s(&p["id"])),
        ("pane_pid", s(&p["pid"])),
        ("pane_width", s(&p["size"]["cols"])),
        ("pane_height", s(&p["size"]["rows"])),
        ("pane_left", s(&p["rect"]["x"])),
        ("pane_top", s(&p["rect"]["y"])),
        ("pane_active", flag(w["active_pane"] == p["id"])),
        (
            "pane_current_command",
            program.trim_end_matches(".exe").to_string(),
        ),
        (
            "pane_synchronized",
            flag(w["options"]["synchronize-panes"] == true),
        ),
        ("pane_dead", flag(p["running"] == false)),
    ] {
        v.insert(k.to_string(), val);
    }
    v
}

/// The window and pane positions a format is about.
fn pick(windows: &[serde_json::Value], active_w: &str, pane: Option<&str>) -> (usize, usize) {
    let active = |ws: &[serde_json::Value], wi: usize| {
        ws[wi]["panes"]
            .as_array()
            .and_then(|ps| ps.iter().position(|p| p["id"] == ws[wi]["active_pane"]))
            .unwrap_or(0)
    };
    let wa = windows
        .iter()
        .position(|w| w["id"] == active_w)
        .unwrap_or(0);
    let Some(target) = pane.filter(|p| !p.is_empty()) else {
        return (wa, active(windows, wa));
    };
    for (wi, w) in windows.iter().enumerate() {
        if let Some(pi) = w["panes"]
            .as_array()
            .and_then(|ps| ps.iter().position(|p| p["id"] == target))
        {
            return (wi, pi);
        }
    }
    let (win, pn) = target
        .split_once('.')
        .map_or((target, None), |(a, b)| (a, Some(b)));
    let wi = win
        .parse::<u64>()
        .ok()
        .and_then(|n| windows.iter().position(|w| w["index"] == n))
        .or_else(|| windows.iter().position(|w| w["name"] == win))
        .unwrap_or(wa);
    let pi = pn
        .and_then(|n| n.parse::<u64>().ok())
        .and_then(|n| {
            windows[wi]["panes"]
                .as_array()
                .and_then(|ps| ps.iter().position(|p| p["index"] == n))
        })
        .unwrap_or_else(|| active(windows, wi));
    (wi, pi)
}

/// Step 25: expands tmux formats: `#{name}`, `#{?name,yes,no}` (true when not empty and not
/// `0`), the short forms `#S #I #W #P #D #F #T`, and `##`. Unknown names expand to nothing (as in
/// tmux).
pub fn format(fmt: &str, vars: &std::collections::BTreeMap<String, String>) -> String {
    let get = |k: &str| vars.get(k).cloned().unwrap_or_default();
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '#' || i + 1 >= chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = chars[i + 1];
        i += 2;
        match n {
            '#' => out.push('#'),
            'S' => out.push_str(&get("session_name")),
            'I' => out.push_str(&get("window_index")),
            'W' => out.push_str(&get("window_name")),
            'P' => out.push_str(&get("pane_index")),
            'D' => out.push_str(&get("pane_id")),
            'F' => out.push_str(&get("window_flags")),
            'T' => {}
            '{' => {
                let mut depth = 1;
                let start = i;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                let body: String = chars[start..i.saturating_sub(1)].iter().collect();
                if let Some(cond) = body.strip_prefix('?') {
                    let mut parts = cond.splitn(3, ',');
                    let (name, yes, no) = (
                        parts.next().unwrap_or(""),
                        parts.next().unwrap_or(""),
                        parts.next().unwrap_or(""),
                    );
                    let v = get(name);
                    let pick = if !v.is_empty() && v != "0" { yes } else { no };
                    out.push_str(&format(pick, vars));
                } else {
                    out.push_str(&get(&body));
                }
            }
            other => {
                out.push('#');
                out.push(other);
            }
        }
    }
    out
}

/// The bytes of one `send-keys` word (tmux key names): `Enter`, `Escape`, `Tab`, `BSpace`,
/// `Space`, arrows, `Home`, `End`, `PPage`/`PageUp`, `NPage`/`PageDown`, `IC`, `DC`, `BTab`,
/// `F1`–`F12`, with `C-` (Ctrl), `M-` (Meta: ESC first) and `S-` modifiers; anything else is
/// typed as the text it is (tmux does the same). xterm sequences in normal cursor mode.
pub fn key_bytes(word: &str) -> Vec<u8> {
    let (mut ctrl, mut meta, mut shift) = (false, false, false);
    let mut name = word;
    loop {
        if let Some(r) = name.strip_prefix("C-").filter(|r| !r.is_empty()) {
            ctrl = true;
            name = r;
        } else if let Some(r) = name.strip_prefix("M-").filter(|r| !r.is_empty()) {
            meta = true;
            name = r;
        } else if let Some(r) = name.strip_prefix("S-").filter(|r| !r.is_empty()) {
            shift = true;
            name = r;
        } else if let Some(r) = name.strip_prefix('^').filter(|r| r.chars().count() == 1) {
            ctrl = true;
            name = r;
        } else {
            break;
        }
    }
    let modifier = 1 + u8::from(shift) + 2 * u8::from(meta) + 4 * u8::from(ctrl);
    let csi = |c: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{n};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let base: Option<Vec<u8>> = match name {
        "Enter" => Some(b"\r".to_vec()),
        "Escape" => Some(b"\x1b".to_vec()),
        "Tab" => Some(b"\t".to_vec()),
        "BTab" => Some(b"\x1b[Z".to_vec()),
        "BSpace" => Some(b"\x7f".to_vec()),
        "Space" => Some(b" ".to_vec()),
        "Up" => Some(csi('A')),
        "Down" => Some(csi('B')),
        "Right" => Some(csi('C')),
        "Left" => Some(csi('D')),
        "Home" => Some(csi('H')),
        "End" => Some(csi('F')),
        "IC" | "Insert" => Some(tilde(2)),
        "DC" | "Delete" => Some(tilde(3)),
        "PPage" | "PageUp" | "PgUp" => Some(tilde(5)),
        "NPage" | "PageDown" | "PgDn" => Some(tilde(6)),
        f if f.starts_with('F') && f.len() <= 3 => match f[1..].parse::<u8>() {
            Ok(n @ 1..=4) => Some(if modifier > 1 {
                format!("\x1b[1;{modifier}{}", char::from(b'P' + n - 1)).into_bytes()
            } else {
                format!("\x1bO{}", char::from(b'P' + n - 1)).into_bytes()
            }),
            Ok(n @ 5..=12) => {
                let code = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
                Some(tilde(code))
            }
            _ => None,
        },
        _ => None,
    };
    let modified_named = base.is_some()
        && (ctrl || shift)
        && !matches!(name, "Enter" | "Escape" | "Tab" | "BSpace" | "Space");
    if let Some(b) = base {
        let mut out = Vec::new();
        if meta && !modified_named {
            out.push(0x1b);
        }
        out.extend(b);
        return out;
    }
    // A single character with modifiers: C-a … C-z, C-[ … ; M-x is ESC x.
    let mut cs = name.chars();
    if let (Some(c), None) = (cs.next(), cs.next())
        && (ctrl || meta)
    {
        let mut out = Vec::new();
        if meta {
            out.push(0x1b);
        }
        if ctrl {
            let u = c.to_ascii_uppercase();
            if ('@'..='_').contains(&u) {
                out.push(u as u8 & 0x1F);
            } else if c == '?' {
                out.push(0x7f);
            } else if c == ' ' {
                out.push(0);
            } else {
                out.extend(c.to_string().bytes());
            }
        } else {
            out.extend(c.to_string().bytes());
        }
        return out;
    }
    word.as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<Vec<String>> {
        split(line).unwrap()
    }

    #[test]
    fn words_quotes_escapes_semicolons_and_comments() {
        assert_eq!(words("split-window -h"), [["split-window", "-h"]]);
        assert_eq!(
            words(r#"rename-window 'my logs'  ;  new-window -n "a \"b\"" "#),
            vec![
                vec!["rename-window".to_string(), "my logs".into()],
                vec!["new-window".into(), "-n".into(), "a \"b\"".into()]
            ]
        );
        assert_eq!(
            words(r"send-keys echo\ hi\; Enter"),
            [["send-keys", "echo hi;", "Enter"]]
        );
        assert_eq!(
            words("selectp -L; selectp -R"),
            [["selectp", "-L"], ["selectp", "-R"]]
        );
        assert_eq!(words(r#"set-buffer "a\tb\n""#), [["set-buffer", "a\tb\n"]]);
        assert_eq!(words("lsp # a comment; not a command"), [["lsp"]]);
        assert_eq!(words("set-buffer 'it''s'"), [["set-buffer", "its"]]);
        assert_eq!(words("  ;;  "), Vec::<Vec<String>>::new());
        assert_eq!(split("rename-window 'open").unwrap_err().kind, "syntax");
        assert_eq!(split("x \"open").unwrap_err().kind, "syntax");
        assert_eq!(split("x \\").unwrap_err().kind, "syntax");
        // Korean and other Unicode stay as they are.
        assert_eq!(
            words("rename-window '빌드 창'"),
            [["rename-window", "빌드 창"]]
        );
    }

    #[test]
    fn commands_flags_and_aliases() {
        assert_eq!(
            parse("splitw -hd -l 30% -t work:1.0 -- cmd.exe /k echo").unwrap(),
            [Command::SplitWindow {
                target: Some("work:1.0".into()),
                horizontal: true,
                before: false,
                detached: true,
                size: Some(Size::Percent(30)),
                command: vec!["cmd.exe".into(), "/k".into(), "echo".into()],
            }]
        );
        assert_eq!(
            parse("split-window -l20 -b").unwrap(),
            [Command::SplitWindow {
                target: None,
                horizontal: false,
                before: true,
                detached: false,
                size: Some(Size::Cells(20)),
                command: vec![],
            }]
        );
        assert_eq!(
            parse("selectp -U; resize-pane -R 5; resizep -Z; neww -n build -d").unwrap(),
            [
                Command::SelectPane {
                    target: None,
                    side: Some(Side::Up)
                },
                Command::ResizePane {
                    target: None,
                    side: Some(Side::Right),
                    cells: 5,
                    zoom: false
                },
                Command::ResizePane {
                    target: None,
                    side: None,
                    cells: 1,
                    zoom: true
                },
                Command::NewWindow {
                    target: None,
                    name: Some("build".into()),
                    detached: true,
                    command: vec![]
                },
            ]
        );
        assert_eq!(
            parse("swapp -s work:0.1 -t work:0.0").unwrap()[0],
            Command::SwapPane {
                target: Some("work:0.0".into()),
                source: Some("work:0.1".into()),
                next: false
            }
        );
        assert_eq!(
            parse("joinp -h -s work:1.0 -t work:0").unwrap()[0],
            Command::JoinPane {
                source: Some("work:1.0".into()),
                target: Some("work:0".into()),
                horizontal: true,
                before: false,
                detached: false,
                size: None
            }
        );
        assert_eq!(
            parse("send -t work 'echo hi' Enter").unwrap()[0],
            Command::SendKeys {
                target: Some("work".into()),
                literal: false,
                keys: vec!["echo hi".into(), "Enter".into()]
            }
        );
        assert_eq!(parse("ls").unwrap()[0], Command::ListSessions);
        assert_eq!(parse("detach").unwrap()[0].name(), "detach-client");
        assert!(parse("kill-pane -t work:0.1").unwrap()[0].destructive());
    }

    #[test]
    fn errors_are_stable_and_never_guess() {
        let kind = |l: &str| parse(l).unwrap_err().kind;
        assert_eq!(kind(""), "empty");
        assert_eq!(kind("splt-window"), "unknown_command");
        assert_eq!(kind("split-window -q"), "unknown_flag");
        assert_eq!(kind("split-window -f"), "unsupported_flag");
        assert_eq!(kind("split-window -h -v"), "conflicting_flags");
        assert_eq!(kind("split-window -l 0"), "bad_value");
        assert_eq!(kind("split-window -l 100%"), "bad_value");
        assert_eq!(kind("split-window -t"), "missing_value");
        assert_eq!(kind("select-pane -t {last}"), "bad_target");
        assert_eq!(kind("select-window -t :+"), "bad_target");
        assert_eq!(kind("select-pane -t %3"), "bad_target");
        assert_eq!(kind("select-pane -L -R"), "conflicting_flags");
        assert_eq!(kind("resize-pane"), "missing_argument");
        assert_eq!(kind("resize-pane -L x"), "bad_value");
        assert_eq!(kind("rename-window"), "missing_argument");
        assert_eq!(kind("rename-window a b"), "missing_argument");
        assert_eq!(kind("list-panes extra"), "extra_argument");
        assert_eq!(kind("swap-pane"), "missing_argument");
        assert_eq!(kind("select-layout spiral"), "bad_value");
        assert_eq!(kind("send-keys -X cancel"), "unsupported_flag");
        let e = parse("split-window -f").unwrap_err();
        assert!(e.hint.unwrap().contains("Step 25"));
    }

    #[test]
    fn plans_use_the_context_and_ask_for_consent() {
        let ctx = Context {
            terminal: Some("t-1".into()),
            window: Some("w-1".into()),
            pane: Some("p-1".into()),
            yes: false,
        };
        let one = |l: &str| plan(&parse(l).unwrap()[0], &ctx);
        let Ok(Plan::Request { method, params }) = one("splitw -h -l 25%") else {
            panic!()
        };
        assert_eq!(method, "pane.split");
        assert_eq!(params["target"], "p-1");
        assert_eq!(params["direction"], "horizontal");
        assert_eq!(params["percent"], 25);
        let Ok(Plan::Request { method, params }) = one("neww -n build -t work") else {
            panic!()
        };
        assert_eq!(
            (method, params["target"].as_str()),
            ("window.create", Some("work"))
        );
        assert_eq!(
            one("next").unwrap(),
            Plan::Relative {
                terminal: "t-1".into(),
                delta: 1
            }
        );
        assert_eq!(one("copy-mode").unwrap(), Plan::Local("copy-mode"));
        assert_eq!(one("kill-pane").unwrap_err().kind, "confirmation_required");
        let yes = Context {
            yes: true,
            ..ctx.clone()
        };
        let Ok(Plan::Request { params, .. }) = plan(&parse("killp").unwrap()[0], &yes) else {
            panic!()
        };
        assert_eq!(params["force"], true);
        // No context and no -t: a stable error, never a guess.
        let none = Context::default();
        assert_eq!(
            plan(&parse("splitw").unwrap()[0], &none).unwrap_err().kind,
            "no_target"
        );
        assert_eq!(
            plan(&parse("select-window").unwrap()[0], &ctx)
                .unwrap_err()
                .kind,
            "no_target"
        );
        let Ok(Plan::Request { method, params }) =
            plan(&parse("send -t work:0.1 -l 'a b'").unwrap()[0], &none)
        else {
            panic!()
        };
        assert_eq!(
            (method, params["literal"].as_bool()),
            ("pane.send_keys", Some(true))
        );
        let Ok(Plan::Request { method, .. }) = plan(&parse("lsb").unwrap()[0], &none) else {
            panic!()
        };
        assert_eq!(method, "buffer.list");
    }

    #[test]
    fn step_25_commands_tokens_and_formats() {
        let ctx = Context {
            terminal: Some("t-1".into()),
            window: Some("w-1".into()),
            pane: Some("p-1".into()),
            yes: false,
        };
        let one = |l: &str| plan(&parse(l).unwrap()[0], &ctx).unwrap();
        let Plan::Request { method, params } = one("setw synchronize-panes on") else {
            panic!()
        };
        assert_eq!(
            (method, params["value"].as_bool()),
            ("window.set_option", Some(true))
        );
        let Plan::Request { params, .. } = one("set -w monitor-activity") else {
            panic!()
        };
        assert!(params["value"].is_null(), "no value toggles");
        let Plan::Request { method, params } = one("last-window") else {
            panic!()
        };
        assert_eq!(
            (method, params["target"].as_str()),
            ("window.select", Some("t-1:{last}"))
        );
        let Plan::Request { params, .. } = one("lastp") else {
            panic!()
        };
        assert_eq!(params["target"], "t-1:.{last}");
        let Plan::Request { params, .. } = one("selectl -E") else {
            panic!()
        };
        assert_eq!(params["layout"], "spread");
        let Plan::Request { params, .. } = one("nextl") else {
            panic!()
        };
        assert_eq!(params["layout"], "next");
        let Plan::Request { params, .. } = one("select-layout main-vertical") else {
            panic!()
        };
        assert_eq!(params["layout"], "main-vertical");
        assert!(matches!(
            one("display -p '#S'"),
            Plan::Format { print: true, .. }
        ));
        let Plan::Request { method, params } =
            one("set-hook after-split-window 'select-layout tiled'")
        else {
            panic!()
        };
        assert_eq!(
            (method, params["command"].as_str()),
            ("hook.set", Some("select-layout tiled"))
        );
        let Plan::Request { params, .. } = one("set-hook -u alert-bell") else {
            panic!()
        };
        assert!(params["command"].is_null());
        assert!(matches!(one("lscm"), Plan::Text(t) if t.contains("split-window (splitw)")));
        // Tokens after a Terminal pass; a bare token or tmux ids are refused.
        assert!(parse("select-window -t 'work:{last}'").is_ok());
        assert!(parse("select-pane -t work:.{left-of}").is_ok());
        assert!(parse("select-window -t work:+1").is_ok());
        assert_eq!(parse("select-window -t !").unwrap_err().kind, "bad_target");
        assert_eq!(parse("select-pane -t %3").unwrap_err().kind, "bad_target");
        assert_eq!(
            parse("setw -g monitor-bell off").unwrap_err().kind,
            "unsupported_flag"
        );
        assert_eq!(parse("setw status on").unwrap_err().kind, "bad_value");
        assert_eq!(
            parse("setw synchronize-panes maybe").unwrap_err().kind,
            "bad_value"
        );
        assert_eq!(parse("set-hook").unwrap_err().kind, "missing_argument");
        // Formats.
        let t = serde_json::json!({"id": "t-1", "name": "work", "active_window": "w-1",
        "windows": [{"id": "w-1", "index": 0, "name": "cmd", "active_pane": "p-2",
            "zoomed": null, "activity": false, "bell": true,
            "options": {"synchronize-panes": true}, "size": {"cols": 80, "rows": 24},
            "panes": [
                {"id": "p-1", "index": 0, "pid": 10, "size": {"cols": 40, "rows": 24}, "rect": {"x": 0, "y": 0}, "executable": "C:\\Windows\\cmd.exe", "running": true},
                {"id": "p-2", "index": 1, "pid": 11, "size": {"cols": 39, "rows": 24}, "rect": {"x": 41, "y": 0}, "command": {"program": "pwsh.exe"}, "running": true}
            ]}]});
        let v = vars(&t, None);
        assert_eq!(
            format(
                "#S:#I.#P #{pane_current_command} #{?pane_synchronized,sync,} ##",
                &v
            ),
            "work:0.1 pwsh sync #"
        );
        assert_eq!(format("#{window_flags}", &v), "*!");
        let v0 = vars(&t, Some("p-1"));
        assert_eq!(
            format(
                "#{pane_width}x#{pane_height} #{?pane_active,yes,no} #{nope}",
                &v0
            ),
            "40x24 no "
        );
        assert_eq!(
            format("#{pane_current_command}", &vars(&t, Some("0.0"))),
            "cmd"
        );
    }

    #[test]
    fn send_keys_names_follow_tmux() {
        assert_eq!(key_bytes("Enter"), b"\r");
        assert_eq!(key_bytes("C-c"), [3]);
        assert_eq!(key_bytes("^C"), [3]);
        assert_eq!(key_bytes("C-b"), [2]);
        assert_eq!(key_bytes("M-x"), b"\x1bx");
        assert_eq!(key_bytes("Up"), b"\x1b[A");
        assert_eq!(key_bytes("C-Up"), b"\x1b[1;5A");
        assert_eq!(key_bytes("S-Left"), b"\x1b[1;2D");
        assert_eq!(key_bytes("F1"), b"\x1bOP");
        assert_eq!(key_bytes("F5"), b"\x1b[15~");
        assert_eq!(key_bytes("C-F5"), b"\x1b[15;5~");
        assert_eq!(key_bytes("PPage"), b"\x1b[5~");
        assert_eq!(key_bytes("BSpace"), [0x7f]);
        assert_eq!(key_bytes("BTab"), b"\x1b[Z");
        assert_eq!(key_bytes("M-Enter"), b"\x1b\r");
        // Not a key name: the text itself (Unicode included).
        assert_eq!(key_bytes("echo hi"), b"echo hi");
        assert_eq!(key_bytes("한글"), "한글".as_bytes());
        assert_eq!(key_bytes("C-"), b"C-");
        assert_eq!(key_bytes("F99"), b"F99");
    }
}
