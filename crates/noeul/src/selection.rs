//! Capture sessions: the frozen picture and its selection (NOEUL-FR-010, NOEUL-FR-013,
//! NOEUL-FR-025; ADR 0023 §3).
//!
//! A session owns the frozen picture from the moment it is taken until the capture ends. At
//! most one session exists at a time: a second request while one is open is refused, so rapid
//! repeated presses never open a second overlay (NOEUL-FR-013). The selection is a rectangle in
//! device pixels that must lie inside the picture. Finishing a session with "pin" hands out the
//! selected part as a new frame and frees the frozen picture; cancelling frees it at once.
//! Pinning never depends on the clipboard, and a pin that cannot be created gives its picture
//! back ([`Captures::restore`]) so nothing is lost silently (NOEUL-FR-025).
//!
//! Two paths open sessions: `noeul capture --region … --pin` (Step 49: the region is captured
//! and selected as a whole, [`Captures::finish_pin`]) and the selection overlay of
//! `capture.region` (`F1`, Step 50: the picture of every monitor, the selection from the
//! pointer and the keyboard, `crate::overlay`).
//!
//! # The selection result: [`CaptureSelection`]
//!
//! An output action (pin now; copy and save in Step 51) asks the open session for its confirmed
//! selection with [`Captures::selection`]. The answer is a [`CaptureSelection`]: its own copy of
//! the selected pixels (a [`Frame`] cropped from the frozen picture), the rectangle in
//! virtual-desktop device pixels and the monitors it covers (position, DPI, primary).
//!
//! - **Ownership.** The `CaptureSelection` owns its frame; the session keeps the frozen picture
//!   until the action reports success with [`Captures::complete`], which frees it. So a failed
//!   output (a pin window that cannot be created) loses nothing: the overlay stays with its
//!   selection and the user can try again (NOEUL-FR-025). A pin takes the frame with
//!   [`CaptureSelection::into_frame`] and owns it from then on; a copy or save (Step 51) borrows
//!   it with [`CaptureSelection::frame`] and drops the selection afterwards.
//! - **Lifetime.** From the output action until that action ends, on the instance's UI thread;
//!   never stored, never sent over IPC (only its geometry is reported), never written anywhere.
//!   Nothing creates one automatically: confirming a selection in the overlay only records the
//!   rectangle; the pixels stay in the frozen picture until the user picks an output. In
//!   particular nothing is copied to the clipboard and nothing reaches the disk (NOEUL-SEC-002,
//!   NOEUL-SEC-005).
//! - Like every frame, it is counted in the instance's ledger and `Debug` never prints pixels.
//!
//! # The capture history (Step 60, NOEUL-FR-050)
//!
//! A successful capture (pinned, copied or saved) also keeps **the picture it produced** — with
//! its annotations and redactions drawn in, never the pixels a redaction covers — in memory, as
//! a [`Kept`] entry with a number, so it can be pinned again later (`noeul history pin`, the
//! tray). The caller hands that picture over with [`Captures::keep`] when [`Captures::keeps`].
//! The entries are bounded by count and by the pictures' total size (`[history] count`,
//! `memory_mb`; the oldest go first; a picture larger than the whole budget is not kept).
//! A capture the user marked **private** (`capture.private`, `P` in the overlay) joins neither
//! the pictures nor the rectangles; a cancelled one joins only with `[history] keep_cancelled`.
//! Writing them to disk is the caller's opt-in (`crate::store`).

use crate::capture::Monitor;
use crate::frame::{Frame, Rect};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::SystemTime;

/// One earlier capture kept in memory (Step 60).
#[derive(Debug)]
pub struct Kept {
    /// Numbered from 1 for the instance (continuing the disk history when that is on).
    pub id: u64,
    pub rect: Rect,
    pub frame: Frame,
    pub at: SystemTime,
    pub outcome: Outcome,
}

/// The history's limits (`[history]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepLimits {
    pub count: usize,
    pub bytes: u64,
    pub keep_cancelled: bool,
}

/// No pictures until the instance sets the limits from its settings.
impl Default for KeepLimits {
    fn default() -> KeepLimits {
        KeepLimits {
            count: 0,
            bytes: 256 * 1024 * 1024,
            keep_cancelled: false,
        }
    }
}

