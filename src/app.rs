//! 应用生命周期与模块组装。
//!
//! 负责：
//! - 加载配置、同步开机自启
//! - 启动 UDP 广播监听 / TCP 握手监听（后台线程）
//! - 安装全局热键钩子（后台线程）
//! - 通过事件通道把网络/热键事件汇聚到主循环

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config;
use crate::core::session::Session;
use crate::network::{self, NetEvent};
use crate::platform::hotkey::{self, HotkeyEvent, HotkeyHook};

/// 应用层事件（网络 + 热键统一）
#[derive(Debug, Clone)]
pub enum AppEvent {
    Net(NetEvent),
    Hotkey(HotkeyEvent),
}

/// 后台服务集合
pub struct App {
    pub session: Session,
    event_rx: Receiver<AppEvent>,
    broadcast: network::BroadcastListener,
    handshake: network::HandshakeListener,
    hook: HotkeyHook,
}

impl App {
    /// 组装应用：加载配置、启动网络与热键后台任务。
    pub fn start() -> Self {
        let session = Session::new();

        // 首次运行/设置变更时同步开机自启（默认开启），与原版一致
        let _ = crate::platform::autostart::set_autostart(session.settings.autostart);

        // 统一事件通道：网络线程与钩子线程共用发送端
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

        // 组装初始热键列表
        let keys = build_hotkey_keys(&session);
        let hook = HotkeyHook::install(keys, forward_hotkey(tx));

        App {
            session,
            event_rx: rx,
            broadcast,
            handshake,
            hook,
        }
    }

    /// 阻塞等待一个事件（供主循环使用）。
    pub fn next_event(&self) -> Option<AppEvent> {
        self.event_rx.recv().ok()
    }

    /// 尝试非阻塞地取一个事件。
    pub fn try_event(&self) -> Option<AppEvent> {
        self.event_rx.try_recv().ok()
    }

    /// 热键配置变化后刷新钩子按键表。
    pub fn reload_hotkeys(&self) {
        self.hook.update_keys(build_hotkey_keys(&self.session));
    }

    /// 处理一个事件（核心业务），返回需要 UI 展示的状态提示（可选）。
    pub fn handle(&mut self, ev: AppEvent) -> Option<String> {
        match ev {
            AppEvent::Net(NetEvent::Handshake { ip, name }) => {
                let is_new = self.session.upsert_device(&ip, &name);
                Some(if is_new {
                    format!("已连接 {}", name)
                } else {
                    format!("切换到 {}", name)
                })
            }
            AppEvent::Net(NetEvent::BroadcastHit(ip, _port)) => {
                // 广播命中：先探测再入库
                if let Some((ip, name)) = network::check_ip(
                    &ip,
                    config::PORT,
                    std::time::Duration::from_secs(1),
                ) {
                    self.session.upsert_device(&ip, &name);
                    Some(format!("发现 {}", name))
                } else {
                    None
                }
            }
            AppEvent::Hotkey(HotkeyEvent::Main) => {
                let name = crate::core::generate_name();
                Some(format!("已生成 {}", name))
            }
            AppEvent::Hotkey(HotkeyEvent::Mapping(key)) => {
                let items = self.session.mappings.get(&key).cloned().unwrap_or_default();
                match items.len() {
                    0 => None,
                    1 => Some(items[0].clone()),
                    _ => Some(format!("映射 {} 有多条内容，待弹窗选择", key)),
                }
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.broadcast.stop();
        self.handshake.stop();
        // hook 会在自身 Drop 中卸载
    }
}

/// 组装热键 vk -> 事件 映射表（主热键 + 快捷映射，F1 保留给主热键）
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

/// 把热键事件转发到统一通道
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

/// 把网络事件转发到统一通道
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
