//! 应用生命周期与模块组装。
//!
//! 架构：
//! - 启动一个 tokio 多线程运行时，承载所有网络异步任务（UDP 监听、TCP 握手、扫描、发送、心跳）。
//! - 所有异步任务通过一个无界 channel 把结果发回 UI 线程。
//! - UI 线程只消费事件，**永不执行阻塞 I/O**。

use std::collections::HashMap;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::config;
use crate::core::session::Session;
use crate::network::{self, NetEvent};
use crate::platform::foreground;
use crate::platform::hotkey::{self, HotkeyEvent, HotkeyHook};
use crate::platform::{clipboard, input};

/// 应用层事件（异步任务 + 热键 -> UI 线程）
#[derive(Debug, Clone)]
pub enum AppEvent {
    Net(NetEvent),
    Hotkey(HotkeyEvent),
    /// 主动扫描结果
    ScanResult(Vec<(String, String)>),
    /// 广播探测完成
    BroadcastResolved { ip: String, name: String },
    /// 发送结果
    SendResult { ok: bool, message: String },
    /// 心跳离线结果
    HeartbeatOffline(Vec<String>),
}

/// 处理事件后返回给 UI 的动作
#[derive(Debug, Clone)]
pub enum UiAction {
    None,
    Flash(String, String),
    SetInput(String),
    ShowMappingChooser(String, Vec<String>),
}

/// 后台服务集合
pub struct App {
    pub session: Session,
    event_rx: UnboundedReceiver<AppEvent>,
    /// 异步任务回传通道（克隆给各处使用）
    tx: UnboundedSender<AppEvent>,
    /// tokio 运行时句柄（保持存活）
    runtime: tokio::runtime::Runtime,
    hook: HotkeyHook,
    pub hwnd: isize,
}

impl App {
    pub fn start() -> Self {
        let session = Session::new();
        let _ = crate::platform::autostart::set_autostart(session.settings.autostart);

        // 多线程 tokio 运行时
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("tokio runtime");

        let (tx, rx) = unbounded_channel::<AppEvent>();

        // 启动网络异步任务
        {
            let tx_net = tx.clone();
            runtime.spawn(async move {
                let (ntx, mut nrx) = unbounded_channel::<NetEvent>();
                let ntx2 = ntx.clone();
                tokio::spawn(network::run_broadcast_listener(config::BROADCAST_PORT, ntx));
                tokio::spawn(network::run_handshake_listener(config::HANDSHAKE_PORT, ntx2));
                while let Some(ev) = nrx.recv().await {
                    if tx_net.send(AppEvent::Net(ev)).is_err() {
                        break;
                    }
                }
            });
        }

        // 热键钩子（独立线程，通过 std channel 桥接到 tokio）
        let keys = build_hotkey_keys(&session);
        let (hot_tx, hot_rx) = std::sync::mpsc::channel::<HotkeyEvent>();
        let hook = HotkeyHook::install(keys, hot_tx);
        {
            let tx_hot = tx.clone();
            runtime.spawn(async move {
                // 阻塞接收 std channel（放到专用线程里）
                let (async_tx, mut async_rx) = unbounded_channel::<HotkeyEvent>();
                std::thread::spawn(move || {
                    while let Ok(ev) = hot_rx.recv() {
                        if async_tx.send(ev).is_err() {
                            break;
                        }
                    }
                });
                while let Some(ev) = async_rx.recv().await {
                    if tx_hot.send(AppEvent::Hotkey(ev)).is_err() {
                        break;
                    }
                }
            });
        }

        App {
            session,
            event_rx: rx,
            tx,
            runtime,
            hook,
            hwnd: 0,
        }
    }

    /// 非阻塞取事件
    pub fn try_event(&mut self) -> Option<AppEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn reload_hotkeys(&self) {
        self.hook.update_keys(build_hotkey_keys(&self.session));
    }

    pub fn hotkey_watchdog_tick(&self) {
        self.hook.update_keys(build_hotkey_keys(&self.session));
    }

    fn is_assistant_focused(&self) -> bool {
        if self.hwnd != 0 && foreground::cursor_in_window(self.hwnd) {
            return true;
        }
        foreground::window_belongs_to_current_process(foreground::foreground_hwnd())
    }

