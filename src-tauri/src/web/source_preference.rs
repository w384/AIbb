//! Chinese-source preference for public web exploration.
//!
//! The user is located in China and wants AIbb's explorations to prefer
//! Chinese pages (science forums, frontier news, and similar), excluding
//! foreign/overseas content unless the user explicitly asks for it. This
//! module provides:
//! - [`explicitly_wants_foreign`]: whether the user's current input signals
//!   an explicit request for foreign/overseas/English content (the escape
//!   hatch),
//! - [`order_for_chinese`]: reorder search results so Chinese sources come
//!   first and known-foreign hosts are dropped, applied only when the
//!   preference is active.

use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceRegion {
    Chinese,
    Foreign,
    Neutral,
}

/// Well-known Chinese sites, biased toward science forums and frontier news
/// per the user's stated direction.
const CHINESE_SITES: &[&str] = &[
    // 科学论坛 / 前沿资讯
    "sciencenet.cn",
    "muchong.com",
    "zhihu.com",
    "guokr.com",
    "jiqizhixin.com",
    "qbitai.com",
    "cnbeta.com",
    "ithome.com",
    "36kr.com",
    "huxiu.com",
    "jiemian.com",
    "thepaper.cn",
    "leiphone.com",
    "ifanr.com",
    "sspai.com",
    "solidot.org",
    // 门户 / 机构
    "sina.com.cn",
    "163.com",
    "qq.com",
    "sohu.com",
    "people.com.cn",
    "xinhuanet.com",
    "cctv.com",
    "cas.cn",
    "baidu.com",
    "weibo.com",
    "douban.com",
    "bilibili.com",
    "cnki.net",
    "wanfangdata.com.cn",
    "nsfc.gov.cn",
];

/// Well-known foreign/overseas hosts that are dropped when the preference is
/// active.
const FOREIGN_SITES: &[&str] = &[
    "google.com",
    "googleusercontent.com",
    "youtube.com",
    "x.com",
    "twitter.com",
    "facebook.com",
    "instagram.com",
    "reddit.com",
    "wikipedia.org",
    "wikimedia.org",
    "github.com",
    "github.io",
    "medium.com",
    "substack.com",
    "bloomberg.com",
    "reuters.com",
    "nytimes.com",
    "bbc.com",
    "bbc.co.uk",
    "cnn.com",
    "theguardian.com",
    "theverge.com",
    "wired.com",
    "arstechnica.com",
    "techcrunch.com",
    "nature.com",
    "sciencedaily.com",
    "phys.org",
    "livescience.com",
    "space.com",
    "stackoverflow.com",
    "quora.com",
    "linkedin.com",
    "pinterest.com",
    "tiktok.com",
    "amazon.com",
    "ebay.com",
    "forbes.com",
    "wsj.com",
    "ft.com",
];

const FOREIGN_KEYWORDS: &[&str] = &[
    "外网",
    "国外",
    "海外",
    "境外",
    "外媒",
    "外文",
    "英文",
    "英语",
    "外语",
    "外国",
    "国际",
    "国际版",
    "reddit",
    "youtube",
    "google",
    "twitter",
    "facebook",
    "instagram",
    "wikipedia",
    "github",
    "hacker news",
    "hackernews",
    "medium",
    "substack",
    "foreign",
    "english",
    "overseas",
    "international",
];

/// True when the user's current input explicitly asks for foreign/overseas or
/// English content, which disables the Chinese-source preference for that run.
pub fn explicitly_wants_foreign(input: &str) -> bool {
    let lower = input.to_lowercase();
    FOREIGN_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(keyword))
}

/// Reorder search results for the Chinese-source preference. When
/// `prefer_chinese` is false the list is returned unchanged. When true,
/// Chinese sources come first (preserving relative order), neutral hosts
/// follow, and known-foreign hosts are dropped.
pub fn order_for_chinese(urls: Vec<String>, prefer_chinese: bool) -> Vec<String> {
    if !prefer_chinese {
        return urls;
    }
    let mut chinese = Vec::new();
    let mut neutral = Vec::new();
    for url in urls {
        match source_region(&url) {
            SourceRegion::Chinese => chinese.push(url),
            SourceRegion::Neutral => neutral.push(url),
            SourceRegion::Foreign => {}
        }
    }
    chinese.extend(neutral);
    chinese
}

