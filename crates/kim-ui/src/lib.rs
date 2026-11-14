//! The Kim design system for egui (docs/DESIGN_SYSTEM.md, ADR 0092).
//!
//! One theme and one set of components for every egui window of Kim: Kim Manager, the Ddalgak
//! rule editor and Noeul's settings window. The values come from [`kim_design`]; Subak's window,
//! Noeul's pins and Ddalgak's tray app draw the same tokens with Win32. A library compiled into
//! each program, never a shared runtime (ADR 0002).
//!
//! [`apply`] installs the theme (both `kim-dark` and `kim-light`, switched with
//! [`set_theme`]), the spacing, the type scale, the Korean font fallback and reduced motion. The
//! components are in [`widgets`].

pub use kim_design::{self as design, Lang, LangPref, Theme, ThemePref};

pub mod keys;
pub mod widgets;

pub use widgets::*;

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use kim_design::{Palette, Rgb, font, motion, radius, size, space};

/// The family name of the semibold UI face (titles, the selected tab, primary labels).
pub const SEMIBOLD: &str = "kim-semibold";

/// An egui colour from a token.
pub fn c32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// The colour roles of a theme as egui colours.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub bg_base: Color32,
    pub bg_raised: Color32,
    pub bg_overlay: Color32,
    pub bg_hover: Color32,
    pub bg_selected: Color32,
    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_disabled: Color32,
    pub text_on_accent: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_pressed: Color32,
    pub accent_soft: Color32,
    pub focus: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub info: Color32,
    pub switch_off: Color32,
}

impl Colors {
    /// The colours of `theme`.
    pub fn of(theme: Theme) -> Colors {
        let p: &Palette = theme.palette();
        Colors {
            bg_base: c32(p.bg_base),
            bg_raised: c32(p.bg_raised),
            bg_overlay: c32(p.bg_overlay),
            bg_hover: c32(p.bg_hover),
            bg_selected: c32(p.bg_selected),
            border_subtle: c32(p.border_subtle),
            border_strong: c32(p.border_strong),
            text_primary: c32(p.text_primary),
            text_secondary: c32(p.text_secondary),
            text_muted: c32(p.text_muted),
            text_disabled: c32(p.text_disabled),
            text_on_accent: c32(p.text_on_accent),
            accent: c32(p.accent),
            accent_hover: c32(p.accent_hover),
            accent_pressed: c32(p.accent_pressed),
            accent_soft: c32(p.accent_soft),
            focus: c32(p.focus),
            success: c32(p.success),
            warning: c32(p.warning),
            danger: c32(p.danger),
            info: c32(p.info),
            switch_off: c32(p.switch_off),
        }
    }

    /// The colours of the theme `ctx` draws with now.
    pub fn get(ctx: &egui::Context) -> Colors {
        Colors::of(theme_of(ctx))
    }
}

/// The theme `ctx` draws with now.
pub fn theme_of(ctx: &egui::Context) -> Theme {
    if ctx.global_style().visuals.dark_mode {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// The egui visuals of a theme.
pub fn visuals(theme: Theme) -> egui::Visuals {
    let c = Colors::of(theme);
    let mut v = match theme {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    };
    v.panel_fill = c.bg_base;
    v.window_fill = c.bg_overlay;
    v.window_stroke = Stroke::new(size::BORDER, c.border_subtle);
    v.window_corner_radius = CornerRadius::same(radius::L as u8);
    v.menu_corner_radius = CornerRadius::same(radius::M as u8);
    let shadow_alpha = if theme == Theme::Dark { 115 } else { 40 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.extreme_bg_color = c.bg_raised;
    v.text_edit_bg_color = Some(c.bg_raised);
    v.faint_bg_color = c.bg_raised;
    v.code_bg_color = c.bg_raised;
    v.override_text_color = None;
    v.hyperlink_color = c.accent;
    v.warn_fg_color = c.warning;
    v.error_fg_color = c.danger;
    v.selection.bg_fill = c.accent_soft;
    v.selection.stroke = Stroke::new(size::BORDER, c.accent);
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    let r = CornerRadius::same(radius::S as u8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c.bg_raised;
    w.noninteractive.weak_bg_fill = c.bg_raised;
    w.noninteractive.bg_stroke = Stroke::new(size::BORDER, c.border_subtle);
    w.noninteractive.fg_stroke = Stroke::new(size::BORDER, c.text_primary);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = c.bg_overlay;
    w.inactive.weak_bg_fill = c.bg_overlay;
    w.inactive.bg_stroke = Stroke::new(size::BORDER, c.border_subtle);
    w.inactive.fg_stroke = Stroke::new(size::BORDER, c.text_primary);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = c.bg_hover;
    w.hovered.weak_bg_fill = c.bg_hover;
    w.hovered.bg_stroke = Stroke::new(size::BORDER, c.border_strong);
    w.hovered.fg_stroke = Stroke::new(size::BORDER, c.text_primary);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    // egui shows a keyboard-focused widget with the `active` look: the 2 px focus ring.
    w.active.bg_fill = c.bg_selected;
    w.active.weak_bg_fill = c.bg_selected;
    w.active.bg_stroke = Stroke::new(size::FOCUS_RING, c.focus);
    w.active.fg_stroke = Stroke::new(size::BORDER, c.text_primary);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open.bg_fill = c.bg_selected;
    w.open.weak_bg_fill = c.bg_selected;
    w.open.bg_stroke = Stroke::new(size::BORDER, c.border_strong);
    w.open.fg_stroke = Stroke::new(size::BORDER, c.text_primary);
    w.open.corner_radius = r;
    v
}

/// What [`apply`] sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub theme: Theme,
    /// No animation at all (Windows' "Show animations" off, or the program's own setting).
    pub reduced_motion: bool,
    /// Load the Windows UI fonts (Segoe UI, Malgun Gothic for Korean, Cascadia Mono).
    pub system_fonts: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            theme: Theme::Dark,
            reduced_motion: !kim_design::system::animations_enabled(),
            system_fonts: true,
        }
    }
}

/// Installs the theme, spacing, type scale, motion and fonts.
pub fn apply(ctx: &egui::Context, o: Options) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals(Theme::Dark));
    ctx.set_visuals_of(egui::Theme::Light, visuals(Theme::Light));
    set_theme(ctx, o.theme);
    let anim = motion::seconds(motion::BASE_MS, o.reduced_motion);
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(font::CAPTION)),
            (TextStyle::Body, FontId::proportional(font::BODY)),
            (TextStyle::Button, FontId::proportional(font::BODY)),
            (TextStyle::Heading, FontId::proportional(font::TITLE)),
            (TextStyle::Monospace, FontId::monospace(font::BODY)),
        ]
        .into();
        let sp = &mut s.spacing;
        sp.item_spacing = egui::vec2(space::S, space::S);
        sp.button_padding = egui::vec2(space::S, space::XS);
        sp.interact_size = egui::vec2(size::HIT_MIN * 2.0, size::CONTROL_COMPACT);
        sp.icon_width = size::CHECKBOX;
        sp.icon_width_inner = size::CHECKBOX - 6.0;
        sp.icon_spacing = space::S;
        sp.menu_margin = Margin::same(space::XS as i8);
        sp.window_margin = Margin::same(space::DIALOG as i8);
        sp.combo_width = 160.0;
        sp.text_edit_width = 240.0;
        sp.tooltip_width = size::TOOLTIP_MAX_WIDTH;
        sp.indent = space::L;
        sp.scroll = egui::style::ScrollStyle::thin();
        s.animation_time = anim;
        s.interaction.tooltip_delay = 0.4;
    });
    install_fonts(ctx, o.system_fonts);
}

