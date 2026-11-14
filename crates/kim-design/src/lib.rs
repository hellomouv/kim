//! The Kim design system (docs/DESIGN_SYSTEM.md, ADR 0092) as plain Rust values.
//!
//! Every Kim program draws with these values: Kim Manager and the Ddalgak rule editor through the
//! egui components of `kim-ui`, Subak's window, Noeul's pins and Ddalgak's tray app through their
//! own Win32 painting. The crate has no GUI toolkit and no runtime: it is compiled into each
//! program (ADR 0002), so the headless tools may use it too.
//!
//! `design/tokens.json` stays the documented source; a test keeps the values here equal to it.

pub mod lang;
pub mod system;

pub use lang::{Lang, LangPref};

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// `0xRRGGBB`.
    pub const fn hex(v: u32) -> Rgb {
        Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    /// The Win32 `COLORREF` (`0x00BBGGRR`).
    pub const fn colorref(self) -> u32 {
        self.r as u32 | (self.g as u32) << 8 | (self.b as u32) << 16
    }

    /// `#RRGGBB`.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// The colour mixed with `other`: `t` = 0 is `self`, 1 is `other`.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgb {
            r: m(self.r, other.r),
            g: m(self.g, other.g),
            b: m(self.b, other.b),
        }
    }

    /// WCAG 2.2 relative luminance.
    pub fn luminance(self) -> f64 {
        let ch = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(self.r) + 0.7152 * ch(self.g) + 0.0722 * ch(self.b)
    }

    /// WCAG 2.2 contrast ratio between two colours (1–21).
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// The two themes. Dark is the reference; light uses the same roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    /// The colours of this theme.
    pub const fn palette(self) -> &'static Palette {
        match self {
            Theme::Dark => &DARK,
            Theme::Light => &LIGHT,
        }
    }

    /// `kim-dark` / `kim-light` (the names in `tokens.json`).
    pub const fn token_name(self) -> &'static str {
        match self {
            Theme::Dark => "kim-dark",
            Theme::Light => "kim-light",
        }
    }

    /// The theme Windows' "Choose your app mode" asks for (dark when it cannot be read).
    pub fn system() -> Theme {
        if system::apps_use_light_theme() {
            Theme::Light
        } else {
            Theme::Dark
        }
    }
}

/// A theme choice a program saves: follow Windows, or always dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemePref {
    pub const ALL: [ThemePref; 3] = [ThemePref::System, ThemePref::Dark, ThemePref::Light];

    /// `system`, `dark` or `light`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ThemePref::System => "system",
            ThemePref::Dark => "dark",
            ThemePref::Light => "light",
        }
    }

    pub fn parse(s: &str) -> Option<ThemePref> {
        match s.trim().to_ascii_lowercase().as_str() {
            "system" | "auto" => Some(ThemePref::System),
            "dark" | "kim-dark" => Some(ThemePref::Dark),
            "light" | "kim-light" => Some(ThemePref::Light),
            _ => None,
        }
    }

    /// The theme to draw with now.
    pub fn resolve(self) -> Theme {
        match self {
            ThemePref::System => Theme::system(),
            ThemePref::Dark => Theme::Dark,
            ThemePref::Light => Theme::Light,
        }
    }
}

/// The colour roles of one theme (DESIGN_SYSTEM §2.1). Purple (`accent*`) marks focus,
/// selection and the one primary action; everything else is black, white and grey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg_base: Rgb,
    pub bg_raised: Rgb,
    pub bg_overlay: Rgb,
    pub bg_hover: Rgb,
    pub bg_selected: Rgb,
    pub border_subtle: Rgb,
    pub border_strong: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_muted: Rgb,
    pub text_disabled: Rgb,
    pub text_on_accent: Rgb,
    pub accent: Rgb,
    pub accent_hover: Rgb,
    pub accent_pressed: Rgb,
    pub accent_soft: Rgb,
    pub focus: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub danger: Rgb,
    pub info: Rgb,
    /// The track of a switch that is off (a neutral grey, ≥ 3:1 on the surfaces).
    pub switch_off: Rgb,
}

