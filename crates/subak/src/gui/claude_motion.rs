//! Step 44 (`SUBAK-FR-054`, ADR 0073): the tiny per-Terminal Claude Code status motion.
//!
//! A pure function of one Terminal's Claude state and the time since that state began: which
//! glyph its mark shows, whether it is drawn faint, and **when the next frame is due** (`None`:
//! nothing will change, so no timer runs). The window draws a frame only for tabs it shows,
//! only while it is not minimized, and arms one timer at the earliest due frame; the Host only
//! reports state changes (`terminal.claude_changed`) and animates nothing.
//!
//! The motion language (docs/SUBAK_BRAND.md §4), shape first, never colour alone:
//!
//! | State | Glyph | Motion | Then |
//! | --- | --- | --- | --- |
//! | in-progress | ◐ ◓ ◑ ◒ | a quarter turn every 300 ms (a 1.2 s revolution) | continues while working |
//! | need-input | `!` | three faint/full blinks, 250 ms each (1.5 s) | steady, full |
//! | error | `✕` | two short flashes, 150 ms each (0.6 s) | steady, full |
//! | done | `✓` | faint then full, 150 ms (a soft arrival) | steady |
//! | idle | `◦` | none (resting) | — |
//! | unknown | `?` | none (honest, static) | — |
//!
//! Idle does not "breathe": it is the common state of a Terminal with Claude, and a slow pulse
//! there would wake the window several times a second for no information. **Static mode**
//! (`terminal.claude_animation = false`, `appearance.motion = "reduced"`, or Windows' "Show
//! animations" off) shows each state's final frame at once and never arms a timer.

use super::float::art::Claude;
use std::time::Duration;

/// In-progress: one quarter turn.
pub const SPIN_MS: u64 = 300;
/// The in-progress glyphs, a quarter turn apart.
pub const SPIN: [char; 4] = ['◐', '◓', '◑', '◒'];
/// Need-input: one blink half (faint or full), and how many halves.
pub const BLINK_MS: u64 = 250;
pub const BLINK_HALVES: u64 = 6;
/// Error: one flash half, and how many halves.
pub const FLASH_MS: u64 = 150;
pub const FLASH_HALVES: u64 = 4;
/// Done: the faint arrival before the steady check.
pub const ARRIVE_MS: u64 = 150;

/// How the marks move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The motion above.
    Animated,
    /// Final frames only, no timer.
    Static,
}

/// One frame of a Terminal's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub glyph: char,
    /// Drawn faint (the dim half of a blink or flash, the arrival of done).
    pub faint: bool,
}

