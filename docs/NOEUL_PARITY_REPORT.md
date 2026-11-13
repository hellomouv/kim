# Noeul parity report (Snipaste feature matrix)

- **Status:** current (Step 63, 2026-11-12). Source of truth: [NOEUL_SNIPASTE_MATRIX](NOEUL_SNIPASTE_MATRIX.md)
  (every row with its status and evidence; `cargo xtask docs-check` resolves every evidence path).
  Performance: [PERFORMANCE §5](PERFORMANCE.md#5-noeul-step-63); security: [SECURITY §9](SECURITY.md#9-noeul-step-63);
  the maintainer's physical acceptance script: [NOEUL_ACCEPTANCE](NOEUL_ACCEPTANCE.md).

**No fabricated completeness.** Most rows are *partial*: the core of the feature works and is tested,
and the row says exactly what is missing. *Implemented* means nothing the row describes is missing.
*Not intended* rows are design decisions (each names its reason, e.g. no keyboard hook, no OCR
service, no hot corners).

## 1. Counts

| Area | Rows | Implemented | Partial | Planned | Not intended |
| --- | --- | --- | --- | --- | --- |
| Starting a capture and selecting (`SNIP`) | 22 | 1 | 12 | 1 | 8 |
| Detection, magnifier, precision (`DET`) | 12 | 0 | 11 | 1 | 0 |
| Colour picker (`COLOR`) | 7 | 0 | 7 | 0 | 0 |
| History (`HIST`) | 6 | 5 | 1 | 0 | 0 |
| Output and clipboard writes (`OUT`) | 12 | 1 | 10 | 0 | 1 |
| Annotation (`ANN`) | 17 | 0 | 14 | 3 | 0 |
| Pins (`PIN`) | 30 | 4 | 21 | 3 | 2 |
| Paste (F3) (`PASTE`) | 14 | 0 | 12 | 0 | 2 |
| Displays and colour (`DISP`) | 6 | 0 | 5 | 0 | 1 |
| Settings and shortcuts (`SET`) | 22 | 3 | 12 | 1 | 6 |
| Command line (`CLI`) | 9 | 2 | 6 | 0 | 1 |
| Application (`APP`) | 7 | 0 | 3 | 1 | 3 |
| **All** | **164** | **16** | **114** | **10** | **24** |

## 2. The critical workflows

| Workflow | State | Evidence (automated; real windows off-screen, posted input) |
| --- | --- | --- |
| `F1` → drag a region → **double-click inside → floating pin** (the signature flow) | works; p95 17 ms (1080p) / 60 ms (4K) from the second press to the pin (budget 50 / 100) | `crates/noeul/tests/double_click.rs`, `crates/noeul/src/app.rs` the_double_click_pins_exactly_the_selection_with_no_clipboard_or_file, `cargo xtask bench-noeul` |
| `F3` pins the clipboard (image, files, colour, HTML, text) | works | `crates/noeul/tests/paste.rs`, `crates/noeul/src/app/paste_tests.rs` |
| Smart selection: windows and UI elements, magnifier, pixel keys | works (partial rows: parent/child stepping, key swap) | `crates/noeul/tests/smart_selection.rs` |
| Colour picker (`C`, `Shift+C`) | works | `crates/noeul/tests/color_picker.rs` |
| Annotation and redaction on the overlay and on pins | works (missing: Pro shapes) | `crates/noeul/tests/annotation.rs`, `crates/noeul/src/app.rs` a_solid_redaction_is_shown_copied_and_saved_without_a_pixel_of_the_original |
| Pin transforms: zoom, rotate, flip, opacity, thumbnail, click-through with a way back | works | `crates/noeul/tests/pin_transform.rs` |
| History and recovery (in memory; opt-in disk history and pin backup) | works | `crates/noeul/tests/history_backup.rs`, `crates/noeul/src/app/keeping_tests.rs` |
| Groups | works (missing: group manager window) | `crates/noeul/tests/pin_transform.rs` |
| Monitors and DPI (mixed DPI, monitors removed, pins brought back) | works | `crates/noeul/tests/displays.rs` |
| Customisation: every shortcut, gestures, settings window | works (missing: other settings sections) | `crates/noeul/tests/settings_window.rs`, `crates/noeul/tests/instance.rs` |
| Install through Kim, CLI, agents (MCP), interop | works | `e2e/tests/step62_noeul_agents_kim.rs`, `crates/noeul/tests/mcp_adapter.rs` |

**Not verified by the agents** (they never use the maintainer's screen, mouse or keyboard): the same
workflows with a physical mouse and keyboard on visible windows, SmartScreen with a signed build,
Windows 11, real multi-monitor hardware. The maintainer's script: [NOEUL_ACCEPTANCE](NOEUL_ACCEPTANCE.md).

## 3. Remaining planned rows (tracked)

Each is tracked in [#99](https://github.com/madebymouv/kim/issues/99) (except `APP-03`); *partial* rows name
their gap in the matrix. #99 moved `SNIP-11`, `CLI-02`, `HIST-06`, `OUT-06`, `OUT-07`, `OUT-12` and
`SET-11`, then `PIN-21`, `PIN-22`, `PIN-27`, `SET-06`, `DET-09` and `SNIP-10` to *partial*, and
`SNIP-12` (typed size and fixed ratio), `DET-03` (the wheel steps to a parent element) and `PIN-08`
(free-size thumbnail) to *supported*.

- `ANN-12` (partial since #99: the double arrow) Numbered markers, rounded rectangles, dashes, styled arrows, filled or outlined text, magnifier shape (Pro)
- `ANN-13` (partial since #99: per-tool colours) Rotatable shapes; several pen sizes; shareable palette (Pro)
- `ANN-14` Right-click marks the detected element in rectangle, ellipse or fill modes
- `PIN-15` Group manager window (Pro)
- `PIN-28` Images on virtual desktops (Pro)
- `SET-21` Interface translations
- `APP-03` Windows, macOS and Linux editions (Linux: tracked by [#69](https://github.com/madebymouv/kim/issues/69), done on the Linux server)