/// How many rectangles of successful captures the instance remembers, in memory only
/// (`selection.last`, `history.*`; NOEUL-FR-014, NOEUL-FR-050, NOEUL-SEC-005).
pub const HISTORY: usize = crate::overlay::HISTORY;

/// Who asked for the capture (NOEUL-SEC-001: always an explicit action).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Hotkey,
    Tray,
    Cli,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Hotkey => "hotkey",
            Origin::Tray => "tray",
            Origin::Cli => "cli",
        }
    }
}

/// How a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pinned,
    /// Copied to the clipboard (Step 51).
    Copied,
    /// Saved to a file (Step 51).
    Saved,
    Cancelled,
}

#[derive(Debug)]
struct Session {
    id: u64,
    origin: Origin,
    frame: Frame,
    monitors: Vec<Monitor>,
    selection: Option<Rect>,
    /// Marked private (Step 60): joins no history.
    private: bool,
}

/// The confirmed selection handed to an output action. See the module documentation for its
/// ownership and lifetime.
pub struct CaptureSelection {
    session: u64,
    origin: Origin,
    monitors: Vec<Monitor>,
    frame: Frame,
}

impl std::fmt::Debug for CaptureSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Geometry only, never pixels (NOEUL-SEC-008).
        f.debug_struct("CaptureSelection")
            .field("session", &self.session)
            .field("rect", &self.frame.rect())
            .field("monitors", &self.monitors.len())
            .finish_non_exhaustive()
    }
}

impl CaptureSelection {
    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Where the selection is, in virtual-desktop device pixels.
    pub fn rect(&self) -> Rect {
        self.frame.rect()
    }

    /// The monitors the selection covers, in layout order.
    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// The monitor that holds most of the selection (the first of equals).
    pub fn main_monitor(&self) -> Option<Monitor> {
        let r = self.rect();
        let area = |m: &Monitor| {
            crate::overlay::intersection(m.rect, r)
                .map_or(0, |i| u64::from(i.width) * u64::from(i.height))
        };
        let mut best: Option<Monitor> = None;
        for m in &self.monitors {
            if best.is_none_or(|b| area(m) > area(&b)) {
                best = Some(*m);
            }
        }
        best
    }

    /// The selected pixels, for an action that only reads them (copy, save).
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// The selected pixels, moved into a pin.
    pub fn into_frame(self) -> Frame {
        self.frame
    }

