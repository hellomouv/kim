# ADR 0091: Windows open larger on large screens

- Status: accepted
- Date: 2026-11-14
- Issue: maintainer request 2026-11-14 ("Kim.exe 및 다른 앱들도 지금 창이 너무 작아. 창 크기를
  3~4배 키워도 될 것 같아. 좀 크게 크게 쓸 수 있도록 해줘."); extends ADR 0090 (the launcher
  size) and the first-window sizes of Subak (Step 16), the Ddalgak rule editor (ADR 0049) and
  Noeul's settings window

## Context

The maintainer's screen is 5120 × 1440 at 100 % display scaling. Sizes chosen for a 1080p
laptop look tiny there: the launcher was 368 × 224 pixels with 11–12 px text, Subak's first
window 800 × 600, Noeul's settings 600 × 740 with the 9-point message font. Windows display
scaling would fix it for the whole desktop, but that is the user's choice and must not be
changed by Kim (AGENTS.md §5).

## Decision

Each window picks its size from the **work area of the screen it opens on** (without the
taskbar), in steps of 25 %, never smaller than before:

| Window | Rule | 5120 × 1410 (this PC) | 1920 × 1040 | Small screens |
|---|---|---|---|---|
| Kim (launcher) | everything zoomed so that the largest view, Settings (580 × 460 points plus the title bar), still fits; up to 300 % | 275 % (launcher 1012 × 616) | 200 % | 100 % |
| Subak, first window (nothing saved yet) and default text | 800 × 600 logical pixels, up to twice, within 80 % of the work area (never smaller than the default grid); the text size the same factor larger when `config.toml` does not set `appearance.font_size` | 1400 × 1050 (or the 120 × 30 grid if larger), text 23 px | 1000 × 750, 16 px | 800 × 600, 13 px |
| Ddalgak rule editor | everything zoomed, 1060 × 680 within 90 %; up to 200 % | 175 % | 125 % | 100 % |
| Noeul settings | controls and font zoomed, 600 × 740 within 90 %; up to 200 % | 150 % | 125 % | 100 % |

1. In Kim and the rule editor (egui) the zoom is egui's zoom factor: text, icons, dialogs and
   menus grow together and the layout in points is unchanged (ADR 0090's layout and its tests
   hold). Kim has a **Size** choice in Settings → General: *Auto (n %)* (the default, the rule
   above) or a fixed 100 / 150 / 200 / 250 / 300 %, applied at once (the window grows with it),
   saved as `scale` in `manager.json` and read before the next window opens.
2. Subak keeps restoring the size the user left it at (`gui.json`); the rule only sets the
   first window. Its text follows the same factor unless `config.toml` sets a size (that value
   always wins; *Reset zoom* returns to the screen's default, `Ctrl+=` / `Ctrl+-` save a size as
   before). The tab bar keeps its DPI size. Noeul's settings window multiplies its DPI layout
   by the zoom (and now paints its dark background, which it did not).
3. Kim moves a window that grows for Settings or Information (or a larger Size) up or left so
   that it stays within the work area of its screen, and back when it shrinks.
4. Nothing changes Windows' display scaling or any other desktop setting. Test windows (the
   off-screen test modes) keep their fixed sizes, so the existing tests are unaffected.

## Consequences

- On a large screen everything is readable without changing Windows' scaling; on a laptop
  nothing changes.
- Tested: unit tests for each rule (`kim_manager::auto_scale`, `subak::gui::layout::first_size`,
  `screen_zoom`, `default_font`,
  `ddalgak_editor::window_scale`, `noeul::win::settings::zoom_for`), the Kim UI test
  `the_size_follows_the_screen_or_the_choice_and_persists` (Auto, a fixed size applied at once,
  saved and read again, out-of-range values clamped), and the real desktop (each window opened
  at its new size on this PC). An e2e scenario does not apply: the size depends on the real
  screen, and the e2e windows open off-screen at fixed sizes.
