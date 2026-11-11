//! **Test fixture only — not a Kim utility, never packaged or installed by Kim.** Step 38
//! (ADR 0071): a second tool for the interoperability tests of `kim tools find|invoke`. Tests
//! copy it into a sandboxed programs directory themselves.
//!
//! It answers `capabilities --json` and `interop invoke --json --no-input` (a
//! `kim.interop.request` on stdin). `FIXTURE_MODE` picks a behaviour: `normal`, `old` (predates
//! interop: no `provides`, no `interop` command), `hang`, `garbage`, `fail`. `FIXTURE_VERSION`
//! sets its version, `FIXTURE_DIR` the only folder `fixture.touch`/`fixture.wipe` may change.

use serde_json::{Value, json};
use std::io::Read;
use std::process::ExitCode;

fn mode() -> String {
    std::env::var("FIXTURE_MODE").unwrap_or_else(|_| "normal".into())
}

fn version() -> String {
    std::env::var("FIXTURE_VERSION").unwrap_or_else(|_| "0.1.0".into())
}

#[allow(clippy::needless_pass_by_value)] // a test fixture: brevity over borrowing
fn envelope(command: &str, data: Value) -> Value {
    json!({"schema_version": 1, "command": command, "ok": true, "data": data, "warnings": []})
}

fn failure(command: &str, code: &str, exit: u8, message: &str) -> (Value, u8) {
    (
        json!({"schema_version": 1, "command": command, "ok": false, "error": {"code": code, "exit_code": exit, "message": message, "hint": null, "details": []}}),
        exit,
    )
}

fn capabilities() -> Value {
    let old = mode() == "old";
    let cmd = |name: &str, usage: &str, effect: &str| json!({"name": name, "usage": usage, "summary": "fixture", "effect": effect, "network": false, "dry_run": effect != "read", "requires_yes": effect == "destructive", "opens_window": false});
    let mut commands = vec![cmd(
        "capabilities",
        "kim-fixture capabilities [--json]",
        "read",
    )];
    let mut doc = json!({
        "schema": "kim.capabilities", "schema_version": 1, "tool": "kim-fixture", "version": version(),
        "cli_contract": 1, "json_schema_version": 1, "implementation": "implemented",
        "summary": "Test fixture for Kim interoperability tests; not a Kim utility",
    });
    if !old {
        commands.push(cmd(
            "interop.invoke",
            "kim-fixture interop invoke --json --no-input",
            "destructive",
        ));
        let p = |id: &str, v: &str, effect: &str, window: bool| json!({"id": id, "version": v, "summary": "fixture", "effect": effect, "network": false, "opens_window": window});
        doc["provides"] = json!([
            p("fixture.echo", "1.2.0", "read", false),
            p("fixture.touch", "1.0.0", "write", false),
            p("fixture.wipe", "1.0.0", "destructive", false),
            p("fixture.window", "1.0.0", "read", true),
            p("fixture.next", "2.0.0", "read", false),
        ]);
    }
    doc["commands"] = json!(commands);
    doc
}

/// A path the request may change: a plain name inside FIXTURE_DIR.
fn inside(name: &str) -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("FIXTURE_DIR")?;
    (!name.is_empty() && !name.contains(['/', '\\', ':']) && name != "..")
        .then(|| std::path::PathBuf::from(dir).join(name))
}

fn invoke() -> (Value, u8) {
    const C: &str = "interop.invoke";
    match mode().as_str() {
        "old" => return failure("kim-fixture", "usage", 2, "unknown command `interop`"),
        "hang" => std::thread::sleep(std::time::Duration::from_secs(120)),
        "garbage" => {
            print!("this is not an envelope");
            std::process::exit(0);
        }
        "fail" => {
            return failure(
                C,
                "io",
                1,
                "the disk is full; token=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            );
        }
        _ => {}
    }
    let mut input = String::new();
    if std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut input)
        .is_err()
        || input.len() > 64 * 1024
    {
        return failure(C, "usage", 2, "the request is missing or too large");
    }
    let Ok(req) = serde_json::from_str::<Value>(&input) else {
        return failure(C, "usage", 2, "the request is not JSON");
    };
    if req["schema"] != "kim.interop.request" || req["schema_version"] != 1 {
        return failure(C, "tool_incompatible", 5, "not a kim.interop.request v1");
    }
    let cap = req["capability"].as_str().unwrap_or("");
    let iface = req["interface"].as_str().unwrap_or("");
    let provided = capabilities()["provides"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let Some(p) = provided.iter().find(|p| p["id"] == cap) else {
        return failure(
            C,
            "capability_missing",
            5,
            &format!("kim-fixture does not provide `{cap}`"),
        );
    };
    let major = p["version"]
        .as_str()
        .and_then(|v| v.split('.').next())
        .unwrap_or("");
    if iface != major {
        return failure(
            C,
            "tool_incompatible",
            5,
            &format!("`{cap}` speaks interface {major}, not {iface}"),
        );
    }
    let params = &req["params"];
    let dry = req["dry_run"] == true;
    let data = match cap {
        "fixture.echo" | "fixture.next" => {
            json!({"echo": params["text"], "caller": req["caller"]["tool"]})
        }
        "fixture.window" => json!({"opened": false}),
        "fixture.touch" | "fixture.wipe" => {
            let Some(path) = params["name"].as_str().and_then(inside) else {
                return failure(C, "usage", 2, "`name` must be a plain file name");
            };
            if dry {
                json!({"dry_run": true, "would_change": path.display().to_string()})
            } else if cap == "fixture.touch" {
                let _ = std::fs::write(&path, b"touched");
                json!({"created": path.display().to_string()})
            } else {
                json!({"removed": std::fs::remove_file(&path).is_ok()})
            }
        }
        _ => unreachable!(),
    };
    (envelope(C, data), 0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let (doc, code) = match words.as_slice() {
        ["capabilities"] => (envelope("capabilities", capabilities()), 0),
        ["interop", "invoke"] => invoke(),
        _ => failure("kim-fixture", "usage", 2, "unknown command"),
    };
    println!("{doc}");
    ExitCode::from(code)
}
