//! Step 60 — the opt-in durable stores (NOEUL-FR-051, NOEUL-FR-066, NOEUL-SEC-005, ADR 0074):
//! capture history on disk and the pin backup. Both are **off by default**; with them off Noeul
//! writes nothing here and keeps history in memory only.
//!
//! Layout under Noeul's user-only data folder (`%LOCALAPPDATA%\mouv\noeul\`, ADR 0006):
//!
//! ```text
//! history\index.json        schema "noeul.history" v1: id, rect, time, file, size, SHA-256
//! history\<id>.png          one capture (no metadata, NOEUL-SEC-006)
//! pins\backup.json          schema "noeul.pins" v1: each pin's place, zoom, turn, opacity, group…
//! pins\img\<sha256>.png     the pins' pictures, by content (two pins of one picture: one file)
//! ```
//!
//! **Integrity.** Every write goes to a temporary file in the same folder and is renamed into
//! place, so a crash leaves the old file or the new one, never half of one (a leftover `*.tmp`
//! is ignored and removed on the next write). Every image is checked against its SHA-256 before
//! it is used; a damaged entry is skipped and reported, never half-restored. An index that is not
//! JSON is set aside as `<name>.corrupt-<time>` (kept for the user, never parsed again) and
//! reported; one from a newer Noeul (`schema_version` above 1) is left untouched and the store
//! is read-only until that Noeul is used again.
//!
//! **Privacy.** Pictures are the pixels the user captured, nothing else: no window titles, paths
//! or user names, here or in the indexes. A pin's annotations are stored **drawn into its
//! picture**: a redaction never sits on disk above the pixels it hides, and text annotations are
//! never stored as text (ADR 0074).

use crate::frame::Rect;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const HISTORY_SCHEMA: &str = "noeul.history";
pub const PINS_SCHEMA: &str = "noeul.pins";
pub const VERSION: u64 = 1;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The content key of a picture: SHA-256 over its size and pixels (equal pictures, one file).
pub fn picture_key(width: u32, height: u32, pixels: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(width.to_le_bytes());
    h.update(height.to_le_bytes());
    h.update(pixels);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes `bytes` to `path` through a temporary file beside it and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How an index file was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Index {
    Missing,
    Read(Value),
    /// Not JSON or not this schema: set aside under this name.
    Corrupt(String),
    /// A newer `schema_version`: left as it is; the store is read-only.
    Newer(u64),
}

/// Reads `path` as an index of `schema`.
pub fn read_index(path: &Path, schema: &str) -> Index {
    let Ok(bytes) = std::fs::read(path) else {
        return Index::Missing;
    };
    let v: Option<Value> = serde_json::from_slice(&bytes).ok();
    match v {
        Some(v) if v["schema"] == schema => match v["schema_version"].as_u64() {
            Some(VERSION) => Index::Read(v),
            Some(n) if n > VERSION => Index::Newer(n),
            _ => set_aside(path),
        },
        _ => set_aside(path),
    }
}

fn set_aside(path: &Path) -> Index {
    let name = format!(
        "{}.corrupt-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("index"),
        now_secs()
    );
    let _ = std::fs::rename(path, path.with_file_name(&name));
    Index::Corrupt(name)
}

// ---------------------------------------------------------------------------------------------
// History on disk (NOEUL-FR-051).

/// One capture kept on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskEntry {
    pub id: u64,
    pub rect: Rect,
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub bytes: u64,
    pub sha256: String,
}

impl DiskEntry {
    fn file(&self) -> String {
        format!("{}.png", self.id)
    }

    fn to_json(&self) -> Value {
        json!({"id": self.id, "rect": self.rect.to_json(), "at": self.at, "file": self.file(),
               "bytes": self.bytes, "sha256": self.sha256})
    }

    fn from_json(v: &Value) -> Option<DiskEntry> {
        Some(DiskEntry {
            id: v["id"].as_u64()?,
            rect: Rect::from_json(&v["rect"])?,
            at: v["at"].as_u64()?,
            bytes: v["bytes"].as_u64()?,
            sha256: v["sha256"].as_str()?.to_string(),
        })
    }
}

/// The limits of the history (`[history] count`, `keep_days`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub count: usize,
    pub keep_days: u64,
}

/// `history\` and its index.
#[derive(Debug)]
pub struct HistoryDir {
    dir: PathBuf,
    entries: Vec<DiskEntry>,
    read_only: bool,
    pub problems: Vec<String>,
}

