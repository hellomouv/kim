//! Step 60 (NOEUL-FR-050, NOEUL-FR-051, NOEUL-FR-066, NOEUL-SEC-001, NOEUL-SEC-005, ADR 0074):
//! the capture history and the pin backup in the running instance.
//!
//! - **History.** Successful captures keep their pictures in memory ([`Captures::keep`]),
//!   bounded by `[history] count` and `memory_mb`. With `[history] disk = "on"` they are also
//!   written to the user-only data folder ([`store::HistoryDir`]) and read back at the next
//!   start, within `keep_days`. Any of them can be pinned again (`noeul history pin`, the tray's
//!   "Pin last capture again"); `noeul history clear` forgets them all, in memory and on disk.
//! - **Backup.** With `[pins] backup = "on"`, a change to the pins arms a one-second timer and the
//!   pins are written once it fires ([`store::PinStore`]); nothing polls. At the next start the
//!   backed-up pins are **offered**, never restored by themselves: the tray's "Restore pins from
//!   last time" or `noeul pin restore`. Until then they stay in the backup (a new save keeps
//!   them). A restore skips pins that are open (no duplicates), checks every picture, moves a
//!   pin no monitor shows onto one, never restores click-through, and never captures anything.

use super::*;
use crate::store::{self, HistoryDir, Limits, PinRecord, PinStore};

/// How long the pins must be still before a backup is written.
pub const BACKUP_DELAY: Duration = Duration::from_secs(1);

