//! 用户界面与系统托盘（原生 Win32 自绘，低内存）。

pub mod theme;
pub mod tray;
pub mod window;

pub use window::run;
