//! Keyboard shortcuts in egui windows (HOTKEY_CONTRACT §5, ADR 0092): egui's keys as the
//! contract's key names, and the **key picker**: a shortcut built by clicking keys instead of
//! pressing them (keys that are hard to press in a window, like `PrintScreen`, side-specific
//! modifiers or `CapsLock`).

use crate::{Button, Colors, ui_font};
use egui::{RichText, Ui};
use kim_design::{font, space};

/// The contract's name of an egui key (`ArrowUp` → `Up`, `F5`, `A`, …); `None` for keys the
/// contract cannot bind.
pub fn key_name(k: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    let name = match k {
        K::ArrowUp => "Up",
        K::ArrowDown => "Down",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        K::Plus | K::Equals => "Plus",
        K::Minus => "Minus",
        K::Comma => "Comma",
        K::Period => "Period",
        other => other.name(),
    };
    kim_hotkeys::keys::NAMED_KEYS
        .iter()
        .find(|n| **n == name)
        .copied()
}

/// `Ctrl+Shift+K` for a key and its modifiers (contract order: Ctrl, Alt, Shift, Win); `None`
/// for a key without a contract name.
pub fn chord_text(k: egui::Key, m: egui::Modifiers) -> Option<String> {
    let key = key_name(k)?;
    let mut parts: Vec<&str> = Vec::new();
    if m.ctrl {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// The modifiers a picker offers: either-side ones, or side-specific ones as well.
pub fn modifiers(sides: bool) -> Vec<&'static str> {
    let mut v = vec!["Ctrl", "Alt", "Shift", "Win"];
    if sides {
        v.extend([
            "LCtrl", "RCtrl", "LAlt", "RAlt", "LShift", "RShift", "LWin", "RWin",
        ]);
    }
    v
}

/// The key picker's state: chosen modifiers and key, and the search text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyPicker {
    pub mods: Vec<&'static str>,
    pub key: Option<String>,
    pub search: String,
}

/// What the picker's buttons did this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    None,
    /// `Confirm`: the chord text (`Ctrl+PrintScreen`).
    Confirm(String),
    Cancel,
}

impl KeyPicker {
    /// The chord built so far (`RCtrl+CapsLock`), or `None` without a key.
    pub fn chord(&self) -> Option<String> {
        let key = self.key.as_ref()?;
        let mut parts: Vec<&str> = self.mods.clone();
        parts.push(key);
        Some(parts.join("+"))
    }

    /// Draws the picker: modifier toggles, a searchable grid of `keys`, the result as keycaps
    /// and Clear / Cancel / Confirm. `sides` offers left/right modifiers too.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        keys: &[&str],
        sides: bool,
        lang: kim_design::Lang,
    ) -> Picked {
        let c = Colors::get(ui.ctx());
        let mut out = Picked::None;
        ui.label(
            RichText::new(lang.pick("Modifiers", "보조 키"))
                .font(ui_font(ui.ctx(), font::CAPTION, true))
                .color(c.text_secondary),
        );
        ui.horizontal_wrapped(|ui| {
            for m in modifiers(sides) {
                let on = self.mods.contains(&m);
                if ui
                    .add(Button::secondary(m).compact().selected(on))
                    .clicked()
                {
                    if on {
                        self.mods.retain(|x| *x != m);
                    } else {
                        // One of each family: `Ctrl` replaces `LCtrl`, and so on.
                        let fam = m.trim_start_matches(['L', 'R']).to_string();
                        self.mods
                            .retain(|x| x.trim_start_matches(['L', 'R']) != fam);
                        self.mods.push(m);
                        self.mods.sort_by_key(|x| {
                            ["Ctrl", "Alt", "Shift", "Win"]
                                .iter()
                                .position(|f| x.ends_with(f))
                                .unwrap_or(9)
                        });
                    }
                }
            }
        });
        ui.add_space(space::S);
        ui.label(
            RichText::new(lang.pick("Key", "키"))
                .font(ui_font(ui.ctx(), font::CAPTION, true))
                .color(c.text_secondary),
        );
        crate::text_field(
            ui,
            &mut self.search,
            lang.pick(
                "Search keys (e.g. Print, Caps, F5)",
                "키 검색 (예: Print, Caps, F5)",
            ),
            ui.available_width(),
        );
        let q = self.search.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("kim-ui-key-picker")
            .max_height(132.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for k in keys
                        .iter()
                        .filter(|k| q.is_empty() || k.to_lowercase().contains(&q))
                    {
                        let on = self.key.as_deref() == Some(*k);
                        if ui
                            .add(Button::secondary(*k).compact().selected(on))
                            .clicked()
                        {
                            self.key = Some((*k).to_string());
                        }
                    }
                });
            });
        ui.add_space(space::S);
        ui.horizontal(|ui| {
            ui.label(crate::caption(ui, lang.pick("Shortcut:", "단축키:")));
            match self.chord() {
                Some(t) => {
                    crate::keycaps(ui, &t);
                }
                None => {
                    ui.label(crate::caption(
                        ui,
                        lang.pick("choose a key", "키를 고르세요"),
                    ));
                }
            }
        });
        crate::dialog_actions(ui, |ui| {
            if ui
                .add_enabled(self.chord().is_some(), Button::primary("Confirm"))
                .clicked()
                && let Some(t) = self.chord()
            {
                out = Picked::Confirm(t);
            }
            if ui.add(Button::secondary("Cancel")).clicked() {
                out = Picked::Cancel;
            }
            if ui.add(Button::ghost("Clear")).clicked() {
                *self = KeyPicker::default();
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_keys_get_contract_names() {
        assert_eq!(key_name(egui::Key::ArrowUp), Some("Up"));
        assert_eq!(key_name(egui::Key::F5), Some("F5"));
        assert_eq!(key_name(egui::Key::A), Some("A"));
        assert_eq!(
            chord_text(egui::Key::K, egui::Modifiers::CTRL | egui::Modifiers::SHIFT).as_deref(),
            Some("Ctrl+Shift+K")
        );
    }

    #[test]
    fn the_picker_builds_side_specific_chords() {
        let mut p = KeyPicker::default();
        assert_eq!(p.chord(), None);
        p.mods = vec!["RCtrl"];
        p.key = Some("CapsLock".into());
        assert_eq!(p.chord().as_deref(), Some("RCtrl+CapsLock"));
        assert!(modifiers(true).contains(&"LShift"));
        assert!(!modifiers(false).contains(&"LShift"));
    }
}
