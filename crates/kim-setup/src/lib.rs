//! Step 43 (ADR 0072): the logic of `Kim-Setup.exe`, testable without a window.
//!
//! `Kim-Setup.exe` is this program with the Kim package (`kim-<v>-windows-x86_64.zip`, from
//! `cargo xtask package kim`) appended, followed by a footer: `KIMSETUP`, the payload length and
//! its SHA-256. It installs **only Kim** (`kim.exe`, `kim-manager.exe`), per user, without
//! administrator rights and without any network access, into Kim's own layout
//! (`%LOCALAPPDATA%\Programs\mouv\kim\versions\<v>`, `current.json`), so `kim` and Kim Manager
//! see a normal install. It never installs Subak or any other tool, never starts a background
//! service and never updates anything by itself.
#![cfg(windows)]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub mod sys;

/// The footer magic after the payload.
pub const MAGIC: &[u8; 8] = b"KIMSETUP";
/// Footer: magic, payload length (u64 LE), SHA-256.
pub const FOOTER_LEN: u64 = 8 + 8 + 32;
/// The programs the package must contain.
pub const PROGRAMS: [&str; 2] = ["kim.exe", "kim-manager.exe"];

/// A failure with a stable code (the `--json` envelope's `error.code`) and an exit code.
#[derive(Debug)]
pub struct SetupError {
    pub code: &'static str,
    pub exit: u8,
    pub message: String,
    pub hint: Option<String>,
}

impl SetupError {
    pub fn new(code: &'static str, exit: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            exit,
            message: message.into(),
            hint: None,
        }
    }
    #[must_use]
    pub fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
}

fn io(what: &str, e: &std::io::Error) -> SetupError {
    SetupError::new("io", 1, format!("{what}: {e}"))
}

/// Where the payload ends: before an Authenticode certificate table if the file is signed
/// (signing appends it after everything else), else at the end of the file.
pub fn payload_end(f: &mut std::fs::File) -> std::io::Result<u64> {
    let len = f.metadata()?.len();
    // Anything that is not a readable PE header means: no certificate table.
    Ok(cert_table_offset(f, len).unwrap_or(len))
}

fn cert_table_offset(f: &mut std::fs::File, len: u64) -> std::io::Result<u64> {
    let mut dos = [0u8; 64];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Ok(len);
    }
    let pe = u64::from(u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]));
    let mut hdr = [0u8; 24 + 2];
    f.seek(SeekFrom::Start(pe))?;
    f.read_exact(&mut hdr)?;
    if &hdr[..4] != b"PE\0\0" {
        return Ok(len);
    }
    let opt = pe + 24;
    let magic = u16::from_le_bytes([hdr[24], hdr[25]]);
    // The data directories start at 96 (PE32) or 112 (PE32+); entry 4 is the security table,
    // whose "virtual address" is a file offset.
    let dirs = opt + if magic == 0x20b { 112 } else { 96 };
    let mut sec = [0u8; 8];
    f.seek(SeekFrom::Start(dirs + 4 * 8))?;
    f.read_exact(&mut sec)?;
    let off = u64::from(u32::from_le_bytes([sec[0], sec[1], sec[2], sec[3]]));
    let size = u32::from_le_bytes([sec[4], sec[5], sec[6], sec[7]]);
    Ok(if size > 0 && off > 0 && off <= len {
        off
    } else {
        len
    })
}

/// The payload appended to `exe`, verified against its footer's SHA-256.
pub fn read_payload(exe: &Path) -> Result<Vec<u8>, SetupError> {
    let bad = |m: &str| {
        SetupError::new(
            "integrity_failed",
            4,
            format!("this installer is damaged: {m}"),
        )
        .hint("download Kim-Setup.exe again from the official release page")
    };
    let mut f = std::fs::File::open(exe).map_err(|e| io("cannot read the installer", &e))?;
    let end = payload_end(&mut f).map_err(|e| io("cannot read the installer", &e))?;
    if end < FOOTER_LEN {
        return Err(bad("no package inside"));
    }
    let mut footer = [0u8; FOOTER_LEN as usize];
    f.seek(SeekFrom::Start(end - FOOTER_LEN))
        .map_err(|e| io("seek", &e))?;
    f.read_exact(&mut footer).map_err(|e| io("read", &e))?;
    if &footer[..8] != MAGIC {
        return Err(bad("no package inside"));
    }
    let n = u64::from_le_bytes(footer[8..16].try_into().unwrap_or([0; 8]));
    if n == 0 || n > end - FOOTER_LEN || n > 512 * 1024 * 1024 {
        return Err(bad("the package length is wrong"));
    }
    let mut payload = vec![0u8; usize::try_from(n).unwrap_or(0)];
    f.seek(SeekFrom::Start(end - FOOTER_LEN - n))
        .map_err(|e| io("seek", &e))?;
    f.read_exact(&mut payload).map_err(|e| io("read", &e))?;
    if Sha256::digest(&payload).as_slice() != &footer[16..] {
        return Err(bad("the package does not match its checksum"));
    }
    Ok(payload)
}

