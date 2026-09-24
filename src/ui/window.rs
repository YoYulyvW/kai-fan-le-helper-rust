//! egui 主窗口：现代无边框置顶工具条。
//!
//! 设计语言（沿用原版配色）：
//! - 深色背景 #1c1c1e，圆角 12px
//! - 强调色 #6366F1，发送绿 #34C759
//! - 无边框、始终置顶、紧凑工具条

use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::{Color32, RichText, Rounding, Stroke, Vec2};

use crate::app::{App, UiAction};
use crate::config::{WIN_HEIGHT, WIN_WIDTH};

/// 加载系统中文字体（微软雅黑），并作为首选比例字体
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 微软雅黑（含中文）
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\msyh.ttc") {
        fonts.font_data.insert(
            "msyh".to_owned(),
            egui::FontData::from_owned(data),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "msyh".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("msyh".to_owned());
    }

    // Segoe UI Emoji（含 emoji）
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\seguiemj.ttf") {
        fonts.font_data.insert(
            "emoji".to_owned(),
            egui::FontData::from_owned(data),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("emoji".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// 深色调色板
#[derive(Clone)]
struct Palette {
    bg: Color32,
    border: Color32,
    text: Color32,
    text_sub: Color32,
    input_bg: Color32,
    accent: Color32,
    ok: Color32,
    warn: Color32,
    err: Color32,
}

impl Palette {
    fn clone_for_use(&self) -> Palette {
        self.clone()
    }

    fn dark() -> Self {
        Palette {
            bg: Color32::from_rgb(28, 28, 30),
            border: Color32::from_rgba_unmultiplied(255, 255, 255, 36),
            text: Color32::from_rgb(255, 255, 255),
            text_sub: Color32::from_rgb(142, 142, 147),
            input_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            accent: Color32::from_rgb(99, 102, 241),
            ok: Color32::from_rgb(52, 199, 89),
            warn: Color32::from_rgb(255, 149, 0),
            err: Color32::from_rgb(255, 59, 48),
        }
    }
}

/// 工具条状态
struct ToolbarState {
    input: String,
    status: String,
    status_color: Color32,
    toast: Option<(String, std::time::Instant)>,
}

impl Default for ToolbarState {
    fn default() -> Self {
        ToolbarState {
            input: String::new(),
            status: "● 扫描中".to_string(),
            status_color: Color32::from_rgb(255, 149, 0),
            toast: None,
        }
    }
}

/// 应用 UI
struct HelperApp {
    app: Arc<Mutex<App>>,
    state: ToolbarState,
    palette: Palette,
    frame_count: u32,
}

impl HelperApp {
    fn new(app: App) -> Self {
        HelperApp {
            app: Arc::new(Mutex::new(app)),
            state: ToolbarState::default(),
            palette: Palette::dark(),
            frame_count: 0,
        }
    }

    /// 后台事件泵
    fn pump_events(&mut self) {
        let mut actions = Vec::new();
        if let Ok(mut a) = self.app.lock() {
            while let Some(ev) = a.try_event() {
                actions.push(a.handle(ev));
            }
        }
        for action in actions {
            match action {
                UiAction::None => {}
                UiAction::Flash(text, _color) => self.flash(&text),
                UiAction::SetInput(text) => {
                    self.state.input = text;
                }
                UiAction::ShowMappingChooser(_key, _items) => {
                    self.flash("多条内容，请选择");
                }
            }
        }
    }

    fn flash(&mut self, text: &str) {
        self.state.status = format!("● {}", text);
        self.state.status_color = self.palette.ok;
        self.state.toast = Some((text.to_string(), std::time::Instant::now()));
    }
}

impl eframe::App for HelperApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 透明背景，配合圆角面板
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_events();

        // toast 1.5s 后恢复
        if let Some((_, t)) = &self.state.toast {
            if t.elapsed() > std::time::Duration::from_millis(1500) {
                self.state.toast = None;
                self.update_status_text();
            }
        }

        // 延迟若干帧后，用 egui 官方命令定位到屏幕右上角
        self.frame_count += 1;
        if self.frame_count == 20 {
            // 使用 egui 视角的显示器逻辑尺寸，避免 DPI 缩放导致越界
            let monitor = ctx.input(|i| i.viewport().monitor_size);
            let (sw, sh) = match monitor {
                Some(s) => (s.x, s.y),
                None => screen_size(),
            };
            let x = (sw - WIN_WIDTH as f32 - 20.0).max(0.0);
            let y = 20.0_f32.min(sh - WIN_HEIGHT as f32);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }

        // 请求持续重绘以响应后台事件
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        let p = self.palette.clone_for_use();

        // 圆角面板
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(p.bg)
                    .rounding(Rounding::same(12.0))
                    .stroke(Stroke::new(1.0, p.border))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // 状态圆点 + 文本
                    let (dot, label) = split_status(&self.state.status);
                    ui.label(RichText::new(dot).color(self.state.status_color).size(10.0));
                    ui.label(
                        RichText::new(label)
                            .color(self.state.status_color)
                            .size(11.0),
                    );

                    ui.add_space(6.0);

                    // 输入框（圆角）
                    let input = egui::TextEdit::singleline(&mut self.state.input)
                        .hint_text("等待剪贴板...")
                        .desired_width(148.0)
                        .text_color(p.text)
                        .vertical_align(egui::Align::Center)
                        .frame(true);
                    let resp = ui.add_sized([148.0, 26.0], input);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.do_send();
                    }

                    ui.add_space(6.0);

                    // 历史
                    if ghost_button(ui, "历史", &p).clicked() {
                        self.flash("历史");
                    }
                    // 生成名字
                    if ghost_button(ui, "起名", &p).clicked() {
                        let name = crate::core::generate_name();
                        crate::platform::clipboard::set_text(&name);
                        self.state.input = name.clone();
                        self.flash(&format!("已复制 {}", name));
                    }
                    // 发送
                    if solid_button(ui, "发送", p.ok).clicked() {
                        self.do_send();
                    }
                    // 关闭
                    if ghost_button(ui, "✕", &p).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}

