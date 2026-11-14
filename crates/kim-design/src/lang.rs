//! The UI language (DESIGN_SYSTEM §6): titles, navigation, feature and button names stay
//! English in both languages; explanations, descriptions, help and messages follow the user's
//! language. English and Korean are supported; the default follows Windows' display language.

/// A UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Ko,
}

impl Lang {
    /// `en` / `ko`.
    pub const fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
        }
    }

    /// `en`, `english`, `ko`, `kr`, `korean`, `한국어` (any case).
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_lowercase().as_str() {
            "en" | "english" | "en-us" | "en-gb" => Some(Lang::En),
            "ko" | "kr" | "korean" | "ko-kr" | "한국어" => Some(Lang::Ko),
            _ => None,
        }
    }

    /// The language's own name, as a language menu shows it.
    pub const fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ko => "한국어",
        }
    }

    /// Windows' display language: Korean when it is Korean, English otherwise.
    pub fn system() -> Lang {
        if crate::system::ui_language_is_korean() {
            Lang::Ko
        } else {
            Lang::En
        }
    }

    /// The text for this language.
    pub fn pick<'a>(self, en: &'a str, ko: &'a str) -> &'a str {
        match self {
            Lang::En => en,
            Lang::Ko => ko,
        }
    }
}

/// A saved language choice: follow Windows, or always English or Korean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LangPref {
    #[default]
    System,
    En,
    Ko,
}

impl LangPref {
    pub const ALL: [LangPref; 3] = [LangPref::System, LangPref::En, LangPref::Ko];

    /// `system`, `en` or `ko` (`auto` is read as `system`).
    pub const fn as_str(self) -> &'static str {
        match self {
            LangPref::System => "system",
            LangPref::En => "en",
            LangPref::Ko => "ko",
        }
    }

    pub fn parse(s: &str) -> Option<LangPref> {
        match s.trim().to_lowercase().as_str() {
            "system" | "auto" | "" => Some(LangPref::System),
            other => Lang::parse(other).map(|l| match l {
                Lang::En => LangPref::En,
                Lang::Ko => LangPref::Ko,
            }),
        }
    }

    /// The language to show now.
    pub fn resolve(self) -> Lang {
        match self {
            LangPref::System => Lang::system(),
            LangPref::En => Lang::En,
            LangPref::Ko => Lang::Ko,
        }
    }

    /// The label a language choice shows (`System (한국어)` names what it resolves to).
    pub fn label(self) -> String {
        match self {
            LangPref::System => format!("System ({})", Lang::system().native_name()),
            LangPref::En => Lang::En.native_name().to_string(),
            LangPref::Ko => Lang::Ko.native_name().to_string(),
        }
    }
}
