//! 设备会话：设备列表、当前选中、扫描编排、发送与粘贴流程的纯逻辑部分。
//! 与 UI 解耦，通过方法返回值/回调与外部交互。

use crate::config::{self, HistoryItem, Settings};
use crate::core::{self, title};

/// 一台已发现设备
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub ip: String,
    pub name: String,
}

/// 会话状态机：维护设备与当前选中，触发节流与退避。
pub struct Session {
    pub devices: Vec<Device>,
    pub current_index: usize,
    pub settings: Settings,
    pub history: Vec<HistoryItem>,
    pub mappings: std::collections::BTreeMap<String, Vec<String>>,
    backoff_idx: usize,
    discovering: bool,
}

impl Session {
    pub fn new() -> Self {
        Session {
            devices: Vec::new(),
            current_index: 0,
            settings: Settings::load(),
            history: config::load_history(),
            mappings: config::load_mappings(),
            backoff_idx: 0,
            discovering: false,
        }
    }

    pub fn current(&self) -> Option<&Device> {
        self.devices.get(self.current_index)
    }

    pub fn current_ip(&self) -> Option<&str> {
        self.current().map(|d| d.ip.as_str())
    }

    pub fn set_discovering(&mut self, v: bool) {
        self.discovering = v;
    }

    pub fn is_discovering(&self) -> bool {
        self.discovering
    }

    fn sort_devices(&mut self) {
        self.devices.sort_by_key(|d| ip_sort_key(&d.ip));
    }

    /// 记录/更新一台设备并选中它。返回是否为新设备。
    pub fn upsert_device(&mut self, ip: &str, name: &str) -> bool {
        self.settings.remember_ip(ip);
        self.settings.save();

        if let Some(i) = self.devices.iter().position(|d| d.ip == ip) {
            self.current_index = i;
            self.reset_backoff();
            return false;
        }
        self.devices.push(Device { ip: ip.to_string(), name: name.to_string() });
        self.sort_devices();
        if let Some(i) = self.devices.iter().position(|d| d.ip == ip) {
            self.current_index = i;
        }
        self.reset_backoff();
        true
    }

    /// 扫描完成后替换设备列表，尽量保留原选中项。
    pub fn set_scan_result(&mut self, list: Vec<Device>) {
        let old_ip = self.current_ip().map(|s| s.to_string());
        self.devices = list;
        self.sort_devices();
        if let Some(ip) = old_ip {
            if let Some(i) = self.devices.iter().position(|d| d.ip == ip) {
                self.current_index = i;
                return;
            }
        }
        self.current_index = 0;
        if !self.devices.is_empty() {
            self.reset_backoff();
            for d in self.devices.clone() {
                self.settings.remember_ip(&d.ip);
            }
            self.settings.save();
        }
    }

    /// 心跳失败：移除离线设备。返回移除后是否发生变化。
    pub fn remove_offline(&mut self, ips: &[String]) -> bool {
        let before = self.devices.len();
        let removed_current = self
            .current_ip()
            .map(|ip| ips.contains(&ip.to_string()))
            .unwrap_or(false);
        self.devices.retain(|d| !ips.contains(&d.ip));
        if self.devices.len() == before {
            return false;
        }
        if self.current_index >= self.devices.len() {
            self.current_index = self.devices.len().saturating_sub(1);
        } else if removed_current {
            self.current_index = 0;
        }
        true
    }

    pub fn reset_backoff(&mut self) {
        self.backoff_idx = 0;
    }

    pub fn advance_backoff(&mut self) {
        if self.backoff_idx < config::SCAN_BACKOFF_SEQUENCE.len() - 1 {
            self.backoff_idx += 1;
        }
    }

    pub fn current_backoff_secs(&self) -> u64 {
        config::SCAN_BACKOFF_SEQUENCE[self.backoff_idx]
    }

    /// 处理一段剪贴板/分享文本：解析剧名、写历史、返回展示文本。
    /// 命中返回 Some(display)，否则 None（表示非分享内容）。
    pub fn ingest_share_text(&mut self, text: &str) -> Option<String> {
        if text.is_empty()
            || core::is_noise_clipboard(text)
            || !core::looks_like_douyin_share(text)
        {
            return None;
        }
        let parsed = title::parse(text)?;
        let display = if parsed.is_fast {
            format!("{} - 极速", parsed.title)
        } else {
            parsed.title.clone()
        };
        config::history_add(&mut self.history, text, &parsed.title);
        Some(display)
    }

    /// 从历史文本得到展示字符串（用于双击历史）
    pub fn display_for_history(&self, text: &str) -> String {
        match title::parse(text) {
            Some(p) if p.is_fast => format!("{} - 极速", p.title),
            Some(p) => p.title,
            None => text.to_string(),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

fn ip_sort_key(ip: &str) -> (u32, u32, u32, u32) {
    let mut it = ip.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session() -> Session {
        Session {
            devices: Vec::new(),
            current_index: 0,
            settings: Settings::default(),
            history: Vec::new(),
            mappings: Default::default(),
            backoff_idx: 0,
            discovering: false,
        }
    }

    #[test]
    fn upsert_and_select() {
        let mut s = temp_session();
        assert!(s.upsert_device("192.168.1.10", "手机A"));
        assert!(s.upsert_device("192.168.1.2", "手机B"));
        // 排序后 .2 在前
        assert_eq!(s.devices[0].ip, "192.168.1.2");
        assert_eq!(s.current_ip(), Some("192.168.1.2"));
        // 重复 upsert 不新增
        assert!(!s.upsert_device("192.168.1.10", "手机A"));
        assert_eq!(s.devices.len(), 2);
    }

    #[test]
    fn remove_offline_devices() {
        let mut s = temp_session();
        s.upsert_device("10.0.0.1", "A");
        s.upsert_device("10.0.0.2", "B");
        assert!(s.remove_offline(&["10.0.0.1".to_string()]));
        assert_eq!(s.devices.len(), 1);
        assert_eq!(s.devices[0].ip, "10.0.0.2");
    }

    #[test]
    fn backoff_progression() {
        let mut s = temp_session();
        assert_eq!(s.current_backoff_secs(), 5);
        for _ in 0..20 {
            s.advance_backoff();
        }
        assert_eq!(s.current_backoff_secs(), 60);
        s.reset_backoff();
        assert_eq!(s.current_backoff_secs(), 5);
    }
}
