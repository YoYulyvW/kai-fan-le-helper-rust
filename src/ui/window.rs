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
const ID_STATUS: i32 = 1000;
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
        pub status: HWND,
        /// 上次剪贴板内容（防抖去重）
        pub last_clipboard: String,
        /// 上次心跳时间
        pub last_heartbeat: std::time::Instant,
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
                status: 0,
                last_clipboard: String::new(),
                last_heartbeat: std::time::Instant::now(),
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

                    // 处理后台事件
                    if let Ok(mut a) = ctx.app.lock() {
                        while let Some(ev) = a.try_event() {
                            let action = a.handle(ev);
                            apply_action(ctx, action);
                        }
                    }

                    // 剪贴板监听（500ms 轮询，去重）
                    poll_clipboard(ctx);

                    // 心跳（每 HEARTBEAT_INTERVAL 秒）
                    if ctx.last_heartbeat.elapsed()
                        >= std::time::Duration::from_secs(crate::config::HEARTBEAT_INTERVAL)
                    {
                        ctx.last_heartbeat = std::time::Instant::now();
                        run_heartbeat(ctx);
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
        let static_class = to_wide("STATIC");
        let empty = to_wide("");

        // 状态栏（左侧）
        let status = CreateWindowExW(
            0,
            static_class.as_ptr(),
            to_wide("● 扫描中").as_ptr(),
            WS_CHILD | WS_VISIBLE, // SS_LEFT 为 0，默认左对齐
            6, 13, 52, 18,
            hwnd,
            ID_STATUS as isize as _,
            hinstance,
            std::ptr::null(),
        );
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
        if !ptr.is_null() {
            (*ptr).status = status;
        }

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

    /// 更新状态栏文本
    unsafe fn set_status(ctx: &Ctx, text: &str) {
        if ctx.status != 0 {
            use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
            SetWindowTextW(ctx.status, to_wide(text).as_ptr());
        }
    }

    /// 剪贴板轮询：读取文本，去重后交给业务处理
    fn poll_clipboard(ctx: &mut Ctx) {
        let text = crate::platform::clipboard::get_text();
        let text = text.trim().to_string();
        if text.is_empty() || text == ctx.last_clipboard {
            return;
        }
        ctx.last_clipboard = text.clone();

        // 命中分享文本则填入输入框
        if let Ok(mut a) = ctx.app.lock() {
            if let Some(display) = a.on_clipboard_text(&text) {
                unsafe {
                    if ctx.input != 0 {
                        use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                        SetWindowTextW(ctx.input, to_wide(&display).as_ptr());
                    }
                }
            }
        }
    }

    /// 心跳：在后台探测设备离线
    fn run_heartbeat(ctx: &mut Ctx) {
        let offline = if let Ok(a) = ctx.app.lock() {
            if a.session.devices.is_empty() {
                return;
            }
            a.heartbeat_offline()
        } else {
            return;
        };
        if offline.is_empty() {
            return;
        }
        if let Ok(mut a) = ctx.app.lock() {
            a.apply_heartbeat(&offline);
        }
    }

    /// 应用 UI 动作
    unsafe fn apply_action(ctx: &Ctx, action: UiAction) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowTextW, WM_SETTEXT};
        match action {
            UiAction::None => {}
            UiAction::Flash(text, _color) => {
                set_status(ctx, &format!("● {}", text));
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
                            // 读取输入框文本并发送
                            if ctx.input != 0 {
                                use windows_sys::Win32::UI::WindowsAndMessaging::{
                                    GetWindowTextLengthW, GetWindowTextW, SetWindowTextW,
                                };
                                let len = GetWindowTextLengthW(ctx.input);
                                if len > 0 {
                                    let mut buf = vec![0u16; (len + 1) as usize];
                                    GetWindowTextW(ctx.input, buf.as_mut_ptr(), len + 1);
                                    let text = String::from_utf16_lossy(&buf[..len as usize]);
                                    let text = text.trim().to_string();
                                    if !text.is_empty() {
                                        if let Ok(a) = ctx.app.lock() {
                                            if a.send_text(&text) {
                                                SetWindowTextW(ctx.input, to_wide("").as_ptr());
                                                set_status(ctx, "● 已发送");
                                            } else {
                                                set_status(ctx, "● 未连接");
                                            }
                                        }
                                    }
                                }
                            }
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
