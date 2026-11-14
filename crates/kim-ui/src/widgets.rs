//! The components (DESIGN_SYSTEM §3): buttons, the switch, segmented choices, dropdowns, text
//! fields, navigation items, list rows, settings groups and rows, dialogs, notices, empty
//! states, keycaps and the shortcut field, the settings gear and progress.
//!
//! Every component has the states rest · hover · pressed · focused · selected · disabled: the
//! keyboard focus always shows the 2 px `focus.ring`; disabled controls fade to
//! `state::DISABLED_OPACITY` (egui's disabled painter) and are never clickable. Each one reports
//! its role and label to screen readers (AccessKit), which is also how the tests find it.

use crate::{Colors, ui_font};
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Margin, Pos2, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetInfo, WidgetText, pos2, vec2,
};
use kim_design::{font, radius, size, space, state};

/// The role of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one main action of a view: accent fill.
    Primary,
    /// Ordinary actions: a quiet filled button.
    Secondary,
    /// Low-emphasis actions in toolbars and rows: no fill until hovered.
    Ghost,
    /// Destructive actions: danger outline and text.
    Danger,
}

/// A button of the design system.
#[must_use = "add it with `ui.add(..)`"]
#[derive(Debug)]
pub struct Button {
    text: String,
    kind: ButtonKind,
    compact: bool,
    min_width: f32,
    selected: bool,
}

impl Button {
    pub fn new(kind: ButtonKind, text: impl Into<String>) -> Button {
        Button {
            text: text.into(),
            kind,
            compact: false,
            min_width: if kind == ButtonKind::Ghost {
                0.0
            } else {
                size::BUTTON_MIN_WIDTH
            },
            selected: false,
        }
    }
    pub fn primary(text: impl Into<String>) -> Button {
        Button::new(ButtonKind::Primary, text)
    }
    pub fn secondary(text: impl Into<String>) -> Button {
        Button::new(ButtonKind::Secondary, text)
    }
    pub fn ghost(text: impl Into<String>) -> Button {
        Button::new(ButtonKind::Ghost, text)
    }
    pub fn danger(text: impl Into<String>) -> Button {
        Button::new(ButtonKind::Danger, text)
    }
    /// The 24-point compact height (rows, toolbars).
    pub fn compact(mut self) -> Button {
        self.compact = true;
        self.min_width = self.min_width.min(48.0);
        self
    }
    pub fn min_width(mut self, w: f32) -> Button {
        self.min_width = w;
        self
    }
    /// Shown pressed in (a toggle-like toolbar button).
    pub fn selected(mut self, on: bool) -> Button {
        self.selected = on;
        self
    }
}

impl Widget for Button {
    fn ui(self, ui: &mut Ui) -> Response {
        let c = Colors::get(ui.ctx());
        let h = if self.compact {
            size::CONTROL_COMPACT
        } else {
            size::CONTROL
        };
        let pad = if self.compact {
            space::S
        } else {
            size::BUTTON_PAD_X
        };
        let semibold = self.kind == ButtonKind::Primary;
        let galley = ui.painter().layout_no_wrap(
            self.text.clone(),
            ui_font(ui.ctx(), font::BODY, semibold),
            Color32::PLACEHOLDER,
        );
        let w = (galley.size().x + 2.0 * pad).max(self.min_width);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        let text = self.text.clone();
        resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &text));
        if ui.is_rect_visible(rect) {
            let hot = resp.hovered() && resp.enabled();
            let down = resp.is_pointer_button_down_on();
            let (fill, stroke, fg) = match self.kind {
                ButtonKind::Primary => (
                    if down {
                        c.accent_pressed
                    } else if hot {
                        c.accent_hover
                    } else {
                        c.accent
                    },
                    Stroke::NONE,
                    c.text_on_accent,
                ),
                ButtonKind::Secondary => (
                    if down || self.selected {
                        c.bg_selected
                    } else if hot {
                        c.bg_hover
                    } else {
                        c.bg_overlay
                    },
                    Stroke::new(size::BORDER, c.border_subtle),
                    c.text_primary,
                ),
                ButtonKind::Ghost => (
                    if down || self.selected {
                        c.bg_selected
                    } else if hot {
                        c.bg_hover
                    } else {
                        Color32::TRANSPARENT
                    },
                    Stroke::NONE,
                    if hot || self.selected {
                        c.text_primary
                    } else {
                        c.text_secondary
                    },
                ),
                ButtonKind::Danger => (
                    if down {
                        c.danger.gamma_multiply(0.28)
                    } else if hot {
                        c.danger.gamma_multiply(0.16)
                    } else {
                        Color32::TRANSPARENT
                    },
                    Stroke::new(size::BORDER, c.danger),
                    c.danger,
                ),
            };
            let p = ui.painter();
            p.rect(
                rect,
                CornerRadius::same(radius::S as u8),
                fill,
                stroke,
                StrokeKind::Inside,
            );
            let pos = rect.center() - galley.size() / 2.0;
            p.galley(pos, galley, fg);
            focus_ring(ui, &resp, rect, radius::S);
        }
        if resp.clicked() || resp.hovered() {
            ui.ctx().set_cursor_icon(if resp.enabled() {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            });
        }
        resp
    }
}

