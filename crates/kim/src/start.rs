//! ADR 0089: what a double-click on `kim.exe` does.
//!
//! `kim` is a headless CLI (`KIM-FR-012`): with arguments, from a terminal, a script or an agent
//! it never opens a window. A double-click in Explorer (or a shortcut) is none of these: Windows
//! gives the program a console of its own, nobody else is attached to it, and there are no
//! arguments, and its input and output are that console. Only then does `kim.exe` hand over to
//! the Kim window (`kim-manager.exe`) and end; the console it was given closes with it. A
//! terminal always shares its console with the shell; a script or agent passes arguments, runs
//! without a console or with a hidden one (`CREATE_NO_WINDOW`: no console window), or reads and
//! writes pipes; and `KIM_NO_INPUT=1` turns the hand-over off entirely.
//!
//! The window is looked for next to `kim.exe` first (a portable folder or a build), then in
//! Kim's own install. A local mirror next to `kim.exe` (`kim-local\catalog.json`, written by
//! `cargo xtask local-mirror`) is passed on, so the window can install the tools built on this
//! PC without the network.

use crate::paths::Paths;
use std::path::{Path, PathBuf};

/// The window program.
pub const MANAGER_EXE: &str = "kim-manager.exe";
/// The folder of a local mirror next to `kim.exe`.
pub const MIRROR_DIR: &str = "kim-local";

/// What [`is_double_click`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launch {
    /// `argv` length, the program name included.
    pub args: usize,
    /// Processes attached to this console ([`console_processes`]).
    pub console_processes: u32,
    /// The console has a window (none under `CREATE_NO_WINDOW`).
    pub console_window: bool,
    /// Standard input and output are the console, not pipes or files.
    pub stdio_is_console: bool,
    /// `KIM_NO_INPUT` is set.
    pub no_input: bool,
}

/// Whether this start is a double-click: no arguments, alone on a console that has a window,
/// input and output on that console, and input not turned off.
pub fn is_double_click(l: Launch) -> bool {
    l.args <= 1 && l.console_processes == 1 && l.console_window && l.stdio_is_console && !l.no_input
}

/// This process's [`Launch`].
pub fn this_launch(args: usize, no_input: bool) -> Launch {
    use std::io::IsTerminal;
    Launch {
        args,
        console_processes: console_processes(),
        console_window: console_window(),
        stdio_is_console: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        no_input,
    }
}

/// Where the window may be, in order: next to `kim.exe`, then Kim's installed version.
pub fn manager_candidates(exe_dir: &Path, paths: Option<&Paths>) -> Vec<PathBuf> {
    let mut out = vec![exe_dir.join(MANAGER_EXE)];
    if let Some(p) = paths
        && let Ok(text) = std::fs::read_to_string(p.install_record("kim"))
        && let Ok(rec) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(v) = rec["version"].as_str()
        && !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        && !v.contains("..")
    {
        out.push(p.tool_dir("kim").join("versions").join(v).join(MANAGER_EXE));
    }
    out
}

/// A `file:///` URL for a local path.
pub fn file_url(path: &Path) -> String {
    let s = path.display().to_string().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    let encoded: String = s
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            c => c.to_string(),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// The local mirror next to `kim.exe`, as the window's `--catalog` source.
pub fn mirror_beside(exe_dir: &Path) -> Option<String> {
    let c = exe_dir.join(MIRROR_DIR).join("catalog.json");
    (c.is_file() && c.with_extension("json.sig").is_file()).then(|| file_url(&c))
}

/// How many processes share this process's console (0: no console).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn console_processes() -> u32 {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    let mut ids = [0u32; 4];
    // SAFETY: `ids` is a valid, writable buffer of the length passed.
    unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) }
}

#[cfg(not(windows))]
pub fn console_processes() -> u32 {
    0
}

/// Whether this process's console has a window.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn console_window() -> bool {
    // SAFETY: no arguments; returns a window handle or null.
    !unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null()
}

#[cfg(not(windows))]
pub fn console_window() -> bool {
    false
}

/// Lets go of the console Windows made for the double-click, so it closes now.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn release_console() {
    // SAFETY: no arguments; detaching from the console has no other effect on this process.
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
}

#[cfg(not(windows))]
pub fn release_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_lone_start_without_arguments_is_a_double_click() {
        let explorer = Launch {
            args: 1,
            console_processes: 1,
            console_window: true,
            stdio_is_console: true,
            no_input: false,
        };
        assert!(is_double_click(explorer));
        let not = |l: Launch, why: &str| assert!(!is_double_click(l), "{why}");
        not(
            Launch {
                args: 2,
                ..explorer
            },
            "arguments: the CLI",
        );
        not(
            Launch {
                console_processes: 2,
                ..explorer
            },
            "a terminal shares the console",
        );
        not(
            Launch {
                console_processes: 0,
                ..explorer
            },
            "no console: a script or an agent",
        );
        not(
            Launch {
                console_window: false,
                ..explorer
            },
            "CREATE_NO_WINDOW: a hidden start",
        );
        not(
            Launch {
                stdio_is_console: false,
                ..explorer
            },
            "pipes: a script or an agent",
        );
        not(
            Launch {
                no_input: true,
                ..explorer
            },
            "KIM_NO_INPUT turns it off",
        );
    }

    #[test]
    fn file_urls_are_absolute_and_escaped() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\a b\kim-local\catalog.json")),
            "file:///C:/Users/a%20b/kim-local/catalog.json"
        );
        assert_eq!(
            file_url(Path::new(r"\\?\C:\x#y\c.json")),
            "file:///C:/x%23y/c.json"
        );
    }

    #[test]
    fn the_window_next_to_kim_comes_first_then_the_installed_one() {
        let d = std::env::temp_dir().join(format!("kim-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let paths = Paths::under(&d);
        std::fs::create_dir_all(paths.tool_dir("kim")).unwrap();
        std::fs::write(paths.install_record("kim"), r#"{"version":"0.0.2"}"#).unwrap();
        let c = manager_candidates(Path::new(r"C:\portable"), Some(&paths));
        assert_eq!(c[0], Path::new(r"C:\portable").join(MANAGER_EXE));
        assert_eq!(
            c[1],
            paths
                .tool_dir("kim")
                .join("versions")
                .join("0.0.2")
                .join(MANAGER_EXE)
        );
        assert_eq!(mirror_beside(&d), None);
        std::fs::create_dir_all(d.join(MIRROR_DIR)).unwrap();
        std::fs::write(d.join(MIRROR_DIR).join("catalog.json"), "{}").unwrap();
        assert_eq!(mirror_beside(&d), None, "no signature, no mirror");
        std::fs::write(d.join(MIRROR_DIR).join("catalog.json.sig"), "{}").unwrap();
        assert!(
            mirror_beside(&d)
                .unwrap()
                .ends_with("/kim-local/catalog.json")
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
