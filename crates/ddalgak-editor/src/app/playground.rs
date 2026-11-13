//! Step 76 (DDALGAK-FR-040…042, ADR 0077, docs/DDALGAK_GUI_JOURNEYS.md J5): the test
//! playground page — try the saved rules, or the rule being edited before it is saved, on a
//! chosen profile, mode and context, without touching anything outside the playground.
//!
//! - **Simulate** (the default): `ddalgak::simulate` describes the decision, every candidate and
//!   every step; nothing is typed, sent, opened, moved or changed.
//! - **Live** (only after the user ticks the consent box): what a rule would type goes into the
//!   playground's own test field, never into another program; a program or website opens only
//!   when the user presses *Confirm and open* for that one step; the pointer, the modes and the
//!   engine are still only simulated.
//! - **Step** reveals one effect at a time; **Reset** clears the field, the result and the
//!   simulated state. The rules file and the running engine are never changed by the playground.

use super::*;
use ddalgak::simulate::{self, DESKTOP, Input, Kind};
use serde_json::Value;

/// The focus kinds a test may pretend.
pub const FOCUS: [(&str, &str); 4] = [
    ("normal", "play.focus.normal"),
    ("password_field", "play.focus.password"),
    ("elevated_window", "play.focus.elevated"),
    ("secure_desktop", "play.focus.secure"),
];

pub const ON: [&str; 4] = ["press", "release", "hold", "repeat"];

/// The playground's own state (never written to disk).
#[derive(Debug, Clone, Default)]
pub struct Playground {
    pub keys: String,
    pub on: usize,
    /// The profile to test (`None`: the active one).
    pub profile: Option<String>,
    pub mode: Option<String>,
    pub paused: bool,
    pub focus: usize,
    pub app: String,
    pub title: String,
    pub ime: bool,
    pub injected: bool,
    /// Test the rule being edited (unsaved) together with the saved ones.
    pub with_form: bool,
    pub live: bool,
    pub live_consent: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    /// How many effects are shown (Step).
    pub shown: usize,
    /// Live: the test field (only the playground's own text).
    pub field: String,
    /// Live: launch steps the user confirmed and their outcome.
    pub opened: Vec<(usize, String)>,
    pub elapsed_us: u128,
    /// The simulated pointer.
    pub pointer: (i32, i32),
}

impl Playground {
    /// Reset: the field, the result and the simulated state; the inputs stay for another try.
    pub fn reset(&mut self) {
        self.result = None;
        self.error = None;
        self.shown = 0;
        self.field.clear();
        self.opened.clear();
        self.pointer = (DESKTOP.2 / 2, DESKTOP.3 / 2);
    }
}

impl EditorApp {
    /// The document the playground tests: the saved rules, plus the form's unsaved rule when
    /// asked (replacing the saved rule of the same ID, or added at the end).
    fn playground_doc(&self) -> Option<ddalgak::model::Document> {
        let mut doc = self.doc()?.clone();
        if self.play.with_form
            && let Some(f) = &self.form
        {
            // The form's rule belongs to its own profile.
            let pid = f.profile.clone();
            let profile = doc.profile(&pid).cloned();
            let id = f.id.clone().unwrap_or_else(|| "draft-rule".into());
            if let Ok(rule) = f.build(&id, profile.as_ref())
                && let Some(p) = doc.profiles.iter_mut().find(|p| p.id == pid)
            {
                let entry = ddalgak::model::RuleEntry::Typed(Box::new(rule));
                match p.rules.iter().position(|e| e.id() == Some(id.as_str())) {
                    Some(i) => p.rules[i] = entry,
                    None => p.rules.push(entry),
                }
            }
        }
        Some(doc)
    }

