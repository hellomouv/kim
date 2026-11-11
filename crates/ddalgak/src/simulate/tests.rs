//! Step 76: the simulator against every `ddalgak-simulation-v1` contract example (each names its
//! rules example: `<rules example>--<what>.json`), and its output against the schema.

use super::*;
use std::path::PathBuf;

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/examples")
}

fn schema() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/schemas/ddalgak-simulation-v1.schema.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn valid(v: &Value) -> Vec<String> {
    crate::jsonschema::Validator::new(&schema())
        .validate(v)
        .unwrap()
}

/// The input an example was made from.
fn input_of(ex: &Value) -> Input {
    let c = &ex["context"];
    let e = &ex["event"];
    let pointer = ex["effects"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find_map(|f| f["preview"].get("from").filter(|p| p.is_object()))
        })
        .map(|p| {
            (
                p["x"].as_i64().unwrap() as i32,
                p["y"].as_i64().unwrap() as i32,
            )
        });
    Input {
        kind: if ex["kind"] == "live" {
            Kind::Live
        } else {
            Kind::Simulate
        },
        profile: ex["profile"].as_str().map(String::from),
        revision: ex["revision"].as_u64(),
        mode: c["mode"].as_str().map(String::from),
        paused: c["engine"] == "paused",
        focus: c["focus"].as_str().unwrap().to_string(),
        app_exe: c["app"]["exe"].as_str().map(String::from),
        window_title: c["app"]["window_title"].as_str().map(String::from),
        ime_composing: c["ime_composing"] == true,
        keys: e["keys"].as_str().unwrap().to_string(),
        on: e["on"].as_str().unwrap().to_string(),
        injected: e["source"] == "injected_by_other_program",
        pointer: pointer.unwrap_or((DESKTOP.2 / 2, DESKTOP.3 / 2)),
        confirmed: ex["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| f["confirmed"] == true)
            .filter_map(|f| f["step"].as_u64().map(|n| n as usize))
            .collect(),
    }
}