impl HelperApp {
    fn update_status_text(&mut self) {
        if let Ok(a) = self.app.lock() {
            let n = a.session.devices.len();
            if a.session.is_discovering() {
                self.state.status = "● 扫描中".to_string();
                self.state.status_color = self.palette.warn;
            } else if n == 0 {
                self.state.status = "● 未找到".to_string();
                self.state.status_color = self.palette.err;
            } else {
                self.state.status = if n == 1 {
                    "● 已连接".to_string()
                } else {
                    format!("● 已连接 ({})", n)
                };
                self.state.status_color = self.palette.ok;
            }
        }
    }

    fn do_send(&mut self) {
        let text = self.state.input.trim().to_string();
        if text.is_empty() {
            self.flash("无内容");
            return;
        }
        let ok = if let Ok(a) = self.app.lock() {
            a.send_text(&text)
        } else {
            false
        };
        if ok {
            self.state.input.clear();
            self.flash("已发送");
        } else {
            self.flash("未连接");
        }
    }
}

/// 拆分 "● 文本" 为圆点与文本
fn split_status(s: &str) -> (&str, &str) {
    match s.split_once(' ') {
        Some((dot, rest)) => (dot, rest),
        None => ("●", s),
    }
}

/// 实心按钮（用于主操作，如发送）
fn solid_button(ui: &mut egui::Ui, label: &str, bg: Color32) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).size(12.0).strong())
        .fill(bg)
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(44.0, 26.0));
    ui.add(btn)
}

/// 幽灵按钮（半透明背景，用于次要操作）
fn ghost_button(ui: &mut egui::Ui, label: &str, p: &Palette) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(p.text).size(12.0))
        .fill(p.input_bg)
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(if label.chars().count() > 1 { 40.0 } else { 26.0 }, 26.0));
    ui.add(btn)
}

/// 获取主屏尺寸（像素）
fn screen_size() -> (f32, f32) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };
        let w = GetSystemMetrics(SM_CXSCREEN) as f32;
        let h = GetSystemMetrics(SM_CYSCREEN) as f32;
        if w > 0.0 && h > 0.0 {
            return (w, h);
        }
    }
    (1920.0, 1080.0)
}

/// 运行 UI 主循环（阻塞直到退出）
pub fn run(app: App) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([WIN_WIDTH as f32, WIN_HEIGHT as f32])
            .with_decorations(false)
            .with_always_on_top()
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false),
        ..Default::default()
    };

    eframe::run_native(
        "开饭了助手",
        native_options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Box::new(HelperApp::new(app))
        }),
    )
}
