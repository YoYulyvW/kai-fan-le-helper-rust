//! Win32 主窗口：无边框置顶工具条 + 控件 + 消息循环。
//!
//! 布局（紧凑工具条）：
//!   [状态两行] [输入框] [📋] [🎲] [发送] [✕]
//!
//! 本模块集中窗口与控件相关的 unsafe Win32 调用。

use std::sync::{Arc, Mutex};

use crate::app::{App, AppEvent, UiAction};
use crate::config::{WIN_HEIGHT, WIN_WIDTH};
use crate::ui::tray::{Tray, ID_QUIT, ID_RESCAN, ID_SHOW, WM_TRAYICON};

/// 窗口类名
const CLASS_NAME: &str = "KaiFanLeHelperWnd";

// 控件 ID
const ID_INPUT: i32 = 1001;
const ID_BTN_HISTORY: i32 = 1002;
const ID_BTN_NAME: i32 = 1003;
const ID_BTN_SEND: i32 = 1004;
const ID_BTN_CLOSE: i32 = 1005;

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
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetWindowLongPtrW, LoadCursorW,
        PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowLongPtrW, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG,
        SW_SHOW, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN, WNDCLASSEXW, WS_CHILD,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    /// 主窗口上下文
    pub struct Ctx {
        pub app: Arc<Mutex<App>>,
        pub tray: Option<Tray>,
        pub input: HWND,
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

            let ctx = Box::new(Ctx {
                app: app.clone(),
                tray: None,
                input: 0,
            });
            let ctx_ptr = Box::into_raw(ctx);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

            // 创建子控件
            create_controls(hwnd, hinstance);

            // 系统托盘
            {
                let ctx = &mut *(ctx_ptr);
                ctx.tray = Some(Tray::new(hwnd));
                if let Ok(mut a) = ctx.app.lock() {
                    a.hwnd = hwnd as isize;
                }
            }

            ShowWindow(hwnd, SW_SHOW);

            let mut msg: MSG = std::mem::zeroed();
            loop {
                while windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut msg, 0, 0, 0, 1,
                ) != 0
                {
                    if msg.message == 0x0012 {
                        cleanup(hwnd);
                        return;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                // 处理应用后台事件
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    if let Ok(mut a) = ctx.app.lock() {
                        while let Some(ev) = a.try_event() {
                            let action = a.handle(ev);
                            apply_action(ctx, action);
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }

    /// 创建工具条子控件
    unsafe fn create_controls(hwnd: HWND, hinstance: isize) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, ES_AUTOHSCROLL, WS_BORDER, WS_TABSTOP,
        };

        let edit_class = to_wide("EDIT");
        let btn_class = to_wide("BUTTON");
        let empty = to_wide("");

        // 输入框
        let input = CreateWindowExW(
            0,
            edit_class.as_ptr(),
            empty.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | (ES_AUTOHSCROLL as u32),
            62, 10, 150, 24,
            hwnd,
            ID_INPUT as isize as _,
            hinstance,
            std::ptr::null(),
        );
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
        if !ptr.is_null() {
            (*ptr).input = input;
        }

        // 按钮：历史 / 名字 / 发送 / 关闭
        let mk_btn = |text: &str, id: i32, x: i32, w: i32| {
            CreateWindowExW(
                0,
                btn_class.as_ptr(),
                to_wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                x, 10, w, 24,
                hwnd,
                id as isize as _,
                hinstance,
                std::ptr::null(),
            );
        };
        mk_btn("📋", ID_BTN_HISTORY, 216, 28);
        mk_btn("🎲", ID_BTN_NAME, 246, 28);
        mk_btn("发送", ID_BTN_SEND, 276, 48);
        mk_btn("✕", ID_BTN_CLOSE, 328, 22);
    }

    /// 应用 UI 动作
    unsafe fn apply_action(ctx: &Ctx, action: UiAction) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowTextW, WM_SETTEXT};
        match action {
            UiAction::None => {}
            UiAction::Flash(_text, _color) => {
                // 状态栏文本更新（阶段 7 完善颜色渲染）
            }
            UiAction::SetInput(text) => {
                if ctx.input != 0 {
                    SetWindowTextW(ctx.input, to_wide(&text).as_ptr());
                }
            }
            UiAction::ShowMappingChooser(_key, _items) => {
                // 阶段 7 弹出选择窗口
            }
        }
        let _ = WM_SETTEXT;
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
        match msg {
            WM_CREATE => 0,
            m if m == WM_TRAYICON => {
                if lparam as u32 == 0x0205 {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                    if !ptr.is_null() {
                        let ctx = &mut *ptr;
                        let cmd = ctx.tray.as_ref().map(|t| t.show_menu()).unwrap_or(0);
                        match cmd {
                            ID_SHOW => {
                                use windows_sys::Win32::UI::WindowsAndMessaging::{
                                    IsWindowVisible, SW_HIDE,
                                };
                                if IsWindowVisible(hwnd) != 0 {
                                    ShowWindow(hwnd, SW_HIDE);
                                } else {
                                    ShowWindow(hwnd, SW_SHOW);
                                }
                            }
                            ID_RESCAN => {
                                if let Ok(mut a) = ctx.app.lock() {
                                    a.session.set_discovering(false);
                                }
                            }
                            ID_QUIT => PostQuitMessage(0),
                            _ => {}
                        }
                    }
                }
                0
            }
            WM_COMMAND => {
                let id = (wparam & 0xFFFF) as i32;
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    match id {
                        ID_BTN_CLOSE => {
                            use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
                            ShowWindow(hwnd, SW_HIDE);
                        }
                        ID_BTN_SEND => {
                            // 读取输入框文本并发送（阶段 7 完善）
                        }
                        ID_BTN_NAME => {
                            // 生成随机姓名并复制到剪贴板，同时填入输入框
                            let name = crate::core::generate_name();
                            crate::platform::clipboard::set_text(&name);
                            if ctx.input != 0 {
                                use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                                SetWindowTextW(ctx.input, to_wide(&name).as_ptr());
                            }
                        }
                        _ => {}
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                SendMessageW(hwnd, 0x00A1, 2, 0);
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
