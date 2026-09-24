//! 用户界面与系统托盘。
//!
//! 形态（遵循 DEVELOPMENT.md）：
//! - 无边框、始终置顶、工具窗口（不出现在任务栏）
//! - 小尺寸工具条，紧凑布局
//! - 系统托盘提供显示/隐藏、设置、退出等入口
//!
//! 所有 Win32 unsafe 调用集中在本模块，并加注释。

pub mod theme;
pub mod tray;
pub mod window;

pub use window::run;