/// Draws the keyboard focus ring around `rect` when `resp` has the focus.
pub fn focus_ring(ui: &Ui, resp: &Response, rect: Rect, r: f32) {
    if resp.has_focus() {
        let c = Colors::get(ui.ctx());
        ui.painter().rect_stroke(
            rect.expand(size::FOCUS_OFFSET),
            CornerRadius::same((r + size::FOCUS_OFFSET) as u8),
            Stroke::new(size::FOCUS_RING, c.focus),
            StrokeKind::Outside,
        );
    }
}

/// A shadcn-style switch: a pill track with a round thumb that slides (`duration.base`, none
/// with reduced motion). Space or Enter toggles it when focused; the label is its accessible name.
#[must_use = "add it with `ui.add(..)`"]
#[derive(Debug)]
pub struct Switch<'a> {
    on: &'a mut bool,
    label: String,
}

impl<'a> Switch<'a> {
    pub fn new(on: &'a mut bool, label: impl Into<String>) -> Switch<'a> {
        Switch {
            on,
            label: label.into(),
        }
    }
}

/// Paints a switch in `rect` (the track is centred in it) at animation position `t` (0 off,
/// 1 on).
pub fn paint_switch(ui: &Ui, rect: Rect, t: f32, hot: bool) {
    let c = Colors::get(ui.ctx());
    let track =
        Rect::from_center_size(rect.center(), vec2(size::SWITCH_WIDTH, size::SWITCH_HEIGHT));
    let mut fill = lerp_color(c.switch_off, c.accent, t);
    if hot {
        fill = lerp_color(fill, c.text_primary, 0.12);
    }
    let p = ui.painter();
    p.rect_filled(track, CornerRadius::same(255), fill);
    let r = size::SWITCH_THUMB / 2.0;
    let inset = (size::SWITCH_HEIGHT - size::SWITCH_THUMB) / 2.0;
    let x0 = track.left() + inset + r;
    let x1 = track.right() - inset - r;
    let center = pos2(x0 + (x1 - x0) * t, track.center().y);
    p.circle_filled(center, r, c.text_on_accent);
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

impl Widget for Switch<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let hit = vec2(size::SWITCH_WIDTH + 4.0, size::HIT_MIN);
        let (rect, mut resp) = ui.allocate_exact_size(hit, Sense::click());
        if resp.clicked() {
            *self.on = !*self.on;
            resp.mark_changed();
        }
        let on = *self.on;
        let label = self.label;
        resp.widget_info(|| WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), on, &label));
        if ui.is_rect_visible(rect) {
            let t = ui.ctx().animate_bool_responsive(resp.id, on);
            paint_switch(ui, rect, t, resp.hovered() && resp.enabled());
            let track = Rect::from_center_size(
                rect.center(),
                vec2(size::SWITCH_WIDTH, size::SWITCH_HEIGHT),
            );
            focus_ring(ui, &resp, track, size::SWITCH_HEIGHT / 2.0);
        }
        resp
    }
}

/// A settings row that *is* a switch: the title (and an optional description below it) on the
/// left, the switch on the right; a click anywhere on the row toggles it. One control with one
/// accessible name (the title).
#[must_use = "add it with `ui.add(..)`"]
#[derive(Debug)]
pub struct SwitchRow<'a> {
    on: &'a mut bool,
    title: String,
    description: Option<String>,
}

