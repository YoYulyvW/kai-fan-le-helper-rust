//! Windows 平台能力封装：全局热键钩子、SendInput、注册表自启、主题检测。
//!
//! 所有 unsafe / Win32 调用都集中在本模块内，并加注释说明。

pub mod autostart;
pub mod clipboard;
pub mod foreground;
pub mod hotkey;
pub mod input;

/// 系统主题检测（浅色/深色），读取注册表 AppsUseLightTheme。
pub fn detect_system_theme() -> &'static str {
    theme::detect()
}

#[cfg(windows)]
mod theme {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    pub fn detect() -> &'static str {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        );
        if let Ok(k) = key {
            if let Ok(v) = k.get_value::<u32, _>("AppsUseLightTheme") {
                return if v == 1 { "light" } else { "dark" };
            }
        }
        "light"
    }
}

#[cfg(not(windows))]
mod theme {
    pub fn detect() -> &'static str {
        "light"
    }
}
