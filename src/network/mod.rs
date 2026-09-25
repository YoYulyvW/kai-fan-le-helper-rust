//! 网络通信：UDP 广播发现、TCP 握手监听、HTTP 发送、网段扫描。
//!
//! 全部基于 tokio 异步实现：网络 I/O 不阻塞任何线程，扫描/探测高并发。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::UnboundedSender;

use crate::config::{HANDSHAKE_PORT, PORT, SCAN_TIMEOUT};
use crate::core::net::{parse_device_name, ping_request, response_is_ok, submit_body, BroadcastMsg};

/// 网络事件（异步任务 -> UI 线程）
#[derive(Debug, Clone)]
pub enum NetEvent {
    /// 发现设备（来自 UDP 广播）ip, port
    BroadcastHit(String, u16),
    /// 收到 TCP 握手 ip, device_name
    Handshake { ip: String, name: String },
}

/// 获取本机局域网出口 IP（异步探测）
pub async fn get_local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    sock.connect("8.8.8.8:80").await.ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        _ => None,
    }
}

/// 收集本机所有局域网 IPv4 地址（多网卡场景）
pub async fn all_local_ips() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Some(ip) = get_local_ip().await {
        ips.push(ip);
    }
    for candidate in ["192.168.1.1", "192.168.2.1", "10.0.0.1"] {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await {
            if sock.connect((candidate, 80)).await.is_ok() {
                if let Ok(addr) = sock.local_addr() {
                    if let IpAddr::V4(v4) = addr.ip() {
                        if !ips.contains(&v4) {
                            ips.push(v4);
                        }
                    }
                }
            }
        }
    }
    ips
}

/// 探测单个 IP：GET /ping，返回 (ip, device_name)
pub async fn check_ip(ip: &str, port: u16, timeout: Duration) -> Option<(String, String)> {
    let addr: SocketAddr = format!("{}:{}", ip, port).parse().ok()?;
    let fut = async {
        let mut stream = TcpStream::connect(addr).await.ok()?;
        stream.write_all(&ping_request(ip)).await.ok()?;
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.ok()?;
        let data = &buf[..n];
        if !response_is_ok(data) {
            return None;
        }
        let name = parse_device_name(data).unwrap_or_else(|| "开饭了".to_string());
        Some((ip.to_string(), name))
    };
    tokio::time::timeout(timeout, fut).await.ok()?
}

/// 扫描网段：优先探测已知 IP，命中立即返回；否则并发扫描所有本机网段的 /24。
pub async fn scan_network(port: u16, priority_ips: Vec<String>) -> Vec<(String, String)> {
    // 1) 优先探测已知 IP
    if !priority_ips.is_empty() {
        crate::utils::log(&format!("scan: priority ips {:?}", priority_ips));
        let hits = scan_list(priority_ips, port, Duration::from_millis(500)).await;
        if !hits.is_empty() {
            crate::utils::log(&format!("scan: priority hit {:?}", hits));
            return hits;
        }
    }

    // 2) 收集所有本机网段，逐一并发扫描
    let locals = all_local_ips().await;
    crate::utils::log(&format!("scan: local ips {:?}", locals));
    if locals.is_empty() {
        return Vec::new();
    }

    let mut all_hits: Vec<(String, String)> = Vec::new();
    let timeout = Duration::from_millis((SCAN_TIMEOUT * 1000.0) as u64);
    for local in locals {
        let octets = local.octets();
        if octets[0] == 169 || octets[0] == 127 {
            continue;
        }
        let prefix = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        let ips: Vec<String> = (1..=254).map(|i| format!("{}{}", prefix, i)).collect();
        let hits = scan_list(ips, port, timeout).await;
        all_hits.extend(hits);
    }
    all_hits.sort_by_key(|(ip, _)| ip_sort_key(ip));
    all_hits.dedup_by(|a, b| a.0 == b.0);
    crate::utils::log(&format!("scan: found {:?}", all_hits));
    all_hits
}

/// 并发扫描一组 IP（tokio 并发，无上限 worker 数硬限制）
async fn scan_list(ips: Vec<String>, port: u16, timeout: Duration) -> Vec<(String, String)> {
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::unbounded_channel();
    for ip in ips {
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Some(hit) = check_ip(&ip, port, timeout).await {
                let _ = tx.send(hit);
            }
        });
    }
    drop(tx);

    let mut found = Vec::new();
    while let Some(hit) = rx.recv().await {
        found.push(hit);
    }
    found
}

