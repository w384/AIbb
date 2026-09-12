use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::{
    archive::models::ArchiveLedgerEntry,
    error::{AppError, ErrorCode},
    exploration::{CancelOutcome, ExplorationRecord, ExplorationStatus, ExplorationStore},
};

use super::migrations;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedSettings {
    pub api_base: String,
    pub model: String,
    pub web_mode: String,
    pub always_on_top: bool,
    pub autostart: bool,
    pub persona: String,
    pub first_run_complete: bool,
    pub pet_position: Option<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAibbProfile {
    pub name: String,
    pub avatar_filename: Option<String>,
    pub version: i64,
}

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| storage_error())?;
        }
        let mut connection = Connection::open(path).map_err(|_| storage_error())?;
        migrations::apply(&mut connection).map_err(|_| storage_error())?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn load_settings(&self) -> Result<PersistedSettings, AppError> {
        self.connection()?
            .query_row(
                "SELECT api_base, model, web_mode, always_on_top, autostart, persona, \
                 first_run_complete, pet_x, pet_y \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    let pet_x = row.get::<_, Option<i32>>(7)?;
                    let pet_y = row.get::<_, Option<i32>>(8)?;

                    Ok(PersistedSettings {
                        api_base: row.get(0)?,
                        model: row.get(1)?,
                        web_mode: row.get(2)?,
                        always_on_top: row.get::<_, i64>(3)? != 0,
                        autostart: row.get::<_, i64>(4)? != 0,
                        persona: row.get(5)?,
                        first_run_complete: row.get::<_, i64>(6)? != 0,
                        pet_position: pet_x.zip(pet_y),
                    })
                },
            )
            .map_err(|_| storage_error())
    }

    pub fn save_settings(
        &self,
        api_base: &str,
        model: &str,
        web_mode: &str,
        always_on_top: bool,
        autostart: bool,
        persona: &str,
    ) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET api_base = ?1, model = ?2, web_mode = ?3, \
                 always_on_top = ?4, autostart = ?5, persona = ?6 WHERE singleton = 1",
                params![api_base, model, web_mode, always_on_top, autostart, persona],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn load_aibb_profile(&self) -> Result<PersistedAibbProfile, AppError> {
        self.connection()?
            .query_row(
                "SELECT aibb_name, avatar_filename, profile_version \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    Ok(PersistedAibbProfile {
                        name: row.get(0)?,
                        avatar_filename: row.get(1)?,
                        version: row.get(2)?,
                    })
                },
            )
            .map_err(|_| storage_error())
    }

    pub fn save_aibb_name(&self, name: &str) -> Result<i64, AppError> {
        self.connection()?
            .query_row(
                "UPDATE app_settings SET aibb_name = ?1, profile_version = profile_version + 1 \
                 WHERE singleton = 1 RETURNING profile_version",
                params![name],
                |row| row.get(0),
            )
            .map_err(|_| storage_error())
    }

    pub fn set_aibb_avatar_present(&self, present: bool) -> Result<i64, AppError> {
        let avatar_filename = present.then_some("avatar.webp");
        self.connection()?
            .query_row(
                "UPDATE app_settings SET avatar_filename = ?1, \
                 profile_version = profile_version + 1 WHERE singleton = 1 \
                 RETURNING profile_version",
                params![avatar_filename],
                |row| row.get(0),
            )
            .map_err(|_| storage_error())
    }

    pub fn set_first_run_complete(&self) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET first_run_complete = 1 WHERE singleton = 1",
                [],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn load_pet_position(&self) -> Result<Option<(i32, i32)>, AppError> {
        self.connection()?
            .query_row(
                "SELECT pet_x, pet_y FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    let pet_x = row.get::<_, Option<i32>>(0)?;
                    let pet_y = row.get::<_, Option<i32>>(1)?;
                    Ok(pet_x.zip(pet_y))
                },
            )
            .map_err(|_| storage_error())
    }

    pub fn save_pet_position(&self, x: i32, y: i32) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET pet_x = ?1, pet_y = ?2 WHERE singleton = 1",
                params![x, y],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    // ---- archive ----

    pub fn load_archive_settings(&self) -> Result<(String, bool), AppError> {
        self.connection()?
            .query_row(
                "SELECT archive_root, archive_auto_discover \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)? != 0,
                    ))
                },
            )
            .map_err(|_| storage_error())
    }

    pub fn save_archive_settings(&self, root: &str, auto_discover: bool) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET archive_root = ?1, archive_auto_discover = ?2 \
                 WHERE singleton = 1",
                params![root, auto_discover],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn load_archive_structure_lib(&self) -> Result<Option<(String, String)>, AppError> {
        self.connection()?
            .query_row(
                "SELECT template_name, templates_json \
                 FROM archive_structure_lib WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                _ => Err(storage_error()),
            })
    }

    pub fn save_archive_structure_lib(&self, name: &str, templates_json: &str) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "INSERT INTO archive_structure_lib(singleton, template_name, templates_json) \
                 VALUES (1, ?1, ?2) \
                 ON CONFLICT(singleton) DO UPDATE SET \
                   template_name = excluded.template_name, \
                   templates_json = excluded.templates_json",
                params![name, templates_json],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn insert_archive_ledger(&self, entry: &ArchiveLedgerEntry) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "INSERT INTO archive_ledger(\
                   id, file_name, project, category, period, version, \
                   archive_rel_path, backup_rel_path, status, error_code, created_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    entry.id,
                    entry.file_name,
                    entry.project,
                    entry.category,
                    entry.period,
                    entry.version,
                    entry.archive_rel_path,
                    entry.backup_rel_path,
                    entry.status,
                    entry.error_code,
                    entry.created_at
                ],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn list_archive_ledger(&self, limit: usize) -> Result<Vec<ArchiveLedgerEntry>, AppError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, file_name, project, category, period, version, \
                 archive_rel_path, backup_rel_path, status, error_code, created_at \
                 FROM archive_ledger ORDER BY created_at DESC LIMIT ?",
            )
            .map_err(|_| storage_error())?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(ArchiveLedgerEntry {
                    id: row.get(0)?,
                    file_name: row.get(1)?,
                    project: row.get(2)?,
                    category: row.get(3)?,
                    period: row.get(4)?,
                    version: row.get(5)?,
                    archive_rel_path: row.get(6)?,
                    backup_rel_path: row.get(7)?,
                    status: row.get(8)?,
                    error_code: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })
            .map_err(|_| storage_error())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|_| storage_error())
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.connection.lock().map_err(|_| storage_error())
    }
}

