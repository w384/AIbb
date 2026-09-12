//! Structure discovery: scan the archive area and derive a template from the
//! most recently touched project layout. Faithful port of the reference
//! `local_drop/structure_lib.discover_latest_structure`.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::archive::{
    models::{
        CategoryRule, DiscoveredStructure, HIERARCHY_CATEGORY, HIERARCHY_WEEK, RESERVED_OTHER,
        SOURCE_FOLDER,
    },
    rules,
};

const SKIP_DIRS: [&str; 6] = [SOURCE_FOLDER, "上传区", "__pycache__", ".git", ".gitkeep", "harness.db"];

fn is_skip(name: &str) -> bool {
    SKIP_DIRS.contains(&name) || name.starts_with('.') || name.ends_with(".db")
}

/// "Newest" metric for a directory: the largest modification time among the
/// directory itself and every nested directory.
fn dir_mtime(path: &Path) -> u64 {
    let mut latest = 0u64;
    fn collect(path: &Path, latest: &mut u64) {
        let mtime = fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        *latest = (*latest).max(mtime);
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    collect(&entry.path(), latest);
                }
            }
        }
    }
    collect(path, &mut latest);
    latest
}

fn subdirs(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .map(|name| !is_skip(&name.to_string_lossy()))
                .unwrap_or(false)
        })
        .collect()
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Extract candidate keywords from the file names inside a directory:
/// strip the extension and trailing version, then take the first non-digit
/// token of each file name, up to `limit` unique keywords.
fn keywords_from_dir(path: &Path, limit: usize) -> Vec<String> {
    let mut keywords = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return keywords;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        let Some(file_name) = entry_path.file_name().map(|name| name.to_string_lossy().to_string())
        else {
            continue;
        };
        let stem = rules::strip_version_and_ext(&file_name);
        let mut token = None;
        for part in stem.split(|c: char| c == '_' || c == '-' || c.is_whitespace()) {
            if !part.is_empty() && !part.chars().all(|c| c.is_ascii_digit()) {
                token = Some(part.to_string());
                break;
            }
        }
        if let Some(token) = token {
            if !keywords.contains(&token) {
                keywords.push(token);
            }
            if keywords.len() >= limit {
                break;
            }
        }
    }
    keywords
}

fn push_category(categories: &mut Vec<CategoryRule>, name: String, keywords: Vec<String>) {
    if categories.iter().any(|category| category.name == name) {
        return;
    }
    let mut keywords = keywords;
    if keywords.is_empty() {
        keywords.push(name.clone());
    }
    categories.push(CategoryRule::new(name, keywords));
}

/// Derive a template from the newest project under `archive_dir`. Returns
/// `None` when the archive area has no recognizable structure.
pub fn discover_latest_structure(archive_dir: &Path, project: Option<&str>) -> Option<DiscoveredStructure> {
    if !archive_dir.is_dir() {
        return None;
    }

    let mut projects = subdirs(archive_dir);
    if let Some(project) = project {
        let target = archive_dir.join(project);
        if target.is_dir() {
            projects.sort_by_key(|path| path != &target);
        }
    }
    projects.sort_by_key(|path| std::cmp::Reverse(dir_mtime(path)));
    let latest = projects.first()?;
    let latest_name = dir_name(latest);

    let mut level1 = subdirs(latest);
    level1.sort_by_key(|path| std::cmp::Reverse(dir_mtime(path)));
    if level1.is_empty() {
        return None;
    }

    let l1_looks_week = level1
        .iter()
        .all(|dir| rules::looks_like_week(&dir_name(dir)));

    let (categories, hierarchy) = if l1_looks_week {
        // New layout: project/week/category. Categories live one level down.
        let mut categories = Vec::new();
        for week_dir in &level1 {
            for category_dir in subdirs(week_dir) {
                let name = dir_name(&category_dir);
                let keywords = keywords_from_dir(&category_dir, 3);
                push_category(&mut categories, name, keywords);
            }
        }
        let hierarchy = vec![HIERARCHY_WEEK.to_string(), HIERARCHY_CATEGORY.to_string()];
        (categories, hierarchy)
    } else {
        // Legacy layout: project/category(/week). Categories are level 1.
        let mut categories = Vec::new();
        let mut second_levels: Vec<(String, Vec<String>)> = Vec::new();
        for dir in &level1 {
            let name = dir_name(dir);
            let keywords = keywords_from_dir(dir, 3);
            push_category(&mut categories, name.clone(), keywords);
            let subs: Vec<String> = subdirs(dir).iter().map(|path| dir_name(path)).collect();
            if !subs.is_empty() {
                second_levels.push((name, subs));
            }
        }

        let mut hierarchy = vec![HIERARCHY_CATEGORY.to_string()];
        if !second_levels.is_empty() {
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for (_, subs) in &second_levels {
                for sub in subs {
                    *counts.entry(sub.as_str()).or_insert(0) += 1;
                }
            }
            let threshold = second_levels.len() / 2 + 1;
            let mut picked: Option<String> = None;
            let mut ordered: Vec<(&str, usize)> = counts.into_iter().collect();
            ordered.sort_by(|a, b| b.1.cmp(&a.1));
            for (name, count) in ordered {
                if count < threshold {
                    break;
                }
                picked = Some(name.to_string());
                break;
            }
            match picked {
                Some(name) if rules::looks_like_week(&name) => {
                    hierarchy.push(HIERARCHY_WEEK.to_string());
                }
                Some(name) => hierarchy.push(name),
                None => hierarchy.push(HIERARCHY_WEEK.to_string()),
            }
        }
        (categories, hierarchy)
    };

    Some(DiscoveredStructure {
        hierarchy,
        categories,
        detected_from: Some(latest_name),
    })
}

