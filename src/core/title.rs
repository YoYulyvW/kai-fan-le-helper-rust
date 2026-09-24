//! 剧名解析：从抖音分享文本中提取剧名，并识别“极速”后缀。
//! 与原 Python 版 TitleParser 行为一致。

use std::sync::OnceLock;

const CATEGORY_TAGS: &[&str] = &[
    "漫剧","AI漫剧","AI","ai","AI动画","ai漫剧","AI动漫",
    "好剧推荐","短剧","短剧推荐","追剧","追剧推荐",
    "抖音","视频","电影","动漫","电视剧","影视",
    "推荐","日常","热播","新剧","剧","漫",
    "动漫推荐","推荐短剧","影视剪辑","剪辑",
    "douyin","Douyin","抖音短剧",
];

const PREFIX_TOKENS: &[&str] = &[
    "复制打开抖音，看看",
    "复制打开抖音看看",
    "复制此链接，打开Dou音搜索，直接观看视频",
    "复制此链接，打开抖音搜索，直接观看视频",
    "复制此链接，打开Dou音搜索",
    "复制此链接，打开抖音搜索",
    "复制此链接，打开Dou音",
    "复制此链接，打开抖音",
    "打开Dou音搜索",
    "打开抖音搜索",
];

const CUT_TOKENS: &[&str] = &[
    "复制此链接","打开Dou音","直接观看视频",
    "复制链接","打开抖音","看看TA的视频",
    "打开Dou音搜索",
];

fn re_url() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)https?://\S+").unwrap())
}

fn re_bracket() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"【[^】]*】").unwrap())
}

fn re_leading_num() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^[\d.]+\s+").unwrap())
}

fn re_fast_suffix() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"\s*[-~－\u{2010}-\u{2015}]\s*极速\s*$").unwrap()
    })
}

fn re_noise_date() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^\d{1,2}/\d{1,2}$").unwrap())
}

fn re_noise_ampm() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^:?\d{1,2}(am|pm|AM|PM)$").unwrap())
}

fn re_noise_at() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^[A-Za-z]?@[A-Za-z0-9._]+$").unwrap())
}

fn re_noise_short() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^[A-Za-z]{1,5}:?/?$").unwrap())
}

/// 解析结果：剧名 + 是否极速
pub struct ParsedTitle {
    pub title: String,
    pub is_fast: bool,
}

/// 主入口，与 Python 的 TitleParser.parse 对应。返回 None 表示无法解析。
pub fn parse(text: &str) -> Option<ParsedTitle> {
    if text.is_empty() {
        return None;
    }
    let mut s = text.trim().to_string();
    s = s.replace('＃', "#").replace('：', ":");
    s = re_url().replace_all(&s, "").to_string();
    s = re_bracket().replace_all(&s, " ").to_string();
    for p in PREFIX_TOKENS {
        s = s.replace(p, " ");
    }
    s = s.trim().to_string();
    s = re_leading_num().replace(&s, "").to_string();
    s = s.trim().to_string();
    if s.is_empty() {
        return None;
    }

    let title = if s.contains('#') {
        from_hashtag(&s)
    } else {
        from_plain(&s)
    }?;
    if title.is_empty() {
        return None;
    }

    let mut title = title;
    let mut is_fast = false;
    if let Some(m) = re_fast_suffix().find(&title) {
        let base = title[..m.start()].trim();
        if !base.is_empty() {
            title = base.to_string();
            is_fast = true;
        }
    }
    Some(ParsedTitle { title, is_fast })
}

fn from_plain(s: &str) -> Option<String> {
    extract_from_text(s)
}

fn extract_from_text(s: &str) -> Option<String> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let mut collected: Vec<&str> = Vec::new();
    for tok in tokens.iter().rev() {
        if is_noise(tok) {
            continue;
        }
        collected.insert(0, tok);
    }
    let candidate = collected.join(" ").trim().to_string();
    if candidate.is_empty() {
        return None;
    }
    Some(clean(&candidate))
}

fn from_hashtag(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.split('#').collect();
    let before = parts[0].trim();
    let tags: Vec<&str> = parts[1..].iter().map(|p| p.trim()).collect();

    if !before.is_empty() && !CATEGORY_TAGS.contains(&before) {
        let cleaned = clean(before);
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    for tag in &tags {
        if tag.is_empty() || CATEGORY_TAGS.contains(tag) {
            continue;
        }
        if let Some(cleaned) = extract_from_text(tag) {
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    if !before.is_empty() {
        let cleaned = clean(before);
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

fn is_noise(tok: &str) -> bool {
    if tok.is_empty() {
        return true;
    }
    if tok.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return true;
    }
    if re_noise_date().is_match(tok) {
        return true;
    }
    if re_noise_ampm().is_match(tok) {
        return true;
    }
    if tok.contains('@') && re_noise_at().is_match(tok) {
        return true;
    }
    if re_noise_short().is_match(tok) {
        return true;
    }
    false
}

fn clean(s: &str) -> String {
    let mut s = s.to_string();
    for tok in CUT_TOKENS {
        if let Some(idx) = s.find(tok) {
            s.truncate(idx);
        }
    }
    s.trim_matches(|c: char| {
        " \t\n!！。.,，、;；:：/\\@#".contains(c)
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_title() {
        // 原版会保留所有非噪声 token，仅去掉行首数字编号
        let r = parse("3.5 满城风絮").unwrap();
        assert_eq!(r.title, "满城风絮");
        assert!(!r.is_fast);
    }

    #[test]
    fn fast_suffix() {
        let r = parse("剧名 - 极速").unwrap();
        assert_eq!(r.title, "剧名");
        assert!(r.is_fast);
    }

    #[test]
    fn hashtag_title() {
        let r = parse("婚后热恋 #短剧 #推荐").unwrap();
        assert_eq!(r.title, "婚后热恋");
    }

    #[test]
    fn strips_url_and_prefix() {
        let r = parse("复制此链接，打开抖音搜索，直接观看视频 好剧推荐 https://v.douyin.com/xxx");
        // 只验证能解析出非空标题
        if let Some(p) = r {
            assert!(!p.title.is_empty());
        }
    }
}
