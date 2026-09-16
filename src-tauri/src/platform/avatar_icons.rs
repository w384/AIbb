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
use tauri::{image::Image, AppHandle, Manager, Runtime};

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
/// multi-size shortcut ICO. The avatar is clipped to a circle (transparent
/// corners) so the pet identity stays round everywhere it appears — tray,
/// taskbar, shortcuts — matching the round pet itself.
pub fn render_avatar_assets(webp: &[u8]) -> Result<AvatarIconAssets, AppError> {
    let image = decode_avatar(webp)?;
    let tray_png = encode_png(&to_circle(&resize(&image, TRAY_ICON_SIZE)))?;
    let mut shortcut_frames = Vec::with_capacity(SHORTCUT_ICON_SIZES.len());
    for size in SHORTCUT_ICON_SIZES {
        shortcut_frames.push((size, to_circle(&resize(&image, size))));
    }
    Ok(AvatarIconAssets {
        tray_png,
        shortcut_ico: encode_png_ico(
            &shortcut_frames
                .iter()
                .map(|(size, frame)| (*size, frame))
                .collect::<Vec<_>>(),
        )?,
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
                    apply_window_icons(app, Some(&assets.tray_png));
                    #[cfg(windows)]
                    normalize_desktop_shortcut_name();
                    update_shortcut_icon(Some(&ico_path));
                }
                Err(_) => {
                    set_tray_icon(app, None);
                    apply_window_icons(app, None);
                }
            },
            Err(_) => {
                set_tray_icon(app, None);
                apply_window_icons(app, None);
            }
        },
        None => {
            set_tray_icon(app, None);
            apply_window_icons(app, None);
            update_shortcut_icon(None);
        }
    }
}

/// Point every open window's title-bar / taskbar icon at the avatar PNG so
/// the chat window (and any other window) matches the uploaded picture.
/// `png: None` restores the embedded default icon.
#[cfg(desktop)]
pub fn apply_window_icons<R: Runtime>(app: &AppHandle<R>, png: Option<&[u8]>) {
    let owned = png.map(|bytes| bytes.to_vec());
    let app = app.clone();
    let _ = app.run_on_main_thread({
        let app = app.clone();
        move || {
            let Some(icon) = owned
                .as_deref()
                .and_then(|bytes| Image::from_bytes(bytes).ok())
                .or_else(|| Image::from_bytes(DEFAULT_TRAY_ICON_PNG).ok())
            else {
                return;
            };
            for (_, window) in app.webview_windows() {
                let _ = window.set_icon(icon.clone());
            }
        }
    });
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
    let candidates: Vec<PathBuf> = shortcut_candidates()
        .into_iter()
        .filter(|path| path.exists())
        .collect();
    if candidates.is_empty() {
        return;
    }
    let icon_ps = ico
        .map(|path| path.to_string_lossy().replace('\'', "''"))
        .unwrap_or_default();
    // One PowerShell pass: re-point every shortcut at the avatar ICO, then tell
    // the shell the icons changed. Without the SHChangeNotify calls Windows
    // keeps showing the cached icon even after avatar.ico is rewritten — the
    // tray (a live PNG) updates, the desktop shortcut does not.
    let mut script = String::new();
    script.push_str(
        "Add-Type -TypeDefinition @\"\n\
         using System;\n\
         using System.Runtime.InteropServices;\n\
         public static class ShellIconRefresh {\n\
           [DllImport(\"shell32.dll\", CharSet = CharSet.Unicode)]\n\
           public static extern void SHChangeNotify(int wEventId, uint wFlags, IntPtr dwItem1, IntPtr dwItem2);\n\
         }\n\
         \"@;\n\
         $sh = New-Object -ComObject WScript.Shell;\n",
    );
    for shortcut in &candidates {
        let shortcut_ps = shortcut.to_string_lossy().replace('\'', "''");
        script.push_str(&format!(
            "$sc = $sh.CreateShortcut('{shortcut_ps}'); \
             $sc.IconLocation = '{icon_ps}'; \
             $sc.Save();\n",
            shortcut_ps = shortcut_ps,
            icon_ps = icon_ps,
        ));
        script.push_str(&format!(
            "$ptr = [System.Runtime.InteropServices.Marshal]::StringToHGlobalUni('{shortcut_ps}'); \
             [ShellIconRefresh]::SHChangeNotify(0x00000010, 0x0001, $ptr, [IntPtr]::Zero); \
             [System.Runtime.InteropServices.Marshal]::FreeHGlobal($ptr);\n",
            shortcut_ps = shortcut_ps,
        ));
    }
    script.push_str(
        "[ShellIconRefresh]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero);\n",
    );
    let _ = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
}

