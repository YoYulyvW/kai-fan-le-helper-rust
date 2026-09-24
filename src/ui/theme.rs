//! 主题配色：浅色 / 深色 / 自动（跟随系统）。
//! 提供 RGB 颜色与 Win32 画刷创建。

/// 主题模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Auto,
        }
    }
}

/// 解析出的实际主题（仅浅色/深色）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTheme {
    Light,
    Dark,
}

impl ThemeMode {
    pub fn resolve(&self) -> ResolvedTheme {
        match self {
            ThemeMode::Light => ResolvedTheme::Light,
            ThemeMode::Dark => ResolvedTheme::Dark,
            ThemeMode::Auto => {
                if crate::platform::detect_system_theme() == "dark" {
                    ResolvedTheme::Dark
                } else {
                    ResolvedTheme::Light
                }
            }
        }
    }
}

/// 一套配色
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// 窗口背景 COLORREF（0x00BBGGRR）
    pub bg: u32,
    /// 文本颜色
    pub text: u32,
    /// 输入框背景
    pub input_bg: u32,
    /// 状态颜色（连接/扫描/错误）
    pub ok: u32,
    pub warn: u32,
    pub err: u32,
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

impl Palette {
    pub fn for_theme(t: ResolvedTheme) -> Self {
        match t {
            ResolvedTheme::Dark => Palette {
                bg: rgb(28, 28, 30),
                text: rgb(255, 255, 255),
                input_bg: rgb(44, 44, 46),
                ok: rgb(52, 199, 89),
                warn: rgb(255, 149, 0),
                err: rgb(255, 59, 48),
            },
            ResolvedTheme::Light => Palette {
                bg: rgb(242, 242, 242),
                text: rgb(28, 28, 30),
                input_bg: rgb(255, 255, 255),
                ok: rgb(40, 167, 69),
                warn: rgb(255, 149, 0),
                err: rgb(255, 59, 48),
            },
        }
    }
}
