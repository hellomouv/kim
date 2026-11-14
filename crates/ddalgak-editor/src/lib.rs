//! Ddalgak's visual rule editor (Step 74; docs/DDALGAK_GUI_JOURNEYS.md, ADR 0049).
//!
//! `ddalgak-editor.exe` is a separate, on-demand process: the tray app (`ddalgak-gui.exe`)
//! starts it from its menu or the `editor.open` shortcut, and the user can start it directly.
//! It never runs the engine, installs a hook, injects input, opens a program or a website: it
//! edits `rules.json` through the rule store exactly like the CLI ([`repo`]) and asks the
//! running app, over the same identity-checked pipe as the CLI, to load what it saved
//! ([`sync`]). Closing the window ends the process and frees its memory; the engine's idle cost
//! does not change (ADR 0049).
//!
//! - [`form`]: one rule as a sentence, the eight effect tiles and their forms on the engine's
//!   own data models, the simulation for **Test**;
//! - [`listing`]: rows, badges, search, filters and grouping;
//! - [`capture`]: key capture; [`shortcuts`]: the rule list's shortcuts from `hotkeys.json`;
//! - [`transfer`]: export and import hooks (imports arrive turned off, without consent);
//! - [`app`]: the window; [`theme`]: the Kim design tokens; [`i18n`]: Korean and English.

pub mod app;
pub mod capture;
pub mod form;
pub mod i18n;
pub mod listing;
pub mod recorder;
pub mod repo;
pub mod shortcuts;
pub mod sync;
pub mod theme;

/// ADR 0091: the editor's zoom on a screen whose work area is `w` × `h` points: as large as
/// keeps its first size (1060 × 680 plus the title bar) within 90 % of the screen, in steps of
/// 25 %, between 100 % and 200 %.
pub fn window_scale(w: f32, h: f32) -> f32 {
    let fit = (w * 0.9 / 1060.0).min(h * 0.9 / 712.0);
    ((fit * 4.0).floor() / 4.0).clamp(1.0, 2.0)
}

#[cfg(test)]
mod scale_tests {
    #[test]
    fn the_editor_is_larger_on_a_large_screen() {
        assert!((super::window_scale(5120.0, 1410.0) - 1.75).abs() < 1e-6);
        assert!((super::window_scale(1920.0, 1040.0) - 1.25).abs() < 1e-6);
        assert!((super::window_scale(1366.0, 728.0) - 1.0).abs() < 1e-6);
        assert!((super::window_scale(7680.0, 4280.0) - 2.0).abs() < 1e-6);
    }
}
pub mod transfer;

pub use app::{EditorApp, Options};

/// The editor's file name; the tray app starts it from its own folder.
pub const EDITOR_EXE: &str = "ddalgak-editor.exe";
