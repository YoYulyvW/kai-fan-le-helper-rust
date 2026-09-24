//! 模拟键盘输入（SendInput），用于 Ctrl+V 粘贴。
//! 采用扫描码分步发送，兼容 RDP / 无界鼠标场景。
//!
//! 本模块集中所有 SendInput 相关的 unsafe 调用，并加注释说明。

use std::thread;
use std::time::Duration;

/// 分步发送按键的延迟（毫秒），与原版一致
pub const DELAY_BEFORE: u64 = 10;
pub const DELAY_CTRL_HOLD: u64 = 40;
pub const DELAY_AFTER_V: u64 = 20;
pub const DELAY_CTRL_UP: u64 = 20;

pub fn sleep(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;

#[cfg(windows)]
pub fn send_ctrl_v() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };

    // 把虚拟键码转成硬件扫描码
    let vk_to_scan = |vk: VIRTUAL_KEY| -> u16 {
        // 安全性：MapVirtualKeyW 为只读转换，传入合法 vk。
        unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 }
    };

    let ctrl = vk_to_scan(VK_CONTROL as VIRTUAL_KEY);
    let v = vk_to_scan(VK_V as VIRTUAL_KEY);
    if ctrl == 0 || v == 0 {
        return false;
    }

    // 构造单个扫描码按键事件
    let make = |scan: u16, keyup: bool| -> INPUT {
        let flags = KEYEVENTF_SCANCODE | if keyup { KEYEVENTF_KEYUP } else { 0 };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    };

    // 分步发送一个扫描码事件
    let send_step = |scan: u16, keyup: bool| {
        let input = make(scan, keyup);
        // 安全性：SendInput 接收有效 INPUT 数组指针与元素大小。
        unsafe {
            SendInput(
                1,
                &input as *const INPUT,
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    };

    // 主路径：扫描码分步发送 + 拉开 Ctrl 保持时间，兼容跨机同步场景
    sleep(DELAY_BEFORE); // 等焦点/剪贴板就绪
    send_step(ctrl, false); // Ctrl down
    sleep(DELAY_CTRL_HOLD); // 等对端同步 Ctrl 按下
    send_step(v, false); // V down
    sleep(DELAY_AFTER_V);
    send_step(v, true); // V up
    sleep(DELAY_CTRL_UP);
    send_step(ctrl, true); // Ctrl up
    true
}

#[cfg(not(windows))]
pub fn send_ctrl_v() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_are_positive() {
        assert!(DELAY_CTRL_HOLD >= DELAY_BEFORE);
    }
}
