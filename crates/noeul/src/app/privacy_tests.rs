//! Step 62 (NOEUL-SEC-010, NOEUL-SEC-002, NOEUL-SEC-008, ADR 0076): who may capture or read the
//! clipboard without the keyboard. CLI captures notify by default, are silent with `allow` and
//! refused with `deny` before anything is captured or read; an agent (`via: mcp`) captures only
//! with `privacy.mcp_captures = "on"`, always with the notice, and never reads the clipboard. A
//! capture to a file writes a PNG at an absolute local path, never over a file unless asked, and
//! never echoes the path.

use super::tests::{FakeShell, app, set_config};
use super::*;
use crate::capture::DesktopState;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("noeul-priv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn region() -> Value {
    json!({"x": 1, "y": 1, "width": 4, "height": 3})
}

fn notices(a: &App<FakeShell>) -> usize {
    a.shell.notices.len()
}

#[test]
fn cli_captures_notify_by_default_are_silent_when_allowed_and_refused_when_denied() {
    let dir = tmp("cli");
    let mut a = app(&dir, DesktopState::Available);
    a.request("capture", &json!({"region": region(), "pin": true}))
        .unwrap();
    assert_eq!(
        a.shell.notices.last().map(String::as_str),
        Some("Noeul captured a region (command line).")
    );
    set_config(&mut a, &[("privacy.cli", "allow")]);
    let n = notices(&a);
    a.request("capture", &json!({"region": region(), "pin": true}))
        .unwrap();
    assert_eq!(notices(&a), n, "allowed: silently");
    set_config(&mut a, &[("privacy.cli", "deny")]);
    let pins = a.pins.len();
    let live = a.ledger.live();
    for p in [
        json!({"region": region(), "pin": true}),
        json!({"region": region(), "pin": true, "dry_run": true}),
    ] {
        let e = a.request("capture", &p).unwrap_err();
        assert_eq!(e.code, Code::CaptureNotAllowed, "{p}");
        assert!(e.hint.unwrap().contains("privacy.cli"));
    }
    let e = a
        .request(
            "pin.create",
            &json!({"source": "region", "region": region()}),
        )
        .unwrap_err();
    assert_eq!(e.code, Code::CaptureNotAllowed);
    let reads = a.shell.clipboard_reads;
    let e = a
        .request("pin.create", &json!({"source": "clipboard"}))
        .unwrap_err();
    assert_eq!(
        (e.code, a.shell.clipboard_reads),
        (Code::CaptureNotAllowed, reads),
        "the clipboard was never opened"
    );
    assert_eq!(
        (a.pins.len(), a.ledger.live()),
        (pins, live),
        "nothing captured"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agents_capture_only_when_the_user_turned_it_on_and_never_read_the_clipboard() {
    let dir = tmp("mcp");
    let mut a = app(&dir, DesktopState::Available);
    let out = dir.join("agent.png");
    let p = json!({"region": region(), "output": out.display().to_string(), "via": "mcp"});
    let e = a.request("capture", &p).unwrap_err();
    assert_eq!(e.code, Code::CaptureNotAllowed);
    assert!(e.hint.unwrap().contains("privacy.mcp_captures"));
    assert!(!out.exists());
    // `privacy.cli = "allow"` does not open the door for agents.
    set_config(&mut a, &[("privacy.cli", "allow")]);
    assert_eq!(
        a.request("capture", &p).unwrap_err().code,
        Code::CaptureNotAllowed
    );
    set_config(&mut a, &[("privacy.mcp_captures", "on")]);
    let n = notices(&a);
    let v = a.request("capture", &p).unwrap();
    assert!(out.is_file());
    assert_eq!(notices(&a), n + 1, "an agent's capture is always announced");
    assert!(
        !v.to_string().contains("agent.png"),
        "the path is never echoed"
    );
    let e = a
        .request("pin.create", &json!({"source": "clipboard", "via": "mcp"}))
        .unwrap_err();
    assert_eq!(
        e.code,
        Code::CaptureNotAllowed,
        "agents never read the clipboard"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_capture_to_a_file_is_a_png_at_a_local_absolute_path_never_over_a_file() {
    let dir = tmp("file");
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = app(&dir, DesktopState::Available);
    let out = dir.join("shot.png");
    let p = |replace: bool| json!({"region": region(), "output": out.display().to_string(), "replace": replace});
    let dry = a
        .request(
            "capture",
            &json!({"region": region(), "output": out.display().to_string(), "dry_run": true}),
        )
        .unwrap();
    assert_eq!(dry["dry_run"], true);
    assert!(!out.exists(), "a dry run writes nothing");
    let v = a.request("capture", &p(false)).unwrap();
    assert_eq!(v["file"]["written"], true);
    assert_eq!(a.pins.len(), 0, "written, not pinned");
    let png = std::fs::read(&out).unwrap();
    let f = crate::decode::decode(&a.ledger, &png, crate::decode::Format::Png)
        .unwrap()
        .frame;
    assert_eq!((f.width(), f.height()), (4, 3));
    // Never over an existing file without `replace`.
    std::fs::write(&out, b"keep me").unwrap();
    let e = a.request("capture", &p(false)).unwrap_err();
    assert_eq!(e.code, Code::Io);
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");
    a.request("capture", &p(true)).unwrap();
    assert_ne!(std::fs::read(&out).unwrap(), b"keep me");
    // Only absolute .png (since #99 also .jpg) paths on a local drive.
    for bad in [
        "shot.png",
        "C:\\\\x.gif",
        "\\\\\\\\server\\\\share\\\\x.png",
        "C:\\\\a\\\\..\\\\x.png",
    ] {
        let e = a
            .request("capture", &json!({"region": region(), "output": bad}))
            .unwrap_err();
        assert_eq!(e.code, Code::Usage, "{bad}");
    }
    // Exactly one target: a pin or a file.
    let e = a
        .request(
            "capture",
            &json!({"region": region(), "pin": true, "output": out.display().to_string()}),
        )
        .unwrap_err();
    assert_eq!(e.code, Code::Usage);
    let _ = std::fs::remove_dir_all(&dir);
}
