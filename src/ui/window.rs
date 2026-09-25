//! 原生 Win32 自绘工具条窗口（DPI 感知、圆角、双缓冲、悬停效果）。
//!
//! 复刻 Python 版观感：深色圆角面板、双行状态、圆角输入框、圆角按钮。
//! 布局以逻辑像素定义，运行时按系统 DPI 缩放为物理像素。

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
    use std::sync::atomic::{AtomicI32, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
        CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, GetDC,
        GetDeviceCaps, GetStockObject, InvalidateRect, ReleaseDC, RoundRect, SelectObject,
        SetBkMode, SetTextColor, SetWindowRgn, TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
        DEFAULT_PITCH, FF_DONTCARE, HDC, LOGPIXELSX, NULL_BRUSH, NULL_PEN, OUT_TT_PRECIS,
        PAINTSTRUCT, SRCCOPY, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
        GetParent, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
        LoadCursorW, PeekMessageW, PostQuitMessage, RegisterClassExW, SendMessageW,
        SetCursor, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_DBLCLKS,
        CS_HREDRAW, CS_VREDRAW,
        CW_USEDEFAULT,
        GWLP_USERDATA, IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL, LBS_NOTIFY, MSG, SM_CXSCREEN, SW_HIDE,
        SW_SHOW, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
        WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_BORDER,
        WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_HSCROLL, WS_POPUP, WS_VISIBLE, WS_VSCROLL,
    };

    /// DPI 缩放因子（百分比）
    static DPI_SCALE: AtomicI32 = AtomicI32::new(100);

    /// 逻辑像素 -> 物理像素
    fn dp(v: i32) -> i32 {
        (v * DPI_SCALE.load(Ordering::Relaxed)) / 100
    }

    // ---- 逻辑布局（96dpi 基线）----
    const BAR_W: i32 = 440;
    const BAR_H: i32 = 46;
    const RADIUS: i32 = 12;
    const STATUS_X: i32 = 12;
    const INPUT_X: i32 = 106;
    const INPUT_W: i32 = 160;
    const INPUT_Y: i32 = 9;
    const INPUT_H: i32 = 28;
    const BTN_Y: i32 = 9;
    const BTN_H: i32 = 28;
    const BTN_HISTORY_X: i32 = 272;
    const BTN_HISTORY_W: i32 = 36;
    const BTN_NAME_X: i32 = 312;
    const BTN_NAME_W: i32 = 36;
    const BTN_SEND_X: i32 = 352;
    const BTN_SEND_W: i32 = 48;
    const BTN_CLOSE_X: i32 = 404;
    const BTN_CLOSE_W: i32 = 30;

    const TIMER_ID: usize = 1;
    const ID_POPUP_LIST: i32 = 3001;
    const WM_TRAYICON: u32 = 0x8001;

    const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
    }

    /// 运行时主题（浅色/深色），字段为 COLORREF
    #[derive(Clone, Copy)]
    struct Theme {
        bg_top: u32,
        bg_bottom: u32,
        border: u32,
        text: u32,
        text_sub: u32,
        input_bg: u32,
        btn_top: u32,
        btn_bottom: u32,
        btn_hover_top: u32,
        btn_hover_bottom: u32,
        ok: u32,
        ok_top: u32,
        ok_bottom: u32,
        warn: u32,
        err: u32,
        white: u32,
    }

    impl Theme {
        fn light() -> Self {
            Theme {
                bg_top: rgb(252, 252, 253),
                bg_bottom: rgb(238, 238, 242),
                border: rgb(210, 210, 214),
                text: rgb(28, 28, 30),
                text_sub: rgb(110, 110, 115),
                input_bg: rgb(236, 236, 240),
                btn_top: rgb(244, 244, 247),
                btn_bottom: rgb(228, 228, 233),
                btn_hover_top: rgb(236, 236, 240),
                btn_hover_bottom: rgb(220, 220, 226),
                ok: rgb(40, 167, 69),
                ok_top: rgb(52, 199, 89),
                ok_bottom: rgb(40, 175, 72),
                warn: rgb(255, 149, 0),
                err: rgb(255, 59, 48),
                white: rgb(255, 255, 255),
            }
        }
        fn dark() -> Self {
            Theme {
                bg_top: rgb(38, 38, 42),
                bg_bottom: rgb(22, 22, 24),
                border: rgb(58, 58, 60),
                text: rgb(245, 245, 247),
                text_sub: rgb(142, 142, 147),
                input_bg: rgb(54, 54, 58),
                btn_top: rgb(56, 56, 60),
                btn_bottom: rgb(46, 46, 50),
                btn_hover_top: rgb(72, 72, 78),
                btn_hover_bottom: rgb(60, 60, 66),
                ok: rgb(52, 199, 89),
                ok_top: rgb(60, 210, 100),
                ok_bottom: rgb(40, 180, 75),
                warn: rgb(255, 149, 0),
                err: rgb(255, 59, 48),
                white: rgb(255, 255, 255),
            }
        }
        /// 根据系统设置解析主题
        fn detect() -> Self {
            if crate::platform::detect_system_theme() == "dark" {
                Theme::dark()
            } else {
                Theme::light()
            }
        }
    }

    /// 兼容旧引用的常量（浅色默认值）
    struct Colors;
    impl Colors {
        const OK: u32 = rgb(40, 167, 69);
        const WARN: u32 = rgb(255, 149, 0);
        const ERR: u32 = rgb(255, 59, 48);
        const WHITE: u32 = rgb(255, 255, 255);
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Btn {
        History,
        Name,
        Send,
        Close,
    }

    pub struct Ctx {
        pub app: Arc<Mutex<App>>,
        pub tray: Option<crate::ui::tray::Tray>,
        pub gdiplus: Option<crate::ui::gdiplus::GdiPlus>,
        pub theme: Theme,
        pub input: String,
        pub status1: String,
        pub status1_color: u32,
        pub status2: String,
        pub flash_until: Option<std::time::Instant>,
        pub flash_text: String,
        pub flash_color: u32,
        pub last_clipboard: String,
        pub last_scan: std::time::Instant,
        pub popup: HWND,
        pub popup_list: HWND,
        pub hover: Option<Btn>,
        /// 面板是否展开
        pub panel_open: bool,
        /// 面板模式：0=无 1=历史 2=设置 3=映射选择
        pub panel_mode: u8,
        /// 待选的映射项（mode=3 时）：(标题, 完整文本)
        pub mapping_items: Vec<(String, String)>,
        /// 面板列表项：(标题, 完整文本, 时间)
        pub popup_items: Vec<(String, String, String)>,
        /// 面板悬停项索引
        pub popup_hover: i32,
        /// 面板滚动偏移（项数）
        pub popup_scroll: i32,
    }

    pub fn run(app: App) {
        let app = Arc::new(Mutex::new(app));

        unsafe {
            crate::utils::log("ui: run() entered");
            dpi_aware();
            // 读取系统 DPI 缩放
            let screen_dc = GetDC(0);
            let dpi = GetDeviceCaps(screen_dc, LOGPIXELSX as i32);
            ReleaseDC(0, screen_dc);
            let scale = if dpi > 0 { dpi * 100 / 96 } else { 100 };
            DPI_SCALE.store(scale, Ordering::Relaxed);

            let hinstance = GetModuleHandleW(std::ptr::null());
            // 用进程 ID 生成唯一类名，规避崩溃残留类注册导致的 ERROR_ALREADY_EXISTS
            let cls_name = format!("KaiFanLeHelperWnd_{}", std::process::id());
            let class_name = to_wide(&cls_name);

            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinstance;
            wc.hCursor = LoadCursorW(0, IDC_ARROW);
            wc.lpszClassName = class_name.as_ptr();

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let win_w = dp(BAR_W);
            let win_h = dp(BAR_H);
            let x = screen_w - win_w - dp(20);

            let rc = RegisterClassExW(&wc);
            crate::utils::log(&format!("ui: RegisterClassExW rc={} err={}", rc, std::io::Error::last_os_error()));
            crate::utils::log(&format!("ui: hinstance={} x={} y={} w={} h={} clsptr={:?}", hinstance, x, dp(20), win_w, win_h, class_name.as_ptr()));
            windows_sys::Win32::Foundation::SetLastError(0);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                to_wide("开饭了助手").as_ptr(),
                WS_POPUP,
                x,
                dp(20),
                win_w,
                win_h,
                0,
                0,
                hinstance,
                std::ptr::null(),
            );
            crate::utils::log(&format!("ui: main hwnd={} err={}", hwnd, std::io::Error::last_os_error()));
            if hwnd == 0 {
                return;
            }

            // 圆角窗口区域
            let rgn = CreateRoundRectRgn(0, 0, win_w + 1, win_h + 1, dp(RADIUS) * 2, dp(RADIUS) * 2);
            SetWindowRgn(hwnd, rgn, 1);

            let ctx = Box::new(Ctx {
                app: app.clone(),
                tray: None,
                gdiplus: crate::ui::gdiplus::GdiPlus::new(),
                theme: Theme::detect(),
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
                hover: None,
                panel_open: false,
                panel_mode: 0,
                mapping_items: Vec::new(),
                popup_items: Vec::new(),
                popup_hover: -1,
                popup_scroll: 0,
            });
            let ctx_ptr = Box::into_raw(ctx);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);

            {
                let ctx = &mut *ctx_ptr;
                ctx.tray = Some(crate::ui::tray::Tray::new(hwnd));
                if let Ok(mut a) = ctx.app.lock() {
                    a.hwnd = hwnd as isize;
                }
            }
            {
                let ctx = &mut *ctx_ptr;
                if let Ok(mut a) = ctx.app.lock() {
                    if a.session.settings.auto_scan {
                        a.start_scan();
                    }
                }
            }

            ShowWindow(hwnd, SW_SHOW);
            windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(hwnd, TIMER_ID, 80, None);
            crate::utils::log("ui: entering message loop");

            let mut msg: MSG = std::mem::zeroed();
            loop {
                if PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                    if msg.message == 0x0012 {
                        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
                        if !ptr.is_null() {
                            drop(Box::from_raw(ptr));
                        }
                        return;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    windows_sys::Win32::UI::WindowsAndMessaging::WaitMessage();
                }
            }
        }
    }

    unsafe fn pump(hwnd: HWND, ctx: &mut Ctx) {
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
                UiAction::ShowMappingChooser(_key, items) => {
                    // 加载映射候选（标题用解析出的剧名）
                    ctx.mapping_items.clear();
                    for it in items {
                        let title = match crate::core::title::parse(&it) {
                            Some(p) => p.title,
                            None => it.chars().take(20).collect(),
                        };
                        ctx.mapping_items.push((title, it));
                    }
                    toggle_panel(hwnd, ctx, 3);
                }
            }
        }
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

    unsafe fn fill_round(hdc: HDC, x: i32, y: i32, w: i32, h: i32, radius: i32, color: u32) {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        RoundRect(hdc, x, y, x + w, y + h, radius * 2, radius * 2);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(brush);
        let _ = NULL_BRUSH;
    }

    unsafe fn make_font(size_logical: i32) -> windows_sys::Win32::Graphics::Gdi::HFONT {
        CreateFontW(
            -dp(size_logical),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_TT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            4, // ANTIALIASED_QUALITY（避免透明背景上的彩色描边）
            DEFAULT_PITCH as u32 | FF_DONTCARE as u32,
            to_wide("Microsoft YaHei UI").as_ptr(),
        )
    }

    unsafe fn draw_text_center(hdc: HDC, x: i32, y: i32, w: i32, h: i32, s: &str, color: u32, size: i32) {
        let font = make_font(size);
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        SetBkMode(hdc, TRANSPARENT as i32);
        let wide = to_wide(s);
        let tw = text_width(s, size);
        let tx = x + (w - tw) / 2;
        let ty = y + (h - dp(size)) / 2;
        TextOutW(hdc, tx, ty, wide.as_ptr(), (wide.len() - 1) as i32);
        SelectObject(hdc, old);
        DeleteObject(font);
    }

    unsafe fn draw_text_left(hdc: HDC, x: i32, y: i32, s: &str, color: u32, size: i32) {
        let font = make_font(size);
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        SetBkMode(hdc, TRANSPARENT as i32);
        let wide = to_wide(s);
        TextOutW(hdc, x, y, wide.as_ptr(), (wide.len() - 1) as i32);
        SelectObject(hdc, old);
        DeleteObject(font);
    }

    fn text_width(s: &str, size_logical: i32) -> i32 {
        let px = dp(size_logical);
        let mut w = 0i32;
        for c in s.chars() {
            w += if c.is_ascii() { px / 2 } else { px };
        }
        w
    }

    /// COLORREF(0x00BBGGRR) -> GDI+ ARGB(0xAARRGGBB)
    fn argb(colorref: u32) -> u32 {
        let r = colorref & 0xFF;
        let g = (colorref >> 8) & 0xFF;
        let b = (colorref >> 16) & 0xFF;
        0xFF00_0000 | (r << 16) | (g << 8) | b
    }

    unsafe fn paint(hwnd: HWND, ctx: &Ctx) {
        use crate::ui::gdiplus::Graphics;
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);

        let win_w = dp(BAR_W);
        let win_h = if ctx.panel_open { dp(BAR_H) + dp(PANEL_H) } else { dp(BAR_H) };

        // 双缓冲
        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, win_w, win_h);
        let old_bmp = SelectObject(mem_dc, mem_bmp);

        let scale = DPI_SCALE.load(Ordering::Relaxed) as f32 / 100.0;
        let th = &ctx.theme;
        if let Some(g) = Graphics::from_hdc(mem_dc as isize) {
            let f = |v: i32| v as f32 * scale;

            // 背景：主题渐变圆角
            g.fill_round_grad(
                0.0, 0.0, f(win_w), f(win_h), f(RADIUS),
                argb(th.bg_top), argb(th.bg_bottom),
            );

            // 状态双行
            let (s1, s2, s1c) = if let Some(t) = ctx.flash_until {
                if t.elapsed() < std::time::Duration::from_millis(1500) {
                    (ctx.flash_text.clone(), String::new(), ctx.flash_color)
                } else {
                    (ctx.status1.clone(), ctx.status2.clone(), ctx.status1_color)
                }
            } else {
                (ctx.status1.clone(), ctx.status2.clone(), ctx.status1_color)
            };
            // 单行时垂直居中；双行时才上下排布
            if s2.is_empty() {
                g.text(f(STATUS_X), f(0), f(120), f(BAR_H), &s1, argb(s1c), f(13), true, false);
            } else {
                g.text(f(STATUS_X), f(3), f(120), f(20), &s1, argb(s1c), f(13), true, false);
                g.text(f(STATUS_X), f(25), f(120), f(16), &s2, argb(th.text_sub), f(11), false, false);
            }

            // 输入框
            g.fill_round(f(INPUT_X), f(INPUT_Y), f(INPUT_W), f(INPUT_H), f(9), argb(th.input_bg));
            let (shown, ic) = if ctx.input.is_empty() {
                ("等待剪贴板...".to_string(), th.text_sub)
            } else {
                (ctx.input.clone(), th.text)
            };
            g.text(f(INPUT_X + 10), f(INPUT_Y), f(INPUT_W - 16), f(INPUT_H), &shown, argb(ic), f(13), false, false);

            // 按钮
            draw_icon_btn(&g, &f, th, BTN_HISTORY_X, BTN_HISTORY_W, "📋", ctx.hover == Some(Btn::History));
            draw_icon_btn(&g, &f, th, BTN_NAME_X, BTN_NAME_W, "🎲", ctx.hover == Some(Btn::Name));
            draw_btn_gp(&g, &f, th, BTN_SEND_X, BTN_SEND_W, "发送", ctx.hover == Some(Btn::Send), true);
            draw_btn_gp(&g, &f, th, BTN_CLOSE_X, BTN_CLOSE_W, "×", ctx.hover == Some(Btn::Close), false);

            // 展开的面板
            if ctx.panel_open {
                match ctx.panel_mode {
                    1 => draw_history_panel(&g, &f, th, ctx, "📋 历史记录（双击填入）"),
                    3 => draw_history_panel(&g, &f, th, ctx, "📌 选择要粘贴的内容（双击）"),
                    2 => draw_settings_panel(&g, &f, th, ctx),
                    _ => {}
                }
            }
        }

        BitBlt(hdc, 0, 0, win_w, win_h, mem_dc, 0, 0, SRCCOPY);

        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp);
        DeleteDC(mem_dc);
        EndPaint(hwnd, &ps);
    }

    /// 绘制展开的历史面板
    unsafe fn draw_history_panel<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        th: &Theme,
        ctx: &Ctx,
        title: &str,
    ) {
        let panel_top = BAR_H;
        let pw = BAR_W;

        // 分隔线
        g.fill_round(f(12), f(panel_top), f(pw - 24), f(1), 0.0, argb(th.border));

        // 标题
        g.text(
            f(16), f(panel_top + 6), f(pw - 32), f(PANEL_HEADER - 6),
            title, argb(th.text), f(13), true, false,
        );

        let list_top = panel_top + PANEL_HEADER;
        let visible_rows = ((PANEL_H - PANEL_HEADER - 8) / PANEL_ROW_H).max(1);
        let start = ctx.popup_scroll.max(0) as usize;
        let end = (start + visible_rows as usize).min(ctx.popup_items.len());

        if ctx.popup_items.is_empty() {
            g.text(
                f(16), f(list_top + 20), f(pw - 32), f(40),
                "(暂无历史记录)", argb(th.text_sub), f(13), false, false,
            );
            return;
        }

        for (i, idx) in (start..end).enumerate() {
            let item = &ctx.popup_items[idx];
            let row_y = list_top + 4 + (i as i32) * PANEL_ROW_H;
            let hovered = ctx.popup_hover == idx as i32;

            // 悬停高亮
            if hovered {
                g.fill_round(
                    f(8), f(row_y), f(pw - 16), f(PANEL_ROW_H - 4),
                    f(8), argb(rgb(99, 102, 241)),
                );
            }

            // 标题
            let title_color = if hovered { th.white } else { th.text };
            g.text(
                f(18), f(row_y + 4), f(pw - 40), f(20),
                &truncate(&item.0, 26), argb(title_color), f(13), hovered, false,
            );
            // 时间
            let time_color = if hovered { th.white } else { th.text_sub };
            if !item.2.is_empty() {
                g.text(
                    f(18), f(row_y + 24), f(pw - 40), f(16),
                    &item.2, argb(time_color), f(10), false, false,
                );
            }
        }
    }

    // 设置面板行数（含 4 个开关 + 重扫 + 退出）
    const SETTINGS_ROWS: usize = 6;

    /// 绘制设置面板（开关列表）
    unsafe fn draw_settings_panel<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        th: &Theme,
        ctx: &Ctx,
    ) {
        let panel_top = BAR_H;
        let pw = BAR_W;

        // 读取当前设置
        let (auto_scan, auto_push, clear_clip, autostart) = if let Ok(a) = ctx.app.lock() {
            (
                a.session.settings.auto_scan,
                a.session.settings.auto_push,
                a.session.settings.clear_clipboard,
                a.session.settings.autostart,
            )
        } else {
            (true, false, true, true)
        };

        // 分隔线
        g.fill_round(f(12), f(panel_top), f(pw - 24), f(1), 0.0, argb(th.border));
        g.text(
            f(16), f(panel_top + 6), f(pw - 32), f(PANEL_HEADER - 6),
            "⚙ 设置", argb(th.text), f(13), true, false,
        );

        let list_top = panel_top + PANEL_HEADER;
        let rows: [(&str, bool); SETTINGS_ROWS] = [
            ("自动扫描", auto_scan),
            ("识别后自动推送", auto_push),
            ("粘贴后清空剪贴板", clear_clip),
            ("开机自启动", autostart),
            ("重新扫描", false),
            ("退出", false),
        ];

        for (i, (label, on)) in rows.iter().enumerate() {
            let row_y = list_top + 4 + (i as i32) * PANEL_ROW_H;
            let hovered = ctx.popup_hover == i as i32;

            if hovered {
                g.fill_round(
                    f(8), f(row_y), f(pw - 16), f(PANEL_ROW_H - 4),
                    f(8), argb(rgb(99, 102, 241)),
                );
            }

            let label_color = if hovered { th.white } else { th.text };
            g.text(
                f(18), f(row_y + 8), f(pw - 80), f(24),
                label, argb(label_color), f(13), false, false,
            );

            // 开关状态（仅前 4 项）
            if i < 4 {
                let (state_txt, state_color) = if *on {
                    ("开", if hovered { th.white } else { th.ok })
                } else {
                    ("关", if hovered { th.white } else { th.text_sub })
                };
                g.text(
                    f(pw - 60), f(row_y + 8), f(44), f(24),
                    state_txt, argb(state_color), f(13), true, false,
                );
            }
        }
    }

    /// 处理设置面板点击
    unsafe fn handle_settings_click(hwnd: HWND, ctx: &mut Ctx, idx: i32) {
        match idx {
            0 => toggle_bool(ctx, |s| s.auto_scan = !s.auto_scan),
            1 => toggle_bool(ctx, |s| s.auto_push = !s.auto_push),
            2 => toggle_bool(ctx, |s| s.clear_clipboard = !s.clear_clipboard),
            3 => {
                if let Ok(mut a) = ctx.app.lock() {
                    a.session.settings.autostart = !a.session.settings.autostart;
                    a.session.settings.save();
                    let v = a.session.settings.autostart;
                    let _ = crate::platform::autostart::set_autostart(v);
                }
            }
            4 => {
                if let Ok(mut a) = ctx.app.lock() {
                    a.start_scan();
                }
            }
            5 => {
                PostQuitMessage(0);
            }
            _ => {}
        }
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }

    /// 切换设置项并持久化
    unsafe fn toggle_bool<F: FnOnce(&mut crate::config::Settings)>(ctx: &Ctx, f: F) {
        if let Ok(mut a) = ctx.app.lock() {
            f(&mut a.session.settings);
            a.session.settings.save();
        }
    }

    /// 用 GDI+ 画图标按钮（emoji 字体）
    unsafe fn draw_icon_btn<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        th: &Theme,
        x: i32,
        w: i32,
        icon: &str,
        hover: bool,
    ) {
        let (top, bottom) = if hover {
            (th.btn_hover_top, th.btn_hover_bottom)
        } else {
            (th.btn_top, th.btn_bottom)
        };
        g.fill_round_grad(f(x), f(BTN_Y), f(w), f(BTN_H), f(9), argb(top), argb(bottom));
        g.text_font(
            f(x), f(BTN_Y), f(w), f(BTN_H),
            icon, argb(th.text), f(14), false, true, "Segoe UI Emoji",
        );
    }

    /// 用 GDI+ 画按钮
    unsafe fn draw_btn_gp<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        th: &Theme,
        x: i32,
        w: i32,
        label: &str,
        hover: bool,
        primary: bool,
    ) {
        let (top, bottom, fg) = if primary {
            let (t, b) = if hover {
                (th.ok_top, th.ok_bottom)
            } else {
                (th.ok_top, th.ok_bottom)
            };
            (t, b, th.white)
        } else if hover {
            (th.btn_hover_top, th.btn_hover_bottom, th.text)
        } else {
            (th.btn_top, th.btn_bottom, th.text)
        };
        g.fill_round_grad(
            f(x), f(BTN_Y), f(w), f(BTN_H), f(9),
            argb(top), argb(bottom),
        );
        g.text(
            f(x), f(BTN_Y), f(w), f(BTN_H),
            label, argb(fg), f(13), primary, true,
        );
    }

    fn hit_test(x: i32, y: i32) -> Option<Btn> {
        if y < dp(BTN_Y) || y > dp(BTN_Y + BTN_H) {
            return None;
        }
        if x >= dp(BTN_HISTORY_X) && x < dp(BTN_HISTORY_X + BTN_HISTORY_W) {
            Some(Btn::History)
        } else if x >= dp(BTN_NAME_X) && x < dp(BTN_NAME_X + BTN_NAME_W) {
            Some(Btn::Name)
        } else if x >= dp(BTN_SEND_X) && x < dp(BTN_SEND_X + BTN_SEND_W) {
            Some(Btn::Send)
        } else if x >= dp(BTN_CLOSE_X) && x < dp(BTN_CLOSE_X + BTN_CLOSE_W) {
            Some(Btn::Close)
        } else {
            None
        }
    }

    // 弹窗尺寸（逻辑像素）
    const POPUP_W: i32 = 360;
    const POPUP_H: i32 = 400;
    const POPUP_HEADER: i32 = 34;
    const POPUP_ROW_H: i32 = 44;
    const POPUP_PAD: i32 = 8;

    /// 弹出历史记录窗口（GDI+ 自绘，现代圆角风格）
    unsafe fn show_history_popup(parent: HWND, ctx: &mut Ctx) {
        let hinstance = GetModuleHandleW(std::ptr::null());
        // 类名持久化为 'static，避免临时 Vec 释放后系统引用悬垂指针
        let cls = popup_class_name();

        // 仅注册一次
        static REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !REGISTERED.load(Ordering::Relaxed) {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
            wc.lpfnWndProc = Some(popup_proc);
            wc.hInstance = hinstance;
            wc.hCursor = LoadCursorW(0, IDC_ARROW);
            wc.lpszClassName = cls;
            let rc = RegisterClassExW(&wc);
            crate::utils::log(&format!("popup: RegisterClassExW atom={} err={}", rc, std::io::Error::last_os_error()));
            REGISTERED.store(true, Ordering::Relaxed);
        }

        // 组装列表数据
        ctx.popup_items.clear();
        if let Ok(a) = ctx.app.lock() {
            for it in &a.session.history {
                ctx.popup_items
                    .push((it.title.clone(), it.text.clone(), it.time.clone()));
            }
        }
        ctx.popup_hover = -1;
        ctx.popup_scroll = 0;

        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(parent, &mut rect);

        let pw = dp(POPUP_W);
        let ph = dp(POPUP_H);
        let mut x = rect.left;
        // 防止超出右边缘
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        if x + pw > screen_w {
            x = screen_w - pw - dp(8);
        }

        // 清除上次错误，确保拿到真实的 CreateWindow 错误
        windows_sys::Win32::Foundation::SetLastError(0);
        let popup = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls,
            to_wide("历史记录").as_ptr(),
            WS_POPUP,
            x,
            rect.bottom + dp(4),
            pw,
            ph,
            0, // 不设 owner，避免 owner 引发的创建失败
            0,
            hinstance,
            std::ptr::null(),
        );
        crate::utils::log(&format!("popup: CreateWindowExW hwnd={} size={}x{} err={}", popup, pw, ph, std::io::Error::last_os_error()));
        let _ = parent;
        if popup == 0 {
            return;
        }

        // 圆角区域
        let rgn = CreateRoundRectRgn(0, 0, pw + 1, ph + 1, dp(12) * 2, dp(12) * 2);
        SetWindowRgn(popup, rgn, 1);

        SetWindowLongPtrW(popup, GWLP_USERDATA, ctx as *mut Ctx as isize);
        ShowWindow(popup, SW_SHOW);
        ctx.popup = popup;
        ctx.popup_list = 0;
    }

    /// 绘制历史弹窗
    unsafe fn paint_popup(hwnd: HWND, ctx: &Ctx) {
        use crate::ui::gdiplus::Graphics;
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);

        let pw = dp(POPUP_W);
        let ph = dp(POPUP_H);

        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, pw, ph);
        let old_bmp = SelectObject(mem_dc, mem_bmp);

        let scale = DPI_SCALE.load(Ordering::Relaxed) as f32 / 100.0;
        let th = &ctx.theme;
        if let Some(g) = Graphics::from_hdc(mem_dc as isize) {
            let f = |v: i32| v as f32 * scale;

            // 背景
            g.fill_round_grad(
                0.0, 0.0, f(pw), f(ph), f(12),
                argb(th.bg_top), argb(th.bg_bottom),
            );

            // 标题栏
            g.text(
                f(16), f(6), f(pw - 32), f(POPUP_HEADER),
                "📋 历史记录（双击填入）", argb(th.text), f(14), true, false,
            );

            // 列表
            let list_top = POPUP_HEADER;
            let visible_rows = ((POPUP_H - list_top - POPUP_PAD) / POPUP_ROW_H).max(1);
            let start = ctx.popup_scroll.max(0) as usize;
            let end = (start + visible_rows as usize).min(ctx.popup_items.len());

            if ctx.popup_items.is_empty() {
                g.text(
                    f(16), f(list_top + 20), f(pw - 32), f(40),
                    "(暂无历史记录)", argb(th.text_sub), f(13), false, false,
                );
            }

            for (i, idx) in (start..end).enumerate() {
                let item = &ctx.popup_items[idx];
                let row_y = list_top + POPUP_PAD + (i as i32) * POPUP_ROW_H;
                let hovered = ctx.popup_hover == idx as i32;

                // 行背景（悬停高亮）
                if hovered {
                    g.fill_round(
                        f(POPUP_PAD), f(row_y), f(POPUP_W - POPUP_PAD * 2), f(POPUP_ROW_H - 4),
                        f(8), argb(rgb(99, 102, 241) & 0x00FFFFFF | 0x33000000),
                    );
                }

                // 标题
                g.text(
                    f(POPUP_PAD + 10), f(row_y + 3), f(POPUP_W - POPUP_PAD * 2 - 20), f(20),
                    &truncate(&item.0, 24), argb(th.text), f(13), hovered, false,
                );
                // 时间
                if !item.2.is_empty() {
                    g.text(
                        f(POPUP_PAD + 10), f(row_y + 22), f(POPUP_W - POPUP_PAD * 2 - 20), f(16),
                        &item.2, argb(th.text_sub), f(10), false, false,
                    );
                }
            }
        }

        BitBlt(hdc, 0, 0, pw, ph, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp);
        DeleteDC(mem_dc);
        EndPaint(hwnd, &ps);
    }

    // 历史面板尺寸（逻辑像素）
    const PANEL_H: i32 = 360;
    const PANEL_ROW_H: i32 = 46;
    const PANEL_HEADER: i32 = 30;

    /// 展开/收起/切换面板。mode: 1=历史 2=设置
    unsafe fn toggle_panel(hwnd: HWND, ctx: &mut Ctx, mode: u8) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER};

        let win_w = dp(BAR_W);
        // 同模式再点 -> 收起；不同模式 -> 切换内容
        if ctx.panel_open && ctx.panel_mode == mode {
            ctx.panel_open = false;
            ctx.panel_mode = 0;
            let bar_h = dp(BAR_H);
            SetWindowPos(hwnd, 0, 0, 0, win_w, bar_h, SWP_NOZORDER | 0x0002);
            let rgn = CreateRoundRectRgn(0, 0, win_w + 1, bar_h + 1, dp(RADIUS) * 2, dp(RADIUS) * 2);
            SetWindowRgn(hwnd, rgn, 1);
        } else {
            if mode == 1 {
                // 加载历史
                ctx.popup_items.clear();
                if let Ok(a) = ctx.app.lock() {
                    for it in &a.session.history {
                        // 双击后填入解析过的展示文本（剧名，极速加后缀）
                        let display = a.session.display_for_history(&it.text);
                        ctx.popup_items
                            .push((it.title.clone(), display, it.time.clone()));
                    }
                }
            }
            if mode == 3 {
                // 映射选择：把 mapping_items 转成 popup_items 供列表绘制
                ctx.popup_items.clear();
                for (title, full) in &ctx.mapping_items {
                    ctx.popup_items
                        .push((title.clone(), full.clone(), String::new()));
                }
            }
            ctx.popup_hover = -1;
            ctx.popup_scroll = 0;
            ctx.panel_open = true;
            ctx.panel_mode = mode;

            let total_h = dp(BAR_H) + dp(PANEL_H);
            SetWindowPos(hwnd, 0, 0, 0, win_w, total_h, SWP_NOZORDER | 0x0002);
            let rgn = CreateRoundRectRgn(0, 0, win_w + 1, total_h + 1, dp(RADIUS) * 2, dp(RADIUS) * 2);
            SetWindowRgn(hwnd, rgn, 1);
        }
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }

    /// 面板命中测试：返回项索引
    fn panel_hit(y: i32, scroll: i32) -> i32 {
        let list_top = dp(BAR_H + PANEL_HEADER);
        let rel = y - list_top - dp(4);
        if rel < 0 {
            return -1;
        }
        let row = rel / dp(PANEL_ROW_H);
        scroll + row
    }

    /// 弹窗类名（'static 持久化）
    fn popup_class_name() -> *const u16 {
        use std::sync::OnceLock;
        static NAME: OnceLock<Vec<u16>> = OnceLock::new();
        NAME.get_or_init(|| {
            "KaiFanLePopup ".encode_utf16().collect()
        })
        .as_ptr()
    }

    /// 截断过长标题
    fn truncate(s: &str, max: usize) -> String {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= max {
            s.to_string()
        } else {
            let mut t: String = chars[..max].iter().collect();
            t.push('…');
            t
        }
    }

    /// 命中测试：返回弹窗内的列表项索引
    fn popup_hit(y: i32, scroll: i32) -> i32 {
        let list_top = dp(POPUP_HEADER);
        let rel = y - list_top - dp(POPUP_PAD);
        if rel < 0 {
            return -1;
        }
        let row = rel / dp(POPUP_ROW_H);
        scroll + row
    }

    unsafe extern "system" fn popup_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
        match msg {
            WM_PAINT => {
                if !ctx.is_null() {
                    paint_popup(hwnd, &*ctx);
                }
                0
            }
            WM_MOUSEMOVE => {
                if !ctx.is_null() {
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    let idx = popup_hit(y, (*ctx).popup_scroll);
                    let valid = idx >= 0 && (idx as usize) < (*ctx).popup_items.len();
                    let new_hover = if valid { idx } else { -1 };
                    if new_hover != (*ctx).popup_hover {
                        (*ctx).popup_hover = new_hover;
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                }
                0
            }
            WM_LBUTTONDBLCLK => {
                if !ctx.is_null() {
                    let idx = (*ctx).popup_hover;
                    if idx >= 0 && (idx as usize) < (*ctx).popup_items.len() {
                        (*ctx).input = (*ctx).popup_items[idx as usize].1.clone();
                    }
                    let parent = GetParent(hwnd);
                    DestroyWindow(hwnd);
                    (*ctx).popup = 0;
                    if parent != 0 {
                        InvalidateRect(parent, std::ptr::null(), 0);
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                // 单击也选中（更友好）
                0
            }
            WM_MOUSEWHEEL => {
                if !ctx.is_null() {
                    let delta = ((wparam >> 16) & 0xFFFF) as i16 as i32;
                    if delta > 0 {
                        (*ctx).popup_scroll = ((*ctx).popup_scroll - 1).max(0);
                    } else {
                        let max = ((*ctx).popup_items.len() as i32 - 5).max(0);
                        (*ctx).popup_scroll = ((*ctx).popup_scroll + 1).min(max);
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
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
            // WM_NCCREATE：必须返回 TRUE(1) 才能继续创建窗口
            0x0081 => 1,
            // WM_SETCURSOR：始终用箭头光标，避免出现"忙碌转圈"
            0x0020 => {
                SetCursor(LoadCursorW(0, IDC_ARROW));
                1
            }
            // WM_NCHITTEST：整个窗口都算客户区（保证鼠标事件可达）
            0x0084 => 1, // HTCLIENT
            WM_CREATE => 0,
            m if m == WM_TRAYICON => {
                if !ptr.is_null() {
                    // 托盘消息：鼠标事件在 lparam 低字，图标 ID 在高字
                    let ev = (lparam & 0xFFFF) as u32;
                    if ev == WM_RBUTTONUP as u32 || ev == 0x007B {
                        // 右键：在光标处弹出托盘菜单
                        let ctx = &mut *ptr;
                        let (auto_scan, auto_push, clear_clip, autostart, hotkey) =
                            if let Ok(a) = ctx.app.lock() {
                                (
                                    a.session.settings.auto_scan,
                                    a.session.settings.auto_push,
                                    a.session.settings.clear_clipboard,
                                    a.session.settings.autostart,
                                    a.session.settings.hotkey.clone(),
                                )
                            } else {
                                (true, false, true, true, "F1".to_string())
                            };
                        let cmd = ctx
                            .tray
                            .as_ref()
                            .map(|t| {
                                t.show_menu(
                                    auto_scan,
                                    auto_push,
                                    clear_clip,
                                    autostart,
                                    &hotkey,
                                    "auto",
                                )
                            })
                            .unwrap_or(0);
                        handle_tray_cmd(hwnd, ctx, cmd);
                    } else if ev == 0x0202 {
                        // 左键单击：切换显示 / 隐藏
                        if IsWindowVisible(hwnd) != 0 {
                            ShowWindow(hwnd, SW_HIDE);
                        } else {
                            ShowWindow(hwnd, SW_SHOW);
                        }
                    }
                }
                0
            }
            WM_TIMER => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    pump(hwnd, ctx);
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
            WM_MOUSEMOVE => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    let x = (lparam & 0xFFFF) as i16 as i32;
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    let h = hit_test(x, y);
                    if h != ctx.hover {
                        ctx.hover = h;
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                    // 面板悬停
                    if ctx.panel_open && y > dp(BAR_H) {
                        let idx = panel_hit(y, ctx.popup_scroll);
                        let valid = idx >= 0 && (idx as usize) < ctx.popup_items.len();
                        let new_hover = if valid { idx } else { -1 };
                        if new_hover != ctx.popup_hover {
                            ctx.popup_hover = new_hover;
                            InvalidateRect(hwnd, std::ptr::null(), 0);
                        }
                    }
                }
                0
            }
            WM_LBUTTONDBLCLK => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    if ctx.panel_open && y > dp(BAR_H) {
                        let mode = ctx.panel_mode;
                        let idx = panel_hit(y, ctx.popup_scroll);
                        if idx >= 0 && (idx as usize) < ctx.popup_items.len() {
                            let text = ctx.popup_items[idx as usize].1.clone();
                            if mode == 3 {
                                // 映射选择：复制并粘贴
                                crate::platform::clipboard::set_text(&text);
                                if let Ok(mut a) = ctx.app.lock() {
                                    a.apply_mapping_text(&text);
                                }
                            } else {
                                ctx.input = text;
                            }
                        }
                        toggle_panel(hwnd, ctx, mode);
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    let x = (lparam & 0xFFFF) as i16 as i32;
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    if ctx.panel_open && y > dp(BAR_H) {
                        if ctx.panel_mode == 2 {
                            handle_settings_click(hwnd, ctx, panel_hit(y, 0));
                        }
                        return 0;
                    }
                    let hit = hit_test(x, y);
                    match hit {
                        Some(Btn::History) => {
                            toggle_panel(hwnd, ctx, 1);
                        }
                        Some(Btn::Name) => {
                            let name = crate::core::generate_name();
                            crate::platform::clipboard::set_text(&name);
                            ctx.input = name.clone();
                            ctx.flash_text = format!("已复制 {}", name);
                            ctx.flash_color = Colors::OK;
                            ctx.flash_until = Some(std::time::Instant::now());
                        }
                        Some(Btn::Send) => {
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
                        }
                        Some(Btn::Close) => {
                            ShowWindow(hwnd, SW_HIDE);
                        }
                        None => {
                            if x < dp(INPUT_X) {
                                if let Ok(mut a) = ctx.app.lock() {
                                    a.start_scan();
                                }
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

    unsafe fn handle_tray_cmd(hwnd: HWND, ctx: &mut Ctx, cmd: usize) {
        use crate::ui::tray::*;
        match cmd {
            ID_SHOW => {
                if IsWindowVisible(hwnd) != 0 {
                    ShowWindow(hwnd, SW_HIDE);
                } else {
                    ShowWindow(hwnd, SW_SHOW);
                }
            }
            ID_RESCAN => {
                if let Ok(mut a) = ctx.app.lock() {
                    a.start_scan();
                }
            }
            ID_AUTO_SCAN => set_bool(ctx, |s| s.auto_scan = !s.auto_scan),
            ID_AUTO_PUSH => set_bool(ctx, |s| s.auto_push = !s.auto_push),
            ID_CLEAR_CLIP => set_bool(ctx, |s| s.clear_clipboard = !s.clear_clipboard),
            ID_AUTOSTART => {
                if let Ok(mut a) = ctx.app.lock() {
                    a.session.settings.autostart = !a.session.settings.autostart;
                    a.session.settings.save();
                    let v = a.session.settings.autostart;
                    let _ = crate::platform::autostart::set_autostart(v);
                }
            }
            ID_QUIT => PostQuitMessage(0),
            c if (ID_HOTKEY_BASE..ID_HOTKEY_BASE + 13).contains(&c) => {
                let n = c - ID_HOTKEY_BASE;
                let key = format!("F{}", n);
                if let Ok(mut a) = ctx.app.lock() {
                    a.session.settings.hotkey = key;
                    a.session.settings.save();
                    a.reload_hotkeys();
                }
            }
            _ => {}
        }
    }

    unsafe fn set_bool<F: FnOnce(&mut crate::config::Settings)>(ctx: &Ctx, f: F) {
        if let Ok(mut a) = ctx.app.lock() {
            f(&mut a.session.settings);
            a.session.settings.save();
        }
    }

    fn dpi_aware() {
        #[link(name = "user32")]
        extern "system" {
            fn SetProcessDPIAware() -> i32;
            fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
        }
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
        // 优先用 V2（Win10 1703+，不虚拟化坐标），失败回退 SetProcessDPIAware
        unsafe {
            let ok = SetProcessDpiAwarenessContext(-4);
            if ok == 0 {
                SetProcessDPIAware();
            }
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
