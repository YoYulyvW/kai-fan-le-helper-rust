//! 前台窗口检测：判断当前焦点是否在本助手窗口 / 鼠标是否在窗口内。
//! 本模块集中相关 unsafe Win32 调用。

/// 当前前台窗口句柄
#[cfg(windows)]
pub fn foreground_hwnd() -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe { GetForegroundWindow() as isize }
}

/// 恢复指定窗口到前台
#[cfg(windows)]
pub fn restore_foreground(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    unsafe {
        if hwnd != 0 {
            SetForegroundWindow(hwnd as _);
        }
    }
}

/// 判断鼠标是否在指定窗口矩形内
#[cfg(windows)]
pub fn cursor_in_window(hwnd: isize) -> bool {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        let mut rect: RECT = std::mem::zeroed();
        if GetCursorPos(&mut pt) != 0 && GetWindowRect(hwnd as _, &mut rect) != 0 {
            return pt.x >= rect.left && pt.x <= rect.right && pt.y >= rect.top && pt.y <= rect.bottom;
        }
    }
    false
}

/// 指定窗口是否属于当前进程
#[cfg(windows)]
pub fn window_belongs_to_current_process(hwnd: isize) -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        if hwnd == 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd as _, &mut pid);
        pid == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
pub fn foreground_hwnd() -> isize {
    0
}

#[cfg(not(windows))]
pub fn restore_foreground(_hwnd: isize) {}

#[cfg(not(windows))]
pub fn cursor_in_window(_hwnd: isize) -> bool {
    false
}

#[cfg(not(windows))]
pub fn window_belongs_to_current_process(_hwnd: isize) -> bool {
    false
}
