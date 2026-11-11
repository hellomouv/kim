//! Step 78 (DDALGAK-FR-018, DDALGAK-FR-074, ADR 0079, docs/DDALGAK_GUI_JOURNEYS.md J4 *Several steps*): the
//! macro builder in the rule form and the recorder's capture.
//!
//! The effect form above stays the editor of one step: **Add to macro** puts it into the list;
//! every row shows its sentence with **↑**, **↓**, **Edit** and **✕** (`Alt+Up`/`Alt+Down` move
//! the focused row); **Add a wait** adds a delay; *Repeat* and *If a step fails* are the rule's
//! `times` and `on_error`. **Record keys** starts the recorder (`crate::recorder`): while it runs a
//! banner says so, every key typed into this window becomes a step and nothing reaches the
//! editor's fields or shortcuts; **Stop recording** or `Esc` ends it and the steps join the list
//! for review. The Try stage and the Playground test the macro before it is saved.

use super::*;

fn small(text: impl Into<String>, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text.into())
        .size(Metrics::get().caption)
        .color(color)
}

impl EditorApp {
    /// While recording, this window's key events go to the recorder only.
    pub(super) fn record_events(&mut self, ctx: &egui::Context) {
        if !self.recorder.is_on() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let mut stop = false;
        for ev in &events {
            stop |= self.recorder.feed(ev, now);
        }
        // Pointer events stay (the Stop button must work); keys and text are consumed.
        let rest: Vec<egui::Event> = events
            .into_iter()
            .filter(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Ime(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                )
            })
            .collect();
        ctx.input_mut(|i| i.events = rest);
        if stop {
            self.stop_recording();
        }
    }

    pub(super) fn stop_recording(&mut self) {
        let steps = self.recorder.stop();
        if let Some(f) = self.form.as_mut() {
            let room = 32usize.saturating_sub(f.fields.macro_steps.len());
            f.fields.macro_steps.extend(steps.into_iter().take(room));
        }
    }

    pub(super) fn macro_section(&mut self, ui: &mut egui::Ui, profile: Option<&Profile>) {
        let lang = self.lang;
        let c = Colors::get();
        let recording = self.recorder.is_on();
        let full = self.recorder.full;
        let count = self.recorder.len();
        let Some(f) = self.form.as_mut() else {
            return;
        };
        ui.add_space(6.0);
        ui.label(egui::RichText::new(lang.t("macro.title")).strong());
        ui.label(small(lang.t("macro.note"), c.text_muted));
        let mut start = false;
        let mut stop = false;
        if recording {
            egui::Frame::new()
                .stroke(egui::Stroke::new(2.0, c.danger))
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("● {} ({count})", lang.t("macro.recording")))
                            .color(c.danger)
                            .strong(),
                    );
                    ui.label(small(lang.t("macro.recording_note"), c.text_secondary));
                    if full {
                        ui.label(small(lang.t("macro.full"), c.warning));
                    }
                    stop = ui.button(lang.t("macro.stop")).clicked();
                });
        }
        ui.horizontal_wrapped(|ui| {
            let can_add = f.fields.effect.is_some() && f.fields.macro_steps.len() < 32;
            if ui
                .add_enabled(
                    can_add && !recording,
                    egui::Button::new(lang.t("macro.add")),
                )
                .clicked()
            {
                let _ = f.add_to_macro(profile);
            }
            if ui
                .add_enabled(
                    !recording && f.fields.macro_steps.len() < 32,
                    egui::Button::new(lang.t("macro.add_wait")),
                )
                .clicked()
            {
                f.add_delay(500);
            }
            if !recording && ui.button(format!("● {}", lang.t("macro.record"))).clicked() {
                start = true;
            }
        });
        let summaries = f.step_summaries(profile);
        let n = f.fields.macro_steps.len();
        let mut act: Option<(usize, &str)> = None;
        for (i, s) in summaries.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let row = ui.label(format!("{}. {s}", i + 1));
                if f.fields.macro_steps[i]["type"] == "delay" {
                    let mut ms = f.fields.macro_steps[i]["ms"].as_u64().unwrap_or(500);
                    if ui
                        .add(
                            egui::DragValue::new(&mut ms)
                                .range(1..=10_000)
                                .suffix(" ms"),
                        )
                        .accessible_name(format!("{} {}", lang.t("macro.wait_ms"), i + 1))
                        .changed()
                    {
                        f.fields.macro_steps[i]["ms"] = serde_json::json!(ms);
                    }
                }
                for (label, what, on) in [
                    ("↑", "up", i > 0),
                    ("↓", "down", i + 1 < n),
                    (
                        lang.t("macro.edit"),
                        "edit",
                        f.fields.macro_steps[i]["type"] != "delay",
                    ),
                    ("✕", "remove", true),
                ] {
                    let name = format!(
                        "{} {} {}",
                        lang.t(&format!("macro.{what}")),
                        lang.t("macro.step"),
                        i + 1
                    );
                    if ui
                        .add_enabled(on && !recording, egui::Button::new(label))
                        .on_hover_text(&name)
                        .accessible_name(&name)
                        .clicked()
                    {
                        act = Some((i, what));
                    }
                }
                if row.has_focus()
                    || ui
                        .ctx()
                        .memory(|m| m.focused())
                        .is_some_and(|id| id == row.id)
                {
                    if ui.input(|x| x.modifiers.alt && x.key_pressed(egui::Key::ArrowUp)) {
                        act = Some((i, "up"));
                    }
                    if ui.input(|x| x.modifiers.alt && x.key_pressed(egui::Key::ArrowDown)) {
                        act = Some((i, "down"));
                    }
                }
            });
        }
        match act {
            Some((i, "up")) => f.move_step(i, true),
            Some((i, "down")) => f.move_step(i, false),
            Some((i, "edit")) => f.edit_step(i),
            Some((i, _)) => f.remove_step(i),
            None => {}
        }
        if f.is_macro() {
            ui.horizontal_wrapped(|ui| {
                ui.label(lang.t("macro.times"));
                ui.add(egui::DragValue::new(&mut f.fields.times).range(1..=100))
                    .accessible_name(lang.t("macro.times"));
                ui.checkbox(&mut f.fields.continue_on_error, lang.t("macro.continue"));
            });
        }
        if start {
            self.recorder.start();
        }
        if stop {
            self.stop_recording();
        }
    }
}
