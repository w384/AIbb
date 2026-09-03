use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{ImageFormat, ImageReader, Limits};
use uuid::Uuid;

use crate::error::AppError;

pub(crate) const AVATAR_FILENAME: &str = "avatar.webp";
pub(crate) const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;
const MAX_AVATAR_DIMENSION: u32 = 4096;
const MAX_DECODED_AVATAR_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) struct AvatarFileTransaction {
    target_path: PathBuf,
    backup_path: Option<PathBuf>,
    replacement_installed: bool,
    finished: bool,
}

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

pub(crate) fn normalize_avatar(bytes: &[u8], mime_type: &str) -> Result<Vec<u8>, AppError> {
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return Err(invalid_avatar_error());
    }

    let format = match mime_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::WebP,
        _ => return Err(invalid_avatar_error()),
    };
    let image = decode_avatar(bytes, format).map_err(|_| invalid_avatar_error())?;
    let mut normalized = Cursor::new(Vec::new());
    image
        .write_to(&mut normalized, ImageFormat::WebP)
        .map_err(|_| invalid_avatar_error())?;
    let normalized = normalized.into_inner();
    if normalized.len() > MAX_AVATAR_BYTES {
        return Err(invalid_avatar_error());
    }

    Ok(normalized)
}

impl AvatarFileTransaction {
    pub(crate) fn replace(
        profile_directory: &Path,
        replacement: Option<&[u8]>,
    ) -> Result<Self, AppError> {
        if replacement.is_some() {
            fs::create_dir_all(profile_directory).map_err(|_| avatar_storage_error())?;
        }

        let target_path = profile_directory.join(AVATAR_FILENAME);
        let nonce = Uuid::new_v4();
        let temporary_path =
            replacement.map(|_| profile_directory.join(format!(".avatar-{nonce}.tmp")));
        let backup_candidate = profile_directory.join(format!(".avatar-{nonce}.backup"));

        if let (Some(bytes), Some(path)) = (replacement, temporary_path.as_ref()) {
            write_new_file(path, bytes)?;
        }

        let backup_path = if target_path.exists() {
            if let Err(error) = fs::rename(&target_path, &backup_candidate) {
                remove_if_present(temporary_path.as_deref());
                return Err(avatar_storage_error_with(error));
            }
            Some(backup_candidate)
        } else {
            None
        };

        let replacement_installed = if let Some(path) = temporary_path.as_ref() {
            if let Err(error) = fs::rename(path, &target_path) {
                if let Some(backup) = backup_path.as_ref() {
                    let _ = fs::rename(backup, &target_path);
                }
                remove_if_present(Some(path));
                return Err(avatar_storage_error_with(error));
            }
            true
        } else {
            false
        };

        Ok(Self {
            target_path,
            backup_path,
            replacement_installed,
            finished: false,
        })
    }

    pub(crate) fn rollback(mut self) -> Result<(), AppError> {
        self.rollback_inner()?;
        self.finished = true;
        Ok(())
    }

    pub(crate) fn commit(mut self) {
        remove_if_present(self.backup_path.as_deref());
        self.finished = true;
    }

    fn rollback_inner(&mut self) -> Result<(), AppError> {
        if self.replacement_installed {
            match fs::remove_file(&self.target_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(avatar_storage_error_with(error)),
            }
        }
        if let Some(backup_path) = self.backup_path.as_ref() {
            fs::rename(backup_path, &self.target_path).map_err(avatar_storage_error_with)?;
        }
        Ok(())
    }
}

impl Drop for AvatarFileTransaction {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.rollback_inner();
        }
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(avatar_storage_error_with)?;
    file.write_all(bytes).map_err(avatar_storage_error_with)?;
    file.sync_all().map_err(avatar_storage_error_with)
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
    decode_avatar(&bytes, ImageFormat::WebP).map_err(|_| avatar_storage_error())?;

    Ok(Some(avatar_data_url(&bytes)))
}

pub(crate) fn avatar_data_url(bytes: &[u8]) -> String {
    format!("data:image/webp;base64,{}", BASE64_STANDARD.encode(bytes))
}

fn decode_avatar(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<image::DynamicImage, image::ImageError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_AVATAR_DIMENSION);
    limits.max_image_height = Some(MAX_AVATAR_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_AVATAR_BYTES);
    reader.limits(limits);
    reader.decode()
}

fn remove_if_present(path: Option<&Path>) {
    if let Some(path) = path {
        let _ = fs::remove_file(path);
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

fn avatar_storage_error_with(_: std::io::Error) -> AppError {
    avatar_storage_error()
}
