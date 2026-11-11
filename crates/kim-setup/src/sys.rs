//! The Windows parts of the installer (Step 43, ADR 0072).
#![allow(unsafe_code)]

use crate::SetupError;
use std::path::Path;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether this PC is x64 (the only architecture Kim is built for), asked of the OS itself so
/// an x64 program emulated on ARM64 Windows also refuses.
pub fn supported_machine() -> bool {
    use windows_sys::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    let (mut process, mut native) = (0u16, 0u16);
    // SAFETY: the pseudo handle of this process; two writable u16s.
    let ok =
        unsafe { IsWow64Process2(GetCurrentProcess(), &raw mut process, &raw mut native) } != 0;
    ok && native == IMAGE_FILE_MACHINE_AMD64
}

/// `<kim.exe> --version` must exit 0 within 15 s, without a window.
pub fn self_check(exe: &Path) -> Result<(), SetupError> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| {
            SetupError::new(
                "self_check_failed",
                4,
                format!("kim.exe does not start: {e}"),
            )
        })?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match c.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => {
                return Err(SetupError::new(
                    "self_check_failed",
                    4,
                    format!("kim.exe --version failed ({s})"),
                ));
            }
            Ok(None) if std::time::Instant::now() > end => {
                let _ = c.kill();
                return Err(SetupError::new(
                    "self_check_failed",
                    4,
                    "kim.exe --version did not finish",
                ));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => return Err(SetupError::new("self_check_failed", 4, e.to_string())),
        }
    }
}

/// Moves `from` over `to` (write-through), the atomic commit of the install record.
pub fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let (f, t) = (wide(from.as_os_str()), wide(to.as_os_str()));
    // SAFETY: two NUL-terminated paths.
    if unsafe {
        MoveFileExW(
            f.as_ptr(),
            t.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// A Start Menu shortcut `lnk` → `target` (IShellLinkW + IPersistFile through COM; the two
/// interfaces' methods are called through their documented vtable slots).
pub fn shortcut(lnk: &Path, target: &Path, description: &str) -> std::io::Result<()> {
    use std::ffi::c_void;
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows_sys::core::GUID;
    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x0002_1401_0000_0000_c000_0000_0000_0046);
    const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x0002_14f9_0000_0000_c000_0000_0000_0046);
    const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000_010b_0000_0000_c000_0000_0000_0046);
    type QueryInterface =
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type SetStr = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
    type Save = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    let fail = |what: &str, hr: i32| {
        Err(std::io::Error::other(format!(
            "{what}: HRESULT 0x{:08X}",
            hr as u32
        )))
    };
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // SAFETY: COM on this thread, balanced below.
    let init = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let mut link: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid CLSID/IID; `link` receives an IShellLinkW pointer on success.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_SHELL_LINK,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ISHELL_LINK_W,
            &raw mut link,
        )
    };
    let result = (|| {
        if hr < 0 || link.is_null() {
            return fail("CoCreateInstance(ShellLink)", hr);
        }
        // SAFETY: `link` is a live COM object; its first field is the vtable pointer.
        let vt = unsafe { *(link as *const *const usize) };
        // SAFETY: IShellLinkW vtable slots: 7 SetDescription, 9 SetWorkingDirectory, 20 SetPath.
        let (set_desc, set_dir, set_path): (SetStr, SetStr, SetStr) = unsafe {
            (
                std::mem::transmute::<usize, SetStr>(*vt.add(7)),
                std::mem::transmute::<usize, SetStr>(*vt.add(9)),
                std::mem::transmute::<usize, SetStr>(*vt.add(20)),
            )
        };
        let (t, d, desc) = (
            wide(target.as_os_str()),
            wide(target.parent().unwrap_or(target).as_os_str()),
            w(description),
        );
        // SAFETY: `link` is live; the strings are NUL-terminated and outlive the calls.
        unsafe {
            for (f, s, n) in [
                (set_path, &t, "SetPath"),
                (set_dir, &d, "SetWorkingDirectory"),
                (set_desc, &desc, "SetDescription"),
            ] {
                let hr = f(link, s.as_ptr());
                if hr < 0 {
                    return fail(n, hr);
                }
            }
        }
        let mut pf: *mut c_void = std::ptr::null_mut();
        // SAFETY: slot 0 of every COM vtable is QueryInterface.
        let qi: QueryInterface = unsafe { std::mem::transmute(*vt) };
        // SAFETY: `link` is live; `pf` receives IPersistFile.
        let hr = unsafe { qi(link, &IID_IPERSIST_FILE, &raw mut pf) };
        if hr < 0 || pf.is_null() {
            return fail("QueryInterface(IPersistFile)", hr);
        }
        // SAFETY: `pf` is live; IPersistFile slot 6 is Save, slot 2 Release.
        let r = unsafe {
            let pvt = *(pf as *const *const usize);
            let save: Save = std::mem::transmute(*pvt.add(6));
            let release: Release = std::mem::transmute(*pvt.add(2));
            let l = wide(lnk.as_os_str());
            let hr = save(pf, l.as_ptr(), 1);
            release(pf);
            hr
        };
        if r < 0 {
            fail("IPersistFile::Save", r)
        } else {
            Ok(())
        }
    })();
    if !link.is_null() {
        // SAFETY: release the IShellLinkW reference taken above (slot 2).
        unsafe {
            let vt = *(link as *const *const usize);
            let release: Release = std::mem::transmute(*vt.add(2));
            release(link);
        }
    }
    if init >= 0 {
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    result
}

