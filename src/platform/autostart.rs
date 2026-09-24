//! 开机自启动：读写 HKCU\...\Run 注册表项。

use std::path::PathBuf;

use crate::config::{AUTOSTART_REG_NAME, AUTOSTART_REG_PATH};

/// 当前可执行文件的绝对路径（用于写入注册表）
pub fn autostart_exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kai-fan-le-helper.exe"))
}

/// 设置/取消开机自启。成功返回 true。
#[cfg(windows)]
pub fn set_autostart(enable: bool) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(AUTOSTART_REG_PATH, KEY_SET_VALUE) {
        Ok(k) => k,
        Err(_) => return false,
    };
    if enable {
        let path = format!("\"{}\"", autostart_exe_path().display());
        key.set_value(AUTOSTART_REG_NAME, &path).is_ok()
    } else {
        // 不存在也视为成功
        let _ = key.delete_value(AUTOSTART_REG_NAME);
        true
    }
}

#[cfg(not(windows))]
pub fn set_autostart(_enable: bool) -> bool {
    false
}
