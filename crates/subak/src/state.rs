//! Step 29 (ADR 0006, ADR 0063): the durable state store — `state.json` (`subak.state`).
//!
//! - **What:** Terminal names and ids, internal windows, panes, layouts, working directories,
//!   the command each pane was started with (only when it holds nothing secret-looking), the
//!   history limit, the restore policy and Claude session references. Never PIDs, screen
//!   content, environment values or client identities.
//! - **How:** canonical JSON with a CRC-32; written to `state.json.tmp`, flushed, then moved
//!   over `state.json` with write-through (a crash leaves the old or the new file, never a torn
//!   one); a copy of the previous file goes to `backups\` once per Host run (newest 5 kept).
//! - **Load:** parse → `schema` → CRC → migrate step by step (backup first) → check. A corrupt
//!   file is kept as `state.corrupt-<time>.json` and the newest valid backup is used; a file from
//!   a newer version is never rewritten (`state_from_newer_version`).
//!
//! Loading never starts anything: Terminals saved by an earlier Host are kept as *saved*
//! records until they are restored (Step 30) or forgotten.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "subak.state";
/// v1: the Step 03 draft (never written by a release). v2 (Step 29): `restore` per Terminal,
/// layout `cells`, window `size`, pane `history_limit`, `command_line`; no `env` values.
pub const VERSION: u64 = 2;
/// A state file larger than this is refused (bounded disk use; ~100 000 panes fit).
pub const MAX_BYTES: u64 = 32 << 20;
/// Backups kept (`backups\state-<time>.json`).
pub const BACKUPS: usize = 5;
/// Changes are written at most this often (coalesced).
pub const COALESCE: std::time::Duration = std::time::Duration::from_millis(500);

