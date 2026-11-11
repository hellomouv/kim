# Releasing (Windows)

- **Status:** current (Step 40, 2026-11-12). Policy and consent rules: [RELEASE_POLICY](RELEASE_POLICY.md);
  what must be true before a public release: [RELEASE_READINESS](RELEASE_READINESS.md).

## 1. Build the distributable

```powershell
cargo xtask package subak     # or: kim
```

builds the release binaries and writes to `target/dist/`:

- `<tool>-<version>-windows-x86_64.zip` — the programs, `capabilities.json` (generated from
  `<tool> capabilities --json`, so Kim reads it without starting the tool), `LICENSE-MIT`,
  `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.txt` (every crate linked in, with its license, from
  `cargo tree`) and `README.txt` (version, commit, compiler, target).
- `<tool>-<version>-release.json` — size and SHA-256 of the zip and of every file in it, the
  build (commit, `rustc -V`, target), `"signed"`, and the `release` entry for
  `catalog/catalog.json`.

For `kim`, it also writes **`Kim-Setup-<version>-x86_64.exe`**: the `kim-setup` program with
the zip appended and a footer (`KIMSETUP`, length, SHA-256) the installer checks before
unpacking ([ADR 0072](adr/0072-one-step-windows-installer.md)). Signing the finished installer
keeps it working (the footer is found before the certificate table), and changes its SHA-256 —
publish the signed file's hash.

**Reproducibility.** The zip is deterministic: sorted entries, fixed timestamps (1980-01-01),
fixed permissions; packing the same binaries twice gives the same bytes (`package` checks it on
every run; Step 40: `subak` packed twice from the same build gave the same SHA-256). Bit-identical
*binaries* across separate builds are not verified yet (they depend on the toolchain and paths);
the manifest records what produced them.

## 2. The release workflow

`.github/workflows/release.yml` (manual `workflow_dispatch` only; never on push): installs the
pinned toolchain, runs `cargo xtask ci`, runs `cargo xtask package <tool>`, and uploads the zip and
the manifest as a workflow **artifact** — not a release. Publishing stays a manual step.

**Signing** happens only when the repository has a legitimate certificate as the secrets
`WINDOWS_SIGNING_PFX_BASE64` and `WINDOWS_SIGNING_PFX_PASSWORD`: signtool signs every program
(SHA-256, RFC 3161 timestamp), then `package --signed` re-packs and **refuses** unless every
program has a valid Authenticode signature (`Get-AuthenticodeSignature` = `Valid`). Without the
secrets nothing is signed and the manifest says `"signed": false`. The workflow has not been run
yet (Step 40: no certificate is configured).

## 3. Publish (maintainer)

1. Download the artifact; check the SHA-256 against the manifest; smoke-test it (§4).
2. Create the GitHub release `<tool>-v<version>` and upload the zip.
3. Add the manifest's `catalog_release` to `catalog/catalog.json` (set `published_at`), bump
   `catalog_version`, keep `expires_at` ≤ 6 months ahead.
4. `cargo xtask catalog-sign --key <release key file>` — the release key is created offline
   for the first real release (RELEASE_POLICY §4) — and copy the catalog to
   `docs/contracts/examples/kim-catalog-v1/bundled.json`; `cargo xtask ci`; push.

## 4. Smoke test of a release

Automated (`e2e/tests/step40_release.rs`, in the daily suite): the real Subak programs packaged
as above, a test-signed catalog over loopback, the debug `kim` — first start installs nothing,
explicit install, a Terminal from the installed Subak, uninstall refused while it runs, update
side by side without stopping it (the new CLI talks to the old Host), rollback and back, offline
mode refusing a refresh, uninstall keeping the user data.

Manual on a clean Windows user account (the maintainer, before publishing): `kim install`, start
Subak, reboot and `subak recover list`, `kim update`, `kim rollback`, `kim uninstall`, offline
mode — and the SmartScreen behaviour of the (un)signed programs.
