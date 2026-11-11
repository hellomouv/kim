//! Step 76 (DDALGAK-FR-040…042, GUI_JOURNEYS J5): the test playground, headless (egui_kittest:
//! no window). Machine safety: the launcher is `FakeLauncher` (it records, never starts), the
//! engine is `NoEngine`, and Live types only into the playground's own field.

use ddalgak::launch::{Call, FakeLauncher};
use ddalgak::paths::Paths;
use ddalgak_editor::sync::NoEngine;
use ddalgak_editor::{EditorApp, Options};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

struct Sandbox {
    root: PathBuf,
    paths: Paths,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sandbox(tag: &str, rules: &Value) -> Sandbox {
    let root = std::env::temp_dir().join(format!("ddalgak-play-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::under(&root);
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.rules(), serde_json::to_vec_pretty(rules).unwrap()).unwrap();
    Sandbox { root, paths }
}

const GRANT: &str = "2026-11-04T09:00:00Z";

fn rules() -> Value {
    json!({
        "schema": "ddalgak.rules", "schema_version": 1, "hotkey_contract": 1, "revision": 3,
        "active_profile": "work",
        "profiles": [ { "id": "work", "title": "Work", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" } ],
            "rules": [
                { "id": "thanks", "title": "Thanks", "enabled": true,
                  "trigger": { "keys": "Ctrl+Shift+F2" }, "steps": [ { "type": "type_text", "text": "감사합니다." } ],
                  "consent": { "input-injection": { "granted_at": GRANT, "via": "gui" } } },
                { "id": "site", "title": "Site", "enabled": true,
                  "trigger": { "keys": "Ctrl+Shift+F3" }, "steps": [ { "type": "open_url", "url": "https://example.com/x?secret=1" } ],
                  "consent": { "launch-programs": { "granted_at": GRANT, "via": "gui",
                      "fingerprint": ddalgak::validate::launch_fingerprint(&json!({ "steps": [ { "type": "open_url", "url": "https://example.com/x?secret=1" } ] })) } } }
            ] },
          { "id": "games", "title": "Games", "default_mode": "base",
            "modes": [ { "id": "base", "title": "Base" } ],
            "rules": [
                { "id": "games-thanks", "title": "GG", "enabled": true,
                  "trigger": { "keys": "Ctrl+Shift+F2" }, "steps": [ { "type": "type_text", "text": "gg" } ],
                  "consent": { "input-injection": { "granted_at": GRANT, "via": "gui" } } }
            ] } ]
    })
}

fn harness(sb: &Sandbox, launcher: Arc<FakeLauncher>) -> Harness<'static, EditorApp> {
    let mut o = Options::for_test(sb.paths.clone(), Arc::new(NoEngine));
    o.launcher = launcher;
    Harness::builder()
        .with_size(egui::vec2(1280.0, 1400.0))
        .build_ui_state(|ui, app: &mut EditorApp| app.ui(ui), EditorApp::new(o))
}

fn click(h: &mut Harness<'static, EditorApp>, label: &str) {
    h.get_all_by_role_and_label(Role::Button, label)
        .next()
        .unwrap_or_else(|| panic!("no button {label}"))
        .click();
    h.run();
}

fn keys(h: &mut Harness<'static, EditorApp>, k: &str) {
    h.get_by_role_and_label(Role::TextInput, "Keys")
        .set_text_value_accesskit(k);
    h.run();
}

/// Simulate describes and changes nothing: the trace says who fires and what it would type,
/// the test field stays empty, the rules file is unchanged and nothing is launched.
#[test]
fn simulate_explains_and_does_nothing() {
    let sb = sandbox("sim", &rules());
    let before = std::fs::read(sb.paths.rules()).unwrap();
    let launcher = Arc::new(FakeLauncher::default());
    let mut h = harness(&sb, Arc::clone(&launcher));
    h.run();
    click(&mut h, "Playground");
    keys(&mut h, "Ctrl+Shift+F2");
    click(&mut h, "Run test");
    assert!(
        h.query_by_label_contains("thanks fires (Simulate)")
            .is_some()
    );
    assert!(
        h.query_by_label_contains("Types 6 characters on 1 line")
            .is_some()
    );
    let (field, trace) = h.state().playground();
    assert_eq!(field, "");
    assert_eq!(trace.unwrap()["kind"], "simulate");
    // Another profile, chosen in the page, is tested without switching to it.
    h.get_by_role_and_label(Role::ComboBox, "Profile").click();
    h.run();
    h.get_by_label("Games").click();
    h.run();
    click(&mut h, "Run test");
    let t = h.state().playground().1.unwrap().clone();
    assert_eq!(
        (t["profile"].as_str(), t["decision"]["rule"].as_str()),
        (Some("games"), Some("games-thanks"))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(sb.paths.rules()).unwrap()).unwrap()["active_profile"],
        "work"
    );
    // A key no rule takes passes through; a misconfigured key is refused in words.
    keys(&mut h, "F19");
    click(&mut h, "Run test");
    assert!(h.query_by_label_contains("No rule matches").is_some());
    keys(&mut h, "Ctrl+Nope");
    click(&mut h, "Run test");
    assert!(h.query_by_label_contains("is not a key or chord").is_some());
    assert_eq!(std::fs::read(sb.paths.rules()).unwrap(), before);
    assert!(launcher.calls.lock().unwrap().is_empty());
}