/// CRC-32 (IEEE 802.3, reflected, as zlib).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = u32::try_from(i).expect("< 256");
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in bytes {
        crc = table[((crc ^ u32::from(*b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// The CRC of a document: over its canonical form (sorted keys, no whitespace) without `crc32`.
pub fn checksum(doc: &Value) -> String {
    let mut d = doc.clone();
    if let Value::Object(m) = &mut d {
        m.remove("crc32");
    }
    // serde_json's map is ordered by key: `to_string` is the canonical form.
    format!(
        "{:08x}",
        crc32(serde_json::to_string(&d).expect("JSON").as_bytes())
    )
}

/// A document ready to write: `written_at`, the writer and the CRC.
pub fn seal(terminals: &[Value], now: &str) -> Value {
    let mut d = json!({
        "schema": SCHEMA,
        "schema_version": VERSION,
        "written_at": now,
        "writer": {"version": env!("CARGO_PKG_VERSION")},
        "terminals": terminals,
    });
    d["crc32"] = json!(checksum(&d));
    d
}

/// Why a state file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bad {
    /// Not JSON, not `subak.state`, a wrong CRC, too large, or an invalid structure.
    Corrupt(String),
    /// Written by a newer Subak: used read-only, never rewritten.
    Newer(u64),
}

/// v1 → v2: the Step 03 draft gains the Step 29 fields (defaults that change nothing) and
/// loses environment values (never kept since Step 29).
fn v1_to_v2(mut d: Value) -> Value {
    for t in d["terminals"].as_array_mut().into_iter().flatten() {
        if t.get("restore").is_none() {
            t["restore"] = json!("ask");
        }
        for w in t["windows"].as_array_mut().into_iter().flatten() {
            for p in w["panes"].as_array_mut().into_iter().flatten() {
                let names: Vec<String> = p["env"]
                    .as_object()
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                p["env"] = json!(names);
                if let Value::Object(m) = p {
                    m.remove("env_secret_refs");
                    m.entry("history_limit").or_insert(json!(2000));
                    m.entry("command_line").or_insert(Value::Null);
                }
            }
        }
    }
    d["schema_version"] = json!(2);
    d
}

/// The migration chain, oldest first: `MIGRATIONS[i]` takes version `i + 1` to `i + 2`.
const MIGRATIONS: [fn(Value) -> Value; 1] = [v1_to_v2];

/// Parses, checks and migrates a state file's text. Returns the document at [`VERSION`] and
/// whether it was migrated.
pub fn parse(text: &str) -> Result<(Value, bool), Bad> {
    if text.len() as u64 > MAX_BYTES {
        return Err(Bad::Corrupt(format!("larger than {} MiB", MAX_BYTES >> 20)));
    }
    let d: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| Bad::Corrupt(format!("not JSON: {e}")))?;
    if d["schema"] != SCHEMA {
        return Err(Bad::Corrupt("not a subak.state document".into()));
    }
    let Some(version) = d["schema_version"].as_u64().filter(|v| *v >= 1) else {
        return Err(Bad::Corrupt("no valid schema_version".into()));
    };
    if version > VERSION {
        // The CRC of a newer format is still checked: a newer file is used only if intact.
        if d["crc32"].as_str() != Some(checksum(&d).as_str()) {
            return Err(Bad::Corrupt("the checksum does not match".into()));
        }
        return Err(Bad::Newer(version));
    }
    if d["crc32"].as_str() != Some(checksum(&d).as_str()) {
        return Err(Bad::Corrupt(
            "the checksum does not match (a torn or edited file)".into(),
        ));
    }
    let mut d = d;
    let migrated = version < VERSION;
    for m in &MIGRATIONS[usize::try_from(version - 1).unwrap_or(0)..] {
        d = m(d);
    }
    check(&d).map_err(Bad::Corrupt)?;
    Ok((d, migrated))
}

/// The structure a Host relies on: ids, names, a window and a pane each, a layout naming
/// exactly the window's panes, unique ids.
pub fn check(d: &Value) -> Result<(), String> {
    let ts = d["terminals"].as_array().ok_or("no terminals list")?;
    let mut ids = std::collections::BTreeSet::new();
    for t in ts {
        let tid = t["id"]
            .as_str()
            .filter(|s| crate::session::is_id(s, 't'))
            .ok_or("a Terminal without a valid id")?;
        if !ids.insert(tid.to_string()) {
            return Err(format!("the id {tid} appears twice"));
        }
        t["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .ok_or(format!("{tid} has no name"))?;
        if !matches!(t["restore"].as_str(), Some("ask" | "auto" | "never")) {
            return Err(format!("{tid} has no valid restore policy"));
        }
        let ws = t["windows"]
            .as_array()
            .filter(|w| !w.is_empty())
            .ok_or(format!("{tid} has no window"))?;
        for w in ws {
            let wid = w["id"]
                .as_str()
                .filter(|s| crate::session::is_id(s, 'w'))
                .ok_or(format!("{tid}: a window without a valid id"))?;
            if !ids.insert(wid.to_string()) {
                return Err(format!("the id {wid} appears twice"));
            }
            let ps = w["panes"]
                .as_array()
                .filter(|p| !p.is_empty())
                .ok_or(format!("{wid} has no pane"))?;
            let mut mine = Vec::new();
            for p in ps {
                let pid = p["id"]
                    .as_str()
                    .filter(|s| crate::session::is_id(s, 'p'))
                    .ok_or(format!("{wid}: a pane without a valid id"))?;
                if !ids.insert(pid.to_string()) {
                    return Err(format!("the id {pid} appears twice"));
                }
                mine.push(pid.to_string());
            }
            let mut named = Vec::new();
            layout_panes(&w["layout"], &mut named, 0)?;
            named.sort();
            mine.sort();
            if named != mine {
                return Err(format!("{wid}: its layout does not name exactly its panes"));
            }
        }
    }
    Ok(())
}

fn layout_panes(n: &Value, out: &mut Vec<String>, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("a layout nested too deep".into());
    }
    match n["type"].as_str() {
        Some("pane") => {
            out.push(
                n["pane"]
                    .as_str()
                    .ok_or("a layout pane without an id")?
                    .to_string(),
            );
            Ok(())
        }
        Some("split") => {
            let c = n["children"]
                .as_array()
                .filter(|c| c.len() >= 2)
                .ok_or("a split with fewer than two children")?;
            for x in c {
                layout_panes(&x["node"], out, depth + 1)?;
            }
            Ok(())
        }
        _ => Err("an invalid layout node".into()),
    }
}

/// The record of a live Terminal (the Host's model), as `state.json` keeps it.
pub fn terminal_record(
    t: &crate::session::Terminal,
    restore: &str,
    claude_resume: bool,
    claude: &dyn Fn(&str) -> Option<String>,
    cwd_now: &dyn Fn(&str) -> Option<String>,
) -> Value {
    let windows: Vec<Value> = t
        .windows
        .iter()
        .filter(|w| w.layout.is_some() && !w.panes.is_empty())
        .map(|w| {
            let panes: Vec<Value> = w
                .panes
                .iter()
                .map(|p| {
                    // A command that holds a secret-looking value is not kept (it could not be
                    // kept without the secret; restore then uses the profile or the shell).
                    let args_clean = crate::redact::args(&p.command.args) == p.command.args;
                    let prog_clean = crate::redact::program(&p.command.program) == p.command.program;
                    let line_clean = p
                        .command_line
                        .as_deref()
                        .is_none_or(|l| crate::redact::command_line(l) == l);
                    let keep = args_clean && prog_clean && line_clean;
                    json!({
                        "id": p.id,
                        "index": p.index,
                        // Step 33: the folder the shell reported last, else where it started.
                        "cwd": cwd_now(&p.id).or_else(|| p.cwd.clone()),
                        "command": keep.then(|| json!({"program": p.command.program, "args": p.command.args})),
                        "command_line": if keep { json!(p.command_line) } else { Value::Null },
                        "command_withheld": !keep,
                        "restart_command": null,
                        "env": p.env.set,
                        "env_unset": p.env.unset,
                        "history_limit": p.history_limit,
                        "title": null,
                        "claude": claude(&p.id).map(|sid| json!({"session_id": sid, "resume": claude_resume})),
                        "scrollback": {"persisted": false},
                    })
                })
                .collect();
            json!({
                "id": w.id,
                "index": w.index,
                "name": w.name,
                "automatic_name": w.automatic_name,
                "active_pane": w.panes[w.active_pane].id,
                "size": {"cols": w.layout().size().0, "rows": w.layout().size().1},
                "layout": w.layout().json(),
                "panes": panes,
            })
        })
        .collect();
    json!({
        "id": t.id,
        "name": t.name.display,
        "created_at": t.created_at,
        "profile": null,
        "restore": restore,
        "remain_on_exit": t.remain_on_exit,
        "active_window": t.windows[t.active_window].id,
        "windows": windows,
    })
}

// ---------------------------------------------------------------------------------------------
// Files.

/// Where a Host keeps its state: `%LOCALAPPDATA%\mouv\subak\state` for the default endpoint.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("mouv").join("subak").join("state"))
}

/// What a load found.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The document (at [`VERSION`]); `None`: no state yet (or nothing usable).
    pub doc: Option<Value>,
    /// The file it came from when not `state.json` (a backup after corruption).
    pub recovered_from: Option<PathBuf>,
    /// A corrupt `state.json` kept aside.
    pub corrupt_kept: Option<PathBuf>,
    /// A newer version's file: read-only, never rewritten.
    pub newer: Option<u64>,
    pub migrated: bool,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
}

fn stamp() -> String {
    crate::proto::rfc3339(std::time::SystemTime::now())
        .replace([':', '-'], "")
        .replace('.', "")
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn file(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    /// Backups, newest first.
    pub fn backups(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.backup_dir())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("state-") && n.ends_with(".json"))
            })
            .collect();
        v.sort();
        v.reverse();
        v
    }

    /// Loads `state.json`, recovering from the newest valid backup when it is corrupt.
    pub fn load(&self) -> Loaded {
        let mut out = Loaded::default();
        let file = self.file();
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
            Err(e) => {
                out.problems
                    .push(format!("cannot read {}: {e}", file.display()));
                return out;
            }
        };
        match parse(&text) {
            Ok((d, migrated)) => {
                if migrated {
                    // The old version stays as a backup before anything is rewritten.
                    if let Err(e) = self.backup_text(&text, "pre-migration") {
                        out.problems
                            .push(format!("cannot back up before migrating: {e}"));
                    }
                }
                out.doc = Some(d);
                out.migrated = migrated;
            }
            Err(Bad::Newer(v)) => {
                out.newer = Some(v);
                out.problems.push(format!(
                    "state.json is from a newer Subak (schema version {v}); it is not changed"
                ));
            }
            Err(Bad::Corrupt(why)) => {
                out.problems.push(format!("state.json is corrupt: {why}"));
                let kept = self.dir.join(format!("state.corrupt-{}.json", stamp()));
                if std::fs::rename(&file, &kept).is_ok() {
                    out.corrupt_kept = Some(kept);
                }
                for b in self.backups() {
                    if let Ok(t) = std::fs::read_to_string(&b)
                        && let Ok((d, _)) = parse(&t)
                    {
                        out.doc = Some(d);
                        out.recovered_from = Some(b);
                        break;
                    }
                }
            }
        }
        out
    }

    fn backup_text(&self, text: &str, tag: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(self.backup_dir())?;
        let mut n = 0;
        let path = loop {
            let name = if n == 0 {
                format!("state-{}-{tag}.json", stamp())
            } else {
                format!("state-{}-{tag}-{n}.json", stamp())
            };
            let p = self.backup_dir().join(name);
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        std::fs::write(&path, text)?;
        // Bounded: the newest BACKUPS stay.
        for old in self.backups().into_iter().skip(BACKUPS) {
            let _ = std::fs::remove_file(old);
        }
        Ok(path)
    }

    /// Copies the current `state.json` (if any) to `backups\` (once per Host run).
    pub fn backup_current(&self, tag: &str) -> std::io::Result<Option<PathBuf>> {
        match std::fs::read_to_string(self.file()) {
            Ok(t) => self.backup_text(&t, tag).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Writes a document atomically (tmp, flush, replace with write-through).
    pub fn write(&self, doc: &Value) -> std::io::Result<u64> {
        use std::io::Write;
        std::fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(doc).expect("JSON") + "\n";
        if text.len() as u64 > MAX_BYTES {
            return Err(std::io::Error::other("the state is larger than the limit"));
        }
        let tmp = self.dir.join("state.json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        replace(&tmp, &self.file())?;
        Ok(text.len() as u64)
    }
}

#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    crate::win::replace_file(from, to)
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

// ---------------------------------------------------------------------------------------------
// The CLI (`subak state …`). Reading never changes a file; changes go through a running Host
// (it owns the file) or, without one, are written here the same way (backup, atomic replace).

use crate::cli::{Args, Cmd};
use crate::output::{CliError, Code};

fn store_for(a: &Args) -> Result<Store, CliError> {
    a.state_dir
        .as_ref()
        .map(PathBuf::from)
        .or_else(default_dir)
        .map(Store::new)
        .ok_or_else(|| {
            CliError::new(
                Code::Io,
                "cannot find the state folder (%LOCALAPPDATA% is not set)",
            )
        })
}

/// A saved Terminal for people: ids, names, counts, and what a restore would not find.
pub fn summary(t: &Value) -> Value {
    let mut panes = 0;
    let mut missing = Vec::new();
    for w in t["windows"].as_array().into_iter().flatten() {
        for p in w["panes"].as_array().into_iter().flatten() {
            panes += 1;
            if let Some(cwd) = p["cwd"].as_str()
                && !Path::new(cwd).is_dir()
            {
                missing.push(json!({"pane_id": p["id"], "missing": "cwd", "path": cwd}));
            }
            if let Some(prog) = p["command"]["program"].as_str() {
                let path = Path::new(prog);
                if path.is_absolute() && !path.is_file() {
                    missing.push(json!({"pane_id": p["id"], "missing": "program", "path": prog}));
                }
            }
        }
    }
    json!({
        "id": t["id"], "name": t["name"], "restore": t["restore"],
        "windows": t["windows"].as_array().map_or(0, Vec::len), "panes": panes,
        "missing": missing,
    })
}

/// Reads `state.json` without changing anything (no corrupt file is moved, nothing migrated).
fn inspect(store: &Store) -> Value {
    let file = store.file();
    let backups: Vec<String> = store
        .backups()
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    let base = json!({"path": file.display().to_string(), "backups": backups});
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut v = base;
            v["exists"] = json!(false);
            v["valid"] = json!(true);
            v["terminals"] = json!([]);
            return v;
        }
        Err(e) => {
            let mut v = base;
            v["exists"] = json!(true);
            v["valid"] = json!(false);
            v["problem"] = json!(format!("cannot read it: {e}"));
            return v;
        }
    };
    let mut v = base;
    v["exists"] = json!(true);
    v["bytes"] = json!(text.len());
    match parse(&text) {
        Ok((d, migrated)) => {
            v["valid"] = json!(true);
            v["schema_version"] = json!(VERSION);
            v["migrates_from"] = if migrated {
                serde_json::from_str::<Value>(&text)
                    .map_or(Value::Null, |o| o["schema_version"].clone())
            } else {
                Value::Null
            };
            v["written_at"] = d["written_at"].clone();
            v["terminals"] = json!(
                d["terminals"]
                    .as_array()
                    .map(|t| t.iter().map(summary).collect::<Vec<_>>())
                    .unwrap_or_default()
            );
        }
        Err(Bad::Newer(n)) => {
            v["valid"] = json!(true);
            v["schema_version"] = json!(n);
            v["read_only"] = json!(true);
            v["terminals"] = json!([]);
        }
        Err(Bad::Corrupt(why)) => {
            v["valid"] = json!(false);
            v["problem"] = json!(why);
        }
    }
    v
}

fn human_show(v: &Value) -> String {
    let mut out = v["path"].as_str().unwrap_or("").to_string();
    if v["exists"] == false {
        out.push_str(": no saved state yet\n");
        return out;
    }
    if v["valid"] == false {
        out.push_str(&format!(
            ": CORRUPT ({})\n",
            v["problem"].as_str().unwrap_or("")
        ));
        return out;
    }
    out.push_str(&format!(
        " (schema {}, written {})\n",
        v["schema_version"],
        v["written_at"].as_str().unwrap_or("?")
    ));
    for t in v["terminals"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {} ({}) restore {} · {} window(s), {} pane(s){}\n",
            t["name"].as_str().unwrap_or(""),
            t["id"].as_str().unwrap_or(""),
            t["restore"].as_str().unwrap_or(""),
            t["windows"],
            t["panes"],
            if t["missing"].as_array().is_some_and(|m| !m.is_empty()) {
                " · something it ran is missing"
            } else {
                ""
            }
        ));
    }
    out.push_str(&format!(
        "  {} backup(s)\n",
        v["backups"].as_array().map_or(0, Vec::len)
    ));
    out
}

