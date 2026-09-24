//! 应用生命周期与模块组装。
//!
//! 负责：
//! - 加载配置、同步开机自启
//! - 启动 UDP 广播监听 / TCP 握手监听（后台线程）
//! - 安装全局热键钩子（后台线程）
//! - 通过事件通道把网络/热键事件汇聚到主循环
//! - 处理业务闭环：热键 → 生成名字 → 判断前台 → Ctrl+V 粘贴

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config;
use crate::core::session::Session;
use crate::network::{self, NetEvent};
use crate::platform::foreground;
use crate::platform::hotkey::{self, HotkeyEvent, HotkeyHook};
use crate::platform::{clipboard, input};

/// 应用层事件（网络 + 热键统一）
#[derive(Debug, Clone)]
pub enum AppEvent {
    Net(NetEvent),
    Hotkey(HotkeyEvent),
}

/// 处理事件后返回给 UI 的动作
#[derive(Debug, Clone)]
pub enum UiAction {
    /// 无动作
    None,
    /// 在状态栏闪示文本（文本, 颜色）
    Flash(String, String),
    /// 把文本填入输入框
    SetInput(String),
    /// 选择目标内容弹窗（多条映射）
    ShowMappingChooser(String, Vec<String>),
}

/// 后台服务集合
pub struct App {
    pub session: Session,
    event_rx: Receiver<AppEvent>,
    broadcast: network::BroadcastListener,
    handshake: network::HandshakeListener,
    hook: HotkeyHook,
    /// 助手窗口句柄（用于前台判断）
    pub hwnd: isize,
}

impl App {
    /// 组装应用：加载配置、启动网络与热键后台任务。
    pub fn start() -> Self {
        let session = Session::new();

        // 首次运行/设置变更时同步开机自启（默认开启），与原版一致
        let _ = crate::platform::autostart::set_autostart(session.settings.autostart);

        let (tx, rx): (Sender<AppEvent>, Receiver<AppEvent>) = channel();

        let net_tx = tx.clone();
        let broadcast = network::BroadcastListener::start(
            config::BROADCAST_PORT,
            forward_net(net_tx.clone()),
        );
        let handshake = network::HandshakeListener::start(
            config::HANDSHAKE_PORT,
            forward_net(net_tx),
        );

        let keys = build_hotkey_keys(&session);
        let hook = HotkeyHook::install(keys, forward_hotkey(tx));

        App {
            session,
            event_rx: rx,
            broadcast,
            handshake,
            hook,
            hwnd: 0,
        }
    }

    pub fn next_event(&self) -> Option<AppEvent> {
        self.event_rx.recv().ok()
    }

    pub fn try_event(&self) -> Option<AppEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn reload_hotkeys(&self) {
        self.hook.update_keys(build_hotkey_keys(&self.session));
    }

    /// 热键看门狗：定期调用，检测钩子健康（当前实现为刷新按键表）
    pub fn hotkey_watchdog_tick(&self) {
        self.hook.update_keys(build_hotkey_keys(&self.session));
    }

    /// 判断助手窗口当前是否"处于前台"（鼠标在窗口内 或 前台窗口属于本进程）
    fn is_assistant_focused(&self) -> bool {
        if self.hwnd != 0 && foreground::cursor_in_window(self.hwnd) {
            return true;
        }
        foreground::window_belongs_to_current_process(foreground::foreground_hwnd())
    }

    /// 处理一个事件，返回 UI 需要执行的动作。
    pub fn handle(&mut self, ev: AppEvent) -> UiAction {
        match ev {
            AppEvent::Net(NetEvent::Handshake { ip, name }) => {
                let is_new = self.session.upsert_device(&ip, &name);
                UiAction::Flash(
                    if is_new {
                        format!("已连接 {}", name)
                    } else {
                        format!("切换到 {}", name)
                    },
                    "#34C759".to_string(),
                )
            }
            AppEvent::Net(NetEvent::BroadcastHit(ip, _port)) => {
                if let Some((ip, name)) = network::check_ip(
                    &ip,
                    config::PORT,
                    std::time::Duration::from_secs(1),
                ) {
                    self.session.upsert_device(&ip, &name);
                    UiAction::Flash(format!("发现 {}", name), "#34C759".to_string())
                } else {
                    UiAction::None
                }
            }
            AppEvent::Hotkey(HotkeyEvent::Main) => self.on_main_hotkey(),
            AppEvent::Hotkey(HotkeyEvent::Mapping(key)) => self.on_mapping_hotkey(&key),
        }
    }

