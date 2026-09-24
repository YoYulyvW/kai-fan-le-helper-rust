//! 网络通信：UDP 广播发现、TCP 握手监听、HTTP 发送、网段扫描。
//! 使用标准库，避免引入重型异步运行时，降低内存占用。

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{HANDSHAKE_PORT, PORT, SCAN_TIMEOUT};
use crate::core::net::{parse_device_name, ping_request, response_is_ok, submit_body, BroadcastMsg};

/// 网络事件（发送到 UI 线程）
#[derive(Debug, Clone)]
pub enum NetEvent {
    /// 发现设备（来自 UDP 广播）ip, port
    BroadcastHit(String, u16),
    /// 收到 TCP 握手 ip, device_name
    Handshake { ip: String, name: String },
}

/// 获取本机局域网 IP（连接 8.8.8.8 探测出口地址）
pub fn get_local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        _ => None,
    }
}

/// 收集本机所有局域网 IPv4 地址（用于多网卡场景）
pub fn all_local_ips() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    // 主路径：连外网探测
    if let Some(ip) = get_local_ip() {
        ips.push(ip);
    }
    // 补充：常见私有网段的本地地址
    for candidate in ["192.168.1.1", "192.168.2.1", "10.0.0.1"] {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            if sock.connect((candidate, 80)).is_ok() {
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
pub fn check_ip(ip: &str, port: u16, timeout: Duration) -> Option<(String, String)> {
    let addr: SocketAddr = format!("{}:{}", ip, port).parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream.write_all(&ping_request(ip)).ok()?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).ok()?;
    let data = &buf[..n];
    if !response_is_ok(data) {
        return None;
    }
    let name = parse_device_name(data).unwrap_or_else(|| "开饭了".to_string());
    Some((ip.to_string(), name))
}

/// 扫描网段：优先探测已知 IP，命中立即返回；否则扫描所有本机网段的 /24。
pub fn scan_network(port: u16, priority_ips: Vec<String>) -> Vec<(String, String)> {
    // 1) 优先探测已知 IP（曾连上过），命中立即返回
    if !priority_ips.is_empty() {
        crate::utils::log(&format!("scan: priority ips {:?}", priority_ips));
        let hits = scan_list(&priority_ips, port, Duration::from_millis(500));
        if !hits.is_empty() {
            crate::utils::log(&format!("scan: priority hit {:?}", hits));
            return hits;
        }
    }

    // 2) 收集所有本机网段（多网卡场景），逐一扫描 /24
    let locals = all_local_ips();
    crate::utils::log(&format!("scan: local ips {:?}", locals));
    if locals.is_empty() {
        return Vec::new();
    }

    let mut all_hits: Vec<(String, String)> = Vec::new();
    let timeout = Duration::from_millis((SCAN_TIMEOUT * 1000.0) as u64);
    for local in locals {
        let octets = local.octets();
        // 跳过 169.254.x（链路本地）和 127.x
        if octets[0] == 169 || octets[0] == 127 {
            continue;
        }
        let prefix = format!("{}.{}.{}.", octets[0], octets[1], octets[2]);
        let ips: Vec<String> = (1..=254).map(|i| format!("{}{}", prefix, i)).collect();
        let hits = scan_list(&ips, port, timeout);
        all_hits.extend(hits);
    }
    all_hits.sort_by_key(|(ip, _)| ip_sort_key(ip));
    all_hits.dedup_by(|a, b| a.0 == b.0);
    crate::utils::log(&format!("scan: found {:?}", all_hits));
    all_hits
}

/// 并发扫描一组 IP，结果按 IP 排序
fn scan_list(ips: &[String], port: u16, timeout: Duration) -> Vec<(String, String)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ips = Arc::new(ips.to_vec());
    let workers = crate::config::SCAN_MAX_WORKERS.min(ips.len().max(1));
    let chunk = (ips.len() + workers - 1) / workers.max(1);

    let mut handles = Vec::new();
    for w in 0..workers {
        let start = w * chunk;
        if start >= ips.len() {
            break;
        }
        let end = (start + chunk).min(ips.len());
        let slice: Vec<String> = ips[start..end].to_vec();
        let tx = tx.clone();
        handles.push(std::thread::spawn(move || {
            for ip in slice {
                if let Some(hit) = check_ip(&ip, port, timeout) {
                    let _ = tx.send(hit);
                }
            }
        }));
    }
    drop(tx);
    let mut found: Vec<(String, String)> = rx.iter().collect();
    for h in handles {
        let _ = h.join();
    }
    found.sort_by_key(|(ip, _)| ip_sort_key(ip));
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
pub fn ping_phone(ip: &str, port: u16, timeout: Duration) -> bool {
    let addr: SocketAddr = match format!("{}:{}", ip, port).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let mut stream = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return false;
    }
    if stream.write_all(&ping_request(ip)).is_err() {
        return false;
    }
    let mut buf = [0u8; 4096];
    match stream.read(&mut buf) {
        Ok(n) => response_is_ok(&buf[..n]),
        Err(_) => false,
    }
}

