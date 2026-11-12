//! `noeul startup status | enable | disable [--dry-run]` (#99, `SET-03`, NOEUL-FR-002): opt-in
//! start at sign-in through one per-user Run value that starts `noeulw.exe` (no console, so
//! nothing flashes). Off by default; never written by anything else. Debug builds accept a
//! private test key in `NOEUL_DEV_STARTUP_KEY` so no test touches the real Run key.

use crate::{DryRun, Outcome};
use noeul::error::{Code, NoeulError};
use noeul::startup::Store;
use serde_json::{Value, json};

/// `noeulw.exe` next to this `noeul.exe`.
fn starter() -> Result<std::path::PathBuf, NoeulError> {
    let me = std::env::current_exe()
        .map_err(|e| NoeulError::new(Code::Io, format!("this program's path: {e}")))?;
    Ok(me.with_file_name("noeulw.exe"))
}

fn state_json(store: &dyn Store) -> Result<Value, NoeulError> {
    let program = starter()?;
    let s = noeul::startup::state(store, &program)
        .map_err(|e| NoeulError::new(Code::Io, format!("the sign-in setting: {e}")))?;
    Ok(json!({
        "enabled": s.enabled,
        "command": s.command,
        "foreign": s.foreign,
        "location": store.location(),
        "program": program.display().to_string(),
    }))
}

#[cfg(windows)]
fn store() -> noeul::win::startup::Registry {
    noeul::win::startup::Registry::for_cli()
}

pub fn status() -> Result<Outcome, NoeulError> {
    #[cfg(windows)]
    {
        let v = state_json(&store())?;
        let human = if v["enabled"] == true {
            format!(
                "Noeul starts when you sign in ({}).\n",
                v["location"].as_str().unwrap_or("")
            )
        } else {
            "Noeul does not start when you sign in (the default).\n".to_string()
        };
        Ok(Outcome::new(v, human))
    }
    #[cfg(not(windows))]
    {
        let _ = state_json;
        Err(NoeulError::new(
            Code::UnsupportedPlatform,
            "start at sign-in is Windows only for now",
        ))
    }
}

pub fn change(enable: bool, a: &DryRun) -> Result<Outcome, NoeulError> {
    #[cfg(windows)]
    {
        let mut s = store();
        let before = state_json(&s)?;
        let o = if enable {
            noeul::startup::enable(&mut s, &starter()?, a.dry_run)
        } else {
            noeul::startup::disable(&mut s, a.dry_run)
        }
        .map_err(|e| NoeulError::new(Code::Io, e))?;
        let human = match (o.changed, a.dry_run, enable) {
            (false, _, _) => "Nothing to change.\n".to_string(),
            (true, true, true) => "Would start Noeul when you sign in (dry run).\n".to_string(),
            (true, true, false) => {
                "Would no longer start Noeul at sign-in (dry run).\n".to_string()
            }
            (true, false, true) => "Noeul now starts when you sign in.\n".to_string(),
            (true, false, false) => "Noeul no longer starts when you sign in.\n".to_string(),
        };
        Ok(Outcome::new(
            json!({
                "dry_run": a.dry_run,
                "changed": o.changed,
                "before": before,
                "after": o.after,
                "location": s.location(),
            }),
            human,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (enable, a);
        Err(NoeulError::new(
            Code::UnsupportedPlatform,
            "start at sign-in is Windows only for now",
        ))
    }
}
