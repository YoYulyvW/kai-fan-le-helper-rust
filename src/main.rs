//! 开饭了助手 - Rust 重构版入口。

mod app;
mod config;
mod core;
mod network;
mod platform;
mod ui;

use std::time::Duration;

fn main() {
    let mut app = app::App::start();

    println!("开饭了助手 Rust 版启动 (v{})", env!("CARGO_PKG_VERSION"));
    println!("配置目录: {}", config::home_dir().display());
    println!(
        "监听端口: UDP {} / 握手 TCP {} / 通信 TCP {}",
        config::BROADCAST_PORT,
        config::HANDSHAKE_PORT,
        config::PORT
    );
    println!("后台服务已启动（UDP 发现 / TCP 握手 / 全局热键）。");

    // 主循环：轮询事件并处理（UI 阶段将替换为图形消息循环）
    loop {
        match app.try_event() {
            Some(ev) => {
                if let Some(msg) = app.handle(ev) {
                    println!("[事件] {}", msg);
                }
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
