//! 轻量日志：写入用户目录日志文件，便于诊断。
//!
//! 多线程安全：用全局互斥锁保证每次写入是一条完整、不交错的日志。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

static LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn log_file() -> PathBuf {
    crate::config::home_dir().join(".kai_fan_le_helper.log")
}

/// 追加一行日志（线程安全，带时间戳）
pub fn log(msg: &str) {
    let lock = LOG_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock();
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
