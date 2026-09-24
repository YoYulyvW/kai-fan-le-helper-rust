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
        SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
        GWLP_USERDATA, IDC_ARROW, LB_ADDSTRING, LB_GETCURSEL, LBS_NOTIFY, MSG, SM_CXSCREEN, SW_HIDE,
        SW_SHOW, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT,
        WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_BORDER, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_HSCROLL, WS_POPUP, WS_VISIBLE, WS_VSCROLL,
    };

    /// DPI 缩放因子（百分比，默认 100）
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

    struct Colors;
    impl Colors {
        const BG: u32 = rgb(28, 28, 30);
        const TEXT: u32 = rgb(245, 245, 247);
        const TEXT_SUB: u32 = rgb(142, 142, 147);
        const INPUT_BG: u32 = rgb(54, 54, 58);
        const BTN_BG: u32 = rgb(58, 58, 60);
        const BTN_HOVER: u32 = rgb(78, 78, 82);
        const OK: u32 = rgb(52, 199, 89);
        const OK_HOVER: u32 = rgb(48, 209, 88);
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
    }

    pub fn run(app: App) {
        let app = Arc::new(Mutex::new(app));

        unsafe {
            dpi_aware();
            // 读取系统 DPI 缩放
            let screen_dc = GetDC(0);
            let dpi = GetDeviceCaps(screen_dc, LOGPIXELSX as i32);
            ReleaseDC(0, screen_dc);
            let scale = if dpi > 0 { dpi * 100 / 96 } else { 100 };
            DPI_SCALE.store(scale, Ordering::Relaxed);

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
            let win_w = dp(BAR_W);
            let win_h = dp(BAR_H);
            let x = screen_w - win_w - dp(20);

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
        let win_h = dp(BAR_H);

        // 双缓冲
        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, win_w, win_h);
        let old_bmp = SelectObject(mem_dc, mem_bmp);

        let scale = DPI_SCALE.load(Ordering::Relaxed) as f32 / 100.0;
        if let Some(g) = Graphics::from_hdc(mem_dc as isize) {
            let f = |v: i32| v as f32 * scale;

            // 背景：深色竖向渐变圆角
            g.fill_round_grad(
                0.0, 0.0, f(win_w), f(win_h), f(RADIUS),
                argb(rgb(38, 38, 42)),
                argb(rgb(22, 22, 24)),
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
            g.text(f(STATUS_X), f(4), f(120), f(18), &s1, argb(s1c), f(13), true, false);
            if !s2.is_empty() {
                g.text(f(STATUS_X), f(23), f(120), f(16), &s2, argb(Colors::TEXT_SUB), f(11), false, false);
            }

            // 输入框
            g.fill_round(f(INPUT_X), f(INPUT_Y), f(INPUT_W), f(INPUT_H), f(9), argb(Colors::INPUT_BG));
            let (shown, ic) = if ctx.input.is_empty() {
                ("等待剪贴板...".to_string(), Colors::TEXT_SUB)
            } else {
                (ctx.input.clone(), Colors::TEXT)
            };
            g.text(f(INPUT_X + 10), f(INPUT_Y), f(INPUT_W - 16), f(INPUT_H), &shown, argb(ic), f(13), false, false);

            // 按钮（历史/起名用 emoji 图标，发送/关闭用文字）
            draw_icon_btn(&g, &f, BTN_HISTORY_X, BTN_HISTORY_W, "📋", ctx.hover == Some(Btn::History));
            draw_icon_btn(&g, &f, BTN_NAME_X, BTN_NAME_W, "🎲", ctx.hover == Some(Btn::Name));
            draw_btn_gp(&g, &f, BTN_SEND_X, BTN_SEND_W, "发送", ctx.hover == Some(Btn::Send), true);
            draw_btn_gp(&g, &f, BTN_CLOSE_X, BTN_CLOSE_W, "×", ctx.hover == Some(Btn::Close), false);
        }

        BitBlt(hdc, 0, 0, win_w, win_h, mem_dc, 0, 0, SRCCOPY);

        SelectObject(mem_dc, old_bmp);
        DeleteObject(mem_bmp);
        DeleteDC(mem_dc);
        EndPaint(hwnd, &ps);
    }

    /// 用 GDI+ 画图标按钮（emoji 字体）
    unsafe fn draw_icon_btn<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        x: i32,
        w: i32,
        icon: &str,
        hover: bool,
    ) {
        let (top, bottom) = if hover {
            (rgb(72, 72, 78), rgb(60, 60, 66))
        } else {
            (rgb(56, 56, 60), rgb(46, 46, 50))
        };
        g.fill_round_grad(f(x), f(BTN_Y), f(w), f(BTN_H), f(9), argb(top), argb(bottom));
        g.text_font(
            f(x), f(BTN_Y), f(w), f(BTN_H),
            icon, argb(Colors::TEXT), f(14), false, true, "Segoe UI Emoji",
        );
    }

    /// 用 GDI+ 画按钮
    unsafe fn draw_btn_gp<F: Fn(i32) -> f32>(
        g: &crate::ui::gdiplus::Graphics,
        f: &F,
        x: i32,
        w: i32,
        label: &str,
        hover: bool,
        primary: bool,
    ) {
        let (top, bottom, fg) = if primary {
            let (t, b) = if hover {
                (rgb(60, 210, 100), rgb(40, 180, 75))
            } else {
                (rgb(52, 199, 89), rgb(40, 175, 72))
            };
            (t, b, Colors::WHITE)
        } else if hover {
            (rgb(72, 72, 78), rgb(60, 60, 66), Colors::TEXT)
        } else {
            (rgb(56, 56, 60), rgb(46, 46, 50), Colors::TEXT)
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
            dp(340),
            dp(340),
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
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | WS_HSCROLL | (LBS_NOTIFY as u32),
            4,
            4,
            dp(332),
            dp(332),
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
                        let parent = GetParent(hwnd);
                        DestroyWindow(hwnd);
                        (*ctx).popup = 0;
                        (*ctx).popup_list = 0;
                        if parent != 0 {
                            InvalidateRect(parent, std::ptr::null(), 0);
                        }
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
            m if m == WM_TRAYICON => {
                if lparam as u32 == WM_RBUTTONUP as u32 && !ptr.is_null() {
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
                    handle_tray_cmd(hwnd, ctx, cmd);
                }
                0
            }
            WM_TIMER => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    pump(ctx);
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
                }
                0
            }
            WM_LBUTTONDOWN => {
                if !ptr.is_null() {
                    let ctx = &mut *ptr;
                    let x = (lparam & 0xFFFF) as i16 as i32;
                    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    let hit = hit_test(x, y);
                    crate::utils::log(&format!("ui: click lparam=({},{}) hit={:?}", x, y, hit));
                    match hit {
                        Some(Btn::History) => {
                            if ctx.popup != 0 {
                                DestroyWindow(ctx.popup);
                                ctx.popup = 0;
                                ctx.popup_list = 0;
                            } else {
                                show_history_popup(hwnd, ctx);
                            }
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
        }
        unsafe {
            SetProcessDPIAware();
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
