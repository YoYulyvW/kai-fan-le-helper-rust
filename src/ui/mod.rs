//! 用户界面与系统托盘。
//!
//! 主窗口使用 egui/eframe 实现现代圆角风格；系统托盘使用原生 Shell_NotifyIcon。

pub mod theme;
pub mod tray;
pub mod window;

pub use window::run;
