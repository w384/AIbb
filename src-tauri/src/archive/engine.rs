//! Archive engine: receives a dropped file, builds a deterministic plan
//! (classification, week period, version with conflict detection) and executes
//! it: copy to `上传区`, verify hash, back up the original under `源文件`,
//! and place the versioned file under the project's classified directory.
//!
//! Faithful port of the reference `miniharness` controlled chain, reduced to
//! the single-file sync core the renderer drives per dropped file.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    archive::{
        models::{
            ArchiveFileResult, CategoryRule, DiscoveredStructure, StructureTemplate,
            HIERARCHY_CATEGORY, HIERARCHY_WEEK, RESERVED_OTHER, SOURCE_FOLDER,
        },
        rules,
        structure_lib::{discover_latest_structure, merge_categories},
    },
};

const UPLOAD_FOLDER: &str = "上传区";
const ARCHIVE_FOLDER: &str = "归档区";
const MAX_VERSION_ATTEMPTS: usize = 20;

#[derive(Clone)]
pub struct ArchiveEngine {
    pub root: PathBuf,
    pub template: StructureTemplate,
    pub auto_discover: bool,
}

impl ArchiveEngine {
    pub fn new(root: PathBuf, template: StructureTemplate, auto_discover: bool) -> Self {
        Self {
            root,
            template,
            auto_discover,
        }
    }

    /// Archive one dropped file under `project`. Never panics: every failure
    /// becomes a per-file `ArchiveFileResult`.
    pub fn archive_file(&self, source_path: &Path, project: &str) -> ArchiveFileResult {
        let file_name = match source_path.file_name().and_then(|name| name.to_str()) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => return ArchiveFileResult::failure("<unknown>", "无法读取被拖入的文件名。"),
        };

        if !source_path.is_file() {
            return ArchiveFileResult::failure(&file_name, "拖入的内容不是文件，无法归档。");
        }
        let project = project.trim();
        if !rules::valid_project(project) {
            return ArchiveFileResult::failure(
                &file_name,
                "项目名无效：不能为空、不能包含路径分隔符。",
            );
        }

        let upload_folder = self.root.join(UPLOAD_FOLDER);
        let archive_folder = self.root.join(ARCHIVE_FOLDER);
        if let Err(reason) = ensure_dirs(&[&self.root, &upload_folder, &archive_folder]) {
            return ArchiveFileResult::failure(&file_name, reason);
        }

        // 1. Receive: copy the dropped file into the upload zone.
        let upload_path = upload_folder.join(&file_name);
        if let Err(reason) = copy_file(source_path, &upload_path) {
            return ArchiveFileResult::failure(&file_name, reason);
        }

        // 2. Fingerprint the received copy.
        let sha = match sha256_of(&upload_path) {
            Ok(sha) => sha,
            Err(reason) => {
                let _ = fs::remove_file(&upload_path);
                return ArchiveFileResult::failure(&file_name, reason);
            }
        };

        // 3. Resolve the effective structure (template + optional discovery).
        let (categories, hierarchy, include_source) = self.effective_structure(&archive_folder);

        // 4. Classify and period.
        let category = rules::classify(&file_name, &categories);
        let (_, period) = rules::current_week();

        // 5. Resolve the version with conflict detection against existing files.
        let Some((version, archive_rel, existing)) = self.resolve_version_and_target(
            &archive_folder,
            project,
            &hierarchy,
            &category,
            &period,
            &file_name,
            &sha,
        ) else {
            let _ = fs::remove_file(&upload_path);
            return ArchiveFileResult::failure(
                &file_name,
                "版本号尝试超过上限，请重命名文件后再试。",
            );
        };

        match existing {
            Some(existing_path) => {
                let _ = fs::remove_file(&upload_path);
                return ArchiveFileResult::duplicate(
                    &file_name,
                    "归档区已存在完全相同内容的文件，为避免重复归档已拒绝。",
                    existing_path,
                );
            }
            None => {}
        }

        // 6. Execute: backup original under 源文件, then archive the versioned copy.
        let backup_rel = if include_source {
            Some(format!("{project}/{SOURCE_FOLDER}/{period}/{file_name}"))
        } else {
            None
        };

