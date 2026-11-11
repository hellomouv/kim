//! Step 38 — capability interoperability between installed tools (ADR 0071, CLI_CONTRACT §9.8).
//! `KIM-FR-020`, `KIM-FR-021`, `KIM-SEC-001`, `KIM-NFR-001`.
//!
//! The real `kim` and a second tool: the test-only fixture `examples/interop_fixture.rs`,
//! copied into a sandboxed programs directory by this test (Kim never installs it). Covers
//! discovery from manifests, invocation under the consent rules of each effect, version
//! negotiation, an old tool, a missing tool, a hung tool, a broken answer, a failing tool whose
//! message carries a secret, and that a call never installs anything.

mod common;

use common::{Sandbox, install_fixture};
use serde_json::{Value, json};
use std::path::Path;

fn json(sb: &Sandbox, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let (code, v, _) = sb.json(&a);
    (code, v)
}

fn err(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

#[test]
fn a_tool_finds_and_invokes_another_tools_capability_with_consent() {
    let mut sb = Sandbox::new("interop");
    let work = sb.root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    sb.extra_env
        .push(("FIXTURE_DIR".into(), work.display().to_string()));
    install_fixture(&sb, "0.1.0", "normal");
    let before = std::fs::read_dir(sb.programs()).unwrap().count();

    // Discovery reads manifests: providers, interface versions, availability.
    let (code, v) = json(&sb, &["tools", "find", "fixture.echo"]);
    assert_eq!(code, 0, "{v}");
    let p = &v["data"]["providers"][0];
    assert_eq!(p["tool"], "kim-fixture");
    assert_eq!(p["capability"]["version"], "1.2.0");
    assert_eq!(
        (p["available"].clone(), p["compatible"].clone()),
        (json!(true), json!(true))
    );
    assert!(Path::new(p["executable"].as_str().unwrap()).is_file());
    let (_, v) = json(
        &sb,
        &["tools", "find", "fixture.echo", "--require-version", "^2"],
    );
    assert_eq!(v["data"]["providers"][0]["compatible"], false);
    let (_, v) = json(&sb, &["tools", "find", "nobody.has-this"]);
    assert_eq!(v["data"]["providers"], json!([]));
    let (code, v) = json(&sb, &["tools", "find", "NotAnId"]);
    assert_eq!((code, err(&v)), (2, "usage"));

    // Read: any time.
    let (code, v) = json(
        &sb,
        &[
            "tools",
            "invoke",
            "fixture.echo",
            "--params",
            r#"{"text":"hello"}"#,
        ],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["echo"], "hello");
    assert_eq!(v["data"]["result"]["caller"], "kim");
    assert_eq!(v["data"]["provider"]["interface"], "1.2.0");

    // Write: only with --allow-write; a dry run changes nothing and needs no consent.
    let touch = [
        "tools",
        "invoke",
        "fixture.touch",
        "--params",
        r#"{"name":"a.txt"}"#,
    ];
    let (code, v) = json(&sb, &touch);
    assert_eq!((code, err(&v)), (7, "confirmation_required"), "{v}");
    let mut dry = touch.to_vec();
    dry.push("--dry-run");
    let (code, v) = json(&sb, &dry);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["result"]["dry_run"], true);
    assert!(!work.join("a.txt").exists());
    let mut go = touch.to_vec();
    go.push("--allow-write");
    let (code, v) = json(&sb, &go);
    assert_eq!(code, 0, "{v}");
    assert!(work.join("a.txt").exists());

    // Destructive: --allow-write and --yes.
    let wipe = [
        "tools",
        "invoke",
        "fixture.wipe",
        "--params",
        r#"{"name":"a.txt"}"#,
        "--allow-write",
    ];
    let (code, v) = json(&sb, &wipe);
    assert_eq!((code, err(&v)), (7, "confirmation_required"));
    assert!(work.join("a.txt").exists());
    let mut yes = wipe.to_vec();
    yes.push("--yes");
    let (code, v) = json(&sb, &yes);
    assert_eq!(code, 0, "{v}");
    assert!(!work.join("a.txt").exists());

    // A window never from a headless call unless the user asked for it.
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.window"]);
    assert_eq!((code, err(&v)), (7, "confirmation_required"));
    let (code, _) = json(
        &sb,
        &["tools", "invoke", "fixture.window", "--allow-window"],
    );
    assert_eq!(code, 0);

    // Version negotiation: interface 2 is not ^1; asking for ^2 works.
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.next"]);
    assert_eq!((code, err(&v)), (5, "tool_incompatible"), "{v}");
    let (code, v) = json(
        &sb,
        &[
            "tools",
            "invoke",
            "fixture.next",
            "--require-version",
            "^2",
            "--params",
            r#"{"text":"v2"}"#,
        ],
    );
    assert_eq!(code, 0, "{v}");

    // Missing: capability, tool. Nothing is installed by a call.
    let (code, v) = json(&sb, &["tools", "invoke", "nobody.has-this"]);
    assert_eq!((code, err(&v)), (5, "capability_missing"));
    let (code, v) = json(
        &sb,
        &["tools", "invoke", "terminal.open", "--tool", "subak"],
    );
    assert_eq!((code, err(&v)), (3, "not_installed"));
    assert_eq!(
        std::fs::read_dir(sb.programs()).unwrap().count(),
        before,
        "nothing was installed"
    );

    // A failing tool: its code is passed through, its secret is not.
    sb.extra_env.push(("FIXTURE_MODE".into(), "fail".into()));
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.echo"]);
    assert_eq!((code, err(&v)), (1, "tool_failed"), "{v}");
    assert!(
        v["error"]["details"][0]
            .as_str()
            .unwrap()
            .contains("tool_code: io")
    );
    assert!(
        !v.to_string().contains("ghp_abcdefghijklmnop"),
        "redacted: {v}"
    );
    // A broken answer, a hung tool.
    sb.extra_env.retain(|(k, _)| k != "FIXTURE_MODE");
    sb.extra_env.push(("FIXTURE_MODE".into(), "garbage".into()));
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.echo"]);
    assert_eq!((code, err(&v)), (1, "tool_failed"));
    sb.extra_env.retain(|(k, _)| k != "FIXTURE_MODE");
    sb.extra_env.push(("FIXTURE_MODE".into(), "hang".into()));
    let t0 = std::time::Instant::now();
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.echo", "--timeout", "2"]);
    assert_eq!((code, err(&v)), (6, "timeout"));
    assert!(t0.elapsed() < std::time::Duration::from_secs(20));
    sb.extra_env.retain(|(k, _)| k != "FIXTURE_MODE");

    // An old tool (no `provides`): found by nobody, invoked by nobody.
    install_fixture(&sb, "0.0.9", "old");
    let (_, v) = json(&sb, &["tools", "find", "fixture.echo"]);
    assert_eq!(v["data"]["providers"], json!([]), "{v}");
    let (code, v) = json(&sb, &["tools", "invoke", "fixture.echo"]);
    assert_eq!((code, err(&v)), (5, "capability_missing"));

    // Kim lists the commands it really has.
    let (_, v) = json(&sb, &["capabilities"]);
    let names: Vec<&str> = v["data"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    assert!(names.contains(&"tools.find") && names.contains(&"tools.invoke"));
}
