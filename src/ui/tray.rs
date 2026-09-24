//! 系统托盘图标与右键菜单（原生 Shell_NotifyIcon 实现，Win7 兼容、零重依赖）。
//!
//! 菜单项：显示/隐藏、重新扫描、设置、退出。
//! 本模块集中托盘相关的 unsafe Win32 调用。

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
        TrackPopupMenu, IDI_APPLICATION, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD,
        TPM_RIGHTBUTTON,
    };

    /// 托盘回调消息（自定义）
    pub const WM_TRAYICON: u32 = 0x8001;

    /// 菜单命令 ID
    pub const ID_SHOW: usize = 1001;
    pub const ID_RESCAN: usize = 1002;
    pub const ID_SETTINGS: usize = 1003;
    pub const ID_QUIT: usize = 1004;

    pub struct Tray {
        hwnd: HWND,
        added: bool,
    }

    // 托盘句柄在主线程使用；此处仅保证可放入 Ctx。
    unsafe impl Send for Tray {}
    unsafe impl Sync for Tray {}

    impl Tray {
        /// 创建托盘图标并绑定到窗口
        pub fn new(hwnd: HWND) -> Self {
            let mut t = Tray { hwnd, added: false };
            t.add();
            t
        }

        fn add(&mut self) {
            unsafe {
                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = self.hwnd;
                nid.uID = 1;
                nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
                nid.uCallbackMessage = WM_TRAYICON;
                nid.hIcon = LoadIconW(0, IDI_APPLICATION);
                let tip: Vec<u16> = "开饭了助手\0".encode_utf16().collect();
                let n = tip.len().min(nid.szTip.len());
                nid.szTip[..n].copy_from_slice(&tip[..n]);
                if Shell_NotifyIconW(NIM_ADD, &nid) != 0 {
                    self.added = true;
                }
            }
        }

        /// 弹出右键菜单，返回被选中的命令 ID（0 表示取消）
        pub fn show_menu(&self) -> usize {
            unsafe {
                let menu = CreatePopupMenu();
                if menu == 0 {
                    return 0;
                }
                AppendMenuW(menu, MF_STRING, ID_SHOW, wide("显示/隐藏").as_ptr());
                AppendMenuW(menu, MF_STRING, ID_RESCAN, wide("重新扫描").as_ptr());
                AppendMenuW(menu, MF_STRING, ID_SETTINGS, wide("设置").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                AppendMenuW(menu, MF_STRING, ID_QUIT, wide("退出").as_ptr());

                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                SetForegroundWindow(self.hwnd);
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    pt.x,
                    pt.y,
                    0,
                    self.hwnd,
                    std::ptr::null(),
                ) as usize;
                DestroyMenu(menu);
                cmd
            }
        }

        pub fn remove(&mut self) {
            if self.added {
                unsafe {
                    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    nid.hWnd = self.hwnd;
                    nid.uID = 1;
                    Shell_NotifyIconW(NIM_DELETE, &nid);
                }
                self.added = false;
            }
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            self.remove();
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
pub struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub fn new(_hwnd: isize) -> Self {
        Tray
    }
    pub fn show_menu(&self) -> usize {
        0
    }
}