/// `stub` with `payload` and its footer appended (what `cargo xtask package kim` writes).
pub fn assemble(stub: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(stub.len() + payload.len() + FOOTER_LEN as usize);
    v.extend_from_slice(stub);
    v.extend_from_slice(payload);
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    v.extend_from_slice(&Sha256::digest(payload));
    v
}

/// The version of the package: its `capabilities.json` (`tool` must be `kim`).
pub fn package_version(payload: &[u8]) -> Result<String, SetupError> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(payload)).map_err(|e| {
        SetupError::new(
            "package_invalid",
            4,
            format!("the package is not a zip: {e}"),
        )
    })?;
    let mut caps = String::new();
    z.by_name("capabilities.json")
        .map_err(|_| SetupError::new("package_invalid", 4, "the package has no capabilities.json"))?
        .read_to_string(&mut caps)
        .map_err(|e| io("capabilities.json", &e))?;
    let v: Value = serde_json::from_str(&caps)
        .map_err(|e| SetupError::new("package_invalid", 4, format!("capabilities.json: {e}")))?;
    if v["tool"] != "kim" {
        return Err(SetupError::new(
            "package_invalid",
            4,
            "the package is not Kim",
        ));
    }
    let version = v["version"].as_str().unwrap_or("").to_string();
    if parse_version(&version).is_none() {
        return Err(SetupError::new(
            "package_invalid",
            4,
            format!("`{version}` is not a version"),
        ));
    }
    for p in PROGRAMS {
        if z.by_name(p).is_err() {
            return Err(SetupError::new(
                "package_invalid",
                4,
                format!("the package has no {p}"),
            ));
        }
    }
    Ok(version)
}

/// `major.minor.patch` (a pre-release suffix sorts before the release).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, bool)> {
    let (core, pre) = v.split_once('-').map_or((v, false), |(c, _)| (c, true));
    let mut it = core.split('.').map(|x| x.parse::<u64>().ok());
    let r = (it.next()??, it.next()??, it.next()??, !pre);
    it.next().is_none().then_some(r)
}

/// Where things go.
#[derive(Debug, Clone)]
pub struct Layout {
    /// `%LOCALAPPDATA%\Programs\mouv\kim`.
    pub tool_dir: PathBuf,
    /// `%APPDATA%\mouv\kim` (settings; kept on uninstall unless purged).
    pub data_dir: PathBuf,
    /// The Start Menu shortcut.
    pub shortcut: PathBuf,
}

impl Layout {
    pub fn from_env() -> Result<Self, SetupError> {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| SetupError::new("io", 1, format!("%{k}% is not set")))
        };
        let local = var("LOCALAPPDATA")?;
        let roaming = var("APPDATA")?;
        Ok(Self {
            tool_dir: local.join("Programs").join("mouv").join("kim"),
            data_dir: roaming.join("mouv").join("kim"),
            shortcut: roaming
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Kim Manager.lnk"),
        })
    }

    pub fn record(&self) -> PathBuf {
        self.tool_dir.join("current.json")
    }
}

/// The installed version and its record, if any.
pub fn installed(l: &Layout) -> Option<(String, Value)> {
    let v: Value = serde_json::from_slice(&std::fs::read(l.record()).ok()?).ok()?;
    Some((v["version"].as_str()?.to_string(), v))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub repair: bool,
    pub allow_downgrade: bool,
    pub dry_run: bool,
}