#[cfg(windows)]
fn host_session(a: &Args) -> Option<crate::client::Session> {
    let ep = crate::client::endpoint(a).ok()?;
    crate::client::Session::try_open(&ep, std::time::Duration::from_millis(300)).ok()
}

#[cfg(not(windows))]
fn host_session(_a: &Args) -> Option<()> {
    None
}

/// Edits `state.json` without a Host: backup, change, seal, atomic replace.
fn edit_file(
    store: &Store,
    dry_run: bool,
    f: impl FnOnce(&mut Vec<Value>) -> Result<Value, CliError>,
) -> Result<Value, CliError> {
    let text = std::fs::read_to_string(store.file()).map_err(|e| {
        CliError::new(
            Code::TargetNotFound,
            format!("no saved state at `{}` ({e})", store.file().display()),
        )
    })?;
    let (d, _) = parse(&text).map_err(|b| match b {
        Bad::Newer(v) => CliError::new(
            Code::StateFromNewerVersion,
            format!("state.json is from a newer Subak (schema version {v}); it is not changed"),
        ),
        Bad::Corrupt(w) => CliError::new(
            Code::StateCorrupt,
            format!("state.json is corrupt ({w}); nothing was changed"),
        )
        .hint("`subak state rollback` restores the newest backup"),
    })?;
    let mut terminals = d["terminals"].as_array().cloned().unwrap_or_default();
    let result = f(&mut terminals)?;
    if dry_run {
        return Ok(json!({"dry_run": true, "would": result}));
    }
    store
        .backup_current("edit")
        .map_err(|e| CliError::new(Code::Io, format!("cannot back up: {e}")))?;
    store
        .write(&seal(
            &terminals,
            &crate::proto::rfc3339(std::time::SystemTime::now()),
        ))
        .map_err(|e| CliError::new(Code::Io, format!("cannot write state.json: {e}")))?;
    Ok(result)
}

