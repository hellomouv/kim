# Subak brand: the watermelon icon and the Claude status motion

- **Status:** current (Step 44, 2026-11-12). Decision record: [ADR 0073](adr/0073-subak-icon-and-claude-motion.md).
  Requirement: `SUBAK-FR-054` (and `SUBAK-FR-051` for the aggregate). Kim's own design system
  (near-black and lilac, [DESIGN_SYSTEM](DESIGN_SYSTEM.md)) stays as it is; this black-and-red
  palette is **Subak's identity only**.

## 1. The symbol

Subak (수박) is a **watermelon slice with a near-black rind and vivid red flesh** — not the
usual green rind. A half-disc, flat edge on top, on Kim's rounded `bg.overlay` tile:

| Part | Token (`subak.*`) | Colour | Notes |
| --- | --- | --- | --- |
| Tile | `bg.overlay` | `#1C1B24` | radius 10 of 40, 1 px `border.subtle` `#26252F` |
| Rind edge | `subak.rind-edge` | `#6E6B7C` | a light rim so the rind never reads black-on-black |
| Rind | `subak.rind` | `#232226` | charcoal; at least 1.6 device px at every size |
| Band | `subak.band` | `#F1E7D0` | a hairline between rind and flesh |
| Flesh | `subak.flesh` | `#E5383B` | saturated red |
| Seeds | `subak.seed` | `#161418` | five drops; left out below 25 px (sub-pixel specks) |

Geometry (40-unit master): centre (20, 14.5), radius 17 — the slice is 34 of 40 units wide so it
reads at 16 px. The rind is 2.6 units (more at small sizes), the band 1 unit.

**Measured contrast** (`crates/subak/tests/icon_assets.rs`, WCAG relative luminance): flesh
against rind **3.73 : 1**. The share of icon pixels that stand at least 3 : 1 apart from the
surface behind it, at 16 / 20 / 24 / 32 / 48 px: on near-black `#0B0B0D` 17 / 20 / 20 / 17 / 19 %
(the red flesh and the light rim; the dark tile itself blends in by design), on black the same,
on white 92 / 92 / 92 / 93 / 91 %, on light grey `#F3F3F3` 91–92 %. The test fails below 12 %.

## 2. Variants and assets

All files in [`assets/subak/`](../assets/subak/) (MIT OR Apache-2.0, like the repository):

| File | Use |
| --- | --- |
| `subak.svg` | the vector master (Linux desktop entries later, documents, scaling) |
| `subak.ico` | Windows: 16, 20, 24, 32, 40, 48, 64, 128, 256 px, PNG entries (≈ 22 KB) |
| `subak-{16,20,24,32,48,64,128,256}.png` | PNG sizes (Linux hicolor sizes later, docs, catalog) |
| `subak-mono-dark-{16,32,48}.png` | monochrome, black outline on white (light High Contrast, print) |
| `subak-mono-light-{16,32,48}.png` | monochrome, white outline on black (dark High Contrast) |
| `preview-dark.png`, `preview-light.png` | 16–48 px on a near-black and a light surface (the visual check) |

**One renderer, reproducible.** Every PNG and the `.ico` are produced by the same code that
draws the window icon, the floating icon and the tray at run time
(`crates/subak/src/gui/float/art.rs`, 4 × 4 supersampled). `tests/icon_assets.rs` regenerates
them in memory and **fails when a committed file's pixels differ** (decoded, so the PNG
compressor in use does not matter); regenerate with
`SUBAK_WRITE_ICONS=1 cargo test -p subak --test icon_assets`. The test also checks the SVG names
the renderer's colours, the size of the files, and the legibility numbers above.

