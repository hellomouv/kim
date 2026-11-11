# ADR 0073 — Subak's watermelon icon and the per-Terminal Claude status motion

- **Status:** Accepted (Step 44, 2026-11-12)
- **Requirements:** `SUBAK-FR-054` (new), `SUBAK-FR-051`, `SUBAK-FR-052`, `SUBAK-NFR-002`
- **Builds on:** [ADR 0062](0062-claude-code-status-indicators.md) (Claude states), DESIGN_SYSTEM §2.5 and §4.5
  (motion, reduced motion), the floating icon art of Step 18 / Step 20

## Decision

1. **Identity.** Subak's symbol is a watermelon slice with a near-black rind and red flesh
   ([SUBAK_BRAND](../SUBAK_BRAND.md)); Kim's near-black/lilac design system is unchanged.
2. **One renderer is the source of truth.** The procedural art (`gui/float/art.rs`) draws the
   window, floating and tray icons at run time at any DPI; the committed PNGs and `.ico` are its
   output, checked pixel for pixel by a test; the SVG master repeats its geometry and colours
   (checked). No image library at run time.
3. **Motion is a pure function** (`gui/claude_motion.rs`): state + time since the state began →
   glyph, faint, and the time of the next change (`None` = no timer). In-progress turns
   continuously; need-input, error and done give a short cue and then hold; idle and unknown never
   move.
4. **Timers only while needed.** The window arms one timer at the earliest change among its
   marks and kills it when nothing moves or it is minimized; the floating icon likewise while
   shown. The tray keeps the resting badge. The Host only reports changes.
5. **Static mode** for `terminal.claude_animation = false` (new setting, default true) and for
   reduced motion (`appearance.motion`, Windows' animation setting).
6. **Aggregation order stays fixed** (`SUBAK-FR-051`), not configurable: the issue asked for a
   configurable rule "preferably"; one urgent-first order is simpler to learn and the per-Terminal
   marks are never hidden by it.

## Alternatives rejected

- **A breathing idle mark** — constant wakeups for the most common state.
- **Animating the tray icon** — Explorer redraws per frame; little benefit (the tray is often
  in the overflow).
- **Colour-only pulses** (brightness via colour mixing) — breaks theme colour mapping and
  says nothing without colour; SGR 2 faint plus distinct shapes instead.
- **A generated `.ico` via an external tool (Pillow, Inkscape)** — a second renderer that could
  drift from the run-time art.

## Consequences

- `subak.exe` / `subak-gui.exe` do not yet carry the icon as a resource (GNU toolchain, no
  resource compiler); added with the MSVC release build.
- Changing the art means regenerating the assets (`SUBAK_WRITE_ICONS=1`), enforced by the test.

## Testing

`gui/claude_motion.rs` unit tests (shapes, timings, settle, static mode, the earliest timer),
`gui/float/art.rs` (badge frames), `gui/tabs.rs` (a frame at the same width, SGR 2),
`tests/icon_assets.rs` (reproducible assets, SVG colours, legibility at 16–48 px on four
surfaces), `tests/gui_window.rs::claude_marks_move_per_terminal_only_while_shown` (two
Terminals, turning and blinking, minimized, static, settle, a new window, CPU),
`tests/gui_float.rs::the_icon_badge_moves_with_the_most_urgent_claude_state_and_the_tray_rests`,
`e2e/tests/step44_claude_motion.rs`. **Not tested:** visible windows on the maintainer's screen,
Windows 11, Linux.
