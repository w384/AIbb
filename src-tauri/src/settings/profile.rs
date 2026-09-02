use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use uuid::Uuid;

use crate::error::AppError;

pub(crate) const AVATAR_FILENAME: &str = "avatar.webp";
pub(crate) const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;

pub(crate) fn validate_aibb_name(name: String) -> Result<String, AppError> {
    let name = name.trim();

    if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err(AppError::new(
            "invalidProfile",
            "AIbb's name must be 1 to 24 characters and cannot contain control characters.",
        ));
    }

    Ok(name.to_string())
}

pub(crate) fn validate_avatar(bytes: &[u8], mime_type: &str) -> Result<(), AppError> {
    if bytes.is_empty()
        || bytes.len() > MAX_AVATAR_BYTES
        || !matches!(mime_type, "image/png" | "image/jpeg" | "image/webp")
    {
        return Err(invalid_avatar_error());
    }

    Ok(())
}

pub(crate) fn write_avatar_atomically(
    profile_directory: &Path,
    bytes: &[u8],
) -> Result<(), AppError> {
    fs::create_dir_all(profile_directory).map_err(|_| avatar_storage_error())?;
    let temporary_path = profile_directory.join(format!(".avatar-{}.tmp", Uuid::new_v4()));
    let target_path = profile_directory.join(AVATAR_FILENAME);
    let result = (|| {
        let mut temporary = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|_| avatar_storage_error())?;
        temporary
            .write_all(bytes)
            .map_err(|_| avatar_storage_error())?;
        temporary.sync_all().map_err(|_| avatar_storage_error())?;
        drop(temporary);
        fs::rename(&temporary_path, target_path).map_err(|_| avatar_storage_error())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary_path);
    }
    result
}

pub(crate) fn load_avatar_data_url(
    profile_directory: &Path,
    avatar_filename: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(filename) = avatar_filename else {
        return Ok(None);
    };
    if filename != AVATAR_FILENAME {
        return Err(avatar_storage_error());
    }

    let path = profile_directory.join(AVATAR_FILENAME);
    let metadata = fs::metadata(&path).map_err(|_| avatar_storage_error())?;
    if metadata.len() > MAX_AVATAR_BYTES as u64 {
        return Err(avatar_storage_error());
    }
    let bytes = fs::read(path).map_err(|_| avatar_storage_error())?;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(avatar_storage_error());
    }

    Ok(Some(format!(
        "data:image/webp;base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

pub(crate) fn remove_avatar(profile_directory: &Path) -> Result<(), AppError> {
    let path = profile_directory.join(AVATAR_FILENAME);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(avatar_storage_error()),
    }
}

fn invalid_avatar_error() -> AppError {
    AppError::new(
        "invalidProfile",
        "The avatar must be a PNG, JPEG, or WebP image no larger than 5 MiB.",
    )
}

fn avatar_storage_error() -> AppError {
    AppError::new(
        "profileStorageUnavailable",
        "The AIbb profile image could not be accessed.",
    )
}
