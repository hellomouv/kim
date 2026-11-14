//! What Windows says about the user's preferences, read-only: the app theme, "Show animations
//! in Windows" and the display language. Nothing here changes a setting.

/// Windows' "Choose your default app mode" is *Light* (`AppsUseLightTheme` = 1).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn apps_use_light_theme() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
        .encode_utf16()
        .chain([0])
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme".encode_utf16().chain([0]).collect();
    let mut v: u32 = 0;
    let mut len = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and live across the call; `v` and `len` are valid
    // for writes of a DWORD and its size.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut v).cast(),
            &raw mut len,
        )
    };
    r == 0 && v == 1
}

#[cfg(not(windows))]
pub fn apps_use_light_theme() -> bool {
    false
}

/// "Show animations in Windows" is on (`SPI_GETCLIENTAREAANIMATION`; on when it cannot be read).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn animations_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut on: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut on).cast(), 0) };
    ok == 0 || on != 0
}

#[cfg(not(windows))]
pub fn animations_enabled() -> bool {
    true
}

/// Windows' display language is Korean (primary language `LANG_KOREAN`, 0x12).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn ui_language_is_korean() -> bool {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    // SAFETY: no arguments; returns a LANGID.
    let id = unsafe { GetUserDefaultUILanguage() };
    id & 0x3ff == 0x12
}

#[cfg(not(windows))]
pub fn ui_language_is_korean() -> bool {
    false
}