impl<'a> SwitchRow<'a> {
    pub fn new(on: &'a mut bool, title: impl Into<String>) -> SwitchRow<'a> {
        SwitchRow {
            on,
            title: title.into(),
            description: None,
        }
    }
    pub fn description(mut self, d: impl Into<String>) -> SwitchRow<'a> {
        let d = d.into();
        self.description = (!d.is_empty()).then_some(d);
        self
    }
}

impl Widget for SwitchRow<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let c = Colors::get(ui.ctx());
        let width = ui.available_width().max(160.0);
        let text_w = width - size::SWITCH_WIDTH - space::L;
        let title = ui.painter().layout(
            self.title.clone(),
            ui_font(ui.ctx(), font::BODY, false),
            c.text_primary,
            text_w,
        );
        let desc = self.description.as_ref().map(|d| {
            ui.painter().layout(
                d.clone(),
                ui_font(ui.ctx(), font::CAPTION, false),
                c.text_muted,
                text_w,
            )
        });
        let text_h = title.size().y + desc.as_ref().map_or(0.0, |d| d.size().y + 2.0);
        let h = text_h.max(size::HIT_MIN) + space::S;
        let (rect, mut resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
        if resp.clicked() {
            *self.on = !*self.on;
            resp.mark_changed();
        }
        let on = *self.on;
        let t_label = self.title.clone();
        let d_label = self.description;
        resp.widget_info(|| {
            let mut i = WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), on, &t_label);
            i.hint_text = d_label.clone();
            i
        });
        if ui.is_rect_visible(rect) {
            let top = rect.center().y - text_h / 2.0;
            let p = ui.painter();
            let th = title.size().y;
            p.galley(pos2(rect.left(), top), title, c.text_primary);
            if let Some(d) = desc {
                p.galley(pos2(rect.left(), top + th + 2.0), d, c.text_muted);
            }
            let sw = Rect::from_min_size(
                pos2(rect.right() - size::SWITCH_WIDTH - 2.0, rect.top()),
                vec2(size::SWITCH_WIDTH, rect.height()),
            );
            let t = ui.ctx().animate_bool_responsive(resp.id, on);
            paint_switch(ui, sw, t, resp.hovered() && resp.enabled());
            if resp.has_focus() {
                focus_ring(ui, &resp, rect.expand2(vec2(space::XS, 0.0)), radius::S);
            }
        }
        resp
    }
}

/// A segmented choice (2–5 short options, e.g. the theme): one selected segment with the
/// accent-soft fill. Returns whether the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let c = Colors::get(ui.ctx());
    let mut changed = false;
    let widths: Vec<f32> = options
        .iter()
        .map(|(v, l)| {
            ui.painter()
                .layout_no_wrap(
                    (*l).to_string(),
                    ui_font(ui.ctx(), font::BODY, *v == *value),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
                + 2.0 * space::M
        })
        .collect();
    let gap = 2.0;
    let total = widths.iter().sum::<f32>() + gap * (widths.len().max(1) - 1) as f32 + 4.0;
    let h = size::CONTROL_COMPACT + 4.0;
    // Exactly its own size, always left to right (also inside a right-aligned settings row).
    let (outer, _) = ui.allocate_exact_size(vec2(total, h), Sense::hover());
    ui.painter().rect(
        outer,
        CornerRadius::same(radius::M as u8),
        c.bg_raised,
        Stroke::new(size::BORDER, c.border_subtle),
        StrokeKind::Inside,
    );
    let mut x = outer.left() + 2.0;
    for ((v, label), w) in options.iter().zip(widths) {
        let sel = *value == *v;
        let rect = Rect::from_min_size(pos2(x, outer.top() + 2.0), vec2(w, size::CONTROL_COMPACT));
        x += w + gap;
        let resp = ui.interact(
            rect,
            ui.scope_id().with(("segment", *label)),
            Sense::click(),
        );
        let l = (*label).to_string();
        resp.widget_info(|| {
            WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), sel, &l)
        });
        let hot = resp.hovered() && resp.enabled();
        let fill = if sel {
            c.bg_selected
        } else if hot {
            c.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(radius::S as u8), fill);
        let fg = if sel || hot {
            c.text_primary
        } else {
            c.text_secondary
        };
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            *label,
            ui_font(ui.ctx(), font::BODY, sel),
            fg,
        );
        focus_ring(ui, &resp, rect, radius::S);
        if resp.clicked() && !sel {
            *value = *v;
            changed = true;
        }
    }
    changed
}

