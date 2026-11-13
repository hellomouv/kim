//! The tab bar (Step 17, DESIGN_SYSTEM §3.5, docs/SUBAK_GUI.md §2): one tab per Terminal of the
//! Host that this window shows, in order, with its state, its name, a close `×` that **hides**
//! the tab (a detach; the Terminal keeps running, ADR 0020 §7) and a `+` that creates a Terminal.
//!
//! The bar is one row of cells drawn by the window's own renderer (the terminal font chain, so a
//! Korean name looks as it does in the pane), from VT text built here. This module is pure: it
//! lays the tabs out in columns, scrolls them when they do not fit (the active tab always stays
//! visible, `‹`/`›` say that more tabs are hidden on that side), and returns the clickable spans,
//! so the layout is unit-tested without a window. State is never colour-only (DESIGN_SYSTEM
//! §3.8): every state has its own shape, the active tab is bold with its own fill (and a lilac
//! marker drawn by the window), and High Contrast turns fills into reverse video.

use super::screens::tokens::{
    ACCENT, BG_BASE, BG_HOVER, BG_OVERLAY, BORDER_SUBTLE, DANGER, SUCCESS, TEXT_MUTED,
    TEXT_PRIMARY, TEXT_SECONDARY, WARNING,
};
use subak_term::render::Rgb;

/// A tab's Terminal state, as the bar shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Starting,
    Stopping,
    /// Kept by the Host without a process (`stopped`).
    Stopped,
    /// Kept after its start failed.
    Failed,
    /// No longer in the Host (exited or removed); only the shown tab stays, with its last screen.
    Gone,
    /// Not known while the window is disconnected from the Host.
    Unknown,
}

impl Status {
    /// The bar's status from a Host state (`terminal.list`, events).
    pub fn from_state(state: &str) -> Self {
        match state {
            "running" => Self::Running,
            "starting" => Self::Starting,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            "failed" => Self::Failed,
            "removed" | "exited" | "gone" => Self::Gone,
            _ => Self::Unknown,
        }
    }

    /// The shape (DESIGN_SYSTEM §3.1: shape and colour, never colour alone).
    pub fn glyph(self) -> char {
        match self {
            Self::Running => '●',
            Self::Starting | Self::Stopping => '◐',
            Self::Stopped => '■',
            Self::Failed => '▲',
            Self::Gone | Self::Unknown => '○',
        }
    }

    /// The state in words (test reports, the context menu).
    pub fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Gone => "gone",
            Self::Unknown => "unknown",
        }
    }

    /// The state's colour (always beside the shape).
    pub fn color(self) -> Rgb {
        match self {
            Self::Running => SUCCESS,
            Self::Starting | Self::Stopping => WARNING,
            Self::Failed => DANGER,
            Self::Stopped | Self::Gone | Self::Unknown => TEXT_MUTED,
        }
    }
}

/// One tab to lay out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<'a> {
    pub name: &'a str,
    pub status: Status,
    /// Step 28: the Terminal's Claude Code state (`None`: no mark).
    pub claude: Claude,
    /// Step 44: the mark's current motion frame (`None`: the state's resting glyph).
    pub frame: Option<crate::gui::claude_motion::Frame>,
    /// #98 T3: output or a bell since the tab was last shown.
    pub alert: Option<crate::gui::model::Alert>,
    /// #98 T3: the progress the Terminal's program reports.
    pub progress: Option<subak_term::screen::Progress>,
    /// #98 T1 (`CORE-14`): pinned: a `◆` mark and no hide button.
    pub pinned: bool,
    /// #98 T1 (`CORE-10`, `tabs.close_buttons`): the tab has its `×` (hide) button.
    pub closable: bool,
}

/// #98 T3 (`CORE-78`): a tab's progress: `42%` (accent), `42%!` (failed, danger), `‖42%`
/// (paused, warning) or `…` (busy without a value); text and shape, never colour alone.
pub fn progress_mark(p: Option<subak_term::screen::Progress>) -> Option<(String, Rgb)> {
    use subak_term::screen::ProgressState as S;
    let p = p?;
    Some(match p.state {
        S::Normal => (format!("{}%", p.value), ACCENT),
        S::Error => (format!("{}%!", p.value), DANGER),
        S::Paused => (format!("\u{2016}{}%", p.value), WARNING),
        S::Indeterminate => ("\u{2026}".to_string(), ACCENT),
    })
}