#[async_trait]
impl ExplorationStore for Database {
    async fn create_queued(&self, id: Uuid, direction: Option<&str>) -> Result<(), AppError> {
        let connection = self.connection()?;
        let now = unix_milliseconds()?;
        connection
            .execute(
                "INSERT INTO explorations(\
                   id, status, user_direction, created_at, updated_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    id.to_string(),
                    ExplorationStatus::Queued.as_storage_value(),
                    direction,
                    now
                ],
            )
            .map(|_| ())
            .map_err(|error| {
                if is_constraint_violation(&error) {
                    exploration_already_running_error()
                } else {
                    exploration_storage_error()
                }
            })
    }

    async fn transition(&self, id: Uuid, next: ExplorationStatus) -> Result<(), AppError> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|_| exploration_storage_error())?;
        let (current, updated_at) = status_and_updated_at(&transaction, id)?;
        if !current.can_transition_to(next) {
            return Err(invalid_transition_error());
        }
        let now = unix_milliseconds()?.max(updated_at.saturating_add(1));
        let changed = transaction
            .execute(
                "UPDATE explorations SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![next.as_storage_value(), now, id.to_string()],
            )
            .map_err(|_| exploration_storage_error())?;
        if changed != 1 {
            return Err(exploration_storage_error());
        }
        transaction
            .commit()
            .map_err(|_| exploration_storage_error())
    }

    async fn complete(
        &self,
        id: Uuid,
        result: &crate::domain::ExplorationResult,
        safe_raw_response: &str,
    ) -> Result<(), AppError> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|_| exploration_storage_error())?;
        let (current, updated_at) = status_and_updated_at(&transaction, id)?;
        if !current.can_transition_to(ExplorationStatus::Completed) {
            return Err(invalid_transition_error());
        }
        let now = unix_milliseconds()?.max(updated_at.saturating_add(1));
        let items =
            serde_json::to_string(&result.items).map_err(|_| exploration_storage_error())?;
        let sources =
            serde_json::to_string(&result.sources).map_err(|_| exploration_storage_error())?;
        let round_number =
            i64::try_from(result.round_number).map_err(|_| exploration_storage_error())?;
        let elapsed_seconds =
            i64::try_from(result.elapsed_seconds).map_err(|_| exploration_storage_error())?;
        let changed = transaction
            .execute(
                "UPDATE explorations SET status = ?1, items_json = ?2, \
                   diary = ?3, sources_json = ?4, round_number = ?5, elapsed_seconds = ?6, \
                   next_outing_request = NULL, raw_response = ?7, error_code = NULL, \
                   updated_at = ?8 WHERE id = ?9",
                params![
                    ExplorationStatus::Completed.as_storage_value(),
                    items,
                    result.diary,
                    sources,
                    round_number,
                    elapsed_seconds,
                    safe_raw_response,
                    now,
                    id.to_string()
                ],
            )
            .map_err(|_| exploration_storage_error())?;
        if changed != 1 {
            return Err(exploration_storage_error());
        }
        let latest_message = transaction
            .query_row(
                "SELECT COALESCE(MAX(created_at), 0) FROM messages",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| exploration_storage_error())?;
        let message_created_at = now.max(latest_message.saturating_add(1));
        transaction
            .execute(
                "INSERT INTO messages(id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    format!("message-{}", Uuid::new_v4()),
                    "assistant",
                    result.diary,
                    message_created_at
                ],
            )
            .map_err(|_| exploration_storage_error())?;
        transaction
            .commit()
            .map_err(|_| exploration_storage_error())
    }

    async fn fail(
        &self,
        id: Uuid,
        error_code: &str,
        raw_response: Option<&str>,
    ) -> Result<(), AppError> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|_| exploration_storage_error())?;
        let (current, updated_at) = status_and_updated_at(&transaction, id)?;
        if !current.can_transition_to(ExplorationStatus::Failed) {
            return Err(invalid_transition_error());
        }
        let now = unix_milliseconds()?.max(updated_at.saturating_add(1));
        transaction
            .execute(
                "UPDATE explorations SET status = ?1, error_code = ?2, raw_response = ?3, \
                 updated_at = ?4 WHERE id = ?5",
                params![
                    ExplorationStatus::Failed.as_storage_value(),
                    error_code,
                    raw_response,
                    now,
                    id.to_string()
                ],
            )
            .map_err(|_| exploration_storage_error())?;
        transaction
            .commit()
            .map_err(|_| exploration_storage_error())
    }

    async fn cancel(&self, id: Uuid) -> Result<CancelOutcome, AppError> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|_| exploration_storage_error())?;
        let Some((current, updated_at)) = optional_status_and_updated_at(&transaction, id)? else {
            return Ok(CancelOutcome::NotFound);
        };
        if current == ExplorationStatus::Cancelled {
            return Ok(CancelOutcome::AlreadyCancelled);
        }
        if current.is_terminal() {
            return Ok(CancelOutcome::NotCancellable);
        }
        let now = unix_milliseconds()?.max(updated_at.saturating_add(1));
        transaction
            .execute(
                "UPDATE explorations SET status = ?1, error_code = ?2, updated_at = ?3 \
                 WHERE id = ?4",
                params![
                    ExplorationStatus::Cancelled.as_storage_value(),
                    ErrorCode::Cancelled.as_str(),
                    now,
                    id.to_string()
                ],
            )
            .map_err(|_| exploration_storage_error())?;
        transaction
            .commit()
            .map_err(|_| exploration_storage_error())?;
        Ok(CancelOutcome::Cancelled)
    }

    async fn recover_interrupted(&self) -> Result<usize, AppError> {
        let connection = self.connection()?;
        let now = unix_milliseconds()?;
        connection
            .execute(
                "UPDATE explorations SET status = ?1, updated_at = \
                   CASE WHEN updated_at >= ?2 THEN updated_at + 1 ELSE ?2 END \
                 WHERE status IN (\
                   'queued', 'choosing', 'native_searching', 'public_searching', \
                   'reading', 'writing', 'correcting'\
                 )",
                params![ExplorationStatus::Interrupted.as_storage_value(), now],
            )
            .map_err(|_| exploration_storage_error())
    }

    async fn load(&self, id: Uuid) -> Result<Option<ExplorationRecord>, AppError> {
        let connection = self.connection()?;
        exploration_from_connection(&connection, id)
    }

    async fn completed_outings(&self) -> Result<u64, AppError> {
        let count = self
            .connection()?
            .query_row(
                "SELECT COUNT(*) FROM explorations WHERE status = 'completed'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| exploration_storage_error())?;
        u64::try_from(count).map_err(|_| exploration_storage_error())
    }
}