/// Switches between the two themes (both are installed by [`apply`]).
pub fn set_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::Dark => egui::Theme::Dark,
        Theme::Light => egui::Theme::Light,
    });
}

/// Turns animations off or on (Windows' setting or the program's own).
pub fn set_reduced_motion(ctx: &egui::Context, reduced: bool) {
    let t = motion::seconds(motion::BASE_MS, reduced);
    ctx.all_styles_mut(|s| s.animation_time = t);
}

/// The Windows UI fonts with the Korean and symbol fallback (DESIGN_SYSTEM §2.3): Segoe UI
/// (semibold for [`SEMIBOLD`]), then Malgun Gothic, then Segoe UI Symbol; Cascadia Mono or
/// Consolas for monospace. A font that is not installed is skipped; egui's own fonts stay as
/// the last fallback, so text always renders. Without `system` only egui's fonts are used (the
/// [`SEMIBOLD`] family then names them too).
pub fn install_fonts(ctx: &egui::Context, system: bool) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"C:\Windows".into())
        .join("Fonts");
    let mut load = |name: &str, files: &[&str]| -> Option<String> {
        if !system {
            return None;
        }
        for f in files {
            if let Ok(bytes) = std::fs::read(dir.join(f)) {
                fonts.font_data.insert(
                    name.into(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                return Some(name.to_string());
            }
        }
        None
    };
    let ui = load("segoe", &["segoeui.ttf"]);
    let semi = load("segoe-semibold", &["seguisb.ttf", "segoeuib.ttf"]);
    let malgun = load("malgun", &["malgun.ttf"]);
    let malgun_bold = load("malgun-bold", &["malgunbd.ttf"]);
    let symbol = load("symbol", &["seguisym.ttf"]);
    let mono = load("mono", &["CascadiaMono.ttf", "consola.ttf"]);
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mono_defaults = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let chain = |first: Vec<Option<String>>, rest: &[String]| -> Vec<String> {
        let mut v: Vec<String> = first.into_iter().flatten().collect();
        for r in rest {
            if !v.contains(r) {
                v.push(r.clone());
            }
        }
        v
    };
    let prop = chain(vec![ui.clone(), malgun.clone(), symbol.clone()], &defaults);
    let semibold = chain(
        vec![semi.or(ui), malgun_bold.or(malgun.clone()), symbol.clone()],
        &defaults,
    );
    let monos = chain(vec![mono, malgun, symbol], &mono_defaults);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts.families.insert(FontFamily::Monospace, monos);
    ctx.set_fonts(fonts);
}

/// A font of the UI face at `size` points; `semibold` uses [`SEMIBOLD`] once the context has it
/// (fonts take effect a frame after [`install_fonts`]; a context without the design system
/// uses the regular face instead of failing).
pub fn ui_font(ctx: &egui::Context, size: f32, semibold: bool) -> FontId {
    let ready = semibold
        && ctx.fonts(|f| {
            f.definitions()
                .families
                .contains_key(&FontFamily::Name(SEMIBOLD.into()))
        });
    if ready {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    } else {
        FontId::proportional(size)
    }
}

#[cfg(test)]
mod tests;