/// Live needs consent; then typing lands in the playground's own field, a website opens only
/// after Confirm and open, the trace never carries the URL's path or query, and Reset clears.
#[test]
fn live_types_into_the_field_and_opens_only_when_confirmed() {
    let sb = sandbox("live", &rules());
    let launcher = Arc::new(FakeLauncher::default());
    let mut h = harness(&sb, Arc::clone(&launcher));
    h.run();
    click(&mut h, "Playground");
    h.get_by_label("Live").click();
    h.run();
    keys(&mut h, "Ctrl+Shift+F2");
    assert!(
        h.get_all_by_role_and_label(Role::Button, "Run test")
            .next()
            .unwrap()
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_label_contains("I understand").click();
    h.run();
    click(&mut h, "Run test");
    assert_eq!(h.state().playground().0, "감사합니다.");
    assert_eq!(h.state().playground().1.unwrap()["injected_events"], 12);

    keys(&mut h, "Ctrl+Shift+F3");
    click(&mut h, "Run test");
    assert!(
        launcher.calls.lock().unwrap().is_empty(),
        "nothing opens before confirming"
    );
    let t = h.state().playground().1.unwrap().clone();
    assert_eq!(t["effects"][0]["needs_confirmation"], true, "{t:#}");
    assert!(!t.to_string().contains("secret"), "{t}");
    click(&mut h, "Confirm and open");
    let calls = launcher.calls.lock().unwrap().clone();
    assert!(
        matches!(&calls[..], [Call::Shell(r)] if r.target == "https://example.com/x?secret=1"),
        "{calls:?}"
    );
    let t = h.state().playground().1.unwrap().clone();
    assert_eq!(
        (
            t["effects"][0]["performed"].as_bool(),
            t["opened"][0].as_str()
        ),
        (Some(true), Some("https://example.com"))
    );
    click(&mut h, "Reset");
    assert_eq!(h.state().playground(), ("", None));
}

/// Step 77: the playground shows what fires in a chosen app before anything is activated: the
/// app's rule in that app, the global one elsewhere (the same matcher as the engine).
#[test]
fn the_playground_shows_the_winner_per_app() {
    let mut doc = rules();
    let mut code = doc["profiles"][0]["rules"][0].clone();
    code["id"] = json!("in-code");
    code["when"] = json!({ "apps": [ { "exe": "Code.exe" } ] });
    doc["profiles"][0]["rules"]
        .as_array_mut()
        .unwrap()
        .insert(0, code);
    let sb = sandbox("apps", &doc);
    let mut h = harness(&sb, Arc::new(FakeLauncher::default()));
    h.run();
    click(&mut h, "Playground");
    keys(&mut h, "Ctrl+Shift+F2");
    h.get_by_role_and_label(Role::TextInput, "Program (exe)")
        .set_text_value_accesskit("Code.exe");
    h.run();
    click(&mut h, "Run test");
    let t = h.state().playground().1.unwrap().clone();
    assert_eq!(t["decision"]["rule"], "in-code", "{t:#}");
    assert_eq!(t["decision"]["candidates"][1]["result"], "lower_precedence");
    h.get_by_role_and_label(Role::TextInput, "Program (exe)")
        .set_text_value_accesskit("notepad.exe");
    h.run();
    click(&mut h, "Run test");
    let t = h.state().playground().1.unwrap().clone();
    assert_eq!(t["decision"]["rule"], "thanks");
    assert_eq!(t["decision"]["candidates"][0]["result"], "condition_false");
}
