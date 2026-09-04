use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use uuid::Uuid;

use crate::error::AppError;

pub(crate) const AVATAR_FILENAME: &str = "avatar.webp";
pub(crate) const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;
const MAX_AVATAR_DIMENSION: u32 = 4096;
const MAX_DECODED_AVATAR_BYTES: u64 = 64 * 1024 * 1024;
const NORMALIZED_AVATAR_SIZE: u32 = 256;

pub(crate) struct AvatarFileTransaction {
    target_path: PathBuf,
    backup_path: Option<PathBuf>,
    replacement_installed: bool,
    finished: bool,
}

enum AvatarLoadFailure {
    Missing,
    Invalid,
    Unavailable,
}

trait FileRenamer {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
}

struct StdFileRenamer;

impl FileRenamer for StdFileRenamer {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }
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
    let crop_size = image.width().min(image.height());
    let image = image
        .crop_imm(
            (image.width() - crop_size) / 2,
            (image.height() - crop_size) / 2,
            crop_size,
            crop_size,
        )
        .resize_exact(
            NORMALIZED_AVATAR_SIZE,
            NORMALIZED_AVATAR_SIZE,
            FilterType::Lanczos3,
        );
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
        Self::replace_with_renamer(profile_directory, replacement, &StdFileRenamer)
    }

    fn replace_with_renamer(
        profile_directory: &Path,
        replacement: Option<&[u8]>,
        renamer: &impl FileRenamer,
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
            if let Err(error) = renamer.rename(&target_path, &backup_candidate) {
                remove_if_present(temporary_path.as_deref());
                return Err(avatar_storage_error_with(error));
            }
            Some(backup_candidate)
        } else {
            None
        };

        let replacement_installed = if let Some(path) = temporary_path.as_ref() {
            if let Err(install_error) = renamer.rename(path, &target_path) {
                if let Some(backup) = backup_path.as_ref() {
                    if renamer.rename(backup, &target_path).is_err() {
                        restore_backup_by_copy(backup, &target_path)?;
                        remove_if_present(Some(backup));
                    }
                }
                remove_if_present(Some(path));
                return Err(avatar_storage_error_with(install_error));
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

fn restore_backup_by_copy(backup_path: &Path, target_path: &Path) -> Result<(), AppError> {
    fs::copy(backup_path, target_path).map_err(avatar_recovery_error_with)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(target_path)
        .and_then(|file| file.sync_all())
        .map_err(avatar_recovery_error_with)
}

pub(crate) fn load_or_recover_avatar_data_url(
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
    let initial_failure = match load_avatar_bytes(&path) {
        Ok(bytes) => {
            cleanup_avatar_artifacts(profile_directory);
            return Ok(Some(avatar_data_url(&bytes)));
        }
        Err(failure) => failure,
    };

    if let Some(bytes) = newest_valid_avatar_backup(profile_directory)? {
        let transaction = AvatarFileTransaction::replace(profile_directory, Some(&bytes))?;
        transaction.commit();
        cleanup_avatar_artifacts(profile_directory);
        return Ok(Some(avatar_data_url(&bytes)));
    }

    if matches!(initial_failure, AvatarLoadFailure::Unavailable) {
        return Err(avatar_storage_error());
    }

    remove_if_present(Some(&path));
    cleanup_avatar_artifacts(profile_directory);
    Ok(None)
}

fn load_avatar_bytes(path: &Path) -> Result<Vec<u8>, AvatarLoadFailure> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AvatarLoadFailure::Missing
        } else {
            AvatarLoadFailure::Unavailable
        }
    })?;
    if metadata.len() > MAX_AVATAR_BYTES as u64 {
        return Err(AvatarLoadFailure::Invalid);
    }
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AvatarLoadFailure::Missing
        } else {
            AvatarLoadFailure::Unavailable
        }
    })?;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(AvatarLoadFailure::Invalid);
    }
    decode_avatar(&bytes, ImageFormat::WebP).map_err(|_| AvatarLoadFailure::Invalid)?;

    Ok(bytes)
}

fn newest_valid_avatar_backup(profile_directory: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let entries = match fs::read_dir(profile_directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(avatar_storage_error()),
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".avatar-") && name.ends_with(".backup")
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|entry| entry.metadata().and_then(|value| value.modified()).ok());
    for candidate in candidates.into_iter().rev() {
        if let Ok(bytes) = load_avatar_bytes(&candidate.path()) {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn cleanup_avatar_artifacts(profile_directory: &Path) {
    let Ok(entries) = fs::read_dir(profile_directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".avatar-") && (name.ends_with(".backup") || name.ends_with(".tmp")) {
            remove_if_present(Some(&entry.path()));
        }
    }
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

fn avatar_recovery_error_with(_: std::io::Error) -> AppError {
    AppError::new(
        "profileRecoveryRequired",
        "The previous AIbb profile image could not be restored automatically.",
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, io};

    use super::*;
    use crate::{
        settings::{FixedCredentialStore, SettingsService},
        storage::Database,
    };

    struct FailingInstallAndRestoreRenamer {
        calls: Cell<usize>,
    }

    impl FileRenamer for FailingInstallAndRestoreRenamer {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let call = self.calls.get() + 1;
            self.calls.set(call);
            if call == 1 {
                fs::rename(from, to)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "forced rename failure",
                ))
            }
        }
    }

    #[tokio::test]
    async fn failed_install_and_failed_rename_restore_keep_the_visible_profile_coherent() {
        let directory = tempfile::tempdir().unwrap();
        let profile_directory = directory.path().join("aibb-profile");
        fs::create_dir_all(&profile_directory).unwrap();
        let target = profile_directory.join(AVATAR_FILENAME);
        let mut previous = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut previous, ImageFormat::WebP)
            .unwrap();
        let previous = previous.into_inner();
        fs::write(&target, &previous).unwrap();
        let database = Database::open(directory.path().join("aibb.sqlite3")).unwrap();
        database.set_aibb_avatar_present(true).unwrap();
        let persisted_before = database.load_aibb_profile().unwrap();
        let service = SettingsService::new_with_app_data_dir(
            database.clone(),
            FixedCredentialStore::new(None),
            directory.path(),
        );
        let visible_before = service.load_aibb_profile().await.unwrap();
        let renamer = FailingInstallAndRestoreRenamer {
            calls: Cell::new(0),
        };

        let result = AvatarFileTransaction::replace_with_renamer(
            &profile_directory,
            Some(b"replacement avatar"),
            &renamer,
        );

        assert!(result.is_err());
        assert_eq!(renamer.calls.get(), 3);
        assert_eq!(fs::read(&target).unwrap(), previous);
        assert_eq!(database.load_aibb_profile().unwrap(), persisted_before);
        assert_eq!(service.load_aibb_profile().await.unwrap(), visible_before);
        assert_eq!(
            fs::read_dir(&profile_directory)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>(),
            vec![std::ffi::OsString::from(AVATAR_FILENAME)]
        );
    }
}