/// A dropdown of `options`; returns whether the value changed.
pub fn dropdown<T: PartialEq + Clone>(
    ui: &mut Ui,
    id_salt: &str,
    label: &str,
    value: &mut T,
    options: &[(T, String)],
) -> bool {
    let current = options
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, l)| l.clone())
        .unwrap_or_default();
    let mut changed = false;
    let r = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current)
        .width(180.0)
        .height(320.0)
        .show_ui(ui, |ui| {
            for (v, l) in options {
                if ui.selectable_label(v == value, l.as_str()).clicked() && v != value {
                    *value = v.clone();
                    changed = true;
                }
            }
        });
    let l = label.to_string();
    r.response
        .widget_info(|| WidgetInfo::labeled(egui::Role::ComboBox, true, &l));
    changed
}

/// A single-line text field at the control height with a hint; `label` is its accessible name
/// (a settings row paints its title, so the field carries the name itself).
pub fn text_field(ui: &mut Ui, text: &mut String, label: &str, hint: &str, width: f32) -> Response {
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .margin(Margin::symmetric(space::S as i8, 5)),
    );
    ui.ctx()
        .accesskit_node_builder(r.id, |b| b.set_label(label.to_string()));
    r
}

/// An item of a left navigation rail: full width, the selected one marked by the selected fill
/// and a 2 px accent bar on its left.
pub fn nav_item(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let c = Colors::get(ui.ctx());
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, size::NAV_ITEM), Sense::click());
    let l = label.to_string();
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), selected, &l));
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() && resp.enabled();
        let p = ui.painter();
        let fill = if selected {
            c.bg_selected
        } else if hot {
            c.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        p.rect_filled(rect, CornerRadius::same(radius::M as u8), fill);
        if selected {
            let bar = Rect::from_min_size(
                pos2(rect.left(), rect.top() + 7.0),
                vec2(2.0, rect.height() - 14.0),
            );
            p.rect_filled(bar, CornerRadius::same(1), c.accent);
        }
        p.text(
            pos2(rect.left() + space::M, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            ui_font(ui.ctx(), font::BODY, selected),
            if selected || hot {
                c.text_primary
            } else {
                c.text_secondary
            },
        );
        focus_ring(ui, &resp, rect, radius::M);
    }
    resp
}

/// A clickable list row of `height` with hover and selection, its content drawn by `add`. The
/// row's response (click to select, double-click to open) is returned with the content's value.
pub fn list_row<R>(
    ui: &mut Ui,
    id: Id,
    selected: bool,
    height: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let c = Colors::get(ui.ctx());
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    let hot = resp.hovered();
    let fill = if selected {
        c.bg_selected
    } else if hot {
        c.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::M as u8), fill);
    if selected {
        let bar = Rect::from_min_size(
            pos2(rect.left(), rect.top() + 8.0),
            vec2(2.0, rect.height() - 16.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(1), c.accent);
    }
    focus_ring(ui, &resp, rect, radius::M);
    let inner = rect.shrink2(vec2(space::M, 0.0));
    let r = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
            add,
        )
        .inner;
    (resp, r)
}

/// A titled group of settings rows on a raised surface (DESIGN_SYSTEM §3.9).
pub fn setting_group<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let c = Colors::get(ui.ctx());
    ui.add_space(space::XS);
    ui.label(
        RichText::new(title)
            .font(ui_font(ui.ctx(), font::CAPTION, true))
            .color(c.text_secondary),
    );
    ui.add_space(2.0);
    let r = egui::Frame::new()
        .fill(c.bg_raised)
        .stroke(Stroke::new(size::BORDER, c.border_subtle))
        .corner_radius(CornerRadius::same(radius::M as u8))
        .inner_margin(Margin::symmetric(space::M as i8, space::S as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            add(ui)
        })
        .inner;
    ui.add_space(space::GROUP);
    r
}