fn ip_sort_key(ip: &str) -> (u32, u32, u32, u32) {
    let mut it = ip.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// 心跳探测单台设备
pub async fn ping_phone(ip: &str, port: u16, timeout: Duration) -> bool {
    let addr: SocketAddr = match format!("{}:{}", ip, port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let fut = async {
        let mut stream = TcpStream::connect(addr).await.ok()?;
        stream.write_all(&ping_request(ip)).await.ok()?;
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.ok()?;
        Some(response_is_ok(&buf[..n]))
    };
    tokio::time::timeout(timeout, fut).await.ok().flatten().unwrap_or(false)
}

/// 发送文本到手机，返回服务器 JSON 响应
pub async fn send_to_phone(ip: &str, text: &str, port: u16, timeout: Duration) -> serde_json::Value {
    let body = submit_body(text);
    let addr: SocketAddr = match format!("{}:{}", ip, port).parse() {
        Ok(a) => a,
        Err(e) => return serde_json::json!({"ok": false, "message": e.to_string()}),
    };
    let fut = async {
        let mut stream = TcpStream::connect(addr).await.ok()?;
        let req = format!(
            "POST /submit HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json; charset=utf-8\r\nUser-Agent: KaiFanLe-Helper/1.0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            ip,
            body.len()
        );
        stream.write_all(req.as_bytes()).await.ok()?;
        stream.write_all(&body).await.ok()?;
        let mut resp = Vec::new();
        stream.read_to_end(&mut resp).await.ok()?;
        let text = String::from_utf8_lossy(&resp);
        let json_str = text.split("\r\n\r\n").nth(1).unwrap_or(&text);
        Some(
            serde_json::from_str(json_str)
                .unwrap_or_else(|_| serde_json::json!({"ok": false, "message": "invalid response"})),
        )
    };
    tokio::time::timeout(timeout, fut)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| serde_json::json!({"ok": false, "message": "timeout"}))
}

/// UDP 广播监听任务（异步）
pub async fn run_broadcast_listener(listen_port: u16, tx: UnboundedSender<NetEvent>) {
    let sock = match UdpSocket::bind(("0.0.0.0", listen_port)).await {
        Ok(s) => s,
        Err(e) => {
            crate::utils::log(&format!("broadcast: bind failed {:?}", e));
            return;
        }
    };
    let _ = sock.set_broadcast(true);
    crate::utils::log(&format!("broadcast: listening udp :{}", listen_port));

    let mut buf = [0u8; 2048];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, addr)) => {
                if let Ok(msg) = serde_json::from_slice::<BroadcastMsg>(&buf[..n]) {
                    if msg.is_valid_hello() {
                        let ip = addr.ip().to_string();
                        if ip != "0.0.0.0" {
                            let _ = tx.send(NetEvent::BroadcastHit(ip, msg.effective_port()));
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }
}

/// TCP 握手监听任务（异步）
pub async fn run_handshake_listener(listen_port: u16, tx: UnboundedSender<NetEvent>) {
    let listener = match TcpListener::bind(("0.0.0.0", listen_port)).await {
        Ok(l) => l,
        Err(e) => {
            crate::utils::log(&format!("handshake: bind failed {:?}", e));
            return;
        }
    };
    crate::utils::log(&format!("handshake: listening tcp :{}", listen_port));

    loop {
        let (mut conn, addr) = match listener.accept().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let n = tokio::time::timeout(Duration::from_secs(2), conn.read(&mut buf))
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or(0);
            let data = &buf[..n];
            let body = if let Some(pos) = crate::core::net::find_subslice(data, b"\r\n\r\n") {
                &data[pos + 4..]
            } else {
                data
            };
            let msg: BroadcastMsg = serde_json::from_slice(body).unwrap_or(BroadcastMsg {
                magic: String::new(),
                action: String::new(),
                port: None,
                device: None,
            });
            if msg.is_valid_hello() {
                let ip = addr.ip().to_string();
                let name = msg.device.clone().unwrap_or_else(|| "手机".to_string());
                let _ = tx.send(NetEvent::Handshake { ip, name });
                let resp = b"{\"ok\":true}";
                let _ = conn
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            resp.len()
                        )
                        .as_bytes(),
                    )
                    .await;
                let _ = conn.write_all(resp).await;
            } else {
                let _ = conn
                    .write_all(
                        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            }
        });
    }
}

pub fn default_ports() -> (u16, u16, u16) {
    (PORT, crate::config::BROADCAST_PORT, HANDSHAKE_PORT)
}
