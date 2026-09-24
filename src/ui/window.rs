//! 原生 Win32 自绘工具条窗口。
//!
//! 特点：
//! - 无边框、置顶、工具窗口，定位右上角
//! - 手动 GDI 绘制圆角面板、按钮、双行状态（复刻 Python 版观感）
//! - 自绘命中测试，无系统灰控件
//!
//! 所有 Win32 unsafe 调用集中在本模块并加注释。

use std::sync::{Arc, Mutex};

use crate::app::App;

/// 运行 UI 主循环（阻塞直到退出）。
pub fn run(app: App) {
    #[cfg(windows)]
    win::run(app);

    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, SetBkMode,
        SetTextColor, TextOutW, HDC, PAINTSTRUCT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
        GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, LoadCursorW, PeekMessageW,
        PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowLongPtrW, ShowWindow,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, LB_ADDSTRING,
        LB_GETCURSEL, MSG, SM_CXSCREEN, SW_SHOW, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN,
        WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_BORDER, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP, WS_VISIBLE, WS_VSCROLL,
    };

    // 工具条尺寸（像素）
    const BAR_W: i32 = 430;
    const BAR_H: i32 = 44;

    // 按钮区域（x, w）
    const BTN_HISTORY: (i32, i32) = (216, 44);
    const BTN_NAME: (i32, i32) = (264, 44);
    const BTN_SEND: (i32, i32) = (312, 48);
    const BTN_CLOSE: (i32, i32) = (366, 26);

    const TIMER_ID: usize = 1;
    const ID_POPUP_LIST: i32 = 3001;

    /// 主题配色（深色，RGB）
    struct Colors;
    impl Colors {
        const BG: u32 = rgb(28, 28, 30);
        const BORDER: u32 = rgb(58, 58, 60);
        const TEXT: u32 = rgb(255, 255, 255);
        const TEXT_SUB: u32 = rgb(142, 142, 147);
        const INPUT_BG: u32 = rgb(44, 44, 46);
        const BTN_BG: u32 = rgb(58, 58, 60);
        const OK: u32 = rgb(52, 199, 89);
        const WARN: u32 = rgb(255, 149, 0);
        const ERR: u32 = rgb(255, 59, 48);
        const INPUT_FG: u32 = rgb(200, 200, 205);
    }

    const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
    }

    /// 主窗口上下文
    pub struct Ctx {
        pub app: Arc<Mutex<App>>,
        pub tray: Option<crate::ui::tray::Tray>,
        /// 输入框文本
        pub input: String,
        /// 状态行1（如"● 已连接"）
        pub status1: String,
        pub status1_color: u32,
        /// 状态行2（设备名）
        pub status2: String,
        /// 闪烁提示
        pub flash_until: Option<std::time::Instant>,
        pub flash_text: String,
        pub flash_color: u32,
        /// 最近剪贴板
        pub last_clipboard: String,
        /// 心跳/扫描计时
        pub last_scan: std::time::Instant,
        /// 历史弹窗句柄
        pub popup: HWND,
        pub popup_list: HWND,
    }

    pub fn run(app: App) {
        let app = Arc::new(Mutex::new(app));

        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class_name = to_wide("KaiFanLeHelperWnd");

            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinstance;
            wc.hCursor = LoadCursorW(0, IDC_ARROW);
            wc.lpszClassName = class_name.as_ptr();
            RegisterClassExW(&wc);

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let x = screen_w - BAR_W - 20;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                to_wide("开饭了助手").as_ptr(),
                WS_POPUP,
                x,
                20,
                BAR_W,
                BAR_H,
                0,
                0,
                hinstance,
                std::ptr::null(),
            );
            if hwnd == 0 {
                return;
            }

            let ctx = Box::new(Ctx {
                app: app.clone(),
                tray: None,
                input: String::new(),
                status1: "● 扫描中".to_string(),
                status1_color: Colors::WARN,
                status2: String::new(),
                flash_until: None,
                flash_text: String::new(),
                flash_color: Colors::OK,
                last_clipboard: String::new(),
                last_scan: std::time::Instant::now(),
                popup: 0,
                popup_list: 0,
            });
            let ctx_ptr = Box::into_raw(ctx);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

            // 托盘
            {
                let ctx = &mut *ctx_ptr;
                ctx.tray = Some(crate::ui::tray::Tray::new(hwnd));
                if let Ok(mut a) = ctx.app.lock() {
                    a.hwnd = hwnd as isize;
                }
            }

            // 启动即扫描（修复连不上手机的关键）
            {
                let ctx = &mut *ctx_ptr;
                if let Ok(mut a) = ctx.app.lock() {
                    if a.session.settings.auto_scan {
                        a.start_scan();
                    }
                }
            }

            ShowWindow(hwnd, SW_SHOW);
            // 设置定时器，每 80ms 泵一次事件/重绘
            windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(hwnd, TIMER_ID, 80, None);

            let mut msg: MSG = std::mem::zeroed();
            // 消息循环：PeekMessage 非阻塞，无消息时也让出 CPU
            loop {
                if PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                    if msg.message == 0x0012 {
                        // WM_QUIT
                        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                        if !ptr.is_null() {
                            drop(Box::from_raw(ptr));
                        }
                        return;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    // 无消息时等待事件（阻塞式），避免空转
                    windows_sys::Win32::UI::WindowsAndMessaging::WaitMessage();
                }
            }
        }
    }

    /// 泵后台事件并更新状态
    unsafe fn pump(ctx: &mut Ctx) {
        let mut actions = Vec::new();
        if let Ok(mut a) = ctx.app.lock() {
            while let Some(ev) = a.try_event() {
                actions.push(a.handle(ev));
            }
        }
        for action in actions {
            use crate::app::UiAction;
            match action {
                UiAction::None => {}
                UiAction::Flash(text, color) => {
                    ctx.flash_text = text.clone();
                    ctx.flash_color = parse_color(&color);
                    ctx.flash_until = Some(std::time::Instant::now());
                }
                UiAction::SetInput(text) => ctx.input = text,
                UiAction::ShowMappingChooser(_, _) => {}
            }
        }
        // 更新状态文本
        if let Ok(a) = ctx.app.lock() {
            let n = a.session.devices.len();
            if a.session.is_discovering() {
                ctx.status1 = "● 扫描中".to_string();
                ctx.status1_color = Colors::WARN;
                ctx.status2.clear();
            } else if n == 0 {
                ctx.status1 = "● 未找到".to_string();
                ctx.status1_color = Colors::ERR;
                ctx.status2.clear();
            } else {
                ctx.status1 = if n == 1 {
                    "● 已连接".to_string()
                } else {
                    format!("● 已连接 ({})", n)
                };
                ctx.status1_color = Colors::OK;
                ctx.status2 = a.session.current_name().unwrap_or("").to_string();
            }
        }
    }

    /// 颜色字符串 "#RRGGBB" -> COLORREF
    fn parse_color(s: &str) -> u32 {
        let s = s.trim_start_matches('#');
        if s.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&s[0..2], 16),
                u8::from_str_radix(&s[2..4], 16),
                u8::from_str_radix(&s[4..6], 16),
            ) {
                return rgb(r, g, b);
            }
        }
        Colors::OK
    }

    /// 绘制整个工具条
    unsafe fn paint(hwnd: HWND, ctx: &Ctx) {
        use windows_sys::Win32::Graphics::Gdi::{
            CreateFontW, DeleteObject as DelObj, SelectObject, CLIP_DEFAULT_PRECIS,
            DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, OUT_TT_PRECIS,
        };
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);

        // 选择微软雅黑字体；高度取负值 = 字符高度；质量 5 = CLEARTYPE_QUALITY
        const CLEARTYPE_QUALITY: u32 = 5;
        let font = CreateFontW(
            -15, 0, 0, 0, 400, 0, 0, 0,
            DEFAULT_CHARSET as u32,
            OUT_TT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH as u32 | FF_DONTCARE as u32,
            to_wide("Microsoft YaHei UI").as_ptr(),
        );
        let old_font = SelectObject(hdc, font);

        // 背景
        let bg = CreateSolidBrush(Colors::BG);
        FillRect(hdc, &rect, bg);
        DeleteObject(bg);

        SetBkMode(hdc, TRANSPARENT as i32);

        // 状态行（左侧两行）
        let (s1, s2) = if let Some(t) = ctx.flash_until {
            if t.elapsed() < std::time::Duration::from_millis(1500) {
                (ctx.flash_text.clone(), String::new())
            } else {
                (ctx.status1.clone(), ctx.status2.clone())
            }
        } else {
            (ctx.status1.clone(), ctx.status2.clone())
        };

        let s1_color = if ctx.flash_until.is_some()
            && ctx.flash_until.unwrap().elapsed() < std::time::Duration::from_millis(1500)
        {
            ctx.flash_color
        } else {
            ctx.status1_color
        };

        draw_text(hdc, 8, 6, &s1, s1_color, 12);
        if !s2.is_empty() {
            draw_text(hdc, 8, 24, &s2, Colors::TEXT_SUB, 11);
        }

        // 输入框背景
        let input_rect = RECT { left: 96, top: 8, right: 96 + 160, bottom: 36 };
        let input_bg = CreateSolidBrush(Colors::INPUT_BG);
        FillRect(hdc, &input_rect, input_bg);
        DeleteObject(input_bg);
        // 输入文字
        let shown = if ctx.input.is_empty() {
            "等待剪贴板...".to_string()
        } else {
            ctx.input.clone()
        };
        let input_color = if ctx.input.is_empty() {
            Colors::TEXT_SUB
        } else {
            Colors::TEXT
        };
        draw_text(hdc, 96 + 8, 14, &shown, input_color, 12);

        // 按钮
        draw_button(hdc, BTN_HISTORY, "历史", Colors::BTN_BG, Colors::TEXT);
        draw_button(hdc, BTN_NAME, "起名", Colors::BTN_BG, Colors::TEXT);
        draw_button(hdc, BTN_SEND, "发送", Colors::OK, rgb(255, 255, 255));
        draw_button(hdc, BTN_CLOSE, "✕", Colors::BG, Colors::TEXT_SUB);

        SelectObject(hdc, old_font);
        DelObj(font);
        EndPaint(hwnd, &ps);
    }

    /// 画一个按钮（圆角矩形 + 文本）
    unsafe fn draw_button(hdc: HDC, area: (i32, i32), label: &str, bg: u32, fg: u32) {
        let (x, w) = area;
        let r = RECT { left: x, top: 8, right: x + w, bottom: 36 };
        // 圆角近似：用矩形（GDI 无圆角 FillRect，圆角需 GDI+，这里用矩形保持轻量）
        let brush = CreateSolidBrush(bg);
        FillRect(hdc, &r, brush);
        DeleteObject(brush);
        // 居中文字
        let (tw, _) = text_size(hdc, label, 12);
        let tx = x + (w - tw) / 2;
        draw_text(hdc, tx, 14, label, fg, 12);
    }

    /// 估算文本宽（中文按 12px，ASCII 按 6px 近似）
    unsafe fn text_size(_hdc: HDC, s: &str, size: i32) -> (i32, i32) {
        let mut w = 0i32;
        for c in s.chars() {
            if c.is_ascii() {
                w += size / 2;
            } else {
                w += size;
            }
        }
        (w, size)
    }

    /// 绘制文本
    unsafe fn draw_text(hdc: HDC, x: i32, y: i32, s: &str, color: u32, _size: i32) {
        SetTextColor(hdc, color);
        let wide = to_wide(s);
        TextOutW(hdc, x, y, wide.as_ptr(), (wide.len() - 1) as i32);
    }

    /// 弹出历史记录列表（原生 LISTBOX）
    unsafe fn show_history_popup(parent: HWND, ctx: &mut Ctx) {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let cls = to_wide("KaiFanLePopup");
        let list_cls = to_wide("LISTBOX");

        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(popup_proc);
        wc.hInstance = hinstance;
        wc.lpszClassName = cls.as_ptr();
        RegisterClassExW(&wc);

        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(parent, &mut rect);

        let popup = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            to_wide("历史记录").as_ptr(),
            WS_POPUP | WS_BORDER,
            rect.left,
            rect.bottom + 4,
            320,
            320,
            parent,
            0,
            hinstance,
            std::ptr::null(),
        );
        if popup == 0 {
            return;
        }

        let list = CreateWindowExW(
            0,
            list_cls.as_ptr(),
            to_wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL,
            4,
            4,
            312,
            312,
            popup,
            ID_POPUP_LIST as isize as _,
            hinstance,
            std::ptr::null(),
        );

        if let Ok(a) = ctx.app.lock() {
            for it in &a.session.history {
                SendMessageW(list, LB_ADDSTRING, 0, to_wide(&it.title).as_ptr() as isize);
            }
        }

        SetWindowLongPtrW(popup, GWLP_USERDATA, ctx as *mut Ctx as isize);
        ShowWindow(popup, SW_SHOW);

        ctx.popup = popup;
        ctx.popup_list = list;
    }

    /// 历史弹窗消息：双击选中填入输入框
    unsafe extern "system" fn popup_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_COMMAND => {
                let code = ((wparam >> 16) & 0xFFFF) as u32;
                if code == 2 {
                    // LBN_DBLCLK
                    let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                    if !ctx.is_null() {
                        let list = (*ctx).popup_list;
                        let sel = SendMessageW(list, LB_GETCURSEL, 0, 0);
                        if sel >= 0 {
                            if let Ok(a) = (*ctx).app.lock() {
                                if let Some(it) = a.session.history.get(sel as usize) {
                                    (*ctx).input = it.text.clone();
                                }
                            }
                        }
                        DestroyWindow(hwnd);
                        (*ctx).popup = 0;
                        (*ctx).popup_list = 0;
                    }
                }
                0
            }
            WM_DESTROY => 0,
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;

        match msg {
            WM_CREATE => 0,
            WM_TIMER => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    pump(ctx);
                    // 剪贴板轮询
                    let text = crate::platform::clipboard::get_text();
                    let text = text.trim().to_string();
                    if !text.is_empty() && text != ctx.last_clipboard {
                        ctx.last_clipboard = text.clone();
                        if let Ok(mut a) = ctx.app.lock() {
                            if let Some(display) = a.on_clipboard_text(&text) {
                                ctx.input = display;
                            }
                        }
                    }
                    // 定时重扫（未找到设备时每 3s）
                    if ctx.last_scan.elapsed() >= std::time::Duration::from_secs(3) {
                        ctx.last_scan = std::time::Instant::now();
                        if let Ok(mut a) = ctx.app.lock() {
                            if a.session.settings.auto_scan
                                && a.session.devices.is_empty()
                                && !a.session.is_discovering()
                            {
                                a.start_scan();
                            }
                        }
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                0
            }
            WM_PAINT => {
                if !ptr.is_null() {
                    paint(hwnd, &*ptr);
                }
                0
            }
            WM_LBUTTONDOWN => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    let x = (lparam & 0xFFFF) as i16 as i32;
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    if y >= 8 && y <= 36 {
                        if x >= BTN_HISTORY.0 && x < BTN_HISTORY.0 + BTN_HISTORY.1 {
                            if ctx.popup != 0 {
                                DestroyWindow(ctx.popup);
                                ctx.popup = 0;
                                ctx.popup_list = 0;
                            } else {
                                show_history_popup(hwnd, ctx);
                            }
                        } else if x >= BTN_NAME.0 && x < BTN_NAME.0 + BTN_NAME.1 {
                            let name = crate::core::generate_name();
                            crate::platform::clipboard::set_text(&name);
                            ctx.input = name.clone();
                            ctx.flash_text = format!("已复制 {}", name);
                            ctx.flash_color = Colors::OK;
                            ctx.flash_until = Some(std::time::Instant::now());
                        } else if x >= BTN_SEND.0 && x < BTN_SEND.0 + BTN_SEND.1 {
                            let text = ctx.input.trim().to_string();
                            if !text.is_empty() {
                                let ok = if let Ok(a) = ctx.app.lock() {
                                    a.send_text(&text)
                                } else {
                                    false
                                };
                                if ok {
                                    ctx.input.clear();
                                    ctx.flash_text = "已发送".to_string();
                                    ctx.flash_color = Colors::OK;
                                } else {
                                    ctx.flash_text = "未连接".to_string();
                                    ctx.flash_color = Colors::ERR;
                                }
                                ctx.flash_until = Some(std::time::Instant::now());
                            }
                        } else if x >= BTN_CLOSE.0 && x < BTN_CLOSE.0 + BTN_CLOSE.1 {
                            ShowWindow(hwnd, 0); // SW_HIDE
                        } else if x >= 8 && x < 88 {
                            // 点击状态区：立即扫描
                            if let Ok(mut a) = ctx.app.lock() {
                                a.start_scan();
                            }
                        }
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
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
