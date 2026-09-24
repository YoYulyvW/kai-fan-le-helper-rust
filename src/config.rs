//! 配置读写与兼容逻辑
//!
//! 与原 Python 版本保持一致：
//! - 设置文件：~/.kai_fan_le_helper_settings.json
//! - 历史文件：~/.kai_fan_le_helper_history.json
//! - 快捷映射：mappings.txt（优先 exe 同目录，其次用户目录）

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ============================================================
// 网络 / 行为常量（必须与原版一致，不得更改）
// ============================================================
pub const PORT: u16 = 8848;
pub const BROADCAST_PORT: u16 = 8849;
pub const HANDSHAKE_PORT: u16 = 8850;
pub const SCAN_TIMEOUT: f64 = 0.3;
pub const SCAN_MAX_WORKERS: usize = 128;
pub const HEARTBEAT_INTERVAL: u64 = 6;
pub const HEARTBEAT_TIMEOUT: f64 = 1.5;
pub const CLIPBOARD_DEBOUNCE: u64 = 400;
pub const SEND_TIMEOUT: u64 = 4;
pub const MAX_HISTORY: usize = 50;

pub const SCAN_BACKOFF_SEQUENCE: [u64; 10] = [5, 5, 5, 5, 5, 5, 10, 15, 30, 60];
pub const IDLE_SCAN_INTERVAL: u64 = 3;
pub const KNOWN_IPS_MAX: usize = 10;

pub const WIN_WIDTH: i32 = 380;
pub const WIN_HEIGHT: i32 = 44;

pub const DEFAULT_HOTKEY: &str = "F1";
pub const HOTKEY_WATCHDOG_INTERVAL: u64 = 15;
pub const HOTKEY_WATCHDOG_FORCE_EVERY: u32 = 12;

pub const AUTOSTART_REG_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const AUTOSTART_REG_NAME: &str = "KaiFanLeHelper";

pub const MAPPING_FILE_NAME: &str = "mappings.txt";

/// 用户主目录（Windows 下取 USERPROFILE）
pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn settings_file() -> PathBuf {
    home_dir().join(".kai_fan_le_helper_settings.json")
}

pub fn history_file() -> PathBuf {
    home_dir().join(".kai_fan_le_helper_history.json")
}

/// exe 所在目录（打包后即 exe 目录；开发时为 target 目录）
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

// ============================================================
// 设置
// ============================================================

fn default_true() -> bool { true }
fn default_auto_scan() -> bool { true }
fn default_hotkey() -> String { DEFAULT_HOTKEY.to_string() }
fn default_hotkey_enabled() -> bool { true }
fn default_mapping_enabled() -> bool { false }
fn default_show_name_btn() -> bool { true }
fn default_auto_push() -> bool { false }
fn default_clear_clipboard() -> bool { true }
fn default_clipboard_materialize() -> bool { false }
fn default_autostart() -> bool { true }
fn default_scale() -> f64 { 1.0 }

/// 设置项。字段带默认值，缺失键可读，未知键忽略（向后兼容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_auto_scan")]
    pub auto_scan: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_hotkey_enabled")]
    pub hotkey_enabled: bool,
    #[serde(default = "default_mapping_enabled")]
    pub mapping_enabled: bool,
    #[serde(default = "default_show_name_btn")]
    pub show_name_btn: bool,
    #[serde(default = "default_auto_push")]
    pub auto_push: bool,
    #[serde(default = "default_clear_clipboard")]
    pub clear_clipboard: bool,
    #[serde(default = "default_clipboard_materialize")]
    pub clipboard_materialize: bool,
    #[serde(default = "default_autostart")]
    pub autostart: bool,
    #[serde(default = "default_scale")]
    pub scale_multiplier: f64,
    #[serde(default)]
    pub known_ips: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_scan: default_auto_scan(),
            hotkey: default_hotkey(),
            hotkey_enabled: default_hotkey_enabled(),
            mapping_enabled: default_mapping_enabled(),
            show_name_btn: default_show_name_btn(),
            auto_push: default_auto_push(),
            clear_clipboard: default_clear_clipboard(),
            clipboard_materialize: default_clipboard_materialize(),
            autostart: default_autostart(),
            scale_multiplier: default_scale(),
            known_ips: Vec::new(),
        }
    }
}

