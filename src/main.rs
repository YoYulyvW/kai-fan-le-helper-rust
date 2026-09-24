//! 开饭了助手 - Rust 重构版入口。

mod app;
mod config;
mod core;
mod network;
mod platform;
mod ui;

fn main() {
    let _app = app::App::new();
    println!("开饭了助手 Rust 版启动 (v{})", env!("CARGO_PKG_VERSION"));
    println!("配置目录: {}", config::home_dir().display());
    println!("监听端口: UDP {} / 握手 TCP {} / 通信 TCP {}",
        config::BROADCAST_PORT, config::HANDSHAKE_PORT, config::PORT);
}