        let mut executed = Vec::new();
        if let Some(backup_rel) = &backup_rel {
            let backup_path = archive_folder.join(backup_rel);
            match copy_file(&upload_path, &backup_path) {
                Ok(()) => executed.push(backup_path),
                Err(reason) => return self.fail_with_cleanup(&file_name, &upload_path, &executed, reason),
            }
        }

        let archive_path = archive_folder.join(&archive_rel);
        match copy_file(&upload_path, &archive_path) {
            Ok(()) => executed.push(archive_path.clone()),
            Err(reason) => return self.fail_with_cleanup(&file_name, &upload_path, &executed, reason),
        }

        // 7. Verify the archive copy matches the fingerprint.
        match sha256_of(&archive_path) {
            Ok(actual) if actual == sha => {}
            _ => {
                let _ = fs::remove_file(&archive_path);
                let reason = "归档后校验失败，目标文件与源文件不一致，已回滚本次归档。";
                if let Some(backup_rel) = &backup_rel {
                    let _ = fs::remove_file(archive_folder.join(backup_rel));
                }
                let _ = fs::remove_file(&upload_path);
                return ArchiveFileResult::failure(&file_name, reason);
            }
        }

        // 8. Drop the upload temp copy; the original file is untouched.
        let _ = fs::remove_file(&upload_path);

        ArchiveFileResult {
            file_name,
            ok: true,
            duplicate: false,
            reason: None,
            project: Some(project.to_string()),
            category: Some(category),
            period: Some(period),
            version: Some(version),
            archive_rel: Some(archive_rel.clone()),
            archive_abs: Some(archive_path.display().to_string()),
            backup_rel,
        }
    }

    /// Resolve the version + archive relative path. Returns
    /// `Some((version, archive_rel, existing_path))` where `existing_path` is
    /// `Some` when an identical-content file already occupies the target
    /// (duplicate); `None` when the version attempts were exhausted.
    fn resolve_version_and_target(
        &self,
        archive_folder: &Path,
        project: &str,
        hierarchy: &[String],
        category: &str,
        period: &str,
        file_name: &str,
        sha: &str,
    ) -> Option<(String, String, Option<String>)> {
        let mut version = rules::extract_version(file_name);
        let stem = rules::strip_version_and_ext(file_name);
        let extension = match file_name.rsplit_once('.') {
            Some((_, ext)) => format!(".{ext}"),
            None => String::new(),
        };

        for _ in 0..MAX_VERSION_ATTEMPTS {
            let archive_name = format!("{stem}{version}{extension}");
            let archive_rel = build_archive_rel(project, hierarchy, category, period, &archive_name);
            let target = archive_folder.join(&archive_rel);
            if !target.exists() {
                return Some((version, archive_rel, None));
            }
            if target.is_file() && sha256_of(&target).map(|existing| existing == sha).unwrap_or(false) {
                return Some((version, archive_rel.clone(), Some(archive_rel)));
            }
            version = rules::bump_version(&version);
        }

        None
    }

    fn effective_structure(
        &self,
        archive_folder: &Path,
    ) -> (Vec<CategoryRule>, Vec<String>, bool) {
        let template = self.template.clone();
        if !self.auto_discover {
            let categories = normalize_categories(&template.categories);
            return (categories, template.hierarchy, template.include_source);
        }
        let discovered = discover_latest_structure(archive_folder, None);
        match discovered {
            Some(DiscoveredStructure {
                hierarchy,
                categories,
                ..
            }) => (merge_categories(&template.categories, &categories), hierarchy, true),
            None => (
                normalize_categories(&template.categories),
                template.hierarchy,
                template.include_source,
            ),
        }
    }

    fn fail_with_cleanup(
        &self,
        file_name: &str,
        upload_path: &Path,
        executed: &[PathBuf],
        reason: &str,
    ) -> ArchiveFileResult {
        for path in executed {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_file(upload_path);
        ArchiveFileResult::failure(file_name, reason)
    }
}

/// Categories with an empty keyword list (other than the reserved `其他`
/// fallback) match their own name, mirroring the reference normalize step.
fn normalize_categories(categories: &[CategoryRule]) -> Vec<CategoryRule> {
    categories
        .iter()
        .map(|category| {
            if category.name == RESERVED_OTHER || !category.keywords.is_empty() {
                category.clone()
            } else {
                CategoryRule::new(category.name.clone(), vec![category.name.clone()])
            }
        })
        .collect()
}