/// A settings row: the title and an optional description on the left, the control `add` puts
/// on the right (laid out right to left). The text is painted, not a separate label, so the
/// control itself carries the name (pass the title as its accessible name where it has none).
pub fn setting_row<R>(
    ui: &mut Ui,
    title: &str,
    description: Option<&str>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let c = Colors::get(ui.ctx());
    let width = ui.available_width();
    let top = ui.cursor().min;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(top, vec2(width, 2000.0)))
            .layout(Layout::right_to_left(Align::Min)),
    );
    let r = add(&mut child);
    let ctrl = child.min_rect();
    let text_w = (width - ctrl.width() - space::L).max(80.0);
    let tg = ui.painter().layout(
        title.to_string(),
        ui_font(ui.ctx(), font::BODY, false),
        c.text_primary,
        text_w,
    );
    let dg = description.map(|d| {
        ui.painter().layout(
            d.to_string(),
            ui_font(ui.ctx(), font::CAPTION, false),
            c.text_muted,
            text_w,
        )
    });
    let text_h = tg.size().y + dg.as_ref().map_or(0.0, |d| 2.0 + d.size().y);
    let h = ctrl.height().max(text_h).max(size::CONTROL_COMPACT);
    ui.allocate_rect(Rect::from_min_size(top, vec2(width, h)), Sense::hover());
    let y = top.y + ((ctrl.height().max(size::CONTROL_COMPACT) - text_h) / 2.0).max(0.0);
    let th = tg.size().y;
    ui.painter().galley(pos2(top.x, y), tg, c.text_primary);
    if let Some(d) = dg {
        ui.painter()
            .galley(pos2(top.x, y + th + 2.0), d, c.text_muted);
    }
    r
}

/// A settings row whose control is too wide to sit beside its text (a segmented choice with
/// many options): the title and description first, the control below.
pub fn setting_stack<R>(
    ui: &mut Ui,
    title: &str,
    description: Option<&str>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let c = Colors::get(ui.ctx());
    let width = ui.available_width();
    let tg = ui.painter().layout(
        title.to_string(),
        ui_font(ui.ctx(), font::BODY, false),
        c.text_primary,
        width,
    );
    let dg = description.map(|d| {
        ui.painter().layout(
            d.to_string(),
            ui_font(ui.ctx(), font::CAPTION, false),
            c.text_muted,
            width,
        )
    });
    let th = tg.size().y;
    let h = th + dg.as_ref().map_or(0.0, |d| 2.0 + d.size().y);
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    ui.painter().galley(rect.min, tg, c.text_primary);
    if let Some(d) = dg {
        ui.painter()
            .galley(rect.min + vec2(0.0, th + 2.0), d, c.text_muted);
    }
    ui.add_space(2.0);
    add(ui)
}

/// A thin divider between the rows of a group.
pub fn row_divider(ui: &mut Ui) {
    let c = Colors::get(ui.ctx());
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(size::BORDER, c.border_subtle),
    );
}

/// A page or section title.
pub fn title(ui: &mut Ui, text: &str) -> Response {
    let c = Colors::get(ui.ctx());
    ui.label(
        RichText::new(text)
            .font(ui_font(ui.ctx(), font::TITLE, true))
            .color(c.text_primary),
    )
}

/// Caption text (11 px, muted).
pub fn caption(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(font::CAPTION)
        .color(Colors::get(ui.ctx()).text_muted)
}

/// Secondary text (12 px).
pub fn secondary(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(Colors::get(ui.ctx()).text_secondary)
}

/// The tone of a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

impl Tone {
    fn color(self, c: &Colors) -> Color32 {
        match self {
            Tone::Info => c.info,
            Tone::Success => c.success,
            Tone::Warning => c.warning,
            Tone::Error => c.danger,
        }
    }
    /// The shape that carries the tone besides the colour (state is never colour-only).
    pub fn glyph(self) -> &'static str {
        match self {
            Tone::Info => "ⓘ",
            Tone::Success => "✓",
            Tone::Warning => "⚠",
            Tone::Error => "✕",
        }
    }
}

