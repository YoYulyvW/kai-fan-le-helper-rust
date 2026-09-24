//! Win32 主窗口：无边框置顶工具条 + 消息循环。
//!
//! 本模块集中窗口相关的 unsafe Win32 调用。

use std::sync::{Arc, Mutex};

use crate::app::{App, AppEvent};
use crate::config::{WIN_HEIGHT, WIN_WIDTH};
use crate::ui::tray::Tray;

/// 窗口尺寸（逻辑像素，后续按 DPI 缩放）
const CLASS_NAME: &str = "KaiFanLeHelperWnd";

/// 运行 UI 主循环（阻塞直到退出）。
pub fn run(app: App) {
    #[cfg(windows)]
    win::run(app);

    #[cfg(not(windows))]
    {
        let _ = app;
        eprintln!("当前平台暂不支持图形界面");
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, LoadCursorW, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW,
        ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
        IDC_ARROW, MSG, SW_SHOW, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSEXW,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    /// 主窗口上下文：持有 App 与托盘
    pub struct Ctx {
        pub app: Arc<Mutex<App>>,
        pub tray: Option<Tray>,
        pub visible: bool,
    }

    pub fn run(app: App) {
        let app = Arc::new(Mutex::new(app));

        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class_name = to_wide(CLASS_NAME);

            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinstance;
            wc.hCursor = LoadCursorW(0, IDC_ARROW);
            wc.lpszClassName = class_name.as_ptr();

            if RegisterClassExW(&wc) == 0 {
                eprintln!("注册窗口类失败");
                return;
            }

            // 无边框 + 置顶 + 工具窗口（不显示在任务栏）
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                to_wide("开饭了助手").as_ptr(),
                WS_POPUP,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                WIN_WIDTH,
                WIN_HEIGHT,
                0,
                0,
                hinstance,
                std::ptr::null(),
            );
            if hwnd == 0 {
                eprintln!("创建窗口失败");
                return;
            }

            // 将上下文指针存入窗口用户数据
            let ctx = Box::new(Ctx {
                app: app.clone(),
                tray: None,
                visible: true,
            });
            let ctx_ptr = Box::into_raw(ctx);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

            // 创建系统托盘
            {
                let ctx = &mut *(ctx_ptr);
                ctx.tray = Some(Tray::new(hwnd));
            }

            ShowWindow(hwnd, SW_SHOW);

            // 消息循环：同时泵窗口消息与应用事件
            let mut msg: MSG = std::mem::zeroed();
            loop {
                // 处理窗口消息（非阻塞）
                while windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut msg, 0, 0, 0, 1, // PM_REMOVE
                ) != 0
                {
                    if msg.message == 0x0012 {
                        // WM_QUIT
                        cleanup(hwnd);
                        return;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                // 处理应用后台事件
                if let Ok(mut a) = app.lock() {
                    while let Some(ev) = a.try_event() {
                        handle_app_event(hwnd, &mut a, ev);
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }

    fn handle_app_event(_hwnd: HWND, app: &mut App, ev: AppEvent) {
        if let Some(_msg) = app.handle(ev) {
            // 阶段 6 后续：更新窗口状态文本
        }
    }

    unsafe fn cleanup(hwnd: HWND) {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        use crate::ui::tray::{
            ID_QUIT, ID_RESCAN, ID_SETTINGS, ID_SHOW, WM_TRAYICON,
        };
        match msg {
            WM_CREATE => 0,
            // 托盘图标回调
            m if m == WM_TRAYICON => {
                // 右键（WM_RBUTTONUP = 0x0205）弹出菜单
                if lparam as u32 == 0x0205 {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                    if !ptr.is_null() {
                        let ctx = &mut *ptr;
                        let cmd = ctx
                            .tray
                            .as_ref()
                            .map(|t| t.show_menu())
                            .unwrap_or(0);
                        match cmd {
                            ID_SHOW => {
                                use windows_sys::Win32::UI::WindowsAndMessaging::{
                                    IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOW,
                                };
                                if IsWindowVisible(hwnd) != 0 {
                                    ShowWindow(hwnd, SW_HIDE);
                                } else {
                                    ShowWindow(hwnd, SW_SHOW);
                                }
                            }
                            ID_RESCAN => {
                                // 触发一次扫描（业务事件）
                                if let Ok(mut a) = ctx.app.lock() {
                                    a.session.set_discovering(false);
                                }
                            }
                            ID_SETTINGS => {
                                // 阶段 7 展开设置窗口
                            }
                            ID_QUIT => {
                                PostQuitMessage(0);
                            }
                            _ => {}
                        }
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                // 拖动窗口（无边框窗口需要手动处理）
                windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                    hwnd,
                    0x00A1, // WM_NCLBUTTONDOWN
                    2,      // HTCAPTION
                    0,
                );
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}