/// The Apps & Features entry, per user. Debug builds take `KIM_SETUP_TEST_REGISTRY_KEY` (a
/// key below HKCU) so tests never touch the real entry.
fn uninstall_key() -> String {
    #[cfg(debug_assertions)]
    if let Ok(k) = std::env::var("KIM_SETUP_TEST_REGISTRY_KEY")
        && k.starts_with(r"Software\mouv-tests\")
    {
        return k;
    }
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\MouvKim".to_string()
}

pub fn register(version: &str, dir: &Path, icon: &Path, uninstaller: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegSetValueExW,
    };
    let mut key: HKEY = std::ptr::null_mut();
    let name = w(&uninstall_key());
    // SAFETY: valid NUL-terminated subkey; `key` receives an open handle.
    let r = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &raw mut key,
            std::ptr::null_mut(),
        )
    };
    if r != 0 {
        return Err(std::io::Error::from_raw_os_error(r as i32));
    }
    let size_kb = dir_size(dir) / 1024;
    let strings = [
        ("DisplayName", "Kim".to_string()),
        ("DisplayVersion", version.to_string()),
        ("Publisher", "mouv".to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("DisplayIcon", icon.display().to_string()),
        (
            "UninstallString",
            format!("\"{}\" --uninstall", uninstaller.display()),
        ),
        (
            "QuietUninstallString",
            format!("\"{}\" --uninstall --quiet --yes", uninstaller.display()),
        ),
        (
            "URLInfoAbout",
            "https://github.com/hellomouv/kim".to_string(),
        ),
    ];
    let mut err = 0;
    for (n, v) in strings {
        let (n, v) = (w(n), w(&v));
        // SAFETY: open key; REG_SZ data is the NUL-terminated UTF-16 string with its size.
        err |= unsafe {
            RegSetValueExW(
                key,
                n.as_ptr(),
                0,
                REG_SZ,
                v.as_ptr().cast(),
                u32::try_from(v.len() * 2).unwrap_or(0),
            )
        };
    }
    for (n, v) in [
        ("NoModify", 1u32),
        ("NoRepair", 0),
        ("EstimatedSize", u32::try_from(size_kb).unwrap_or(u32::MAX)),
    ] {
        let n = w(n);
        // SAFETY: open key; four bytes of DWORD data.
        err |= unsafe { RegSetValueExW(key, n.as_ptr(), 0, REG_DWORD, (&raw const v).cast(), 4) };
    }
    // SAFETY: opened above.
    unsafe { RegCloseKey(key) };
    if err != 0 {
        Err(std::io::Error::other(
            "a registry value could not be written",
        ))
    } else {
        Ok(())
    }
}

pub fn unregister() -> std::io::Result<()> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
    let name = w(&uninstall_key());
    // SAFETY: valid NUL-terminated subkey of HKCU.
    let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, name.as_ptr()) };
    if r == 0 || r == 2 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(r as i32))
    }
}

/// Whether the Apps & Features entry exists (tests).
pub fn registered() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let (k, v) = (w(&uninstall_key()), w("DisplayVersion"));
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: valid strings; `buf` holds `size` bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    (r == 0).then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
}

fn dir_size(d: &Path) -> u64 {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// The running uninstaller cannot delete itself: if it lives in `dir`, it is moved to `%TEMP%`
/// (allowed for a running program on the same volume) so the folder can go; Windows' storage
/// cleanup removes the leftover later.
pub fn move_self_out(dir: &Path) {
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let (Ok(me_c), Ok(dir_c)) = (std::fs::canonicalize(&me), std::fs::canonicalize(dir)) else {
        return;
    };
    if !me_c.starts_with(&dir_c) {
        return;
    }
    let dest =
        std::env::temp_dir().join(format!("kim-setup-uninstalled-{}.exe", std::process::id()));
    let _ = std::fs::rename(&me, dest);
}
