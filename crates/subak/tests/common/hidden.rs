//! Step 35: programs that would open console windows (the test sshd starts `cmd.exe /c
//! sftp-server.exe` with a new console for every SFTP session) run on a private desktop of
//! this test process, so nothing ever appears on the user's screen (AGENTS.md §5).
#![allow(dead_code)]

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
};

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        mode: *const core::ffi::c_void,
        flags: u32,
        access: u32,
        sa: *const core::ffi::c_void,
    ) -> isize;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The private desktop's name (created once; it lives as long as this process).
pub fn desktop() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let name = format!("subak-test-{}", std::process::id());
        let w = wide(&name);
        // SAFETY: a NUL-terminated name; GENERIC_ALL; the handle is kept for the process.
        let h = unsafe {
            CreateDesktopW(
                w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                0x1000_0000,
                std::ptr::null(),
            )
        };
        assert!(h != 0, "CreateDesktopW failed");
        name
    })
}

/// Starts `exe args…` on the private desktop without a window or inherited handles; its pid.
pub fn spawn(exe: &Path, args: &[String]) -> u32 {
    let mut line: Vec<u16> = std::iter::once(format!("\"{}\"", exe.display()))
        .chain(args.iter().map(|a| format!("\"{a}\"")))
        .collect::<Vec<_>>()
        .join(" ")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let app: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut desk = wide(desktop());
    // SAFETY: plain C structs; all-zero is valid.
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = u32::try_from(size_of::<STARTUPINFOW>()).unwrap();
    si.lpDesktop = desk.as_mut_ptr();
    // SAFETY: as above.
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: NUL-terminated strings, a writable command line, valid structs.
    let ok = unsafe {
        CreateProcessW(
            app.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &raw const si,
            &raw mut pi,
        )
    };
    assert!(ok != 0, "CreateProcessW {}", exe.display());
    // SAFETY: handles from CreateProcessW.
    unsafe {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    pi.dwProcessId
}

/// Ends `pid` and every process it started (sshd and its sessions).
pub fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