    /// 触发主动扫描（异步）
    pub fn start_scan(&mut self) {
        if self.session.is_discovering() {
            return;
        }
        self.session.set_discovering(true);
        let priority = self.session.settings.known_ips_limited();
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let result = network::scan_network(config::PORT, priority).await;
            let _ = tx.send(AppEvent::ScanResult(result));
        });
    }

    /// 发送文本（异步）
    pub fn send_text(&self, text: &str) -> bool {
        let ip = match self.session.current_ip() {
            Some(ip) => ip.to_string(),
            None => return false,
        };
        let text = text.to_string();
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let resp = network::send_to_phone(
                &ip,
                &text,
                config::PORT,
                std::time::Duration::from_secs(config::SEND_TIMEOUT),
            )
            .await;
            let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let message = resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let _ = tx.send(AppEvent::SendResult { ok, message });
        });
        true
    }

    /// 触发心跳（异步）
    pub fn start_heartbeat(&self) {
        let snapshot: Vec<String> = self.session.devices.iter().map(|d| d.ip.clone()).collect();
        if snapshot.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let mut offline = Vec::new();
            for ip in snapshot {
                if !network::ping_phone(
                    &ip,
                    config::PORT,
                    std::time::Duration::from_millis((config::HEARTBEAT_TIMEOUT * 1000.0) as u64),
                )
                .await
                {
                    offline.push(ip);
                }
            }
            if !offline.is_empty() {
                let _ = tx.send(AppEvent::HeartbeatOffline(offline));
            }
        });
    }

    /// 处理事件，返回 UI 动作（**纯计算，无阻塞 I/O**）
    pub fn handle(&mut self, ev: AppEvent) -> UiAction {
        match ev {
            AppEvent::ScanResult(list) => {
                self.session.set_discovering(false);
                let devices: Vec<crate::core::session::Device> = list
                    .into_iter()
                    .map(|(ip, name)| crate::core::session::Device { ip, name })
                    .collect();
                let n = devices.len();
                self.session.set_scan_result(devices);
                let msg = if n == 0 {
                    "未找到设备".to_string()
                } else {
                    format!("已连接 {}", self.session.current_name().unwrap_or_default())
                };
                UiAction::Flash(msg, if n == 0 { "#FF3B30" } else { "#34C759" }.to_string())
            }
            AppEvent::Net(NetEvent::Handshake { ip, name }) => {
                let is_new = self.session.upsert_device(&ip, &name);
                UiAction::Flash(
                    if is_new { format!("已连接 {}", name) } else { format!("切换到 {}", name) },
                    "#34C759".to_string(),
                )
            }
            AppEvent::Net(NetEvent::BroadcastHit(ip, _port)) => {
                // 异步探测，绝不阻塞 UI
                let tx = self.tx.clone();
                self.runtime.spawn(async move {
                    if let Some((ip, name)) =
                        network::check_ip(&ip, config::PORT, std::time::Duration::from_secs(1)).await
                    {
                        let _ = tx.send(AppEvent::BroadcastResolved { ip, name });
                    }
                });
                UiAction::None
            }
            AppEvent::BroadcastResolved { ip, name } => {
                let is_new = self.session.upsert_device(&ip, &name);
                UiAction::Flash(
                    if is_new { format!("发现 {}", name) } else { format!("已连接 {}", name) },
                    "#34C759".to_string(),
                )
            }
            AppEvent::SendResult { ok, message } => {
                if ok {
                    UiAction::Flash("已发送".to_string(), "#34C759".to_string())
                } else {
                    UiAction::Flash(
                        if message.is_empty() { "发送失败".to_string() } else { message },
                        "#FF3B30".to_string(),
                    )
                }
            }
            AppEvent::HeartbeatOffline(ips) => {
                if self.session.remove_offline(&ips) {
                    UiAction::Flash("设备已断开".to_string(), "#FF9500".to_string())
                } else {
                    UiAction::None
                }
            }
            AppEvent::Hotkey(HotkeyEvent::Main) => self.on_main_hotkey(),
            AppEvent::Hotkey(HotkeyEvent::Mapping(key)) => self.on_mapping_hotkey(&key),
        }
    }

    fn on_main_hotkey(&mut self) -> UiAction {
        let name = crate::core::generate_name();
        clipboard::set_text(&name);
        if self.is_assistant_focused() {
            UiAction::SetInput(name)
        } else {
            paste_to_foreground();
            UiAction::Flash(format!("已粘贴 {}", name), "#34C759".to_string())
        }
    }

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

    pub fn apply_mapping_text(&mut self, text: &str) {
        clipboard::set_text(text);
        let _ = self.session.ingest_share_text(text);
        if !self.is_assistant_focused() {
            paste_to_foreground();
        }
    }

    pub fn on_clipboard_text(&mut self, text: &str) -> Option<String> {
        self.session.ingest_share_text(text)
    }
}

/// 在独立线程延迟发送 Ctrl+V
fn paste_to_foreground() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(40));
        let _ = input::send_ctrl_v();
    });
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
