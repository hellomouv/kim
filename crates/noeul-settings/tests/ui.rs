//! ADR 0093: Noeul's settings window drives the documented `noeul` commands, section by section,
//! in English and Korean (a fake runner answers like `noeul.exe` and records every command).

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use noeul_settings::{Runner, Section, SettingsApp};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Fake {
    log: Mutex<Vec<Vec<String>>>,
    config: Mutex<Value>,
}

impl Fake {
    fn new() -> Arc<Fake> {
        let f = Fake::default();
        *f.config.lock().unwrap() = json!({
            "tray_icon": true,
            "language": "en",
            "tray_clicks": {"left": "capture", "middle": "pin_clipboard"},
            "detection": "window",
            "magnifier": "on",
            "pointer": "off",
            "keyboard_help": "first-run",
            "quick_save_folder": null,
            "gestures": {"double_click": "pin", "middle_click": "pin", "preset": "noeul"},
            "pins": {"border": "on", "shadow": "on", "backup": "off", "double_click": "close"},
            "history": {"count": 20, "memory_mb": 256, "keep_days": 7, "disk": "off",
                        "keep_cancelled": "off", "auto_save": "off"},
            "privacy": {"cli": "notify", "mcp_captures": "off"},
            "output": {"format": "png", "jpeg_quality": 90, "copy_as_file": "off"},
            "paste": {"paths": "on", "file_cards": "off"},
            "color": {"format": "hex", "hex_case": "upper", "alpha": "off"},
            "path": "C:\\fake\\config.toml"
        });
        Arc::new(f)
    }

    fn commands(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.join(" "))
            .filter(|c| {
                !matches!(
                    c.as_str(),
                    "hotkeys list" | "config show" | "pin group list"
                )
            })
            .collect()
    }
}

impl Runner for Fake {
    fn run(&self, args: &[String]) -> Value {
        self.log.lock().unwrap().push(args.to_vec());
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["hotkeys", "list"] => json!({"ok": true, "data": {"actions": [
                {"id": "capture.region", "title": "Capture a region", "bindable": true,
                 "source": "default", "bindings": [{"keys": "F1", "active": true}]},
                {"id": "paste.float", "title": "Pin the clipboard", "bindable": true,
                 "source": "default", "bindings": [{"keys": "F3", "active": true}]}
            ]}}),
            ["config", "show"] => {
                json!({"ok": true, "data": {"settings": self.config.lock().unwrap().clone()}})
            }
            ["pin", "group", "list"] => json!({"ok": true, "data": {"current": "default",
                "groups": [{"name": "default", "pins": 2, "hidden": 0}]}}),
            ["config", "set", key, value] => {
                // Remember the value like config.toml would.
                let mut c = self.config.lock().unwrap();
                let path: Vec<&str> = match *key {
                    "pins.double_click" => vec!["pins", "double_click"],
                    "capture.detection" => vec!["detection"],
                    "general.tray_icon" => vec!["tray_icon"],
                    "general.language" => vec!["language"],
                    other => other.split('.').collect(),
                };
                let mut v = &mut *c;
                for p in &path[..path.len() - 1] {
                    v = &mut v[*p];
                }
                v[path[path.len() - 1]] = match *value {
                    "true" => json!(true),
                    "false" => json!(false),
                    s => json!(s),
                };
                json!({"ok": true, "data": {}})
            }
            _ => json!({"ok": true, "data": {}}),
        }
    }

    fn pause_hotkeys(&self, seconds: u64) {
        self.log.lock().unwrap().push(vec![
            "ipc".into(),
            "hotkeys.pause".into(),
            seconds.to_string(),
        ]);
    }
}

fn harness(fake: &Arc<Fake>) -> Harness<'static, SettingsApp> {
    let app = SettingsApp::new(fake.clone());
    Harness::builder()
        .with_size(egui::vec2(760.0, 620.0))
        .with_max_steps(12)
        .build_ui_state(|ui, app: &mut SettingsApp| app.ui(ui), app)
}

fn settle(h: &mut Harness<'static, SettingsApp>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(10)));
    h.run();
}

