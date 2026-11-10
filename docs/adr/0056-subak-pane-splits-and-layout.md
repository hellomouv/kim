# ADR 0056 — Subak pane splits and layout: a Host-owned split tree in cells, the latest client's size, read-only views for the other panes

Status: **Accepted** (Step 22; the visible window and a real mouse pending the maintainer,
[SUBAK_GUI §14](../SUBAK_GUI.md)) · Date: 2026-11-11 · Builds on
[ADR 0054](0054-subak-internal-windows-and-panes.md) (Host-owned topology and selection; `pane.create`
before `pane.split`), [ADR 0004](0004-pty-and-process-ownership.md) (one pseudo console per pane),
[ADR 0030](0030-host-terminal-model-scrollback-and-replay.md) (snapshot + live stream) · Details:
[SESSION_MODEL §3](../SESSION_MODEL.md), [IPC_PROTOCOL §4.5](../contracts/IPC_PROTOCOL.md),
[SUBAK_CLI §3.15](../SUBAK_CLI.md), [TMUX_COMPATIBILITY](../TMUX_COMPATIBILITY.md)

## Context

Step 21 gave each Terminal windows and panes, but a window showed one pane at a time and every pane
had its own size. Step 22 asks for true horizontal and vertical splits, nested and equal at first;
resize, directional focus, swap, zoom and close that keep unaffected processes and their sizes;
the right rows and columns for every pseudo console after any change; a layout that does not live in
the GUI and survives detach and reconnect; no full-rate drawing of hidden panes; a keyboard path; and
the same through the CLI and IPC.

## Decision

1. **A split tree per window, owned by the Host, in cells.** A node is a pane or a split with a
   direction and children; each child has its extent along the split in cells; siblings are one cell
   apart (the border, as tmux draws it). Terms are tmux's: **horizontal** puts children side by side
   (`split-window -h`), **vertical** stacks them (`-v`, the default). A split in the same direction as
   its parent adds a sibling; otherwise it nests. The window's panes are listed in layout order and
   indexed so. The contract's `Layout` shape (STATE_SCHEMA) is used as reserved, with `size` as the
   child's share (0–1) plus `cells`.
2. **Sizes.** A new pane gets half of the pane it splits (or `cells` / `percent`); an odd cell stays
   with the split pane. Every pane keeps at least **2 × 2** cells: a split that cannot give both
   panes that much is refused (`invalid_state`, `need`/`have` in `details`) and nothing changes. A
   window smaller than its layout's minimum is laid out at the minimum (clients clip it).
3. **The window's size is the latest interactive client's** (tmux `window-size latest`): a
   `pane.attach` with a size and `pane.resize` set the window's size, and every pane whose share
   changed is resized (model first, then the pseudo console, outside the lock). A read-only viewer
   never resizes. The reply of `pane.resize` is the pane's share plus `window_size`. A new window
   starts at the size of the Terminal's active window unless a size is given.
4. **Methods.** `pane.split` (target pane, or a window's or Terminal's active pane; `direction`,
   `before`, `cells` or `percent`, and the program parameters of `pane.create`), `pane.adjust`
   (tmux `resize-pane -L/-R/-U/-D`: move the border on that side, else the opposite border; never
   below the minimum; answers `moved`), `pane.swap` (two panes of one window, or with the
   `next`/`previous` one), `pane.zoom` (`zoom` true, false or omitted to toggle), and a `direction`
   for `pane.select` (`left/right/up/down` by geometry, the most-overlapping neighbour, wrapping as
   tmux does; `next/previous` by index). `pane.create` is now a vertical split of the window's active
   pane. All take `dry_run`. Feature flag `terminal.layout`.
5. **Zoom** gives the zoomed pane the whole window; the others keep their places and sizes, unseen
   (`visible: false`). Selecting another pane, splitting or moving a border unzooms (tmux).
6. **`window.layout`** announces a window's layout, size, zoomed pane and pane places after every
   change of a window with more than one pane (a window of one pane is just that pane's size, as
   before). Window objects carry `layout`, `size`, `zoomed`; pane objects `rect` and `visible`.
7. **Closing a pane** gives its space to its next sibling (else the previous one); a split left with
   one child is replaced by it, and same-direction splits merge.
8. **The window draws every visible pane** of a split window at its place: the active pane is its
   interactive attachment (keys, selection, search, IME); every other visible pane is a read-only
   attachment drawn by its own renderer. Panes nobody sees (another tab, a zoom) are not attached,
   so hidden panes cost nothing. Borders are drawn in the gap; the active pane's in the accent colour.
   A click on another pane selects it. Keys (context `terminal`, customizable): `Alt+Shift+Plus` /
   `Alt+Shift+Minus` split right / down, `Alt+Arrow` focus, `Alt+Shift+Arrow` resize,
   `Ctrl+Shift+Z` zoom, `Ctrl+Shift+W` close the pane (an idle shell; a busy pane is refused with a
   notice) — the Windows Terminal defaults where it has them.

## Consequences

- One layout for every client: two windows on one Terminal show the same split; the last one to
  resize sets the size. Persistence across a Host restart is Step 29.
- `layout` in IPC and the CLI changed from "always one pane" to the tree; clients that read only
  `type`/`pane` of a one-pane window see the same as before.
- Not yet: mouse drag of borders, `select-layout` presets and `rotate-window` (Step 25),
  `join-pane`/`break-pane`/`move-pane` across windows (Step 25), prefix keys (Step 23).

## Alternatives

- **Layout in the GUI.** Rejected: a CLI or a second client would see different sizes, and the
  layout would vanish with the window.
- **Every pane keeps its own size (no window size).** Rejected: the panes of one window must share
  one area for a split to mean anything.
- **One renderer for all panes.** Rejected for now: the renderer keeps per-row damage for one view;
  a renderer per visible pane keeps drawing incremental.
