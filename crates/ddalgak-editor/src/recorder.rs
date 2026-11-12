//! Step 78 (DDALGAK-FR-074, DDALGAK-SEC-001, ADR 0079): the macro recorder.
//!
//! Recording is **explicit and local**: it starts only with *Record keys*, shows a banner the
//! whole time, ends with *Stop recording* or `Esc`, and sees only the keys typed into the
//! editor's own window — never another program, never a password field elsewhere. Since #101 *Record mouse*
//! records presses and wheel turns (`ddalgak::win::mouse_record`, ADR 0086) the same explicit way. There is no hook, no background recording and nothing is written until the user saves
//! the rule after reviewing (and editing or removing) every recorded step.
//!
//! Typed characters become `type_text` steps, keys and shortcuts that type nothing (Enter, Tab,
//! arrows, F-keys, anything with Ctrl or Alt) become `send_keys` steps of up to eight chords, and
//! a pause longer than [`PAUSE_S`] between them becomes a `delay` step (rounded to 50 ms, at most
//! 10 s).

use eframe::egui::{self, Event, Key, Modifiers};
use serde_json::{Value, json};

/// A pause longer than this between keys is kept as a `delay` step.
pub const PAUSE_S: f64 = 0.3;
/// The most steps one recording makes (a macro has at most 32).
pub const MAX_STEPS: usize = 32;

#[derive(Debug, Default, Clone)]
pub struct Recorder {
    on: bool,
    steps: Vec<Value>,
    text: String,
    chords: Vec<String>,
    last: Option<f64>,
    /// The recording reached [`MAX_STEPS`] and stopped taking keys.
    pub full: bool,
}

/// The contract name of an egui key (`None`: not recordable).
fn key_name(k: Key) -> Option<String> {
    let n = match k {
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Space => "Space",
        _ => {
            let n = k.name();
            let ok = (n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric()))
                || (n.starts_with('F')
                    && n[1..].parse::<u8>().is_ok_and(|f| (1..=24).contains(&f)));
            return ok.then(|| n.to_string());
        }
    };
    Some(n.to_string())
}

/// Keys that type nothing on their own (recorded as a chord even without modifiers).
fn is_command(k: Key) -> bool {
    matches!(
        k,
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Enter
            | Key::Tab
            | Key::Backspace
            | Key::Delete
            | Key::Insert
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
    ) || key_name(k).is_some_and(|n| n.starts_with('F') && n.len() > 1)
}

impl Recorder {
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// How many steps the recording holds so far (pending text and keys count as one each).
    pub fn len(&self) -> usize {
        self.steps.len() + usize::from(!self.text.is_empty()) + usize::from(!self.chords.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn start(&mut self) {
        *self = Recorder {
            on: true,
            ..Recorder::default()
        };
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let t = std::mem::take(&mut self.text);
            self.push(json!({ "type": "type_text", "text": t }));
        }
    }

    fn flush_chords(&mut self) {
        if !self.chords.is_empty() {
            let k = std::mem::take(&mut self.chords).join(" ");
            self.push(json!({ "type": "send_keys", "keys": k }));
        }
    }

    fn push(&mut self, step: Value) {
        if self.steps.len() < MAX_STEPS {
            self.steps.push(step);
        }
        if self.steps.len() >= MAX_STEPS {
            self.full = true;
        }
    }

    fn gap(&mut self, now: f64) {
        if let Some(last) = self.last {
            let s = now - last;
            if s > PAUSE_S {
                self.flush_text();
                self.flush_chords();
                let ms = ((s * 1000.0 / 50.0).round() * 50.0).clamp(50.0, 10_000.0) as u32;
                self.push(json!({ "type": "delay", "ms": ms }));
            }
        }
        self.last = Some(now);
    }

    /// Takes one event of the editor window; returns `true` when it ended the recording (`Esc`).
    pub fn feed(&mut self, ev: &Event, now: f64) -> bool {
        if !self.on || self.full {
            return matches!(
                ev,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            );
        }
        match ev {
            Event::Key {
                key: Key::Escape,
                pressed: true,
                ..
            } => return true,
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let shortcut = modifiers.ctrl || modifiers.alt || modifiers.command;
                if !(shortcut || is_command(*key)) {
                    return false; // its character arrives as text
                }
                let Some(name) = key_name(*key) else {
                    return false;
                };
                self.gap(now);
                self.flush_text();
                let m = modifiers;
                let mut chord = String::new();
                for (on, n) in [
                    (m.ctrl || m.command, "Ctrl"),
                    (m.alt, "Alt"),
                    (m.shift, "Shift"),
                ] {
                    if on {
                        chord.push_str(n);
                        chord.push('+');
                    }
                }
                chord.push_str(&name);
                self.chords.push(chord);
                if self.chords.len() == 8 {
                    self.flush_chords();
                }
            }
            Event::Text(t) | Event::Ime(egui::ImeEvent::Commit(t)) => {
                if t.chars().all(char::is_control) {
                    return false;
                }
                self.gap(now);
                self.flush_chords();
                if self.text.chars().count() + t.chars().count() > 2000 {
                    self.flush_text();
                }
                self.text.push_str(t);
            }
            _ => {}
        }
        false
    }