impl HistoryDir {
    /// Opens (reads) the history folder under `data_dir`; nothing is created until a write.
    pub fn open(data_dir: &Path) -> HistoryDir {
        let dir = data_dir.join("history");
        let mut h = HistoryDir {
            dir,
            entries: Vec::new(),
            read_only: false,
            problems: Vec::new(),
        };
        match read_index(&h.index(), HISTORY_SCHEMA) {
            Index::Missing => {}
            Index::Read(v) => {
                for e in v["entries"].as_array().into_iter().flatten() {
                    match DiskEntry::from_json(e) {
                        Some(e) => h.entries.push(e),
                        None => h.problems.push("a damaged history entry was skipped".into()),
                    }
                }
            }
            Index::Corrupt(name) => h.problems.push(format!(
                "the history index was damaged and was set aside as {name}; the history starts empty"
            )),
            Index::Newer(n) => {
                h.read_only = true;
                h.problems.push(format!(
                    "the history was written by a newer Noeul (version {n}); it is left as it is"
                ));
            }
        }
        h
    }

    fn index(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn entries(&self) -> &[DiskEntry] {
        &self.entries
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// The next free id (after every id on disk).
    pub fn next_id(&self) -> u64 {
        self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1
    }

    fn save_index(&self) -> std::io::Result<()> {
        let v = json!({"schema": HISTORY_SCHEMA, "schema_version": VERSION,
            "entries": self.entries.iter().map(DiskEntry::to_json).collect::<Vec<_>>()});
        write_atomic(
            &self.index(),
            &serde_json::to_vec_pretty(&v).unwrap_or_default(),
        )
    }

    /// Adds one capture (its PNG) and applies the limits; returns the ids that fell out.
    pub fn add(
        &mut self,
        id: u64,
        rect: Rect,
        png: &[u8],
        at: u64,
        limits: Limits,
    ) -> std::io::Result<Vec<u64>> {
        if self.read_only {
            return Err(std::io::Error::other(
                "the history is read-only (newer version)",
            ));
        }
        let e = DiskEntry {
            id,
            rect,
            at,
            bytes: png.len() as u64,
            sha256: sha256_hex(png),
        };
        write_atomic(&self.dir.join(e.file()), png)?;
        self.entries.retain(|x| x.id != id);
        self.entries.push(e);
        let gone = self.prune_entries(limits, at);
        self.save_index()?;
        Ok(gone)
    }

    fn prune_entries(&mut self, limits: Limits, now: u64) -> Vec<u64> {
        let oldest = now.saturating_sub(limits.keep_days.saturating_mul(86_400));
        let mut gone = Vec::new();
        self.entries.sort_by_key(|e| e.id);
        while self.entries.len() > limits.count.max(1) {
            gone.push(self.entries.remove(0).id);
        }
        self.entries.retain(|e| {
            let keep = e.at >= oldest;
            if !keep {
                gone.push(e.id);
            }
            keep
        });
        for id in &gone {
            let _ = std::fs::remove_file(self.dir.join(format!("{id}.png")));
        }
        gone
    }

    /// Applies the limits now (at start and when they change); returns the ids removed.
    pub fn prune(&mut self, limits: Limits, now: u64) -> std::io::Result<Vec<u64>> {
        if self.read_only {
            return Ok(Vec::new());
        }
        let gone = self.prune_entries(limits, now);
        if !gone.is_empty() {
            self.save_index()?;
        }
        Ok(gone)
    }

    /// The PNG of entry `id`, checked against its SHA-256.
    pub fn read(&self, id: u64) -> Result<Vec<u8>, String> {
        let e = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("no history entry {id}"))?;
        let bytes = std::fs::read(self.dir.join(e.file()))
            .map_err(|_| format!("history entry {id}: its picture is missing"))?;
        if sha256_hex(&bytes) != e.sha256 {
            return Err(format!("history entry {id}: its picture is damaged"));
        }
        Ok(bytes)
    }

    /// Removes one entry.
    pub fn forget(&mut self, id: u64) -> std::io::Result<bool> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() == before {
            return Ok(false);
        }
        let _ = std::fs::remove_file(self.dir.join(format!("{id}.png")));
        self.save_index()?;
        Ok(true)
    }

