# Kim Design System

Status: **current** · Step 04, ADR 0092 · Applies to every Kim program: Kim Manager, Subak,
Noeul and Ddalgak ·
Tokens: [design/tokens.json](../design/tokens.json) (checked by `cargo xtask design-check`) ·
Code: `kim-design` (tokens, language, Windows preferences; no toolkit) and `kim-ui` (the egui
theme and components) · Toolkit: [ADR 0014](adr/0014-gui-technology.md) ·
Decision: [ADR 0092](adr/0092-unified-design-system.md)

Kim looks like a quiet instrument: a nearly black surface, compact type, very little chrome, and
one controlled lilac accent that marks *where you are* and *what needs you*. Terminal content is
the hero; everything else steps back.

## 1. Principles

1. **Content first.** Chrome is small and dim; the terminal grid and the user's text get the
   contrast budget.
2. **One accent, used sparingly.** Lilac marks focus, selection, the active tab and primary
   actions — never decoration.
3. **Dense but clear.** A 4 px grid, compact controls (28 px), generous only where reading
   happens.
4. **State is never color-only.** Every status has a shape and a text label/tooltip.
5. **Calm motion.** Short (80–160 ms), decelerating, purposeful; none when the user asks for
   reduced motion.
6. **Keyboard is first-class.** Every action is reachable and visible-focused by keyboard.
7. **Shared language, separate runtimes.** Every program compiles the same `kim-design`
   values (and, for egui windows, the same `kim-ui` components); nothing is shared at run time
   and no headless binary links GUI code ([ADR 0002](adr/0002-independent-tool-binaries.md)).
8. **Monochrome first.** Black and near-black surfaces, white and greys for text and controls;
   purple only for focus, the selected item, the one primary action, a switch that is on and
   subtle accents. No purple backgrounds, no colourful controls, no decorative panels or extra
   borders: whitespace, alignment and type carry the structure.

## 2. Tokens

The token file is the source of truth; this section explains intent. Sizes are logical pixels
(device-independent), multiplied by the monitor's DPI scale.

### 2.1 Color — theme `kim-dark` (default)

| Role | Token | Value | Use |
| --- | --- | --- | --- |
| Surface | `bg.base` | `#0E0E12` | Window and terminal background |
| | `bg.raised` | `#15151B` | Tab bar, side panels, list background |
| | `bg.overlay` | `#1C1B24` | Menus, dialogs, active tab, tooltips |
| | `bg.hover` / `bg.selected` | `#22212C` / `#2A2838` | Hover and selected rows/tabs |
| Lines | `border.subtle` | `#26252F` | Dividers, pane separators (decorative) |
| | `border.strong` | `#6A6680` | Input/control boundaries (≥ 3:1) |
| Text | `text.primary` / `.secondary` / `.muted` | `#E8E6F0` / `#A9A6B8` / `#9592A8` | Body / secondary / captions (all ≥ 4.5:1 on every surface) |
| | `text.disabled` | `#55536A` | Disabled only (exempt from contrast rules, never the only cue) |
| Accent | `accent` / `.hover` / `.pressed` | `#B7A6F5` / `#C7B9FA` / `#9B86F0` | Focus, selection, active tab marker, primary button |
| | `accent.soft` | `#2B2742` | Selected-row tint, accent chips |
| | `text.on-accent` | `#0E0E12` | Labels on accent fills (dark on lilac, 9:1) |
| Focus | `focus.ring` | `#C9BCFF` | 2 px ring, 1 px offset |
| Status | `status.success/warning/danger/info` | `#8BC79A` `#E8C27A` `#F0868F` `#8AB4F8` | Results, banners, toasts |

Measured contrast ratios are printed by `cargo xtask design-check`; the build fails if any
declared pair drops below its minimum (WCAG 2.2: 4.5:1 text, 3:1 non-text UI and focus).