fn exploration_from_connection(
    connection: &Connection,
    id: Uuid,
) -> Result<Option<ExplorationRecord>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, status, user_direction, items_json, diary, sources_json, \
             round_number, elapsed_seconds, raw_response, error_code, created_at, updated_at \
             FROM explorations WHERE id = ?1",
        )
        .map_err(|_| exploration_storage_error())?;
    let mut rows = statement
        .query(params![id.to_string()])
        .map_err(|_| exploration_storage_error())?;
    rows.next()
        .map_err(|_| exploration_storage_error())?
        .map(exploration_from_row)
        .transpose()
}

fn exploration_from_row(row: &rusqlite::Row<'_>) -> Result<ExplorationRecord, AppError> {
    let id = row
        .get::<_, String>(0)
        .map_err(|_| exploration_storage_error())?;
    let status = row
        .get::<_, String>(1)
        .map_err(|_| exploration_storage_error())?;
    let items = row
        .get::<_, Option<String>>(3)
        .map_err(|_| exploration_storage_error())?
        .map(|items| serde_json::from_str::<[String; 4]>(&items))
        .transpose()
        .map_err(|_| exploration_storage_error())?;
    let sources = row
        .get::<_, Option<String>>(5)
        .map_err(|_| exploration_storage_error())?
        .map(|sources| serde_json::from_str(&sources))
        .transpose()
        .map_err(|_| exploration_storage_error())?;
    Ok(ExplorationRecord {
        id: Uuid::parse_str(&id).map_err(|_| exploration_storage_error())?,
        status: ExplorationStatus::from_storage_value(&status)
            .ok_or_else(exploration_storage_error)?,
        user_direction: row.get(2).map_err(|_| exploration_storage_error())?,
        items,
        diary: row.get(4).map_err(|_| exploration_storage_error())?,
        sources,
        round_number: row
            .get::<_, Option<i64>>(6)
            .map_err(|_| exploration_storage_error())?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| exploration_storage_error())?,
        elapsed_seconds: row
            .get::<_, Option<i64>>(7)
            .map_err(|_| exploration_storage_error())?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| exploration_storage_error())?,
        raw_response: row.get(8).map_err(|_| exploration_storage_error())?,
        error_code: row.get(9).map_err(|_| exploration_storage_error())?,
        created_at: row.get(10).map_err(|_| exploration_storage_error())?,
        updated_at: row.get(11).map_err(|_| exploration_storage_error())?,
    })
}

