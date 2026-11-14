//! Kim Manager's look: the shared Kim design system (`kim-ui`, ADR 0092) with the theme the
//! user chose (ADR 0090).

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

pub use kim_ui::Colors as UiColors;

/// ADR 0090: whether the light theme (`kim-light`) is in use; dark (`kim-dark`) otherwise.
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Whether the light theme is in use.
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// The colours of the theme in use.
#[derive(Debug)]
pub struct Colors;

impl Colors {
    pub fn get() -> kim_ui::Colors {
        kim_ui::Colors::of(if is_light() {
            kim_ui::Theme::Light
        } else {
            kim_ui::Theme::Dark
        })
    }
}

/// Installs the design system (dark first; [`set_light`] follows the user's choice).
pub fn apply(ctx: &egui::Context) {
    kim_ui::apply(
        ctx,
        kim_ui::Options {
            theme: kim_ui::Theme::Dark,
            ..kim_ui::Options::default()
        },
    );
    LIGHT.store(false, Ordering::Relaxed);
}

/// ADR 0090: switches between the dark and the light theme.
pub fn set_light(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    kim_ui::set_theme(
        ctx,
        if light {
            kim_ui::Theme::Light
        } else {
            kim_ui::Theme::Dark
        },
    );
}