/// An inline notice: a tinted band with the tone's glyph and the message (the error code in
/// mono when given). Returns `true` when its Dismiss button was clicked (`dismiss` labels it).
pub fn notice(
    ui: &mut Ui,
    tone: Tone,
    message: &str,
    code: Option<&str>,
    dismiss: Option<&str>,
) -> bool {
    let c = Colors::get(ui.ctx());
    let col = tone.color(&c);
    let mut dismissed = false;
    egui::Frame::new()
        .fill(col.gamma_multiply(0.12))
        .stroke(Stroke::new(size::BORDER, col.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(radius::M as u8))
        .inner_margin(Margin::symmetric(space::M as i8, space::S as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tone.glyph()).color(col));
                ui.label(RichText::new(message).color(c.text_primary));
                if let Some(code) = code {
                    ui.label(
                        RichText::new(code)
                            .monospace()
                            .size(font::CAPTION)
                            .color(c.text_muted),
                    );
                }
                if let Some(d) = dismiss
                    && ui.add(Button::ghost(d).compact()).clicked()
                {
                    dismissed = true;
                }
            });
        });
    dismissed
}

/// A validation message under a field.
pub fn field_error(ui: &mut Ui, message: &str) -> Response {
    let c = Colors::get(ui.ctx());
    ui.label(
        RichText::new(format!("✕ {message}"))
            .size(font::CAPTION)
            .color(c.danger),
    )
}

/// An empty state: one title, one muted sentence and at most one action. Returns `true` when the
/// action was clicked.
pub fn empty_state(ui: &mut Ui, title_text: &str, body: &str, action: Option<&str>) -> bool {
    let c = Colors::get(ui.ctx());
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(space::XXL);
        ui.label(
            RichText::new(title_text)
                .font(ui_font(ui.ctx(), font::TITLE, true))
                .color(c.text_primary),
        );
        ui.add_space(space::XS);
        ui.label(RichText::new(body).color(c.text_muted));
        if let Some(a) = action {
            ui.add_space(space::M);
            clicked = ui.add(Button::primary(a)).clicked();
        }
        ui.add_space(space::XXL);
    });
    clicked
}

