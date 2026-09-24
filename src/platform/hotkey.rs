//! 全局热键：Win32 低级键盘钩子（WH_KEYBOARD_LL）+ 看门狗。
//! 实现在阶段 5 补充；此处定义对外接口。

/// 热键事件（从钩子线程发往 UI 线程）
#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// 主热键（默认 F1）触发
    Main,
    /// 快捷映射热键触发（如 F2/F3/F4）
    Mapping(String),
}

/// 虚拟键码映射：F1..F12 -> 0x70..0x7B
pub fn vk_for(key: &str) -> Option<u32> {
    let k = key.to_uppercase();
    if let Some(num) = k.strip_prefix('F') {
        if let Ok(n) = num.parse::<u32>() {
            if (1..=12).contains(&n) {
                return Some(0x6F + n);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_mapping() {
        assert_eq!(vk_for("F1"), Some(0x70));
        assert_eq!(vk_for("f12"), Some(0x7B));
        assert_eq!(vk_for("F13"), None);
        assert_eq!(vk_for("A"), None);
    }
}
