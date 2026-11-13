//! Keyboard dispatch of the window (ADR 0009 order, docs/SUBAK_GUI.md §3): the IME guard, then
//! the window's own shortcuts, then everything else to the pane in the xterm encoding of
//! `subak-term`.
//!
//! The shortcuts are Subak's actions of the Kim hotkey contract (docs/HOTKEY_CONTRACT.md §13.1):
//! the shipped manifest ([`crate::hotkeys`]) layered with the user's `hotkeys.json`, matched with
//! `kim_hotkeys::keys::steps_overlap` like Kim Manager does (backend `focus` inside the window):
//! logical keys (the layout's virtual key), physical `code:` keys (the scan code) and `char:`
//! keys (the character the layout produces), sequences (a pending step waits without a timer;
//! `Esc`, a click or focus loss cancels it; an unbound next step is swallowed), and the `press`,
//! `release` and `hold` triggers. Bindings of the `terminal` context win over `app` bindings
//! with the same keys (§9.1). A key that is a shortcut never reaches the program — not even its
//! auto-repeat (`repeat: false` swallows it) — and a key that is not one always does
//! (`SUBAK-FR-034`). The `Ctrl+B` prefix and its pass-through are Step 23's: until then
//! `Ctrl+B` goes to the program, and a shadowed key such as `Ctrl+W` reaches the program only
//! after `subak hotkeys disable` or a rebind.

use crate::hotkeys as sh;
use kim_hotkeys::Scope;
use kim_hotkeys::check::{ToolState, analyze};
use kim_hotkeys::keys::{CODE_KEYS, Chord, Key as HKey, Mods as HMods, NAMED_KEYS, Step};
use kim_hotkeys::manifest::{BindingSpec, Trigger};
use kim_hotkeys::store::{self, ConfigState, Loaded};
use std::path::{Path, PathBuf};
use subak_term::{Key, Mods};

/// A window action a shortcut runs (an implemented action of Subak's manifest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    NewTerminal,
    CloseTerminal,
    NextTab,
    PreviousTab,
    HideTab,
    MoveTabLeft,
    MoveTabRight,
    RenameTerminal,
    Copy,
    Paste,
    /// Step 19: copy with colours, paste as one line, select all, keyboard selection, search,
    /// save the output.
    CopyHtml,
    PasteOneLine,
    SelectAll,
    MarkMode,
    FindOpen,
    SaveOutput,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollPageUp,
    ScrollPageDown,
    ScrollLineUp,
    ScrollLineDown,
    ScrollTop,
    ScrollBottom,
    /// The Terminal manager's actions (context `manager`, Step 18).
    ManagerOpen,
    ManagerNew,
    ManagerRename,
    ManagerDelete,
    ManagerSearch,
    ManagerPin,
    ManagerClose,
    /// Step 22: splits and the layout of the shown window (context `terminal`).
    PaneSplitRight,
    PaneSplitDown,
    PaneFocusLeft,
    PaneFocusRight,
    PaneFocusUp,
    PaneFocusDown,
    PaneResizeLeft,
    PaneResizeRight,
    PaneResizeUp,
    PaneResizeDown,
    PaneZoom,
    PaneClose,
    /// Step 23: the prefix table's window, help, detach and literal-prefix actions.
    HelpShow,
    SendPrefix,
    WindowNew,
    WindowNext,
    WindowPrevious,
    /// `window.select-0` … `window.select-9`.
    WindowSelect(u8),
    WindowRename,
    WindowKill,
    ClientDetach,
    /// Step 23: more of tmux's prefix table, on Step 22's layout methods.
    PaneNext,
    PaneSwapPrevious,
    PaneSwapNext,
    WindowRotate,
    WindowRotateBack,
    LayoutEvenHorizontal,
    LayoutEvenVertical,
    LayoutTiled,
    /// Step 25.
    LayoutMainHorizontal,
    LayoutMainVertical,
    LayoutNext,
    WindowLast,
    PaneLast,
    /// Step 33.
    NewFromProfile,
    LayoutMainHorizontalMirrored,
    LayoutMainVerticalMirrored,
    LayoutSpread,
    WindowInfo,
    PaneResizeUp5,
    PaneResizeDown5,
    PaneResizeLeft5,
    PaneResizeRight5,
    Refresh,
    /// Step 24: the command prompt, copy mode and paste buffers.
    CommandPrompt,
    CopyEnter,
    BufferPaste,
    CopyExit,
    CopyBeginSelection,
    CopyCopySelection,
    PaneBreak,
    BufferList,
    BufferDelete,
    CopyPageUp,
    /// #98 T2.
    InsertNewline,
    /// #98 T2.
    LineHome,
    /// #98 T2.
    LineEnd,
    /// #98 T2.
    LinePreviousWord,
    /// #98 T2.
    LineNextWord,
    /// #98 T2.
    LineDeletePreviousWord,
    /// #98 T2.
    LineDeleteLine,
}