/// #98 T1 (`CORE-23`, `tabs.title`): a tab's text from the format: `{name}`, `{title}` (what
/// the program set), `{program}`, `{cwd}` (the folder's name). An empty result falls back to
/// the name, so a tab is never blank.
pub fn label(
    format: &str,
    name: &str,
    title: Option<&str>,
    program: &str,
    cwd: Option<&str>,
) -> String {
    let folder = cwd
        .map(|c| c.trim_end_matches(['\\', '/']))
        .and_then(|c| c.rsplit(['\\', '/']).next())
        .unwrap_or("");
    let text = format
        .replace("{name}", name)
        .replace("{title}", title.unwrap_or(""))
        .replace("{program}", program)
        .replace("{cwd}", folder);
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let text = text.trim();
    if text.is_empty() {
        name.to_string()
    } else {
        text.to_string()
    }
}

/// #98 T3: the mark of a tab with unseen output (`•`) or a bell (`♪`), a shape each.
pub fn alert_mark(a: Option<crate::gui::model::Alert>) -> Option<(char, Rgb, &'static str)> {
    use crate::gui::model::Alert;
    Some(match a? {
        Alert::Output => ('•', ACCENT, "new output"),
        Alert::Bell => ('♪', WARNING, "bell"),
    })
}

/// The columns of a tab's marks (Claude and alert), each with its space.
fn marks_width(it: &Item<'_>) -> usize {
    mark_width(it.claude)
        + if it.pinned { 2 } else { 0 }
        + alert_mark(it.alert).map_or(0, |(g, _, _)| width(&g.to_string()) + 1)
        + progress_mark(it.progress).map_or(0, |(t, _)| width(&t) + 1)
}

pub use crate::gui::float::art::Claude;

/// Step 28: the tab's Claude mark — a shape and a colour per state (never colour alone,
/// DESIGN_SYSTEM §3.1) — and its words for the title, menus and tests.
pub fn claude_mark(c: Claude) -> Option<(char, Rgb, &'static str)> {
    Some(match c {
        Claude::NeedInput => ('!', WARNING, "Claude needs input"),
        Claude::Error => ('✕', DANGER, "Claude error"),
        Claude::Done => ('✓', SUCCESS, "Claude done"),
        Claude::InProgress => ('◔', ACCENT, "Claude working"),
        Claude::Unknown => ('?', TEXT_MUTED, "Claude state unknown"),
        Claude::Idle => ('◦', TEXT_MUTED, "Claude idle"),
        Claude::None => return None,
    })
}

/// The columns a tab's Claude mark takes (the glyph and a space).
fn mark_width(c: Claude) -> usize {
    claude_mark(c).map_or(0, |(g, _, _)| width(&g.to_string()) + 1)
}

/// What a click on a column of the bar means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The tab: select it (double click: rename; middle click: hide; right click: its menu).
    Tab(usize),
    /// The tab's `×`: hide the tab (a detach; the Terminal keeps running).
    Close(usize),
    /// `+`: a new Terminal.
    New,
    /// `‹` / `›`: scroll the tabs.
    Left,
    Right,
}

/// The rename field shown in place of the active tab's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit<'a> {
    pub text: &'a str,
    /// The caret, in characters from the start of `text`.
    pub cursor: usize,
}

/// A laid-out bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// VT text for a one-row view of the bar's width.
    pub vt: Vec<u8>,
    /// Clickable column ranges `[start, end)`.
    pub spans: Vec<(u16, u16, Hit)>,
    /// The active tab's columns (the window fills it to the pane and draws the marker above it).
    pub active: Option<(u16, u16)>,
    /// The rename caret's column, while renaming.
    pub caret: Option<u16>,
    /// The first visible tab.
    pub first: usize,
}

impl Bar {
    /// What a click at `col` means.
    pub fn hit(&self, col: u16) -> Option<Hit> {
        self.spans
            .iter()
            .find(|(a, b, _)| (*a..*b).contains(&col))
            .map(|s| s.2)
    }
}

/// Tab widths in cells (DESIGN_SYSTEM §2.4: 96–220 px at the UI size).
pub const TAB_MIN: usize = 12;
pub const TAB_MAX: usize = 28;
/// The rename field's widest extent.
const EDIT_MAX: usize = 40;

fn width(s: &str) -> usize {
    s.chars()
        .map(|c| usize::from(subak_term::screen::char_width(c)))
        .sum()
}