fn status_and_updated_at(
    transaction: &rusqlite::Transaction<'_>,
    id: Uuid,
) -> Result<(ExplorationStatus, i64), AppError> {
    optional_status_and_updated_at(transaction, id)?.ok_or_else(exploration_not_found_error)
}

fn optional_status_and_updated_at(
    transaction: &rusqlite::Transaction<'_>,
    id: Uuid,
) -> Result<Option<(ExplorationStatus, i64)>, AppError> {
    let mut statement = transaction
        .prepare("SELECT status, updated_at FROM explorations WHERE id = ?1")
        .map_err(|_| exploration_storage_error())?;
    let mut rows = statement
        .query(params![id.to_string()])
        .map_err(|_| exploration_storage_error())?;
    let Some(row) = rows.next().map_err(|_| exploration_storage_error())? else {
        return Ok(None);
    };
    let status = row
        .get::<_, String>(0)
        .map_err(|_| exploration_storage_error())?;
    Ok(Some((
        ExplorationStatus::from_storage_value(&status).ok_or_else(exploration_storage_error)?,
        row.get(1).map_err(|_| exploration_storage_error())?,
    )))
}

fn unix_milliseconds() -> Result<i64, AppError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| exploration_storage_error())?;
    i64::try_from(elapsed.as_millis()).map_err(|_| exploration_storage_error())
}

fn exploration_storage_error() -> AppError {
    AppError::new(
        "exploration_storage_unavailable",
        "The exploration task could not be stored.",
    )
}

fn exploration_already_running_error() -> AppError {
    AppError::new(
        "exploration_already_running",
        "Another outing is already running.",
    )
}

fn is_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn exploration_not_found_error() -> AppError {
    AppError::new(
        "exploration_not_found",
        "The exploration task was not found.",
    )
}

fn invalid_transition_error() -> AppError {
    AppError::new(
        "invalid_exploration_transition",
        "The exploration task state transition is invalid.",
    )
}

fn storage_error() -> AppError {
    AppError::new(
        "storageUnavailable",
        "Application settings could not be accessed.",
    )
}