    /// Ends the recording; the steps go to the macro for review (nothing is saved here).
    pub fn stop(&mut self) -> Vec<Value> {
        self.flush_text();
        self.flush_chords();
        self.on = false;
        std::mem::take(&mut self.steps)
    }
}

/// The modifiers an egui key event needs to be a shortcut in tests.
pub fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::default()
    }
}

/// #101: the steps a mouse recording becomes: a `mouse_click` at each press's screen point (two
/// presses of one button within 500 ms and 4 px become a `double_click`), wheel turns in one
/// direction within 300 ms one `mouse_scroll` (notches rounded, at least 1, at most 50), and a
/// pause longer than [`PAUSE_S`] a `delay` (rounded to 50 ms, at most 10 s). At most
/// [`MAX_STEPS`] steps.
pub fn mouse_steps(events: &[ddalgak::mouse::RecEvent]) -> Vec<Value> {
    use ddalgak::mouse::RecEvent;
    let mut out: Vec<Value> = Vec::new();
    let mut last: Option<(u64, &RecEvent)> = None;
    let mut wheel_delta = 0i32;
    for e in events {
        let at = match e {
            RecEvent::Click { at_ms, .. } | RecEvent::Wheel { at_ms, .. } => *at_ms,
        };
        let gap = last.map_or(0, |(t, _)| at.saturating_sub(t));
        let joined = match (e, last) {
            (
                RecEvent::Click { button, x, y, .. },
                Some((
                    _,
                    RecEvent::Click {
                        button: b,
                        x: px,
                        y: py,
                        ..
                    },
                )),
            ) if button == b
                && gap <= 500
                && (x - px).abs() <= 4
                && (y - py).abs() <= 4
                && out
                    .last()
                    .is_some_and(|s| s["type"] == "mouse_click" && s.get("op").is_none()) =>
            {
                if let Some(s) = out.last_mut() {
                    s["op"] = json!("double_click");
                }
                true
            }
            (
                RecEvent::Wheel {
                    direction, delta, ..
                },
                Some((_, RecEvent::Wheel { direction: d, .. })),
            ) if direction == d && gap <= 300 => {
                wheel_delta += delta;
                if let Some(s) = out.last_mut() {
                    s["notches"] = json!(((wheel_delta + 60) / 120).clamp(1, 50));
                }
                true
            }
            _ => false,
        };
        if !joined {
            if last.is_some() && gap as f64 / 1000.0 > PAUSE_S {
                let ms = ((gap + 25) / 50 * 50).clamp(50, 10_000);
                out.push(json!({ "type": "delay", "ms": ms }));
            }
            match e {
                RecEvent::Click { button, x, y, .. } => out.push(json!({
                    "type": "mouse_click", "button": button,
                    "at": { "x": (*x).clamp(-32768, 32767), "y": (*y).clamp(-32768, 32767) }
                })),
                RecEvent::Wheel {
                    direction, delta, ..
                } => {
                    wheel_delta = *delta;
                    out.push(json!({
                        "type": "mouse_scroll", "direction": direction,
                        "notches": ((delta + 60) / 120).clamp(1, 50)
                    }));
                }
            }
        }
        last = Some((at, e));
        if out.len() >= MAX_STEPS {
            out.truncate(MAX_STEPS);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Text, shortcuts and pauses become steps in order; Esc stops and is not recorded.
    #[test]
    fn typed_text_shortcuts_and_pauses_become_steps() {
        let mut r = Recorder::default();
        assert!(
            !r.feed(&Event::Text("x".into()), 0.0) && r.is_empty(),
            "off: nothing"
        );
        r.start();
        r.feed(&Event::Text("안".into()), 0.0);
        r.feed(&Event::Ime(egui::ImeEvent::Commit("녕".into())), 0.1);
        r.feed(&key(Key::A, Modifiers::default()), 0.15); // its text comes as Text
        r.feed(&key(Key::Enter, Modifiers::default()), 0.2);
        r.feed(&key(Key::S, ctrl()), 1.2);
        assert!(r.feed(&key(Key::Escape, Modifiers::default()), 1.3));
        let steps = r.stop();
        assert_eq!(
            steps,
            [
                json!({"type": "type_text", "text": "안녕"}),
                json!({"type": "send_keys", "keys": "Enter"}),
                json!({"type": "delay", "ms": 1000}),
                json!({"type": "send_keys", "keys": "Ctrl+S"}),
            ]
        );
        assert!(!r.is_on());
        // What the recorder makes is a macro the engine accepts.
        let n = ddalgak::rules::check_macro(&steps, 1, &ddalgak::modes::ModeSet::single());
        assert_eq!(n, Ok(4));
    }

    /// At most 32 steps; then it stops taking keys (the banner says the recording is full).
    #[test]
    fn a_recording_is_bounded() {
        let mut r = Recorder::default();
        r.start();
        for i in 0..100 {
            r.feed(&key(Key::Enter, Modifiers::default()), f64::from(i));
        }
        assert!(r.full);
        assert_eq!(r.stop().len(), MAX_STEPS);
    }

    /// #101: presses become clicks at their points, a quick second press a double click, wheel
    /// turns one scroll step, pauses delays; the result is bounded.
    #[test]
    fn mouse_events_become_steps() {
        use ddalgak::mouse::RecEvent::{Click, Wheel};
        let ev = [
            Click {
                button: "left",
                x: 100,
                y: 200,
                at_ms: 0,
            },
            Click {
                button: "left",
                x: 101,
                y: 201,
                at_ms: 200,
            },
            Wheel {
                direction: "down",
                delta: 120,
                at_ms: 1000,
            },
            Wheel {
                direction: "down",
                delta: 240,
                at_ms: 1100,
            },
            Click {
                button: "right",
                x: -5,
                y: 40_000,
                at_ms: 1150,
            },
        ];
        assert_eq!(
            mouse_steps(&ev),
            [
                json!({"type": "mouse_click", "button": "left", "op": "double_click", "at": {"x": 100, "y": 200}}),
                json!({"type": "delay", "ms": 800}),
                json!({"type": "mouse_scroll", "direction": "down", "notches": 3}),
                json!({"type": "mouse_click", "button": "right", "at": {"x": -5, "y": 32767}}),
            ]
        );
        let many: Vec<_> = (0..100)
            .map(|i| Click {
                button: "left",
                x: i * 10,
                y: 0,
                at_ms: u64::try_from(i).unwrap() * 1000,
            })
            .collect();
        assert_eq!(mouse_steps(&many).len(), MAX_STEPS);
    }
}
