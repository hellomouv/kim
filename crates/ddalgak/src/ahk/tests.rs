//! Step 80: the AutoHotkey v2 fixtures (docs/contracts/examples/ddalgak-ahk-v1) against their
//! expected documents and reports (`DDALGAK_WRITE_AHK=1` writes them again), and the rules they
//! make against the schema and the engine's loader.

use super::*;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/examples/ddalgak-ahk-v1")
}

fn rules_schema() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/schemas/ddalgak-rules-v1.schema.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

/// Every fixture converts exactly as its expected file says; the document is valid and loads.
#[test]
fn the_fixtures_convert_as_documented() {
    let mut n = 0;
    for e in std::fs::read_dir(dir()).unwrap().flatten() {
        let p = e.path();
        if !is_ahk(&p) {
            continue;
        }
        n += 1;
        let src = std::fs::read_to_string(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let c = convert(&src, &format!("Imported from {name}"));
        let got = json!({ "document": c.document, "report": c.report(&name) });
        let expected = p.with_extension("expected.json");
        if std::env::var_os("DDALGAK_WRITE_AHK").is_some() {
            std::fs::write(
                &expected,
                serde_json::to_string_pretty(&got).unwrap() + "\n",
            )
            .unwrap();
        }
        let want: Value = serde_json::from_slice(&std::fs::read(&expected).unwrap()).unwrap();
        assert_eq!(got, want, "{name} differs from {}", expected.display());
        let errors = crate::jsonschema::Validator::new(&rules_schema())
            .validate(&c.document)
            .unwrap();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        // The engine loads every converted rule (consent comes later, in the import's review).
        let tmp =
            std::env::temp_dir().join(format!("ddalgak-ahk-{}-{name}.json", std::process::id()));
        std::fs::write(&tmp, c.document.to_string()).unwrap();
        let l = crate::rules::load(&tmp);
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(l.errors, 0, "{name}: {:?}", l.inactive);
        for i in &l.inactive {
            assert!(i.code == "consent_missing", "{name}: {i:?}");
        }
    }
    assert!(n >= 2);
}

#[test]
fn the_convertible_fixture_covers_the_subset_and_the_manual_one_converts_nothing_unsafe() {
    let c = convert(
        &std::fs::read_to_string(dir().join("01-convertible.ahk")).unwrap(),
        "t",
    );
    assert!(
        c.diagnostics.iter().all(|d| d.level != Level::Manual),
        "{:#?}",
        c.diagnostics
    );
    assert!(c.rules >= 18, "{}", c.rules);
    let rules = c.document["profiles"][0]["rules"]
        .as_array()
        .unwrap()
        .clone();
    let insert = rules
        .iter()
        .find(|r| r["trigger"]["keys"] == "Insert")
        .unwrap();
    assert_eq!(
        insert["steps"],
        json!([{ "type": "block" }]),
        "`Insert::return` disables the key"
    );
    let m = convert(
        &std::fs::read_to_string(dir().join("02-manual.ahk")).unwrap(),
        "t",
    );
    assert_eq!(m.rules, 0, "nothing in the manual fixture is approximated");
    let lines: Vec<usize> = m
        .diagnostics
        .iter()
        .filter(|d| d.level == Level::Manual)
        .map(|d| d.line)
        .collect();
    assert_eq!(
        lines,
        [3, 5, 7, 8, 9, 10, 11, 12, 13, 15, 16, 19, 20, 21],
        "{:#?}",
        m.diagnostics
    );
}

#[test]
fn send_strings_and_keys() {
    assert_eq!(parse_keys("^+v").unwrap().keys, "Ctrl+Shift+V");
    assert_eq!(parse_keys("<^>!a").unwrap().keys, "LCtrl+RAlt+A");
    assert!(parse_keys("~F14").unwrap().pass_through);
    assert!(parse_keys("^F15 up").unwrap().release);
    assert!(parse_keys("#c").unwrap().win);
    assert!(parse_keys("a & b").is_err());
    assert_eq!(
        parse_send("^c{Tab}^v", false).unwrap(),
        [
            Piece::Chord("Ctrl+C".into()),
            Piece::Chord("Tab".into()),
            Piece::Chord("Ctrl+V".into())
        ]
    );
    assert_eq!(
        parse_send("Hi{!}", false).unwrap(),
        [Piece::Text("Hi!".into())]
    );
    assert_eq!(
        parse_send("{Text}^raw", false).unwrap(),
        [Piece::Text("^raw".into())]
    );
    assert!(parse_send("{Shift down}", false).is_err());
    assert_eq!(quoted(r#""a`nb ""q""""#).as_deref(), Some("a\nb \"q\""));
}

/// Nothing is ever run: converting a script that would delete files only reports it.
#[test]
fn conversion_never_runs_anything() {
    let marker = std::env::temp_dir().join(format!("ddalgak-ahk-never-{}.txt", std::process::id()));
    let src = format!(
        "F13::FileAppend \"x\", \"{}\"\nF14::Run \"cmd.exe /c echo x > {}\"\n",
        marker.display(),
        marker.display()
    );
    let c = convert(&src, "t");
    assert_eq!(c.rules, 0);
    assert!(!marker.exists());
}
