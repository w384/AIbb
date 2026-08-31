use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::{params, Connection};

use crate::error::AppError;

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

fn storage_error() -> AppError {
    AppError {
        code: "storageUnavailable".to_string(),
        message: "Application settings could not be accessed.".to_string(),
    }
}