impl Action {
    /// Every action with its manifest ID, in manifest order ([`crate::hotkeys::IMPLEMENTED`]).
    pub const ALL: [(Action, &'static str); 102] = [
        (Action::NewTerminal, "terminal.new"),
        (Action::CloseTerminal, "terminal.close"),
        (Action::NextTab, "tab.next"),
        (Action::PreviousTab, "tab.previous"),
        (Action::HideTab, "tab.hide"),
        (Action::MoveTabLeft, "tab.move-left"),
        (Action::MoveTabRight, "tab.move-right"),
        (Action::HelpShow, "help.show"),
        (Action::SendPrefix, "input.send-prefix"),
        (Action::WindowNew, "window.new"),
        (Action::WindowNext, "window.next"),
        (Action::WindowPrevious, "window.previous"),
        (Action::WindowSelect(0), "window.select-0"),
        (Action::WindowSelect(1), "window.select-1"),
        (Action::WindowSelect(2), "window.select-2"),
        (Action::WindowSelect(3), "window.select-3"),
        (Action::WindowSelect(4), "window.select-4"),
        (Action::WindowSelect(5), "window.select-5"),
        (Action::WindowSelect(6), "window.select-6"),
        (Action::WindowSelect(7), "window.select-7"),
        (Action::WindowSelect(8), "window.select-8"),
        (Action::WindowSelect(9), "window.select-9"),
        (Action::WindowRename, "window.rename"),
        (Action::WindowKill, "window.kill"),
        (Action::PaneSplitRight, "pane.split-right"),
        (Action::PaneSplitDown, "pane.split-down"),
        (Action::PaneFocusUp, "pane.focus-up"),
        (Action::PaneFocusDown, "pane.focus-down"),
        (Action::PaneFocusLeft, "pane.focus-left"),
        (Action::PaneFocusRight, "pane.focus-right"),
        (Action::PaneResizeUp, "pane.resize-up"),
        (Action::PaneResizeDown, "pane.resize-down"),
        (Action::PaneResizeLeft, "pane.resize-left"),
        (Action::PaneResizeRight, "pane.resize-right"),
        (Action::PaneZoom, "pane.zoom"),
        (Action::PaneClose, "pane.kill"),
        (Action::PaneNext, "pane.next"),
        (Action::PaneSwapPrevious, "pane.swap-previous"),
        (Action::PaneSwapNext, "pane.swap-next"),
        (Action::WindowRotate, "window.rotate"),
        (Action::WindowRotateBack, "window.rotate-back"),
        (Action::LayoutEvenHorizontal, "layout.even-horizontal"),
        (Action::LayoutEvenVertical, "layout.even-vertical"),
        (Action::LayoutTiled, "layout.tiled"),
        (Action::NewFromProfile, "terminal.new-from"),
        (
            Action::LayoutMainHorizontalMirrored,
            "layout.main-horizontal-mirrored",
        ),
        (
            Action::LayoutMainVerticalMirrored,
            "layout.main-vertical-mirrored",
        ),
        (Action::LayoutSpread, "layout.spread"),
        (Action::WindowInfo, "window.info"),
        (Action::LayoutMainHorizontal, "layout.main-horizontal"),
        (Action::LayoutMainVertical, "layout.main-vertical"),
        (Action::LayoutNext, "layout.next"),
        (Action::WindowLast, "window.last"),
        (Action::PaneLast, "pane.last"),
        (Action::PaneResizeUp5, "pane.resize-up-5"),
        (Action::PaneResizeDown5, "pane.resize-down-5"),
        (Action::PaneResizeLeft5, "pane.resize-left-5"),
        (Action::PaneResizeRight5, "pane.resize-right-5"),
        (Action::Refresh, "view.refresh"),
        (Action::CommandPrompt, "command.prompt"),
        (Action::CopyEnter, "copy.enter"),
        (Action::BufferPaste, "buffer.paste"),
        (Action::PaneBreak, "pane.break"),
        (Action::BufferList, "buffer.list"),
        (Action::BufferDelete, "buffer.delete"),
        (Action::CopyPageUp, "copy.page-up"),
        (Action::ClientDetach, "client.detach"),
        (Action::RenameTerminal, "terminal.rename"),
        (Action::Copy, "clipboard.copy"),
        (Action::Paste, "clipboard.paste"),
        (Action::CopyHtml, "clipboard.copy-html"),
        (Action::PasteOneLine, "clipboard.paste-one-line"),
        (Action::SelectAll, "selection.all"),
        (Action::MarkMode, "selection.mark"),
        (Action::FindOpen, "search.open"),
        (Action::SaveOutput, "output.save"),
        (Action::ZoomIn, "view.zoom-in"),
        (Action::ZoomOut, "view.zoom-out"),
        (Action::ZoomReset, "view.zoom-reset"),
        (Action::ScrollPageUp, "scroll.page-up"),
        (Action::ScrollPageDown, "scroll.page-down"),
        (Action::ScrollLineUp, "scroll.line-up"),
        (Action::ScrollLineDown, "scroll.line-down"),
        (Action::ScrollTop, "scroll.top"),
        (Action::ScrollBottom, "scroll.bottom"),
        (Action::CopyExit, "copy.exit"),
        (Action::CopyBeginSelection, "copy.begin-selection"),
        (Action::CopyCopySelection, "copy.copy-selection"),
        (Action::ManagerOpen, "manager.open"),
        (Action::ManagerNew, "manager.new"),
        (Action::ManagerRename, "manager.rename"),
        (Action::ManagerDelete, "manager.delete"),
        (Action::ManagerSearch, "manager.search"),
        (Action::ManagerPin, "manager.pin"),
        (Action::ManagerClose, "manager.close"),
        (Action::InsertNewline, "terminal.insert-newline"),
        (Action::LineHome, "line.home"),
        (Action::LineEnd, "line.end"),
        (Action::LinePreviousWord, "line.previous-word"),
        (Action::LineNextWord, "line.next-word"),
        (Action::LineDeletePreviousWord, "line.delete-previous-word"),
        (Action::LineDeleteLine, "line.delete-line"),
    ];

    pub fn from_id(id: &str) -> Option<Action> {
        Self::ALL.iter().find(|(_, i)| *i == id).map(|(a, _)| *a)
    }

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(a, _)| *a == self)
            .map_or("", |(_, i)| i)
    }
}

/// One key press as the matcher sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Press {
    /// The virtual key (the layout's logical key).
    pub vk: u32,
    /// The scan code (`lParam` bits 16–23) and the extended-key flag (bit 24).
    pub scan: u32,
    pub extended: bool,
    /// The modifiers held, with their side when only one side is down.
    pub mods: HMods,
    /// The character the key produces on the current layout, when a binding needs it.
    pub ch: Option<char>,
    /// An auto-repeat of a key that is held down.
    pub repeat: bool,
}

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a shortcut: the key goes on to the program (or the state screen).
    Ignored,
    /// The first steps of a sequence: waiting for the next one (no timer).
    Pending,
    /// A pending sequence did not continue: the key is swallowed and a hint is shown.
    Cancelled,
    /// A shortcut's key that does nothing now (an auto-repeat, or the press of a `release`
    /// binding): it is swallowed, never sent to the program.
    Swallowed,
    /// A `hold` binding is down: fire it if it is still held after this many milliseconds.
    Hold(u64),
    /// Run this action.
    Fire(Action),
    /// Step 23: after the prefix, the keys of a single-chord `app` binding (`Ctrl+B Ctrl+T`):
    /// they go to the program, not to the shortcut they are bound to (HOTKEY_CONTRACT §13.1).
    PassThrough,
}

/// tmux `repeat-time`: how long after a repeatable prefix binding (`pane.resize-*`) its key
/// works again without the prefix (Step 23, settings `terminal.repeat_time_ms`).
pub const REPEAT_MS: u64 = 500;

/// One active binding.
#[derive(Debug, Clone)]
struct Active {
    action: Action,
    keys: String,
    steps: Vec<Step>,
    scope: Scope,
    trigger: Trigger,
    hold_ms: u64,
    repeat: bool,
}

/// Which surface a matcher serves: the window (its `app` actions and the `terminal` context) or
/// the Terminal manager (the `manager` context, Step 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Window,
    Manager,
}

impl Surface {
    /// Whether a binding of `scope` is active on this surface.
    fn takes(self, scope: &Scope) -> bool {
        match self {
            Self::Window => {
                matches!(scope, Scope::App)
                    || *scope == Scope::Context(sh::TERMINAL_CONTEXT.into())
                    || *scope == Scope::Context(sh::COPY_CONTEXT.into())
            }
            Self::Manager => *scope == Scope::Context(sh::MANAGER_CONTEXT.into()),
        }
    }

    /// The surface's innermost context (its bindings win over `app` ones, §9.1).
    fn context(self) -> &'static str {
        match self {
            Self::Window => sh::TERMINAL_CONTEXT,
            Self::Manager => sh::MANAGER_CONTEXT,
        }
    }
}

/// The window's shortcuts: Subak's bindings after layering, and the state of a pending sequence
/// or an armed `release`/`hold` binding.
#[derive(Debug)]
pub struct Matcher {
    surface: Surface,
    loaded: Loaded,
    active: Vec<Active>,
    pending: Option<(usize, Vec<usize>)>,
    /// A `release` or `hold` binding whose key is down: (binding, virtual key).
    armed: Option<(usize, u32)>,
    off: Option<&'static str>,
    /// Why `hotkeys.json` is not used (safe mode), or why a reload was ignored.
    pub problem: Option<String>,
    /// Step 23: the `profiles` overlay of `hotkeys.json` in use, if any.
    profile: Option<String>,
    /// Step 23: the prefix leader's chord now (`None`: disabled).
    leader: Option<Chord>,
    /// Step 23: `repeat-time` in milliseconds (0: no repeat without the prefix).
    repeat_ms: u64,
    /// Step 23: until when the repeatable prefix bindings fire without the prefix.
    repeating: Option<std::time::Instant>,
    /// Step 24: the window is in copy mode: its `copy-mode` bindings are active and win.
    copy_mode: bool,
}

