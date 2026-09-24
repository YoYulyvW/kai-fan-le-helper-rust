//! 全局热键：Win32 低级键盘钩子（WH_KEYBOARD_LL）。
//!
//! 设计（与原版行为一致）：
//! - 在独立线程安装钩子并运行 GetMessage 消息循环，保证跨进程/远程/注入按键都能捕获。
//! - 匹配热键时返回 1 屏蔽按键，阻止继续传给前台程序。
//! - 不过滤注入事件，兼容远程桌面（ToDesk/UU）、无界鼠标在被控端注入的按键。
//! - 触发节流：主热键 300ms，映射热键每键 300ms。
//!
//! 本模块集中所有 Win32 unsafe 调用，并加注释说明。

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 热键事件（从钩子线程发往 UI 线程）
#[derive(Debug, Clone, PartialEq, Eq)]
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

const THROTTLE: Duration = Duration::from_millis(300);

/// 钩子线程共享状态（供 C 回调读取）
struct HookShared {
    /// vk -> 事件
    keys: HashMap<u32, HotkeyEvent>,
    tx: Sender<HotkeyEvent>,
    last_main: Option<Instant>,
    last_map: HashMap<String, Instant>,
}

static SHARED: OnceLock<Mutex<HookShared>> = OnceLock::new();

/// 全局热键钩子管理器
pub struct HotkeyHook {
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl HotkeyHook {
    /// 创建并启动钩子线程。keys 为初始热键列表。
    pub fn install(keys: HashMap<u32, HotkeyEvent>, tx: Sender<HotkeyEvent>) -> Self {
        let shared = HookShared {
            keys,
            tx,
            last_main: None,
            last_map: HashMap::new(),
        };
        let _ = SHARED.set(Mutex::new(shared));

        let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let thread_id = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let r = running.clone();
        let tid_out = thread_id.clone();

        let thread = std::thread::spawn(move || {
            run_hook_thread(r, tid_out);
        });

        HotkeyHook { running, thread: Some(thread), thread_id }
    }

    /// 更新热键列表（不重启线程）
    pub fn update_keys(&self, keys: HashMap<u32, HotkeyEvent>) {
        if let Some(m) = SHARED.get() {
            if let Ok(mut s) = m.lock() {
                s.keys = keys;
            }
        }
    }

    /// 卸载钩子并结束线程
    pub fn uninstall(&mut self) {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
        // 通知消息循环退出
        let tid = self.thread_id.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            if tid != 0 {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
        #[cfg(not(windows))]
        let _ = tid;

        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for HotkeyHook {
    fn drop(&mut self) {
        self.uninstall();
    }
}

#[cfg(windows)]
fn run_hook_thread(
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tid_out: std::sync::Arc<std::sync::atomic::AtomicU32>,
) {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL,
    };

    // 记录本线程 ID，供 PostThreadMessageW 退出
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    tid_out.store(tid, Ordering::Relaxed);

    let hook = unsafe {
        let hmod = GetModuleHandleW(std::ptr::null());
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0)
    };
    crate::utils::log(&format!("hotkey: SetWindowsHookExW hook={} tid={}", hook, tid));
    if hook == 0 {
        return;
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while running.load(Ordering::Relaxed) {
        let ret = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        UnhookWindowsHookEx(hook);
    }
}

#[cfg(windows)]
unsafe extern "system" fn hook_proc(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    if code == 0 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode;
        let is_down = wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN;

        if let Some(m) = SHARED.get() {
            if let Ok(mut s) = m.lock() {
                if let Some(ev) = s.keys.get(&vk).cloned() {
                    if is_down {
                        let now = Instant::now();
                        let fire = match &ev {
                            HotkeyEvent::Main => {
                                let ok = s
                                    .last_main
                                    .map(|t| now.duration_since(t) >= THROTTLE)
                                    .unwrap_or(true);
                                if ok {
                                    s.last_main = Some(now);
                                }
                                ok
                            }
                            HotkeyEvent::Mapping(k) => {
                                let ok = s
                                    .last_map
                                    .get(k)
                                    .map(|t| now.duration_since(*t) >= THROTTLE)
                                    .unwrap_or(true);
                                if ok {
                                    s.last_map.insert(k.clone(), now);
                                }
                                ok
                            }
                        };
                        if fire {
                            crate::utils::log(&format!("hotkey: fire {:?}", ev));
                            let _ = s.tx.send(ev);
                        }
                    }
                    // 按下和抬起都屏蔽，避免热键传给前台程序
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(0, code, wparam, lparam)
}

#[cfg(not(windows))]
fn run_hook_thread(
    _running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _tid_out: std::sync::Arc<std::sync::atomic::AtomicU32>,
) {
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