/// Splits `Ctrl+Shift+K` into its keys (`Ctrl++` keeps the `+` key).
pub fn split_chord(chord: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in chord.chars() {
        if ch == '+' && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Draws a key combination as keycaps (`Ctrl` `Shift` `K`); a sequence (`Ctrl+K, S` or
/// `Ctrl+K then S`) is drawn step by step with a muted "then" between the steps.
pub fn keycaps(ui: &mut Ui, keys: &str) -> Response {
    let c = Colors::get(ui.ctx());
    let steps: Vec<&str> = keys
        .split(", ")
        .flat_map(|s| s.split(" then "))
        .filter(|s| !s.trim().is_empty())
        .collect();
    // Measure first, then lay out exactly that size left to right.
    let cap_w = |ui: &Ui, k: &str| {
        (ui.painter()
            .layout_no_wrap(
                k.to_string(),
                egui::FontId::monospace(font::CAPTION),
                c.text_primary,
            )
            .size()
            .x
            + 10.0)
            .max(20.0)
    };
    let then_w = ui
        .painter()
        .layout_no_wrap(
            "then".into(),
            ui_font(ui.ctx(), font::CAPTION, false),
            c.text_muted,
        )
        .size()
        .x;
    let mut total = 0.0;
    let mut n = 0;
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            total += then_w;
            n += 1;
        }
        for k in split_chord(step.trim()) {
            total += cap_w(ui, &k);
            n += 1;
        }
    }
    total += space::XS * (n.max(1) - 1) as f32;
    ui.allocate_ui_with_layout(
        vec2(total.max(1.0), 20.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            for (i, step) in steps.iter().enumerate() {
                if i > 0 {
                    ui.label(
                        RichText::new("then")
                            .size(font::CAPTION)
                            .color(c.text_muted),
                    );
                }
                for k in split_chord(step.trim()) {
                    keycap(ui, &k);
                }
            }
        },
    )
    .response
}

/// One keycap.
pub fn keycap(ui: &mut Ui, key: &str) -> Response {
    let c = Colors::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        key.to_string(),
        egui::FontId::monospace(font::CAPTION),
        c.text_primary,
    );
    let sz = vec2((galley.size().x + 10.0).max(20.0), 20.0);
    let (rect, resp) = ui.allocate_exact_size(sz, Sense::hover());
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(radius::S as u8),
        c.bg_overlay,
        Stroke::new(size::BORDER, c.border_strong.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
    // The key's lower edge, like a physical cap.
    p.hline(
        rect.x_range().shrink(2.0),
        rect.bottom() - 1.0,
        Stroke::new(size::BORDER, c.border_strong.gamma_multiply(0.6)),
    );
    p.galley(
        rect.center() - galley.size() / 2.0 - vec2(0.0, 0.5),
        galley,
        c.text_primary,
    );
    resp
}

/// A shortcut field: shows the keys as keycaps, `placeholder` when there are none, and
/// "recording" with an accent outline while the program records keys. A click (or Enter) starts
/// recording; the program captures the keys itself.
pub fn shortcut_field(
    ui: &mut Ui,
    label: &str,
    keys: &str,
    recording: bool,
    placeholder: &str,
    recording_text: &str,
    width: f32,
) -> Response {
    let c = Colors::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, size::CONTROL), Sense::click());
    let l = label.to_string();
    let v = keys.to_string();
    resp.widget_info(|| {
        let mut i = WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &l);
        i.current_text_value = Some(v.clone());
        i
    });
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() && resp.enabled();
        let stroke = if recording {
            Stroke::new(size::FOCUS_RING, c.accent)
        } else if hot {
            Stroke::new(size::BORDER, c.border_strong)
        } else {
            Stroke::new(size::BORDER, c.border_subtle)
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(radius::S as u8),
            c.bg_raised,
            stroke,
            StrokeKind::Inside,
        );
        let inner = rect.shrink2(vec2(space::S, 0.0));
        if recording || keys.is_empty() {
            ui.painter().text(
                pos2(inner.left(), rect.center().y),
                Align2::LEFT_CENTER,
                if recording {
                    recording_text
                } else {
                    placeholder
                },
                ui_font(ui.ctx(), font::BODY, false),
                if recording { c.accent } else { c.text_muted },
            );
        } else {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            keycaps(&mut child, keys);
        }
        if !recording {
            focus_ring(ui, &resp, rect, radius::S);
        }
    }
    resp
}

/// Paints the settings gear (eight teeth, a hole) centred at `center` with outer radius `r`.
pub fn paint_gear(p: &egui::Painter, center: Pos2, r: f32, col: Color32) {
    let teeth = 8;
    let mut pts = Vec::with_capacity(teeth * 4);
    for i in 0..teeth {
        let a = i as f32 / teeth as f32 * std::f32::consts::TAU;
        let w = std::f32::consts::TAU / teeth as f32;
        for (da, rr) in [(-0.30, r * 0.74), (-0.18, r), (0.18, r), (0.30, r * 0.74)] {
            let aa = a + da * w;
            pts.push(center + vec2(aa.cos(), aa.sin()) * rr);
        }
    }
    p.add(egui::Shape::closed_line(pts, Stroke::new(r * 0.16, col)));
    p.circle_stroke(center, r * 0.32, Stroke::new(r * 0.16, col));
}

/// The settings gear button (DESIGN_SYSTEM §3.10): a ghost icon button of `size::GEAR` in the
/// lower-right corner of `container`, `size::GEAR_INSET` from both edges. Its accessible name is
/// `label` ("Settings"); `open` shows it selected while the settings view is open.
pub fn settings_gear(ui: &mut Ui, container: Rect, label: &str, open: bool) -> Response {
    let rect = Rect::from_min_size(
        container.right_bottom() - Vec2::splat(size::GEAR + size::GEAR_INSET - 4.0),
        Vec2::splat(size::GEAR),
    );
    gear_at(ui, rect, label, open)
}

/// The gear button in `rect`.
pub fn gear_at(ui: &mut Ui, rect: Rect, label: &str, open: bool) -> Response {
    let c = Colors::get(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("kim-ui-gear", label)), Sense::click());
    let l = label.to_string();
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, open, &l));
    let hot = resp.hovered();
    if hot || open {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(radius::M as u8),
            if open { c.bg_selected } else { c.bg_hover },
        );
    }
    let col = if hot || open {
        c.text_primary
    } else {
        c.text_muted
    };
    let r = if resp.is_pointer_button_down_on() {
        7.0 * state::PRESSED_SCALE
    } else {
        7.0
    };
    paint_gear(ui.painter(), rect.center(), r, col);
    focus_ring(ui, &resp, rect, radius::M);
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(label)
}