/// The contract's name of a virtual key (§5.2), if it has one.
pub fn named_key(vk: u32, extended: bool) -> Option<&'static str> {
    let name: String = match vk {
        0x41..=0x5A | 0x30..=0x39 => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x0D if extended => "NumpadEnter".into(),
        _ => match vk {
            0x1B => "Escape",
            0x09 => "Tab",
            0x0D => "Enter",
            0x20 => "Space",
            0x08 => "Backspace",
            0x2D => "Insert",
            0x2E => "Delete",
            0x24 => "Home",
            0x23 => "End",
            0x21 => "PageUp",
            0x22 => "PageDown",
            0x26 => "Up",
            0x28 => "Down",
            0x25 => "Left",
            0x27 => "Right",
            0xBB => "Plus",
            0xBD => "Minus",
            0xBC => "Comma",
            0xBE => "Period",
            0x6A => "NumpadMultiply",
            0x6B => "NumpadAdd",
            0x6D => "NumpadSubtract",
            0x6E => "NumpadDecimal",
            0x6F => "NumpadDivide",
            0x2C => "PrintScreen",
            0x13 => "Pause",
            0x91 => "ScrollLock",
            0x5D => "ContextMenu",
            0xB0 => "MediaNext",
            0xB1 => "MediaPrevious",
            0xB2 => "MediaStop",
            0xB3 => "MediaPlayPause",
            0xAD => "VolumeMute",
            0xAE => "VolumeDown",
            0xAF => "VolumeUp",
            0x15 => "HangulMode",
            0x19 => "Hanja",
            0x1C => "Convert",
            0x1D => "NonConvert",
            _ => return None,
        }
        .into(),
    };
    NAMED_KEYS.iter().find(|n| **n == name).copied()
}

/// The `code:` name of a scan code (set 1, as Windows reports it; §6.1), if it has one.
pub fn code_key(scan: u32, extended: bool) -> Option<&'static str> {
    if extended {
        return None;
    }
    let name = match scan {
        0x1E => "KeyA",
        0x30 => "KeyB",
        0x2E => "KeyC",
        0x20 => "KeyD",
        0x12 => "KeyE",
        0x21 => "KeyF",
        0x22 => "KeyG",
        0x23 => "KeyH",
        0x17 => "KeyI",
        0x24 => "KeyJ",
        0x25 => "KeyK",
        0x26 => "KeyL",
        0x32 => "KeyM",
        0x31 => "KeyN",
        0x18 => "KeyO",
        0x19 => "KeyP",
        0x10 => "KeyQ",
        0x13 => "KeyR",
        0x1F => "KeyS",
        0x14 => "KeyT",
        0x16 => "KeyU",
        0x2F => "KeyV",
        0x11 => "KeyW",
        0x2D => "KeyX",
        0x15 => "KeyY",
        0x2C => "KeyZ",
        0x02 => "Digit1",
        0x03 => "Digit2",
        0x04 => "Digit3",
        0x05 => "Digit4",
        0x06 => "Digit5",
        0x07 => "Digit6",
        0x08 => "Digit7",
        0x09 => "Digit8",
        0x0A => "Digit9",
        0x0B => "Digit0",
        0x0C => "Minus",
        0x0D => "Equal",
        0x1A => "BracketLeft",
        0x1B => "BracketRight",
        0x2B => "Backslash",
        0x27 => "Semicolon",
        0x28 => "Quote",
        0x29 => "Backquote",
        0x33 => "Comma",
        0x34 => "Period",
        0x35 => "Slash",
        0x56 => "IntlBackslash",
        0x73 => "IntlRo",
        0x7D => "IntlYen",
        0x72 => "Lang1",
        0x71 => "Lang2",
        _ => return None,
    };
    CODE_KEYS.iter().find(|n| **n == name).copied()
}

/// Whether a key press can be a binding's step (a `char:` step needs the produced character).
fn hits(p: &Press, s: &Step) -> bool {
    let Step::Chord(want) = s else {
        return false;
    };
    let mut candidates: Vec<Step> = Vec::with_capacity(3);
    if let Some(n) = named_key(p.vk, p.extended) {
        candidates.push(Step::Chord(Chord {
            mods: p.mods,
            key: HKey::Named(n),
        }));
    }
    if let Some(c) = code_key(p.scan, p.extended) {
        candidates.push(Step::Chord(Chord {
            mods: p.mods,
            key: HKey::Code(c),
        }));
    }
    if let (HKey::Char(c), Some(ch)) = (want.key, p.ch) {
        // A `char:` step is whatever produces that character (Shift is implied, §5.5); Ctrl,
        // Alt and Win make it a chord, not a character.
        return c == ch && !p.mods.has_command();
    }
    candidates
        .iter()
        .any(|c| kim_hotkeys::keys::steps_overlap(c, s))
}

impl Matcher {
    /// Subak's shortcuts from `hotkeys.json` in `dir` over the shipped defaults. `off`
    /// (`--no-hotkeys`, `KIM_NO_HOTKEYS`) turns every shortcut off without touching the file.
    pub fn load(dir: Option<&Path>, off: Option<&'static str>) -> Matcher {
        Self::load_for(Surface::Window, dir, off)
    }

    /// The shortcuts of `surface` (the window's, or the Terminal manager's).
    pub fn load_for(surface: Surface, dir: Option<&Path>, off: Option<&'static str>) -> Matcher {
        let (_, loaded) = sh::load(dir);
        let mut m = Matcher {
            surface,
            problem: sh::safe_mode_warning(&loaded),
            loaded,
            active: Vec::new(),
            pending: None,
            armed: None,
            off,
            profile: None,
            leader: None,
            repeat_ms: REPEAT_MS,
            repeating: None,
            copy_mode: false,
        };
        m.rebuild();
        m
    }

    /// The defaults only (tests, no settings folder).
    #[cfg(test)]
    pub fn defaults() -> Matcher {
        Self::load(None, None)
    }

    fn state(&self) -> ToolState {
        let mut s = sh::state(sh::manifest(), &self.loaded);
        s.profile.clone_from(&self.profile);
        s
    }

    /// Step 23: uses the `profiles` overlay `name` of `hotkeys.json` (`None`: none).
    pub fn set_profile(&mut self, name: Option<String>) {
        if self.profile != name {
            self.profile = name;
            self.rebuild();
        }
    }

    /// Step 23: tmux `repeat-time` (0 turns repeating without the prefix off).
    pub fn set_repeat_time(&mut self, ms: u64) {
        self.repeat_ms = ms;
        self.repeating = None;
    }

    /// Step 24: copy mode on or off (its `copy-mode` bindings are active only then).
    pub fn set_copy_mode(&mut self, on: bool) {
        self.copy_mode = on;
        self.pending = None;
    }

    /// Step 23: the prefix leader's chord now, if it is enabled.
    pub fn leader(&self) -> Option<&Chord> {
        self.leader.as_ref()
    }

    /// Whether `step` is the prefix leader's chord.
    fn is_leader(&self, step: &Step) -> bool {
        matches!((step, &self.leader), (Step::Chord(c), Some(l)) if c == l)
    }