/// `kim-dark`, the reference theme.
pub const DARK: Palette = Palette {
    bg_base: Rgb::hex(0x0E0E12),
    bg_raised: Rgb::hex(0x15151B),
    bg_overlay: Rgb::hex(0x1C1B24),
    bg_hover: Rgb::hex(0x22212C),
    bg_selected: Rgb::hex(0x2A2838),
    border_subtle: Rgb::hex(0x26252F),
    border_strong: Rgb::hex(0x6A6680),
    text_primary: Rgb::hex(0xE8E6F0),
    text_secondary: Rgb::hex(0xA9A6B8),
    text_muted: Rgb::hex(0x9592A8),
    text_disabled: Rgb::hex(0x55536A),
    text_on_accent: Rgb::hex(0x0E0E12),
    accent: Rgb::hex(0xB7A6F5),
    accent_hover: Rgb::hex(0xC7B9FA),
    accent_pressed: Rgb::hex(0x9B86F0),
    accent_soft: Rgb::hex(0x2B2742),
    focus: Rgb::hex(0xC9BCFF),
    success: Rgb::hex(0x8BC79A),
    warning: Rgb::hex(0xE8C27A),
    danger: Rgb::hex(0xF0868F),
    info: Rgb::hex(0x8AB4F8),
    switch_off: Rgb::hex(0x6A6680),
};

/// `kim-light`.
pub const LIGHT: Palette = Palette {
    bg_base: Rgb::hex(0xF7F6FA),
    bg_raised: Rgb::hex(0xECEBF2),
    bg_overlay: Rgb::hex(0xFFFFFF),
    bg_hover: Rgb::hex(0xE3E1EB),
    bg_selected: Rgb::hex(0xDCD7EE),
    border_subtle: Rgb::hex(0xD9D7E2),
    border_strong: Rgb::hex(0x77738C),
    text_primary: Rgb::hex(0x1C1B24),
    text_secondary: Rgb::hex(0x45425A),
    text_muted: Rgb::hex(0x55526B),
    text_disabled: Rgb::hex(0xA9A6B8),
    text_on_accent: Rgb::hex(0xFFFFFF),
    accent: Rgb::hex(0x5F49C4),
    accent_hover: Rgb::hex(0x523CB5),
    accent_pressed: Rgb::hex(0x4631A3),
    accent_soft: Rgb::hex(0xE7E1FB),
    focus: Rgb::hex(0x5A43C8),
    success: Rgb::hex(0x23703E),
    warning: Rgb::hex(0x7D5700),
    danger: Rgb::hex(0xB4283B),
    info: Rgb::hex(0x235BB8),
    switch_off: Rgb::hex(0x77738C),
};

/// The spacing scale (logical pixels): every gap, padding and margin is one of these.
pub mod space {
    /// Hairline gaps inside a control (an icon and its badge).
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    /// The whole scale, smallest first.
    pub const SCALE: [f32; 7] = [XXS, XS, S, M, L, XL, XXL];

    /// Padding of a window's content.
    pub const WINDOW: f32 = L;
    /// Between two sections of a page.
    pub const SECTION: f32 = XL;
    /// Between the rows of a group, and inside controls.
    pub const ROW: f32 = S;
    /// Between groups of controls.
    pub const GROUP: f32 = M;
    /// Padding of a dialog.
    pub const DIALOG: f32 = L;
}

/// Font sizes (logical pixels) and weights (DESIGN_SYSTEM §2.3).
pub mod font {
    pub const CAPTION: f32 = 11.0;
    pub const BODY: f32 = 12.0;
    pub const EMPHASIS: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    /// A page's or a large empty state's heading.
    pub const DISPLAY: f32 = 20.0;
    pub const TERMINAL: f32 = 13.0;
    pub const WEIGHT_REGULAR: u16 = 400;
    pub const WEIGHT_SEMIBOLD: u16 = 600;
    pub const LINE_HEIGHT_UI: f32 = 1.35;
    pub const LINE_HEIGHT_TERMINAL: f32 = 1.2;
    /// The UI font chain on Windows (the first installed one wins, Korean from Malgun Gothic).
    pub const UI_FAMILY: [&str; 5] = [
        "Segoe UI Variable Text",
        "Segoe UI",
        "Malgun Gothic",
        "Segoe UI Emoji",
        "Segoe UI Symbol",
    ];
    pub const MONO_FAMILY: [&str; 6] = [
        "Cascadia Mono",
        "Consolas",
        "D2Coding",
        "Malgun Gothic",
        "Segoe UI Emoji",
        "Segoe UI Symbol",
    ];
}

/// Corner radii.
pub mod radius {
    /// Controls, chips, keycaps.
    pub const S: f32 = 4.0;
    /// Menus, tabs, list rows, popovers.
    pub const M: f32 = 6.0;
    /// Dialogs, the floating icon, cards.
    pub const L: f32 = 10.0;
    /// Switches and pills.
    pub const PILL: f32 = 999.0;
}