impl Settings {
    /// 读取设置文件。文件不存在或解析失败均返回默认值（与原版一致）。
    pub fn load() -> Self {
        let path = settings_file();
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<Settings>(&s).unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(settings_file(), s);
        }
    }

    /// 记住一个 IP（放到最前，去重，最多 KNOWN_IPS_MAX 个）
    pub fn remember_ip(&mut self, ip: &str) {
        if ip.is_empty() {
            return;
        }
        self.known_ips.retain(|x| x != ip);
        self.known_ips.insert(0, ip.to_string());
        self.known_ips.truncate(KNOWN_IPS_MAX);
    }

    pub fn known_ips_limited(&self) -> Vec<String> {
        self.known_ips.iter().take(KNOWN_IPS_MAX).cloned().collect()
    }
}

// ============================================================
// 历史记录
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub text: String,
    pub title: String,
    #[serde(default)]
    pub time: String,
}

pub fn load_history() -> Vec<HistoryItem> {
    match fs::read_to_string(history_file()) {
        Ok(s) => {
            let v: Vec<HistoryItem> = serde_json::from_str(&s).unwrap_or_default();
            v.into_iter().take(MAX_HISTORY).collect()
        }
        Err(_) => Vec::new(),
    }
}

pub fn save_history(items: &[HistoryItem]) {
    if let Ok(s) = serde_json::to_string_pretty(&items[..items.len().min(MAX_HISTORY)]) {
        let _ = fs::write(history_file(), s);
    }
}

/// 追加历史：同 title 去重后置顶，返回更新后的列表。
pub fn history_add(items: &mut Vec<HistoryItem>, text: &str, title: &str) {
    items.retain(|it| it.title != title);
    items.insert(0, HistoryItem {
        text: text.to_string(),
        title: title.to_string(),
        time: now_mmdd_hhmm(),
    });
    items.truncate(MAX_HISTORY);
    save_history(items);
}

fn now_mmdd_hhmm() -> String {
    // 简化：使用系统时间，格式与 Python 的 "%m-%d %H:%M" 一致
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 转为本地时间需要额外处理；此处仅占位，后续用 chrono 或 win32 补齐。
    format!("{}", secs)
}

// ============================================================
// 快捷映射（mappings.txt）
// ============================================================

pub fn default_mappings() -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    m.insert("F2".to_string(), vec!["懂车帝".to_string()]);
    m.insert("F3".to_string(), vec!["易车".to_string()]);
    m.insert("F4".to_string(), vec!["巨量引擎".to_string()]);
    m
}

fn mapping_config_path() -> PathBuf {
    let exe_path = exe_dir().join(MAPPING_FILE_NAME);
    if exe_path.exists() {
        return exe_path;
    }
    let user_path = home_dir().join(format!(".{}", MAPPING_FILE_NAME));
    if user_path.exists() {
        return user_path;
    }
    // 尝试写入默认文件到 exe 目录，失败则退回用户目录
    if write_default_mapping(&exe_path).is_ok() {
        exe_path
    } else {
        user_path
    }
}

fn write_default_mapping(path: &Path) -> std::io::Result<()> {
    let mut lines = vec![
        "# 开饭了助手 - 快捷映射配置".to_string(),
        "# [按键] 开始一个节点，节点下每行一条内容".to_string(),
        "# 单条内容 -> 按热键直接粘贴；多条内容 -> 弹窗选择".to_string(),
        "# F1 固定为生成名字，此处写 F1 会被忽略".to_string(),
        String::new(),
    ];
    for (k, items) in default_mappings() {
        lines.push(format!("[{}]", k));
        lines.extend(items);
        lines.push(String::new());
    }
    fs::write(path, lines.join("\n"))
}

/// 读取映射配置，格式为 [F2] 后跟若干行内容。解析失败返回默认值。
pub fn load_mappings() -> BTreeMap<String, Vec<String>> {
    let path = mapping_config_path();
    let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Ok(content) = fs::read_to_string(&path) {
        let mut cur: Option<String> = None;
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let key = line[1..line.len() - 1].trim().to_uppercase();
                cur = Some(key.clone());
                result.entry(key).or_default();
                continue;
            }
            if let Some(k) = &cur {
                result.get_mut(k).unwrap().push(line.to_string());
            }
        }
    }
    result.retain(|_, v| !v.is_empty());
    if result.is_empty() {
        default_mappings()
    } else {
        result
    }
}