/// A 4 px progress bar; `None` is indeterminate (a moving segment, static with reduced motion).
pub fn progress(ui: &mut Ui, fraction: Option<f32>) -> Response {
    let c = Colors::get(ui.ctx());
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, size::PROGRESS), Sense::hover());
    let r = CornerRadius::same(2);
    let p = ui.painter();
    p.rect_filled(rect, r, c.bg_selected);
    match fraction {
        Some(f) => {
            let mut fill = rect;
            fill.set_width(rect.width() * f.clamp(0.0, 1.0));
            p.rect_filled(fill, r, c.accent);
        }
        None => {
            let moving = ui.style().animation_time > 0.0;
            let t = if moving {
                ui.ctx().request_repaint();
                (ui.input(|i| i.time) as f32 * 0.8).fract()
            } else {
                0.35
            };
            let seg = rect.width() * 0.3;
            let x = rect.left() - seg + (rect.width() + seg) * t;
            let fill = Rect::from_x_y_ranges(
                x.max(rect.left())..=(x + seg).min(rect.right()),
                rect.y_range(),
            );
            p.rect_filled(fill, r, c.accent);
        }
    }
    resp
}

/// The standard dialog (DESIGN_SYSTEM §3.8): a modal of `size::DIALOG_WIDTH` (narrower windows:
/// their width), `bg.overlay`, radius 10, 16 padding, the title in 15 px semibold. Buttons go in
/// [`dialog_actions`]. Escape closes it (`ModalResponse::should_close`).
pub fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    title_text: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    let c = Colors::get(ctx);
    let width = dialog_width(ctx);
    modal(ctx, id).show(ctx, |ui| {
        ui.set_width(width);
        if short(ui.ctx()) {
            ui.spacing_mut().item_spacing.y = space::XS;
        }
        ui.label(
            RichText::new(title_text)
                .font(ui_font(ui.ctx(), font::TITLE, true))
                .color(c.text_primary),
        );
        ui.add_space(space::S);
        body(ui)
    })
}

/// The width of a dialog in this window: `size::DIALOG_WIDTH`, less in a narrow window.
pub fn dialog_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 2.0 * space::L - 2.0 * space::DIALOG)
        .clamp(160.0, size::DIALOG_WIDTH)
}

/// A modal with the dialog look (for dialogs that lay out their own title).
pub fn modal(ctx: &egui::Context, id: &str) -> egui::Modal {
    egui::Modal::new(Id::unique(("kim-ui-dialog", id)))
        .backdrop_color(Color32::from_black_alpha(110))
        .frame(dialog_frame(ctx))
}

/// The frame of dialogs and popovers: `bg.overlay`, a subtle border, radius 10, 16 padding and
/// the one soft shadow.
pub fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    let c = Colors::get(ctx);
    let pad = if short(ctx) { space::M } else { space::DIALOG };
    egui::Frame::new()
        .fill(c.bg_overlay)
        .stroke(Stroke::new(size::BORDER, c.border_subtle))
        .corner_radius(CornerRadius::same(radius::L as u8))
        .inner_margin(Margin::same(pad as i8))
        .shadow(ctx.global_style().visuals.window_shadow)
}

/// A window too short for the regular dialog spacing (under 320 points): dialogs tighten.
pub fn short(ctx: &egui::Context) -> bool {
    ctx.content_rect().height() < 320.0
}

/// The dialog's buttons, right-aligned; `add` adds them right to left (the primary first).
pub fn dialog_actions<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(if short(ui.ctx()) { space::XS } else { space::M });
    ui.with_layout(Layout::right_to_left(Align::Center), add)
        .inner
}

/// Text for a tooltip or a label that follows `lang` (`en` / `ko`).
pub fn tr(lang: kim_design::Lang, en: &str, ko: &str) -> WidgetText {
    WidgetText::from(lang.pick(en, ko).to_string())
}