**Theme `kim-light`** (Step 20; Subak's window, `appearance.theme`, [SUBAK_GUI §13](SUBAK_GUI.md)):
the same roles on a near-white surface — `bg.base` `#F7F6FA`, `bg.raised` `#ECEBF2`,
`bg.overlay` `#FFFFFF`, `bg.hover` `#E3E1EB`, `bg.selected` `#DCD7EE`, `border.subtle` `#D9D7E2`,
`border.strong` `#77738C`, text `#1C1B24` / `#45425A` / `#55526B`, the lilac accent darkened to
`#5F49C4` (4.5:1 as text, white on it 6:1), status `#23703E` `#7D5700` `#B4283B` `#235BB8`. Its own
contrast pairs are declared in `tokens.json` (a focused button's label, drawn in `bg.base` on the
accent or danger fill, is one of them); `design-check` verifies both themes. Kim Manager stays
`kim-dark`.

### 2.2 Terminal palette

Foreground `#D8D6E0` on `#0E0E12` (13.4:1), cursor lilac, selection `#3A3460`. Search matches
(Step 19) are tinted `terminal.search-match` `#4D3F1C` (the foreground stays ≥ 7:1 on it); the
current match is dark text on `terminal.search-current` `#E8C27A` (≥ 7:1), so it differs from the
others in lightness, not hue alone, and the search bar says "3 of 17" in words. The 16 ANSI
colors are tuned for the near-black background; all foreground colors except ANSI black are
≥ 4.5:1, and bright black (used for comments/dim text) stays readable. Programs that set 24-bit
colors are rendered exactly; the palette only maps the 16 indexed colors.

`kim-light`'s terminal palette (Step 20): foreground `#24232E` on `#F7F6FA`, selection
`#D6CDF6`, search match `#F6E3B0`, current match light text on `#6B4A00`. Every ANSI foreground —
the bright ones too, since bold is bright — is ≥ 4.5:1 on the light background, so ANSI "white"
is a mid grey (`#66637A`) and "bright white" the darkest ink (`#14131B`): programs that print white
on an assumed dark background stay readable.

### 2.3 Typography

| Token | Value |
| --- | --- |
| UI family | Segoe UI Variable Text → Segoe UI → **Malgun Gothic** (Korean) → Segoe UI Emoji → Segoe UI Symbol |
| Mono family | Cascadia Mono → Consolas → D2Coding (if installed) → **Malgun Gothic** → Segoe UI Emoji → Segoe UI Symbol |
| Sizes | caption 11 · body 12 · emphasis 13 · title 15 · terminal 13 (user-adjustable) |
| Weights | regular 400 · semibold 600 (titles, active tab) |
| Line height | UI 1.35 · terminal 1.2 (cell height = ceil(size × 1.2) at the device scale, or the font's ascent + descent when larger: 17 px for Cascadia Mono at 100 %, [TERMINAL_EMULATOR §5.4](TERMINAL_EMULATOR.md)) |

### 2.4 Space, radius, size

- Space scale `0, 2, 4, 6, 8, 12, 16, 24, 32` (`kim_design::space`: `XXS 2 · XS 4 · S 8 · M 12 ·
  L 16 · XL 24 · XXL 32`; 6 is kept only for existing Subak chrome). Default gaps: 8 inside
  controls and between rows, 12 between groups, 16 window and dialog padding, 24 between
  sections.
- Sizes (`kim_design::size`, ADR 0092): control 28 (compact 24, large 32); icons 14 / 16 / 20 /
  24; buttons at least 72 wide with 12 horizontal padding; switch 32 × 18 with a 14 thumb;
  checkbox 16; navigation rail 168 with 32-high items (a settings rail 148); list rows 40; menu
  items 26; the settings gear 28 at 12 from the corner; dialogs 420 (at most 440, narrower in a
  narrow window, tighter spacing under 320 points of height); popovers and tooltips 280;
  a settings view 640 × 480.
- Radius `4` (controls, chips), `6` (menus, tabs), `10` (dialogs, floating icon). No radius on
  terminal panes.
- Key sizes: tab bar 32, tab 96–220 wide, control 28 (compact 24), status dot 8, floating icon
  40, progress bar 4, minimum hit target 24×24.

### 2.5 Motion

| Token | Value | Used for |
| --- | --- | --- |
| `duration.fast` | 80 ms | hover, press |
| `duration.base` | 120 ms | tab switch highlight, focus ring appear, list selection |
| `duration.slow` | 160 ms | menu/dialog fade + 4 px rise |
| `easing.standard` | cubic-bezier(0.2, 0, 0, 1) | everything |
| `pulse.period` | 1600 ms | `in-progress` indicator (opacity 0.45 ↔ 1) |

Never animate terminal content, layout of panes, or anything during typing.

Implemented in Subak's window (Step 20, [SUBAK_GUI §13.5](SUBAK_GUI.md)): the active tab's marker
fades in over `duration.base` and a banner fades in rising 4 px over `duration.slow`, both on
`easing.standard`; a 16 ms redraw timer runs only while a transition runs, so an idle window has
no timer. Native menus follow Windows' own menu animation.

### 2.6 Elevation

Flat by default. Overlays (menus, dialogs, floating manager) use `bg.overlay`, a 1 px
`border.subtle` and one soft shadow (`0 8 24 rgba(0,0,0,0.45)`). No stacked shadows, blur or
glass effects (they cost GPU time on idle windows).

Noeul's floating pins (Step 54, [ADR 0044](adr/0044-noeul-pin-transforms-and-control.md)) use a
1 px `border.subtle` outline (`pin.border`) and a smaller soft shadow, `pin.shadow`
(`0 2 10 rgba(0,0,0,0.40)`), both switchable off. A pin window is hit-tested by its pixels, so its
shadow is also the band that resizes it; the overlay's 24-pixel blur would make a ring around
every pin catch clicks meant for the program below. The shadow is painted once into the pin's
retained surface, never animated.

## 3. Components

States for every interactive component: **rest · hover · pressed · focused (ring) · selected ·
disabled**. Focus ring is always drawn on keyboard focus, never on mouse click.

### 3.1 Status dots and the Claude indicator

8 px marks, always with a tooltip and an accessible name ("Claude: needs input").

| State | Color token | Shape (color-blind safe) | Motion |
| --- | --- | --- | --- |
| `idle` | `claude.idle` | filled circle, dim | none |
| `in-progress` | `claude.in-progress` | filled circle | pulse (static ring when reduced motion) |
| `need-input` | `claude.need-input` | filled circle + 1 px outer ring | none (attention without flashing) |
| `error` | `claude.error` | filled triangle | none |
| `done` | `claude.done` | filled circle with check notch | none; cleared when viewed |
| `unknown` | `claude.unknown` | hollow ring | none |
| `none` | — | nothing | — |

**Subak (Step 44):** the tab marks and the floating badge move as specified in
[SUBAK_BRAND §4](SUBAK_BRAND.md#4-motion) (in-progress turns; need-input, error and done give a
short cue, then hold; idle and unknown are still), static under reduced motion; the table's
motion column above is the earlier plan, superseded there for Subak.

Tool install status uses the same shapes with `status.*` colors: installed (filled), update
available (ring + dot), incompatible (triangle), not installed (nothing).

### 3.2 Kim Manager: the launcher (ADR 0090)

Since ADR 0090 the main window is a launcher, not a catalog: one `bg.base` surface, three
tiles from the upper-left corner (Subak, Noeul, Ddalgak; 88 × 100 points, the icon 58, the name
12 px below), a painted gear (lower-right, 22 points from both edges) and one status line
(11 px, lower-left; results fade after 6 s, errors stay until clicked). Window 368 × 224
(minimum 336 × 196), growing to 580 × 460 for Settings or Information. Installed tiles are full
colour; not installed ones are luminance-grey with a download mark on a `bg.overlay` disc; *needs
repair* has the mark on `status.warning`; an update is an `accent` dot. Hover: `bg.hover` at
70 %, radius 12; pressed: the icon shrinks to 54. Dialogs are at most 340 wide (inside the
window); the right-click menu is egui's context menu. Settings: *General*, *Shortcuts*,
*Catalog*, *About* as selectable labels; the theme follows the system or is fixed to
`kim-dark` / `kim-light`. All sizes here are points at 100 %: since ADR 0091 the whole
window is zoomed to fit a large screen (*Size: Auto*, up to 300 %) or to the size chosen in
Settings → General.

The two-pane catalog below is **superseded** (kept as the record of Step 06–47):

- Two-pane layout: list (min 320) | details. Window min 720×480.
- **Row** (48 high): 32 px tool icon · name (emphasis) · one-line description (muted) · version
  (mono, muted) · status chip. Selected row: `accent.soft` fill + 2 px lilac left marker.
- **Details**: icon 64, name (title), description, version/channel/license/platform table,
  release notes (scrollable), and one explicit primary action (`Install` / `Update to x.y.z` /
  `Uninstall` as secondary-danger). No action is ever triggered by selection alone.
- Offline/catalog-stale banner at top (info), with "Refresh catalog" button — never automatic.
- Details also show (Step 47) the release stage (`Planned` / `Preview` / `Available`), support,
  how the tool runs, and below the action: features, keyboard shortcuts (keys in mono, scope
  in muted text, fixed keys marked), *What it may do on your computer* (each permission in
  emphasis with its detail, opt-in ones marked), command-line and MCP entry points (planned
  ones marked), documentation links as selectable text (no browser is started), install
  location and *Updates: manual only*. A planned tool shows why there is nothing to install
  instead of a button. The pane scrolls; the list row shows `planned` instead of a version.

### 3.3 Install progress

- Inline in the details pane: phase label (`Downloading 12.4 / 38.0 MB`, `Verifying
  signature`, `Installing`, `Done`, `Failed`), 4 px bar (lilac on `bg.selected`, indeterminate
  shimmer only when size unknown; static stripes in reduced motion), `Cancel` (secondary).
- Failure: danger banner with the stable error code, a human sentence, `Retry` and `Copy
  details` (no secrets). Old version state is stated explicitly ("Version 0.2.0 is still
  installed and working").

### 3.4 Subak floating icon and tray

Subak's own identity — the black-rind, red-flesh watermelon, its colour tokens, sizes and
assets — is in [SUBAK_BRAND](SUBAK_BRAND.md) (Step 44).

- Floating icon: 40×40, radius 10, `bg.overlay`, 1 px `border.subtle`, Subak glyph centered;
  aggregate Claude indicator as a 10 px badge at the bottom-right. Draggable; snaps 8 px to
  monitor edges; hover shows a tooltip "Subak — 3 terminals, 1 needs input".
- Click: opens the compact manager (§3.6) anchored to the icon. Right-click: menu (Open
  Subak, New terminal, Hide icon, Quit GUI — never "kill").
- Tray: same aggregate badge on the 16/24/32 px tray icon; menu identical.

### 3.5 Tab bar and terminal window

- Integrated title bar: tab bar (32) in the title area, caption buttons at the right (46 wide,
  native hover colors; close button hover uses `status.danger`).
- **Tab**: 6 px top radius, 8 px horizontal padding, Claude dot (left) · name (ellipsis) ·
  close `×` on hover/active. Active tab: `bg.base` fill joining the terminal surface, semibold
  name, 2 px lilac top marker. Inactive: `bg.raised`, `text.secondary`; hovered: `bg.hover`
  (Subak, Step 20: every tab keeps its `×`, dim unless hovered or active, so the click targets
  never move).
- `+` button after the last tab (new Terminal), overflow chevron when tabs exceed width.
- Terminal surface: `terminal.background`, 8 px inner padding (configurable 0–16), no border.
- Status line (optional, tmux-style, Step 25): 22 px at the bottom, `bg.raised`, mono caption.

### 3.6 Session manager (full and compact)

- List of all Terminals the Host owns: indicator · name · state (`running`, `stopped`,
  `restorable`, `failed`) · windows/panes count · last activity. Search field on top (`/` or
  `Ctrl+F` focuses it).
- Actions: Open, Rename (`F2`), New (`Ctrl+N`), Stop, Delete (danger, confirm). "Close manager"
  never affects Terminals.

### 3.7 Panes

- Divider: 1 px `border.subtle`; drag handle area 6 px (invisible). Active pane: 1 px `accent`
  inner border at 60 % opacity (optional, default on when > 1 pane).
- Zoomed pane: small `Z` chip in the status line / tab.

### 3.8 Menus, dialogs, errors

- **Menu**: `bg.overlay`, radius 6, item height 26, shortcut hints right-aligned (muted mono),
  separators `border.subtle`. Opens in 160 ms (fade + 4 px rise).
- **Dialog**: max width 440, radius 10, title 15 semibold, body 12, buttons right-aligned;
  primary = accent fill + `text.on-accent`; destructive = `status.danger` outline/text; default
  focus is the **safe** choice (Cancel). Kill confirmation lists exactly what will end
  ("2 panes: `cargo run`, `claude`").
- **Errors**: inline banner (danger tint `#F0868F` at 12 % on `bg.raised`, danger icon, message,
  error code in mono, actions). Toasts (bottom-right, 4 s, pause on hover) only for
  non-critical outcomes; errors needing action stay until dismissed.
- **Empty / disconnected / loading** states: one muted sentence plus one action (e.g. "Host
  not running — Start"), never a blank window.

### 3.9 Buttons, switches and choices (ADR 0092, `kim-ui`)

- **Buttons**: *primary* (accent fill, `text.on-accent`, semibold; one per view), *secondary*
  (`bg.overlay`, 1 px `border.subtle`), *ghost* (no fill until hovered; rows and toolbars),
  *danger* (`status.danger` outline and text, a 16 % tint on hover). Hover and pressed change
  the fill (`accent.hover` / `accent.pressed`, `bg.hover` / `bg.selected`); keyboard focus draws
  the 2 px `focus.ring` 1 px outside; disabled fades to 45 % and never acts.
- **Switch** (shadcn-like): a pill track 32 × 18, `switch.off` grey when off and `accent` when
  on, a round 14 thumb in `text.on-accent` that slides over `duration.base` (instantly with
  reduced motion). Space or Enter toggles it; its accessible role is *switch* with its state.
  A **switch row** is the usual form in settings: the title (and a muted description) on the
  left, the switch on the right, the whole row clickable, one accessible name.
- **Segmented choice** for 2–6 short options (theme, size, language): a raised track with the
  selected segment in `bg.selected`, semibold. **Dropdowns** for longer lists; **checkboxes** only
  for confirmations ("I understand…"), never for settings.
- **Keycaps** show a shortcut (`Ctrl` `Shift` `K`, a sequence with a muted "then"); the
  **shortcut field** shows them, records new keys when clicked (accent outline and "Press
  keys…" while recording) and is named by its action.

### 3.10 Settings views and the gear (ADR 0092)

- Every program's main window has a small gear in its lower-right corner (ghost icon button,
  28 points, 12 from both edges; selected while Settings is open). Settings never fill the main
  view.
- A settings view: the sections on a left rail (`bg.raised`), the selected section on the
  right: its title (15 semibold), then **setting groups** (an 11 px semibold heading in
  `text.secondary` over a `bg.raised` panel with a subtle border, radius 6) of **rows**
  (title 12 px, description 11 px muted, the control right-aligned; a wide control below its
  text), thin dividers between rows. Common settings first; rarely used ones in an *Advanced*
  section (progressive disclosure). Changes apply at once; destructive ones ask.
- Kim Manager (reference): General (Appearance: Theme, Size, Language; Installing and
  updating: Offline mode, Check for updates when Kim starts, Channel), Shortcuts, Catalog,
  About, and Done at the bottom of the rail (Esc too).

### 3.11 Window structure (ADR 0092)

- No internal top bar or toolbar by default; native caption buttons, dragging, resizing and
  Windows snapping always work.
- A left navigation rail only when a program has several primary sections (Ddalgak); Subak's
  terminal tabs are the exception that stays visible.
- The main view keeps the program's most important function; runtime state that matters
  stays visible (a status line), configuration moves to Settings.
- Empty states: a title, one muted sentence and at most one primary action. Errors: an inline
  notice (tone glyph + message + code), validation under the field (`✕` + message, danger).
  Confirmation dialogs name exactly what happens; the safe choice sits next to the action.
  Loading: a 4 px bar (indeterminate: a moving segment, static with reduced motion).

## 4. Interaction

### 4.1 Keyboard-only

- Tab order follows reading order; `F6` cycles regions (tab bar → terminal → status line /
  list → details); `Esc` closes menus/dialogs; arrow keys move within lists, tabs and menus;
  `Enter`/`Space` activate; `F2` renames; `Shift+F10`/`Menu` opens context menus.
- Terminal focus captures keys for the program; application shortcuts follow
  [ADR 0009](adr/0009-gui-clients-and-keyboard-dispatch.md) dispatch order (IME guard → prefix →
  app shortcuts → PTY).
- A keyboard help overlay (`Ctrl+B ?` and `F1`) lists live bindings.
- Every GUI's shortcut list, keyboard-only binding editor and "Keyboard shortcuts: on/off"
  switch follow [HOTKEY_CONTRACT §11](HOTKEY_CONTRACT.md); the keys above are its reserved
  navigation actions.

### 4.2 High-DPI and multiple monitors

- Per-monitor DPI awareness v2. All tokens are logical px × scale; hairlines (1 px dividers)
  stay one *physical* pixel; text and glyph atlases re-rasterize on DPI change (no bitmap
  scaling).
- Verified scales: 100, 125, 150, 175, 200 %. Moving a window between monitors re-lays out
  without a flash of wrong size.

### 4.3 Contrast and Windows High Contrast

- `kim-dark` meets the declared pairs in `tokens.json` (checked in CI).
- When Windows High Contrast / Contrast themes are active, GUIs switch to system colors
  (`COLOR_WINDOW`, `COLOR_WINDOWTEXT`, `COLOR_HIGHLIGHT`, …) and drop the lilac accent.

### 4.4 Korean and Unicode

- Fallback chains in §2.3; Hangul syllables and CJK are **wide** (2 cells) per Unicode East
  Asian Width; ambiguous-width characters default to narrow (configurable).
- IME: composition text is drawn inline at the cursor with an underline; no shortcut fires
  during composition (ADR 0009).
- Emoji render in color where the renderer supports it; otherwise monochrome fallback, never
  tofu boxes for supported fonts.

### 4.5 Reduced motion

Honors Windows "Show animations in Windows" (`SPI_GETCLIENTAREAANIMATION`) and the app setting:
all durations become 0, pulses become static rings, progress shimmer becomes static stripes.
Subak's setting is `appearance.motion` (`system` follows Windows, `reduced` never animates,
`full` always does).

### 4.6 Language (ADR 0092)

- Titles, navigation labels, feature names, buttons and technical actions are English in both
  languages, so the same word means the same thing everywhere (Settings, Rules, Pins, Install,
  Reset). Descriptions, help, validation and messages follow the chosen language.
- English and Korean; the default is Windows' display language (`LangPref::System`, shown as
  *System (한국어)*). Korean text uses Malgun Gothic from the fallback chain; layouts wrap
  descriptions instead of fixing widths, so longer Korean or English text never overflows.
  Korean IME input works in every text field (egui's IME support; Win32 controls natively).

### 4.7 Theme, scroll, performance

- Theme: *System* follows Windows' app mode, or always dark or light; the choice is saved by
  each program. Scroll bars are thin and appear on hover. An idle window does not repaint: no
  timer runs unless an animation or a transient read-out (`duration.hint`, 1.2 s) is showing.

## 5. Implementation notes

- `kim-design` (ADR 0092) holds the tokens as Rust values (a test keeps them equal to
  `tokens.json`), `px()` for DPI × zoom, `Lang`/`LangPref` and Windows' preferences; it has no
  GUI toolkit, so the Win32 programs (Subak's window, Noeul, Ddalgak's tray app) use it.
  `kim-ui` is the egui theme and components for the egui windows (Kim Manager, the Ddalgak
  rule editor, Noeul's settings window). Headless binaries never depend on `kim-ui`.
- Visual regression: GUIs render headlessly in tests (ADR 0014) and compare snapshots of key
  components at 100 % and 200 %.
- Mock-ups: `design/mockups/*.png` are produced from the tokens by the reproducible generator
  described in [design/README.md](../design/README.md).