    /// Geometry only (NOEUL-SEC-007, NOEUL-SEC-008).
    pub fn to_json(&self) -> Value {
        json!({
            "session": self.session,
            "rect": self.rect().to_json(),
            "monitors": self.monitors.iter().map(|m| json!({
                "rect": m.rect.to_json(), "primary": m.primary, "dpi": m.dpi,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Refusals of the session rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// Another capture is open (`busy`).
    Busy { open: u64 },
    /// No session with this id is open.
    NotOpen,
    /// The selection is empty or not inside the frozen picture.
    BadSelection(Rect),
    /// Nothing is selected yet.
    NoSelection,
}

/// The capture sessions of one instance.
#[derive(Debug, Default)]
pub struct Captures {
    open: Option<Session>,
    next_id: u64,
    /// Rectangles of successful captures, oldest first, at most [`HISTORY`]; never written
    /// anywhere (Step 52).
    history: VecDeque<Rect>,
    started: u64,
    pinned: u64,
    cancelled: u64,
    copied: u64,
    saved: u64,
    ignored: u64,
    /// Captures ended by copying a colour with the picker (Step 59).
    colors: u64,
    /// Step 60: earlier captures' pictures, oldest first, and their limits.
    kept: VecDeque<Kept>,
    next_kept: u64,
    limits: KeepLimits,
    /// Ids kept since [`Captures::take_fresh`] (the caller writes them to disk when that is on).
    fresh: Vec<u64>,
}

impl Captures {
    pub fn new() -> Captures {
        Captures::default()
    }

    /// Opens a session with its frozen picture and the monitors it shows. Refused while another
    /// one is open; the picture is then dropped at once.
    pub fn begin(
        &mut self,
        origin: Origin,
        frame: Frame,
        monitors: Vec<Monitor>,
    ) -> Result<u64, SessionError> {
        if let Some(s) = &self.open {
            return Err(SessionError::Busy { open: s.id });
        }
        self.next_id += 1;
        self.started += 1;
        self.open = Some(Session {
            id: self.next_id,
            origin,
            frame,
            monitors,
            selection: None,
            private: false,
        });
        Ok(self.next_id)
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The id of the open session.
    pub fn open_id(&self) -> Option<u64> {
        self.open.as_ref().map(|s| s.id)
    }

    /// The open session's frozen picture (the overlay draws from it).
    pub fn frozen(&self) -> Option<&Frame> {
        self.open.as_ref().map(|s| &s.frame)
    }

    /// A capture request that found a session open and did nothing (rapid repeated presses,
    /// NOEUL-FR-013); counted for `noeul status`.
    pub fn note_ignored(&mut self) {
        self.ignored += 1;
    }

    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    /// The confirmed selection of the open session as a [`CaptureSelection`] (a copy of its
    /// pixels). The session stays open until [`Captures::complete`].
    pub fn selection(&self, id: u64) -> Result<CaptureSelection, SessionError> {
        let s = match &self.open {
            Some(s) if s.id == id => s,
            _ => return Err(SessionError::NotOpen),
        };
        let rect = s.selection.ok_or(SessionError::NoSelection)?;
        let frame = s.frame.crop(rect).ok_or(SessionError::BadSelection(rect))?;
        Ok(CaptureSelection {
            session: s.id,
            origin: s.origin,
            monitors: s
                .monitors
                .iter()
                .filter(|m| m.rect.intersects(&rect))
                .copied()
                .collect(),
            frame,
        })
    }

    /// Ends the open session after its output succeeded: frees the frozen picture and makes the
    /// selection the "last capture" (`selection.last`, NOEUL-FR-014).
    pub fn complete(&mut self, id: u64, how: Outcome) -> Result<(), SessionError> {
        let s = self.session(id)?;
        let rect = s.selection;
        let private = s.private;
        self.open = None;
        if private {
            match how {
                Outcome::Pinned => self.pinned += 1,
                Outcome::Copied => self.copied += 1,
                Outcome::Saved => self.saved += 1,
                Outcome::Cancelled => self.cancelled += 1,
            }
            return Ok(());
        }
        match how {
            Outcome::Pinned | Outcome::Copied | Outcome::Saved => {
                match how {
                    Outcome::Pinned => self.pinned += 1,
                    Outcome::Copied => self.copied += 1,
                    _ => self.saved += 1,
                }
                if let Some(r) = rect {
                    self.remember(r);
                }
            }
            Outcome::Cancelled => self.cancelled += 1,
        }
        Ok(())
    }

    pub fn origin(&self) -> Option<Origin> {
        self.open.as_ref().map(|s| s.origin)
    }

    fn session(&mut self, id: u64) -> Result<&mut Session, SessionError> {
        match &mut self.open {
            Some(s) if s.id == id => Ok(s),
            _ => Err(SessionError::NotOpen),
        }
    }

    /// Sets (or replaces) the confirmed selection.
    pub fn select(&mut self, id: u64, rect: Rect) -> Result<(), SessionError> {
        let s = self.session(id)?;
        if rect.is_empty() || !s.frame.rect().contains(&rect) {
            return Err(SessionError::BadSelection(rect));
        }
        s.selection = Some(rect);
        Ok(())
    }

    /// Ends the session with "pin": the selected part as a new frame, and the frozen picture is
    /// freed. The rectangle becomes the "last capture" (`selection.last`, NOEUL-FR-014).
    pub fn finish_pin(&mut self, id: u64) -> Result<Frame, SessionError> {
        let s = self.session(id)?;
        let rect = s.selection.ok_or(SessionError::NoSelection)?;
        let private = s.private;
        let out = if rect == s.frame.rect() {
            // The whole picture: move it instead of copying.
            let s = self.open.take().expect("checked above");
            s.frame
        } else {
            let out = s.frame.crop(rect).ok_or(SessionError::BadSelection(rect))?;
            self.open = None;
            out
        };
        if !private {
            self.remember(rect);
        }
        self.pinned += 1;
        Ok(out)
    }

    /// Marks the open session private (Step 60): it joins no history.
    pub fn set_private(&mut self, id: u64, private: bool) -> bool {
        match &mut self.open {
            Some(s) if s.id == id => {
                s.private = private;
                true
            }
            _ => false,
        }
    }

    pub fn is_private(&self) -> bool {
        self.open.as_ref().is_some_and(|s| s.private)
    }

    /// The history's limits; the oldest entries beyond them go at once.
    pub fn set_limits(&mut self, limits: KeepLimits) {
        self.limits = limits;
        self.trim();
    }

    /// The next entry's number (after the disk history's, Step 60).
    pub fn continue_from(&mut self, next: u64) {
        self.next_kept = self.next_kept.max(next.saturating_sub(1));
    }

    /// Keeps a picture (newest last) within the limits; returns its number (`None`: too large).
    pub fn keep(&mut self, frame: Frame, outcome: Outcome) -> Option<u64> {
        if self.limits.count == 0 || frame.rect().bgra_bytes() > self.limits.bytes {
            return None;
        }
        self.next_kept += 1;
        let id = self.next_kept;
        self.kept.push_back(Kept {
            id,
            rect: frame.rect(),
            frame,
            at: SystemTime::now(),
            outcome,
        });
        self.fresh.push(id);
        self.trim();
        Some(id)
    }

    /// Puts an entry read back from the disk history (oldest first) into memory.
    pub fn restore_kept(&mut self, id: u64, frame: Frame, at: SystemTime) {
        self.next_kept = self.next_kept.max(id);
        self.kept.push_back(Kept {
            id,
            rect: frame.rect(),
            frame,
            at,
            outcome: Outcome::Pinned,
        });
        self.trim();
    }

    fn trim(&mut self) {
        let count = self.limits.count;
        while self.kept.len() > count || self.kept_bytes() > self.limits.bytes {
            if self.kept.pop_front().is_none() {
                break;
            }
        }
        let live: Vec<u64> = self.kept.iter().map(|k| k.id).collect();
        self.fresh.retain(|id| live.contains(id));
    }

    /// The pictures' total size in memory.
    pub fn kept_bytes(&self) -> u64 {
        self.kept.iter().map(|k| k.frame.rect().bgra_bytes()).sum()
    }

    /// The kept captures, oldest first.
    pub fn kept(&self) -> &VecDeque<Kept> {
        &self.kept
    }

    pub fn kept_entry(&self, id: u64) -> Option<&Kept> {
        self.kept.iter().find(|k| k.id == id)
    }

    /// Ids kept since the last call (to write to disk when that is on).
    pub fn take_fresh(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.fresh)
    }

    /// Forgets one entry (its picture is freed).
    pub fn forget(&mut self, id: u64) -> bool {
        let before = self.kept.len();
        self.kept.retain(|k| k.id != id);
        self.fresh.retain(|f| *f != id);
        self.kept.len() != before
    }

    /// Forgets every entry and the rectangles (`noeul history clear`).
    pub fn clear_history(&mut self) -> usize {
        let n = self.kept.len();
        self.kept.clear();
        self.fresh.clear();
        self.history.clear();
        n
    }

    /// Adds a successful capture's rectangle to the history (a repeat of the newest is not
    /// added again; the oldest falls out beyond [`HISTORY`]).
    fn remember(&mut self, r: Rect) {
        if self.history.back() != Some(&r) {
            self.history.push_back(r);
        }
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    /// A pin could not be created from `frame`: reopen the session with it so the user can try
    /// again or copy and save instead (NOEUL-FR-025). The counters stay honest.
    pub fn restore(&mut self, origin: Origin, frame: Frame) -> u64 {
        self.pinned = self.pinned.saturating_sub(1);
        self.started = self.started.saturating_sub(1);
        let rect = frame.rect();
        let id = self.begin(origin, frame, Vec::new()).unwrap_or(0);
        let _ = self.select(id, rect);
        id
    }

    /// Ends the open session `id` after the colour picker copied a pixel's colour (Step 59): the
    /// picture is freed and nothing joins the history (no rectangle was captured).
    pub fn color_copied(&mut self, id: u64) -> bool {
        if self.open.as_ref().is_some_and(|s| s.id == id) {
            self.open = None;
            self.colors += 1;
            true
        } else {
            false
        }
    }

    /// How many captures ended with a colour copied by the picker (Step 59).
    pub fn colors(&self) -> u64 {
        self.colors
    }

    /// Cancels the open session (any id); its picture is freed. `false` when none was open.
    pub fn cancel(&mut self) -> bool {
        let was = self.open.take().is_some();
        if was {
            self.cancelled += 1;
        }
        was
    }

    /// Whether a capture ending as `how` joins the picture history (Step 60): history on, the
    /// open session not private, and a cancelled one only with `keep_cancelled`.
    pub fn keeps(&self, how: Outcome) -> bool {
        self.limits.count > 0
            && !self.is_private()
            && (how != Outcome::Cancelled || self.limits.keep_cancelled)
    }

    /// The rectangle of the last successful capture.
    pub fn last(&self) -> Option<Rect> {
        self.history.back().copied()
    }

    /// The rectangles of earlier successful captures, oldest first (memory only).
    pub fn history(&self) -> Vec<Rect> {
        self.history.iter().copied().collect()
    }

    /// `(started, pinned, cancelled)` since the instance started (a local metric, ADR 0012);
    /// ignored requests are [`Captures::ignored`].
    pub fn counts(&self) -> (u64, u64, u64) {
        (self.started, self.pinned, self.cancelled)
    }

    /// `(copied, saved)` since the instance started (Step 51).
    pub fn output_counts(&self) -> (u64, u64) {
        (self.copied, self.saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Ledger;

    #[test]
    fn one_session_at_a_time_and_pictures_are_released() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        let f = Frame::solid(&ledger, Rect::new(0, 0, 10, 10), [0, 0, 0, 255]).unwrap();
        let id = c.begin(Origin::Hotkey, f, Vec::new()).unwrap();
        // Rapid repeated presses: refused, and their pictures are dropped at once.
        for _ in 0..5 {
            let again = Frame::solid(&ledger, Rect::new(0, 0, 10, 10), [0; 4]).unwrap();
            assert_eq!(
                c.begin(Origin::Hotkey, again, Vec::new()).unwrap_err(),
                SessionError::Busy { open: id }
            );
        }
        assert_eq!(
            ledger.live(),
            (1, 400),
            "only the open session's picture is alive"
        );
        assert!(c.cancel());
        assert_eq!(ledger.live(), (0, 0), "cancel frees the frozen picture");
        assert!(!c.cancel());
        assert_eq!(c.counts(), (1, 0, 1));
    }

    #[test]
    fn pinning_hands_out_the_selection_and_frees_the_rest() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        let f = Frame::solid(&ledger, Rect::new(-20, -20, 40, 40), [9, 9, 9, 255]).unwrap();
        let id = c.begin(Origin::Cli, f, Vec::new()).unwrap();
        assert_eq!(c.finish_pin(id).unwrap_err(), SessionError::NoSelection);
        assert_eq!(
            c.select(id, Rect::new(10, 10, 20, 20)),
            Err(SessionError::BadSelection(Rect::new(10, 10, 20, 20)))
        );
        assert_eq!(
            c.select(id + 1, Rect::new(0, 0, 1, 1)),
            Err(SessionError::NotOpen)
        );
        c.select(id, Rect::new(-10, -10, 5, 6)).unwrap();
        let pin = c.finish_pin(id).unwrap();
        assert_eq!(pin.rect(), Rect::new(-10, -10, 5, 6));
        assert!(!c.is_open());
        assert_eq!(ledger.live(), (1, 120), "only the pinned part is kept");
        assert_eq!(c.last(), Some(Rect::new(-10, -10, 5, 6)));
        drop(pin);
        assert_eq!(ledger.live(), (0, 0));
    }

    #[test]
    fn a_failed_pin_gives_the_picture_back() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        let f = Frame::solid(&ledger, Rect::new(0, 0, 8, 8), [1; 4]).unwrap();
        let id = c.begin(Origin::Cli, f, Vec::new()).unwrap();
        c.select(id, Rect::new(0, 0, 8, 8)).unwrap();
        let whole = c.finish_pin(id).unwrap();
        assert_eq!(
            ledger.live(),
            (1, 256),
            "a whole-picture pin moves, not copies"
        );
        let id = c.restore(Origin::Cli, whole);
        assert!(c.is_open());
        assert_eq!(c.counts(), (1, 0, 0));
        let again = c.finish_pin(id).unwrap();
        assert_eq!(again.rect(), Rect::new(0, 0, 8, 8));
    }

    #[test]
    fn a_capture_selection_copies_the_selection_and_keeps_the_session_until_complete() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        let mons = vec![
            Monitor::new(Rect::new(0, 0, 20, 10), true),
            Monitor::new(Rect::new(20, 0, 20, 10), false).with_dpi(144),
        ];
        let f = Frame::solid(&ledger, Rect::new(0, 0, 40, 10), [3; 4]).unwrap();
        let id = c.begin(Origin::Hotkey, f, mons).unwrap();
        assert_eq!(c.selection(id).unwrap_err(), SessionError::NoSelection);
        c.select(id, Rect::new(15, 2, 10, 4)).unwrap();
        let sel = c.selection(id).unwrap();
        assert_eq!(sel.rect(), Rect::new(15, 2, 10, 4));
        assert_eq!(sel.monitors().len(), 2, "it crosses the seam");
        assert_eq!(sel.main_monitor().unwrap().dpi, 96, "a tie keeps the first");
        assert_eq!(sel.origin(), Origin::Hotkey);
        assert!(!format!("{sel:?}").contains("[3"), "no pixels in Debug");
        assert_eq!(sel.to_json()["monitors"][1]["dpi"], 144);
        assert_eq!(
            ledger.live(),
            (2, 40 * 10 * 4 + 10 * 4 * 4),
            "the frozen picture stays while the output runs"
        );
        assert!(c.is_open());
        // The output failed: nothing is lost, the session is still there.
        drop(sel);
        assert_eq!(c.frozen().unwrap().rect(), Rect::new(0, 0, 40, 10));
        // It succeeded this time: the pin owns the pixels, the frozen picture is freed.
        let pin = c.selection(id).unwrap().into_frame();
        c.complete(id, Outcome::Pinned).unwrap();
        assert!(!c.is_open());
        assert_eq!(ledger.live(), (1, 160));
        assert_eq!(c.last(), Some(Rect::new(15, 2, 10, 4)));
        assert_eq!(c.counts(), (1, 1, 0));
        drop(pin);
        assert_eq!(ledger.live(), (0, 0));
        assert_eq!(c.complete(id, Outcome::Pinned), Err(SessionError::NotOpen));
        c.note_ignored();
        assert_eq!(c.ignored(), 1);
    }

    #[test]
    fn the_history_keeps_successful_rectangles_in_memory_bounded() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        c.set_limits(KeepLimits {
            count: 20,
            ..KeepLimits::default()
        });
        let pic = Rect::new(0, 0, 100, 100);
        let run = |c: &mut Captures, r: Rect, how: Outcome| {
            let f = Frame::solid(&ledger, pic, [0; 4]).unwrap();
            let id = c.begin(Origin::Hotkey, f, Vec::new()).unwrap();
            c.select(id, r).unwrap();
            if how == Outcome::Cancelled {
                c.cancel();
            } else {
                c.complete(id, how).unwrap();
            }
        };
        run(&mut c, Rect::new(0, 0, 10, 10), Outcome::Pinned);
        run(&mut c, Rect::new(1, 1, 10, 10), Outcome::Copied);
        run(&mut c, Rect::new(1, 1, 10, 10), Outcome::Saved); // a repeat of the newest
        run(&mut c, Rect::new(2, 2, 10, 10), Outcome::Cancelled); // never recorded
        assert_eq!(
            c.history(),
            [Rect::new(0, 0, 10, 10), Rect::new(1, 1, 10, 10)],
            "only successful captures, oldest first, no repeated newest"
        );
        assert_eq!(c.last(), Some(Rect::new(1, 1, 10, 10)));
        for i in 0..30 {
            run(&mut c, Rect::new(i, 0, 5, 5), Outcome::Pinned);
        }
        let h = c.history();
        assert_eq!(h.len(), HISTORY, "bounded");
        assert_eq!(
            h.first(),
            Some(&Rect::new(10, 0, 5, 5)),
            "the oldest fell out"
        );
        assert_eq!(c.last(), Some(Rect::new(29, 0, 5, 5)));
        // Step 60: the session keeps no picture by itself (the caller hands over what it
        // produced, `Captures::keep`); clearing forgets the rectangles too.
        assert!(c.kept().is_empty());
        assert_eq!(ledger.live(), (0, 0), "rectangles only: no picture is kept");
        c.clear_history();
        assert!(c.history().is_empty());
    }

    /// Step 60 (NOEUL-FR-050, NOEUL-FR-051): pictures kept by count and memory; a private
    /// capture joins nothing; a cancelled one only with `keep_cancelled`; ids continue.
    #[test]
    fn the_picture_history_is_bounded_and_private_captures_are_left_out() {
        let ledger = Ledger::new();
        let mut c = Captures::new();
        let pic = Rect::new(0, 0, 100, 100);
        let start = |c: &mut Captures| {
            let f = Frame::solid(&ledger, pic, [1, 2, 3, 255]).unwrap();
            let id = c.begin(Origin::Hotkey, f, Vec::new()).unwrap();
            c.select(id, Rect::new(0, 0, 10, 10)).unwrap();
            id
        };
        c.set_limits(KeepLimits {
            count: 3,
            bytes: 10 * 10 * 4 * 2,
            keep_cancelled: false,
        });
        c.continue_from(41);
        for _ in 0..3 {
            let id = start(&mut c);
            assert!(c.keeps(Outcome::Copied));
            let sel = c.selection(id).unwrap();
            c.keep(sel.into_frame(), Outcome::Copied);
            c.complete(id, Outcome::Copied).unwrap();
        }
        let ids: Vec<u64> = c.kept().iter().map(|k| k.id).collect();
        assert_eq!(
            ids,
            [42, 43],
            "numbered after the disk history; two fit in the memory budget"
        );
        assert_eq!(c.take_fresh(), [42, 43]);
        // Private: no picture, no rectangle.
        let id = start(&mut c);
        assert!(c.set_private(id, true) && c.is_private());
        assert!(!c.keeps(Outcome::Saved), "a private capture keeps nothing");
        let before = c.history().len();
        c.complete(id, Outcome::Saved).unwrap();
        assert_eq!((c.kept().len(), c.history().len()), (2, before));
        assert!(c.take_fresh().is_empty());
        // Cancelled: kept only when asked.
        let _ = start(&mut c);
        assert!(!c.keeps(Outcome::Cancelled));
        c.cancel();
        c.set_limits(KeepLimits {
            count: 3,
            bytes: 1 << 20,
            keep_cancelled: true,
        });
        let _ = start(&mut c);
        assert!(c.keeps(Outcome::Cancelled));
        c.cancel();
        // A kept copy is separate from the pin's own frame.
        let id = start(&mut c);
        let pinned = c.finish_pin(id).unwrap();
        let kept = c
            .keep(pinned.crop(pinned.rect()).unwrap(), Outcome::Pinned)
            .unwrap();
        assert_eq!(c.kept().back().unwrap().rect, pinned.rect());
        assert!(c.forget(kept));
        // Too large for the whole budget: not kept.
        c.set_limits(KeepLimits {
            count: 3,
            bytes: 16,
            keep_cancelled: false,
        });
        assert!(c.kept().is_empty(), "trimmed to the new budget");
        assert_eq!(
            c.keep(Frame::solid(&ledger, pic, [0; 4]).unwrap(), Outcome::Copied),
            None
        );
        drop(pinned);
        assert_eq!(ledger.live(), (0, 0));
    }
}
