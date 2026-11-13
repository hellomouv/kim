//! #99 (`SET-21`): the language of Noeul's menus.
//!
//! `[general] language` is `auto` (the Windows display language: Korean when it is Korean, else
//! English), `en` or `ko`. The tray menu and the pin's context menu are translated; the overlay,
//! the settings window, messages and the CLI stay English (the matrix names that gap). Every
//! menu text has a Korean entry in [`KO`] (a test checks the menus against it).

use crate::settings::Language;

/// The language the menus are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

impl Lang {
    /// `auto` follows the Windows display language (`ui_korean`: whether it is Korean).
    pub fn of(setting: Language, ui_korean: bool) -> Lang {
        match setting {
            Language::En => Lang::En,
            Language::Ko => Lang::Ko,
            Language::Auto if ui_korean => Lang::Ko,
            Language::Auto => Lang::En,
        }
    }

    /// `text` in this language: the Korean entry of [`KO`], else the English text unchanged.
    pub fn tr(self, text: &str) -> String {
        match self {
            Lang::En => text.to_string(),
            Lang::Ko => KO
                .iter()
                .find(|(en, _)| *en == text)
                .map_or_else(|| text.to_string(), |(_, ko)| (*ko).to_string()),
        }
    }
}

/// Whether the Windows display language is Korean.
#[cfg(windows)]
pub fn ui_is_korean() -> bool {
    crate::win::ui_language_is_korean()
}

#[cfg(not(windows))]
pub fn ui_is_korean() -> bool {
    false
}

/// `(English, 한국어)` for the tray menu and the pin menu.
pub const KO: &[(&str, &str)] = &[
    // The tray menu.
    ("Capture region\tF1", "영역 캡처\tF1"),
    ("Pin clipboard\tF3", "클립보드 고정\tF3"),
    ("Hide all pins", "모든 핀 숨기기"),
    ("Show all pins", "모든 핀 보이기"),
    ("Bring pins on screen", "핀을 화면으로 가져오기"),
    ("Reopen closed pin", "닫은 핀 다시 열기"),
    ("Settings\u{2026}", "설정\u{2026}"),
    ("Pin last capture again", "마지막 캡처 다시 고정"),
    ("Restore pins from last time", "지난번 핀 복원"),
    ("Make all pins clickable", "모든 핀 클릭 가능하게"),
    ("Group", "그룹"),
    ("Keyboard shortcuts: on", "단축키: 켜짐"),
    ("Keyboard shortcuts: off", "단축키: 꺼짐"),
    ("Quit Noeul", "노을 끝내기"),
    // The pin menu.
    ("Copy", "복사"),
    ("Copy text", "텍스트 복사"),
    ("Save\u{2026}", "저장\u{2026}"),
    ("Quick save", "빠른 저장"),
    ("Annotate", "주석 달기"),
    ("Zoom in", "확대"),
    ("Zoom out", "축소"),
    ("Zoom 100 %, opaque", "100 %, 불투명"),
    ("Rotate right", "오른쪽으로 회전"),
    ("Rotate left", "왼쪽으로 회전"),
    ("Flip horizontally", "좌우 뒤집기"),
    ("Flip vertically", "상하 뒤집기"),
    ("More opaque", "더 불투명하게"),
    ("More transparent", "더 투명하게"),
    ("Thumbnail", "썸네일"),
    ("Grayscale", "흑백"),
    ("Invert colours", "색 반전"),
    ("Sharp pixels", "선명한 픽셀"),
    ("Always on top", "항상 위"),
    ("Replace with clipboard image", "클립보드 이미지로 바꾸기"),
    ("Copy colour under the pointer", "포인터 아래 색 복사"),
    ("Click-through", "클릭 통과"),
    ("Move to a new group", "새 그룹으로 옮기기"),
    ("Hide", "숨기기"),
    ("Close", "닫기"),
    ("Destroy", "완전히 닫기"),
    ("Pin clipboard as", "클립보드를 다음으로 고정"),
    ("now in", "현재"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_display_language_and_unknown_texts_stay_english() {
        assert_eq!(Lang::of(Language::Auto, true), Lang::Ko);
        assert_eq!(Lang::of(Language::Auto, false), Lang::En);
        assert_eq!(Lang::of(Language::En, true), Lang::En);
        assert_eq!(Lang::of(Language::Ko, false), Lang::Ko);
        assert_eq!(Lang::Ko.tr("Quit Noeul"), "노을 끝내기");
        assert_eq!(Lang::En.tr("Quit Noeul"), "Quit Noeul");
        assert_eq!(Lang::Ko.tr("Something new"), "Something new");
        let mut seen = std::collections::HashSet::new();
        assert!(KO.iter().all(|(en, _)| seen.insert(*en)), "no duplicates");
    }

    /// Every text of the tray menu and the pin menu has its Korean entry.
    #[test]
    fn every_menu_text_is_translated() {
        use crate::tray::MenuItem;
        let tray = [
            MenuItem::CaptureRegion,
            MenuItem::PinClipboard,
            MenuItem::HideAllPins,
            MenuItem::ShowAllPins,
            MenuItem::BringPinsOnScreen,
            MenuItem::ReopenClosedPin,
            MenuItem::Settings,
            MenuItem::PinLastCapture,
            MenuItem::RestorePins,
            MenuItem::MakeAllPinsClickable,
            MenuItem::Group(0),
            MenuItem::Shortcuts { on: true },
            MenuItem::Shortcuts { on: false },
            MenuItem::Quit,
        ];
        let pin = crate::app::MENU.iter().flatten().map(|(_, text)| *text);
        let missing: Vec<&str> = tray
            .iter()
            .map(|i| i.label())
            .chain(pin)
            .filter(|t| Lang::Ko.tr(t) == *t)
            .collect();
        assert!(missing.is_empty(), "untranslated: {missing:?}");
    }
}
