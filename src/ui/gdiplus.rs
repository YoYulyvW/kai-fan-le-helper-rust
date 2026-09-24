//! GDI+ 高质量 2D 渲染封装：抗锯齿圆角、平滑文字、渐变。
//!
//! 相比 GDI 的 RoundRect/TextOut，GDI+ 提供真正的抗锯齿与灰度文字，观感接近 Qt。
//! 所有 GDI+ 调用集中在本模块。

#![allow(non_snake_case, dead_code)]

use std::ffi::c_void;
use std::ptr;

type Status = i32;
type GpGraphics = *mut c_void;
type GpBrush = *mut c_void;
type GpPath = *mut c_void;
type GpFontFamily = *mut c_void;
type GpFont = *mut c_void;
type GpStringFormat = *mut c_void;

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *mut c_void,
    suppress_bg: i32,
    suppress_codecs: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PointF {
    x: f32,
    y: f32,
}

#[repr(C)]
struct RectF {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> Status;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateFromHDC(hdc: isize, g: *mut GpGraphics) -> Status;
    fn GdipDeleteGraphics(g: GpGraphics) -> Status;
    fn GdipSetSmoothingMode(g: GpGraphics, mode: i32) -> Status;
    fn GdipSetTextRenderingHint(g: GpGraphics, hint: i32) -> Status;
    fn GdipCreateSolidFill(argb: u32, b: *mut GpBrush) -> Status;
    fn GdipDeleteBrush(b: GpBrush) -> Status;
    fn GdipCreateLineBrush(p1: *const PointF, p2: *const PointF, c1: u32, c2: u32, wrap: i32, b: *mut GpBrush) -> Status;
    fn GdipCreatePath(mode: i32, p: *mut GpPath) -> Status;
    fn GdipDeletePath(p: GpPath) -> Status;
    fn GdipAddPathArc(p: GpPath, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> Status;
    fn GdipAddPathLine(p: GpPath, x1: f32, y1: f32, x2: f32, y2: f32) -> Status;
    fn GdipClosePathFigure(p: GpPath) -> Status;
    fn GdipFillPath(g: GpGraphics, b: GpBrush, p: GpPath) -> Status;
    fn GdipCreateFontFamilyFromName(name: *const u16, coll: *mut c_void, f: *mut GpFontFamily) -> Status;
    fn GdipDeleteFontFamily(f: GpFontFamily) -> Status;
    fn GdipCreateFont(fam: GpFontFamily, size: f32, style: i32, unit: i32, f: *mut GpFont) -> Status;
    fn GdipDeleteFont(f: GpFont) -> Status;
    fn GdipCreateStringFormat(attr: i32, lang: u16, fmt: *mut GpStringFormat) -> Status;
    fn GdipDeleteStringFormat(fmt: GpStringFormat) -> Status;
    fn GdipSetStringFormatAlign(fmt: GpStringFormat, align: i32) -> Status;
    fn GdipSetStringFormatLineAlign(fmt: GpStringFormat, align: i32) -> Status;
    fn GdipDrawString(g: GpGraphics, s: *const u16, len: i32, font: GpFont, rect: *const RectF, fmt: GpStringFormat, brush: GpBrush) -> Status;
}

/// GDI+ 运行时（进程生命周期内保持存活）
pub struct GdiPlus {
    token: usize,
}

impl GdiPlus {
    pub fn new() -> Option<Self> {
        let input = StartupInput {
            version: 1,
            callback: ptr::null_mut(),
            suppress_bg: 0,
            suppress_codecs: 0,
        };
        let mut token = 0usize;
        let st = unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) };
        if st == 0 {
            Some(GdiPlus { token })
        } else {
            None
        }
    }
}

impl Drop for GdiPlus {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.token) };
    }
}

/// GDI+ 绘图上下文
pub struct Graphics {
    g: GpGraphics,
}