/// `s` cut to `max` cells, with `…` when cut.
fn fit(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = usize::from(subak_term::screen::char_width(c));
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn fg(c: Rgb) -> String {
    format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2)
}

fn bg(c: Rgb) -> String {
    format!("\x1b[48;2;{};{};{}m", c.0, c.1, c.2)
}

const RESET: &str = "\x1b[0m";

/// The rename field: the visible part of `text` around the caret, the caret's cell offset, and
/// the field's width in cells.
fn edit_window(e: &Edit<'_>) -> (String, usize, usize) {
    let chars: Vec<char> = e.text.chars().collect();
    let cursor = e.cursor.min(chars.len());
    let cw = |c: &char| usize::from(subak_term::screen::char_width(*c));
    let total: usize = chars.iter().map(cw).sum::<usize>() + 1;
    let field = total.clamp(8, EDIT_MAX);
    // Show the text so that the caret (one cell after `cursor` chars) fits in `field` cells.
    let mut start = 0;
    while chars[start..cursor].iter().map(cw).sum::<usize>() + 1 > field {
        start += 1;
    }
    let mut shown = String::new();
    let mut w = 0;
    for c in &chars[start..] {
        if w + cw(c) > field {
            break;
        }
        shown.push(*c);
        w += cw(c);
    }
    let caret = chars[start..cursor].iter().map(cw).sum::<usize>();
    (shown, caret, field)
}

/// The width in cells of tab `i`.
fn tab_width(it: &Item<'_>, edit: Option<&Edit<'_>>) -> usize {
    match edit {
        Some(e) => edit_window(e).2 + 6 + marks_width(it),
        None => (width(it.name) + 6).clamp(TAB_MIN, TAB_MAX) + marks_width(it),
    }
}

