//! Pure archive rules: week period, keyword classification and versioning.
//! No file-system access, so every rule is unit-testable without a temp dir.

use chrono::Datelike;
use regex::Regex;
use std::sync::OnceLock;

use crate::archive::models::{CategoryRule, RESERVED_OTHER};

const VERSION_PATTERN: &str = r"v(\d+(?:\.\d+)*)";
const DEFAULT_VERSION: &str = "v0.1";

fn version_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(VERSION_PATTERN).expect("static version pattern must compile"))
}

fn trailing_version_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)v\d+(?:\.\d+)*\s*$").expect("static trailing version pattern must compile")
    })
}

/// Fixed 7-day month bucket: days 1-7 -> week 1, 8-14 -> week 2, 15-21 -> week 3,
/// 22-28 -> week 4, 29+ -> week 5. Returns the stable period key and display name.
pub fn month_bucket_week(year: i32, month: u32, day: u32) -> (String, String) {
    let week = match day {
        1..=7 => 1,
        8..=14 => 2,
        15..=21 => 3,
        22..=28 => 4,
        _ => 5,
    };
    let key = format!("{year:04}-{month:02}-P{week}");
    let display_name = format!("{year:04}{month:02}w{week}");
    (key, display_name)
}

/// Current week period as display-name and key using the local clock.
pub fn current_week() -> (String, String) {
    let now = chrono::Local::now().naive_local();
    month_bucket_week(now.year(), now.month(), now.day())
}

/// Classify a filename against ordered category rules. A non-`其他` category
/// with no explicit keywords matches its own name as the keyword; `其他` never
/// matches by keyword and is the final fallback. First keyword hit wins.
pub fn classify(filename: &str, categories: &[CategoryRule]) -> String {
    let lowered = filename.to_lowercase();
    for rule in categories {
        if rule.name == RESERVED_OTHER {
            continue;
        }
        let mut keywords = rule.keywords.clone();
        if keywords.is_empty() {
            keywords.push(rule.name.clone());
        }
        if keywords
            .iter()
            .any(|keyword| !keyword.is_empty() && lowered.contains(&keyword.to_lowercase()))
        {
            return rule.name.clone();
        }
    }
    RESERVED_OTHER.to_string()
}

/// Extract the first version marker from a filename, e.g. `XXX方案v0.1.pdf` -> `v0.1`.
/// Files without a marker default to `v0.1`.
pub fn extract_version(filename: &str) -> String {
    match version_regex().find(filename) {
        Some(matched) => format!("v{}", matched.as_str().trim_start_matches(['v', 'V'])),
        None => DEFAULT_VERSION.to_string(),
    }
}

/// Remove the trailing extension and any trailing version marker to get the
/// standard name, e.g. `XXX方案v0.1.pdf` -> `XXX方案`.
pub fn strip_version_and_ext(filename: &str) -> String {
    let stem = match filename.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => filename,
    };
    trailing_version_regex()
        .replace_all(stem, "")
        .trim()
        .to_string()
}

/// Simple version increment: `v0.1` -> `v0.2` -> `v0.3` (major stays, minor bumps).
pub fn bump_version(version: &str) -> String {
    let digits = version.trim_start_matches(['v', 'V']);
    let mut parts = digits.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    format!("v{major}.{}", minor + 1)
}

/// Whether a directory name looks like a period/week segment (either the
/// `YYYYMMwN` generator format or human `X月第N周` / `YYYY-MM` / `YYYY年M月` layouts).
pub fn looks_like_week(name: &str) -> bool {
    let compact = Regex::new(r"^\d{4}\d{2}w[1-5]$").expect("static pattern compiles");
    let human = Regex::new(r"^\d{1,2}月第[一二三四五]周$").expect("static pattern compiles");
    let dash = Regex::new(r"^\d{4}-\d{2}$").expect("static pattern compiles");
    let year_month = Regex::new(r"^\d{4}年\d{1,2}月$").expect("static pattern compiles");
    compact.is_match(name)
        || human.is_match(name)
        || dash.is_match(name)
        || year_month.is_match(name)
}

/// Validate a user-supplied project name: must be non-empty, contain no path
/// separators or traversal segments, and stay within a sane length.
pub fn valid_project(project: &str) -> bool {
    let trimmed = project.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 80
        && !trimmed.contains(['/', '\\'])
        && trimmed != "."
        && trimmed != ".."
        && !trimmed.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> Vec<CategoryRule> {
        vec![
            CategoryRule::new("方案", vec!["方案".into(), "proposal".into()]),
            CategoryRule::new("合同", vec!["合同".into()]),
            CategoryRule::new(RESERVED_OTHER, vec![]),
        ]
    }

    #[test]
    fn classifies_by_first_keyword_hit_case_insensitively() {
        let categories = template();
        assert_eq!(classify("XX方案v0.1.pdf", &categories), "方案");
        assert_eq!(classify("PROPOSAL.pdf", &categories), "方案");
        assert_eq!(classify("采购合同.docx", &categories), "合同");
    }

    #[test]
    fn empty_keyword_category_matches_its_own_name_but_other_never_matches() {
        let categories = vec![
            CategoryRule::new("资料", vec![]),
            CategoryRule::new(RESERVED_OTHER, vec![]),
        ];
        assert_eq!(classify("项目资料.docx", &categories), "资料");
        assert_eq!(classify("随便一个文件.txt", &categories), RESERVED_OTHER);
    }

    #[test]
    fn version_extraction_and_stripping_roundtrip() {
        assert_eq!(extract_version("XXX方案v0.1.pdf"), "v0.1");
        assert_eq!(extract_version("no version.pdf"), "v0.1");
        assert_eq!(extract_version("v1.2.3 版本.docx"), "v1.2.3");
        assert_eq!(strip_version_and_ext("XXX方案v0.1.pdf"), "XXX方案");
        assert_eq!(strip_version_and_ext("plain.txt"), "plain");
    }

    #[test]
    fn bump_version_increments_minor_only() {
        assert_eq!(bump_version("v0.1"), "v0.2");
        assert_eq!(bump_version("v0.2"), "v0.3");
        assert_eq!(bump_version("v1.9"), "v1.10");
        assert_eq!(bump_version("v3"), "v3.1");
    }

    #[test]
    fn month_bucket_boundaries_match_the_reference() {
        assert_eq!(month_bucket_week(2026, 8, 1).1, "202608w1");
        assert_eq!(month_bucket_week(2026, 8, 7).1, "202608w1");
        assert_eq!(month_bucket_week(2026, 8, 8).1, "202608w2");
        assert_eq!(month_bucket_week(2026, 8, 21).1, "202608w3");
        assert_eq!(month_bucket_week(2026, 8, 29).1, "202608w5");
        assert_eq!(month_bucket_week(2026, 8, 31).1, "202608w5");
    }

    #[test]
    fn week_detection_accepts_generator_and_human_formats() {
        assert!(looks_like_week("202608w5"));
        assert!(looks_like_week("8月第四周"));
        assert!(looks_like_week("2026-08"));
        assert!(!looks_like_week("方案"));
        assert!(!looks_like_week("源文件"));
    }

    #[test]
    fn project_name_validation_rejects_traversal() {
        assert!(valid_project("0828"));
        assert!(valid_project("A 项目 2026"));
        assert!(!valid_project(""));
        assert!(!valid_project("../outside"));
        assert!(!valid_project("a/b"));
        assert!(!valid_project("a\\b"));
        assert!(!valid_project(".."));
    }
}
