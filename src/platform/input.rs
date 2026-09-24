//! 模拟键盘输入（SendInput），用于 Ctrl+V 粘贴。
//! 采用扫描码分步发送，兼容 RDP / 无界鼠标场景。

use std::thread;
use std::time::Duration;

/// 发送 Ctrl+V（阻塞约 90ms）。返回是否成功。
#[cfg(windows)]
pub fn send_ctrl_v() -> bool {
    // 详细的 SendInput 实现在阶段 5 补充；此处先返回 false 占位。
    false
}

#[cfg(not(windows))]
pub fn send_ctrl_v() -> bool {
    false
}

/// 分步发送按键的延迟（毫秒），与原版一致
pub const DELAY_BEFORE: u64 = 10;
pub const DELAY_CTRL_HOLD: u64 = 40;
pub const DELAY_AFTER_V: u64 = 20;
pub const DELAY_CTRL_UP: u64 = 20;

pub fn sleep(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}
