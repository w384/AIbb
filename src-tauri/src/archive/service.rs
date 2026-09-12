use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    archive::{
        engine::ArchiveEngine,
        models::{
            ArchiveFileResult, ArchiveLedgerEntry, ArchiveSettingsView, DiscoveredStructure,
            SaveArchiveSettings, StructureTemplate,
        },
        structure_lib::discover_latest_structure,
    },
    error::AppError,
    storage::Database,
};

/// ArchiveService owns the archive configuration, the season-structure library,
/// the pending drop hand-off between the pet window and the chat window, and the
/// SQLite ledger. The actual file work is delegated to a per-file
/// `ArchiveEngine` inside a blocking task.
#[derive(Clone)]
pub struct ArchiveService {
    database: Database,
    app_data_dir: Arc<PathBuf>,
    operation: Arc<AsyncMutex<()>>,
    pending_paths: Arc<AsyncMutex<Vec<String>>>,
}

impl ArchiveService {
    pub fn new(database: Database, app_data_dir: impl Into<PathBuf>) -> Self {
        Self {
            database,
            app_data_dir: Arc::new(app_data_dir.into()),
            operation: Arc::new(AsyncMutex::new(())),
            pending_paths: Arc::new(AsyncMutex::new(Vec::new())),
        }
    }

    /// Default archive root when the user has not configured one.
    pub fn default_root(&self) -> PathBuf {
        self.app_data_dir.join("本地归档")
    }

    async fn resolved_root(&self) -> Result<PathBuf, AppError> {
        let (root, _) = self.database.load_archive_settings()?;
        if root.trim().is_empty() {
            Ok(self.default_root())
        } else {
            Ok(PathBuf::from(root))
        }
    }

    async fn load_structure_library(&self) -> Result<(String, Vec<StructureTemplate>), AppError> {
        let Some((name, json)) = self.database.load_archive_structure_lib()? else {
            return Ok(("默认".to_string(), vec![StructureTemplate::default_template()]));
        };
        let templates = serde_json::from_str::<Vec<StructureTemplate>>(&json)
            .unwrap_or_else(|_| vec![StructureTemplate::default_template()]);
        let name = if templates.iter().any(|template| template.name == name) {
            name
        } else {
            "默认".to_string()
        };
        Ok((name, templates))
    }

    pub async fn load_settings(&self) -> Result<ArchiveSettingsView, AppError> {
        let (root, auto_discover) = self.database.load_archive_settings()?;
        let root = if root.trim().is_empty() {
            self.default_root().display().to_string()
        } else {
            root
        };
        let (template_name, templates) = self.load_structure_library().await?;
        Ok(ArchiveSettingsView {
            root,
            auto_discover,
            template_name,
            templates,
        })
    }

    pub async fn save_settings(&self, settings: SaveArchiveSettings) -> Result<(), AppError> {
        if settings.templates.is_empty() {
            return Err(AppError::new(
                "invalidArchiveSettings",
                "结构库至少需要一个模板。",
            ));
        }
        let root = settings.root.trim().to_string();
        if !root_usable(&root) {
            return Err(AppError::new(
                "invalidArchiveSettings",
                "归档根目录不可用，请检查路径是否有效、是否可写。",
            ));
        }
        let templates_json = serde_json::to_string(&settings.templates).map_err(|_| {
            AppError::new("invalidArchiveSettings", "结构库格式无效。")
        })?;
        let template_name = if settings
            .templates
            .iter()
            .any(|template| template.name == settings.template_name)
        {
            settings.template_name.clone()
        } else {
            settings.templates[0].name.clone()
        };
        self.database
            .save_archive_structure_lib(&template_name, &templates_json)?;
        self.database
            .save_archive_settings(&root, settings.auto_discover)?;
        Ok(())
    }

    pub async fn discover_structure(&self) -> Result<Option<DiscoveredStructure>, AppError> {
        let root = self.resolved_root().await?;
        Ok(discover_latest_structure(&root.join("归档区"), None))
    }

    pub async fn archive_files(
        &self,
        paths: Vec<String>,
        project: String,
    ) -> Result<Vec<ArchiveFileResult>, AppError> {
        let _operation = self.operation.lock().await;
        let root = self.resolved_root().await?;
        let (template_name, templates) = self.load_structure_library().await?;
        let (_, auto_discover) = self.database.load_archive_settings()?;
        let template = StructureTemplate::from_library(&templates, &template_name);

        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            let engine = ArchiveEngine::new(root.clone(), template.clone(), auto_discover);
            let source = path.clone();
            let project = project.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                engine.archive_file(Path::new(&source), &project)
            })
            .await
            .map_err(|_| {
                AppError::new("archiveFailed", "归档任务执行失败，请稍后重试。")
            })?;
            if result.ok {
                self.record_ledger(&result)?;
            }
            results.push(result);
        }
        Ok(results)
    }

    fn record_ledger(&self, result: &ArchiveFileResult) -> Result<(), AppError> {
        let entry = ArchiveLedgerEntry {
            id: Uuid::new_v4().to_string(),
            file_name: result.file_name.clone(),
            project: result.project.clone().unwrap_or_default(),
            category: result.category.clone().unwrap_or_default(),
            period: result.period.clone().unwrap_or_default(),
            version: result.version.clone().unwrap_or_default(),
            archive_rel_path: result.archive_rel.clone().unwrap_or_default(),
            backup_rel_path: result.backup_rel.clone(),
            status: "completed".to_string(),
            error_code: None,
            created_at: unix_millis(),
        };
        self.database.insert_archive_ledger(&entry)
    }

    pub async fn ledger(&self, limit: usize) -> Result<Vec<ArchiveLedgerEntry>, AppError> {
        let limit = limit.clamp(1, 200);
        self.database.list_archive_ledger(limit)
    }

    pub async fn set_pending_paths(&self, paths: Vec<String>) {
        *self.pending_paths.lock().await = paths;
    }

    pub async fn take_pending_paths(&self) -> Vec<String> {
        let mut pending = self.pending_paths.lock().await;
        std::mem::take(&mut *pending)
    }
}

/// Whether the configured archive root is usable: the Windows drive must exist
/// and the directory must be creatable/writable.
fn root_usable(root: &str) -> bool {
    if root.trim().is_empty() {
        return false;
    }
    let path = Path::new(root);
    #[cfg(windows)]
    {
        use std::path::Component;
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            let drive_root = PathBuf::from(format!("{}\\", prefix.as_os_str().to_string_lossy()));
            if !drive_root.exists() {
                return false;
            }
        }
    }
    match fs::create_dir_all(path) {
        Ok(()) => path.is_dir(),
        Err(_) => false,
    }
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_usable_rejects_empty_and_accepts_a_writable_dir() {
        assert!(!root_usable(""));
        assert!(!root_usable("   "));
        let directory = tempfile::tempdir().unwrap();
        assert!(root_usable(directory.path().to_str().unwrap()));
    }
}
