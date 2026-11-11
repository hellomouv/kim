//! `Kim-Setup.exe` (Step 43, ADR 0072): double-click → one small progress window → Kim Manager
//! opens. Per user, no administrator rights, no network, only Kim.
//!
//! ```text
//! Kim-Setup.exe                      install (or upgrade), then start Kim Manager
//! Kim-Setup.exe --repair             restore the files of the installed version
//! Kim-Setup.exe --uninstall          remove Kim (asks first; settings kept unless --purge-data)
//! options: --quiet (no window, no launch)  --json  --yes  --dry-run  --no-launch
//!          --allow-downgrade  --purge-data  --version
//! ```
#![windows_subsystem = "windows"]

#[cfg(windows)]
#[allow(unsafe_code)]
mod gui;

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    return run();
    #[cfg(not(windows))]
    {
        eprintln!(
            "Kim-Setup is the Windows installer; on Linux use the package of your distribution"
        );
        std::process::ExitCode::from(5)
    }
}

#[cfg(windows)]
fn run() -> std::process::ExitCode {
    use kim_setup::{Layout, Options, SetupError};
    use serde_json::json;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--version") {
        println!("kim-setup {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    let known = [
        "--quiet",
        "--json",
        "--yes",
        "--dry-run",
        "--no-launch",
        "--repair",
        "--allow-downgrade",
        "--uninstall",
        "--purge-data",
    ];
    let quiet = has("--quiet");
    let json_out = has("--json");
    let report = |cmd: &str, r: Result<serde_json::Value, SetupError>| -> std::process::ExitCode {
        match r {
            Ok(data) => {
                if json_out {
                    println!(
                        "{}",
                        json!({"schema_version": 1, "command": cmd, "ok": true, "data": data, "warnings": []})
                    );
                }
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                if json_out {
                    println!(
                        "{}",
                        json!({"schema_version": 1, "command": cmd, "ok": false, "error": {"code": e.code, "exit_code": e.exit, "message": e.message, "hint": e.hint, "details": []}})
                    );
                } else if !quiet {
                    gui::message(
                        &format!(
                            "{}{}",
                            e.message,
                            e.hint.map(|h| format!("\n\n{h}")).unwrap_or_default()
                        ),
                        true,
                    );
                }
                std::process::ExitCode::from(e.exit)
            }
        }
    };
    if let Some(bad) = args.iter().find(|a| !known.contains(&a.as_str())) {
        return report("setup", Err(SetupError::new("usage", 2, format!("unknown option `{bad}`"))
            .hint("options: --repair, --uninstall, --quiet, --json, --yes, --dry-run, --no-launch, --allow-downgrade, --purge-data")));
    }
    let layout = match Layout::from_env() {
        Ok(l) => l,
        Err(e) => return report("setup", Err(e)),
    };
    if has("--uninstall") {
        if !has("--yes") && !has("--dry-run") {
            if quiet {
                return report(
                    "uninstall",
                    Err(SetupError::new(
                        "confirmation_required",
                        7,
                        "uninstalling needs --yes when no window may ask",
                    )),
                );
            }
            let purge = if has("--purge-data") {
                "\n\nYour settings will also be DELETED."
            } else {
                "\n\nYour settings in %APPDATA%\\mouv\\kim are kept."
            };
            if !gui::confirm(&format!(
                "Uninstall Kim (kim.exe and Kim Manager)?{purge}\n\nTools you installed with Kim (Subak, …) stay installed."
            )) {
                return report(
                    "uninstall",
                    Err(SetupError::new("cancelled", 8, "nothing was changed")),
                );
            }
        }
        let r = kim_setup::uninstall(&layout, has("--purge-data"), has("--dry-run"));
        if r.is_ok() && !quiet && !json_out && !has("--dry-run") {
            gui::message("Kim was uninstalled.", false);
        }
        return report("uninstall", r);
    }
    let me = std::env::current_exe().unwrap_or_default();
    let payload = match kim_setup::read_payload(&me) {
        Ok(p) => p,
        Err(e) => return report("install", Err(e)),
    };
    let opts = Options {
        repair: has("--repair"),
        allow_downgrade: has("--allow-downgrade"),
        dry_run: has("--dry-run"),
    };
    // Double-clicking an installer of the installed version is a repair (asked first).
    let mut opts = opts;
    if !quiet
        && !opts.repair
        && !opts.dry_run
        && let Ok(v) = kim_setup::package_version(&payload)
        && kim_setup::installed(&layout).is_some_and(|(have, _)| have == v)
    {
        if !gui::confirm(&format!(
            "Kim {v} is already installed.\n\nRepair it (restore its files)?"
        )) {
            return report(
                "install",
                Err(SetupError::new("cancelled", 8, "nothing was changed")),
            );
        }
        opts.repair = true;
    }
    let r = if quiet {
        kim_setup::install(&layout, &payload, &me, opts, &mut |_, _| {})
    } else {
        gui::run_with_progress(move |p| kim_setup::install(&layout, &payload, &me, opts, p))
    };
    if let Ok(d) = &r
        && !quiet
        && !has("--no-launch")
        && !opts.dry_run
        && let Some(m) = d["manager"].as_str()
    {
        let _ = std::process::Command::new(m).spawn();
    }
    report("install", r)
}