    fn rebuild(&mut self) {
        self.pending = None;
        self.armed = None;
        self.repeating = None;
        self.active.clear();
        self.leader = None;
        if self.off.is_some() {
            return;
        }
        let state = self.state();
        // Step 23: `@prefix` steps stand for the leader's current chord (§3.3).
        self.leader = state
            .leader_chord("prefix")
            .and_then(|(k, _)| Chord::parse(&k).ok());
        for b in analyze(std::slice::from_ref(&state), &[]).active {
            let Some(action) = Action::from_id(&b.action) else {
                continue; // not implemented by this build (never shipped)
            };
            if !self.surface.takes(&b.scope) {
                continue;
            }
            let hold_ms = state
                .manifest
                .action(&b.action)
                .and_then(|d| state.specs(d))
                .and_then(|(specs, _)| {
                    specs.into_iter().find_map(|s| match s {
                        BindingSpec::Object(o) if o.keys == b.keys => o.hold_ms,
                        _ => None,
                    })
                })
                .unwrap_or(500);
            let steps: Option<Vec<Step>> = b
                .seq
                .steps()
                .iter()
                .map(|s| match s {
                    Step::Leader(l) => state
                        .leader_chord(l)
                        .and_then(|(k, _)| Chord::parse(&k).ok())
                        .map(Step::Chord),
                    other => Some(other.clone()),
                })
                .collect();
            let Some(steps) = steps else {
                continue; // a disabled leader: its table is off
            };
            self.active.push(Active {
                action,
                keys: b.keys.clone(),
                steps,
                scope: b.scope.clone(),
                trigger: b.trigger,
                hold_ms,
                repeat: b.repeat,
            });
        }
    }

    /// Re-reads `hotkeys.json` (the window regained focus, HOTKEY_CONTRACT §8.3 5). An invalid
    /// file keeps the current bindings and is reported. Returns whether anything changed.
    pub fn reload(&mut self) -> bool {
        let (next, problems) = store::reload(&self.loaded, &sh::manifest());
        let problem = match problems.first() {
            Some(p) => Some(format!(
                "hotkeys.json is invalid ({p}); the previous shortcuts stay active"
            )),
            None => sh::safe_mode_warning(&next),
        };
        let changed = next != self.loaded || problem != self.problem;
        if changed {
            self.problem = problem;
            self.loaded = next;
            self.rebuild();
        }
        changed
    }

    /// Why every shortcut is off for this run, if it is.
    pub fn off(&self) -> Option<&'static str> {
        self.off
    }

    /// The state of the user layer (`default`, `user`, `invalid`, `newer_version`).
    pub fn config_state(&self) -> ConfigState {
        self.loaded.state
    }

    /// Whether any binding needs the produced character of a key press (`char:` steps).
    pub fn wants_chars(&self) -> bool {
        self.active.iter().any(|a| {
            a.steps
                .iter()
                .any(|s| matches!(s, Step::Chord(c) if matches!(c.key, HKey::Char(_))))
        })
    }

    /// Whether a sequence is waiting for its next step.
    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The keys typed so far of a pending sequence (for the hint).
    pub fn pending_keys(&self) -> Option<String> {
        let (step, pool) = self.pending.as_ref()?;
        let first = self.active.get(*pool.first()?)?;
        Some(
            first.steps[..*step]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        )
    }

    /// Cancels a pending sequence and an armed binding (`Esc`, a click, focus loss).
    pub fn cancel(&mut self) {
        self.pending = None;
        self.armed = None;
        self.repeating = None;
    }

    /// One key press (not a lone modifier).
    pub fn press(&mut self, p: &Press) -> Outcome {
        if self.active.is_empty() {
            return Outcome::Ignored;
        }
        if let Some((i, vk)) = self.armed {
            if p.vk == vk && p.repeat {
                return Outcome::Swallowed; // the armed key auto-repeats
            }
            // Another key in between: a `release` binding does not fire; a `hold` stops.
            let _ = i;
            self.armed = None;
        }
        // Step 23, tmux `repeat-time`: right after a repeatable prefix binding fired, its key (or
        // another repeatable one's) works again without the prefix; any other key ends that.
        if let Some(until) = self.repeating.take()
            && self.pending.is_none()
            && std::time::Instant::now() < until
            && let Some(i) = (0..self.active.len()).find(|i| {
                let a = &self.active[*i];
                a.repeat
                    && a.steps.len() == 2
                    && self.is_leader(&a.steps[0])
                    && hits(p, &a.steps[1])
            })
        {
            self.repeating =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(self.repeat_ms));
            return Outcome::Fire(self.active[i].action);
        }
        let (step, pool) = match self.pending.take() {
            Some((step, pool)) => {
                if p.vk == 0x1B && !p.mods.has_command() && !p.mods.shift.on() {
                    return Outcome::Swallowed; // Esc cancels a pending sequence
                }
                (step, pool)
            }
            None => (0, (0..self.active.len()).collect()),
        };
        let copy = Scope::Context(sh::COPY_CONTEXT.into());
        let matched: Vec<usize> = pool
            .into_iter()
            .filter(|i| self.copy_mode || self.active[*i].scope != copy)
            .filter(|i| self.active[*i].steps.get(step).is_some_and(|s| hits(p, s)))
            .collect();
        // Step 24: in copy mode its own keys win over everything else.
        let matched = if self.copy_mode && matched.iter().any(|i| self.active[*i].scope == copy) {
            matched
                .into_iter()
                .filter(|i| self.active[*i].scope == copy)
                .collect()
        } else {
            matched
        };
        // The innermost context wins over `app` bindings with the same keys (§9.1).
        let terminal = Scope::Context(self.surface.context().into());
        let matched = if matched.iter().any(|i| self.active[*i].scope == terminal) {
            matched
                .into_iter()
                .filter(|i| self.active[*i].scope == terminal)
                .collect()
        } else {
            matched
        };
        if matched.is_empty() {
            // Step 23, the pass-through rule: after the prefix alone, the keys of a single-chord
            // `app` binding are sent to the program (`Ctrl+B Ctrl+W` types Ctrl+W).
            if step == 1
                && self.leader.is_some()
                && self
                    .active
                    .iter()
                    .any(|a| a.steps.len() == 1 && a.scope == Scope::App && hits(p, &a.steps[0]))
            {
                return Outcome::PassThrough;
            }
            return if step > 0 {
                Outcome::Cancelled
            } else {
                Outcome::Ignored
            };
        }
        if let Some(&i) = matched
            .iter()
            .find(|i| self.active[**i].steps.len() == step + 1)
        {
            let a = &self.active[i];
            if p.repeat && !a.repeat {
                return Outcome::Swallowed;
            }
            if a.repeat && a.steps.len() == 2 && self.is_leader(&a.steps[0]) && self.repeat_ms > 0 {
                self.repeating = Some(
                    std::time::Instant::now() + std::time::Duration::from_millis(self.repeat_ms),
                );
            }
            let a = &self.active[i];
            return match a.trigger {
                Trigger::Press => Outcome::Fire(a.action),
                Trigger::Release => {
                    self.armed = Some((i, p.vk));
                    Outcome::Swallowed
                }
                Trigger::Hold => {
                    self.armed = Some((i, p.vk));
                    Outcome::Hold(a.hold_ms)
                }
            };
        }
        if p.repeat && step == 0 {
            return Outcome::Swallowed;
        }
        self.pending = Some((step + 1, matched));
        Outcome::Pending
    }

    /// A key went up: a `release` binding of that key fires; a `hold` binding stops waiting.
    pub fn release(&mut self, vk: u32) -> Option<Action> {
        let (i, armed_vk) = self.armed?;
        if armed_vk != vk {
            return None;
        }
        self.armed = None;
        let a = &self.active[i];
        (a.trigger == Trigger::Release).then_some(a.action)
    }

    /// The hold delay of an armed `hold` binding ended while its key is still down.
    pub fn hold_elapsed(&mut self) -> Option<Action> {
        let (i, _) = self.armed.take()?;
        let a = &self.active[i];
        (a.trigger == Trigger::Hold).then_some(a.action)
    }

    /// The first key text bound to `action` (menus and hints), if any.
    pub fn keys_for(&self, action: Action) -> Option<&str> {
        self.active
            .iter()
            .find(|a| a.action == action)
            .map(|a| a.keys.as_str())
    }

    /// Every active binding as `(action, keys, scope)` (test reports).
    pub fn bindings(&self) -> Vec<(&'static str, String, String)> {
        self.active
            .iter()
            .map(|a| (a.action.id(), a.keys.clone(), a.scope.to_string()))
            .collect()
    }

    /// The file the bindings come from.
    pub fn path(&self) -> PathBuf {
        self.loaded.path.clone()
    }
}