/// The steady (static-mode and final) glyph of a state; `None` draws no mark.
pub fn rest(c: Claude) -> Option<char> {
    Some(match c {
        Claude::InProgress => '◔',
        Claude::NeedInput => '!',
        Claude::Error => '✕',
        Claude::Done => '✓',
        Claude::Idle => '◦',
        Claude::Unknown => '?',
        Claude::None => return None,
    })
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The frame of `c`, `since` it began.
pub fn frame(c: Claude, since: Duration, mode: Mode) -> Option<Frame> {
    let glyph = rest(c)?;
    let steady = Frame {
        glyph,
        faint: false,
    };
    if mode == Mode::Static {
        return Some(steady);
    }
    let t = ms(since);
    Some(match c {
        Claude::InProgress => Frame {
            glyph: SPIN[((t / SPIN_MS) % 4) as usize],
            faint: false,
        },
        Claude::NeedInput if t < BLINK_MS * BLINK_HALVES => Frame {
            glyph,
            faint: (t / BLINK_MS).is_multiple_of(2),
        },
        Claude::Error if t < FLASH_MS * FLASH_HALVES => Frame {
            glyph,
            faint: !(t / FLASH_MS).is_multiple_of(2),
        },
        Claude::Done if t < ARRIVE_MS => Frame { glyph, faint: true },
        _ => steady,
    })
}

/// How long until the frame of `c` changes (`None`: never; no timer for it).
pub fn next_change(c: Claude, since: Duration, mode: Mode) -> Option<Duration> {
    if mode == Mode::Static {
        return None;
    }
    let t = ms(since);
    let step = |unit: u64, until: u64| (t < until).then(|| Duration::from_millis(unit - t % unit));
    match c {
        Claude::InProgress => step(SPIN_MS, u64::MAX),
        Claude::NeedInput => step(BLINK_MS, BLINK_MS * BLINK_HALVES),
        Claude::Error => step(FLASH_MS, FLASH_MS * FLASH_HALVES),
        Claude::Done => step(ARRIVE_MS, ARRIVE_MS),
        Claude::Idle | Claude::Unknown | Claude::None => None,
    }
}

/// The earliest change among the shown marks (the one timer the window arms).
pub fn earliest(
    marks: impl IntoIterator<Item = (Claude, Duration)>,
    mode: Mode,
) -> Option<Duration> {
    marks
        .into_iter()
        .filter_map(|(c, s)| next_change(c, s, mode))
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(c: Claude, t: u64) -> Frame {
        frame(c, Duration::from_millis(t), Mode::Animated).unwrap()
    }

    /// `SUBAK-FR-054`: every state has its own shape; motion only adds to it.
    #[test]
    fn each_state_has_a_distinct_resting_shape() {
        let all = [
            Claude::InProgress,
            Claude::NeedInput,
            Claude::Error,
            Claude::Done,
            Claude::Idle,
            Claude::Unknown,
        ];
        let mut g: Vec<char> = all.iter().map(|c| rest(*c).unwrap()).collect();
        g.sort_unstable();
        g.dedup();
        assert_eq!(g.len(), all.len());
        assert_eq!(rest(Claude::None), None);
        assert_eq!(frame(Claude::None, Duration::ZERO, Mode::Animated), None);
        // The spin glyphs never collide with another state's shape.
        for s in SPIN {
            assert!(!g.contains(&s));
        }
    }

    #[test]
    fn in_progress_turns_continuously_a_quarter_every_300_ms() {
        let seq: Vec<char> = (0..8)
            .map(|k| at(Claude::InProgress, k * SPIN_MS).glyph)
            .collect();
        assert_eq!(seq, ['◐', '◓', '◑', '◒', '◐', '◓', '◑', '◒']);
        assert_eq!(at(Claude::InProgress, 299).glyph, '◐');
        let n = |t| next_change(Claude::InProgress, Duration::from_millis(t), Mode::Animated);
        assert_eq!(n(0), Some(Duration::from_millis(300)));
        assert_eq!(
            n(1_000_100),
            Some(Duration::from_millis(100)),
            "never stops while working"
        );
    }

    #[test]
    fn need_input_blinks_three_times_then_holds() {
        let faint: Vec<bool> = (0..7)
            .map(|k| at(Claude::NeedInput, k * BLINK_MS).faint)
            .collect();
        assert_eq!(faint, [true, false, true, false, true, false, false]);
        let n = |t| next_change(Claude::NeedInput, Duration::from_millis(t), Mode::Animated);
        assert_eq!(n(0), Some(Duration::from_millis(250)));
        assert_eq!(n(1_400), Some(Duration::from_millis(100)));
        assert_eq!(n(1_500), None, "no timer once the cue is over");
        assert_eq!(
            at(Claude::NeedInput, 60_000),
            Frame {
                glyph: '!',
                faint: false
            }
        );
    }

    #[test]
    fn error_flashes_briefly_and_done_arrives_softly() {
        let e: Vec<bool> = (0..5)
            .map(|k| at(Claude::Error, k * FLASH_MS).faint)
            .collect();
        assert_eq!(e, [false, true, false, true, false]);
        assert_eq!(
            next_change(Claude::Error, Duration::from_millis(600), Mode::Animated),
            None
        );
        assert!(at(Claude::Done, 0).faint);
        assert!(!at(Claude::Done, ARRIVE_MS).faint);
        assert_eq!(
            next_change(Claude::Done, Duration::from_millis(150), Mode::Animated),
            None
        );
    }

    #[test]
    fn idle_and_unknown_never_move_and_static_mode_never_arms_a_timer() {
        for c in [Claude::Idle, Claude::Unknown, Claude::None] {
            assert_eq!(next_change(c, Duration::ZERO, Mode::Animated), None);
        }
        for c in [
            Claude::InProgress,
            Claude::NeedInput,
            Claude::Error,
            Claude::Done,
        ] {
            assert_eq!(next_change(c, Duration::ZERO, Mode::Static), None);
            let f = frame(c, Duration::ZERO, Mode::Static).unwrap();
            assert_eq!(
                f,
                Frame {
                    glyph: rest(c).unwrap(),
                    faint: false
                },
                "{c:?}"
            );
        }
    }

    /// One timer for all shown marks: the earliest change wins; settled marks add nothing.
    #[test]
    fn the_window_arms_one_timer_at_the_earliest_change() {
        let d = Duration::from_millis;
        let marks = [
            (Claude::Idle, d(0)),
            (Claude::InProgress, d(250)),
            (Claude::NeedInput, d(10)),
            (Claude::Done, d(5_000)),
        ];
        assert_eq!(earliest(marks, Mode::Animated), Some(d(50)));
        assert_eq!(earliest(marks, Mode::Static), None);
        assert_eq!(
            earliest(
                [(Claude::Idle, d(0)), (Claude::Error, d(900))],
                Mode::Animated
            ),
            None
        );
    }
}
