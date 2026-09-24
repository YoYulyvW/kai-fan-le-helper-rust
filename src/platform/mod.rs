//! Windows 平台能力封装：全局热键钩子、SendInput、注册表自启。
//!
//! 所有 unsafe / Win32 调用都集中在本模块内，并加注释说明。

pub mod autostart;
pub mod hotkey;
pub mod input;

/// 系统主题检测（浅色/深色）
pub fn detect_system_theme() -> &'static str {
    "light"
}
