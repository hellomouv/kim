# ADR 0089: Double-click kim.exe, a local mirror, Open buttons and program icons

- Status: accepted
- Date: 2026-11-14
- Issue: maintainer request 2026-11-14 ("root 경로에 있는 kim.exe를 누르면 모든 기능을 사용할 수
  있게 … kim.exe에는 kim 아이콘을 반영"); extends ADR 0013 (installer), ADR 0014 (Kim Manager),
  ADR 0072 (Kim-Setup) and the catalog transport rules (CATALOG_FORMAT §5)

## Context

The maintainer wants the `kim.exe` at the repository root to be a program a user can actually
use: double-click it, get Kim, and from there install and open Subak, Noeul and Ddalgak. Four
things stood in the way:

1. `kim.exe` is the headless CLI (`KIM-FR-012`). A double-click showed a console with the help
   text that closed at once.
2. No release of any tool is published, so Kim Manager had nothing to install (every tool
   showed as *planned*).
3. Kim Manager could install and uninstall, but not start an installed tool.
4. No program carried an icon: Explorer, the taskbar and Task Manager showed the generic one,
   although `assets/icon/kim.ico` (the laver sheet) and `assets/subak/subak.ico` exist.

## Decision

1. **A double-click on `kim.exe` opens the Kim window.** It is not a CLI path: `kim.exe` hands
   over only when there are no arguments, it is the only process on its console, the console has
   a window, standard input and output are that console, and `KIM_NO_INPUT` is not set
   (`kim::start::is_double_click`). A terminal shares its console with the shell; a script or an
   agent passes arguments, has no console or a hidden one (`CREATE_NO_WINDOW` gives no console
   window), or uses pipes, so none of them can trigger it, and `KIM_NO_INPUT=1` turns it off.
   `kim.exe` then starts `kim-manager.exe` (next to it, else Kim's installed version), lets go
   of the console so it closes, and ends. If no window program is found, the console shows the
   help and a note and stays open until Enter. `KIM-FR-012` is unchanged: every CLI start is
   headless (`crates/kim/tests/cli_headless.rs` still runs `kim` without arguments and sees no
   process started).
2. **A local mirror.** `cargo xtask local-mirror --key <file>` packages kim, subak, noeul and
   ddalgak from this checkout (the deterministic zips of `cargo xtask package`), copies them to
   `kim-local\` next to the root `kim.exe` and writes `kim-local\catalog.json`: the bundled
   catalog plus one release per tool whose package URL is `file:///…` into that folder,
   `catalog_version` one above the bundled one, signed with the given catalog key. A
   double-click on `kim.exe` passes it to the window (`kim-manager --catalog file://…`).
   - A catalog may name a `file:///` package only when the catalog itself came from a
     `file://` source; `fetch_verified` refuses such a catalog from any other source, so a
     published catalog cannot point at files on a user's disk. The bundled catalog never does
     (tested).
   - Kim Manager uses a mirror **for the session only** (`store::mirror`, `Source::Mirror`):
     verified like a refresh (signature, content, not older than the trusted catalog) and never
     written to the cache, so it never blocks a later refresh from the publisher. *Refresh* reads
     the mirror again from disk. Installing from it needs no network, so offline mode does not
     block it.
   - Installing stays explicit and transactional (ADR 0013): the button, size and SHA-256 from
     the signed catalog, the self-check, `current.json`.
3. **Open.** For an installed tool the details panel offers to start its window or
   notification-area program from the installed version's folder, only when the button is
   pressed: Subak → `subak-gui.exe`, Noeul → `noeulw.exe` (starts the resident instance),
   Ddalgak → `ddalgak-gui.exe` and `ddalgak-editor.exe`. Kim Manager never stops them.
4. **Program icons and version information.** A new build helper, `kim-winres`, writes a COFF
   object with a `.rsrc` section (`RT_ICON`/`RT_GROUP_ICON` from the `.ico`, `RT_VERSION` with
   the description, product and version) and links it into a package's binaries from its
   `build.rs`. It needs neither `rc.exe` nor `windres`, so the GNU toolchain used here and the
   MSVC one of the release workflow build the same files. `kim.exe`, `kim-manager.exe` and
   `kim-setup.exe` carry the Kim icon; `subak.exe` and `subak-gui.exe` the watermelon. The Kim
   Manager window sets the Kim icon on its title bar and taskbar button; the Subak and Kim-Setup
   windows use their program's icon. Noeul and Ddalgak have no icon asset yet and are unchanged.

## Consequences

- The root of a checkout becomes a usable Kim: `kim.exe`, `kim-manager.exe` and `kim-local\`
  (all ignored by Git) after `cargo build --release -p kim -p kim-manager` and
  `cargo xtask local-mirror`.
- A mirror's release notes say the packages were built on this PC, from which commit, and that
  they are not a published release. Its releases are on the stable channel so they show without
  switching channels.
- Each `local-mirror` run uses the bundled version + 1; a mirror with the same version as a
  cached publisher catalog but different content is refused as a rollback (that would need a
  new bundled catalog first).
- **Not tested** on the real desktop (the maintainer asked on 2026-11-14 not to use the mouse,
  keyboard or screen): the double-click itself, the window and taskbar icons as seen on screen,
  and the tools started by Open. Tested instead: the decision function and a double-click
  simulated without the desktop (a hidden new console, job accounting: exactly one program
  started), the Kim Manager install-and-Open flow headlessly (egui_kittest), the icons read
  back from the built programs (PE resources and Windows' `ExtractIconEx`), and a sandboxed
  install of all three tools from the generated mirror with the release `kim.exe`.
- Manual check: double-click `kim.exe` at the repository root → the Kim window opens with the
  laver icon (title bar, taskbar) → select Subak → *Install 0.0.1* → *Open Subak*; the same for
  Noeul (*Start Noeul*) and Ddalgak (*Start Ddalgak*, *Open rule editor*).