/// Lays out `items` on a bar of `cols` cells. `active` is the shown tab, `edit` the rename field
/// (of the active tab), `first` the first visible tab of the last layout (kept while the active
/// tab stays visible).
pub fn layout(
    items: &[Item<'_>],
    active: Option<usize>,
    edit: Option<&Edit<'_>>,
    cols: u16,
    first: usize,
) -> Bar {
    layout_hover(items, active, edit, cols, first, None)
}

/// [`layout`] with the tab under the pointer (Step 20): an inactive hovered tab gets the
/// `bg.hover` fill and its `×` the secondary text colour, so the hidden-tab action is easier to
/// see before it is clicked. The columns and spans are the same as without a hover.
pub fn layout_hover(
    items: &[Item<'_>],
    active: Option<usize>,
    edit: Option<&Edit<'_>>,
    cols: u16,
    first: usize,
    hover: Option<usize>,
) -> Bar {
    let cols = usize::from(cols);
    let widths: Vec<usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| tab_width(it, edit.filter(|_| Some(i) == active)))
        .collect();
    const PLUS: usize = 3;
    const ARROW: usize = 2;
    let total: usize = widths.iter().sum::<usize>() + PLUS;
    // Which tabs are visible.
    let (first, last) = if total <= cols || items.is_empty() {
        (0, items.len())
    } else {
        let avail = cols.saturating_sub(PLUS + 2 * ARROW);
        let mut first = first.min(items.len() - 1);
        if let Some(a) = active {
            first = first.min(a);
            while first < a && widths[first..=a].iter().sum::<usize>() > avail {
                first += 1;
            }
        }
        let mut last = first;
        let mut used = 0;
        while last < items.len() && used + widths[last] <= avail.max(widths[first]) {
            used += widths[last];
            last += 1;
        }
        (first, last.max(first + 1))
    };
    let mut vt = String::from("\x1b[?25l\x1b[2J\x1b[1;1H");
    let mut spans = Vec::new();
    let mut col = 0usize;
    let mut active_cols = None;
    let mut caret = None;
    let put = |vt: &mut String, col: &mut usize, text: &str| {
        vt.push_str(text);
        *col += width(&strip(text));
    };
    if first > 0 {
        let at = col;
        put(
            &mut vt,
            &mut col,
            &format!("{}‹ {RESET}", fg(TEXT_SECONDARY)),
        );
        spans.push((cell(at), cell(col), Hit::Left));
    }
    for i in first..last {
        let it = &items[i];
        let w = widths[i];
        let start = col;
        let is_active = Some(i) == active;
        let hovered = !is_active && Some(i) == hover;
        let fill = if is_active {
            bg(BG_BASE)
        } else if hovered {
            bg(BG_HOVER)
        } else {
            String::new()
        };
        // A hairline between two inactive tabs.
        let lead = if !is_active && i > first && Some(i - 1) != active {
            format!("{}│", fg(BORDER_SUBTLE))
        } else {
            " ".to_string()
        };
        put(&mut vt, &mut col, &format!("{fill}{lead}"));
        put(
            &mut vt,
            &mut col,
            &format!("{fill}{}{} ", fg(it.status.color()), it.status.glyph()),
        );
        if let Some((g, c, _)) = claude_mark(it.claude) {
            // Step 44: the frame's glyph (same width as the resting one), faint as SGR 2 so
            // every theme's colour mapping still applies.
            let (g, faint) = it.frame.map_or((g, false), |f| (f.glyph, f.faint));
            let (on, off) = if faint {
                ("\x1b[2m", "\x1b[22m")
            } else {
                ("", "")
            };
            put(&mut vt, &mut col, &format!("{fill}{}{on}{g}{off} ", fg(c)));
        }
        if it.pinned {
            put(&mut vt, &mut col, &format!("{fill}{}\u{25c6} ", fg(ACCENT)));
        }
        if let Some((g, c, _)) = alert_mark(it.alert) {
            put(&mut vt, &mut col, &format!("{fill}{}{g} ", fg(c)));
        }
        if let Some((t, c)) = progress_mark(it.progress) {
            put(&mut vt, &mut col, &format!("{fill}{}{t} ", fg(c)));
        }
        match edit.filter(|_| is_active) {
            Some(e) => {
                let (shown, at, field) = edit_window(e);
                caret = Some(cell(col + at));
                let mut s = format!("{}{}\x1b[4m", bg(BG_OVERLAY), fg(TEXT_PRIMARY));
                let mut w = 0;
                for (k, c) in shown.chars().enumerate() {
                    let before: usize = shown.chars().take(k).map(|c| width(&c.to_string())).sum();
                    if before == at {
                        s.push_str(&format!("\x1b[7m{c}\x1b[27m"));
                    } else {
                        s.push(c);
                    }
                    w += width(&c.to_string());
                }
                if at >= w {
                    s.push_str("\x1b[7m \x1b[27m");
                    w += 1;
                }
                s.push_str(&" ".repeat(field.saturating_sub(w)));
                s.push_str(RESET);
                put(&mut vt, &mut col, &s);
            }
            None => {
                let room = w - 6 - marks_width(it);
                let name = fit(it.name, room);
                let pad = " ".repeat(room.saturating_sub(width(&name)));
                let style = if is_active {
                    format!("\x1b[1m{}", fg(TEXT_PRIMARY))
                } else {
                    fg(TEXT_SECONDARY)
                };
                put(
                    &mut vt,
                    &mut col,
                    &format!("{fill}{style}{name}{pad}{RESET}"),
                );
            }
        }
        let close_at = col;
        let closable = it.closable && !it.pinned;
        put(
            &mut vt,
            &mut col,
            &format!(
                "{fill}{} {} {RESET}",
                fg(if hovered { TEXT_SECONDARY } else { TEXT_MUTED }),
                if closable { '×' } else { ' ' }
            ),
        );
        if closable {
            spans.push((cell(start), cell(close_at), Hit::Tab(i)));
            spans.push((cell(close_at), cell(col), Hit::Close(i)));
        } else {
            spans.push((cell(start), cell(col), Hit::Tab(i)));
        }
        if is_active {
            active_cols = Some((cell(start), cell(col)));
        }
    }
    if last < items.len() {
        let at = col;
        put(
            &mut vt,
            &mut col,
            &format!("{} ›{RESET}", fg(TEXT_SECONDARY)),
        );
        spans.push((cell(at), cell(col), Hit::Right));
    }
    if col + PLUS <= cols.max(PLUS) {
        let at = col;
        put(&mut vt, &mut col, &format!("{} + {RESET}", fg(ACCENT)));
        spans.push((cell(at), cell(col), Hit::New));
    }
    Bar {
        vt: vt.into_bytes(),
        spans,
        active: active_cols,
        caret,
        first,
    }
}

fn cell(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// The printable text of a VT fragment (escape sequences removed), to count its cells.
fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use subak_term::TermView;

    fn text(bar: &Bar, cols: u16) -> String {
        let mut v = TermView::new(cols, 1, 0);
        v.feed(&bar.vt);
        v.row_at(0).text().trim_end().to_string()
    }

    fn items<'a>(names: &[&'a str]) -> Vec<Item<'a>> {
        names
            .iter()
            .map(|n| Item {
                name: n,
                status: Status::Running,
                claude: Claude::None,
                frame: None,
                alert: None,
                progress: None,
                pinned: false,
                closable: true,
            })
            .collect()
    }

    /// The cells of `[a, b)` as text.
    fn cells(line: &str, a: u16, b: u16) -> String {
        let mut out = String::new();
        let mut col = 0u16;
        for c in line.chars() {
            if col >= a && col < b {
                out.push(c);
            }
            col += u16::from(subak_term::screen::char_width(c));
        }
        out
    }

    /// Step 28: a tab's Claude mark has its own shape per state, sits after the status, and
    /// keeps the name and the spans in place.
    #[test]
    fn a_motion_frame_replaces_the_glyph_at_the_same_width_and_fades_with_sgr_2() {
        use crate::gui::claude_motion::{Frame, SPIN};
        let mut list = items(&["agent"]);
        list[0].claude = Claude::InProgress;
        let rest = layout(&list, Some(0), None, 80, 0);
        for g in SPIN {
            list[0].frame = Some(Frame {
                glyph: g,
                faint: false,
            });
            let b = layout(&list, Some(0), None, 80, 0);
            assert!(text(&b, 80).contains(g), "{}", text(&b, 80));
            assert_eq!(b.spans, rest.spans, "a frame never moves the tabs");
        }
        list[0].claude = Claude::NeedInput;
        list[0].frame = Some(Frame {
            glyph: '!',
            faint: true,
        });
        assert!(
            String::from_utf8_lossy(&layout(&list, Some(0), None, 80, 0).vt).contains("[2m![22m")
        );
    }

    #[test]
    fn claude_marks_are_distinct_shapes_beside_the_status() {
        let states = [
            Claude::NeedInput,
            Claude::Error,
            Claude::Done,
            Claude::InProgress,
            Claude::Unknown,
            Claude::Idle,
        ];
        let glyphs: Vec<char> = states.iter().map(|c| claude_mark(*c).unwrap().0).collect();
        let mut unique = glyphs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), states.len(), "never colour alone: {glyphs:?}");
        assert!(claude_mark(Claude::None).is_none());
        assert!(
            states
                .iter()
                .all(|c| claude_mark(*c).unwrap().2.starts_with("Claude "))
        );
        let mut list = items(&["agent", "other"]);
        list[0].claude = Claude::NeedInput;
        let bar = layout(&list, Some(1), None, 120, 0);
        let t = text(&bar, 120);
        assert!(t.contains("● ! agent"), "{t:?}");
        assert!(t.contains("● other"), "no mark without a state: {t:?}");
        let plain = layout(&items(&["agent", "other"]), Some(1), None, 120, 0);
        let w = |b: &Bar| {
            b.spans
                .iter()
                .find(|s| s.2 == Hit::Tab(0))
                .map(|s| s.1 - s.0)
                .unwrap()
        };
        assert_eq!(w(&bar), w(&plain) + 2, "the mark adds its own columns");
    }

    /// Step 20: a hovered inactive tab gets `bg.hover` and a brighter `×`; the text and the
    /// clickable spans do not move, and the active tab is not changed by a hover.
    #[test]
    fn a_hovered_tab_is_filled_without_moving_anything() {
        let list = items(&["one", "two", "three"]);
        let plain = layout(&list, Some(0), None, 120, 0);
        let hover = layout_hover(&list, Some(0), None, 120, 0, Some(1));
        assert_eq!(text(&plain, 120), text(&hover, 120));
        assert_eq!(plain.spans, hover.spans);
        let mut v = TermView::new(120, 1, 0);
        v.feed(&hover.vt);
        let (a, b) = hover
            .spans
            .iter()
            .find(|s| s.2 == Hit::Tab(1))
            .map(|s| (s.0, s.1))
            .unwrap();
        let bg_of = |x: u16| v.row_at(0).cell(usize::from(x)).attrs.bg;
        let hovered = subak_term::screen::Color::Rgb(BG_HOVER.0, BG_HOVER.1, BG_HOVER.2);
        assert!((a..b).all(|x| bg_of(x) == hovered));
        assert_ne!(bg_of(1), hovered, "the active tab keeps its fill");
        assert_eq!(
            layout_hover(&list, Some(1), None, 120, 0, Some(1)).vt,
            layout(&list, Some(1), None, 120, 0).vt,
            "hovering the active tab changes nothing"
        );
    }

    #[test]
    fn no_tabs_is_just_the_new_button() {
        let bar = layout(&[], None, None, 80, 0);
        assert_eq!(text(&bar, 80).trim(), "+");
        assert_eq!(bar.spans.len(), 1);
        assert_eq!(bar.hit(1), Some(Hit::New));
        assert_eq!(bar.active, None);
    }

    #[test]
    fn one_and_many_tabs_show_state_name_close_and_plus() {
        let mut list = items(&["빌드", "claude", "server"]);
        list[2].status = Status::Stopped;
        let bar = layout(&list, Some(1), None, 120, 0);
        let line = text(&bar, 120);
        assert!(line.contains("● 빌드"), "{line}");
        assert!(line.contains("● claude"), "{line}");
        assert!(line.contains("■ server"), "{line}");
        assert_eq!(line.matches('×').count(), 3, "{line}");
        assert!(line.ends_with('+'), "{line}");
        // The spans tile the bar from column 0 and name what is under them.
        let mut at = 0;
        for (a, b, hit) in &bar.spans {
            assert_eq!(*a, at, "{:?}", bar.spans);
            at = *b;
            let under = cells(&line, *a, *b);
            match hit {
                Hit::Tab(i) => assert!(under.contains(list[*i].name), "{under}"),
                Hit::Close(_) => assert_eq!(under.trim(), "×"),
                Hit::New => assert_eq!(under.trim(), "+"),
                Hit::Left | Hit::Right => panic!("no scrolling"),
            }
        }
        let (a, b) = bar.active.unwrap();
        assert!(cells(&line, a, b).contains("claude"));
        assert!((TAB_MIN..=TAB_MAX).contains(&usize::from(b - a)));
        // Every state has its own shape.
        let shapes: std::collections::HashSet<char> = [
            Status::Running,
            Status::Starting,
            Status::Stopped,
            Status::Failed,
            Status::Gone,
        ]
        .iter()
        .map(|s| s.glyph())
        .collect();
        assert_eq!(shapes.len(), 5);
        assert_eq!(Status::from_state("stopped"), Status::Stopped);
        assert_eq!(Status::from_state("bogus"), Status::Unknown);
    }

    #[test]
    fn long_names_are_cut_and_too_many_tabs_scroll_with_the_active_one_visible() {
        let long = "a-very-long-terminal-name-that-does-not-fit-in-a-tab";
        let bar = layout(&items(&[long]), Some(0), None, 120, 0);
        let line = text(&bar, 120);
        assert!(line.contains('…'), "{line}");
        let names: Vec<String> = (0..30).map(|i| format!("term-{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let list = items(&refs);
        for active in [0usize, 7, 15, 29] {
            let bar = layout(&list, Some(active), None, 80, 0);
            let line = text(&bar, 80);
            assert!(
                line.contains(&format!("term-{active:02}")),
                "active {active}: {line}"
            );
            assert!(width(&line) <= 80, "{line}");
            assert_eq!(bar.first > 0, line.starts_with('‹'), "{line}");
            let more = bar.spans.iter().any(|s| s.2 == Hit::Right);
            assert_eq!(more, active < 29, "{line}");
        }
        // The previous first tab is kept while the active one stays visible.
        let bar = layout(&list, Some(8), None, 80, 5);
        assert_eq!(bar.first, 5);
        // A very narrow bar still shows the active tab.
        let bar = layout(&list, Some(12), None, 20, 0);
        assert!(text(&bar, 20).contains("term-1"), "{}", text(&bar, 20));
    }

    #[test]
    fn the_rename_field_replaces_the_active_name_with_a_caret() {
        let list = items(&["one", "two"]);
        let e = Edit {
            text: "새이름",
            cursor: 1,
        };
        let bar = layout(&list, Some(1), Some(&e), 80, 0);
        let line = text(&bar, 80);
        assert!(line.contains("새이름"), "{line}");
        assert!(!line.contains("two"), "{line}");
        let (a, _) = bar.active.unwrap();
        // " ● " then the field: the caret is after the first (wide) syllable.
        assert_eq!(bar.caret, Some(a + 3 + 2));
        // The caret at the end sits after the text.
        let e = Edit {
            text: "ab",
            cursor: 2,
        };
        let bar = layout(&list, Some(0), Some(&e), 80, 0);
        let (a, _) = bar.active.unwrap();
        assert_eq!(bar.caret, Some(a + 3 + 2));
        // A long text scrolls so the caret stays in the field.
        let long = "x".repeat(100);
        let e = Edit {
            text: &long,
            cursor: 100,
        };
        let bar = layout(&list, Some(0), Some(&e), 120, 0);
        let (a, b) = bar.active.unwrap();
        let caret = bar.caret.unwrap();
        assert!(caret > a && caret < b, "{a} {caret} {b}");
    }

    /// #98 T3: an unseen tab shows `•` for output and `♪` for a bell, beside its status, and
    /// the tab grows to keep the whole name.
    #[test]
    fn alert_marks_show_beside_the_status() {
        use crate::gui::model::Alert;
        let mut list = items(&["build", "logs", "shell"]);
        list[1].alert = Some(Alert::Output);
        list[2].alert = Some(Alert::Bell);
        let bar = layout(&list, Some(0), None, 120, 0);
        let line = text(&bar, 120);
        assert!(line.contains("● • logs"), "{line}");
        assert!(line.contains("● ♪ shell"), "{line}");
        assert!(
            !line.contains("● • build") && !line.contains("● ♪ build"),
            "{line}"
        );
        assert_eq!(alert_mark(Some(Alert::Bell)).unwrap().2, "bell");
        assert!(alert_mark(None).is_none());
        let plain = tab_width(&items(&["logs"])[0], None);
        assert_eq!(tab_width(&list[1], None), plain + 2);
    }

    /// #98 T3 (`CORE-78`): the tab shows its program's progress in text (`42%`, `7%!`,
    /// `‖9%`, `…`).
    #[test]
    fn progress_shows_on_the_tab() {
        use subak_term::screen::{Progress, ProgressState};
        let p = |state, value| Some(Progress { state, value });
        let mut list = items(&["a", "b", "c", "d"]);
        list[0].progress = p(ProgressState::Normal, 42);
        list[1].progress = p(ProgressState::Error, 7);
        list[2].progress = p(ProgressState::Paused, 9);
        list[3].progress = p(ProgressState::Indeterminate, 0);
        let line = text(&layout(&list, Some(0), None, 160, 0), 160);
        for want in ["● 42% a", "● 7%! b", "● \u{2016}9% c", "● \u{2026} d"] {
            assert!(line.contains(want), "{want} in {line}");
        }
        assert_eq!(progress_mark(None), None);
        assert_eq!(progress_mark(p(ProgressState::Error, 7)).unwrap().1, DANGER);
    }

    /// #98 T1 (`CORE-14`, `CORE-10`): a pinned tab shows `◆` and no `×`; with close buttons
    /// off no tab has one; a click there selects the tab.
    #[test]
    fn pinned_tabs_and_tabs_without_close_buttons() {
        let mut list = items(&["build", "logs"]);
        list[0].pinned = true;
        let bar = layout(&list, Some(1), None, 120, 0);
        let line = text(&bar, 120);
        assert!(line.contains("● \u{25c6} build"), "{line}");
        assert_eq!(line.matches('×').count(), 1, "only logs has one: {line}");
        assert!(!bar.spans.iter().any(|s| s.2 == Hit::Close(0)));
        for it in &mut list {
            it.closable = false;
        }
        let line = text(&layout(&list, Some(1), None, 120, 0), 120);
        assert_eq!(line.matches('×').count(), 0, "{line}");
    }

    /// #98 T1 (`CORE-23`): tab title formats; never a blank tab, never a control character.
    #[test]
    fn tab_titles_follow_the_format() {
        assert_eq!(
            label("{name}", "build", Some("make"), "cmd.exe", None),
            "build"
        );
        assert_eq!(
            label(
                "{name}: {title}",
                "build",
                Some("make -j8"),
                "cmd.exe",
                None
            ),
            "build: make -j8"
        );
        assert_eq!(
            label(
                "{program} in {cwd}",
                "x",
                None,
                "pwsh.exe",
                Some(r"C:\work\kim\")
            ),
            "pwsh.exe in kim"
        );
        assert_eq!(
            label("{title}", "build", None, "cmd.exe", None),
            "build",
            "falls back"
        );
        assert_eq!(label("{title}", "b", Some("a\u{7}b"), "c", None), "ab");
    }
}
