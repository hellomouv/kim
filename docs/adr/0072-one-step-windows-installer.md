# ADR 0072 — One-step Windows installation: `Kim-Setup.exe`

- **Status:** Accepted for Windows (Step 43, 2026-11-12). The Linux half of Step 43 (`.deb`,
  `.rpm`, a single-command path, headless install) is **not done**: Linux work waits for the Linux
  server phase (AGENTS.md §3).
- **Requirements:** `KIM-FR-060` (new), `KIM-SEC-001`, `KIM-SEC-003`, `KIM-SEC-005`, `KIM-FR-034`
- **Builds on:** [ADR 0013](0013-kim-manager-architecture.md), [RELEASE_POLICY](../RELEASE_POLICY.md),
  [RELEASING](../RELEASING.md)

## Decision

1. **One file, one action.** `Kim-Setup-<version>-x86_64.exe` is the `kim-setup` program with
   the Kim package (`kim-<v>-windows-x86_64.zip`: `kim.exe`, `kim-manager.exe`, licenses,
   notices, `capabilities.json`) appended, followed by a footer (`KIMSETUP`, length, SHA-256).
   Double-click → one small progress window → Kim Manager opens. No wizard pages, no options to
   choose, no network access, and no administrator prompt: it installs per user and never asks
   for elevation; Windows' installer-detection heuristic (which elevates programs named like
   "setup") applies only to 32-bit programs, and this one is 64-bit — the tests start
   `Kim-Setup-….exe` unelevated, which would fail with `ERROR_ELEVATION_REQUIRED` otherwise. An
   explicit `asInvoker` manifest is added once the release build embeds resources (MSVC).
2. **It installs only Kim.** Into Kim's own layout (`%LOCALAPPDATA%\Programs\mouv\kim\versions\<v>`,
   `current.json` — the record `kim` writes), so `kim` and Kim Manager see an ordinary install.
   Subak and every other tool stay absent until the user picks one in Kim Manager or runs
   `kim install`. No service, no scheduled task, no autostart, no update check of its own.
3. **Integrity before execution.** The appended package's SHA-256 is checked before anything is
   unpacked (a damaged download is refused, nothing changed); zip entries are confined to the
   staging folder; the unpacked `kim.exe --version` must succeed before activation. The footer is
   found before an Authenticode certificate table, so signing the finished installer keeps it
   working. **Provenance comes from the Authenticode signature of `Kim-Setup.exe` and the
   download page;** the internal hash only proves the file is whole. Unsigned builds say so.
4. **Transactional.** Staging, then the version folder, then an atomic `current.json` replace
   (the commit point); a failure before it leaves the previous state exactly as it was.
5. **Existing versions.** The same version → refused (`already_installed`), or `--repair`
   (double-clicking asks "Repair?"); an older one installed → upgrade, keeping it for
   `kim rollback`; a newer one installed → refused (`newer_installed`) unless `--allow-downgrade`.
   Running Kim programs are never stopped: the new version goes into its own folder.
6. **Integration:** a Start Menu shortcut "Kim Manager" (`IShellLinkW`), a per-user Apps &
   Features entry (`HKCU\…\Uninstall\MouvKim`: version, size, icon, uninstall and quiet uninstall
   commands), a copy of the installer in Kim's folder for uninstall and repair.
7. **Uninstall** (`Apps & Features`, or `kim-setup.exe --uninstall`): asks first; refused while a
   Kim program runs (`tool_running`, never stopped); removes Kim's programs, shortcut and entry;
   keeps `%APPDATA%\mouv\kim` unless `--purge-data`; leaves the tools installed with Kim alone.
   The running uninstaller moves itself to `%TEMP%` so the folder can go.
8. **Architecture.** x64 only, asked of the OS (`IsWow64Process2`), so the x64 installer also
   refuses on ARM64 Windows (`incompatible`) instead of installing an untested build.
9. **Headless:** `--quiet --json` (no window, no launch; one JSON document), `--dry-run`,
   `--yes`; debug builds accept `KIM_SETUP_TEST_REGISTRY_KEY` (below `HKCU\Software\mouv-tests\`)
   so tests never write the real Apps & Features entry.

## Consequences

- One installer per Kim version, built by `cargo xtask package kim` (and the release workflow,
  which signs it when a certificate is configured).
- A user who wants only the CLI can still unzip the package; the installer is the recommended
  route, not the only one.

## Testing

`crates/kim-setup/tests/setup.rs` (the real installer program, test payloads, a sandbox and a
test registry key): dry run, fresh install of only Kim (files, record, shortcut, entry), refused
second run, repair, upgrade keeping the previous version, refused downgrade, a damaged installer
refused with nothing changed, uninstall refused while a Kim program runs, confirmation required
without a window, uninstall keeping the settings. Unit tests: payload footer and checksum, the
certificate-table boundary, version order and the plan. `e2e/tests/step43_setup.rs`: the release
Kim programs installed by the installer, the installed `kim` listing no other tool, uninstall.
**Not tested:** the double-click window path and the launch of Kim Manager (they open windows;
the maintainer checks them), SmartScreen with a signed and an unsigned installer, Windows 11,
ARM64 refusal on real ARM64 hardware.
