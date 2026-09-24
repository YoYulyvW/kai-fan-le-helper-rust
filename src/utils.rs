//! 轻量日志：写入用户目录日志文件，便于诊断。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn log_file() -> PathBuf {
    crate::config::home_dir().join(".kai_fan_le_helper.log")
}

/// 追加一行日志（带时间戳）
pub fn log(msg: &str) {
    let ts = now_string();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

fn now_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}
