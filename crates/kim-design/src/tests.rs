//! The Rust values are the ones `design/tokens.json` documents (DESIGN_SYSTEM §2).

use super::*;

const TOKENS: &str = include_str!("../../../design/tokens.json");

fn tokens() -> serde_json::Value {
    serde_json::from_str(TOKENS).expect("tokens.json is valid")
}

fn hex(t: &serde_json::Value, theme: &str, name: &str) -> Rgb {
    let s = t["themes"][theme]["color"][name]
        .as_str()
        .unwrap_or_else(|| panic!("{theme} {name} missing"));
    Rgb::hex(u32::from_str_radix(s.trim_start_matches('#'), 16).expect("hex"))
}

#[test]
fn both_palettes_equal_the_token_file() {
    let t = tokens();
    for theme in [Theme::Dark, Theme::Light] {
        let p = theme.palette();
        let n = theme.token_name();
        for (name, v) in [
            ("bg.base", p.bg_base),
            ("bg.raised", p.bg_raised),
            ("bg.overlay", p.bg_overlay),
            ("bg.hover", p.bg_hover),
            ("bg.selected", p.bg_selected),
            ("border.subtle", p.border_subtle),
            ("border.strong", p.border_strong),
            ("text.primary", p.text_primary),
            ("text.secondary", p.text_secondary),
            ("text.muted", p.text_muted),
            ("text.disabled", p.text_disabled),
            ("text.on-accent", p.text_on_accent),
            ("accent", p.accent),
            ("accent.hover", p.accent_hover),
            ("accent.pressed", p.accent_pressed),
            ("accent.soft", p.accent_soft),
            ("focus.ring", p.focus),
            ("status.success", p.success),
            ("status.warning", p.warning),
            ("status.danger", p.danger),
            ("status.info", p.info),
            ("switch.off", p.switch_off),
        ] {
            assert_eq!(hex(&t, n, name), v, "{n} {name}");
        }
    }
}

#[test]
fn the_scales_equal_the_token_file() {
    let t = tokens();
    let num = |path: &str| -> f32 {
        let mut v = &t;
        for p in path.split('/') {
            v = &v[p];
        }
        v.as_f64().unwrap_or_else(|| panic!("{path} missing")) as f32
    };
    assert_eq!(num("typography/size.caption"), font::CAPTION);
    assert_eq!(num("typography/size.body"), font::BODY);
    assert_eq!(num("typography/size.emphasis"), font::EMPHASIS);
    assert_eq!(num("typography/size.title"), font::TITLE);
    assert_eq!(num("typography/size.display"), font::DISPLAY);
    assert_eq!(num("radius/s"), radius::S);
    assert_eq!(num("radius/m"), radius::M);
    assert_eq!(num("radius/l"), radius::L);
    for (k, v) in [
        ("tabbar.height", size::TABBAR_HEIGHT),
        ("tab.min-width", size::TAB_MIN_WIDTH),
        ("tab.max-width", size::TAB_MAX_WIDTH),
        ("tab.padding-x", size::TAB_PAD_X),
        ("tab.close", size::TAB_CLOSE),
        ("control.height", size::CONTROL),
        ("control.height.compact", size::CONTROL_COMPACT),
        ("control.height.large", size::CONTROL_LARGE),
        ("icon.s", size::ICON_S),
        ("icon.m", size::ICON_M),
        ("icon.l", size::ICON_L),
        ("icon.xl", size::ICON_XL),
        ("button.min-width", size::BUTTON_MIN_WIDTH),
        ("button.padding-x", size::BUTTON_PAD_X),
        ("switch.width", size::SWITCH_WIDTH),
        ("switch.height", size::SWITCH_HEIGHT),
        ("switch.thumb", size::SWITCH_THUMB),
        ("checkbox", size::CHECKBOX),
        ("nav.width", size::NAV_WIDTH),
        ("nav.item.height", size::NAV_ITEM),
        ("list.row.height", size::LIST_ROW),
        ("menu.item.height", size::MENU_ITEM),
        ("gear.size", size::GEAR),
        ("gear.inset", size::GEAR_INSET),
        ("dialog.width", size::DIALOG_WIDTH),
        ("dialog.max-width", size::DIALOG_MAX_WIDTH),
        ("popover.width", size::POPOVER_WIDTH),
        ("tooltip.max-width", size::TOOLTIP_MAX_WIDTH),
        ("settings.width", size::SETTINGS_WIDTH),
        ("settings.height", size::SETTINGS_HEIGHT),
        ("settings.nav-width", size::SETTINGS_NAV_WIDTH),
        ("focus.ring.width", size::FOCUS_RING),
        ("hit-target.min", size::HIT_MIN),
    ] {
        assert_eq!(num(&format!("size/{k}")), v, "size {k}");
    }
    let scale: Vec<f32> = ["1", "2", "4", "5", "6", "7", "8"]
        .iter()
        .map(|k| num(&format!("space/{k}")))
        .collect();
    assert_eq!(scale, space::SCALE);
    assert_eq!(num("motion/duration.fast") as u32, motion::FAST_MS);
    assert_eq!(num("motion/duration.base") as u32, motion::BASE_MS);
    assert_eq!(num("motion/duration.slow") as u32, motion::SLOW_MS);
    assert_eq!(num("motion/duration.hint") as u32, motion::HINT_MS);
}

