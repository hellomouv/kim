//! ADR 0089: `kim.exe` carries the Kim icon (the laver sheet, `assets/icon/kim.ico`) and its
//! version information, so Explorer, shortcuts and Task Manager show Kim.
#![cfg(windows)]

use kim_winres::{RT_GROUP_ICON, RT_ICON, RT_VERSION, parse_ico, resources, version_string};

#[test]
fn kim_exe_carries_the_kim_icon_and_its_name() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_kim")).unwrap();
    let res = resources(&exe).unwrap();
    let ico = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon/kim.ico"),
    )
    .unwrap();
    let images = parse_ico(&ico).unwrap();
    let icons: Vec<&Vec<u8>> = res
        .iter()
        .filter(|r| r.0 == RT_ICON)
        .map(|r| &r.2)
        .collect();
    assert_eq!(icons.len(), images.len(), "every size of kim.ico");
    for (got, want) in icons.iter().zip(&images) {
        assert_eq!(**got, want.data, "the same pixels as assets/icon/kim.ico");
    }
    assert_eq!(res.iter().filter(|r| r.0 == RT_GROUP_ICON).count(), 1);
    let version = &res.iter().find(|r| r.0 == RT_VERSION).unwrap().2;
    assert_eq!(
        version_string(version, "FileDescription").as_deref(),
        Some("Kim")
    );
    assert_eq!(
        version_string(version, "ProductVersion").as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