/// What an install does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Install,
    Upgrade(String),
    Repair,
    Downgrade(String),
}

impl Action {
    pub fn word(&self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Upgrade(_) => "upgrade",
            Self::Repair => "repair",
            Self::Downgrade(_) => "downgrade",
        }
    }
}

/// Decides what to do, or refuses (`already_installed`, `newer_installed`).
pub fn plan(l: &Layout, version: &str, o: Options) -> Result<Action, SetupError> {
    let Some((have, _)) = installed(l) else {
        return Ok(Action::Install);
    };
    match parse_version(&have).cmp(&parse_version(version)) {
        std::cmp::Ordering::Less => Ok(Action::Upgrade(have)),
        std::cmp::Ordering::Equal if o.repair => Ok(Action::Repair),
        std::cmp::Ordering::Equal => Err(SetupError::new(
            "already_installed",
            7,
            format!("Kim {version} is already installed"),
        )
        .hint("run Kim-Setup.exe --repair to restore its files, or start Kim Manager from the Start menu")),
        std::cmp::Ordering::Greater if o.allow_downgrade => Ok(Action::Downgrade(have)),
        std::cmp::Ordering::Greater => Err(SetupError::new(
            "newer_installed",
            7,
            format!("a newer Kim ({have}) is installed than this installer's {version}"),
        )
        .hint("nothing was changed; use `kim rollback kim` or pass --allow-downgrade")),
    }
}

/// Unpacks `payload` into `dir`, refusing names that would leave it.
fn unpack(payload: &[u8], dir: &Path) -> Result<Vec<String>, SetupError> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(payload))
        .map_err(|e| SetupError::new("package_invalid", 4, e.to_string()))?;
    let mut names = Vec::new();
    for i in 0..z.len() {
        let mut e = z
            .by_index(i)
            .map_err(|e| SetupError::new("package_invalid", 4, e.to_string()))?;
        let Some(rel) = e.enclosed_name() else {
            return Err(SetupError::new(
                "package_invalid",
                4,
                format!("unsafe path in the package: {:?}", e.name()),
            ));
        };
        let out = dir.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| io("create", &e))?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| io("create", &e))?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| io(&out.display().to_string(), &e))?;
        std::io::copy(&mut e, &mut f).map_err(|e| io(&out.display().to_string(), &e))?;
        names.push(rel.display().to_string());
    }
    Ok(names)
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Progress reported to the window (0–100) with a short status.
pub type Progress<'a> = &'a mut dyn FnMut(u32, &str);

