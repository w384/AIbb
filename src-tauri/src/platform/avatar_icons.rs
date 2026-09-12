//! Derive the system-tray icon and the desktop-shortcut icon from the user's
//! avatar, so the whole product identity follows the uploaded picture.
//!
//! - Tray icon: the avatar is resized to a 32×32 PNG and applied to the
//!   "aibb-tray" tray icon at runtime ([`set_tray_icon`]).
//! - Desktop shortcut: a multi-size PNG-in-ICO file is written next to the
//!   app data, and the `IconLocation` of `AIbb.lnk` (user / OneDrive / public
//!   desktop) is pointed at it via a short PowerShell COM call
//!   ([`update_shortcut_icon`]).
//!
//! Every icon update is best-effort: the avatar save itself must never fail
//! because an icon file could not be written or a shortcut could not be
//! touched.

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use tauri::{image::Image, AppHandle, Runtime};

use crate::error::AppError;

/// Tray icon PNG written next to the app data (32×32).
pub const TRAY_AVATAR_FILENAME: &str = "avatar-tray.png";
/// Multi-size ICO written next to the app data for the desktop shortcut.
pub const SHORTCUT_AVATAR_FILENAME: &str = "avatar.ico";

const TRAY_ICON_SIZE: u32 = 32;
const SHORTCUT_ICON_SIZES: [u32; 4] = [16, 32, 48, 256];
const MAX_AVATAR_DIMENSION: u32 = 4096;
const MAX_DECODED_AVATAR_BYTES: u64 = 64 * 1024 * 1024;

/// Default tray icon: the app's 128×128 PNG embedded at compile time.
const DEFAULT_TRAY_ICON_PNG: &[u8] = include_bytes!("../../icons/128x128.png");

/// Rendered avatar icon assets.
pub struct AvatarIconAssets {
    pub tray_png: Vec<u8>,
    pub shortcut_ico: Vec<u8>,
}

/// Decode the normalized WebP avatar and render the tray PNG plus the
/// multi-size shortcut ICO.
pub fn render_avatar_assets(webp: &[u8]) -> Result<AvatarIconAssets, AppError> {
    let image = decode_avatar(webp)?;
    let tray_png = encode_png(&resize(&image, TRAY_ICON_SIZE))?;
    let mut shortcut_pngs = Vec::with_capacity(SHORTCUT_ICON_SIZES.len());
    for size in SHORTCUT_ICON_SIZES {
        shortcut_pngs.push((size, encode_png(&resize(&image, size))?));
    }
    Ok(AvatarIconAssets {
        tray_png,
        shortcut_ico: encode_png_ico(&shortcut_pngs)?,
    })
}

/// Write the rendered assets into `directory`; returns the tray PNG and ICO
/// paths.
pub fn write_avatar_assets(
    directory: &Path,
    assets: &AvatarIconAssets,
) -> Result<(PathBuf, PathBuf), AppError> {
    let tray_path = directory.join(TRAY_AVATAR_FILENAME);
    let ico_path = directory.join(SHORTCUT_AVATAR_FILENAME);
    fs::write(&tray_path, &assets.tray_png).map_err(icon_error)?;
    fs::write(&ico_path, &assets.shortcut_ico).map_err(icon_error)?;
    Ok((tray_path, ico_path))
}

/// Apply the avatar to the tray icon (and the desktop shortcut when it is
/// rendered from the same bytes). `webp: None` restores the default tray icon
/// and points the shortcut back at the application executable.
#[cfg(desktop)]
pub fn apply_avatar_icons<R: Runtime>(app: &AppHandle<R>, directory: &Path, webp: Option<&[u8]>) {
    match webp {
        Some(bytes) => match render_avatar_assets(bytes) {
            Ok(assets) => match write_avatar_assets(directory, &assets) {
                Ok((_, ico_path)) => {
                    set_tray_icon(app, Some(&assets.tray_png));
                    update_shortcut_icon(Some(&ico_path));
                }
                Err(_) => set_tray_icon(app, None),
            },
            Err(_) => set_tray_icon(app, None),
        },
        None => {
            set_tray_icon(app, None);
            update_shortcut_icon(None);
        }
    }
}

/// Apply the avatar only to the tray icon (used at startup, where touching
/// the shortcut is unnecessary and would be wasteful).
#[cfg(desktop)]
pub fn apply_tray_avatar<R: Runtime>(app: &AppHandle<R>, webp: Option<&[u8]>) {
    match webp {
        Some(bytes) => match render_avatar_assets(bytes) {
            Ok(assets) => set_tray_icon(app, Some(&assets.tray_png)),
            Err(_) => set_tray_icon(app, None),
        },
        None => set_tray_icon(app, None),
    }
}

#[cfg(desktop)]
fn set_tray_icon<R: Runtime>(app: &AppHandle<R>, png: Option<&[u8]>) {
    let owned = png.map(|bytes| bytes.to_vec());
    let app = app.clone();
    let _ = app.run_on_main_thread({
        let app = app.clone();
        move || {
            let Some(tray) = app.tray_by_id("aibb-tray") else {
                return;
            };
            let icon = owned
                .as_deref()
                .and_then(|bytes| Image::from_bytes(bytes).ok())
                .or_else(|| Image::from_bytes(DEFAULT_TRAY_ICON_PNG).ok());
            let _ = tray.set_icon(icon);
        }
    });
}

