# Noeul acceptance on real Windows (the maintainer's script)

- **Status:** current (Step 63, 2026-11-12). Run by the maintainer with a physical mouse and keyboard
  on visible windows — the development agents never do this (AGENTS.md §5). Record the date, the
  build (`noeul --version`, `git rev-parse --short HEAD`) and each result in the Step 63 issue.

## 0. Prepare

1. `cargo xtask package noeul` (or a downloaded release), then `kim install noeul` from a catalog that
   lists it — or unzip and run `noeul.exe start`.
2. A clean profile: `noeul config reset --yes`, `noeul hotkeys reset --all --yes`.
3. Optional: a second monitor at a different scale (100 % and 150 %).

## 1. The signature flow (must pass)

| # | Do | Expect |
| --- | --- | --- |
| 1 | Press `F1` | the overlay covers every monitor at once, frozen, within a blink; the keyboard help shows at the first run |
| 2 | Drag a region; double-click inside it | a floating pin of exactly that region appears **immediately** where it was; the overlay is gone; the clipboard is unchanged |
| 3 | Drag the pin; mouse wheel; `1`/`2`/`3`/`4`; `Ctrl`+wheel | it moves, zooms around the pointer, turns, flips, fades (never below 10 %) |
| 4 | `Esc` on the pin; tray → Reopen closed pin | it closes; it comes back in the same place |
| 5 | Copy an image in a browser; press `F3` | a pin of the image at the pointer |
| 6 | `F1`, hover windows and controls, `E`, `M`, `C` | windows and elements highlight; the magnifier follows; `C` copies the colour under the crosshair |
| 7 | `F1`, select, draw a rectangle, a redaction and text; `Ctrl+C`; paste into Paint | the pasted picture has the drawing; nothing under the redaction |
| 8 | `F1`, select, `P`, `Ctrl+C`; `noeul history list` | copied, but the capture is not in the history |
| 9 | Tray → Settings…; select "Capture a region", Change, press `Ctrl+Alt+F9` | the overlay does **not** open while recording; F1 no longer opens it, `Ctrl+Alt+F9` does; Reset brings F1 back |
| 10 | Settings: Tab through every control; Narrator on | each control is reached and read by its name; Esc in the recorder cancels |
| 11 | Windows High Contrast on; dark mode on | the settings window, overlay and pins stay readable |
| 12 | Pin, then `Ctrl+Shift+T` (click-through); tray → Make all pins clickable | clicks pass through; the tray item brings the pin back |
| 13 | Unplug or rearrange a monitor with a pin on it; tray → Bring pins on screen | the pin is kept, then brought onto a visible monitor |
| 14 | Lock the session (`Win+L`) and run `noeul capture --region 0,0,10,10 --pin` from another session or a scheduled task | refused with `desktop_unavailable`; nothing captured |
| 15 | `noeul config set pins.backup on`; pin two images; quit; start again; tray → Restore pins | the notice offers the pins; nothing comes back until chosen; both return in place |

## 2. Resources (compare with `docs/perf/noeul-baseline.json`)

Task Manager with Noeul idle in the tray and three pins: CPU 0 %, memory near the baseline;
`cargo xtask bench-noeul --check` passes on this machine.

## 3. Release and removal

`kim update noeul` with a newer catalog entry (never by itself); `kim uninstall noeul` keeps
`%APPDATA%\mouv\noeul` and `%LOCALAPPDATA%\mouv\noeul`; `kim uninstall noeul --purge-data` removes
both after confirmation. SmartScreen with an unsigned and (when a certificate exists) a signed build.

Any failure: a new issue with the row number, what happened and the build.
