//! 网络协议常量与纯解析逻辑（无 IO）。

/// UDP 广播消息结构，字段与原版一致：magic=KFL, action=hello
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BroadcastMsg {
    pub magic: String,
    pub action: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub device: Option<String>,
}

impl BroadcastMsg {
    /// 校验是否为合法的 KFL hello 广播
    pub fn is_valid_hello(&self) -> bool {
        self.magic == "KFL" && self.action == "hello"
    }

    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(crate::config::PORT)
    }
}

/// 从 HTTP 响应字节中解析设备名（/ping 响应体里 device 或 model 字段）
pub fn parse_device_name(data: &[u8]) -> Option<String> {
    let pos = find_subslice(data, b"\r\n\r\n")?;
    let body = &data[pos + 4..];
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("device")
        .or_else(|| v.get("model"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

/// 响应是否包含 ok 标志
pub fn response_is_ok(data: &[u8]) -> bool {
    find_subslice(data, b"\"ok\"").is_some() || find_subslice(data, b"200 OK").is_some()
}

pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// 构造 /ping 请求
pub fn ping_request(ip: &str) -> Vec<u8> {
    format!("GET /ping HTTP/1.0\r\nHost: {}\r\n\r\n", ip).into_bytes()
}

/// 构造 /submit POST 请求体
pub fn submit_body(text: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "text": text })).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hello() {
        let m: BroadcastMsg =
            serde_json::from_str(r#"{"magic":"KFL","action":"hello","port":8848}"#).unwrap();
        assert!(m.is_valid_hello());
        assert_eq!(m.effective_port(), 8848);
    }

    #[test]
    fn invalid_hello() {
        let m: BroadcastMsg =
            serde_json::from_str(r#"{"magic":"XXX","action":"hello"}"#).unwrap();
        assert!(!m.is_valid_hello());
    }

    #[test]
    fn parse_ping_body() {
        let data = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true,\"device\":\"小米\"}".as_bytes();
        assert!(response_is_ok(data));
        assert_eq!(parse_device_name(data).as_deref(), Some("小米"));
    }

    #[test]
    fn ping_req_format() {
        let r = ping_request("192.168.1.5");
        assert_eq!(
            String::from_utf8(r).unwrap(),
            "GET /ping HTTP/1.0\r\nHost: 192.168.1.5\r\n\r\n"
        );
    }
}
