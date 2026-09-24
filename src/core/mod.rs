//! 核心纯逻辑：剧名解析、随机姓名、剪贴板过滤。
//! 不依赖 Windows API，可单元测试。

pub mod net;
pub mod title;

/// 抖音分享文本提示词（与原版 DOUYIN_HINTS 一致）
pub const DOUYIN_HINTS: &[&str] = &[
    "v.douyin.com",
    "douyin.com",
    "iesdouyin.com",
    "复制打开抖音",
    "复制此链接，打开dou音",
    "复制此链接，打开抖音",
    "打开dou音搜索",
    "打开抖音搜索",
    "抖音搜索",
    "dou音搜索",
];

/// 判断文本是否像抖音分享内容
pub fn looks_like_douyin_share(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let s = text.to_lowercase();
    DOUYIN_HINTS.iter().any(|h| s.contains(&h.to_lowercase()))
}

/// 判断剪贴板内容是否为“噪声”（文件路径等），与原版 is_noise_clipboard 对齐。
pub fn is_noise_clipboard(text: &str) -> bool {
    let s = text.trim();
    if s.is_empty() {
        return true;
    }
    let lower = s.to_lowercase();
    if lower.starts_with("file:") {
        return true;
    }
    // 盘符路径，如 C:\ 或 C:/
    let bytes: Vec<char> = s.chars().collect();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == ':'
        && (bytes[2] == '\\' || bytes[2] == '/')
    {
        return true;
    }
    if s.starts_with("\\\\") {
        return true;
    }
    if s.starts_with('/') && !s.starts_with("//") {
        return true;
    }
    // 无空格无换行且以文件扩展名结尾
    if !s.contains(' ') && !s.contains('\n') && has_file_ext(s) {
        return true;
    }
    false
}

const FILE_EXTS: &[&str] = &[
    "apk","exe","zip","rar","7z","tar","gz","bz2","xz",
    "png","jpg","jpeg","gif","bmp","webp","svg","ico",
    "mp4","avi","mov","mkv","flv","wmv","webm",
    "mp3","wav","flac","aac","ogg","m4a",
    "txt","doc","docx","xls","xlsx","ppt","pptx","pdf",
    "py","js","ts","java","cpp","c","h","hpp","cs","go","rs","rb","php",
    "json","xml","yaml","yml","toml","ini","cfg","conf","log","md",
    "html","htm","css","scss","less","sql","sh","bat","ps1",
];

fn has_file_ext(s: &str) -> bool {
    match s.rfind('.') {
        Some(i) => {
            let ext = s[i + 1..].to_ascii_lowercase();
            FILE_EXTS.contains(&ext.as_str())
        }
        None => false,
    }
}

// ============================================================
// 随机中文姓名
// ============================================================

const SURNAME_POOL: &str = "王王王王王王王李李李李李李李张张张张张张张刘刘刘刘刘陈陈陈陈陈杨杨杨黄黄黄赵赵吴吴周周徐徐孙孙马马朱朱胡胡郭郭何何高高林林罗罗郑梁谢宋唐许韩冯邓曹彭曾肖田董袁潘于蒋蔡余杜叶程苏魏吕丁任沈姚卢姜崔钟谭陆汪范金石廖贾夏韦傅方白邹孟熊秦邱江尹薛闫段雷侯龙史陶黎贺顾毛郝龚邵";

fn male_names() -> &'static [&'static str] {
    &[
        "宇轩","浩宇","子轩","浩然","俊杰","宇航","沐辰","奕辰","子墨","泽宇",
        "一鸣","天佑","明轩","睿轩","昱辰","昊然","承泽","思远","梓豪","睿泽",
        "俊熙","铭泽","皓轩","星辰","锦程","亦辰","亦泽","书豪","柏宇","博文",
        "梓轩","昊宇","嘉豪","子豪","俊宇","逸辰","泽楷","予安","予泽","景行",
        "致远","锦泽","沐阳","宇宸","瑞霖","泽睿","皓宇","思齐",
    ]
}

fn female_names() -> &'static [&'static str] {
    &[
        "欣怡","梓涵","诗涵","雨桐","语汐","若曦","可馨","思彤","嘉怡","梦琪",
        "紫萱","依诺","一诺","芷晴","悦涵","语桐","诗琪","晨曦","若彤","梦瑶",
        "佳怡","雨欣","诗蕊","语嫣","晓彤","语晨","恬欣","依涵","梓萱","若涵",
        "汐月","悦昕","语诺","沐妍","书瑶","婉清","楚涵","思妍","瑾萱","语乔",
        "思琪","可欣","雅涵","雨萱","诗妍","语昕","念安","知微",
    ]
}

const SINGLE_GIVEN: &[&str] = &[
    "伟","芳","娜","敏","静","丽","强","磊","军","洋","勇","艳","杰","娟","涛",
    "明","超","霞","平","刚","华","文","玉","建","国","志","海","峰","鹏","浩",
    "宇","轩","涵","欣","怡","佳","琪","诺","宸","泽",
];

/// 生成一个随机中文姓名（用简单的 xorshift 伪随机，避免引第三方 rng）。
pub fn generate_name() -> String {
    let mut rng = SimpleRng::from_time();
    let surnames: Vec<char> = SURNAME_POOL.chars().collect();
    let surname = surnames[rng.next_usize(surnames.len())];

    let given = if rng.next_f64() < 0.85 {
        let pool = if rng.next_f64() < 0.5 { male_names() } else { female_names() };
        pool[rng.next_usize(pool.len())]
    } else {
        SINGLE_GIVEN[rng.next_usize(SINGLE_GIVEN.len())]
    };
    format!("{}{}", surname, given)
}

/// 简单可复现的伪随机数生成器（xorshift64*）
pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    pub fn from_time() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            ^ (std::process::id() as u64).wrapping_mul(0x2545F4914F6CDD1D);
        SimpleRng { state: seed | 1 }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_usize(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_detects_paths() {
        assert!(is_noise_clipboard("C:\\Users\\a.txt"));
        assert!(is_noise_clipboard("\\\\server\\share"));
        assert!(is_noise_clipboard("file:///x"));
        assert!(is_noise_clipboard("photo.png"));
        assert!(!is_noise_clipboard("复制打开抖音，看看这个剧"));
    }

    #[test]
    fn douyin_detection() {
        assert!(looks_like_douyin_share("复制此链接，打开抖音搜索"));
        assert!(!looks_like_douyin_share("普通文本"));
    }

    #[test]
    fn name_generation_nonempty() {
        for _ in 0..20 {
            let n = generate_name();
            assert!(!n.is_empty());
        }
    }
}