/// Installs (or upgrades, repairs) Kim from `payload`. Running Kim programs are never stopped:
/// the new version goes into its own folder; the previous version stays for `kim rollback`.
pub fn install(
    l: &Layout,
    payload: &[u8],
    setup_exe: &Path,
    o: Options,
    progress: Progress,
) -> Result<Value, SetupError> {
    progress(5, "Checking the package");
    let version = package_version(payload)?;
    if !sys::supported_machine() {
        return Err(SetupError::new(
            "incompatible",
            5,
            "this Kim is built for 64-bit Intel/AMD (x64) Windows; this PC is a different architecture",
        )
        .hint("Windows on ARM and 32-bit Windows are not supported yet"));
    }
    let action = plan(l, &version, o)?;
    let previous = installed(l).map(|(v, _)| v);
    let mut out = json!({
        "action": action.word(),
        "version": version,
        "previous": previous,
        "tool_dir": l.tool_dir.display().to_string(),
        "shortcut": l.shortcut.display().to_string(),
        "dry_run": o.dry_run,
    });
    if o.dry_run {
        return Ok(out);
    }
    // 1. Unpack into staging and check the program.
    progress(20, "Unpacking");
    let staging = l
        .tool_dir
        .join(".staging")
        .join(format!("setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| io("staging", &e))?;
    let cleanup = Cleanup(staging.clone());
    let files = unpack(payload, &staging)?;
    progress(50, "Checking Kim");
    sys::self_check(&staging.join("kim.exe"))?;
    // 2. Move into versions\<v>; a version folder already there (repair) is set aside first.
    progress(65, "Installing");
    let versions = l.tool_dir.join("versions");
    std::fs::create_dir_all(&versions).map_err(|e| io("versions", &e))?;
    let target = versions.join(&version);
    let mut aside = None;
    if target.exists() {
        let a = versions.join(format!("{version}.replaced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&a);
        std::fs::rename(&target, &a).map_err(|e| {
            SetupError::new(
                "tool_running",
                7,
                format!("cannot replace Kim {version} (in use?): {e}"),
            )
            .hint("close Kim Manager and retry; nothing was changed")
        })?;
        aside = Some(a);
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        if let Some(a) = &aside {
            let _ = std::fs::rename(a, &target);
        }
        return Err(io("cannot install the files", &e));
    }
    drop(cleanup);
    // 3. The commit point: current.json, replaced atomically (the same record `kim` writes).
    let record = json!({
        "schema": "kim.install", "schema_version": 1, "tool": "kim", "version": version,
        "entrypoint": format!("versions/{version}/kim.exe"), "installed_at": now_rfc3339(),
        "sha256": Sha256::digest(payload).iter().map(|b| format!("{b:02x}")).collect::<String>(), "source_url": "Kim-Setup.exe",
        "catalog_version": 0,
        "previous_version": previous.as_ref().filter(|p| **p != version),
    });
    let tmp = l.record().with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&record).unwrap_or_default();
    if let Err(e) = std::fs::write(&tmp, &body).and_then(|()| sys::replace(&tmp, &l.record())) {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&target);
        if let Some(a) = &aside {
            let _ = std::fs::rename(a, &target);
        }
        return Err(io("cannot record the install", &e));
    }
    if let Some(a) = aside {
        let _ = std::fs::remove_dir_all(a);
    }
    // 4. Integration: a copy of this installer for uninstall and repair, the Start Menu
    //    shortcut, the Apps & Features entry.
    progress(85, "Adding Kim Manager to the Start menu");
    let setup_copy = l.tool_dir.join("kim-setup.exe");
    if setup_exe != setup_copy {
        let _ = std::fs::copy(setup_exe, &setup_copy);
    }
    let manager = target.join("kim-manager.exe");
    sys::shortcut(
        &l.shortcut,
        &manager,
        "Kim Manager: install the Kim tools you choose",
    )
    .map_err(|e| io("cannot create the Start Menu shortcut", &e))?;
    sys::register(&version, &l.tool_dir, &manager, &setup_copy)
        .map_err(|e| io("cannot add Kim to Apps & Features", &e))?;
    // 5. Older versions not in use: keep the previous one for `kim rollback`.
    let mut removed = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&versions) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n == version || Some(&n) == previous.as_ref() {
                continue;
            }
            if kim::running::processes_under(&e.path()).is_empty()
                && std::fs::remove_dir_all(e.path()).is_ok()
            {
                removed.push(n);
            }
        }
    }
    progress(100, "Done");
    out["files"] = json!(files);
    out["manager"] = json!(manager.display().to_string());
    out["removed_versions"] = json!(removed);
    out["running"] = json!(
        kim::running::processes_under(&l.tool_dir)
            .into_iter()
            .filter(|p| p.pid != std::process::id())
            .map(|p| p.pid)
            .collect::<Vec<_>>()
    );
    Ok(out)
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Removes Kim's programs, shortcut and Apps & Features entry. Refused while a Kim program
/// runs (Kim never ends one). The settings (`%APPDATA%\mouv\kim`) stay unless `purge`.
pub fn uninstall(l: &Layout, purge: bool, dry_run: bool) -> Result<Value, SetupError> {
    let me = std::process::id();
    let running: Vec<u32> = kim::running::processes_under(&l.tool_dir)
        .into_iter()
        .map(|p| p.pid)
        .filter(|p| *p != me)
        .collect();
    let version = installed(l).map(|(v, _)| v);
    let out = json!({
        "version": version, "tool_dir": l.tool_dir.display().to_string(),
        "data_dir": l.data_dir.display().to_string(), "purge_data": purge, "dry_run": dry_run,
        "running": running,
    });
    if version.is_none() && !l.tool_dir.exists() {
        return Err(SetupError::new("not_installed", 3, "Kim is not installed"));
    }
    if !running.is_empty() {
        return Err(SetupError::new(
            "tool_running",
            7,
            format!("Kim is running (PID {running:?}); it is never stopped by the uninstaller"),
        )
        .hint("close Kim Manager and any kim.exe, then retry; nothing was changed"));
    }
    if dry_run {
        return Ok(out);
    }
    let _ = std::fs::remove_file(&l.shortcut);
    let _ = sys::unregister();
    sys::move_self_out(&l.tool_dir);
    // The running uninstaller may be the copy inside the folder: everything else goes now,
    // that file is deleted when it exits (sys::delete_after_exit).
    let mut leftover = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&l.tool_dir) {
        for e in rd.flatten() {
            let p = e.path();
            let r = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            if r.is_err() {
                leftover.push(p);
            }
        }
    }
    if leftover.is_empty() {
        let _ = std::fs::remove_dir(&l.tool_dir);
    }
    if purge {
        let _ = std::fs::remove_dir_all(&l.data_dir);
    }
    let mut out = out;
    out["removed"] = json!(leftover.is_empty());
    out["left_behind"] = json!(
        leftover
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
    out["data_kept"] = json!(!purge && l.data_dir.exists());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(version: &str, tool: &str) -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, b) in [
            ("kim.exe", b"x".to_vec()),
            ("kim-manager.exe", b"y".to_vec()),
            (
                "capabilities.json",
                format!("{{\"tool\":\"{tool}\",\"version\":\"{version}\"}}").into_bytes(),
            ),
        ] {
            z.start_file(n, o).unwrap();
            z.write_all(&b).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn the_payload_is_found_verified_and_refused_when_damaged() {
        let dir = std::env::temp_dir().join(format!("kim-setup-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stub = b"MZ-not-a-real-pe".repeat(8);
        let payload = pkg("1.2.3", "kim");
        let exe = dir.join("setup.exe");
        std::fs::write(&exe, assemble(&stub, &payload)).unwrap();
        assert_eq!(read_payload(&exe).unwrap(), payload);
        assert_eq!(package_version(&payload).unwrap(), "1.2.3");
        let mut bytes = std::fs::read(&exe).unwrap();
        let mid = stub.len() + payload.len() / 2;
        bytes[mid] ^= 0xff;
        std::fs::write(&exe, &bytes).unwrap();
        assert_eq!(read_payload(&exe).unwrap_err().code, "integrity_failed");
        std::fs::write(&exe, &stub).unwrap();
        assert_eq!(read_payload(&exe).unwrap_err().code, "integrity_failed");
        assert_eq!(
            package_version(&pkg("1.0.0", "subak")).unwrap_err().code,
            "package_invalid"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_order_and_the_plan_follows_what_is_installed() {
        assert!(parse_version("0.0.1") < parse_version("0.0.2"));
        assert!(parse_version("1.0.0-rc.1") < parse_version("1.0.0"));
        assert!(parse_version("1.0").is_none() && parse_version("x").is_none());
        let dir = std::env::temp_dir().join(format!("kim-setup-plan-{}", std::process::id()));
        let l = Layout {
            tool_dir: dir.join("kim"),
            data_dir: dir.join("data"),
            shortcut: dir.join("k.lnk"),
        };
        std::fs::create_dir_all(&l.tool_dir).unwrap();
        assert_eq!(
            plan(&l, "0.0.2", Options::default()).unwrap(),
            Action::Install
        );
        std::fs::write(l.record(), r#"{"version":"0.0.2"}"#).unwrap();
        assert_eq!(
            plan(&l, "0.0.2", Options::default()).unwrap_err().code,
            "already_installed"
        );
        assert_eq!(
            plan(
                &l,
                "0.0.2",
                Options {
                    repair: true,
                    ..Options::default()
                }
            )
            .unwrap(),
            Action::Repair
        );
        assert_eq!(
            plan(&l, "0.0.3", Options::default()).unwrap(),
            Action::Upgrade("0.0.2".into())
        );
        assert_eq!(
            plan(&l, "0.0.1", Options::default()).unwrap_err().code,
            "newer_installed"
        );
        assert_eq!(
            plan(
                &l,
                "0.0.1",
                Options {
                    allow_downgrade: true,
                    ..Options::default()
                }
            )
            .unwrap(),
            Action::Downgrade("0.0.2".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(now_rfc3339().ends_with('Z') && now_rfc3339().len() == 20);
    }
}