**Where it is used:** the Subak window's taskbar, Alt+Tab and caption icon (`WM_SETICON`, drawn
at the monitor's DPI), the floating icon and the tray icon (the High Contrast variant under
Windows High Contrast). **Not yet:** the icon resource inside `subak.exe` / `subak-gui.exe`
(Explorer's file icon) — the GNU toolchain here has no resource compiler; it is added with the
MSVC release build (as Kim-Setup's `asInvoker` manifest, ADR 0072). Subak has no installer of
its own (Kim installs it); Kim Manager does not show tool icons yet.

## 3. Aggregation

Each Terminal has one Claude state (its most urgent pane, Step 28). The floating icon and tray
show the most urgent of all Terminals, in the fixed order **need-input > error > done >
in-progress > unknown > idle** (`SUBAK-FR-051`); the manager's rows and the tabs keep every
Terminal's own state, so the aggregate never hides one. A failed Terminal outranks done; need-input
and error outrank a failure. The order is fixed (not a setting): urgent states first is the only
order that answers "do I need to look?"; a configurable order is a tracked idea, not built.

## 4. Motion

Shape first, never colour alone; motion only adds to the shape. One small mark per tab (a
character of the tab bar) and the 10 px badge of the floating icon:

| State | Tab mark | Floating badge | Motion | Then |
| --- | --- | --- | --- | --- |
| in-progress | `◐ ◓ ◑ ◒` | half-filled disc | a quarter turn every **300 ms** (1.2 s per turn) | continues while Claude works |
| need-input | `!` | filled disc | **three blinks**: faint/full, 250 ms each (1.5 s) | steady |
| error | `✕` | square | **two short flashes**, 150 ms each (0.6 s) | steady |
| done | `✓` | ring with a centre | faint for 150 ms, then full (a soft arrival) | steady |
| idle | `◦` | none | none (resting) | — |
| unknown | `?` | ring | none: honest, static | — |

The motion clock of a mark restarts when its Terminal's state changes; a newly opened window
shows each cue once. **Idle does not breathe**: it is the most common state of a Terminal with
Claude, and a slow pulse would wake the window several times a second to say nothing. Faint is
drawn as SGR 2 in the tab bar (every theme's colours still apply) and at 40 % coverage on the badge.

**Static mode** — `terminal.claude_animation = false`, `appearance.motion = "reduced"`, or
Windows' "Show animations in Windows" off: every mark shows its resting glyph (`◔ ! ✕ ✓ ◦ ?`)
at once and no timer runs.

## 5. Lifecycle and cost

- **One timer per window**, armed at the earliest frame any shown mark needs and killed when
  none moves (idle, unknown, settled cues, static mode) or the window is **minimized**. The
  floating icon has its own, only while it is shown with a moving badge.
- **The tray keeps the resting badge**: a notification icon updated every 300 ms would make
  Explorer redraw it; the tray changes only when the state changes.
- **The Host animates nothing**: it only sends `terminal.claude_changed` (Step 28). Headless
  `subak` commands never draw.
- **Measured** (debug build, off-screen window, Windows 10 22H2; `gui_window.rs`,
  `gui_float.rs`): while one tab turns, 10–11 tab-bar frames in 3 s and **0.0 ms of CPU time
  over 10 s** (below one 15.6 ms accounting tick of `GetProcessTimes`); minimized 2 s: 0 frames,
  0.0 ms; static 2 s: 0 frames, 0.0 ms. The floating badge: 8 frames in 2.5 s, 4 distinct
  pictures, no tray update per frame; after the need-input cue no timer although a Terminal is
  still working (the badge shows the most urgent state, which has settled).

## 6. Accessibility

Every state keeps its own shape and its words: the window title "(Claude needs input)", the
floating icon's tooltip and accessible name ("…, Claude needs input"), the manager's row
("· ! Claude need-input": the resting mark and the words; the row does not move — the manager
is a short-lived flyout, the tab and the badge carry the motion). Static mode keeps all of it. High Contrast draws the watermelon as an
outline in the system text colour and the badge in the highlight colour. Tab keys, focus and IME
are untouched (the mark is part of the tab's text, not a separate control).

## 7. Linux (later)

The SVG master and the PNG sizes are ready for a hicolor icon theme and a `.desktop` entry; the
motion module is portable Rust. **Not tested on Linux** (no Linux UI exists; AGENTS.md §3).

## 8. Not tested

A visible window and tray on the maintainer's screen (the agents work headless): the look of the
turning mark in the real font, the badge on a real taskbar at 125–200 % scale, Windows 11, a
light taskbar, Narrator reading the title. Screenshots are added with those checks.
