# Install and get started (Windows)

- **Status:** current (Step 40, 2026-11-12). **Pre-release:** no stable release is published
  yet; this guide describes how releases are installed once they are (and how to use a package
  built from this repository). Release notes: [RELEASE_NOTES](RELEASE_NOTES.md); readiness and
  blockers: [RELEASE_READINESS](RELEASE_READINESS.md).

## Install Kim — one step

**Windows:** download **`Kim-Setup.exe`** and double-click it. A small window shows the progress,
then Kim Manager opens. No administrator rights, no other downloads; only Kim is installed — pick
any tool you want in Kim Manager afterwards. If Windows SmartScreen warns about an unsigned
preview build, choose "More info" → "Run anyway" only if you downloaded it from the official
release page (the signed release will not warn).

**Linux:** not available yet (Steps 41–43).

If something goes wrong: run `Kim-Setup.exe` again (it offers a repair), or unzip
`kim-<version>-windows-x86_64.zip` and run `kim-manager.exe` from there. Remove Kim in Settings →
Apps (your settings are kept).

## Use Kim from a build of this repository (no release needed)

[ADR 0089](adr/0089-double-click-kim-local-mirror-and-program-icons.md): build the programs and a
local mirror of the tools, then double-click `kim.exe`:

```sh
cargo build --release -p kim -p kim-manager
copy target\release\kim.exe .  &  copy target\release\kim-manager.exe .
cargo xtask local-mirror --key <catalog key file>   # writes kim-local\ (packages + signed catalog)
```

A double-click on `kim.exe` (no arguments, its own console) opens the Kim window with the
packages in `kim-local\`: select Subak, Noeul or Ddalgak, press **Install**, then **Open Subak**,
**Start Noeul**, **Start Ddalgak** or **Open rule editor**. Nothing is downloaded; the mirror is
used for that session only and never replaces Kim's catalog cache. From a terminal, `kim` stays
the headless CLI (`kim --help`).

## 1. Supported systems

| System | State |
| --- | --- |
| Windows 10 22H2 (build 19045), x64 | **Tested** (all automated tests, the daily e2e suite) |
| Windows 11, x64 | Expected to work (same APIs); **not tested** on this project's machines |
| Windows on ARM64 | Not built (Tabby matrix `APP-13`, blocked) |
| Linux | Not built yet (Steps 41–42) |

No administrator rights are needed or ever requested. Everything installs per user under
`%LOCALAPPDATA%\Programs\mouv` and keeps its settings under `%APPDATA%\mouv`.

## 2. Kim and the tools are independent

Kim (`kim.exe`, and the Kim Manager window) is a catalog and an explicit installer. It installs
nothing by itself — not on first start, not on a schedule, not as part of another install — and
it is not needed at run time: Subak, Noeul and Ddalgak run without Kim, and Kim can be removed
without touching them.

| Package | Contents |
| --- | --- |
| `kim-<version>-windows-x86_64.zip` | `kim.exe` (CLI), `kim-manager.exe` (window), licenses, notices, `capabilities.json` |
| `subak-<version>-windows-x86_64.zip` | `subak.exe` (CLI and Host), `subak-gui.exe` (window), licenses, notices, `capabilities.json` |
| `noeul-<version>-windows-x86_64.zip` | `noeul.exe` (the resident screenshot-and-pin app, its CLI, the optional MCP adapter and the Kim interop provider), licenses, notices, `capabilities.json` |

Every package is a zip with a SHA-256 in the signed catalog; Kim checks size, hash and the
catalog's Ed25519 signature before activating anything. Authenticode signatures of the programs
are present only when a release was signed with a real certificate (the release manifest says
`"signed": true`); unsigned programs may show a SmartScreen warning.

## 3. Install

Kim itself: `Kim-Setup.exe` (above). Tools, with Kim (recommended; integrity-checked,
reversible):

```powershell
kim catalog refresh          # explicit: fetch the signed catalog (or use the one Kim ships with)
kim catalog show subak       # what would be installed, its permissions and dependencies
kim install subak --dry-run  # the plan: nothing downloaded
kim install subak
```

Without Kim: unpack the zip anywhere and run `subak.exe` (nothing is registered; delete the
folder to remove it).

## 4. First steps with Subak

```powershell
subak new work               # a Terminal in a detached per-user Host (starts on demand)
subak attach work            # this console becomes its window; Ctrl+B d detaches
subak-gui                    # the window: tabs, splits, the floating manager
subak ls                     # Terminals keep running when every window is closed
```

Guides: the CLI ([SUBAK_CLI](SUBAK_CLI.md)), the window ([SUBAK_GUI](SUBAK_GUI.md)), headless
use and JSON ([CLI_CONTRACT](CLI_CONTRACT.md)), SSH and SFTP ([SUBAK_CLI §3.22–3.24](SUBAK_CLI.md)),
agents and MCP ([MCP_GUIDE](MCP_GUIDE.md)), tool-to-tool use ([INTEROP](INTEROP.md)),
recovery after a crash or reboot ([RECOVERY](RECOVERY.md)), tmux compatibility
([TMUX_COMPATIBILITY](TMUX_COMPATIBILITY.md)), what Tabby has that Subak does not yet
([TABBY_PARITY_REPORT](TABBY_PARITY_REPORT.md)).

## 5. Update, roll back, uninstall

```powershell
kim update check             # shows available updates; never downloads
kim update subak             # explicit; the previous version stays on disk
kim rollback subak           # the previous version active again (nothing downloaded)
kim uninstall subak          # asks first (or --yes); your settings are kept
kim uninstall subak --purge-data   # also deletes %APPDATA%\mouv\subak after a second confirmation
```

**Running work is never stopped.** An update or rollback while Subak runs installs side by side:
the running Host keeps its own files and Terminals, the new CLI talks to it (protocol versions
are negotiated; an incompatible pair is reported, never forced), and the new version is used
the next time the Host starts. An uninstall while Subak runs is refused (`tool_running`) until
you end it yourself — Kim never ends a program.

## 6. Offline and privacy

`kim settings set offline_mode true` refuses every network operation (`offline_mode`). No tool
sends telemetry, needs an account, or opens a network listener; Subak reaches the network only
for the SSH, SFTP and Telnet connections you start ([SECURITY](SECURITY.md)).

## 7. Known limitations

The current list is [RELEASE_READINESS §3](RELEASE_READINESS.md#3-blockers-and-known-limitations):
among others, unsigned binaries until a certificate exists, the power-transition recovery matrix
(#31) and several visible-window checks still awaiting the maintainer, one extra handle per pane
on Windows 10's inbox ConPTY, and the tmux, Tabby and Claude Code features listed as unsupported
or planned in their matrices.

Screenshots are not included yet: the development agents work headless (AGENTS.md §5) and never
open windows on the maintainer's screen; the maintainer adds them with the visible-window checks.