#[test]
fn the_switch_and_focus_stay_visible_in_both_themes() {
    for theme in [Theme::Dark, Theme::Light] {
        let p = theme.palette();
        for bg in [p.bg_base, p.bg_raised, p.bg_overlay] {
            assert!(p.switch_off.contrast(bg) >= 3.0, "{theme:?} off track");
            assert!(p.accent.contrast(bg) >= 3.0, "{theme:?} on track");
            assert!(p.focus.contrast(bg) >= 3.0, "{theme:?} focus");
        }
        // The thumb is drawn in text.on-accent on the accent track.
        assert!(
            p.text_on_accent.contrast(p.accent) >= 3.0,
            "{theme:?} on thumb"
        );
    }
}

#[test]
fn colours_convert_for_win32_and_mix() {
    let c = Rgb::hex(0x123456);
    assert_eq!(c.colorref(), 0x0056_3412);
    assert_eq!(c.to_hex(), "#123456");
    assert_eq!(Rgb::hex(0).mix(Rgb::hex(0xFFFFFF), 0.5), Rgb::hex(0x808080));
    assert!((Rgb::hex(0).contrast(Rgb::hex(0xFFFFFF)) - 21.0).abs() < 0.01);
}

#[test]
fn pixels_round_and_hairlines_survive() {
    assert_eq!(px(1.0, 1.0), 1);
    assert_eq!(px(1.0, 0.5), 1);
    assert_eq!(px(32.0, 1.75), 56);
    assert_eq!(px(0.0, 2.0), 0);
}

#[test]
fn language_and_theme_choices_parse_and_resolve() {
    assert_eq!(LangPref::parse("auto"), Some(LangPref::System));
    assert_eq!(LangPref::parse("KO"), Some(LangPref::Ko));
    assert_eq!(LangPref::parse("한국어"), Some(LangPref::Ko));
    assert_eq!(LangPref::parse("fr"), None);
    assert_eq!(LangPref::En.resolve(), Lang::En);
    assert_eq!(Lang::Ko.pick("Save", "저장"), "저장");
    assert!(LangPref::System.label().starts_with("System ("));
    assert_eq!(ThemePref::parse("kim-light"), Some(ThemePref::Light));
    assert_eq!(ThemePref::Dark.resolve(), Theme::Dark);
    for p in ThemePref::ALL {
        assert_eq!(ThemePref::parse(p.as_str()), Some(p));
    }
    // Reading Windows' choices never fails.
    let _ = (
        Theme::system(),
        Lang::system(),
        system::animations_enabled(),
    );
}

#[test]
fn reduced_motion_removes_every_duration() {
    assert_eq!(motion::seconds(motion::BASE_MS, true), 0.0);
    assert!((motion::seconds(motion::BASE_MS, false) - 0.12).abs() < 1e-6);
}