impl Graphics {
    pub fn from_hdc(hdc: isize) -> Option<Self> {
        let mut g: GpGraphics = ptr::null_mut();
        let st = unsafe { GdipCreateFromHDC(hdc, &mut g) };
        if st == 0 && !g.is_null() {
            unsafe {
                GdipSetSmoothingMode(g, 4); // AntiAlias
                GdipSetTextRenderingHint(g, 4); // AntiAlias（灰度，无彩边）
            }
            Some(Graphics { g })
        } else {
            None
        }
    }

    fn solid_brush(&self, argb: u32) -> GpBrush {
        let mut b: GpBrush = ptr::null_mut();
        unsafe { GdipCreateSolidFill(argb, &mut b) };
        b
    }

    /// 填充抗锯齿圆角矩形
    pub fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, r: f32, argb: u32) {
        unsafe {
            let mut path: GpPath = ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            let d = 2.0 * r;
            GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathLine(path, x + r, y, x + w - r, y);
            GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathLine(path, x + w, y + r, x + w, y + h - r);
            GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathLine(path, x + w - r, y + h, x + r, y + h);
            GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            GdipAddPathLine(path, x, y + h - r, x, y + r);
            GdipClosePathFigure(path);
            let brush = self.solid_brush(argb);
            GdipFillPath(self.g, brush, path);
            GdipDeleteBrush(brush);
            GdipDeletePath(path);
        }
    }

    /// 填充竖向渐变圆角矩形（上→下）
    pub fn fill_round_grad(&self, x: f32, y: f32, w: f32, h: f32, r: f32, top: u32, bottom: u32) {
        unsafe {
            let mut path: GpPath = ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            let d = 2.0 * r;
            GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathLine(path, x + r, y, x + w - r, y);
            GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathLine(path, x + w, y + r, x + w, y + h - r);
            GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathLine(path, x + w - r, y + h, x + r, y + h);
            GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            GdipAddPathLine(path, x, y + h - r, x, y + r);
            GdipClosePathFigure(path);

            let p1 = PointF { x, y };
            let p2 = PointF { x, y: y + h };
            let mut brush: GpBrush = ptr::null_mut();
            GdipCreateLineBrush(&p1, &p2, top, bottom, 0, &mut brush);
            GdipFillPath(self.g, brush, path);
            GdipDeleteBrush(brush);
            GdipDeletePath(path);
        }
    }

    /// 绘制文字（默认雅黑）。center=true 时居中。
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        s: &str,
        argb: u32,
        size: f32,
        bold: bool,
        center: bool,
    ) {
        self.text_font(x, y, w, h, s, argb, size, bold, center, "Microsoft YaHei UI");
    }

    /// 绘制文字，可指定字体名（如 "Segoe UI Emoji"）
    #[allow(clippy::too_many_arguments)]
    pub fn text_font(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        s: &str,
        argb: u32,
        size: f32,
        bold: bool,
        center: bool,
        font_name: &str,
    ) {
        unsafe {
            let name: Vec<u16> = format!("{}\0", font_name).encode_utf16().collect();
            let mut family: GpFontFamily = ptr::null_mut();
            if GdipCreateFontFamilyFromName(name.as_ptr(), ptr::null_mut(), &mut family) != 0 {
                return;
            }
            let mut font: GpFont = ptr::null_mut();
            let style = if bold { 1 } else { 0 };
            if GdipCreateFont(family, size, style, 2, &mut font) != 0 {
                GdipDeleteFontFamily(family);
                return;
            }
            let mut fmt: GpStringFormat = ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            let align = if center { 1 } else { 0 };
            GdipSetStringFormatAlign(fmt, align);
            GdipSetStringFormatLineAlign(fmt, align);

            let rect = RectF { x, y, w, h };
            let mut text: Vec<u16> = s.encode_utf16().collect();
            if text.is_empty() {
                text.push(0);
            }
            let brush = self.solid_brush(argb);
            GdipDrawString(
                self.g,
                text.as_ptr(),
                text.len() as i32,
                font,
                &rect,
                fmt,
                brush,
            );
            GdipDeleteBrush(brush);
            GdipDeleteStringFormat(fmt);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
        }
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe { GdipDeleteGraphics(self.g) };
    }
}