#[test]
fn every_section_is_on_the_rail_and_switches_save_through_config_set() {
    let fake = Fake::new();
    let mut h = harness(&fake);
    h.run();
    for s in Section::ALL {
        assert!(
            h.query_all_by_label(s.title()).count() >= 1,
            "{} on the rail",
            s.title()
        );
    }
    // General: the tray icon is a switch row.
    h.get_by_label("Tray icon").click();
    settle(&mut h);
    assert_eq!(fake.commands(), ["config set general.tray_icon false"]);
    assert_eq!(h.state().value(&["tray_icon"]), "false", "read back");
    // Capture: Detect is a segmented choice.
    h.get_by_label("Capture").click();
    h.run();
    h.get_by_label("Elements").click();
    settle(&mut h);
    assert!(
        fake.commands()
            .contains(&"config set capture.detection element".to_string())
    );
}

#[test]
fn double_click_on_a_pin_is_a_setting_with_close_by_default() {
    let fake = Fake::new();
    let mut h = harness(&fake);
    h.run();
    h.get_by_label("Pins").click();
    h.run();
    // `Close` is the setting's segment and each group's action.
    assert!(h.query_all_by_label("Close").count() >= 2);
    h.get_by_label("Nothing").click();
    settle(&mut h);
    assert!(
        fake.commands()
            .contains(&"config set pins.double_click none".to_string())
    );
    // The running instance's groups are listed with their actions.
    assert!(h.query_by_label("Switch").is_some());
}

#[test]
fn shortcuts_record_with_hotkeys_paused_and_can_be_picked() {
    let fake = Fake::new();
    let mut h = harness(&fake);
    h.run();
    h.get_by_label("Shortcuts").click();
    h.run();
    let field = h.get_by_label("Capture a region");
    assert_eq!(field.value().as_deref(), Some("F1"));
    field.click();
    settle(&mut h);
    assert_eq!(h.state().recording(), Some("capture.region"));
    assert!(
        fake.commands()
            .contains(&"ipc hotkeys.pause 30".to_string()),
        "{:?}",
        fake.commands()
    );
    h.key_press(egui::Key::F2);
    settle(&mut h);
    let c = fake.commands();
    assert!(
        c.contains(&"hotkeys set capture.region F2".to_string()),
        "{c:?}"
    );
    assert!(c.contains(&"ipc hotkeys.pause 0".to_string()), "{c:?}");
    assert_eq!(h.state().recording(), None);
    // Pick: PrintScreen cannot be pressed in a window; it can be chosen.
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run();
    assert!(h.query_by_label("Confirm").is_some(), "the picker is open");
    h.get_by_label("Search keys").click();
    h.run();
    h.get_by_label("Search keys").type_text("Print");
    h.run();
    h.get_by_label("PrintScreen").click();
    h.run();
    assert_eq!(
        h.state().picked(),
        Some(Some("PrintScreen".to_string())),
        "the key is chosen"
    );
    h.get_by_label("Confirm").click();
    settle(&mut h);
    assert!(
        fake.commands()
            .contains(&"hotkeys set capture.region PrintScreen".to_string()),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn descriptions_follow_noeuls_language_and_numbers_are_checked() {
    let fake = Fake::new();
    let mut h = harness(&fake);
    h.state_mut().set_lang(kim_ui::Lang::Ko);
    h.run();
    h.get_by_label("Advanced").click();
    h.run();
    assert!(
        h.query_by_label_contains("설정 파일:").is_some(),
        "Korean caption"
    );
    assert!(
        h.query_all_by_label("History").count() >= 1,
        "titles stay English"
    );
    // A number out of range shows a message and is not saved.
    h.get_all_by_role(egui::Role::TextInput)
        .next()
        .unwrap()
        .click();
    h.run();
    h.get_all_by_role(egui::Role::TextInput)
        .next()
        .unwrap()
        .type_text("999");
    h.run();
    assert!(h.query_by_label_contains("다음 범위의 정수").is_some());
    assert!(fake.commands().is_empty(), "{:?}", fake.commands());
}