    /// 主热键：生成名字 → 复制 → 判断前台 → 填入或粘贴
    fn on_main_hotkey(&mut self) -> UiAction {
        let name = crate::core::generate_name();
        clipboard::set_text(&name);

        if self.is_assistant_focused() {
            // 助手窗口前台：直接填入输入框
            UiAction::SetInput(name)
        } else {
            // 其它程序聚焦：模拟 Ctrl+V 粘贴
            paste_to_foreground(&name);
            UiAction::Flash(format!("已粘贴 {}", name), "#34C759".to_string())
        }
    }

    /// 快捷映射热键
    fn on_mapping_hotkey(&mut self, key: &str) -> UiAction {
        let items = self.session.mappings.get(key).cloned().unwrap_or_default();
        match items.len() {
            0 => UiAction::None,
            1 => {
                let text = items[0].clone();
                self.apply_mapping_text(&text);
                UiAction::None
            }
            _ => UiAction::ShowMappingChooser(key.to_string(), items),
        }
    }

    /// 应用一段映射文本：复制 → 解析分享文本 → 前台则填入，否则粘贴
    pub fn apply_mapping_text(&mut self, text: &str) {
        clipboard::set_text(text);
        let parsed = self.session.ingest_share_text(text);
        if self.is_assistant_focused() {
            if parsed.is_none() {
                // 由 UI 填入（这里通过事件无法直接操作 UI，交由调用方）
            }
        } else {
            paste_to_foreground(text);
        }
    }

    /// 发送文本到当前选中设备（在后台线程执行，结果通过事件回传）
    pub fn send_text(&self, text: &str) -> bool {
        let ip = match self.session.current_ip() {
            Some(ip) => ip.to_string(),
            None => return false,
        };
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = network::send_to_phone(
                &ip,
                &text,
                config::PORT,
                std::time::Duration::from_secs(config::SEND_TIMEOUT),
            );
        });
        true
    }

    /// 心跳：探测所有设备，返回离线的 IP 列表
    pub fn heartbeat_offline(&self) -> Vec<String> {
        let snapshot: Vec<String> = self
            .session
            .devices
            .iter()
            .map(|d| d.ip.clone())
            .collect();
        let mut offline = Vec::new();
        for ip in snapshot {
            if !network::ping_phone(
                &ip,
                config::PORT,
                std::time::Duration::from_millis((config::HEARTBEAT_TIMEOUT * 1000.0) as u64),
            ) {
                offline.push(ip);
            }
        }
        offline
    }

    /// 应用心跳结果：移除离线设备，返回是否有变化
    pub fn apply_heartbeat(&mut self, offline: &[String]) -> bool {
        self.session.remove_offline(offline)
    }

    /// 处理剪贴板文本（防抖后调用）：命中分享则返回展示文本
    pub fn on_clipboard_text(&mut self, text: &str) -> Option<String> {
        self.session.ingest_share_text(text)
    }
}

/// 在独立线程延迟发送 Ctrl+V（避免主线程键盘钩子上下文干扰注入）
fn paste_to_foreground(_text: &str) {
    std::thread::spawn(|| {
        // 稍延迟，确保剪贴板就绪、前台窗口稳定
        std::thread::sleep(std::time::Duration::from_millis(40));
        let _ = input::send_ctrl_v();
    });
}

impl Drop for App {
    fn drop(&mut self) {
        self.broadcast.stop();
        self.handshake.stop();
    }
}

fn build_hotkey_keys(session: &Session) -> HashMap<u32, HotkeyEvent> {
    let mut keys = HashMap::new();
    if session.settings.hotkey_enabled {
        if let Some(vk) = hotkey::vk_for(&session.settings.hotkey) {
            keys.insert(vk, HotkeyEvent::Main);
        }
    }
    if session.settings.mapping_enabled {
        for k in session.mappings.keys() {
            if k == "F1" {
                continue;
            }
            if session.settings.hotkey_enabled && *k == session.settings.hotkey {
                continue;
            }
            if let Some(vk) = hotkey::vk_for(k) {
                keys.entry(vk).or_insert_with(|| HotkeyEvent::Mapping(k.clone()));
            }
        }
    }
    keys
}

fn forward_hotkey(tx: Sender<AppEvent>) -> Sender<HotkeyEvent> {
    let (hot_tx, hot_rx) = channel::<HotkeyEvent>();
    std::thread::spawn(move || {
        while let Ok(ev) = hot_rx.recv() {
            if tx.send(AppEvent::Hotkey(ev)).is_err() {
                break;
            }
        }
    });
    hot_tx
}

fn forward_net(tx: Sender<AppEvent>) -> Sender<NetEvent> {
    let (net_tx, net_rx) = channel::<NetEvent>();
    std::thread::spawn(move || {
        while let Ok(ev) = net_rx.recv() {
            if tx.send(AppEvent::Net(ev)).is_err() {
                break;
            }
        }
    });
    net_tx
}