fn source_region(url: &str) -> SourceRegion {
    let Ok(parsed) = Url::parse(url) else {
        return SourceRegion::Neutral;
    };
    let Some(host) = parsed.host_str() else {
        return SourceRegion::Neutral;
    };
    let host = host.to_ascii_lowercase();

    if CHINESE_SITES
        .iter()
        .any(|site| host == *site || host.ends_with(&format!(".{site}")))
    {
        return SourceRegion::Chinese;
    }
    if FOREIGN_SITES
        .iter()
        .any(|site| host == *site || host.ends_with(&format!(".{site}")))
    {
        return SourceRegion::Foreign;
    }
    if host.ends_with(".cn") || host.ends_with(".中国") {
        return SourceRegion::Chinese;
    }
    SourceRegion::Neutral
}

#[cfg(test)]
mod tests {
    use super::{explicitly_wants_foreign, order_for_chinese, source_region, SourceRegion};

    #[test]
    fn classifies_chinese_hosts_by_site_and_tld() {
        assert_eq!(
            source_region("https://www.zhihu.com/question/1"),
            SourceRegion::Chinese
        );
        assert_eq!(
            source_region("https://www.solidot.org/story/1"),
            SourceRegion::Chinese
        );
        assert_eq!(
            source_region("https://news.sciencenet.cn/1"),
            SourceRegion::Chinese
        );
        assert_eq!(
            source_region("https://www.edu.cn/1"),
            SourceRegion::Chinese
        );
        assert_eq!(
            source_region("https://www.made-up-site.example.cn/1"),
            SourceRegion::Chinese
        );
    }

    #[test]
    fn classifies_foreign_hosts_and_neutral_unknowns() {
        assert_eq!(
            source_region("https://en.wikipedia.org/wiki/AIbb"),
            SourceRegion::Foreign
        );
        assert_eq!(
            source_region("https://www.reddit.com/r/ai/"),
            SourceRegion::Foreign
        );
        assert_eq!(
            source_region("https://unknown-example.com/a"),
            SourceRegion::Neutral
        );
    }

    #[test]
    fn chinese_ordering_keeps_chinese_first_then_neutral_and_drops_foreign() {
        let urls = vec![
            "https://en.wikipedia.org/wiki/AIbb".to_string(),
            "https://unknown-example.com/a".to_string(),
            "https://www.zhihu.com/question/1".to_string(),
            "https://www.reddit.com/r/ai/".to_string(),
        ];

        let ordered = order_for_chinese(urls.clone(), true);

        assert_eq!(
            ordered,
            vec![
                "https://www.zhihu.com/question/1".to_string(),
                "https://unknown-example.com/a".to_string(),
            ]
        );
    }

    #[test]
    fn disabled_preference_returns_the_original_list() {
        let urls = vec![
            "https://en.wikipedia.org/wiki/AIbb".to_string(),
            "https://www.zhihu.com/question/1".to_string(),
        ];

        assert_eq!(order_for_chinese(urls.clone(), false), urls);
    }

    #[test]
    fn chinese_only_direction_does_not_request_foreign_content() {
        assert!(!explicitly_wants_foreign("去玩"));
        assert!(!explicitly_wants_foreign("看看国内的科学新闻"));
    }

    #[test]
    fn explicit_foreign_requests_are_detected() {
        for input in [
            "帮我看下英文的最新论文",
            "查一下国外的前沿资讯",
            "去 Reddit 看看讨论",
            "找 YouTube 上的科普视频",
            "搜下海外科学论坛",
        ] {
            assert!(
                explicitly_wants_foreign(input),
                "should detect foreign request: {input}"
            );
        }
    }
}
