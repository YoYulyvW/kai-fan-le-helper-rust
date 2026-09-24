//! 用户界面与系统托盘（阶段 6 实现）。
//! 当前仅定义对外接口，保证编译通过。

/// UI 状态（供托盘菜单和主窗口共享）
#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub status_text: String,
    pub status_color: String,
    pub connected: bool,
}