/// 发送文本到手机，返回服务器 JSON 响应
pub fn send_to_phone(ip: &str, text: &str, port: u16, timeout: Duration) -> serde_json::Value {
    let body = submit_body(text);
    let result = (|| -> std::io::Result<serde_json::Value> {
        let addr: SocketAddr = format!("{}:{}", ip, port)
            .parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "bad addr"))?;
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let req = format!(
            "POST /submit HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json; charset=utf-8\r\nUser-Agent: KaiFanLe-Helper/1.0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            ip,
            body.len()
        );
        stream.write_all(req.as_bytes())?;
        stream.write_all(&body)?;
        let mut resp = Vec::new();
        stream.read_to_end(&mut resp)?;
        let text = String::from_utf8_lossy(&resp);
        let json_str = text.split("\r\n\r\n").nth(1).unwrap_or(&text);
        Ok(serde_json::from_str(json_str).unwrap_or_else(|_| {
            serde_json::json!({"ok": false, "message": "invalid response"})
        }))
    })();

    result.unwrap_or_else(|e| serde_json::json!({"ok": false, "message": e.to_string()}))
}

/// UDP 广播监听器（后台线程）
pub struct BroadcastListener {
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl BroadcastListener {
    pub fn start(listen_port: u16, tx: Sender<NetEvent>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let handle = std::thread::spawn(move || {
            let sock = match UdpSocket::bind(("0.0.0.0", listen_port)) {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = sock.set_broadcast(true);
            let _ = sock.set_read_timeout(Some(Duration::from_millis(1000)));
            let mut buf = [0u8; 2048];
            while r.load(Ordering::Relaxed) {
                match sock.recv_from(&mut buf) {
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
        });
        BroadcastListener { running, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// TCP 握手监听器（手机打开时主动连本机 8850 端口）
pub struct HandshakeListener {
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl HandshakeListener {
    pub fn start(listen_port: u16, tx: Sender<NetEvent>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let handle = std::thread::spawn(move || {
            let listener = match TcpListener::bind(("0.0.0.0", listen_port)) {
                Ok(l) => l,
                Err(_) => return,
            };
            let _ = listener.set_nonblocking(true);
            while r.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut conn, addr)) => {
                        let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buf = [0u8; 4096];
                        let n = conn.read(&mut buf).unwrap_or(0);
                        let data = &buf[..n];
                        let body = if let Some(pos) = crate::core::net::find_subslice(data, b"\r\n\r\n") {
                            &data[pos + 4..]
                        } else {
                            data
                        };
                        let msg: BroadcastMsg =
                            serde_json::from_slice(body).unwrap_or(BroadcastMsg {
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
                            let _ = conn.write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    resp.len()
                                )
                                .as_bytes(),
                            );
                            let _ = conn.write_all(resp);
                        } else {
                            let _ = conn.write_all(
                                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            );
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    Err(_) => break,
                }
            }
        });
        HandshakeListener { running, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn default_ports() -> (u16, u16, u16) {
    (PORT, crate::config::BROADCAST_PORT, HANDSHAKE_PORT)
}