fn ensure_dirs(dirs: &[&Path]) -> Result<(), &'static str> {
    for dir in dirs {
        fs::create_dir_all(dir).map_err(|_| "无法创建归档目录，请检查归档根目录是否可写。")?;
    }
    Ok(())
}

fn copy_file(source: &Path, target: &Path) -> Result<(), &'static str> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|_| "无法创建归档目录。")?;
    }
    fs::copy(source, target).map(|_| ()).map_err(|_| "文件复制失败。")
}

pub fn sha256_of(path: &Path) -> Result<String, &'static str> {
    let mut file = fs::File::open(path).map_err(|_| "无法读取文件，请确认它仍然存在。")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "读取文件内容失败。")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Build the archive relative path from the hierarchy: each `week`/`category`
/// keyword resolves to the period/category segment, other segments are fixed.
fn build_archive_rel(
    project: &str,
    hierarchy: &[String],
    category: &str,
    period: &str,
    archive_name: &str,
) -> String {
    let mut segments = vec![project.to_string()];
    let hierarchy: &[String] = if hierarchy.is_empty() {
        &[HIERARCHY_WEEK.to_string(), HIERARCHY_CATEGORY.to_string()]
    } else {
        hierarchy
    };
    for segment in hierarchy {
        match segment.as_str() {
            HIERARCHY_WEEK => segments.push(period.to_string()),
            HIERARCHY_CATEGORY => segments.push(category.to_string()),
            fixed => segments.push(fixed.to_string()),
        }
    }
    segments.push(archive_name.to_string());
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::archive::models::StructureTemplate;

    fn engine(root: &Path) -> ArchiveEngine {
        ArchiveEngine::new(
            root.to_path_buf(),
            StructureTemplate::default_template(),
            false,
        )
    }

    fn write_file(path: &Path, content: &str) {
        fs::write(path, content).expect("write test file");
    }

    #[test]
    fn archives_a_file_with_classification_version_and_backup() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("集成方案v0.1.pdf");
        write_file(&source, "plan content");

        let result = engine(root.path()).archive_file(&source, "0828");

        assert!(result.ok, "{:?}", result.reason);
        assert!(!result.duplicate);
        assert_eq!(result.category.as_deref(), Some("方案"));
        assert_eq!(result.version.as_deref(), Some("v0.1"));
        assert!(result.archive_rel.as_deref().unwrap().contains("0828/"));
        assert!(result.archive_rel.as_deref().unwrap().contains("方案"));
        // source untouched
        assert!(source.exists());
        // backup created under 源文件
        let backup = root
            .path()
            .join("归档区")
            .join("0828")
            .join("源文件");
        assert!(backup.exists());
        // upload temp cleaned
        assert!(!root.path().join("上传区").join("集成方案v0.1.pdf").exists());
    }

    #[test]
    fn duplicate_content_is_rejected_but_revision_is_versioned() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("合同v0.1.docx");
        write_file(&source, "same bytes");

        let first = engine(root.path()).archive_file(&source, "A");
        assert!(first.ok);

        // identical bytes -> duplicate
        let dup = engine(root.path()).archive_file(&source, "A");
        assert!(dup.duplicate);
        assert!(!dup.ok);

        // different bytes -> v0.2
        write_file(&source, "different bytes");
        let second = engine(root.path()).archive_file(&source, "A");
        assert!(second.ok);
        assert_eq!(second.version.as_deref(), Some("v0.2"));
    }

    #[test]
    fn invalid_project_or_missing_file_fails_without_panic() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("方案.pdf");
        write_file(&source, "x");

        let bad_project = engine(root.path()).archive_file(&source, "../outside");
        assert!(!bad_project.ok);

        let missing = engine(root.path()).archive_file(&root.path().join("nope.pdf"), "P");
        assert!(!missing.ok);
    }

    #[test]
    fn archive_file_never_touches_the_original() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("资料v0.1.txt");
        write_file(&source, "original");
        let before = fs::read(&source).unwrap();

        let result = engine(root.path()).archive_file(&source, "P");
        assert!(result.ok);
        assert_eq!(fs::read(&source).unwrap(), before);
    }
}
