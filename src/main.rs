//! 开饭了助手 - Rust 重构版入口。

// 隐藏控制台窗口（release 下无黑框）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod core;
mod network;
mod platform;
mod ui;
mod utils;

fn main() {
    let app = app::App::start();
    // 进入 UI 主循环，阻塞直到退出
    ui::run(app);
}