/// The instance's stores (Step 60).
#[derive(Debug, Default)]
pub struct Keeping {
    /// The disk history while `[history] disk` is on.
    disk: Option<HistoryDir>,
    /// Stable backup keys of open pins (restored ones keep theirs).
    keys: BTreeMap<PinId, String>,
    /// Backed-up pins not restored yet: kept in every save until restored or cleared.
    pending: Vec<PinRecord>,
    /// What the last backup wrote (a change arms the timer).
    saved: Option<u64>,
    timer: Option<Duration>,
    armed: bool,
    /// Problems of the stores not yet shown.
    problems: Vec<String>,
    next_key: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl<S: Shell> App<S> {
    fn store_dir(&self) -> PathBuf {
        self.paths.data_dir.clone()
    }

    fn pin_store(&self) -> PinStore {
        PinStore::new(&self.store_dir())
    }

    fn history_limits(&self) -> Limits {
        let h = self.settings.settings.history;
        Limits {
            count: h.count,
            keep_days: h.keep_days,
        }
    }

    /// Applies `[history]` and `[pins] backup` (at start and after a settings change): the
    /// memory limits, the disk history opened and read back (or closed), the backup offered.
    pub(super) fn apply_keeping(&mut self, starting: bool) {
        let h = self.settings.settings.history;
        self.captures.set_limits(crate::selection::KeepLimits {
            count: h.count,
            bytes: h.memory_mb * 1024 * 1024,
            keep_cancelled: h.keep_cancelled,
        });
        if h.disk && self.keeping.disk.is_none() {
            let mut d = HistoryDir::open(&self.store_dir());
            let _ = d.prune(self.history_limits(), now_secs());
            self.captures.continue_from(d.next_id());
            for e in d.entries().to_vec() {
                let Ok(png) = d.read(e.id) else {
                    d.problems
                        .push(format!("history entry {} was damaged and skipped", e.id));
                    continue;
                };
                match crate::decode::decode(&self.ledger, &png, crate::decode::Format::Png) {
                    Ok(dec) => {
                        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(e.at);
                        self.captures.restore_kept(
                            e.id,
                            dec.frame.moved_to(e.rect.x, e.rect.y),
                            at,
                        );
                    }
                    Err(_) => d
                        .problems
                        .push(format!("history entry {} could not be read", e.id)),
                }
            }
            self.keeping.problems.append(&mut d.problems);
            self.keeping.disk = Some(d);
        } else if !h.disk {
            // Turning it off stops writing; what is on disk stays until `noeul history clear`.
            self.keeping.disk = None;
        } else if let Some(d) = &mut self.keeping.disk {
            let _ = d.prune(
                Limits {
                    count: h.count,
                    keep_days: h.keep_days,
                },
                now_secs(),
            );
        }
        if starting && self.settings.settings.pin_backup {
            let r = self.pin_store().read();
            self.keeping.problems.extend(r.problems);
            self.keeping.pending = r.records;
            let n = self.keeping.pending.len();
            if n > 0 {
                self.shell.notice(&format!(
                    "Noeul can restore {n} pin{} from last time: tray → Restore pins, or `noeul pin restore`.",
                    if n == 1 { "" } else { "s" }
                ));
            }
        }
        self.show_store_problems();
    }

    fn show_store_problems(&mut self) {
        for p in std::mem::take(&mut self.keeping.problems) {
            self.shell.notice(&format!("Noeul: {p}."));
        }
    }

    /// After every batch of events: writes new history pictures when the disk history is on,
    /// and arms the backup timer when the pins changed.
    pub fn housekeeping(&mut self) {
        let fresh = self.captures.take_fresh();
        if let Some(d) = &mut self.keeping.disk {
            let limits = Limits {
                count: self.settings.settings.history.count,
                keep_days: self.settings.settings.history.keep_days,
            };
            for id in fresh {
                let Some(k) = self.captures.kept_entry(id) else {
                    continue;
                };
                let written = output::png(&k.frame)
                    .map_err(std::io::Error::other)
                    .and_then(|png| d.add(id, k.rect, &png, now_secs(), limits));
                if let Err(e) = written {
                    self.keeping
                        .problems
                        .push(format!("the capture history could not be written ({e})"));
                    break;
                }
            }
        }
        if self.settings.settings.pin_backup && !self.keeping.armed {
            let sig = self.backup_signature();
            if self.keeping.saved != Some(sig) {
                self.keeping.timer = Some(BACKUP_DELAY);
                self.keeping.armed = true;
            }
        }
        self.show_store_problems();
    }

    /// The delay to arm the backup timer with, once per change that asked for it.
    pub fn take_backup_timer(&mut self) -> Option<Duration> {
        self.keeping.timer.take()
    }

    /// The backup timer fired: write the backup if the pins still differ from it.
    pub fn backup_due(&mut self) {
        self.keeping.armed = false;
        if !self.settings.settings.pin_backup {
            return;
        }
        if self.keeping.saved != Some(self.backup_signature())
            && let Err(e) = self.save_backup()
        {
            self.shell
                .notice(&format!("Noeul could not back up the pins: {}", e.message));
        }
    }

    /// What the backup holds, as a number: the pins' places, transforms, groups, annotations and
    /// the records still waiting to be restored (never pixels).
    fn backup_signature(&self) -> u64 {
        let mut s = self.pins.list(&self.capture.desktop()).to_string();
        for id in self.pins.ids() {
            s.push_str(&format!("{:?}", self.pins.annotations(id)));
        }
        for r in &self.keeping.pending {
            s.push_str(&r.key);
        }
        fnv(&s)
    }

    fn key_of(&mut self, id: PinId) -> String {
        if let Some(k) = self.keeping.keys.get(&id) {
            return k.clone();
        }
        self.keeping.next_key += 1;
        let started = self
            .started_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let k = format!("{started:x}-{}-{}", self.keeping.next_key, id);
        self.keeping.keys.insert(id, k.clone());
        k
    }

    /// Writes the backup now: the open pins (pictures with their annotations drawn in) and the
    /// pins still waiting to be restored.
    pub(super) fn save_backup(&mut self) -> Result<Value> {
        let mut records = Vec::new();
        let mut pictures: BTreeMap<String, Frame> = BTreeMap::new();
        self.keeping.keys.retain(|id, _| self.pins.contains(*id));
        for id in self.pins.ids() {
            let (Some(info), Some(base)) = (self.pins.info(id), self.pins.picture(id)) else {
                continue;
            };
            let annotations = self.pins.annotations(id).unwrap_or(&[]);
            let annotated = !annotations.is_empty();
            let Some(pic) =
                crate::pins::render(base, annotations).or_else(|| base.crop(base.rect()))
            else {
                continue;
            };
            let image = store::picture_key(pic.width(), pic.height(), pic.pixels());
            let group = self.pins.group(id).unwrap_or("Default").to_string();
            let key = self.key_of(id);
            let o = info.transforms.orientation;
            records.push(PinRecord {
                key,
                group,
                x: info.rect.x,
                y: info.rect.y,
                zoom: info.zoom,
                rotation: o.rotation(),
                mirrored: o.mirrored(),
                opacity: info.transforms.opacity,
                thumbnail: info.transforms.thumbnail,
                visible: info.visible,
                source: info.source.as_str().to_string(),
                annotated,
                width: pic.width(),
                height: pic.height(),
                image: image.clone(),
            });
            pictures.entry(image).or_insert(pic);
        }
        let open_keys: Vec<String> = records.iter().map(|r| r.key.clone()).collect();
        records.extend(
            self.keeping
                .pending
                .iter()
                .filter(|r| !open_keys.contains(&r.key))
                .cloned(),
        );
        let written = self
            .pin_store()
            .save(&records, &mut |key| {
                pictures
                    .get(key)
                    .ok_or_else(|| "a pending pin's picture is missing".to_string())
                    .and_then(output::png)
            })
            .map_err(|e| NoeulError::new(Code::Io, format!("the pin backup: {e}")))?;
        self.keeping.saved = Some(self.backup_signature());
        Ok(json!({"dry_run": false, "saved": records.len(), "written": written}))
    }

    /// `backup.status`.
    pub(super) fn backup_status(&self) -> Value {
        let r = self.pin_store().read();
        let open: Vec<&String> = self.keeping.keys.values().collect();
        let restorable = r.records.iter().filter(|x| !open.contains(&&x.key)).count();
        json!({
            "enabled": self.settings.settings.pin_backup,
            "saved_at": r.saved_at.map(|s| rfc3339(SystemTime::UNIX_EPOCH + Duration::from_secs(s))),
            "pins": r.records.len(),
            "restorable": restorable,
            "problems": r.problems,
        })
    }

    /// Backed-up pins not open now (the tray item).
    pub(super) fn restorable(&self) -> usize {
        let open: Vec<&String> = self.keeping.keys.values().collect();
        self.keeping
            .pending
            .iter()
            .filter(|r| !open.contains(&&r.key))
            .count()
    }

    /// `backup.restore`: every backed-up pin that is not open, checked, in its place.
    pub(super) fn restore_pins(&mut self, dry_run: bool) -> Result<Value> {
        let store = self.pin_store();
        let read = store.read();
        let mut problems = read.problems;
        let open: Vec<String> = self.keeping.keys.values().cloned().collect();
        let (already, todo): (Vec<PinRecord>, Vec<PinRecord>) = read
            .records
            .into_iter()
            .partition(|r| open.contains(&r.key));
        if dry_run {
            return Ok(
                json!({"dry_run": true, "restored": [], "already_open": already.len(),
                "would_restore": todo.len(), "skipped": 0, "problems": problems}),
            );
        }
        let desktop = self.capture.desktop();
        let mut restored = Vec::new();
        let mut skipped = 0;
        for r in todo {
            let frame = store.image(&r).and_then(|png| {
                crate::decode::decode(&self.ledger, &png, crate::decode::Format::Png)
                    .map_err(|e| e.message)
                    .map(|d| d.frame)
            });
            let frame = match frame {
                Ok(f)
                    if (f.width(), f.height()) == (r.width, r.height)
                        && store::picture_key(f.width(), f.height(), f.pixels()) == r.image =>
                {
                    f
                }
                Ok(_) => {
                    problems.push(format!(
                        "pin {}: its picture is damaged",
                        store::short(&r.key)
                    ));
                    skipped += 1;
                    continue;
                }
                Err(e) => {
                    problems.push(e);
                    skipped += 1;
                    continue;
                }
            };
            let source = match r.source.as_str() {
                "clipboard" => Source::Clipboard,
                "file" => Source::File,
                _ => Source::Capture,
            };
            let new = NewPin {
                frame: frame.moved_to(r.x, r.y),
                source,
                placement: Placement::Exact,
                focus: false,
                guard: None,
                annotations: Vec::new(),
            };
            let id = match self.pins.create(self.shell.pin_windows(), &desktop, new) {
                Ok(id) => id,
                Err((_, why)) => {
                    problems.push(format!("a pin window could not be created: {why}"));
                    skipped += 1;
                    continue;
                }
            };
            if let Some(name) = crate::pins::group_name(&r.group) {
                let _ = self.pins.set_group(id, &name);
            }
            if let Some(o) = Orientation::new(r.rotation, r.mirrored) {
                let _ = self.pins.orient(id, |_| o);
            }
            if r.zoom != zoom::FULL {
                let _ = self.pins.set_zoom(id, r.zoom, (r.x, r.y));
            }
            let _ = self.pins.set_opacity(id, r.opacity.clamp(10, 100));
            if r.thumbnail.is_some() {
                let _ = self.pins.set_thumbnail(id, true);
            }
            if !self
                .pins
                .info(id)
                .is_some_and(|i| crate::pins::on_screen(&desktop, i.rect))
            {
                let _ = self.pins.bring_on_screen(id, &desktop);
            }
            if !r.visible {
                let _ = self.pins.hide(id);
            }
            self.keeping.keys.insert(id, r.key.clone());
            restored.push(id.to_string());
        }
        self.keeping
            .pending
            .retain(|p| !self.keeping.keys.values().any(|k| k == &p.key));
        if !restored.is_empty() {
            self.shell.notice(&format!(
                "Noeul restored {} pin{}.",
                restored.len(),
                if restored.len() == 1 { "" } else { "s" }
            ));
        }
        self.last_action = Some(("pin.restore".into(), "restored"));
        Ok(
            json!({"dry_run": false, "restored": restored, "already_open": already.len(),
            "would_restore": 0, "skipped": skipped, "problems": problems}),
        )
    }

    /// `backup.clear`: the backup and its pictures removed (open pins stay open).
    pub(super) fn clear_backup(&mut self, dry_run: bool) -> Result<Value> {
        let n = self.pin_store().read().records.len();
        if !dry_run {
            self.pin_store()
                .clear()
                .map_err(|e| NoeulError::io(self.store_dir().display(), &e))?;
            self.keeping.pending.clear();
            self.keeping.saved = None;
        }
        Ok(json!({"dry_run": dry_run, "removed": n}))
    }

    /// `history.list`: the kept captures, newest last (geometry and time only, never pixels).
    pub(super) fn history_list(&self) -> Value {
        let on_disk: Vec<u64> = self
            .keeping
            .disk
            .as_ref()
            .map(|d| d.entries().iter().map(|e| e.id).collect())
            .unwrap_or_default();
        let entries: Vec<Value> = self
            .captures
            .kept()
            .iter()
            .map(|k| {
                json!({"id": k.id, "rect": k.rect.to_json(), "at": rfc3339(k.at),
                    "outcome": match k.outcome {
                        crate::selection::Outcome::Pinned => "pinned",
                        crate::selection::Outcome::Copied => "copied",
                        crate::selection::Outcome::Saved => "saved",
                        crate::selection::Outcome::Cancelled => "cancelled",
                    },
                    "on_disk": on_disk.contains(&k.id)})
            })
            .collect();
        json!({"disk": self.keeping.disk.is_some(), "count": entries.len(),
            "bytes": self.captures.kept_bytes(), "entries": entries})
    }

    fn history_id(params: &Value) -> Result<Option<u64>> {
        match params.get("entry") {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| NoeulError::new(Code::Usage, "`entry` must be a history number")),
        }
    }

