//! The components behave as DESIGN_SYSTEM §3 says: by mouse and keyboard, with their roles and
//! names for screen readers, in both themes.

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn setup(ctx: &egui::Context, theme: Theme) {
    apply(
        ctx,
        Options {
            theme,
            reduced_motion: true,
            system_fonts: false,
        },
    );
}

#[derive(Default)]
struct S {
    on: bool,
    row: bool,
    choice: u8,
    clicks: u32,
    themed: bool,
}

fn harness(theme: Theme) -> Harness<'static, S> {
    Harness::new_ui_state(
        move |ui, s: &mut S| {
            if !s.themed {
                setup(ui.ctx(), theme);
                s.themed = true;
            }
            ui.add(Switch::new(&mut s.on, "Offline mode"));
            ui.add(SwitchRow::new(&mut s.row, "Start with Windows").description("Runs at sign-in"));
            segmented(
                ui,
                &mut s.choice,
                &[(0, "System"), (1, "Dark"), (2, "Light")],
            );
            if ui.add(Button::primary("Save")).clicked() {
                s.clicks += 1;
            }
            ui.add_enabled(false, Button::danger("Delete"));
            keycaps(ui, "Ctrl+Shift+K");
        },
        S::default(),
    )
}

#[test]
fn the_switch_toggles_by_click_and_by_keyboard() {
    let mut h = harness(Theme::Dark);
    h.run();
    h.get_by_label("Offline mode").click();
    h.run();
    assert!(h.state().on, "a click turns it on");
    h.get_by_label("Offline mode").focus();
    h.run();
    h.key_press(egui::Key::Space);
    h.run();
    assert!(!h.state().on, "Space turns it off again");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().on, "Enter toggles too");
}

#[test]
fn a_switch_row_toggles_from_its_text_and_has_one_name() {
    let mut h = harness(Theme::Light);
    h.run();
    // One node carries the title (no separate label to confuse a screen reader or a test).
    let row = h.get_by_label("Start with Windows");
    let r = row.rect();
    h.hover_at(r.left_center() + egui::vec2(4.0, 0.0));
    row.click();
    h.run();
    assert!(h.state().row);
}

#[test]
fn segments_buttons_and_disabled_buttons() {
    let mut h = harness(Theme::Dark);
    h.run();
    h.get_by_label("Light").click();
    h.run();
    assert_eq!(h.state().choice, 2);
    h.get_by_label("Save").click();
    h.run();
    assert_eq!(h.state().clicks, 1);
    // A disabled button is shown but never acts.
    h.get_by_label("Delete").click();
    h.run();
    assert_eq!(h.state().clicks, 1);
}

#[test]
fn both_themes_are_installed_and_switchable() {
    let ctx = egui::Context::default();
    setup(&ctx, Theme::Light);
    assert_eq!(theme_of(&ctx), Theme::Light);
    assert_eq!(
        ctx.global_style().visuals.panel_fill,
        c32(kim_design::LIGHT.bg_base)
    );
    set_theme(&ctx, Theme::Dark);
    assert_eq!(theme_of(&ctx), Theme::Dark);
    assert_eq!(Colors::get(&ctx).accent, c32(kim_design::DARK.accent));
    set_reduced_motion(&ctx, false);
    assert!(ctx.global_style().animation_time > 0.0);
    set_reduced_motion(&ctx, true);
    assert_eq!(ctx.global_style().animation_time, 0.0);
}

#[test]
fn chords_split_into_keycaps() {
    assert_eq!(split_chord("Ctrl+Shift+K"), ["Ctrl", "Shift", "K"]);
    assert_eq!(split_chord("Ctrl++"), ["Ctrl", "+"]);
    assert_eq!(split_chord("RCtrl+CapsLock"), ["RCtrl", "CapsLock"]);
}

#[test]
fn the_gear_sits_in_the_lower_right_corner() {
    let mut h = Harness::new_ui_state(
        |ui, opened: &mut bool| {
            let rect = ui.max_rect();
            if settings_gear(ui, rect, "Settings", *opened).clicked() {
                *opened = true;
            }
        },
        false,
    );
    h.set_size(egui::vec2(400.0, 300.0));
    h.run();
    let g = h.get_by_label("Settings").rect();
    assert!(g.right() > 360.0 && g.bottom() > 260.0, "{g:?}");
    h.get_by_label("Settings").click();
    h.run();
    assert!(*h.state());
}

#[test]
fn the_key_picker_takes_clicks() {
    let mut h = Harness::new_ui_state(
        |ui, s: &mut (keys::KeyPicker, Option<String>)| {
            setup(ui.ctx(), Theme::Dark);
            if let keys::Picked::Confirm(t) =
                s.0.ui(ui, &["F5", "PrintScreen", "CapsLock"], true, Lang::En)
            {
                s.1 = Some(t);
            }
        },
        (keys::KeyPicker::default(), None),
    );
    h.run();
    h.get_by_label("RCtrl").click();
    h.run();
    h.get_by_label("CapsLock").click();
    h.run();
    assert_eq!(h.state().0.chord().as_deref(), Some("RCtrl+CapsLock"));
    h.get_by_label("Confirm").click();
    h.run();
    assert_eq!(h.state().1.as_deref(), Some("RCtrl+CapsLock"));
}
