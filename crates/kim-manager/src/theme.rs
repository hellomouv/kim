//! Kim design tokens (design/tokens.json) applied to egui.

use eframe::egui;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// ADR 0090: whether the light theme (`kim-light`) is in use; dark (`kim-dark`) otherwise.
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Whether the light theme is in use.
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

const TOKENS: &str = include_str!("../../../design/tokens.json");

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub bg_base: egui::Color32,
    pub bg_raised: egui::Color32,
    pub bg_overlay: egui::Color32,
    pub bg_hover: egui::Color32,
    pub bg_selected: egui::Color32,
    pub border_subtle: egui::Color32,
    pub border_strong: egui::Color32,
    pub text_primary: egui::Color32,
    pub text_secondary: egui::Color32,
    pub text_muted: egui::Color32,
    pub text_on_accent: egui::Color32,
    pub accent: egui::Color32,
    pub accent_soft: egui::Color32,
    pub focus: egui::Color32,
    pub success: egui::Color32,
    pub warning: egui::Color32,
    pub danger: egui::Color32,
    pub info: egui::Color32,
}

fn color(tokens: &serde_json::Value, theme: &str, name: &str) -> egui::Color32 {
    let hex = tokens["themes"][theme]["color"][name]
        .as_str()
        .unwrap_or("#ff00ff");
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xff00ff);
    let [_, r, g, b] = v.to_be_bytes();
    egui::Color32::from_rgb(r, g, b)
}

impl Colors {
    /// The colours of the theme in use.
    pub fn get() -> Self {
        static DARK: OnceLock<Colors> = OnceLock::new();
        static LIGHT_COLORS: OnceLock<Colors> = OnceLock::new();
        if is_light() {
            *LIGHT_COLORS.get_or_init(|| Self::load("kim-light"))
        } else {
            *DARK.get_or_init(|| Self::load("kim-dark"))
        }
    }

    fn load(theme: &str) -> Self {
        {
            let t: serde_json::Value =
                serde_json::from_str(TOKENS).expect("design/tokens.json is valid");
            let color = |name: &str| color(&t, theme, name);
            Colors {
                bg_base: color("bg.base"),
                bg_raised: color("bg.raised"),
                bg_overlay: color("bg.overlay"),
                bg_hover: color("bg.hover"),
                bg_selected: color("bg.selected"),
                border_subtle: color("border.subtle"),
                border_strong: color("border.strong"),
                text_primary: color("text.primary"),
                text_secondary: color("text.secondary"),
                text_muted: color("text.muted"),
                text_on_accent: color("text.on-accent"),
                accent: color("accent"),
                accent_soft: color("accent.soft"),
                focus: color("focus.ring"),
                success: color("status.success"),
                warning: color("status.warning"),
                danger: color("status.danger"),
                info: color("status.info"),
            }
        }
    }
}

/// Applies the dark theme and the Korean/symbol font fallback chain.
pub fn apply(ctx: &egui::Context) {
    set_light(ctx, false);
    fonts(ctx);
}

/// ADR 0090: switches between the dark and the light theme (design tokens).
pub fn set_light(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    let c = Colors::get();
    let mut v = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.panel_fill = c.bg_base;
    v.window_fill = c.bg_overlay;
    v.extreme_bg_color = c.bg_base;
    v.faint_bg_color = c.bg_raised;
    v.override_text_color = Some(c.text_primary);
    v.hyperlink_color = c.accent;
    v.selection.bg_fill = c.accent_soft;
    v.selection.stroke = egui::Stroke::new(1.0, c.accent);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, c.border_subtle);
    v.widgets.inactive.bg_fill = c.bg_overlay;
    v.widgets.inactive.weak_bg_fill = c.bg_overlay;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, c.border_strong);
    v.widgets.hovered.bg_fill = c.bg_hover;
    v.widgets.hovered.weak_bg_fill = c.bg_hover;
    v.widgets.active.bg_fill = c.bg_selected;
    v.widgets.active.weak_bg_fill = c.bg_selected;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(2.0, c.focus);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, c.text_primary);
    ctx.set_visuals(v);
}

fn fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, file) in [("malgun", "malgun.ttf"), ("symbol", "seguisym.ttf")] {
        if let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{file}")) {
            fonts.font_data.insert(
                name.into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}
