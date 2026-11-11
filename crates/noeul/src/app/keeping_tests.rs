//! Step 60 tests (NOEUL-FR-050, NOEUL-FR-051, NOEUL-FR-066, NOEUL-SEC-001, NOEUL-SEC-005,
//! ADR 0074): the capture history keeps what was output (redactions drawn in) and nothing the
//! user marked private; it can be pinned again, forgotten and cleared; the disk history is
//! opt-in, bounded, survives a restart and refuses damaged pictures; the pin backup is opt-in,
//! written once the pins are still, offered (never restored by itself) at the next start,
//! restored once with its transforms, never duplicated, and checked.

use super::tests::{FakeShell, app, app_with_history, choose, drag, key, overlay_app, set_config};
use super::*;
use crate::capture::{DesktopState, Synthetic};
use crate::selection::KeepLimits;

fn history_on(a: &mut App<FakeShell>) {
    a.captures.set_limits(KeepLimits {
        count: 20,
        bytes: 256 << 20,
        keep_cancelled: false,
    });
}

fn px(f: &Frame, x: i32, y: i32) -> [u8; 4] {
    let r = f.rect();
    let i = (((y - r.y) as u32 * f.width() + (x - r.x) as u32) * 4) as usize;
    f.pixels()[i..i + 4].try_into().unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("noeul-keep-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn pin_region(a: &mut App<FakeShell>, x: i32) -> PinId {
    let v = a
        .request(
            "capture",
            &json!({"region": {"x": x, "y": 20, "width": 4, "height": 3}, "pin": true}),
        )
        .unwrap();
    PinId::parse(v["pin"]["id"].as_str().unwrap()).unwrap()
}

#[test]
fn the_history_keeps_what_was_output_and_nothing_marked_private() {
    let (mut a, _syn, _dir) = overlay_app("hist-out");
    history_on(&mut a);
    // A capture with a solid redaction, copied.
    a.action("capture.region", None);
    drag(&mut a, (-600, 20), (-201, 219));
    choose(&mut a, annotation::Tool::Redact);
    drag(&mut a, (-580, 100), (-500, 140));
    key(&mut a, "C", true, false);
    assert!(!a.overlay_open());
    assert_eq!(a.captures.kept().len(), 1);
    let k = a.captures.kept().back().unwrap();
    assert_eq!(k.rect, Rect::new(-600, 20, 400, 200));
    // Inside the redaction: one fill, never a pixel of the original.
    let fill = px(&k.frame, -560, 110);
    for (x, y) in [(-575, 105), (-540, 120), (-510, 135)] {
        assert_eq!(px(&k.frame, x, y), fill);
        assert_ne!(
            px(&k.frame, x, y),
            Synthetic::pixel(x, y),
            "the original at {x},{y}"
        );
    }
    // Outside it: the captured pixels.
    assert_eq!(px(&k.frame, -300, 200), Synthetic::pixel(-300, 200));
    // Marked private with `P`: copied, but kept in no history.
    let rects = a.captures.history().len();
    a.action("capture.region", None);
    drag(&mut a, (-500, 30), (-401, 129));
    key(&mut a, "P", false, false);
    key(&mut a, "C", true, false);
    assert!(!a.overlay_open());
    assert_eq!(a.captures.kept().len(), 1, "private: no picture");
    assert_eq!(a.captures.history().len(), rects, "private: no rectangle");
    // A cancelled capture is not kept by default.
    a.action("capture.region", None);
    drag(&mut a, (-500, 30), (-401, 129));
    key(&mut a, "Escape", false, false);
    assert_eq!(a.captures.kept().len(), 1);
    // Listed (geometry and time only), pinned again, forgotten, cleared.
    let l = a.request("history.list", &json!({})).unwrap();
    assert_eq!(
        (l["count"].as_u64(), l["disk"].as_bool()),
        (Some(1), Some(false))
    );
    assert_eq!(l["entries"][0]["outcome"], "copied");
    assert!(a.tray_menu().contains(&MenuItem::PinLastCapture));
    let n = l["entries"][0]["id"].as_u64().unwrap();
    let dry = a.request("history.pin", &json!({"dry_run": true})).unwrap();
    assert_eq!((dry["entry"].as_u64(), a.pins.len()), (Some(n), 0));
    let v = a.request("history.pin", &json!({"entry": n})).unwrap();
    let pin = PinId::parse(v["pin"].as_str().unwrap()).unwrap();
    assert_eq!(
        a.pins.picture(pin).unwrap().rect(),
        Rect::new(-600, 20, 400, 200)
    );
    assert_eq!(
        px(a.pins.picture(pin).unwrap(), -560, 110),
        fill,
        "the redacted picture"
    );
    let e = a
        .request("history.pin", &json!({"entry": 999}))
        .unwrap_err();
    assert_eq!(e.code, Code::HistoryEntryNotFound);
    a.request("history.forget", &json!({"entry": n})).unwrap();
    assert!(a.captures.kept().is_empty());
    let e = a.request("history.pin", &json!({})).unwrap_err();
    assert_eq!(e.code, Code::HistoryEntryNotFound);
    assert!(!a.tray_menu().contains(&MenuItem::PinLastCapture));
    let c = a.request("history.clear", &json!({})).unwrap();
    assert_eq!(c["cleared"], 0);
}

#[test]
fn the_disk_history_is_opt_in_bounded_survives_a_restart_and_refuses_damage() {
    let dir = tmp("disk");
    let hist = dir.join("Local").join("mouv").join("noeul").join("history");
    let mut a = app(&dir, DesktopState::Available);
    history_on(&mut a);
    pin_region(&mut a, 0);
    a.housekeeping();
    assert_eq!(a.captures.kept().len(), 1, "kept in memory");
    assert!(!hist.exists(), "nothing on disk by default (NOEUL-SEC-005)");
    set_config(&mut a, &[("history.disk", "on"), ("history.count", "2")]);
    for x in [1, 2, 3] {
        pin_region(&mut a, x);
    }
    a.housekeeping();
    let pngs = || {
        std::fs::read_dir(&hist)
            .map(|d| {
                d.flatten()
                    .filter(|f| f.file_name().to_string_lossy().ends_with(".png"))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(pngs(), 2, "bounded by `[history] count`");
    let l = a.request("history.list", &json!({})).unwrap();
    assert_eq!(l["count"], 2);
    assert!(
        l["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["on_disk"] == true)
    );
    drop(a);
    // Restarted: the two captures are back and can be pinned.
    let mut b = app_with_history(&dir, DesktopState::Available);
    let l = b.request("history.list", &json!({})).unwrap();
    assert_eq!(l["count"], 2, "{l}");
    let newest = l["entries"][1]["id"].as_u64().unwrap();
    let v = b.request("history.pin", &json!({"entry": newest})).unwrap();
    assert!(v["pin"].is_string());
    drop(b);
    // A damaged picture is refused and said; the other one stays.
    let victim = std::fs::read_dir(&hist)
        .unwrap()
        .flatten()
        .find(|f| f.file_name().to_string_lossy().ends_with(".png"))
        .unwrap()
        .path();
    std::fs::write(&victim, b"not the picture").unwrap();
    let mut c = app_with_history(&dir, DesktopState::Available);
    assert_eq!(c.request("history.list", &json!({})).unwrap()["count"], 1);
    assert!(
        c.shell.notices.iter().any(|n| n.contains("damaged")),
        "{:?}",
        c.shell.notices
    );
    let v = c.request("history.clear", &json!({})).unwrap();
    assert_eq!(
        (v["cleared"].as_u64(), v["disk_cleared"].as_u64()),
        (Some(1), Some(2))
    );
    assert_eq!(pngs(), 0, "cleared on disk");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_pin_backup_is_opt_in_offered_restored_once_and_checked() {
    let dir = tmp("backup");
    let pins_dir = dir.join("Local").join("mouv").join("noeul").join("pins");
    // Off by default: no timer, no file.
    let mut a = app(&dir, DesktopState::Available);
    let p1 = pin_region(&mut a, 0);
    a.housekeeping();
    assert_eq!(a.take_backup_timer(), None);
    assert!(!pins_dir.exists());
    set_config(&mut a, &[("pins.backup", "on")]);
    let p2 = pin_region(&mut a, 40);
    a.pins.set_zoom(p1, 200, (0, 20)).unwrap();
    a.pins.set_opacity(p1, 50).unwrap();
    a.pins.orient(p1, |o| o.rotate_cw()).unwrap();
    a.pins.set_group(p1, "Work").unwrap();
    a.pins.hide(p2).unwrap();
    // A change arms one timer; it writes once.
    a.housekeeping();
    assert_eq!(a.take_backup_timer(), Some(keeping::BACKUP_DELAY));
    a.housekeeping();
    assert_eq!(a.take_backup_timer(), None, "armed once");
    a.backup_due();
    let backup = pins_dir.join("backup.json");
    let doc: Value = serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
    assert_eq!(doc["schema"], "noeul.pins");
    assert_eq!(doc["pins"].as_array().unwrap().len(), 2);
    assert!(!doc.to_string().contains("\\\\"), "no paths in the backup");
    // Unchanged: no new timer.
    a.housekeeping();
    assert_eq!(a.take_backup_timer(), None);
    drop(a);

    // Next start: offered, not restored.
    let mut b = app(&dir, DesktopState::Available);
    assert_eq!(b.pins.len(), 0, "nothing restored by itself");
    assert!(
        b.shell.notices.iter().any(|n| n.contains("restore 2 pins")),
        "{:?}",
        b.shell.notices
    );
    assert!(b.tray_menu().contains(&MenuItem::RestorePins));
    let s = b.request("backup.status", &json!({})).unwrap();
    assert_eq!(
        (s["pins"].as_u64(), s["restorable"].as_u64()),
        (Some(2), Some(2))
    );
    // A save before the restore keeps the pending pins in the backup.
    b.housekeeping();
    b.backup_due();
    assert_eq!(b.request("backup.status", &json!({})).unwrap()["pins"], 2);
    let dry = b
        .request("backup.restore", &json!({"dry_run": true}))
        .unwrap();
    assert_eq!((dry["would_restore"].as_u64(), b.pins.len()), (Some(2), 0));
    let v = b.request("backup.restore", &json!({})).unwrap();
    assert_eq!(v["restored"].as_array().unwrap().len(), 2, "{v}");
    let infos: Vec<_> = b
        .pins
        .ids()
        .into_iter()
        .map(|id| (id, b.pins.info(id).unwrap()))
        .collect();
    let (z, hidden) = infos.iter().partition::<Vec<_>, _>(|(_, i)| i.zoom == 200);
    let (id1, i1) = &z[0];
    assert_eq!(
        (i1.transforms.opacity, i1.transforms.orientation.rotation()),
        (50, 90)
    );
    assert_eq!(b.pins.group(*id1), Some("Work"));
    assert!(!hidden[0].1.visible, "the hidden pin comes back hidden");
    assert!(!b.tray_menu().contains(&MenuItem::RestorePins));
    // Again: nothing duplicated.
    let v = b.request("backup.restore", &json!({})).unwrap();
    assert_eq!(
        (
            v["restored"].as_array().unwrap().len(),
            v["already_open"].as_u64()
        ),
        (0, Some(2))
    );
    assert_eq!(b.pins.len(), 2);
    drop(b);

    // A pin no monitor shows comes back on screen.
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
    doc["pins"][1]["x"] = json!(-100_000);
    std::fs::write(&backup, doc.to_string()).unwrap();
    let mut c = app(&dir, DesktopState::Available);
    let v = c.request("backup.restore", &json!({})).unwrap();
    assert_eq!(v["restored"].as_array().unwrap().len(), 2, "{v}");
    for id in c.pins.ids() {
        assert!(crate::pins::on_screen(
            &c.capture.desktop(),
            c.pins.info(id).unwrap().rect
        ));
    }
    drop(c);
    // A damaged picture (both pins share it: stored once) is skipped and said, never half-used.
    let image = doc["pins"][0]["image"].as_str().unwrap().to_string();
    assert_eq!(
        doc["pins"][1]["image"].as_str(),
        Some(image.as_str()),
        "one file for one picture"
    );
    std::fs::write(pins_dir.join("img").join(format!("{image}.png")), b"broken").unwrap();
    let mut c = app(&dir, DesktopState::Available);
    let v = c.request("backup.restore", &json!({})).unwrap();
    assert_eq!(
        (
            v["restored"].as_array().unwrap().len(),
            v["skipped"].as_u64()
        ),
        (0, Some(2)),
        "{v}"
    );
    assert_eq!(v["problems"].as_array().unwrap().len(), 2);
    assert_eq!(c.pins.len(), 0);
    let v = c.request("backup.clear", &json!({})).unwrap();
    assert_eq!(v["removed"], 2);
    assert!(!pins_dir.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pin's annotations are backed up drawn into its picture: a redaction never sits on disk
/// over the pixels it hides.
#[test]
fn an_annotated_pin_is_backed_up_with_its_redactions_drawn_in() {
    let (mut a, _syn, dir) = overlay_app("bk-red");
    set_config(&mut a, &[("pins.backup", "on")]);
    a.action("capture.region", None);
    drag(&mut a, (-600, 20), (-201, 219));
    choose(&mut a, annotation::Tool::Redact);
    drag(&mut a, (-580, 100), (-500, 140));
    key(&mut a, "Enter", false, false);
    assert_eq!(a.pins.len(), 1);
    a.housekeeping();
    a.backup_due();
    let pins_dir = dir.join("Local").join("mouv").join("noeul").join("pins");
    let doc: Value =
        serde_json::from_slice(&std::fs::read(pins_dir.join("backup.json")).unwrap()).unwrap();
    assert_eq!(doc["pins"][0]["annotated"], true);
    let png = std::fs::read(
        pins_dir
            .join("img")
            .join(format!("{}.png", doc["pins"][0]["image"].as_str().unwrap())),
    )
    .unwrap();
    let f = crate::decode::decode(&a.ledger, &png, crate::decode::Format::Png)
        .unwrap()
        .frame
        .moved_to(-600, 20);
    let fill = px(&f, -560, 110);
    for (x, y) in [(-575, 105), (-510, 135)] {
        assert_eq!(px(&f, x, y), fill);
        assert_ne!(px(&f, x, y), Synthetic::pixel(x, y));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
