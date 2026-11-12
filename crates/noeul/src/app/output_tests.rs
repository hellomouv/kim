//! #99 (`OUT-06`, `OUT-07`, `HIST-06`): quick save in JPEG at the configured quality, copies
//! that also carry the picture as a file (removed at quit), and opt-in auto-save of pinned and
//! copied captures into the quick-save folder.

use super::tests::{FakeShell, drag, key, overlay_app, set_config};

fn files(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn quick_save_writes_jpeg_when_asked() {
    let (mut a, _syn, dir) = overlay_app("out-jpeg");
    let shots = dir.join("shots");
    set_config(
        &mut a,
        &[
            ("capture.quick_save_folder", &shots.display().to_string()),
            ("output.format", "jpeg"),
            ("output.jpeg_quality", "70"),
        ],
    );
    a.action("capture.region", None);
    drag(&mut a, (100, 100), (299, 199));
    key(&mut a, "S", true, true);
    assert_eq!(files(&shots), ["noeul-20261104-090507.jpg"]);
    let bytes = std::fs::read(shots.join("noeul-20261104-090507.jpg")).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "a JPEG file");
    let img = image::load_from_memory(&bytes).unwrap();
    assert_eq!((img.width(), img.height()), (200, 100));
    // A quick save from a pin follows the same setting.
    a.action("capture.region", None);
    drag(&mut a, (100, 100), (299, 199));
    key(&mut a, "Enter", false, false);
    let pin = a.pins.ids()[0];
    a.pin_menu_choice(pin, "pin.quick-save");
    assert_eq!(
        files(&shots),
        ["noeul-20261104-090507-2.jpg", "noeul-20261104-090507.jpg"]
    );
    let bytes = std::fs::read(shots.join("noeul-20261104-090507-2.jpg")).unwrap();
    assert_eq!(
        &bytes[..3],
        &[0xFF, 0xD8, 0xFF],
        "the pin's file is a JPEG too"
    );
}

#[test]
fn copy_as_file_and_auto_save_follow_their_settings() {
    let (mut a, _syn, dir) = overlay_app("out-copyfile");
    let shots = dir.join("shots");
    // Off by default: a copy carries no file and nothing is saved.
    a.action("capture.region", None);
    drag(&mut a, (100, 100), (299, 199));
    key(&mut a, "C", true, false);
    assert!(a.shell.copied_files.is_empty());
    set_config(
        &mut a,
        &[
            ("capture.quick_save_folder", &shots.display().to_string()),
            ("output.copy_as_file", "on"),
            ("history.auto_save", "on"),
        ],
    );
    a.action("capture.region", None);
    key(&mut a, "R", false, false);
    key(&mut a, "C", true, false);
    let copied: &FakeShell = &a.shell;
    assert_eq!(copied.copied_files.len(), 1);
    let file = copied.copied_files[0].clone();
    assert!(
        file.starts_with(a.paths.data_dir.join("copies")),
        "{file:?}"
    );
    assert_eq!(&std::fs::read(&file).unwrap()[1..4], b"PNG");
    assert_eq!(files(&shots), ["noeul-20261104-090507.png"], "auto-saved");
    a.shutdown();
    assert!(!a.paths.data_dir.join("copies").exists(), "removed at quit");
}

/// #99 (`PIN-22`): `pin.paste-replace` puts the clipboard's picture into the pin at the same
/// corner; with nothing to paste the pin stays.
#[test]
fn a_pin_takes_the_clipboard_picture_in_place() {
    let (mut a, _syn, _dir) = overlay_app("out-replace");
    a.shell.clipboard = Some((40, 30));
    a.action("paste.float", None);
    let id = crate::pins::PinId(1);
    let before = a.pins.info(id).unwrap();
    assert_eq!(before.image, (40, 30));
    a.shell.clipboard = Some((20, 10));
    a.action("pin.paste-replace", Some(id));
    let after = a.pins.info(id).unwrap();
    assert_eq!(after.image, (20, 10));
    assert_eq!((after.rect.x, after.rect.y), (before.rect.x, before.rect.y));
    assert_eq!(a.status()["last_action"]["result"], "replaced");
    a.shell.clipboard = None;
    a.action("pin.paste-replace", Some(id));
    assert_eq!(
        a.pins.info(id).unwrap().image,
        (20, 10),
        "nothing to paste: unchanged"
    );
}

/// #99 (`SET-06`): the tray icon's left and middle clicks follow `[general] tray_*_click`.
#[test]
fn tray_clicks_follow_their_settings() {
    let (mut a, _syn, _dir) = overlay_app("tray-clicks");
    a.shell.clipboard = Some((40, 30));
    set_config(
        &mut a,
        &[
            ("general.tray_left_click", "pin_clipboard"),
            ("general.tray_middle_click", "none"),
        ],
    );
    a.tray_click(crate::tray::Click::Middle);
    assert!(a.pins.is_empty(), "the middle click does nothing");
    a.tray_click(crate::tray::Click::Left);
    assert_eq!(a.pins.len(), 1, "the left click pinned the clipboard");
    assert!(!a.overlay_open());
    assert_eq!(
        a.settings().to_json()["tray_clicks"],
        serde_json::json!({"left": "pin_clipboard", "middle": "none"})
    );
}

/// #99 (`PIN-20`): `Ctrl`+click selects pins; an action on a selected pin acts
/// on all of them; `pin.select-all` selects every pin; closing ends the selection.
#[test]
fn selected_pins_act_together() {
    let (mut a, _syn, _dir) = overlay_app("pin-select");
    a.shell.clipboard = Some((40, 30));
    a.action("paste.float", None);
    a.shell.pointer_at = Some((700, 400));
    a.action("paste.float", None);
    a.shell.pointer_at = Some((200, 600));
    a.action("paste.float", None);
    let (p1, p2, p3) = (
        crate::pins::PinId(1),
        crate::pins::PinId(2),
        crate::pins::PinId(3),
    );
    for id in [p1, p2] {
        a.pin_event(super::PinEvent::CtrlClick(id));
    }
    assert!(a.pins.info(p1).unwrap().transforms.selected);
    a.action("pin.zoom-in", Some(p1));
    assert_eq!(
        a.pins.info(p2).unwrap().zoom,
        110,
        "both selected pins zoomed"
    );
    assert_eq!(a.pins.info(p3).unwrap().zoom, 100, "the third did not");
    a.action("pin.select-all", Some(p3));
    a.action("pin.close", Some(p3));
    assert!(a.pins.is_empty(), "every selected pin closed");
}