/// Sizes of controls and regions (logical pixels).
pub mod size {
    pub const BORDER: f32 = 1.0;
    pub const FOCUS_RING: f32 = 2.0;
    pub const FOCUS_OFFSET: f32 = 1.0;
    /// The smallest click target.
    pub const HIT_MIN: f32 = 24.0;

    pub const CONTROL: f32 = 28.0;
    pub const CONTROL_COMPACT: f32 = 24.0;
    pub const CONTROL_LARGE: f32 = 32.0;

    pub const ICON_S: f32 = 14.0;
    pub const ICON_M: f32 = 16.0;
    pub const ICON_L: f32 = 20.0;
    pub const ICON_XL: f32 = 24.0;

    pub const BUTTON_MIN_WIDTH: f32 = 72.0;
    pub const BUTTON_PAD_X: f32 = 12.0;

    /// The switch (shadcn-like): a pill track and a round thumb 2 px inside it.
    pub const SWITCH_WIDTH: f32 = 32.0;
    pub const SWITCH_HEIGHT: f32 = 18.0;
    pub const SWITCH_THUMB: f32 = 14.0;
    pub const CHECKBOX: f32 = 16.0;

    /// A left navigation rail (only in programs with several primary sections).
    pub const NAV_WIDTH: f32 = 168.0;
    pub const NAV_ITEM: f32 = 32.0;
    /// A row of a compact list.
    pub const LIST_ROW: f32 = 40.0;
    pub const MENU_ITEM: f32 = 26.0;

    /// The settings gear in the lower-right corner of a main window: its hit area and the
    /// distance of its centre region from both edges.
    pub const GEAR: f32 = 28.0;
    pub const GEAR_INSET: f32 = 12.0;

    pub const DIALOG_WIDTH: f32 = 420.0;
    pub const DIALOG_MAX_WIDTH: f32 = 440.0;
    pub const POPOVER_WIDTH: f32 = 280.0;
    pub const TOOLTIP_MAX_WIDTH: f32 = 280.0;
    /// A compact settings view (a window of its own or a page in the main window).
    pub const SETTINGS_WIDTH: f32 = 640.0;
    pub const SETTINGS_HEIGHT: f32 = 480.0;
    pub const SETTINGS_NAV_WIDTH: f32 = 148.0;

    /// Subak's tab bar (scaled by the DPI and the window's zoom, never fixed pixels).
    pub const TABBAR_HEIGHT: f32 = 32.0;
    pub const TAB_MIN_WIDTH: f32 = 96.0;
    pub const TAB_MAX_WIDTH: f32 = 220.0;
    pub const TAB_PAD_X: f32 = 8.0;
    pub const TAB_CLOSE: f32 = 16.0;
    pub const CAPTION_BUTTON_WIDTH: f32 = 46.0;
    pub const PROGRESS: f32 = 4.0;
    pub const STATUS_DOT: f32 = 8.0;
}

/// Interaction states as opacities over the base colours.
pub mod state {
    /// A disabled control is drawn at this opacity (and never relies on it alone).
    pub const DISABLED_OPACITY: f32 = 0.45;
    /// The hover fill's strength over the surface.
    pub const HOVER_OPACITY: f32 = 0.7;
    /// A pressed icon shrinks to this fraction.
    pub const PRESSED_SCALE: f32 = 0.93;
}

/// Durations in milliseconds; all become 0 with reduced motion.
pub mod motion {
    pub const FAST_MS: u32 = 80;
    pub const BASE_MS: u32 = 120;
    pub const SLOW_MS: u32 = 160;
    /// A toast or a result line stays this long.
    pub const TOAST_MS: u32 = 4000;
    /// A transient value read-out (Noeul's zoom percentage) stays this long after the last
    /// change, then fades over `SLOW_MS`.
    pub const HINT_MS: u32 = 1200;
    pub const EASING: [f32; 4] = [0.2, 0.0, 0.0, 1.0];

    /// A duration in seconds, 0 when motion is reduced.
    pub fn seconds(ms: u32, reduced: bool) -> f32 {
        if reduced { 0.0 } else { ms as f32 / 1000.0 }
    }
}

/// `v` logical pixels at `scale` (DPI × zoom), rounded to whole physical pixels; a hairline
/// (`v` ≤ 1) stays at least one physical pixel.
pub fn px(v: f32, scale: f32) -> i32 {
    let s = (v * scale).round() as i32;
    if v > 0.0 { s.max(1) } else { s }
}

#[cfg(test)]
mod tests;
