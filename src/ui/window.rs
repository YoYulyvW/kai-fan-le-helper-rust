//! Win32 主窗口：无边框置顶工具条 + 控件 + 消息循环。
//!
//! 布局（紧凑工具条）：
//!   [状态两行] [输入框] [📋] [🎲] [发送] [✕]
//!
//! 本模块集中窗口与控件相关的 unsafe Win32 调用。

use std::sync::{Arc, Mutex};

use crate::app::{App, AppEvent, UiAction};
use crate::config::{WIN_HEIGHT, WIN_WIDTH};
use crate::ui::tray::{Tray, WM_TRAYICON};

/// 窗口类名
const CLASS_NAME: &str = "KaiFanLeHelperWnd";

// 弹窗控件 ID
const ID_POPUP_LIST: i32 = 2001;
const ID_POPUP_SEARCH: i32 = 2002;
const ID_POPUP_CLOSE: i32 = 2003;

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
        pub hwnd: HWND,
        pub input: HWND,
        pub status: HWND,
        pub popup: HWND,
        pub popup_list: HWND,
        pub popup_kind: u8, // 0=无 1=设备 2=历史 3=映射
        /// 上次剪贴板内容（防抖去重）
        pub last_clipboard: String,
        /// 上次心跳时间
        pub last_heartbeat: std::time::Instant,
        /// 上次热键看门狗时间
        pub last_watchdog: std::time::Instant,
        /// 当前主题配色
        pub palette: crate::ui::theme::Palette,
        /// 背景画刷句柄
        pub brush: isize,
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
                hwnd,
                input: 0,
                status: 0,
                popup: 0,
                popup_list: 0,
                popup_kind: 0,
                last_clipboard: String::new(),
                last_heartbeat: std::time::Instant::now(),
                last_watchdog: std::time::Instant::now(),
                palette: crate::ui::theme::Palette::for_theme(
                    crate::ui::theme::ThemeMode::Auto.resolve(),
                ),
                brush: 0,
            });
            let ctx_ptr = Box::into_raw(ctx);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

            // 创建子控件
            create_controls(hwnd, hinstance);

            // 创建背景画刷
            {
                use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
                let ctx = &mut *ctx_ptr;
                ctx.brush = CreateSolidBrush(ctx.palette.bg) as isize;
            }

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

                    // 处理后台事件（先取出所有动作，释放锁后再执行 UI 操作）
                    let mut actions = Vec::new();
                    if let Ok(mut a) = ctx.app.lock() {
                        while let Some(ev) = a.try_event() {
                            actions.push(a.handle(ev));
                        }
                    }
                    for action in actions {
                        apply_action(ctx, action);
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

                    // 热键看门狗（每 HOTKEY_WATCHDOG_INTERVAL 秒）
                    if ctx.last_watchdog.elapsed()
                        >= std::time::Duration::from_secs(crate::config::HOTKEY_WATCHDOG_INTERVAL)
                    {
                        ctx.last_watchdog = std::time::Instant::now();
                        if let Ok(a) = ctx.app.lock() {
                            a.hotkey_watchdog_tick();
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

    /// 处理托盘菜单命令
    unsafe fn handle_tray_command(hwnd: HWND, ctx: &mut Ctx, cmd: usize) {
        use crate::ui::tray::*;
        match cmd {
            ID_SHOW => {
                use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, SW_HIDE};
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
                set_status(ctx, "● 扫描中");
            }
            ID_AUTO_SCAN => toggle_setting(ctx, |s| s.auto_scan = !s.auto_scan),
            ID_AUTO_PUSH => toggle_setting(ctx, |s| s.auto_push = !s.auto_push),
            ID_CLEAR_CLIP => toggle_setting(ctx, |s| s.clear_clipboard = !s.clear_clipboard),
            ID_AUTOSTART => {
                let enabled = if let Ok(mut a) = ctx.app.lock() {
                    a.session.settings.autostart = !a.session.settings.autostart;
                    a.session.settings.save();
                    let v = a.session.settings.autostart;
                    crate::platform::autostart::set_autostart(v);
                    v
                } else {
                    false
                };
                set_status(ctx, if enabled { "● 已开启自启" } else { "● 已关闭自启" });
            }
            ID_QUIT => PostQuitMessage(0),
            c if (ID_HOTKEY_BASE..ID_HOTKEY_BASE + 13).contains(&c) => {
                let n = c - ID_HOTKEY_BASE;
                let key = format!("F{}", n);
                if let Ok(mut a) = ctx.app.lock() {
                    a.session.settings.hotkey = key.clone();
                    a.session.settings.save();
                    a.reload_hotkeys();
                }
                set_status(ctx, &format!("● 热键 {}", key));
            }
            c if (ID_THEME_AUTO..=ID_THEME_DARK).contains(&c) => {
                use crate::ui::theme::{Palette, ThemeMode};
                let mode = match c {
                    ID_THEME_LIGHT => ThemeMode::Light,
                    ID_THEME_DARK => ThemeMode::Dark,
                    _ => ThemeMode::Auto,
                };
                // 更新画刷
                if ctx.brush != 0 {
                    use windows_sys::Win32::Graphics::Gdi::DeleteObject;
                    DeleteObject(ctx.brush as _);
                }
                ctx.palette = Palette::for_theme(mode.resolve());
                use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
                ctx.brush = CreateSolidBrush(ctx.palette.bg) as isize;
                // 触发重绘
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 1);
                set_status(ctx, "● 主题已切换");
            }
            _ => {}
        }
    }

    /// 切换一个设置项并持久化
    unsafe fn toggle_setting<F: FnOnce(&mut crate::config::Settings)>(ctx: &Ctx, f: F) {
        if let Ok(mut a) = ctx.app.lock() {
            f(&mut a.session.settings);
            a.session.settings.save();
        }
    }

    /// 打开弹窗（1=设备 2=历史 3=映射）
    unsafe fn open_popup(hwnd: HWND, ctx: &mut Ctx, kind: u8) {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WS_CHILD, WS_VISIBLE, WS_BORDER, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_POPUP,
        };

        close_popup(ctx);

        let hinstance = GetModuleHandleW(std::ptr::null());
        let popup_class = to_wide("KaiFanLePopup");
        let list_class = to_wide("LISTBOX");
        let title = match kind {
            1 => "📱 选择设备",
            2 => "📋 历史记录",
            _ => "📌 选择内容",
        };

        // 注册弹窗类（如未注册）
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(DefWindowProcW);
        wc.hInstance = hinstance;
        wc.lpszClassName = popup_class.as_ptr();
        RegisterClassExW(&wc);

        let popup = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            popup_class.as_ptr(),
            to_wide(title).as_ptr(),
            WS_POPUP,
            100, 100, WIN_WIDTH, 280,
            hwnd,
            0,
            hinstance,
            std::ptr::null(),
        );
        if popup == 0 {
            return;
        }

        // 列表框
        let list = CreateWindowExW(
            0,
            list_class.as_ptr(),
            to_wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            8, 8, WIN_WIDTH - 24, 240,
            popup,
            ID_POPUP_LIST as isize as _,
            hinstance,
            std::ptr::null(),
        );

        // 填充列表
        if let Ok(a) = ctx.app.lock() {
            let items: Vec<String> = match kind {
                1 => a
                    .session
                    .devices
                    .iter()
                    .map(|d| format!("{}  ({})", d.name, d.ip))
                    .collect(),
                2 => a
                    .session
                    .history
                    .iter()
                    .map(|h| h.title.clone())
                    .collect(),
                _ => Vec::new(),
            };
            unsafe {
                for it in &items {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, LB_ADDSTRING};
                    SendMessageW(list, LB_ADDSTRING, 0, to_wide(it).as_ptr() as isize);
                }
            }
        }

        ShowWindow(popup, SW_SHOW);
        ctx.popup = popup;
        ctx.popup_list = list;
        ctx.popup_kind = kind;
    }

    /// 选中弹窗列表项（双击）
    unsafe fn select_popup_item(ctx: &mut Ctx) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, LB_GETCURSEL};
        if ctx.popup_list == 0 {
            return;
        }
        let sel = SendMessageW(ctx.popup_list, LB_GETCURSEL, 0, 0);
        if sel < 0 {
            return;
        }
        let idx = sel as usize;
        match ctx.popup_kind {
            1 => {
                if let Ok(mut a) = ctx.app.lock() {
                    if idx < a.session.devices.len() {
                        a.session.current_index = idx;
                    }
                }
            }
            2 => {
                let text = ctx
                    .app
                    .lock()
                    .ok()
                    .and_then(|a| a.session.history.get(idx).cloned())
                    .map(|h| h.text);
                if let Some(t) = text {
                    if ctx.input != 0 {
                        use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                        SetWindowTextW(ctx.input, to_wide(&t).as_ptr());
                    }
                }
            }
            _ => {}
        }
        close_popup(ctx);
    }

    /// 关闭弹窗
    unsafe fn close_popup(ctx: &mut Ctx) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, SW_HIDE};
        if ctx.popup != 0 {
            ShowWindow(ctx.popup, SW_HIDE);
            DestroyWindow(ctx.popup);
            ctx.popup = 0;
            ctx.popup_list = 0;
            ctx.popup_kind = 0;
        }
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
    unsafe fn apply_action(ctx: &mut Ctx, action: UiAction) {
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
                // 弹出映射选择窗口（kind=3）
                if ctx.hwnd != 0 {
                    let hwnd = ctx.hwnd;
                    open_popup(hwnd, ctx, 3);
                }
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
                        let cmd = if let Ok(a) = ctx.app.lock() {
                            ctx.tray
                                .as_ref()
                                .map(|t| {
                                    t.show_menu(
                                        a.session.settings.auto_scan,
                                        a.session.settings.auto_push,
                                        a.session.settings.clear_clipboard,
                                        a.session.settings.autostart,
                                        &a.session.settings.hotkey,
                                        "auto",
                                    )
                                })
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        handle_tray_command(hwnd, ctx, cmd);
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
                        ID_BTN_HISTORY => {
                            open_popup(hwnd, ctx, 2);
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
                        ID_POPUP_LIST => {
                            // 列表双击选择
                            let code = ((wparam >> 16) & 0xFFFF) as u32;
                            if code == 2 {
                                // LBN_DBLCLK
                                select_popup_item(ctx);
                            }
                        }
                        ID_POPUP_CLOSE => {
                            close_popup(ctx);
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
            // 控件文本颜色
            0x0138 | 0x0132 => {
                // WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                if !ptr.is_null() {
                    use windows_sys::Win32::Graphics::Gdi::SetTextColor;
                    let ctx = &*ptr;
                    let hdc = wparam as isize;
                    SetTextColor(hdc, ctx.palette.text);
                    if ctx.brush != 0 {
                        return ctx.brush as LRESULT;
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            0x0014 => {
                // WM_ERASEBKGND
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                if !ptr.is_null() {
                    let ctx = &*ptr;
                    if ctx.brush != 0 {
                        use windows_sys::Win32::Foundation::RECT;
                        use windows_sys::Win32::Graphics::Gdi::FillRect;
                        let hdc = wparam as isize;
                        let mut rect: RECT = std::mem::zeroed();
                        windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
                        FillRect(hdc, &rect, ctx.brush as _);
                        return 1;
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
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
