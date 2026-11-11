//! Step 40 — `kim rollback` (docs/RELEASE_POLICY.md §2–§3). `KIM-SEC-001`, `KIM-SEC-003`.
//!
//! A test-signed catalog over loopback, the real `kim`: nothing to roll back before an update;
//! after one, a dry run that changes nothing, a rollback that makes the kept previous version
//! active (its program self-checked, nothing downloaded), and a rollback of the rollback.

mod common;

use common::{Sandbox, Server, package, release};
use serde_json::Value;

fn json(sb: &Sandbox, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let (code, v, _) = sb.json(&a);
    (code, v)
}

fn active(sb: &Sandbox) -> String {
    let rec: Value = serde_json::from_slice(
        &std::fs::read(sb.tool_dir("example-tool").join("current.json")).unwrap(),
    )
    .unwrap();
    rec["version"].as_str().unwrap().to_string()
}

#[test]
fn rollback_reactivates_the_kept_version_and_can_be_undone() {
    let sb = Sandbox::new("rollback");
    let srv = Server::start();
    let (v1, v2) = (package(&[("V", b"1")]), package(&[("V", b"2")]));
    let (u1, u2) = (srv.put("/1.zip", v1.clone()), srv.put("/2.zip", v2.clone()));

    let (code, v) = json(&sb, &["rollback", "example-tool"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (3, Some("not_installed")),
        "{v}"
    );
    sb.publish(100, &[release("1.0.0", &u1, &v1, "tool.exe")]);
    let (code, v) = json(&sb, &["install", "example-tool"]);
    assert_eq!(code, 0, "{v}");
    let (code, v) = json(&sb, &["rollback", "example-tool"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (5, Some("no_release")),
        "nothing kept yet: {v}"
    );

    sb.publish(
        101,
        &[
            release("1.0.0", &u1, &v1, "tool.exe"),
            release("2.0.0", &u2, &v2, "tool.exe"),
        ],
    );
    let (code, v) = json(&sb, &["update", "example-tool"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(active(&sb), "2.0.0");

    let before = sb.snapshot();
    let (code, v) = json(&sb, &["rollback", "example-tool", "--dry-run"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["plan"]["to"], "1.0.0");
    assert_eq!(sb.snapshot(), before, "a dry run changes nothing");

    let hits = *srv.hits.lock().unwrap();
    let (code, v) = json(&sb, &["rollback", "example-tool"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["action"], "rollback");
    assert_eq!(active(&sb), "1.0.0");
    assert_eq!(*srv.hits.lock().unwrap(), hits, "nothing was downloaded");
    let (code, _) = json(&sb, &["rollback", "example-tool"]);
    assert_eq!(code, 0);
    assert_eq!(active(&sb), "2.0.0", "the rollback was undone");
    // The kept version's files gone: refused, nothing changed.
    std::fs::remove_dir_all(sb.tool_dir("example-tool").join("versions").join("1.0.0")).unwrap();
    let (code, v) = json(&sb, &["rollback", "example-tool"]);
    assert_eq!(
        (code, v["error"]["code"].as_str()),
        (4, Some("package_invalid")),
        "{v}"
    );
    assert_eq!(active(&sb), "2.0.0");
}
