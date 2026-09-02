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
    pub first_run_complete: bool,
    pub pet_position: Option<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAibbProfile {
    pub name: String,
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
                "SELECT api_base, model, web_mode, always_on_top, autostart, \
                 first_run_complete, pet_x, pet_y \
                 FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    let pet_x = row.get::<_, Option<i32>>(6)?;
                    let pet_y = row.get::<_, Option<i32>>(7)?;

                    Ok(PersistedSettings {
                        api_base: row.get(0)?,
                        model: row.get(1)?,
                        web_mode: row.get(2)?,
                        always_on_top: row.get::<_, i64>(3)? != 0,
                        autostart: row.get::<_, i64>(4)? != 0,
                        first_run_complete: row.get::<_, i64>(5)? != 0,
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
    ) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET api_base = ?1, model = ?2, web_mode = ?3, \
                 always_on_top = ?4, autostart = ?5 WHERE singleton = 1",
                params![api_base, model, web_mode, always_on_top, autostart],
            )
            .map(|_| ())
            .map_err(|_| storage_error())
    }

    pub fn load_aibb_profile(&self) -> Result<PersistedAibbProfile, AppError> {
        self.connection()?
            .query_row(
                "SELECT aibb_name, profile_version FROM app_settings WHERE singleton = 1",
                [],
                |row| {
                    Ok(PersistedAibbProfile {
                        name: row.get(0)?,
                        version: row.get(1)?,
                    })
                },
            )
            .map_err(|_| storage_error())
    }

    pub fn save_aibb_name(&self, name: &str) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET aibb_name = ?1, profile_version = profile_version + 1 \
                 WHERE singleton = 1",
                params![name],
            )
            .map(|_| ())
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
            .map_err(|_| exploration_storage_error())
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
        let changed = transaction
            .execute(
                "UPDATE explorations SET status = ?1, items_json = ?2, \
                   next_outing_request = ?3, raw_response = ?4, error_code = NULL, \
                   updated_at = ?5 WHERE id = ?6",
                params![
                    ExplorationStatus::Completed.as_storage_value(),
                    items,
                    result.next_outing_request,
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
                    result.next_outing_request,
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
}

fn exploration_from_connection(
    connection: &Connection,
    id: Uuid,
) -> Result<Option<ExplorationRecord>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, status, user_direction, items_json, next_outing_request, \
             raw_response, error_code, created_at, updated_at \
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
    Ok(ExplorationRecord {
        id: Uuid::parse_str(&id).map_err(|_| exploration_storage_error())?,
        status: ExplorationStatus::from_storage_value(&status)
            .ok_or_else(exploration_storage_error)?,
        user_direction: row.get(2).map_err(|_| exploration_storage_error())?,
        items,
        next_outing_request: row.get(4).map_err(|_| exploration_storage_error())?,
        raw_response: row.get(5).map_err(|_| exploration_storage_error())?,
        error_code: row.get(6).map_err(|_| exploration_storage_error())?,
        created_at: row.get(7).map_err(|_| exploration_storage_error())?,
        updated_at: row.get(8).map_err(|_| exploration_storage_error())?,
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