#[cfg(not(windows))]
fn update_shortcut_icon(_ico: Option<&Path>) {}

/// 换头像同步时，把桌面上指向本应用的快捷方式规范命名为 `AIbb.lnk`
/// （若尚不存在同名），让桌面图标名跟随产品名，而不是
/// `aibb-desktop-pet.exe - 快捷方式` 这类系统默认命名。
#[cfg(windows)]
fn normalize_desktop_shortcut_name() {
    let folders = [
        std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Desktop")),
        std::env::var_os("USERPROFILE")
            .map(|home| PathBuf::from(home).join("OneDrive").join("Desktop")),
    ];
    let mut already_named = false;
    let mut rename_target: Option<PathBuf> = None;
    for folder in folders.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase());
            match stem.as_deref() {
                Some("aibb") => already_named = true,
                Some(stem) if stem.contains("aibb") && rename_target.is_none() => {
                    rename_target = Some(path);
                }
                _ => {}
            }
        }
    }
    if !already_named {
        if let Some(path) = rename_target {
            if let Some(folder) = path.parent() {
                let _ = std::fs::rename(&path, folder.join("AIbb.lnk"));
            }
        }
    }
}

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
    // 开始菜单快捷方式（用户级 + 全体用户级）。
    if let Some(app_data) = std::env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("AIbb.lnk"),
        );
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        candidates.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("AIbb.lnk"),
        );
    }
    // 兜底：扫描桌面与开始菜单里所有文件名含 AIbb 的快捷方式（大小写不敏感，
    // 覆盖「aibb-desktop-pet.exe - 快捷方式」这类发送到桌面/安装器自定义命名），
    // 保证新装或改名后头像也能同步。
    for folder in [
        std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Desktop")),
        std::env::var_os("USERPROFILE")
            .map(|home| PathBuf::from(home).join("OneDrive").join("Desktop")),
        std::env::var_os("APPDATA").map(|app_data| {
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        }),
        std::env::var_os("PROGRAMDATA").map(|program_data| {
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        }),
    ] {
        let Some(folder) = folder else { continue };
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
                    && path.file_stem().is_some_and(|stem| {
                        stem.to_string_lossy()
                            .to_ascii_lowercase()
                            .contains("aibb")
                    })
                {
                    candidates.push(path);
                }
            }
        }
    }
    candidates
}

fn resize(image: &image::DynamicImage, size: u32) -> image::RgbaImage {
    image.resize_exact(size, size, FilterType::Lanczos3).to_rgba8()
}

/// Clip a square image to a circle: pixels outside the inscribed circle become
/// fully transparent, and the rim gets a 1px soft edge so scaled-down icons do
/// not show jagged corners.
fn to_circle(image: &image::RgbaImage) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let radius = width.min(height) as f64 / 2.0;
    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let mut out = image.clone();
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let dx = x as f64 + 0.5 - center_x;
        let dy = y as f64 + 0.5 - center_y;
        let distance = (dx * dx + dy * dy).sqrt();
        let alpha = if distance > radius {
            0.0
        } else {
            (radius - distance).min(1.0) // 1px antialiased rim
        };
        let a = pixel.0[3] as f64 * alpha;
        pixel.0[3] = a.round() as u8;
    }
    out
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