    fn no_entry(id: Option<u64>) -> NoeulError {
        NoeulError::new(
            Code::HistoryEntryNotFound,
            match id {
                Some(id) => format!("no capture {id} in the history"),
                None => "the capture history is empty".into(),
            },
        )
        .hint("run `noeul history list` to see the kept captures")
    }

    /// `history.pin`: an earlier capture pinned again where it was taken (the newest by default).
    pub(super) fn history_pin(&mut self, params: &Value, dry_run: bool) -> Result<Value> {
        let want = Self::history_id(params)?;
        let entry = match want {
            Some(id) => self.captures.kept_entry(id),
            None => self.captures.kept().back(),
        }
        .ok_or_else(|| Self::no_entry(want))?;
        let n = entry.id;
        if dry_run {
            return Ok(json!({"dry_run": true, "entry": n, "pin": null}));
        }
        let frame = entry
            .frame
            .crop(entry.frame.rect())
            .ok_or_else(|| NoeulError::new(Code::Internal, "the picture could not be copied"))?;
        let desktop = self.capture.desktop();
        let new = NewPin {
            frame,
            source: Source::Capture,
            placement: Placement::Exact,
            focus: false,
            guard: None,
            annotations: Vec::new(),
        };
        let id = self
            .pins
            .create(self.shell.pin_windows(), &desktop, new)
            .map_err(|(_, why)| {
                NoeulError::new(
                    Code::Internal,
                    format!("the pin window could not be created: {why}"),
                )
            })?;
        if !self
            .pins
            .info(id)
            .is_some_and(|i| crate::pins::on_screen(&desktop, i.rect))
        {
            let _ = self.pins.bring_on_screen(id, &desktop);
        }
        self.shell
            .notice(&format!("Noeul pinned capture {n} again."));
        self.last_action = Some(("history.pin".into(), "pinned"));
        Ok(json!({"dry_run": false, "entry": n, "pin": id.to_string()}))
    }