    /// Removes every picture, the index and any leftover temporary or set-aside file.
    pub fn clear(&mut self) -> std::io::Result<usize> {
        let n = self.entries.len();
        self.entries.clear();
        if self.dir.is_dir() {
            for f in std::fs::read_dir(&self.dir)?.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if name.ends_with(".png")
                    || name.ends_with(".tmp")
                    || name.starts_with("index.json")
                {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        self.read_only = false;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------------------------
// Pin backup (NOEUL-FR-066).

/// One backed-up pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRecord {
    /// Stable across restarts: a restore skips a record whose pin is open (no duplicates).
    pub key: String,
    pub group: String,
    pub x: i32,
    pub y: i32,
    pub zoom: u32,
    pub rotation: u32,
    pub mirrored: bool,
    pub opacity: u32,
    pub thumbnail: Option<(u32, u32)>,
    pub visible: bool,
    pub source: String,
    /// The picture has its annotations drawn in.
    pub annotated: bool,
    pub width: u32,
    pub height: u32,
    /// The picture's content key ([`picture_key`]); its file is `img\<image>.png`.
    pub image: String,
}

impl PinRecord {
    pub fn to_json(&self) -> Value {
        json!({"key": self.key, "group": self.group, "x": self.x, "y": self.y, "zoom": self.zoom,
            "rotation": self.rotation, "mirrored": self.mirrored, "opacity": self.opacity,
            "thumbnail": self.thumbnail.map(|(w, h)| json!({"width": w, "height": h})),
            "visible": self.visible, "source": self.source, "annotated": self.annotated,
            "width": self.width, "height": self.height, "image": self.image})
    }

    /// Reads a record; fields added after version 1 get their defaults (the migration rule).
    pub fn from_json(v: &Value) -> Option<PinRecord> {
        let u = |k: &str| v[k].as_u64().and_then(|n| u32::try_from(n).ok());
        let i = |k: &str| v[k].as_i64().and_then(|n| i32::try_from(n).ok());
        let image = v["image"].as_str()?.to_string();
        if image.len() != 64 || !image.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(PinRecord {
            key: v["key"].as_str()?.to_string(),
            group: v["group"].as_str().unwrap_or("Default").to_string(),
            x: i("x")?,
            y: i("y")?,
            zoom: u("zoom").unwrap_or(100),
            rotation: u("rotation").unwrap_or(0),
            mirrored: v["mirrored"].as_bool().unwrap_or(false),
            opacity: u("opacity").unwrap_or(100),
            thumbnail: v["thumbnail"].as_object().and_then(|t| {
                Some((
                    u32::try_from(t.get("width")?.as_u64()?).ok()?,
                    u32::try_from(t.get("height")?.as_u64()?).ok()?,
                ))
            }),
            visible: v["visible"].as_bool().unwrap_or(true),
            source: v["source"].as_str().unwrap_or("capture").to_string(),
            annotated: v["annotated"].as_bool().unwrap_or(false),
            width: u("width")?,
            height: u("height")?,
            image,
        })
    }
}

/// `pins\`.
#[derive(Debug)]
pub struct PinStore {
    dir: PathBuf,
}

/// What a backup holds, as read.
#[derive(Debug, Default)]
pub struct Read {
    pub saved_at: Option<u64>,
    pub records: Vec<PinRecord>,
    pub problems: Vec<String>,
    pub read_only: bool,
}

impl PinStore {
    pub fn new(data_dir: &Path) -> PinStore {
        PinStore {
            dir: data_dir.join("pins"),
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join("backup.json")
    }

    fn image_path(&self, key: &str) -> PathBuf {
        self.dir.join("img").join(format!("{key}.png"))
    }

    pub fn exists(&self) -> bool {
        self.file().is_file()
    }

    pub fn read(&self) -> Read {
        let mut r = Read::default();
        match read_index(&self.file(), PINS_SCHEMA) {
            Index::Missing => {}
            Index::Read(v) => {
                r.saved_at = v["saved_at"].as_u64();
                for p in v["pins"].as_array().into_iter().flatten() {
                    match PinRecord::from_json(p) {
                        Some(rec) => r.records.push(rec),
                        None => r.problems.push("a damaged pin entry was skipped".into()),
                    }
                }
            }
            Index::Corrupt(name) => r.problems.push(format!(
                "the pin backup was damaged and was set aside as {name}; nothing can be restored from it"
            )),
            Index::Newer(n) => {
                r.read_only = true;
                r.problems.push(format!(
                    "the pin backup was written by a newer Noeul (version {n}); it is left as it is"
                ));
            }
        }
        r
    }

    /// Writes the backup: the pictures not yet on disk (`png` makes one on demand), then the
    /// index (the commit point), then removes the pictures no record uses.
    pub fn save(
        &self,
        records: &[PinRecord],
        png: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> Result<usize, String> {
        if self.read().read_only {
            return Err("the pin backup is from a newer Noeul; it is left as it is".into());
        }
        let mut written = 0;
        for r in records {
            let path = self.image_path(&r.image);
            if !path.is_file() {
                let bytes = png(&r.image)?;
                write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
                written += 1;
            }
        }
        let v = json!({"schema": PINS_SCHEMA, "schema_version": VERSION, "saved_at": now_secs(),
            "pins": records.iter().map(PinRecord::to_json).collect::<Vec<_>>()});
        write_atomic(
            &self.file(),
            &serde_json::to_vec_pretty(&v).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
        let keep: std::collections::BTreeSet<&str> =
            records.iter().map(|r| r.image.as_str()).collect();
        if let Ok(dir) = std::fs::read_dir(self.dir.join("img")) {
            for f in dir.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let stem = name.strip_suffix(".png").unwrap_or(&name);
                if !keep.contains(stem) || name.ends_with(".tmp") {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        Ok(written)
    }

    /// The picture of `r`, as PNG, checked: its file must decode to the key it is stored under
    /// (the caller decodes and compares; here the file must exist and be non-empty).
    pub fn image(&self, r: &PinRecord) -> Result<Vec<u8>, String> {
        let bytes = std::fs::read(self.image_path(&r.image))
            .map_err(|_| format!("pin {}: its picture is missing", short(&r.key)))?;
        if bytes.is_empty() {
            return Err(format!("pin {}: its picture is empty", short(&r.key)));
        }
        Ok(bytes)
    }

    /// Removes the backup and its pictures.
    pub fn clear(&self) -> std::io::Result<usize> {
        let n = self.read().records.len();
        if self.dir.is_dir() {
            std::fs::remove_dir_all(&self.dir)?;
        }
        Ok(n)
    }
}

/// A key shortened for messages.
pub fn short(key: &str) -> &str {
    &key[..key.len().min(8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("noeul-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn lim(count: usize, keep_days: u64) -> Limits {
        Limits { count, keep_days }
    }

    /// NOEUL-FR-051: entries by count and age; each picture checked; forget and clear.
    #[test]
    fn disk_history_keeps_within_its_limits_and_checks_every_picture() {
        let d = tmp("hist");
        let mut h = HistoryDir::open(&d);
        assert!(
            h.entries().is_empty() && !h.dir().exists(),
            "nothing written on open"
        );
        let now = 1_000_000;
        for id in 1..=5 {
            h.add(
                id,
                Rect::new(0, 0, 4, 4),
                format!("png{id}").as_bytes(),
                now,
                lim(3, 7),
            )
            .unwrap();
        }
        let ids: Vec<u64> = h.entries().iter().map(|e| e.id).collect();
        assert_eq!(ids, [3, 4, 5], "the oldest fell out");
        assert!(!h.dir().join("1.png").exists());
        assert_eq!(h.read(4).unwrap(), b"png4");
        // Reopened from disk; ids continue.
        let mut h = HistoryDir::open(&d);
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.next_id(), 6);
        // Age: eight days later everything is past `keep_days = 7`.
        assert_eq!(h.prune(lim(3, 7), now + 8 * 86_400).unwrap(), [3, 4, 5]);
        h.add(6, Rect::new(0, 0, 1, 1), b"six", now, lim(3, 7))
            .unwrap();
        // A damaged picture is refused, not used.
        std::fs::write(h.dir().join("6.png"), b"tampered").unwrap();
        assert!(h.read(6).unwrap_err().contains("damaged"));
        assert!(h.forget(6).unwrap());
        h.add(7, Rect::new(0, 0, 1, 1), b"seven", now, lim(3, 7))
            .unwrap();
        std::fs::write(h.dir().join("left.png.tmp"), b"x").unwrap();
        assert_eq!(h.clear().unwrap(), 1);
        assert_eq!(
            std::fs::read_dir(h.dir()).unwrap().count(),
            0,
            "nothing left"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_index_is_set_aside_and_a_newer_one_is_left_alone() {
        let d = tmp("corrupt");
        std::fs::create_dir_all(d.join("history")).unwrap();
        std::fs::write(d.join("history/index.json"), b"{ not json").unwrap();
        let h = HistoryDir::open(&d);
        assert!(h.entries().is_empty());
        assert!(h.problems[0].contains("set aside"), "{:?}", h.problems);
        let aside = std::fs::read_dir(d.join("history"))
            .unwrap()
            .flatten()
            .any(|f| {
                f.file_name()
                    .to_string_lossy()
                    .starts_with("index.json.corrupt-")
            });
        assert!(aside);
        // Newer: untouched, read-only.
        let newer =
            json!({"schema": HISTORY_SCHEMA, "schema_version": 2, "entries": []}).to_string();
        std::fs::write(d.join("history/index.json"), &newer).unwrap();
        let mut h = HistoryDir::open(&d);
        assert!(h.read_only());
        assert!(h.add(1, Rect::new(0, 0, 1, 1), b"x", 1, lim(5, 5)).is_err());
        assert_eq!(
            std::fs::read_to_string(d.join("history/index.json")).unwrap(),
            newer
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    fn rec(key: &str, image: &str) -> PinRecord {
        PinRecord {
            key: key.into(),
            group: "Default".into(),
            x: -2000,
            y: 40,
            zoom: 150,
            rotation: 90,
            mirrored: true,
            opacity: 60,
            thumbnail: None,
            visible: true,
            source: "capture".into(),
            annotated: false,
            width: 3,
            height: 2,
            image: image.into(),
        }
    }

    /// NOEUL-FR-066: pictures stored once by content, the index last, unused pictures removed;
    /// a record round-trips; a crash before the rename leaves the old backup.
    #[test]
    fn the_pin_backup_dedupes_pictures_and_commits_atomically() {
        let d = tmp("pins");
        let s = PinStore::new(&d);
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let mut made = Vec::new();
        let mut png = |k: &str| {
            made.push(k.to_string());
            Ok(format!("png-{k}").into_bytes())
        };
        let n = s
            .save(&[rec("k1", &a), rec("k2", &a), rec("k3", &b)], &mut png)
            .unwrap();
        assert_eq!((n, made.len()), (2, 2), "two pins of one picture: one file");
        let r = s.read();
        assert_eq!(r.records, [rec("k1", &a), rec("k2", &a), rec("k3", &b)]);
        assert!(r.problems.is_empty());
        // Saved again: no picture re-encoded; dropping k3 removes its picture.
        let mut none = |_: &str| -> Result<Vec<u8>, String> { panic!("re-encoded") };
        assert_eq!(s.save(&[rec("k1", &a)], &mut none).unwrap(), 0);
        assert!(!d.join(format!("pins/img/{b}.png")).exists());
        // A crash mid-save: a temporary index next to the real one changes nothing.
        std::fs::write(d.join("pins/backup.json.tmp"), b"{half").unwrap();
        assert_eq!(s.read().records.len(), 1);
        // A damaged record is skipped and said.
        let mut v: Value =
            serde_json::from_slice(&std::fs::read(d.join("pins/backup.json")).unwrap()).unwrap();
        v["pins"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key": "bad", "image": "../../evil"}));
        std::fs::write(d.join("pins/backup.json"), v.to_string()).unwrap();
        let r = s.read();
        assert_eq!((r.records.len(), r.problems.len()), (1, 1));
        assert_eq!(s.clear().unwrap(), 1);
        assert!(!d.join("pins").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Migration: a version-1 record without the fields added later gets their defaults.
    #[test]
    fn a_minimal_record_reads_with_defaults() {
        let v =
            json!({"key": "k", "x": 1, "y": 2, "width": 3, "height": 4, "image": "c".repeat(64)});
        let r = PinRecord::from_json(&v).unwrap();
        assert_eq!(
            (r.zoom, r.opacity, r.rotation, r.visible, r.group.as_str()),
            (100, 100, 0, true, "Default")
        );
        assert_eq!(PinRecord::from_json(&r.to_json()), Some(r));
        assert_eq!(picture_key(1, 1, &[1, 2, 3, 4]).len(), 64);
        assert_ne!(
            picture_key(1, 1, &[1, 2, 3, 4]),
            picture_key(1, 1, &[1, 2, 3, 5])
        );
    }
}
