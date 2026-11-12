//! `noeulw.exe` (#99, `SET-03`): the windowless sign-in starter. Windows runs it from the Run key
//! that `noeul startup enable` writes; it has no console, so nothing flashes at sign-in. It starts
//! `noeul.exe start` next to it without a window (the resident instance starts, or the running
//! one is left alone) and ends. It reads no input and opens nothing else.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let Ok(me) = std::env::current_exe() else {
        std::process::exit(1);
    };
    let noeul = me.with_file_name(if cfg!(windows) { "noeul.exe" } else { "noeul" });
    let mut c = std::process::Command::new(noeul);
    c.args(["start", "--quiet", "--no-input"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let code = c.status().map_or(1, |s| s.code().unwrap_or(1));
    std::process::exit(code);
}
