//! `noeul history …`, `noeul pin restore` and `noeul pin backup …` (Step 60, ADR 0074;
//! NOEUL-FR-050, NOEUL-FR-051, NOEUL-FR-066).
//!
//! They talk to the running Noeul, which holds the history and the pins. Two of them also work
//! when no Noeul runs, on the user-only data folder directly, so private data can always be
//! removed: `noeul history clear` (the disk history) and `noeul pin backup clear`; `noeul pin
//! backup status` reads the backup file. Clearing and forgetting ask first (or need `--yes`);
//! nothing here captures, reads the clipboard or prints pixels, paths of pictures or titles.

use crate::{Ctx, Outcome};
use clap::{Args, Subcommand};
use noeul::error::{Code, NoeulError, Result};
use serde_json::{Value, json};

#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    /// The kept captures: number, rectangle, time, how each ended, whether it is on disk (never
    /// pixels)
    List,
    /// Pin a kept capture again where it was taken (default: the newest)
    Pin(HistoryPinArgs),
    /// Forget one kept capture, in memory and on disk (asks first)
    Forget(HistoryForgetArgs),
    /// Forget every kept capture, in memory and on disk (asks first; works without a running
    /// Noeul for the disk history)
    Clear(YesDryRun),
}

#[derive(Debug, Args)]
pub struct HistoryPinArgs {
    /// The capture's number (`noeul history list`); default: the newest
    pub entry: Option<u64>,
    /// Show what would happen; change nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct HistoryForgetArgs {
    /// The capture's number (`noeul history list`)
    pub entry: u64,
    /// Do not ask
    #[arg(long)]
    pub yes: bool,
    /// Show what would happen; change nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct YesDryRun {
    /// Do not ask
    #[arg(long)]
    pub yes: bool,
    /// Show what would happen; change nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Whether the backup is on, when it was written, how many pins it holds and how many are
    /// not open now (works without a running Noeul)
    Status,
    /// Write the backup now (it is also written by itself a second after the pins change)
    Save(crate::DryRun),
    /// Remove the backup and its pictures; open pins stay open (asks first; works without a
    /// running Noeul)
    Clear(YesDryRun),
}

fn paths() -> Result<noeul::paths::Paths> {
    noeul::paths::Paths::from_env()
}

fn ask(ctx: &Ctx, yes: bool, dry_run: bool, what: &str) -> Result<()> {
    if yes || dry_run {
        return Ok(());
    }
    match crate::confirm(ctx, &format!("{what}? This cannot be undone. [y/N]"), "y") {
        Some(true) => Ok(()),
        Some(false) => Err(NoeulError::new(Code::Cancelled, "nothing was removed")),
        None => Err(NoeulError::new(
            Code::ConfirmationRequired,
            format!("{} needs confirmation", what.to_lowercase()),
        )
        .hint("re-run with --yes (or --dry-run to preview); nothing was removed")),
    }
}

/// The running Noeul's answer to `method`, or `None` when no Noeul runs.
#[cfg(windows)]
fn request(ctx: &Ctx, method: &str, params: Value) -> Result<Option<Value>> {
    match crate::instance::connect(ctx)? {
        Some(mut s) => s.request(method, params).map(Some),
        None => Ok(None),
    }
}

#[cfg(not(windows))]
fn request(_: &Ctx, _: &str, _: Value) -> Result<Option<Value>> {
    Err(NoeulError::unsupported_platform("the Noeul instance"))
}

fn running(ctx: &Ctx, method: &str, params: Value) -> Result<Value> {
    request(ctx, method, params)?.ok_or_else(NoeulError::not_running)
}

