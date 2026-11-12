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