    /// Runs the test with the current inputs (and the confirmed launches, live).
    pub(super) fn play_run(&mut self, confirmed: Vec<usize>) {
        let Some(doc) = self.playground_doc() else {
            self.play.error = Some(self.t("play.no_rules").into());
            return;
        };
        let p = &self.play;
        let input = Input {
            kind: if p.live && p.live_consent {
                Kind::Live
            } else {
                Kind::Simulate
            },
            profile: p.profile.clone(),
            revision: if p.with_form {
                None
            } else {
                Some(self.loaded.revision())
            },
            mode: p.mode.clone(),
            paused: p.paused,
            focus: FOCUS[p.focus.min(3)].0.into(),
            app_exe: (!p.app.trim().is_empty()).then(|| p.app.trim().to_string()),
            window_title: (!p.title.trim().is_empty()).then(|| p.title.trim().to_string()),
            window_class: None,
            local_time: None,
            ime_composing: p.ime,
            keys: p.keys.trim().to_string(),
            on: ON[p.on.min(3)].into(),
            injected: p.injected,
            pointer: p.pointer,
            confirmed,
        };
        let t = Instant::now();
        let r = simulate::simulate(&doc, &input);
        self.play.elapsed_us = t.elapsed().as_micros();
        match r {
            Ok(v) => {
                if input.kind == Kind::Live {
                    self.play_apply_live(&doc, &v);
                }
                // The simulated pointer follows a pointer preview (never the real one).
                if let Some(to) = v["effects"].as_array().and_then(|e| {
                    e.iter()
                        .rev()
                        .find_map(|x| x["preview"].get("to").filter(|t| t.is_object()))
                }) {
                    self.play.pointer = (
                        to["x"].as_i64().unwrap_or(0) as i32,
                        to["y"].as_i64().unwrap_or(0) as i32,
                    );
                }
                self.play.shown = v["effects"].as_array().map_or(0, Vec::len).min(1);
                self.play.result = Some(v);
                self.play.error = None;
            }
            Err(e) => {
                self.play.result = None;
                self.play.error = Some(e);
            }
        }
    }

    /// Live: the winner's typing goes into the playground's own test field.
    fn play_apply_live(&mut self, doc: &ddalgak::model::Document, v: &Value) {
        let Some(id) = v["decision"]["rule"]
            .as_str()
            .filter(|_| v["decision"]["outcome"] == "fired")
        else {
            return;
        };
        let Some(rule) = doc
            .profiles
            .iter()
            .flat_map(|p| p.rules.iter())
            .filter_map(|e| e.rule())
            .find(|r| r.id == id)
        else {
            return;
        };
        for s in &rule.steps {
            match s {
                ddalgak::model::Action::TypeText(t) => self.play.field.push_str(&t.text),
                ddalgak::model::Action::SendKeys(k) => {
                    self.play.field.push_str(&format!("⟨{}⟩", k.keys));
                }
                ddalgak::model::Action::Remap(r) => {
                    self.play.field.push_str(&format!("⟨{}⟩", r.to));
                }
                _ => {}
            }
        }
    }

    /// Live: the user confirmed step `n`'s launch — open it now, through the editor's launcher.
    pub(super) fn play_confirm(&mut self, n: usize) {
        let Some(doc) = self.playground_doc() else {
            return;
        };
        let id = self
            .play
            .result
            .as_ref()
            .and_then(|v| v["decision"]["rule"].as_str().map(String::from));
        let step = id.and_then(|id| {
            doc.profiles
                .iter()
                .flat_map(|p| p.rules.iter())
                .filter_map(|e| e.rule())
                .find(|r| r.id == id)
                .and_then(|r| r.steps.get(n).map(|s| s.to_value()))
        });
        let Some(step) = step else { return };
        let outcome = match ddalgak::launch::compile_step(&step) {
            Ok(target) => {
                let plan = ddalgak::launch::Plan {
                    targets: vec![target],
                    stop_on_error: true,
                };
                ddalgak::launch::execute(
                    &plan,
                    &ddalgak::launch::Exec {
                        launcher: self.launcher.as_ref(),
                        var: &ddalgak::launch::process_var,
                        cancelled: &|| false,
                    },
                )
            }
            Err(_) => "launch_failed",
        };
        self.play.opened.push((n, outcome.to_string()));
        let mut confirmed: Vec<usize> = self
            .play
            .opened
            .iter()
            .filter(|(_, o)| o == "ok")
            .map(|(s, _)| *s)
            .collect();
        confirmed.sort_unstable();
        let field = std::mem::take(&mut self.play.field);
        self.play_run(confirmed);
        self.play.field = field;
    }