/// DDALGAK-FR-040: the decision, every effect's type, performance and preview, the injected
/// events and the opened programs equal what the contract examples say; the summaries are free
/// text. Previews that depend on this machine (whether a program exists) are compared by shape.
#[test]
fn every_contract_example_is_reproduced() {
    let dir = examples().join("ddalgak-simulation-v1");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = f.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        let ex: Value = serde_json::from_slice(&std::fs::read(f.path()).unwrap()).unwrap();
        let rules = name.split("--").next().unwrap();
        let doc: Document = serde_json::from_slice(
            &std::fs::read(
                examples()
                    .join("ddalgak-rules-v1")
                    .join(format!("{rules}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        let got = simulate(&doc, &input_of(&ex)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let errors = valid(&got);
        assert!(
            errors.is_empty(),
            "{name}: violates the schema: {errors:?}\n{got:#}"
        );
        let d = |v: &Value| {
            let mut d = v["decision"].clone();
            for c in d["candidates"].as_array_mut().into_iter().flatten() {
                c.as_object_mut().unwrap().remove("reason");
            }
            d
        };
        assert_eq!(d(&got), d(&ex), "{name}: decision\n{got:#}");
        let eff = |v: &Value| -> Vec<Value> {
            v["effects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    let mut e = e.clone();
                    let o = e.as_object_mut().unwrap();
                    o.remove("summary");
                    if o["type"] == "open_program" {
                        // `found` depends on this machine's files.
                        o.get_mut("preview")
                            .unwrap()
                            .as_object_mut()
                            .unwrap()
                            .remove("found");
                    }
                    e
                })
                .collect()
        };
        assert_eq!(eff(&got), eff(&ex), "{name}: effects\n{got:#}");
        assert_eq!(
            (&got["injected_events"], &got["opened"]),
            (&ex["injected_events"], &ex["opened"]),
            "{name}"
        );
        assert_eq!(got["kind"], ex["kind"], "{name}");
        n += 1;
    }
    assert!(n >= 23, "{n} examples");
}

#[allow(clippy::needless_pass_by_value)]
fn doc(rules: Value) -> Document {
    serde_json::from_value(
        json!({"schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1,
        "active_profile": "p", "profiles": [{"id": "p", "title": "P", "default_mode": "base",
        "modes": [{"id": "base", "title": "Base"}], "rules": rules}]}),
    )
    .unwrap()
}

fn grant() -> Value {
    json!({"input-injection": {"granted_at": "2026-11-01T00:00:00Z", "via": "gui"}})
}

/// Step 6 of the issue: a misconfigured key is refused in words; an injected event is skipped
/// unless the rule accepts it (no injected-event loop); a modifier collision picks one winner by
/// precedence; an IME composition steps aside; a rule without consent does not run.
#[test]
fn edge_cases_are_explained_never_run() {
    let d = doc(json!([
        {"id": "a-rule", "title": "A", "enabled": true, "trigger": {"keys": "Ctrl+Shift+K"}, "steps": [{"type": "type_text", "text": "x"}], "consent": grant()},
        {"id": "b-rule", "title": "B", "enabled": true, "trigger": {"keys": "Ctrl+Shift+K"}, "priority": 5, "steps": [{"type": "send_keys", "keys": "Ctrl+C"}], "consent": grant()},
        {"id": "no-consent", "title": "C", "enabled": true, "trigger": {"keys": "F20"}, "steps": [{"type": "type_text", "text": "x"}]}
    ]));
    let base = Input {
        keys: "Ctrl+Shift+K".into(),
        ..Input::default()
    };
    assert!(
        simulate(
            &d,
            &Input {
                keys: "Ctrl+Nope".into(),
                ..base.clone()
            }
        )
        .unwrap_err()
        .contains("not a key")
    );
    let v = simulate(&d, &base).unwrap();
    assert_eq!(
        (
            v["decision"]["outcome"].as_str(),
            v["decision"]["rule"].as_str()
        ),
        (Some("fired"), Some("b-rule")),
        "higher priority wins: {v:#}"
    );
    assert_eq!(v["decision"]["candidates"][1]["result"], "lower_precedence");
    assert!(valid(&v).is_empty());
    let v = simulate(
        &d,
        &Input {
            injected: true,
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(
        v["decision"]["code"], "skipped_injected_input",
        "no loop through injected input"
    );
    let v = simulate(
        &d,
        &Input {
            ime_composing: true,
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(v["decision"]["code"], "skipped_ime_composition");
    let v = simulate(
        &d,
        &Input {
            keys: "F20".into(),
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(v["decision"]["code"], "consent_missing");
    let v = simulate(
        &d,
        &Input {
            keys: "F21".into(),
            ..base.clone()
        },
    )
    .unwrap();
    assert_eq!(
        (
            v["decision"]["outcome"].as_str(),
            v["decision"]["key_reaches_app"].as_bool()
        ),
        (Some("no_match"), Some(true))
    );
    // Live types into the playground only: performed, counted, nothing opened.
    let v = simulate(
        &d,
        &Input {
            kind: Kind::Live,
            ..base
        },
    )
    .unwrap();
    assert_eq!(
        (
            v["effects"][0]["performed"].as_bool(),
            v["live_target"].as_str()
        ),
        (Some(true), Some("playground"))
    );
    assert!(valid(&v).is_empty(), "{:?}", valid(&v));
}

/// A launch grant given for other targets does not count (the engine's fingerprint check).
#[test]
fn a_stale_launch_grant_is_consent_missing() {
    let mut r = json!({"id": "site", "title": "S", "enabled": true, "trigger": {"keys": "F20"},
        "steps": [{"type": "open_url", "url": "https://example.com/"}]});
    let fp = crate::validate::launch_fingerprint(&r);
    r["consent"] = json!({"launch-programs": {"granted_at": "2026-11-01T00:00:00Z", "via": "gui", "fingerprint": fp}});
    let base = Input {
        keys: "F20".into(),
        ..Input::default()
    };
    assert_eq!(
        simulate(&doc(json!([r.clone()])), &base).unwrap()["decision"]["outcome"],
        "fired"
    );
    r["steps"][0]["url"] = json!("https://other.example/");
    assert_eq!(
        simulate(&doc(json!([r])), &base).unwrap()["decision"]["code"],
        "consent_missing"
    );
}