/// Merge discovered categories over the template defaults: template rules
/// keep priority, discovered rules append anything missing, and the reserved
/// `其他` fallback is always present last.
pub fn merge_categories(template: &[CategoryRule], discovered: &[CategoryRule]) -> Vec<CategoryRule> {
    let mut merged: Vec<CategoryRule> = Vec::new();
    for category in template {
        if category.name != RESERVED_OTHER && !merged.iter().any(|c| c.name == category.name) {
            merged.push(category.clone());
        }
    }
    for category in discovered {
        if !merged.iter().any(|c| c.name == category.name) {
            merged.push(category.clone());
        }
    }
    if !merged.iter().any(|c| c.name == RESERVED_OTHER) {
        merged.push(CategoryRule::new(RESERVED_OTHER, Vec::new()));
    }
    merged
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn file_in(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).expect("write test file");
    }

    #[test]
    fn keywords_are_stripped_of_version_and_digits() {
        let dir = tempfile::tempdir().unwrap();
        file_in(dir.path(), "合同v0.1.pdf", "x");
        file_in(dir.path(), "合同v0.2.pdf", "y");
        file_in(dir.path(), "2026 数据.xlsx", "z");

        let mut keywords = keywords_from_dir(dir.path(), 3);
        keywords.sort();
        assert_eq!(keywords, vec!["合同".to_string(), "数据".to_string()]);
    }

    #[test]
    fn discovers_week_then_category_layout() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("0828");
        let week = project.join("202608w5");
        fs::create_dir_all(week.join("方案")).unwrap();
        file_in(&week.join("方案"), "集成方案v0.1.pdf", "a");

        let discovered = discover_latest_structure(root.path(), None).expect("structure found");
        assert_eq!(discovered.hierarchy, vec!["week", "category"]);
        assert!(discovered.categories.iter().any(|c| c.name == "方案"));
        assert_eq!(discovered.detected_from.as_deref(), Some("0828"));
    }

    #[test]
    fn discovers_legacy_category_first_layout_with_week_second_level() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("旧项目");
        fs::create_dir_all(project.join("方案").join("202608w4")).unwrap();
        fs::create_dir_all(project.join("合同").join("202608w4")).unwrap();
        file_in(&project.join("方案").join("202608w4"), "新方案v0.1.docx", "a");

        let discovered = discover_latest_structure(root.path(), None).expect("structure found");
        assert_eq!(discovered.hierarchy, vec!["category", "week"]);
        assert!(discovered.categories.iter().any(|c| c.name == "方案"));
    }

    #[test]
    fn merge_keeps_template_priority_and_appends_reserved_other() {
        let template = vec![
            CategoryRule::new("方案", vec!["方案".into()]),
            CategoryRule::new("其他", vec![]),
        ];
        let discovered = vec![
            CategoryRule::new("方案", vec!["集成".into()]),
            CategoryRule::new("资料", vec!["资料".into()]),
        ];

        let merged = merge_categories(&template, &discovered);
        let names = merged.iter().map(|c| c.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["方案", "资料", "其他"]);
        assert_eq!(merged[0].keywords, vec!["方案".to_string()]);
    }
}
