# ADR 0092: One design system in code for every Kim program

- Status: accepted
- Date: 2026-11-15
- Issue: maintainer request 2026-11-15 ("Kim — Unified Design System and Complete UX
  Refinement"); extends DESIGN_SYSTEM (Step 04), ADR 0014 (GUI technology), ADR 0090 (the
  launcher) and ADR 0091 (sizes); keeps ADR 0002 (independent binaries) and ADR 0023 (Noeul
  links no GUI framework)

## Context

`design/tokens.json` was the documented source of the look, but every program read it its own
way: Kim Manager and the Ddalgak rule editor each had a `theme.rs` that parsed the JSON at run
time into their own colour structs, the rule editor was always dark, the controls were egui's
defaults (checkboxes, plain buttons, selectable labels as tabs), sizes were literals spread
through the code, and only the rule editor had Korean labels. Subak and Noeul draw with Win32
and repeated the colours as hex literals. The maintainer asked for one coherent, minimal,
monochrome-with-purple ecosystem with a real, reusable implementation, settings behind a gear,
English names with explanations in the user's language, and shadcn-like switches.

## Decision

1. **Two small crates, no runtime.**
   - `kim-design`: the tokens as Rust values (both palettes, the spacing scale
     4 / 8 / 12 / 16 / 24 / 32 with a 2 px hairline step, type sizes and weights, radii, icon,
     control, button, switch, navigation, list, menu, gear, dialog, popover, tooltip, settings and
     tab sizes, state opacities, motion durations), `px()` for DPI × zoom rounding, the UI
     language (`Lang`, `LangPref`: system, English, Korean) and read-only queries of Windows'
     app theme, "Show animations" and display language. No GUI toolkit, so Subak, Noeul and the
     Ddalgak tray app (plain Win32) use it too. A test keeps it equal to `tokens.json`.
   - `kim-ui`: the egui implementation for Kim Manager, the Ddalgak rule editor and Noeul's
     settings window: the theme (both `kim-dark` and `kim-light` installed, switched without
     rebuilding), the type scale, Segoe UI with a semibold face, Malgun Gothic for Korean and
     Cascadia Mono, reduced motion, and the components: primary / secondary / ghost / danger
     buttons, the switch and the switch row, segmented choices, dropdowns, text fields,
     navigation items, list rows, setting groups and rows, dialogs and popovers, notices, field
     errors, empty states, keycaps and the shortcut field, the settings gear and progress.
     Each component reports its role and name to AccessKit, draws the 2 px focus ring on
     keyboard focus and fades when disabled.
   Each program compiles them in; nothing is shared at run time (ADR 0002). `kim-ui` is a GUI
   crate: the headless packages never reach it (`deps-check`).
2. **Window rules.** No internal top bar by default; native caption buttons, dragging,
   resizing and snapping stay. A left navigation rail only where a program has several primary
   sections (Ddalgak's rules, modes, profiles, playground; a settings view's sections); Subak
   keeps its tab bar. Secondary preferences live in Settings, opened by the gear in the
   lower-right corner of the main window, shown with sections on a rail, groups of rows and
   progressive disclosure (advanced options in their own section).
3. **Language.** Titles, navigation, feature names, buttons and technical actions stay English;
   descriptions, help and messages follow the language chosen in Settings (default: Windows'
   display language). English and Korean.
4. **Kim Manager first.** Its Settings view becomes the reference: a rail (General, Shortcuts,
   Catalog, About, Done), groups (*Appearance*: Theme, Size, Language; *Installing and updating*:
   Offline mode and update checks as switch rows, Channel), shortcut rows whose field records
   new keys when clicked, and dialogs with right-aligned actions (primary last on the right,
   the safe choice beside it). The language is saved as `language` in `manager.json`. The
   right-click menu no longer has a header row and scrolls if a window is too short (its last
   item was cut off at 1012 × 616); a long result on the status line shows two lines and an
   ellipsis, the whole text in its tooltip.

## Consequences

- One place to change a colour, a size or a component; programs look alike because they call
  the same code, not because they copy values.
- The rule editor's and Noeul's adoption, Subak's tab metrics and Ddalgak's new screens are
  recorded in their own ADRs (0093–0096) as they land.
- Tested: `kim-design` (palettes and scales equal `tokens.json`, contrast of the switch and
  focus ring in both themes, colour conversions, pixel rounding, language and theme choices,
  reduced motion), `kim-ui` (the switch by click, Space and Enter; a switch row toggles from its
  text and has one accessible name; segmented choices; disabled buttons never act; both themes;
  keycaps; the gear's corner), Kim Manager's UI tests (20, among them the language and the
  menu fitting the window) and the real desktop (sandbox profile, 275 %: Settings in dark and
  light, Korean and English descriptions, a real click on the Offline switch saved
  `offline_mode`, Light saved `theme`, the Ddalgak menu fits with *Information* visible).
