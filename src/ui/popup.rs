//! 通用列表弹窗：设备选择 / 历史记录 / 映射选择。
//!
//! 无边框、置顶、带搜索框与列表框，双击选择。
//! 本模块集中弹窗相关的 unsafe Win32 调用。

/// 弹窗选择结果
pub enum PopupResult {
    Cancelled,
    Selected(usize),
    FilterChanged(String),
}

/// 弹窗类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Device,
    History,
    Mapping,
}
