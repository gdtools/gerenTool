use crate::screenshot::feature::screenshot::capture::{ScreenshotAction, ScreenshotState};
use crate::screenshot::feature::screenshot::draw::draw_skia_shapes_on_image;
use crate::screenshot::model::device::get_screen_phys_rect;
use arboard::{Clipboard, ImageData};
use image::{GenericImage, RgbaImage};
use std::borrow::Cow;
use std::path::PathBuf;
use std::thread;

/// 获取 Windows 桌面路径
///
/// 优先使用 SHGetKnownFolderPath API（最准确），
/// 回退到 USERPROFILE\Desktop，再回退到当前目录。
#[cfg(target_os = "windows")]
fn get_desktop_path() -> PathBuf {
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT, FOLDERID_Desktop};
    use windows::Win32::System::Com::CoTaskMemFree;

    unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) {
            Ok(pwstr) => {
                // PWSTR 是 *mut u16，手动转为 OsString
                let raw = pwstr.0;
                let mut len = 0usize;
                while *raw.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(raw, len);
                let path_str = String::from_utf16_lossy(slice);
                CoTaskMemFree(Some(pwstr.0 as *const _));
                let p = PathBuf::from(&path_str);
                eprintln!("[screenshot] Desktop path (SHGetKnownFolderPath): {:?}", p);
                p
            }
            Err(e) => {
                eprintln!("[screenshot] SHGetKnownFolderPath failed: {:?}, falling back to USERPROFILE", e);
                std::env::var("USERPROFILE")
                    .map(|p| PathBuf::from(p).join("Desktop"))
                    .unwrap_or_else(|_| PathBuf::from("."))
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_desktop_path() -> PathBuf {
    std::env::var("HOME")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 获取本地时区相对于 UTC 的偏移秒数（正数表示东区）
#[cfg(target_os = "windows")]
fn get_local_tz_offset_secs() -> i64 {
    use windows::Win32::System::Time::GetTimeZoneInformation;
    use windows::Win32::System::Time::TIME_ZONE_INFORMATION;
    unsafe {
        let mut tzi = TIME_ZONE_INFORMATION::default();
        GetTimeZoneInformation(&mut tzi);
        // Bias 是"本地时间落后 UTC 的分钟数"，取反得到偏移
        -(tzi.Bias as i64) * 60
    }
}

#[cfg(not(target_os = "windows"))]
fn get_local_tz_offset_secs() -> i64 {
    0
}

/// 将 Unix 时间戳（秒）转换为 (年, 月, 日, 时, 分, 秒)（Howard Hinnant 算法）
fn secs_to_datetime(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    let sec_of_day = secs.rem_euclid(86400);
    let h = (sec_of_day / 3600) as u32;
    let mi = ((sec_of_day % 3600) / 60) as u32;
    let s = (sec_of_day % 60) as u32;

    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y as u32, mo, d, h, mi, s)
}

/// 处理截图保存动作
///
/// 根据动作类型，在后台线程中执行保存到文件或复制到剪贴板。
/// 使用后台线程避免阻塞 UI 线程（图像编码和剪贴板操作可能较慢）。
///
/// 注意：SaveAs 不在此处处理（由 capture/mod.rs 在 UI 线程中处理，因为需要弹文件对话框）
pub(super) fn handle_save_action(
    final_action: ScreenshotAction,
    screenshot_state: &mut ScreenshotState,
) {
    if final_action == ScreenshotAction::SaveAndClose
        || final_action == ScreenshotAction::SaveToClipboard
        || final_action == ScreenshotAction::SaveAs
    {
        if let Some(final_image) = extract_cropped_image(screenshot_state) {
            thread::spawn(move || {
                if final_action == ScreenshotAction::SaveAndClose {
                    // 保存到桌面的 PNG 文件，文件名格式：PIC_YYYY-MM-DD_HH_MM_SS.png
                    let desktop = get_desktop_path();
                    // 使用本地时间格式化文件名
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let tz_offset_secs = get_local_tz_offset_secs();
                    let local_secs = secs as i64 + tz_offset_secs;
                    let (y, mo, d, h, mi, s) = secs_to_datetime(local_secs);
                    let filename = format!(
                        "PIC_{:04}-{:02}-{:02}_{:02}_{:02}_{:02}.png",
                        y, mo, d, h, mi, s
                    );
                    let path = desktop.join(&filename);
                    eprintln!("[screenshot] Saving to: {:?}", path);
                    match final_image.save(&path) {
                        Ok(_) => {
                            eprintln!("[screenshot] Save OK: {:?}", path);
                            tracing::info!("Saved to {:?}", path);
                        }
                        Err(e) => {
                            eprintln!("[screenshot] Save FAILED: {} | path={:?}", e, path);
                            tracing::error!("Save failed: {}", e);
                        }
                    }
                } else if final_action == ScreenshotAction::SaveToClipboard
                    && let Ok(mut clipboard) = Clipboard::new()
                {
                    // 复制图像到系统剪贴板
                    let image_data = ImageData {
                        width: final_image.width() as usize,
                        height: final_image.height() as usize,
                        bytes: Cow::from(final_image.into_raw()),
                    };
                    if let Err(e) = clipboard.set_image(image_data) {
                        tracing::error!("Failed to copy image to clipboard: {}", e);
                    } else {
                        tracing::info!("Copied image to clipboard.");
                    }
                } else if final_action == ScreenshotAction::SaveAs {
                    // 弹出文件保存对话框并保存，默认文件名与"保存到桌面"规则相同
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let tz_offset_secs = get_local_tz_offset_secs();
                    let local_secs = secs as i64 + tz_offset_secs;
                    let (y, mo, d, h, mi, s) = secs_to_datetime(local_secs);
                    let default_name = format!(
                        "PIC_{:04}-{:02}-{:02}_{:02}_{:02}_{:02}.png",
                        y, mo, d, h, mi, s
                    );
                    let file = rfd::FileDialog::new()
                        .set_file_name(&default_name)
                        .add_filter("PNG 图片", &["png"])
                        .add_filter("JPEG 图片", &["jpg", "jpeg"])
                        .add_filter("BMP 图片", &["bmp"])
                        .save_file();
                    if let Some(path) = file {
                        if let Err(e) = final_image.save(&path) {
                            tracing::error!("SaveAs failed: {}", e);
                        } else {
                            tracing::info!("SaveAs saved to {:?}", path);
                        }
                    }
                }
            });
        }
    }
}



/// 从截图状态中提取裁剪后的最终图像
///
/// 根据选区矩形，从多屏幕捕获中裁剪出对应区域并拼接，
/// 然后在上方叠加所有绘图形状（使用 Tiny-Skia 高质量渲染）。
///
/// 返回 None 当无有效选区或选区尺寸为零时
pub fn extract_cropped_image(screenshot_state: &ScreenshotState) -> Option<RgbaImage> {
    let selection_phys = screenshot_state.select.selection?;
    if !selection_phys.is_positive() {
        return None;
    }

    let final_width = selection_phys.width().round() as u32;
    let final_height = selection_phys.height().round() as u32;
    if final_width == 0 || final_height == 0 {
        return None;
    }

    let mut final_image = RgbaImage::new(final_width, final_height);

    // 从每个屏幕捕获中裁剪与选区相交的部分，拼接到最终图像
    for cap in &screenshot_state.capture.captures {
        let monitor_rect_phys = get_screen_phys_rect(&cap.screen_info);
        let intersection = selection_phys.intersect(monitor_rect_phys);
        if !intersection.is_positive() {
            continue;
        }

        let crop_x = (intersection.min.x - monitor_rect_phys.min.x)
            .max(0.0)
            .round() as u32;
        let crop_y = (intersection.min.y - monitor_rect_phys.min.y)
            .max(0.0)
            .round() as u32;
        let crop_w = intersection.width().round() as u32;
        let crop_h = intersection.height().round() as u32;

        if crop_x + crop_w > cap.raw_image.width() || crop_y + crop_h > cap.raw_image.height() {
            continue;
        }

        let cropped_part =
            image::imageops::crop_imm(&*cap.raw_image, crop_x, crop_y, crop_w, crop_h).to_image();
        let paste_x = (intersection.min.x - selection_phys.min.x).max(0.0).round() as u32;
        let paste_y = (intersection.min.y - selection_phys.min.y).max(0.0).round() as u32;
        let _ = final_image.copy_from(&cropped_part, paste_x, paste_y);
    }

    // 在裁剪后的图像上叠加绘图形状
    draw_skia_shapes_on_image(
        &mut final_image,
        &screenshot_state.edit.shapes,
        selection_phys,
    );
    Some(final_image)
}