/// Point every existing `AIbb.lnk` desktop shortcut at `ico` (or back at the
/// executable default when `None`). Uses a tiny PowerShell WScript.Shell call;
/// any failure is ignored so icon polish never breaks the app.
#[cfg(windows)]
fn update_shortcut_icon(ico: Option<&Path>) {
    for shortcut in shortcut_candidates() {
        if !shortcut.exists() {
            continue;
        }
        let shortcut_ps = shortcut.to_string_lossy().replace('\'', "''");
        let icon_ps = ico
            .map(|path| path.to_string_lossy().replace('\'', "''"))
            .unwrap_or_default();
        let script = format!(
            "$sh = New-Object -ComObject WScript.Shell; \
             $sc = $sh.CreateShortcut('{shortcut_ps}'); \
             $sc.IconLocation = '{icon_ps}'; \
             $sc.Save()",
            shortcut_ps = shortcut_ps,
            icon_ps = icon_ps,
        );
        let _ = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
    }
}

#[cfg(not(windows))]
fn update_shortcut_icon(_ico: Option<&Path>) {}

#[cfg(windows)]
fn shortcut_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for home in [std::env::var_os("USERPROFILE"), std::env::var_os("PUBLIC")] {
        let Some(home) = home else { continue };
        let home = PathBuf::from(home);
        candidates.push(home.join("Desktop").join("AIbb.lnk"));
        if home.file_name().is_some_and(|name| name != "Public") {
            candidates.push(home.join("OneDrive").join("Desktop").join("AIbb.lnk"));
        }
    }
    candidates
}

fn resize(image: &image::DynamicImage, size: u32) -> image::RgbaImage {
    image.resize_exact(size, size, FilterType::Lanczos3).to_rgba8()
}

fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>, AppError> {
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    let mut buffer = Vec::new();
    PngEncoder::new(&mut buffer)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(icon_error)?;
    Ok(buffer)
}

/// Pack PNG-encoded frames into a single ICO container. Modern Windows
/// (Vista+) renders PNG-compressed icon entries, so no BMP conversion is
/// needed and arbitrary sizes are supported.
fn encode_png_ico(pngs: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, AppError> {
    if pngs.is_empty() || pngs.len() > u16::MAX as usize {
        return Err(icon_error("invalid icon size list"));
    }
    let mut ico = Vec::with_capacity(
        6 + 16 * pngs.len() + pngs.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
    );
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(pngs.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, bytes) in pngs {
        let dimension = (*size).min(256) as u8; // 0 means 256
        ico.extend_from_slice(&[dimension, dimension, 0, 0]); // width, height, colors, reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
        ico.extend_from_slice(&(bytes.len() as u32).to_le_bytes()); // bytes in resource
        ico.extend_from_slice(&offset.to_le_bytes()); // image offset
        offset += bytes.len() as u32;
    }
    for (_, bytes) in pngs {
        ico.extend_from_slice(bytes);
    }
    Ok(ico)
}

fn decode_avatar(bytes: &[u8]) -> Result<image::DynamicImage, AppError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::WebP);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_AVATAR_DIMENSION);
    limits.max_image_height = Some(MAX_AVATAR_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_AVATAR_BYTES);
    reader.limits(limits);
    reader.decode().map_err(|_| icon_error("avatar decode failed"))
}

fn icon_error(message: impl std::fmt::Display) -> AppError {
    AppError::new("avatar_icon_unavailable", message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    fn sample_webp() -> Vec<u8> {
        let image =
            image::RgbaImage::from_pixel(64, 64, image::Rgba([120, 90, 200, 255]));
        let mut buffer = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut buffer)
            .encode(image.as_raw(), 64, 64, ExtendedColorType::Rgba8)
            .unwrap();
        buffer
    }

    #[test]
    fn renders_a_32px_tray_png_and_a_multi_size_ico() {
        let assets = render_avatar_assets(&sample_webp()).unwrap();

        let tray = image::load_from_memory(&assets.tray_png).unwrap();
        assert_eq!((tray.width(), tray.height()), (32, 32));

        let ico = &assets.shortcut_ico;
        assert_eq!(&ico[0..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, SHORTCUT_ICON_SIZES.len());
        let mut seen_sizes = Vec::new();
        let mut offsets = Vec::new();
        for index in 0..count {
            let entry = 6 + index * 16;
            let dimension = ico[entry] as u32;
            let size = if dimension == 0 { 256 } else { dimension };
            let offset = u32::from_le_bytes([
                ico[entry + 12],
                ico[entry + 13],
                ico[entry + 14],
                ico[entry + 15],
            ]) as usize;
            assert_eq!(
                &ico[offset..offset + 4],
                &[0x89, b'P', b'N', b'G'],
                "icon entry {index} must embed a PNG frame"
            );
            seen_sizes.push(size);
            offsets.push(offset);
        }
        for (index, offset) in offsets.iter().enumerate() {
            let entry = 6 + index * 16;
            let bytes_in_res =
                u32::from_le_bytes([ico[entry + 8], ico[entry + 9], ico[entry + 10], ico[entry + 11]])
                    as usize;
            let next = offsets.get(index + 1).copied().unwrap_or(ico.len());
            assert_eq!(bytes_in_res, next - offset, "entry {index} size");
        }
        assert_eq!(seen_sizes, SHORTCUT_ICON_SIZES.to_vec());
    }

    #[test]
    fn invalid_webp_is_rejected_without_panicking() {
        assert!(render_avatar_assets(b"not an image").is_err());
    }
}