    pub(super) fn playground_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let t = |k: &str| lang.t(k);
        ui.label(
            egui::RichText::new(t("play.title"))
                .size(Metrics::get().title)
                .strong(),
        );
        ui.label(t("play.intro"));
        ui.add_space(6.0);
        let active = self.active_profile_id();
        let profiles: Vec<(String, String)> = self
            .doc()
            .map(|d| {
                d.profiles
                    .iter()
                    .map(|p| (p.id.clone(), p.title.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if self
            .play
            .profile
            .as_ref()
            .is_some_and(|p| !profiles.iter().any(|x| &x.0 == p))
        {
            self.play.profile = None;
        }
        let pid = self.play.profile.clone().unwrap_or(active.clone());
        let modes: Vec<(String, String)> = self
            .profile(&pid)
            .map(|p| {
                p.modes
                    .iter()
                    .map(|m| (m.id.clone(), m.title.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let has_form = self.form.is_some();
        egui::Grid::new("play-inputs")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(t("play.keys"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.play.keys)
                        .hint_text("Ctrl+Shift+K")
                        .desired_width(220.0),
                )
                .accessible_name(t("play.keys"));
                ui.end_row();
                ui.label(t("play.event"));
                egui::ComboBox::from_id_salt("play-on")
                    .selected_text(ON[self.play.on])
                    .show_ui(ui, |ui| {
                        for (i, o) in ON.iter().enumerate() {
                            ui.selectable_value(&mut self.play.on, i, *o);
                        }
                    })
                    .response
                    .accessible_name(t("play.event"));
                ui.end_row();
                ui.label(t("play.profile"));
                let before = self.play.profile.clone();
                egui::ComboBox::from_id_salt("play-profile")
                    .selected_text(
                        profiles
                            .iter()
                            .find(|p| p.0 == pid)
                            .map_or(pid.as_str(), |p| p.1.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for (id, title) in &profiles {
                            let label = if *id == active {
                                format!("{title} ({})", t("play.profile.active"))
                            } else {
                                title.clone()
                            };
                            ui.selectable_value(&mut self.play.profile, Some(id.clone()), label);
                        }
                    })
                    .response
                    .accessible_name(t("play.profile"));
                if self.play.profile != before {
                    // Another profile has other modes: start from its default.
                    self.play.mode = None;
                }
                ui.end_row();
                ui.label(t("play.mode"));
                let current = self.play.mode.clone().unwrap_or_default();
                egui::ComboBox::from_id_salt("play-mode")
                    .selected_text(
                        modes
                            .iter()
                            .find(|m| m.0 == current)
                            .map_or(t("play.mode.default"), |m| m.1.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.play.mode, None, t("play.mode.default"));
                        for (id, title) in &modes {
                            ui.selectable_value(&mut self.play.mode, Some(id.clone()), title);
                        }
                    })
                    .response
                    .accessible_name(t("play.mode"));
                ui.end_row();
                ui.label(t("play.focus"));
                egui::ComboBox::from_id_salt("play-focus")
                    .selected_text(t(FOCUS[self.play.focus].1))
                    .show_ui(ui, |ui| {
                        for (i, (_, k)) in FOCUS.iter().enumerate() {
                            ui.selectable_value(&mut self.play.focus, i, t(k));
                        }
                    })
                    .response
                    .accessible_name(t("play.focus"));
                ui.end_row();
                ui.label(t("play.app"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.play.app)
                        .hint_text("Code.exe")
                        .desired_width(220.0),
                )
                .accessible_name(t("play.app"));
                ui.end_row();
                ui.label(t("play.window_title"));
                ui.add(egui::TextEdit::singleline(&mut self.play.title).desired_width(220.0))
                    .accessible_name(t("play.window_title"));
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.play.paused, t("play.paused"));
            ui.checkbox(&mut self.play.ime, t("play.ime"));
            ui.checkbox(&mut self.play.injected, t("play.injected"));
            if has_form {
                ui.checkbox(&mut self.play.with_form, t("play.with_form"));
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut self.play.live, false, t("play.simulate"));
            ui.radio_value(&mut self.play.live, true, t("play.live"));
        });
        if self.play.live {
            ui.checkbox(&mut self.play.live_consent, t("play.live_consent"));
        }
        let can_run =
            !self.play.keys.trim().is_empty() && (!self.play.live || self.play.live_consent);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_run, egui::Button::new(t("play.run")))
                .clicked()
            {
                self.play.opened.clear();
                self.play_run(Vec::new());
            }
            let total = self
                .play
                .result
                .as_ref()
                .and_then(|v| v["effects"].as_array().map(Vec::len))
                .unwrap_or(0);
            if ui
                .add_enabled(self.play.shown < total, egui::Button::new(t("play.step")))
                .clicked()
            {
                self.play.shown += 1;
            }
            if ui.button(t("play.reset")).clicked() {
                self.play.reset();
            }
        });
        if let Some(e) = &self.play.error {
            ui.colored_label(Colors::get().danger, format!("✕ {e}"));
        }
        let mut confirm = None;
        if let Some(v) = self.play.result.clone() {
            let d = &v["decision"];
            let line = match d["outcome"].as_str() {
                Some("fired") => format!(
                    "✓ {} {} ({})",
                    d["rule"].as_str().unwrap_or(""),
                    t("play.fires"),
                    if v["kind"] == "live" {
                        t("play.live")
                    } else {
                        t("play.simulate")
                    }
                ),
                Some("skipped") => format!(
                    "○ {} {}: {}",
                    d["rule"].as_str().unwrap_or(""),
                    t("play.skipped"),
                    d["code"].as_str().unwrap_or("")
                ),
                _ => format!("○ {}", t("play.no_match")),
            };
            ui.label(egui::RichText::new(line).strong());
            ui.label(if d["key_reaches_app"] == true {
                t("play.key_passes")
            } else {
                t("play.key_consumed")
            });
            ui.label(format!("{} {} µs", t("play.elapsed"), self.play.elapsed_us));
            for c in d["candidates"].as_array().into_iter().flatten() {
                ui.label(format!(
                    "  {} — {}",
                    c["rule"].as_str().unwrap_or(""),
                    c["result"].as_str().unwrap_or("")
                ));
            }
            let effects = v["effects"].as_array().cloned().unwrap_or_default();
            for e in effects.iter().take(self.play.shown) {
                ui.horizontal_wrapped(|ui| {
                    let mark = if e["performed"] == true { "●" } else { "○" };
                    ui.label(format!(
                        "{mark} {}. {}",
                        e["step"],
                        e["summary"].as_str().unwrap_or("")
                    ));
                    if e["needs_confirmation"] == true
                        && ui.button(t("play.confirm_open")).clicked()
                    {
                        confirm = e["step"].as_u64().map(|n| n as usize);
                    }
                });
            }
            if self.play.shown < effects.len() {
                ui.label(format!(
                    "{} {}",
                    effects.len() - self.play.shown,
                    t("play.more_steps")
                ));
            }
        }
        if let Some(n) = confirm {
            self.play_confirm(n);
        }
        for (n, outcome) in &self.play.opened {
            ui.label(format!("{} {n}: {outcome}", t("play.opened")));
        }
        ui.separator();
        ui.label(t("play.field"));
        ui.add(
            egui::TextEdit::multiline(&mut self.play.field)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .accessible_name(t("play.field"));
        // The simulated desktop and pointer (never the real pointer).
        let (w, h) = (240.0, 135.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let c = Colors::get();
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, c.border_subtle),
            egui::StrokeKind::Inside,
        );
        let (px, py) = self.play.pointer;
        let at = rect.min
            + egui::vec2(
                px as f32 / DESKTOP.2 as f32 * w,
                py as f32 / DESKTOP.3 as f32 * h,
            );
        painter.circle_filled(at, 4.0, c.accent);
        ui.label(format!("{} {px}, {py}", t("play.pointer")));
    }
}