pub fn history(ctx: &Ctx, c: &HistoryCommand) -> Result<Outcome> {
    match c {
        HistoryCommand::List => {
            let v = running(ctx, "history.list", json!({}))?;
            let mut human = String::new();
            let entries = v["entries"].as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                human.push_str("The capture history is empty.\n");
            } else {
                for e in &entries {
                    let r = &e["rect"];
                    human.push_str(&format!(
                        "{:>4}  {}x{} at {},{}  {}  {}{}\n",
                        e["id"],
                        r["width"],
                        r["height"],
                        r["x"],
                        r["y"],
                        e["at"].as_str().unwrap_or(""),
                        e["outcome"].as_str().unwrap_or(""),
                        if e["on_disk"] == true {
                            "  (on disk)"
                        } else {
                            ""
                        }
                    ));
                }
            }
            human.push_str(&format!(
                "{} capture{} kept, {} MiB in memory; disk history {}.\n",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" },
                v["bytes"].as_u64().unwrap_or(0) / (1024 * 1024),
                if v["disk"] == true { "on" } else { "off" }
            ));
            Ok(Outcome::new(v, human))
        }
        HistoryCommand::Pin(a) => {
            let v = running(
                ctx,
                "history.pin",
                json!({"entry": a.entry, "dry_run": a.dry_run}),
            )?;
            let human = if a.dry_run {
                format!("Would pin capture {} again.\n", v["entry"])
            } else {
                format!(
                    "Pinned capture {} again as {}.\n",
                    v["entry"],
                    v["pin"].as_str().unwrap_or("")
                )
            };
            Ok(Outcome::new(v, human))
        }
        HistoryCommand::Forget(a) => {
            ask(
                ctx,
                a.yes,
                a.dry_run,
                &format!("Forget capture {}", a.entry),
            )?;
            let v = running(
                ctx,
                "history.forget",
                json!({"entry": a.entry, "dry_run": a.dry_run}),
            )?;
            let human = format!(
                "{} capture {}.\n",
                if a.dry_run { "Would forget" } else { "Forgot" },
                a.entry
            );
            Ok(Outcome::new(v, human))
        }
        HistoryCommand::Clear(a) => {
            ask(ctx, a.yes, a.dry_run, "Clear the capture history")?;
            let v = match request(ctx, "history.clear", json!({"dry_run": a.dry_run}))? {
                Some(v) => v,
                None => {
                    // No Noeul runs: the disk history only (memory has nothing).
                    let mut d = noeul::store::HistoryDir::open(&paths()?.data_dir);
                    let n = d.entries().len();
                    if !a.dry_run {
                        d.clear()
                            .map_err(|e| NoeulError::io(d.dir().display(), &e))?;
                    }
                    json!({"dry_run": a.dry_run, "cleared": 0, "disk_cleared": n})
                }
            };
            let human = format!(
                "{} {} kept capture{} in memory and {} on disk.\n",
                if a.dry_run { "Would clear" } else { "Cleared" },
                v["cleared"],
                if v["cleared"] == 1 { "" } else { "s" },
                v["disk_cleared"]
            );
            Ok(Outcome::new(v, human))
        }
    }
}

/// `noeul settings`: the running Noeul opens (or brings forward) its settings window.
pub fn settings(ctx: &Ctx, dry_run: bool) -> Result<Outcome> {
    let v = running(ctx, "settings.open", json!({"dry_run": dry_run}))?;
    let human = if dry_run {
        "Would open the settings window.\n".to_string()
    } else {
        "The settings window is open.\n".to_string()
    };
    Ok(Outcome::new(v, human))
}

pub fn restore(ctx: &Ctx, dry_run: bool) -> Result<Outcome> {
    let v = running(ctx, "backup.restore", json!({"dry_run": dry_run}))?;
    let mut human = if dry_run {
        format!(
            "Would restore {} pin(s); {} already open.\n",
            v["would_restore"], v["already_open"]
        )
    } else {
        format!(
            "Restored {} pin(s); {} already open, {} skipped.\n",
            v["restored"].as_array().map_or(0, Vec::len),
            v["already_open"],
            v["skipped"]
        )
    };
    for p in v["problems"].as_array().into_iter().flatten() {
        human.push_str(&format!("  {}\n", p.as_str().unwrap_or("")));
    }
    Ok(Outcome::new(v, human))
}

pub fn backup(ctx: &Ctx, c: &BackupCommand) -> Result<Outcome> {
    match c {
        BackupCommand::Status => {
            let v = match request(ctx, "backup.status", json!({}))? {
                Some(v) => v,
                None => {
                    let p = paths()?;
                    let r = noeul::store::PinStore::new(&p.data_dir).read();
                    let enabled = noeul::settings::load(&p.config_file()).settings.pin_backup;
                    json!({"enabled": enabled,
                        "saved_at": r.saved_at.map(|s| noeul::util::rfc3339(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s))),
                        "pins": r.records.len(), "restorable": r.records.len(), "problems": r.problems})
                }
            };
            let human = format!(
                "Pin backup {}; {} pin(s) in it{}, {} not open now.\n",
                if v["enabled"] == true { "on" } else { "off" },
                v["pins"],
                v["saved_at"]
                    .as_str()
                    .map(|s| format!(" (written {s})"))
                    .unwrap_or_default(),
                v["restorable"]
            );
            Ok(Outcome::new(v, human))
        }
        BackupCommand::Save(a) => {
            let v = running(ctx, "backup.save", json!({"dry_run": a.dry_run}))?;
            let human = format!(
                "{} {} pin(s).\n",
                if a.dry_run {
                    "Would back up"
                } else {
                    "Backed up"
                },
                v["saved"]
            );
            Ok(Outcome::new(v, human))
        }
        BackupCommand::Clear(a) => {
            ask(ctx, a.yes, a.dry_run, "Remove the pin backup")?;
            let v = match request(ctx, "backup.clear", json!({"dry_run": a.dry_run}))? {
                Some(v) => v,
                None => {
                    let s = noeul::store::PinStore::new(&paths()?.data_dir);
                    let n = s.read().records.len();
                    if !a.dry_run {
                        s.clear()
                            .map_err(|e| NoeulError::new(Code::Io, e.to_string()))?;
                    }
                    json!({"dry_run": a.dry_run, "removed": n})
                }
            };
            let human = format!(
                "{} the pin backup ({} pin(s)); open pins stay open.\n",
                if a.dry_run { "Would remove" } else { "Removed" },
                v["removed"]
            );
            Ok(Outcome::new(v, human))
        }
    }
}