/// The modifiers of a pane key event as the contract's (generic sides).
pub fn contract_mods(m: Mods) -> HMods {
    use kim_hotkeys::keys::Side;
    let on = |b: bool| if b { Side::Any } else { Side::None };
    HMods {
        ctrl: on(m.ctrl),
        alt: on(m.alt),
        altgr: false,
        shift: on(m.shift),
        win: Side::None,
    }
}

/// Virtual-key codes the window reads (documented `VK_*` values).
pub mod vk {
    pub const BACK: u32 = 0x08;
    pub const TAB: u32 = 0x09;
    pub const RETURN: u32 = 0x0D;
    pub const SHIFT: u32 = 0x10;
    pub const CONTROL: u32 = 0x11;
    pub const MENU: u32 = 0x12;
    pub const ESCAPE: u32 = 0x1B;
    pub const SPACE: u32 = 0x20;
    pub const PRIOR: u32 = 0x21;
    pub const NEXT: u32 = 0x22;
    pub const END: u32 = 0x23;
    pub const HOME: u32 = 0x24;
    pub const LEFT: u32 = 0x25;
    pub const UP: u32 = 0x26;
    pub const RIGHT: u32 = 0x27;
    pub const DOWN: u32 = 0x28;
    pub const INSERT: u32 = 0x2D;
    pub const DELETE: u32 = 0x2E;
    pub const LWIN: u32 = 0x5B;
    pub const RWIN: u32 = 0x5C;
    pub const APPS: u32 = 0x5D;
    pub const F1: u32 = 0x70;
    pub const F3: u32 = 0x72;
    pub const F4: u32 = 0x73;
    pub const F10: u32 = 0x79;
    pub const F12: u32 = 0x7B;
    pub const LSHIFT: u32 = 0xA0;
    pub const RSHIFT: u32 = 0xA1;
    pub const LCONTROL: u32 = 0xA2;
    pub const RCONTROL: u32 = 0xA3;
    pub const LMENU: u32 = 0xA4;
    pub const RMENU: u32 = 0xA5;
    pub const PROCESSKEY: u32 = 0xE5;
}

/// The pane key of a key that produces no character (cursor and editing keys, F1–F12). Keys that
/// type something arrive as characters ([`char_key`]).
/// Step 23: the terminal key of a contract key name (`Tab`, `F5`, `Up`…), if it has one.
pub fn special_by_name(name: &str) -> Option<Key> {
    (0u32..=0xFF)
        .find(|vk| named_key(*vk, false) == Some(name))
        .and_then(special_key)
}

pub fn special_key(code: u32) -> Option<Key> {
    Some(match code {
        vk::UP => Key::Up,
        vk::DOWN => Key::Down,
        vk::LEFT => Key::Left,
        vk::RIGHT => Key::Right,
        vk::HOME => Key::Home,
        vk::END => Key::End,
        vk::PRIOR => Key::PageUp,
        vk::NEXT => Key::PageDown,
        vk::INSERT => Key::Insert,
        vk::DELETE => Key::Delete,
        vk::F1..=vk::F12 => Key::F(u8::try_from(code - vk::F1 + 1).ok()?),
        _ => return None,
    })
}

/// `Ctrl+2` … `Ctrl+8` produce no character on most layouts, but xterm sends the VT220 control
/// codes for them (NUL, ESC, FS, GS, RS, US, DEL): the digit to send with Ctrl, if it is one.
pub fn ctrl_digit(code: u32, mods: Mods) -> Option<char> {
    (mods == Mods::CTRL && (0x32..=0x38).contains(&code)).then(|| char::from(code as u8))
}

/// What a typed character sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Encode this key with these modifiers.
    Key(Key, Mods),
    /// Send these bytes as they are (a control character the layout already produced).
    Raw(Vec<u8>),
}

