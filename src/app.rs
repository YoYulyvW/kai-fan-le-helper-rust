//! 应用生命周期与模块组装（阶段 4/6 实现）。

use crate::config::Settings;

/// 应用上下文：持有配置、网络监听器等。
pub struct App {
    pub settings: Settings,
}

impl App {
    pub fn new() -> Self {
        let settings = Settings::load();
        // 首次运行时同步开机自启（默认开启），与原版一致
        let _ = crate::platform::autostart::set_autostart(settings.autostart);
        App { settings }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