/// Pack the rendered frames into a single ICO container.
///
/// Frames are stored as 32-bit BGRA bitmaps (BITMAPINFOHEADER + XOR + AND
/// mask), not PNG: Windows' shortcut-icon pipeline handles PNG frames poorly —
/// the transparent corners of a round avatar come out as a square opaque
/// background — while 32-bit DIB frames natively render the alpha channel, so
/// the round avatar stays round on the desktop.
fn encode_png_ico(frames: &[(u32, &image::RgbaImage)]) -> Result<Vec<u8>, AppError> {
    if frames.is_empty() || frames.len() > u16::MAX as usize {
        return Err(icon_error("invalid icon size list"));
    }
    let encoded = frames
        .iter()
        .map(|(size, image)| Ok(encode_bmp_frame(*size, image)))
        .collect::<Result<Vec<_>, AppError>>()?;

    let mut ico = Vec::with_capacity(
        6 + 16 * frames.len() + encoded.iter().map(|bytes| bytes.len()).sum::<usize>(),
    );
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&(frames.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * frames.len() as u32;
    for ((size, _), bytes) in frames.iter().zip(&encoded) {
        let dimension = (*size).min(256) as u8; // 0 means 256
        ico.extend_from_slice(&[dimension, dimension, 0, 0]); // width, height, colors, reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
        ico.extend_from_slice(&(bytes.len() as u32).to_le_bytes()); // bytes in resource
        ico.extend_from_slice(&offset.to_le_bytes()); // image offset
        offset += bytes.len() as u32;
    }
    for bytes in &encoded {
        ico.extend_from_slice(bytes);
    }
    Ok(ico)
}

/// Encode one icon frame as a 32-bit DIB bitmap: 40-byte BITMAPINFOHEADER,
/// bottom-up BGRA rows, then an all-zero AND mask. The alpha channel travels
/// natively, so transparent corners survive Windows' icon pipeline.
fn encode_bmp_frame(size: u32, image: &image::RgbaImage) -> Vec<u8> {
    let mut bmp = Vec::with_capacity(40 + (size * size * 4) as usize + 0);
    bmp.extend_from_slice(&40u32.to_le_bytes()); // biSize
    bmp.extend_from_slice(&size.to_le_bytes()); // biWidth
    bmp.extend_from_slice(&(size * 2).to_le_bytes()); // biHeight: XOR + AND
    bmp.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    bmp.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    bmp.extend_from_slice(&[0; 20]); // biCompression(BI_RGB), biSizeImage, ppm, clr used/important
    for y in (0..size).rev() {
        for x in 0..size {
            let pixel = image.get_pixel(x, y);
            bmp.push(pixel.0[2]); // B
            bmp.push(pixel.0[1]); // G
            bmp.push(pixel.0[0]); // R
            bmp.push(pixel.0[3]); // A
        }
    }
    // AND mask: everything from the XOR plane is already alpha-driven.
    let and_row = ((size as usize + 31) / 32) * 4;
    bmp.extend(std::iter::repeat(0u8).take(and_row * size as usize));
    bmp
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
            // Every frame must be a 32-bit DIB bitmap, not a PNG: the shortcut
            // icon pipeline needs the native alpha channel to stay round.
            assert_eq!(
                u32::from_le_bytes([ico[offset], ico[offset + 1], ico[offset + 2], ico[offset + 3]]),
                40,
                "entry {index} must start with a BITMAPINFOHEADER"
            );
            assert_eq!(
                u16::from_le_bytes([ico[offset + 14], ico[offset + 15]]),
                32,
                "entry {index} must be 32-bit BGRA"
            );
            let xor_height = u32::from_le_bytes([
                ico[offset + 8],
                ico[offset + 9],
                ico[offset + 10],
                ico[offset + 11],
            ]) / 2;
            assert_eq!(xor_height, size, "entry {index} XOR height");
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
    fn avatar_assets_are_clipped_to_a_circle() {
        let assets = render_avatar_assets(&sample_webp()).unwrap();

        let tray = image::load_from_memory(&assets.tray_png)
            .unwrap()
            .to_rgba8();
        assert_eq!((tray.width(), tray.height()), (32, 32));
        // 中心像素保留不透明。
        assert_eq!(tray.get_pixel(16, 16).0[3], 255);
        // 四个角落完全透明：头像被剪裁成圆形。
        assert_eq!(tray.get_pixel(0, 0).0[3], 0);
        assert_eq!(tray.get_pixel(31, 0).0[3], 0);
        assert_eq!(tray.get_pixel(0, 31).0[3], 0);
        assert_eq!(tray.get_pixel(31, 31).0[3], 0);
        // 圆边缘像素被抗锯齿软化，不是生硬的 0/255 跳变。
        assert!(tray.get_pixel(0, 16).0[3] < 255);
    }

    #[test]
    fn invalid_webp_is_rejected_without_panicking() {
        assert!(render_avatar_assets(b"not an image").is_err());
    }
}