/// The pane input for a character from `WM_CHAR` (`alt` = it came as `WM_SYSCHAR`), with the
/// modifiers held. `Ctrl+Alt` is AltGr on many layouts: such a character is plain text.
pub fn char_key(c: char, alt: bool, held: Mods) -> Typed {
    let altgr = held.ctrl && held.alt;
    match c {
        '\r' => Typed::Key(Key::Enter, Mods { alt, ..Mods::NONE }),
        // Ctrl+Enter arrives as LF.
        '\n' => Typed::Key(Key::Enter, Mods::NONE),
        '\t' => Typed::Key(
            Key::Tab,
            Mods {
                shift: held.shift,
                ..Mods::NONE
            },
        ),
        '\u{8}' => Typed::Key(Key::Backspace, Mods { alt, ..Mods::NONE }),
        // Ctrl+Backspace arrives as DEL; xterm sends BS for it.
        '\u{7f}' => Typed::Key(Key::Backspace, Mods::CTRL),
        '\u{1b}' => Typed::Key(Key::Escape, Mods { alt, ..Mods::NONE }),
        // Ctrl+letter (and Ctrl+[ \ ] ^ _) arrive as their C0 control already.
        c if (c as u32) < 0x20 => Typed::Raw(vec![c as u8]),
        // Ctrl+Space: NUL.
        ' ' if held.ctrl && !altgr => Typed::Key(Key::Char(' '), Mods::CTRL),
        c => Typed::Key(Key::Char(c), Mods { alt, ..Mods::NONE }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kim_hotkeys::keys::Side;
    use serde_json::json;

    fn press(vk: u32, m: Mods) -> Press {
        Press {
            vk,
            scan: 0,
            extended: false,
            mods: contract_mods(m),
            ch: None,
            repeat: false,
        }
    }

    const CS: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn sandbox(tag: &str, layer: serde_json::Value) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "subak-keys-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(sh::layer_path(&d), layer.to_string()).unwrap();
        d
    }

    #[test]
    fn every_implemented_action_has_an_id_in_the_manifest() {
        let m = sh::manifest();
        assert_eq!(Action::ALL.len(), sh::IMPLEMENTED.len());
        for ((a, id), want) in Action::ALL.iter().zip(sh::IMPLEMENTED) {
            assert_eq!(id, want);
            assert_eq!(Action::from_id(id), Some(*a));
            assert_eq!(a.id(), *id);
            assert!(m.action(id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_requested_tab_keys_and_the_step_16_keys_fire_with_exact_modifiers_only() {
        let mut s = Matcher::defaults();
        let fire = |s: &mut Matcher, vk: u32, m: Mods| s.press(&press(vk, m));
        // Exactly the requested keys (SUBAK-FR-033).
        assert_eq!(
            fire(&mut s, vk::TAB, Mods::CTRL),
            Outcome::Fire(Action::NextTab)
        );
        assert_eq!(
            fire(&mut s, vk::TAB, CS),
            Outcome::Fire(Action::PreviousTab)
        );
        assert_eq!(
            fire(&mut s, u32::from(b'T'), Mods::CTRL),
            Outcome::Fire(Action::NewTerminal)
        );
        assert_eq!(
            fire(&mut s, u32::from(b'W'), Mods::CTRL),
            Outcome::Fire(Action::CloseTerminal)
        );
        assert_eq!(
            fire(&mut s, u32::from(b'R'), CS),
            Outcome::Fire(Action::RenameTerminal)
        );
        // The Step 16 keys keep working (now rebindable).
        assert_eq!(
            fire(&mut s, u32::from(b'C'), CS),
            Outcome::Fire(Action::Copy)
        );
        assert_eq!(
            fire(&mut s, vk::INSERT, Mods::SHIFT),
            Outcome::Fire(Action::Paste)
        );
        assert_eq!(
            fire(&mut s, 0xBB, Mods::CTRL),
            Outcome::Fire(Action::ZoomIn)
        );
        assert_eq!(
            fire(&mut s, vk::PRIOR, CS),
            Outcome::Fire(Action::ScrollPageUp)
        );
        // Keys for the program: Ctrl+C (interrupt), Ctrl+V, Tab, Shift+Tab, Ctrl+Shift+T, Alt+W,
        // plain letters. (Ctrl+B is the prefix and F1 the help since Step 23.)
        for (code, m) in [
            (u32::from(b'C'), Mods::CTRL),
            (u32::from(b'V'), Mods::CTRL),
            (vk::TAB, Mods::NONE),
            (vk::TAB, Mods::SHIFT),
            (u32::from(b'T'), CS),
            (u32::from(b'W'), Mods::ALT),
            (u32::from(b'W'), Mods::NONE),
            (vk::PRIOR, Mods::SHIFT),
        ] {
            assert_eq!(fire(&mut s, code, m), Outcome::Ignored, "{code:#x} {m:?}");
        }
        // Holding Ctrl+W never repeats the close and never leaks to the program.
        let mut p = press(u32::from(b'W'), Mods::CTRL);
        p.repeat = true;
        assert_eq!(s.press(&p), Outcome::Swallowed);
        assert!(!s.is_pending());
    }

    /// A key that produces `ch` on the current layout (a `char:` step needs it).
    fn typed(vk: u32, m: Mods, ch: char) -> Press {
        Press {
            ch: Some(ch),
            ..press(vk, m)
        }
    }

    /// Step 23 (SUBAK-FR-033, HOTKEY_CONTRACT §13.1): the `Ctrl+B` table drives windows and
    /// panes; `Ctrl+B Ctrl+B` sends the prefix; the keys of a tab shortcut after the prefix go to
    /// the program; an unbound key after the prefix is swallowed with a hint; Esc cancels.
    #[test]
    fn the_prefix_table_controls_windows_and_panes_and_passes_tab_keys_through() {
        let mut s = Matcher::defaults();
        let ctrl_b = press(u32::from(b'B'), Mods::CTRL);
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        let seq = |s: &mut Matcher, second: Press| {
            assert_eq!(s.press(&ctrl_b), Outcome::Pending);
            assert_eq!(s.pending_keys().as_deref(), Some("Ctrl+B"));
            s.press(&second)
        };
        for (second, want) in [
            (typed(u32::from(b'C'), Mods::NONE, 'c'), Action::WindowNew),
            (typed(u32::from(b'N'), Mods::NONE, 'n'), Action::WindowNext),
            (
                typed(u32::from(b'P'), Mods::NONE, 'p'),
                Action::WindowPrevious,
            ),
            (typed(0xBC, Mods::NONE, ','), Action::WindowRename),
            (typed(0x37, shift, '&'), Action::WindowKill),
            (typed(0x35, shift, '%'), Action::PaneSplitRight),
            (typed(0xDE, shift, '"'), Action::PaneSplitDown),
            (press(vk::UP, Mods::NONE), Action::PaneFocusUp),
            (press(vk::LEFT, Mods::NONE), Action::PaneFocusLeft),
            (typed(u32::from(b'Z'), Mods::NONE, 'z'), Action::PaneZoom),
            (typed(u32::from(b'X'), Mods::NONE, 'x'), Action::PaneClose),
            (
                typed(u32::from(b'D'), Mods::NONE, 'd'),
                Action::ClientDetach,
            ),
            (typed(0xBF, shift, '?'), Action::HelpShow),
            (press(u32::from(b'B'), Mods::CTRL), Action::SendPrefix),
            (press(u32::from(b'0'), Mods::NONE), Action::WindowSelect(0)),
            (press(u32::from(b'7'), Mods::NONE), Action::WindowSelect(7)),
            (typed(u32::from(b'O'), Mods::NONE, 'o'), Action::PaneNext),
            (typed(0xDB, shift, '{'), Action::PaneSwapPrevious),
            (typed(0xDD, shift, '}'), Action::PaneSwapNext),
            (press(u32::from(b'O'), Mods::CTRL), Action::WindowRotate),
            (press(u32::from(b'O'), Mods::ALT), Action::WindowRotateBack),
            (
                press(u32::from(b'1'), Mods::ALT),
                Action::LayoutEvenHorizontal,
            ),
            (
                press(u32::from(b'2'), Mods::ALT),
                Action::LayoutEvenVertical,
            ),
            (press(u32::from(b'5'), Mods::ALT), Action::LayoutTiled),
            (
                press(u32::from(b'3'), Mods::ALT),
                Action::LayoutMainHorizontal,
            ),
            (
                press(u32::from(b'4'), Mods::ALT),
                Action::LayoutMainVertical,
            ),
            (typed(u32::from(b'L'), Mods::NONE, 'l'), Action::WindowLast),
            (
                press(u32::from(b'7'), Mods::ALT),
                Action::LayoutMainVerticalMirrored,
            ),
            (press(vk::UP, Mods::ALT), Action::PaneResizeUp5),
            (press(vk::RIGHT, Mods::CTRL), Action::PaneResizeRight),
            (typed(u32::from(b'R'), Mods::NONE, 'r'), Action::Refresh),
        ] {
            assert_eq!(seq(&mut s, second), Outcome::Fire(want), "{want:?}");
            assert!(!s.is_pending());
        }
        // `char:` steps follow the layout: `%` on whatever key produces it (e.g. AZERTY).
        assert_eq!(
            seq(&mut s, typed(0xC0, Mods::NONE, '%')),
            Outcome::Fire(Action::PaneSplitRight)
        );
        // The pass-through rule: a tab shortcut's keys after the prefix reach the program.
        for second in [
            press(u32::from(b'T'), Mods::CTRL),
            press(u32::from(b'W'), Mods::CTRL),
            press(vk::TAB, Mods::CTRL),
        ] {
            assert_eq!(seq(&mut s, second), Outcome::PassThrough);
            assert!(!s.is_pending());
        }
        // An unbound key after the prefix is swallowed (a hint is shown); the next key is the
        // program's again. Esc cancels quietly.
        assert_eq!(
            seq(&mut s, typed(u32::from(b'J'), Mods::NONE, 'j')),
            Outcome::Cancelled
        );
        assert_eq!(
            s.press(&typed(u32::from(b'J'), Mods::NONE, 'j')),
            Outcome::Ignored
        );
        assert_eq!(
            seq(&mut s, press(vk::ESCAPE, Mods::NONE)),
            Outcome::Swallowed
        );
        assert!(!s.is_pending());
        // Ctrl+C after the prefix is not a binding: swallowed, never an interrupt by surprise;
        // Ctrl+C alone always reaches the program.
        assert_eq!(
            seq(&mut s, press(u32::from(b'C'), Mods::CTRL)),
            Outcome::Cancelled
        );
        assert_eq!(
            s.press(&press(u32::from(b'C'), Mods::CTRL)),
            Outcome::Ignored
        );
        // The direct keys of Step 22 still work without the prefix.
        assert_eq!(
            s.press(&press(vk::LEFT, Mods::ALT)),
            Outcome::Fire(Action::PaneFocusLeft)
        );
        assert_eq!(
            s.press(&press(vk::F1, Mods::NONE)),
            Outcome::Fire(Action::HelpShow)
        );
    }

    /// tmux `repeat-time`: after `Ctrl+B Ctrl+Up`, `Ctrl+Up` alone resizes again for a while;
    /// any other key ends it; `repeat_time_ms = 0` turns it off.
    #[test]
    fn repeatable_prefix_keys_work_again_without_the_prefix_for_the_repeat_time() {
        let mut s = Matcher::defaults();
        let ctrl_b = press(u32::from(b'B'), Mods::CTRL);
        let ctrl_up = press(vk::UP, Mods::CTRL);
        assert_eq!(s.press(&ctrl_b), Outcome::Pending);
        assert_eq!(s.press(&ctrl_up), Outcome::Fire(Action::PaneResizeUp));
        assert_eq!(s.press(&ctrl_up), Outcome::Fire(Action::PaneResizeUp));
        assert_eq!(
            s.press(&press(vk::DOWN, Mods::CTRL)),
            Outcome::Fire(Action::PaneResizeDown),
            "another repeatable key of the table"
        );
        assert_eq!(
            s.press(&typed(u32::from(b'A'), Mods::NONE, 'a')),
            Outcome::Ignored
        );
        assert_eq!(s.press(&ctrl_up), Outcome::Ignored, "the repeat ended");
        s.set_repeat_time(0);
        assert_eq!(s.press(&ctrl_b), Outcome::Pending);
        assert_eq!(s.press(&ctrl_up), Outcome::Fire(Action::PaneResizeUp));
        assert_eq!(s.press(&ctrl_up), Outcome::Ignored);
        // A non-repeatable key never repeats.
        s.set_repeat_time(500);
        assert_eq!(s.press(&ctrl_b), Outcome::Pending);
        assert_eq!(
            s.press(&typed(u32::from(b'Z'), Mods::NONE, 'z')),
            Outcome::Fire(Action::PaneZoom)
        );
        assert_eq!(
            s.press(&typed(u32::from(b'Z'), Mods::NONE, 'z')),
            Outcome::Ignored
        );
    }

    /// A rebound leader moves the whole table (and `Ctrl+B` goes back to the program; a leader
    /// is changed, never disabled, §8); a `profiles` overlay applies only when chosen.
    #[test]
    fn the_leader_follows_hotkeys_json_and_profiles_overlay_it() {
        let dir = sandbox(
            "leader",
            serde_json::json!({
                "schema": "kim.hotkeys", "schema_version": 1, "tool": "subak",
                "leaders": {"prefix": "Ctrl+A"},
                "overrides": [{"action": "window.new", "bindings": ["@prefix char:w"]}],
                "profiles": {"vim": {"overrides": [{"action": "pane.zoom", "disabled": true}]}}
            }),
        );
        let mut s = Matcher::load(Some(&dir), None);
        assert_eq!(
            s.leader().map(ToString::to_string).as_deref(),
            Some("Ctrl+A")
        );
        assert_eq!(
            s.press(&press(u32::from(b'B'), Mods::CTRL)),
            Outcome::Ignored
        );
        assert_eq!(
            s.press(&press(u32::from(b'A'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(
            s.press(&typed(u32::from(b'W'), Mods::NONE, 'w')),
            Outcome::Fire(Action::WindowNew)
        );
        assert_eq!(
            s.press(&press(u32::from(b'A'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(
            s.press(&press(u32::from(b'A'), Mods::CTRL)),
            Outcome::Fire(Action::SendPrefix),
            "input.send-prefix follows the leader"
        );
        // The `vim` profile disables pane.zoom (also Ctrl+Shift+Z); without it, both work.
        assert_eq!(
            s.press(&press(u32::from(b'A'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(
            s.press(&typed(u32::from(b'Z'), Mods::NONE, 'z')),
            Outcome::Fire(Action::PaneZoom)
        );
        s.set_profile(Some("vim".into()));
        assert_eq!(s.press(&press(u32::from(b'Z'), CS)), Outcome::Ignored);
        assert_eq!(
            s.press(&press(u32::from(b'A'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(
            s.press(&typed(u32::from(b'Z'), Mods::NONE, 'z')),
            Outcome::Cancelled
        );
    }

    #[test]
    fn rebinding_disabling_and_switching_off_follow_hotkeys_json() {
        let dir = sandbox(
            "rebind",
            json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "subak",
            "overrides": [
                {"action": "terminal.close", "disabled": true},
                {"action": "tab.next", "bindings": ["Ctrl+PageDown"]},
                {"action": "tab.hide", "bindings": ["Ctrl+Shift+W"]},
                {"action": "terminal.new", "bindings": ["Ctrl+K Ctrl+N"]}
            ]}),
        );
        let mut s = Matcher::load(Some(&dir), None);
        assert_eq!(s.config_state(), ConfigState::User);
        // Disabled: Ctrl+W goes to the program (readline's delete-word, SUBAK-FR-034).
        assert_eq!(
            s.press(&press(u32::from(b'W'), Mods::CTRL)),
            Outcome::Ignored
        );
        assert_eq!(s.press(&press(vk::TAB, Mods::CTRL)), Outcome::Ignored);
        assert_eq!(
            s.press(&press(vk::NEXT, Mods::CTRL)),
            Outcome::Fire(Action::NextTab)
        );
        assert_eq!(
            s.press(&press(u32::from(b'W'), CS)),
            Outcome::Fire(Action::HideTab)
        );
        // A sequence: the first step waits, the second fires; a wrong second step is swallowed.
        assert_eq!(
            s.press(&press(u32::from(b'K'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(s.pending_keys().as_deref(), Some("Ctrl+K"));
        assert_eq!(
            s.press(&press(u32::from(b'N'), Mods::CTRL)),
            Outcome::Fire(Action::NewTerminal)
        );
        assert_eq!(
            s.press(&press(u32::from(b'K'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(
            s.press(&press(u32::from(b'X'), Mods::NONE)),
            Outcome::Cancelled
        );
        assert!(!s.is_pending());
        assert_eq!(
            s.press(&press(u32::from(b'K'), Mods::CTRL)),
            Outcome::Pending
        );
        assert_eq!(s.press(&press(vk::ESCAPE, Mods::NONE)), Outcome::Swallowed);
        assert!(!s.is_pending());
        // A file that breaks later keeps the bindings in force (§8.3 5).
        std::fs::write(sh::layer_path(&dir), "{ broken").unwrap();
        assert!(s.reload());
        assert!(
            s.problem
                .as_deref()
                .unwrap()
                .contains("previous shortcuts stay")
        );
        assert_eq!(
            s.press(&press(vk::NEXT, Mods::CTRL)),
            Outcome::Fire(Action::NextTab)
        );
        // Starting with a broken file: safe mode, defaults.
        let mut safe = Matcher::load(Some(&dir), None);
        assert_eq!(safe.config_state(), ConfigState::Invalid);
        assert!(safe.problem.is_some());
        assert_eq!(
            safe.press(&press(u32::from(b'W'), Mods::CTRL)),
            Outcome::Fire(Action::CloseTerminal)
        );
        // --no-hotkeys / KIM_NO_HOTKEYS: nothing is a shortcut, every key reaches the program.
        let mut off = Matcher::load(Some(&dir), Some("--no-hotkeys"));
        assert_eq!(off.off(), Some("--no-hotkeys"));
        assert_eq!(off.press(&press(vk::TAB, Mods::CTRL)), Outcome::Ignored);
        assert_eq!(
            off.press(&press(u32::from(b'C'), CS)),
            Outcome::Ignored,
            "copy is off too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn physical_character_release_and_hold_bindings() {
        let dir = sandbox(
            "kinds",
            json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "subak",
            "overrides": [
                {"action": "tab.move-left", "bindings": ["Alt+Shift+code:KeyQ"]},
                {"action": "tab.move-right", "bindings": ["Ctrl+K char:?"]},
                {"action": "tab.hide", "bindings": [{"keys": "F9", "trigger": "release"}]},
                {"action": "terminal.rename", "bindings": [{"keys": "F8", "trigger": "hold", "hold_ms": 700}]},
                {"action": "tab.next", "bindings": [{"keys": "Ctrl+Tab", "repeat": true}]}
            ]}),
        );
        let mut s = Matcher::load(Some(&dir), None);
        assert!(s.wants_chars());
        // `code:KeyQ` is the key in Q's position whatever the layout says (AZERTY: A). (Not
        // Ctrl+Shift: on AZERTY that is also the logical Ctrl+Shift+A, Select all since Step 19.)
        let alt_shift = Mods {
            ctrl: false,
            shift: true,
            alt: true,
        };
        let mut p = press(u32::from(b'A'), alt_shift);
        p.scan = 0x10;
        assert_eq!(s.press(&p), Outcome::Fire(Action::MoveTabLeft));
        // `char:?` after Ctrl+K: whatever produces `?` on this layout.
        assert_eq!(
            s.press(&press(u32::from(b'K'), Mods::CTRL)),
            Outcome::Pending
        );
        let mut q = press(0xBF, Mods::SHIFT);
        q.ch = Some('?');
        assert_eq!(s.press(&q), Outcome::Fire(Action::MoveTabRight));
        // `release`: fires when the key goes up with nothing pressed in between.
        assert_eq!(s.press(&press(0x78, Mods::NONE)), Outcome::Swallowed);
        assert_eq!(s.release(0x78), Some(Action::HideTab));
        assert_eq!(s.press(&press(0x78, Mods::NONE)), Outcome::Swallowed);
        assert_eq!(
            s.press(&press(u32::from(b'X'), Mods::NONE)),
            Outcome::Ignored
        );
        assert_eq!(s.release(0x78), None, "another key in between");
        // `hold`: the window waits hold_ms; an earlier release does nothing.
        assert_eq!(s.press(&press(0x77, Mods::NONE)), Outcome::Hold(700));
        assert_eq!(s.release(0x77), None);
        assert_eq!(s.hold_elapsed(), None);
        assert_eq!(s.press(&press(0x77, Mods::NONE)), Outcome::Hold(700));
        assert_eq!(s.hold_elapsed(), Some(Action::RenameTerminal));
        // `repeat: true` lets auto-repeat fire again.
        let mut r = press(vk::TAB, Mods::CTRL);
        r.repeat = true;
        assert_eq!(s.press(&r), Outcome::Fire(Action::NextTab));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_names_follow_the_contract() {
        assert_eq!(named_key(u32::from(b'T'), false), Some("T"));
        assert_eq!(named_key(0x7B, false), Some("F12"));
        assert_eq!(named_key(0x0D, true), Some("NumpadEnter"));
        assert_eq!(named_key(0x0D, false), Some("Enter"));
        assert_eq!(named_key(vk::TAB, false), Some("Tab"));
        assert_eq!(
            named_key(0xBF, false),
            None,
            "VK_OEM_2 has no layout-stable name"
        );
        assert_eq!(code_key(0x10, false), Some("KeyQ"));
        assert_eq!(code_key(0x0B, false), Some("Digit0"));
        assert_eq!(code_key(0x48, true), None);
        // Side-specific bindings need that side.
        let dir = sandbox(
            "side",
            json!({"schema": "kim.hotkeys", "schema_version": 1, "tool": "subak",
                "overrides": [{"action": "tab.hide", "bindings": ["RCtrl+H"]}]}),
        );
        let mut s = Matcher::load(Some(&dir), None);
        let mut p = press(u32::from(b'H'), Mods::CTRL);
        p.mods.ctrl = Side::Left;
        assert_eq!(s.press(&p), Outcome::Ignored);
        p.mods.ctrl = Side::Right;
        assert_eq!(s.press(&p), Outcome::Fire(Action::HideTab));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn characters_map_to_xterm_keys() {
        let none = Mods::NONE;
        assert_eq!(char_key('a', false, none), Typed::Key(Key::Char('a'), none));
        assert_eq!(
            char_key('한', false, none),
            Typed::Key(Key::Char('한'), none)
        );
        assert_eq!(
            char_key('x', true, Mods::ALT),
            Typed::Key(Key::Char('x'), Mods::ALT)
        );
        assert_eq!(char_key('\r', false, none), Typed::Key(Key::Enter, none));
        assert_eq!(
            char_key('\t', false, Mods::SHIFT),
            Typed::Key(Key::Tab, Mods::SHIFT)
        );
        assert_eq!(char_key('\u{3}', false, Mods::CTRL), Typed::Raw(vec![3]));
        assert_eq!(
            char_key('\u{7f}', false, Mods::CTRL),
            Typed::Key(Key::Backspace, Mods::CTRL)
        );
        assert_eq!(
            char_key(' ', false, Mods::CTRL),
            Typed::Key(Key::Char(' '), Mods::CTRL)
        );
        // AltGr (Ctrl+Alt) characters are text: `@` on a German layout.
        let altgr = Mods {
            ctrl: true,
            alt: true,
            shift: false,
        };
        assert_eq!(
            char_key('@', false, altgr),
            Typed::Key(Key::Char('@'), none)
        );
        assert_eq!(special_key(vk::F1), Some(Key::F(1)));
        assert_eq!(special_key(vk::F12), Some(Key::F(12)));
        assert_eq!(special_key(u32::from(b'A')), None);
        assert_eq!(ctrl_digit(0x32, Mods::CTRL), Some('2'));
        assert_eq!(ctrl_digit(0x30, Mods::CTRL), None);
    }
}
