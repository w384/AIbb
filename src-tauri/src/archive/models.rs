use serde::{Deserialize, Serialize};

/// Reserved fallback category name (规则无法判断时兜底).
pub const RESERVED_OTHER: &str = "其他";
/// Fixed backup folder name inside a project.
pub const SOURCE_FOLDER: &str = "源文件";
/// Reserved categories that cannot be deleted.
pub const RESERVED_CATEGORIES: [&str; 2] = [RESERVED_OTHER, SOURCE_FOLDER];
/// Hierarchy keyword for the week period directory segment.
pub const HIERARCHY_WEEK: &str = "week";
/// Hierarchy keyword for the classified category directory segment.
pub const HIERARCHY_CATEGORY: &str = "category";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRule {
    pub name: String,
    pub keywords: Vec<String>,
}

impl CategoryRule {
    pub fn new(name: impl Into<String>, keywords: Vec<String>) -> Self {
        Self {
            name: name.into(),
            keywords,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StructureTemplate {
    pub name: String,
    pub categories: Vec<CategoryRule>,
    pub hierarchy: Vec<String>,
    pub include_source: bool,
}

impl StructureTemplate {
    pub fn default_template() -> Self {
        fn kw(values: &[&str]) -> Vec<String> {
            values.iter().map(|value| value.to_string()).collect()
        }
        Self {
            name: "默认".to_string(),
            categories: vec![
                CategoryRule::new("方案", kw(&["方案", "proposal", "plan"])),
                CategoryRule::new("合同", kw(&["合同", "contract", "agreement"])),
                CategoryRule::new("会议纪要", kw(&["纪要", "minutes", "meeting"])),
                CategoryRule::new("报价", kw(&["报价", "quote", "quotation"])),
                CategoryRule::new("需求", kw(&["需求", "requirement", "prd"])),
                CategoryRule::new("设计", kw(&["设计", "design"])),
                CategoryRule::new("验收", kw(&["验收", "acceptance"])),
                CategoryRule::new(RESERVED_OTHER, Vec::new()),
            ],
            hierarchy: vec![HIERARCHY_WEEK.to_string(), HIERARCHY_CATEGORY.to_string()],
            include_source: true,
        }
    }

    /// Pick the named template from a library, falling back to the default.
    pub fn from_library(templates: &[StructureTemplate], name: &str) -> Self {
        templates
            .iter()
            .find(|template| template.name == name)
            .cloned()
            .unwrap_or_else(Self::default_template)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSettingsView {
    pub root: String,
    pub auto_discover: bool,
    pub template_name: String,
    pub templates: Vec<StructureTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveArchiveSettings {
    pub root: String,
    pub auto_discover: bool,
    pub template_name: String,
    pub templates: Vec<StructureTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredStructure {
    pub hierarchy: Vec<String>,
    pub categories: Vec<CategoryRule>,
    pub detected_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveFileResult {
    pub file_name: String,
    pub ok: bool,
    pub duplicate: bool,
    pub reason: Option<String>,
    pub project: Option<String>,
    pub category: Option<String>,
    pub period: Option<String>,
    pub version: Option<String>,
    pub archive_rel: Option<String>,
    pub archive_abs: Option<String>,
    pub backup_rel: Option<String>,
}

impl ArchiveFileResult {
    pub fn failure(file_name: &str, reason: impl Into<String>) -> Self {
        Self {
            file_name: file_name.to_string(),
            ok: false,
            duplicate: false,
            reason: Some(reason.into()),
            project: None,
            category: None,
            period: None,
            version: None,
            archive_rel: None,
            archive_abs: None,
            backup_rel: None,
        }
    }

    pub fn duplicate(file_name: &str, reason: impl Into<String>, existing_path: impl Into<String>) -> Self {
        Self {
            file_name: file_name.to_string(),
            ok: false,
            duplicate: true,
            reason: Some(reason.into()),
            project: None,
            category: None,
            period: None,
            version: None,
            archive_rel: Some(existing_path.into()),
            archive_abs: None,
            backup_rel: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveLedgerEntry {
    pub id: String,
    pub file_name: String,
    pub project: String,
    pub category: String,
    pub period: String,
    pub version: String,
    pub archive_rel_path: String,
    pub backup_rel_path: Option<String>,
    pub status: String,
    pub error_code: Option<String>,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_contains_reserved_folders_and_ordered_keywords() {
        let template = StructureTemplate::default_template();

        assert_eq!(template.hierarchy, vec!["week", "category"]);
        assert!(template.include_source);
        let names = template
            .categories
            .iter()
            .map(|category| category.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.last(), Some(&RESERVED_OTHER));
        let proposal = template
            .categories
            .iter()
            .find(|category| category.name == "方案")
            .expect("方案 category must exist");
        assert!(proposal.keywords.contains(&"方案".to_string()));
        assert!(proposal.keywords.contains(&"proposal".to_string()));
    }

    #[test]
    fn reserved_categories_cannot_be_deleted() {
        assert!(RESERVED_CATEGORIES.contains(&"其他"));
        assert!(RESERVED_CATEGORIES.contains(&"源文件"));
    }
}