fn find(terminals: &[Value], target: &str) -> Result<usize, CliError> {
    let key = target.to_lowercase();
    terminals
        .iter()
        .position(|t| {
            t["id"] == target || t["name"].as_str().is_some_and(|n| n.to_lowercase() == key)
        })
        .ok_or_else(|| {
            CliError::new(
                Code::TargetNotFound,
                format!("no saved Terminal `{target}`"),
            )
        })
}

/// `subak state show|check|backups|forget|policy|rollback`.
pub fn run(cmd: Cmd, a: &Args) -> Result<(Value, String), CliError> {
    let store = store_for(a)?;
    let arg = |i: usize| a.positional.get(i).cloned().unwrap_or_default();
    match cmd {
        Cmd::StateShow => {
            let v = inspect(&store);
            let h = human_show(&v);
            Ok((v, h))
        }
        Cmd::StateCheck => {
            let v = inspect(&store);
            if v["valid"] == false {
                return Err(CliError::new(
                    Code::StateCorrupt,
                    format!("{} is corrupt: {}", v["path"].as_str().unwrap_or(""), v["problem"].as_str().unwrap_or("")),
                )
                .hint("a Host keeps it aside and uses the newest valid backup; `subak state rollback` restores one now"));
            }
            if v["read_only"] == true {
                return Err(CliError::new(
                    Code::StateFromNewerVersion,
                    format!(
                        "state.json is from a newer Subak (schema version {})",
                        v["schema_version"]
                    ),
                ));
            }
            let h = human_show(&v);
            Ok((v, h))
        }
        Cmd::StateBackups => {
            let v = json!({"dir": store.backup_dir().display().to_string(),
                "backups": store.backups().iter().map(|p| {
                    let ok = std::fs::read_to_string(p).ok().is_some_and(|t| parse(&t).is_ok());
                    json!({"name": p.file_name().and_then(|n| n.to_str()), "valid": ok,
                        "bytes": std::fs::metadata(p).map_or(0, |m| m.len())})
                }).collect::<Vec<_>>()});
            let h = v["backups"].as_array().map_or(String::new(), |b| {
                b.iter()
                    .map(|x| {
                        format!(
                            "{}{}\n",
                            x["name"].as_str().unwrap_or(""),
                            if x["valid"] == true { "" } else { " (invalid)" }
                        )
                    })
                    .collect()
            });
            Ok((v, h))
        }
        Cmd::StateForget | Cmd::StatePolicy => {
            let target = arg(0);
            let policy = arg(1);
            if cmd == Cmd::StatePolicy && !matches!(policy.as_str(), "ask" | "auto" | "never") {
                return Err(CliError::usage(format!(
                    "`{policy}` is not ask, auto or never"
                )));
            }
            #[cfg(windows)]
            if let Some(mut s) = host_session(a) {
                // The Host owns state.json while it runs.
                let (method, params) = if cmd == Cmd::StateForget {
                    (
                        "state.forget",
                        json!({"target": target, "dry_run": a.dry_run}),
                    )
                } else {
                    (
                        "state.set_restore",
                        json!({"target": target, "restore": policy, "dry_run": a.dry_run,
                            "claude_resume": if a.claude_resume { json!(true) } else { Value::Null }}),
                    )
                };
                let r = s.request(method, &params)?;
                let h = format!("{}\n", r);
                return Ok((json!({"via": "host", "result": r}), h));
            }
            let r = edit_file(&store, a.dry_run, |ts| {
                let i = find(ts, &target)?;
                if cmd == Cmd::StateForget {
                    Ok(json!({"forgotten": summary(&ts.remove(i))}))
                } else {
                    ts[i]["restore"] = json!(policy);
                    if a.claude_resume {
                        for w in ts[i]["windows"].as_array_mut().into_iter().flatten() {
                            for p in w["panes"].as_array_mut().into_iter().flatten() {
                                if p["claude"].is_object() {
                                    p["claude"]["resume"] = json!(true);
                                }
                            }
                        }
                    }
                    Ok(
                        json!({"terminal_id": ts[i]["id"], "name": ts[i]["name"], "restore": policy}),
                    )
                }
            })?;
            let h = format!("{r}\n");
            Ok((json!({"via": "file", "result": r}), h))
        }
        Cmd::StateRollback => {
            #[cfg(windows)]
            if host_session(a).is_some() {
                return Err(CliError::new(
                    Code::HostAlreadyRunning,
                    "a Host is running and owns state.json; nothing was changed",
                )
                .hint("stop it first (`subak host stop`); its Terminals keep running only while it does"));
            }
            let backups = store.backups();
            let chosen = match a.positional.first() {
                Some(name) => backups
                    .iter()
                    .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(name.as_str()))
                    .cloned()
                    .ok_or_else(|| {
                        CliError::new(Code::TargetNotFound, format!("no backup `{name}`"))
                    })?,
                None => backups
                    .iter()
                    .find(|p| {
                        std::fs::read_to_string(p)
                            .ok()
                            .is_some_and(|t| parse(&t).is_ok())
                    })
                    .cloned()
                    .ok_or_else(|| CliError::new(Code::TargetNotFound, "no valid backup"))?,
            };
            let text = std::fs::read_to_string(&chosen)
                .map_err(|e| CliError::new(Code::Io, e.to_string()))?;
            let (d, _) = parse(&text).map_err(|_| {
                CliError::new(
                    Code::StateCorrupt,
                    "that backup is not valid; nothing was changed",
                )
            })?;
            let name = chosen
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let terminals = d["terminals"]
                .as_array()
                .map(|t| t.iter().map(summary).collect::<Vec<_>>())
                .unwrap_or_default();
            if a.dry_run {
                return Ok((
                    json!({"dry_run": true, "would_restore": name, "terminals": terminals}),
                    format!("would restore {name}\n"),
                ));
            }
            // The current file (even a corrupt one) is kept first.
            if store.file().exists() {
                let kept = store
                    .dir
                    .join(format!("state.before-rollback-{}.json", stamp()));
                std::fs::copy(store.file(), &kept)
                    .map_err(|e| CliError::new(Code::Io, e.to_string()))?;
            }
            store
                .write(&seal(
                    d["terminals"].as_array().map_or(&[][..], Vec::as_slice),
                    &crate::proto::rfc3339(std::time::SystemTime::now()),
                ))
                .map_err(|e| CliError::new(Code::Io, e.to_string()))?;
            Ok((
                json!({"restored": name, "terminals": terminals}),
                format!("restored {name}\n"),
            ))
        }
        _ => Err(CliError::new(Code::Internal, "not a state command")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/contracts/examples/subak-state-v1")
                .join(name),
        )
        .unwrap()
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subak-state-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn crc_is_zlibs_and_seals_the_canonical_body() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926, "the IEEE check value");
        let d = seal(&[], "2026-11-11T00:00:00Z");
        assert!(parse(&d.to_string()).is_ok());
        let mut edited = d.clone();
        edited["written_at"] = json!("2026-11-12T00:00:00Z");
        assert_eq!(
            parse(&edited.to_string()),
            Err(Bad::Corrupt(
                "the checksum does not match (a torn or edited file)".into()
            ))
        );
        // Key order and whitespace do not matter (canonical form).
        let pretty = serde_json::to_string_pretty(&d).unwrap();
        assert!(parse(&pretty).is_ok());
    }

    #[test]
    fn the_v1_draft_migrates_to_v2_without_secrets_or_env_values() {
        let (d, migrated) = parse(&fixture("two-terminals.json")).expect("the Step 03 fixture");
        assert!(migrated);
        assert_eq!(d["schema_version"], 2);
        let t = &d["terminals"][0];
        assert!(matches!(
            t["restore"].as_str(),
            Some("ask" | "auto" | "never")
        ));
        let p = &t["windows"][0]["panes"][0];
        assert!(p["env"].is_array(), "names only: {p}");
        assert!(p.get("env_secret_refs").is_none());
        assert_eq!(p["history_limit"], 2000);
        // A newer version is refused for writing, but recognized.
        let mut newer = seal(&[], "2026-11-11T00:00:00Z");
        newer["schema_version"] = json!(9);
        newer["crc32"] = json!(checksum(&newer));
        assert_eq!(parse(&newer.to_string()), Err(Bad::Newer(9)));
        // Structure errors are corruption.
        for bad in [
            json!({"schema": "other"}),
            json!({"schema": SCHEMA, "schema_version": 0}),
        ] {
            assert!(matches!(parse(&bad.to_string()), Err(Bad::Corrupt(_))));
        }
        let mut dup = d.clone();
        let t0 = dup["terminals"][0].clone();
        dup["terminals"].as_array_mut().unwrap().push(t0);
        let dup = seal(dup["terminals"].as_array().unwrap(), "2026-11-11T00:00:00Z");
        assert!(matches!(parse(&dup.to_string()), Err(Bad::Corrupt(m)) if m.contains("twice")));
    }

    #[test]
    fn writes_are_atomic_backups_bounded_and_corruption_recovers() {
        let s = Store::new(dir("store"));
        let (d, _) = parse(&fixture("two-terminals.json")).unwrap();
        let doc = seal(d["terminals"].as_array().unwrap(), "2026-11-11T00:00:00Z");
        s.write(&doc).unwrap();
        assert!(
            !s.dir.join("state.json.tmp").exists(),
            "the temporary file was moved"
        );
        let l = s.load();
        assert_eq!(l.doc.as_ref().unwrap()["terminals"], doc["terminals"]);
        // A crash during a write leaves a tmp file behind: ignored.
        std::fs::write(s.dir.join("state.json.tmp"), "{ torn").unwrap();
        assert!(s.load().doc.is_some());
        // Backups: at most 5.
        for _ in 0..8 {
            s.backup_current("start").unwrap();
        }
        assert_eq!(s.backups().len(), BACKUPS);
        // A torn state.json: kept aside, the newest backup is used.
        let text = std::fs::read_to_string(s.file()).unwrap();
        std::fs::write(s.file(), &text[..text.len() / 2]).unwrap();
        let l = s.load();
        assert!(l.corrupt_kept.as_ref().is_some_and(|p| p.exists()));
        assert!(l.recovered_from.is_some());
        assert_eq!(l.doc.unwrap()["terminals"], doc["terminals"]);
        assert!(!l.problems.is_empty());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn huge_session_trees_stay_bounded_and_fast() {
        // 200 Terminals × 5 windows × 4 panes = 4 000 panes.
        let mut terminals = Vec::new();
        let mut n = 0u64;
        let mut id = |k: char| {
            n += 1;
            format!("{k}-{n:012x}")
        };
        for ti in 0..200 {
            let mut windows = Vec::new();
            for wi in 0..5 {
                let panes: Vec<Value> = (0..4)
                    .map(|pi| {
                        json!({"id": id('p'), "index": pi, "cwd": "C:\\\\w", "command": null,
                        "command_line": null, "command_withheld": false, "restart_command": null,
                        "env": [], "env_unset": [], "history_limit": 2000, "title": null,
                        "claude": null, "scrollback": {"persisted": false}})
                    })
                    .collect();
                let children: Vec<Value> = panes
                    .iter()
                    .map(|p| json!({"size": 0.25, "node": {"type": "pane", "pane": p["id"]}}))
                    .collect();
                windows.push(
                    json!({"id": id('w'), "index": wi, "name": "cmd", "automatic_name": true,
                    "active_pane": panes[0]["id"], "size": {"cols": 80, "rows": 24},
                    "layout": {"type": "split", "direction": "horizontal", "children": children},
                    "panes": panes}),
                );
            }
            terminals.push(json!({"id": id('t'), "name": format!("t{ti}"), "created_at": "2026-11-11T00:00:00Z",
                "profile": null, "restore": "ask", "remain_on_exit": false,
                "active_window": windows[0]["id"], "windows": windows}));
        }
        let s = Store::new(dir("huge"));
        let t = std::time::Instant::now();
        let bytes = s.write(&seal(&terminals, "2026-11-11T00:00:00Z")).unwrap();
        let l = s.load();
        let took = t.elapsed();
        assert!(l.doc.is_some(), "{:?}", l.problems);
        assert!(bytes < MAX_BYTES / 4, "{bytes} bytes");
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}
