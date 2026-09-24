//! 系统托盘图标与右键菜单（原生 Shell_NotifyIcon 实现，Win7 兼容、零重依赖）。
//!
//! 菜单项覆盖原版全部入口：显示/隐藏、重新扫描、自动扫描、识别后自动推送、
//! 粘贴后清空剪贴板、开机自启、热键选择、主题、退出。
//! 本模块集中托盘相关的 unsafe Win32 调用。

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
        TrackPopupMenu, IDI_APPLICATION, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
        TPM_RETURNCMD, TPM_RIGHTBUTTON,
    };

    /// 托盘回调消息
    pub const WM_TRAYICON: u32 = 0x8001;

    // 一级命令
    pub const ID_SHOW: usize = 1001;
    pub const ID_RESCAN: usize = 1002;
    pub const ID_AUTO_SCAN: usize = 1003;
    pub const ID_AUTO_PUSH: usize = 1004;
    pub const ID_CLEAR_CLIP: usize = 1005;
    pub const ID_AUTOSTART: usize = 1006;
    pub const ID_QUIT: usize = 1007;

    // 热键命令：F1..F12 -> 1100..1111
    pub const ID_HOTKEY_BASE: usize = 1100;
    // 主题命令：auto/light/dark -> 1200/1201/1202
    pub const ID_THEME_AUTO: usize = 1200;
    pub const ID_THEME_LIGHT: usize = 1201;
    pub const ID_THEME_DARK: usize = 1202;

    pub struct Tray {
        hwnd: HWND,
        added: bool,
    }

    unsafe impl Send for Tray {}
    unsafe impl Sync for Tray {}

    impl Tray {
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
                let ok = Shell_NotifyIconW(NIM_ADD, &nid);
                crate::utils::log(&format!("tray: NIM_ADD result={} hwnd={}", ok, self.hwnd));
                if ok != 0 {
                    self.added = true;
                }
            }
        }

        /// 弹出右键菜单。参数为当前各项开关状态与当前热键/主题。
        #[allow(clippy::too_many_arguments)]
        pub fn show_menu(
            &self,
            auto_scan: bool,
            auto_push: bool,
            clear_clip: bool,
            autostart: bool,
            hotkey: &str,
            theme: &str,
        ) -> usize {
            unsafe {
                let menu = CreatePopupMenu();
                if menu == 0 {
                    return 0;
                }
                let check = |on: bool| if on { MF_CHECKED } else { 0 };

                AppendMenuW(menu, MF_STRING, ID_SHOW, wide("显示/隐藏").as_ptr());
                AppendMenuW(menu, MF_STRING, ID_RESCAN, wide("重新扫描").as_ptr());
                AppendMenuW(menu, MF_STRING | check(auto_scan), ID_AUTO_SCAN, wide("自动扫描").as_ptr());
                AppendMenuW(menu, MF_STRING | check(auto_push), ID_AUTO_PUSH, wide("识别后自动推送").as_ptr());
                AppendMenuW(menu, MF_STRING | check(clear_clip), ID_CLEAR_CLIP, wide("粘贴后清空剪贴板").as_ptr());
                AppendMenuW(menu, MF_STRING | check(autostart), ID_AUTOSTART, wide("开机自启动").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());

                // 热键子菜单
                let hk_menu = CreatePopupMenu();
                for i in 1..=12u32 {
                    let key = format!("F{}", i);
                    AppendMenuW(
                        hk_menu,
                        MF_STRING | check(hotkey == key),
                        ID_HOTKEY_BASE + i as usize,
                        wide(&key).as_ptr(),
                    );
                }
                AppendMenuW(menu, MF_POPUP, hk_menu as usize, wide("主热键").as_ptr());

                // 主题子菜单
                let theme_menu = CreatePopupMenu();
                AppendMenuW(theme_menu, MF_STRING | check(theme == "auto"), ID_THEME_AUTO, wide("自动").as_ptr());
                AppendMenuW(theme_menu, MF_STRING | check(theme == "light"), ID_THEME_LIGHT, wide("浅色").as_ptr());
                AppendMenuW(theme_menu, MF_STRING | check(theme == "dark"), ID_THEME_DARK, wide("深色").as_ptr());
                AppendMenuW(menu, MF_POPUP, theme_menu as usize, wide("主题").as_ptr());

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
}