    /// `history.forget`: one entry, in memory and on disk.
    pub(super) fn history_forget(&mut self, params: &Value, dry_run: bool) -> Result<Value> {
        let id = Self::history_id(params)?
            .ok_or_else(|| NoeulError::new(Code::Usage, "`entry` is required"))?;
        if self.captures.kept_entry(id).is_none() {
            return Err(Self::no_entry(Some(id)));
        }
        if !dry_run {
            self.captures.forget(id);
            if let Some(d) = &mut self.keeping.disk {
                d.forget(id)
                    .map_err(|e| NoeulError::io(d.dir().display(), &e))?;
            }
        }
        Ok(json!({"dry_run": dry_run, "entry": id, "forgotten": true}))
    }

    /// `history.clear`: every kept capture, in memory and on disk (also when the disk history
    /// is off now but files remain from before).
    pub(super) fn history_clear(&mut self, dry_run: bool) -> Result<Value> {
        let memory = self.captures.kept().len();
        let mut disk = self
            .keeping
            .disk
            .take()
            .unwrap_or_else(|| HistoryDir::open(&self.store_dir()));
        disk.problems.clear();
        let on_disk = disk.entries().len();
        if !dry_run {
            self.captures.clear_history();
            disk.clear()
                .map_err(|e| NoeulError::io(disk.dir().display(), &e))?;
        }
        if self.settings.settings.history.disk {
            self.keeping.disk = Some(disk);
        }
        Ok(json!({"dry_run": dry_run, "cleared": memory, "disk_cleared": on_disk}))
    }
}
