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
    // 单实例保护：已有实例则直接退出，避免多实例争抢托盘/热键/端口
    if !acquire_single_instance() {
        return;
    }

    let app = app::App::start();
    // 进入 UI 主循环，阻塞直到退出
    ui::run(app);
}

/// 通过命名互斥体确保单实例。返回 true 表示本进程是唯一实例。
fn acquire_single_instance() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(attr: *const u8, initial_owner: i32, name: *const u16) -> isize;
        fn GetLastError() -> u32;
    }
    const ERROR_ALREADY_EXISTS: u32 = 183;
    unsafe {
        let name: Vec<u16> = "KaiFanLeHelper_SingleInstance\0".encode_utf16().collect();
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle == 0 {
            return true; // 创建失败则放行
        }
        // 互斥体句柄故意不关闭，保持到进程结束
        std::mem::forget(handle);
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}
