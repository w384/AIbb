use std::{fs, path::PathBuf};

use aibb_desktop_pet_lib::{
    archive::{
        models::{CategoryRule, SaveArchiveSettings, StructureTemplate},
        service::ArchiveService,
    },
    storage::Database,
};
use tempfile::TempDir;

struct TestDatabase {
    _directory: TempDir,
    path: PathBuf,
    database: Database,
}

impl TestDatabase {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("archive.sqlite3");
        let database = Database::open(&path).unwrap();
        Self {
            _directory: directory,
            path,
            database,
        }
    }

    fn handle(&self) -> Database {
        self.database.clone()
    }
}

fn service(db: &TestDatabase, app_data_dir: &TempDir) -> ArchiveService {
    ArchiveService::new(db.handle(), app_data_dir.path())
}

fn default_settings(root: &str) -> SaveArchiveSettings {
    SaveArchiveSettings {
        root: root.to_string(),
        auto_discover: true,
        template_name: "默认".to_string(),
        templates: vec![StructureTemplate::default_template()],
    }
}

#[tokio::test]
async fn archive_settings_default_to_local_root_and_default_template() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let service = service(&db, &app_data);

    let settings = service.load_settings().await.unwrap();

    assert_eq!(
        settings.root,
        app_data.path().join("本地归档").display().to_string()
    );
    assert!(settings.auto_discover);
    assert_eq!(settings.template_name, "默认");
    assert_eq!(settings.templates.len(), 1);
    assert_eq!(settings.templates[0].hierarchy, vec!["week", "category"]);
    assert!(settings.templates[0].include_source);
    assert!(settings.templates[0]
        .categories
        .iter()
        .any(|category| category.name == "方案"));
}

#[tokio::test]
async fn archive_settings_persist_roundtrip() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let service = service(&db, &app_data);
    let root = TempDir::new().unwrap();

    service
        .save_settings(default_settings(root.path().to_str().unwrap()))
        .await
        .unwrap();

    let reloaded = ArchiveService::new(db.handle(), app_data.path()).load_settings().await.unwrap();
    assert_eq!(reloaded.root, root.path().display().to_string());
    assert!(reloaded.auto_discover);
}

#[tokio::test]
async fn archives_a_file_into_project_with_classification_and_ledger() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let service = service(&db, &app_data);
    service
        .save_settings(default_settings(root.path().to_str().unwrap()))
        .await
        .unwrap();

    let source = root.path().join("集成方案v0.1.pdf");
    fs::write(&source, "plan content").unwrap();

    let results = service
        .archive_files(vec![source.display().to_string()], "0828".to_string())
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert!(result.ok, "{:?}", result.reason);
    assert_eq!(result.category.as_deref(), Some("方案"));
    assert_eq!(result.version.as_deref(), Some("v0.1"));
    assert!(source.exists(), "original file must be untouched");
    assert!(root.path().join("归档区").join("0828").join("源文件").is_dir());

    let ledger = service.ledger(20).await.unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].project, "0828");
    assert_eq!(ledger[0].category, "方案");
    assert_eq!(ledger[0].version, "v0.1");
    assert!(ledger[0].archive_rel_path.contains("0828/"));
}

#[tokio::test]
async fn duplicate_is_rejected_but_revision_is_versioned() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let service = service(&db, &app_data);
    service
        .save_settings(default_settings(root.path().to_str().unwrap()))
        .await
        .unwrap();

    let source = root.path().join("需求说明.docx");
    fs::write(&source, "same bytes").unwrap();

    let first = service
        .archive_files(vec![source.display().to_string()], "A".to_string())
        .await
        .unwrap();
    assert!(first[0].ok);

    let duplicate = service
        .archive_files(vec![source.display().to_string()], "A".to_string())
        .await
        .unwrap();
    assert!(duplicate[0].duplicate);

    fs::write(&source, "different bytes").unwrap();
    let revised = service
        .archive_files(vec![source.display().to_string()], "A".to_string())
        .await
        .unwrap();
    assert!(revised[0].ok);
    assert_eq!(revised[0].version.as_deref(), Some("v0.2"));

    let ledger = service.ledger(20).await.unwrap();
    assert_eq!(ledger.len(), 2);
}

#[tokio::test]
async fn pending_paths_hand_off_between_pet_and_chat() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let service = service(&db, &app_data);

    service
        .set_pending_paths(vec!["C:\\drop\\a.pdf".to_string(), "C:\\drop\\b.docx".to_string()])
        .await;

    let taken = service.take_pending_paths().await;
    assert_eq!(taken.len(), 2);
    assert!(service.take_pending_paths().await.is_empty());
}

#[tokio::test]
async fn invalid_root_is_rejected_on_save() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let service = service(&db, &app_data);

    // A root that is an existing FILE cannot become an archive directory.
    let file_root = app_data.path().join("not-a-dir");
    fs::write(&file_root, "block").unwrap();

    let error = service
        .save_settings(default_settings(file_root.to_str().unwrap()))
        .await
        .expect_err("a file root must be rejected");
    assert!(error.message.contains("根目录"));
}

#[tokio::test]
async fn custom_structure_keywords_are_honored() {
    let db = TestDatabase::new();
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let service = service(&db, &app_data);

    let mut template = StructureTemplate::default_template();
    template.categories.push(CategoryRule::new("资料", vec!["资料".to_string()]));
    service
        .save_settings(SaveArchiveSettings {
            root: root.path().to_str().unwrap().to_string(),
            auto_discover: false,
            template_name: "默认".to_string(),
            templates: vec![template],
        })
        .await
        .unwrap();

    let source = root.path().join("项目资料.txt");
    fs::write(&source, "x").unwrap();
    let results = service
        .archive_files(vec![source.display().to_string()], "P".to_string())
        .await
        .unwrap();

    assert_eq!(results[0].category.as_deref(), Some("资料"));
}
