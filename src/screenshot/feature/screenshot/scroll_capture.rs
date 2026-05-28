/// 滚动截图 + 延时截图模块
///
/// 滚动截图实现原理：
/// 1. 保持截图覆盖层全屏，但选区内部透明化，使用 Z-Order 遍历将滚轮事件穿透发送给下方窗口
/// 2. 模拟鼠标滚轮向下滚动，等待页面响应
/// 3. 用选区裁剪屏幕，通过比较相邻帧的重叠区域计算滚动偏移量
/// 4. 将新内容拼接到长图底部
/// 5. 重复直到检测到页面不再滚动、鼠标移出选区或点击停止按钮
/// 6. 合并后保存最终长图到桌面
///
/// 延时截图实现原理：
/// 1. 触发时保存选区矩形和已绘制形状
/// 2. 倒计时结束后，重新截屏所有显示器
/// 3. 用保存的选区裁剪截图，叠加形状，回传主线程创建置顶贴图
use crate::screenshot::feature::screenshot::draw::draw_skia_shapes_on_image;
use crate::screenshot::feature::screenshot::state::DrawnShape;
use eframe::egui::{Pos2, Rect};
use image::{GenericImage, GenericImageView, RgbaImage};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

/// 每次滚动的滚轮格数（负数表示向下）
pub const SCROLL_DELTA: i32 = -4;
/// 每次滚动后等待页面响应的时间（毫秒）
pub const SCROLL_WAIT_MS: u64 = 400;
/// 用于检测重叠区域的比较行数
const OVERLAP_SEARCH_ROWS: u32 = 80;
/// 像素差异阈值（0-255，超过此值认为像素不同）
const PIXEL_DIFF_THRESHOLD: u8 = 15;
/// 判断页面停止滚动的最小偏移量（像素）
pub const MIN_SCROLL_OFFSET: u32 = 5;

pub enum ScrollControlMessage {
    Stop,
}

pub enum ScrollResultMessage {
    Success(PathBuf),
    Error(String),
}

/// 启动滚动截图的后台控制线程
pub fn start_scroll_capture_thread(
    hwnd_usize: usize,
    selection_phys: Rect,
    control_rx: std::sync::mpsc::Receiver<ScrollControlMessage>,
    result_tx: std::sync::mpsc::Sender<ScrollResultMessage>,
) {
    thread::spawn(move || {
        // 自动将鼠标移动到框选区域的正中心位置
        let center = selection_phys.center();
        crate::screenshot::platform::current_platform().set_cursor_pos(center.x as i32, center.y as i32);

        // 稍微等待鼠标位置移动生效
        thread::sleep(Duration::from_millis(150));

        // 捕获当前选区作为第一帧
        let first_frame = match capture_selection(hwnd_usize, selection_phys) {
            Ok(f) => f,
            Err(e) => {
                let _ = result_tx.send(ScrollResultMessage::Error(e.to_string()));
                return;
            }
        };

        let mut long_image = first_frame.clone();
        let mut prev_frame = first_frame;

        loop {
            // 检测是否收到停止信号
            if let Ok(ScrollControlMessage::Stop) = control_rx.try_recv() {
                match save_long_image_to_desktop(&long_image) {
                    Ok(path) => {
                        let _ = result_tx.send(ScrollResultMessage::Success(path));
                    }
                    Err(e) => {
                        let _ = result_tx.send(ScrollResultMessage::Error(e.to_string()));
                    }
                }
                return;
            }

            // 检测鼠标是否移出框选区域，移出则停止滚动（暂停滚动，但不退出）
            let cursor_pos = crate::screenshot::platform::current_platform().get_cursor_pos();
            let cursor_point = Pos2::new(cursor_pos.0 as f32, cursor_pos.1 as f32);
            if !selection_phys.contains(cursor_point) {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            // 模拟滚轮向下滚动，穿透发送到底部窗口
            perform_scroll(hwnd_usize, SCROLL_DELTA);

            // 等待页面响应滚动
            thread::sleep(Duration::from_millis(SCROLL_WAIT_MS));

            // 检测是否在此期间收到停止信号，收到则不再进行截图和拼接，直接保存
            if let Ok(ScrollControlMessage::Stop) = control_rx.try_recv() {
                match save_long_image_to_desktop(&long_image) {
                    Ok(path) => {
                        let _ = result_tx.send(ScrollResultMessage::Success(path));
                    }
                    Err(e) => {
                        let _ = result_tx.send(ScrollResultMessage::Error(e.to_string()));
                    }
                }
                return;
            }

            // 再次捕获选区屏幕图像
            let new_frame = match capture_selection(hwnd_usize, selection_phys) {
                Ok(f) => f,
                Err(e) => {
                    let _ = result_tx.send(ScrollResultMessage::Error(e.to_string()));
                    return;
                }
            };

            // 比较并拼接到长图底部
            if let Some(_) = append_scroll_frame(&mut long_image, &prev_frame, &new_frame) {
                prev_frame = new_frame;
            } else {
                // 如果没有检测到滚动偏移（比如触底），我们暂停模拟滚动动作，但保持运行，直到用户点击红色停止图标
                thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

/// 捕获选区范围内的屏幕图像
///
/// 工作原理：
/// 1. 使用 Win32 API 暂时将覆盖层窗口移到屏幕外（避免覆盖层被截到）
/// 2. 等待合成器刷新后捕获选区
/// 3. 立即将覆盖层窗口移回原位
pub fn capture_selection(hwnd_usize: usize, selection_phys: Rect) -> Result<RgbaImage, Box<dyn std::error::Error + Send + Sync>> {
    // 暂时隐藏覆盖层窗口：移到屏幕外
    hide_overlay_for_capture(hwnd_usize);
    // 等待合成器/DWM 刷新，确保覆盖层已不可见
    thread::sleep(Duration::from_millis(80));

    let result = capture_selection_raw(selection_phys);

    // 立即恢复覆盖层窗口
    show_overlay_after_capture(hwnd_usize);

    result
}

/// 纯捕获逻辑（不涉及窗口隐藏/恢复）
fn capture_selection_raw(selection_phys: Rect) -> Result<RgbaImage, Box<dyn std::error::Error + Send + Sync>> {
    let monitors = xcap::Monitor::all()?;
    let center = selection_phys.center();

    let monitor = monitors
        .into_iter()
        .find(|m| {
            let x = m.x().unwrap_or(0) as f32;
            let y = m.y().unwrap_or(0) as f32;
            let w = m.width().unwrap_or(0) as f32;
            let h = m.height().unwrap_or(0) as f32;
            Rect::from_min_size(eframe::egui::pos2(x, y), eframe::egui::vec2(w, h)).contains(center)
        })
        .ok_or("未找到包含选区的显示器")?;

    let captured = monitor.capture_image()?;

    let mon_x = monitor.x().unwrap_or(0) as f32;
    let mon_y = monitor.y().unwrap_or(0) as f32;

    let crop_x = (selection_phys.min.x - mon_x).max(0.0).round() as u32;
    let crop_y = (selection_phys.min.y - mon_y).max(0.0).round() as u32;
    let crop_w = selection_phys.width().round() as u32;
    let crop_h = selection_phys.height().round() as u32;

    if crop_x + crop_w > captured.width() || crop_y + crop_h > captured.height() {
        return Err("选区超出显示器范围".into());
    }

    let cropped = image::imageops::crop_imm(&captured, crop_x, crop_y, crop_w, crop_h).to_image();
    Ok(cropped)
}

/// 将覆盖层窗口临时移到屏幕外，使 xcap 能捕获到真正的桌面内容
#[cfg(target_os = "windows")]
fn hide_overlay_for_capture(hwnd_usize: usize) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE,
    };

    let hwnd = HWND(hwnd_usize as *mut std::ffi::c_void);
    unsafe {
        // 保存当前窗口位置，以便之后精确恢复
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        SAVED_OVERLAY_POS.set(Some((rect.left, rect.top)));

        let _ = SetWindowPos(
            hwnd,
            None,
            -30000,
            -30000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// 将覆盖层窗口恢复到原来的位置（全屏覆盖）
#[cfg(target_os = "windows")]
fn show_overlay_after_capture(hwnd_usize: usize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        SetWindowPos, SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE,
    };

    let hwnd = HWND(hwnd_usize as *mut std::ffi::c_void);
    unsafe {
        // 优先使用之前保存的精确位置，否则回退到虚拟屏幕左上角
        let (x, y) = SAVED_OVERLAY_POS.take()
            .unwrap_or_else(|| {
                (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN))
            });
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

// 线程局部存储：保存覆盖层窗口隐藏前的位置
#[cfg(target_os = "windows")]
thread_local! {
    static SAVED_OVERLAY_POS: std::cell::Cell<Option<(i32, i32)>> = const { std::cell::Cell::new(None) };
}

#[cfg(not(target_os = "windows"))]
fn hide_overlay_for_capture(_hwnd_usize: usize) {
    // 非 Windows 平台暂不支持
}

#[cfg(not(target_os = "windows"))]
fn show_overlay_after_capture(_hwnd_usize: usize) {
    // 非 Windows 平台暂不支持
}

/// 通过重叠区域匹配，将新帧拼接在长图底部
pub fn append_scroll_frame(
    long_image: &mut RgbaImage,
    prev_frame: &RgbaImage,
    new_frame: &RgbaImage,
) -> Option<u32> {
    let selection_height = prev_frame.height();
    let selection_width = prev_frame.width();

    let scroll_offset = find_scroll_offset(prev_frame, new_frame, selection_height);

    if scroll_offset < MIN_SCROLL_OFFSET {
        tracing::info!("滚动偏移量 {} 小于最小阈值，停止拼接", scroll_offset);
        return None;
    }

    let new_content_y = selection_height.saturating_sub(scroll_offset);
    let new_content_height = scroll_offset.min(selection_height - new_content_y);

    if new_content_height == 0 {
        return None;
    }

    let old_height = long_image.height();
    let new_total_height = old_height + new_content_height;

    let mut extended = RgbaImage::new(selection_width, new_total_height);
    if extended.copy_from(long_image, 0, 0).is_err() {
        return None;
    }

    let new_content = new_frame
        .view(0, new_content_y, selection_width, new_content_height)
        .to_image();

    if extended.copy_from(&new_content, 0, old_height).is_err() {
        return None;
    }

    *long_image = extended;
    Some(scroll_offset)
}

/// 通过模板匹配找到两帧之间的垂直滚动偏移量
fn find_scroll_offset(prev_frame: &RgbaImage, new_frame: &RgbaImage, screen_height: u32) -> u32 {
    let width = prev_frame.width().min(new_frame.width());
    let template_rows = OVERLAP_SEARCH_ROWS.min(screen_height / 4);

    if template_rows == 0 || width == 0 {
        return 0;
    }

    let template_height = template_rows;

    let max_search_offset = screen_height.saturating_sub(template_height);
    let mut best_offset = 0u32;
    let mut best_score = u64::MAX;

    let coarse_step = 8u32;
    let mut coarse_best = 0u32;
    let mut coarse_best_score = u64::MAX;

    let mut y = 0u32;
    while y <= max_search_offset {
        let score = compute_row_diff(prev_frame, new_frame, y, 0, template_height, width);
        if score < coarse_best_score {
            coarse_best_score = score;
            coarse_best = y;
        }
        y += coarse_step;
    }

    let fine_start = coarse_best.saturating_sub(coarse_step);
    let fine_end = (coarse_best + coarse_step).min(max_search_offset);

    for y in fine_start..=fine_end {
        let score = compute_row_diff(prev_frame, new_frame, y, 0, template_height, width);
        if score < best_score {
            best_score = score;
            best_offset = y;
        }
    }

    let avg_diff_per_pixel = best_score / (template_height as u64 * width as u64).max(1);
    if avg_diff_per_pixel > PIXEL_DIFF_THRESHOLD as u64 {
        return 0;
    }

    best_offset
}

/// 计算旧帧与新帧之间的像素差异总和
fn compute_row_diff(
    prev_frame: &RgbaImage,
    new_frame: &RgbaImage,
    prev_y: u32,
    new_y: u32,
    rows: u32,
    width: u32,
) -> u64 {
    let mut total_diff: u64 = 0;
    let sample_step = 4u32;

    for row in 0..rows {
        let py = prev_y + row;
        let ny = new_y + row;

        if py >= prev_frame.height() || ny >= new_frame.height() {
            break;
        }

        let mut x = 0u32;
        while x < width {
            let p = prev_frame.get_pixel(x, py);
            let n = new_frame.get_pixel(x, ny);
            let dr = (p[0] as i32 - n[0] as i32).unsigned_abs() as u64;
            let dg = (p[1] as i32 - n[1] as i32).unsigned_abs() as u64;
            let db = (p[2] as i32 - n[2] as i32).unsigned_abs() as u64;
            total_diff += dr + dg + db;
            x += sample_step;
        }
    }

    total_diff
}

/// 模拟鼠标滚轮滚动，通过 Z-order 遍历将消息穿透发送给覆盖层底部的可见窗口
#[cfg(target_os = "windows")]
pub fn perform_scroll(hwnd_usize: usize, delta: i32) {
    use windows::Win32::Foundation::{HWND, RECT, POINT, WPARAM, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindow, GW_HWNDNEXT, IsWindowVisible, GetWindowRect, PostMessageW, WM_MOUSEWHEEL, GetCursorPos
    };

    let our_hwnd = HWND(hwnd_usize as *mut std::ffi::c_void);

    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }

    let mut current_hwnd = our_hwnd;
    unsafe {
        while let Ok(next_hwnd) = GetWindow(current_hwnd, GW_HWNDNEXT) {
            if next_hwnd.0.is_null() {
                break;
            }
            current_hwnd = next_hwnd;

            if !IsWindowVisible(current_hwnd).as_bool() {
                continue;
            }

            let mut rect = RECT::default();
            if GetWindowRect(current_hwnd, &mut rect).is_ok() {
                if pt.x >= rect.left && pt.x < rect.right && pt.y >= rect.top && pt.y < rect.bottom {
                    // 找到光标处直接位于覆盖层底部的窗口，向其非阻塞投递 WM_MOUSEWHEEL
                    let wheel_delta = (delta * 120) as i16;
                    let wparam = WPARAM(((wheel_delta as u32) << 16) as usize);
                    let lparam = LPARAM(((pt.y as u32) << 16 | (pt.x as u32 & 0xFFFF)) as isize);
                    let _ = PostMessageW(Some(current_hwnd), WM_MOUSEWHEEL, wparam, lparam);
                    break;
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn perform_scroll(_hwnd_usize: usize, _delta: i32) {
    tracing::warn!("当前平台不支持模拟滚轮");
}

/// 将长图保存到桌面，返回保存的绝对路径
pub fn save_long_image_to_desktop(long_image: &RgbaImage) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let path = build_output_path("SCROLL");
    long_image.save(&path)?;
    Ok(path)
}

/// 构建输出文件路径
fn build_output_path(prefix: &str) -> PathBuf {
    let desktop = get_desktop_path();
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let tz_offset = get_local_tz_offset_secs();
    let local_secs = secs as i64 + tz_offset;
    let (y, mo, d, h, mi, s) = secs_to_datetime(local_secs);
    let filename = format!(
        "{prefix}_{:04}-{:02}-{:02}_{:02}_{:02}_{:02}.png",
        y, mo, d, h, mi, s
    );
    desktop.join(filename)
}

/// 获取桌面路径
#[cfg(target_os = "windows")]
fn get_desktop_path() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) {
            Ok(pwstr) => {
                let raw = pwstr.0;
                let mut len = 0usize;
                while *raw.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(raw, len);
                let path_str = String::from_utf16_lossy(slice);
                CoTaskMemFree(Some(pwstr.0 as *const _));
                PathBuf::from(&path_str)
            }
            Err(_) => std::env::var("USERPROFILE")
                .map(|p| PathBuf::from(p).join("Desktop"))
                .unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_desktop_path() -> PathBuf {
    std::env::var("HOME")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 获取本地时区偏移秒数
#[cfg(target_os = "windows")]
fn get_local_tz_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut tzi = TIME_ZONE_INFORMATION::default();
        GetTimeZoneInformation(&mut tzi);
        -(tzi.Bias as i64) * 60
    }
}

#[cfg(not(target_os = "windows"))]
fn get_local_tz_offset_secs() -> i64 {
    0
}

/// 将 Unix 时间戳（秒）转换为 (年, 月, 日, 时, 分, 秒)
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

/// 延时截图完成后回传给主线程的数据
pub struct DelayCaptureOutput {
    /// 裁剪后的截图图像
    pub image: RgbaImage,
    /// 贴图显示位置（逻辑坐标）
    pub pos: Pos2,
}

/// 启动延时截图后台流程
pub fn start_delay_capture(
    selection: Option<Rect>,
    shapes: Vec<DrawnShape>,
    pixels_per_point: f32,
    tx: Sender<Result<DelayCaptureOutput, String>>,
) {
    thread::spawn(move || {
        let result = run_delay_capture(selection, shapes, pixels_per_point)
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
}

/// 延时截图主流程
fn run_delay_capture(
    selection: Option<Rect>,
    shapes: Vec<DrawnShape>,
    pixels_per_point: f32,
) -> Result<DelayCaptureOutput, Box<dyn std::error::Error + Send + Sync>> {
    let selection_phys = match selection {
        Some(r) if r.is_positive() => r,
        _ => return Err("延时截图：无有效选区".into()),
    };

    // 等待截图覆盖层完全消失
    thread::sleep(Duration::from_millis(400));

    // 捕获所有显示器
    let monitors = xcap::Monitor::all()?;

    let final_width = selection_phys.width().round() as u32;
    let final_height = selection_phys.height().round() as u32;
    if final_width == 0 || final_height == 0 {
        return Err("延时截图：选区尺寸为零".into());
    }

    let mut final_image = RgbaImage::new(final_width, final_height);

    for monitor in &monitors {
        let mon_x = monitor.x().unwrap_or(0) as f32;
        let mon_y = monitor.y().unwrap_or(0) as f32;
        let mon_w = monitor.width().unwrap_or(0) as f32;
        let mon_h = monitor.height().unwrap_or(0) as f32;
        if mon_w == 0.0 || mon_h == 0.0 {
            continue;
        }
        let monitor_rect = Rect::from_min_size(
            eframe::egui::pos2(mon_x, mon_y),
            eframe::egui::vec2(mon_w, mon_h),
        );

        let intersection = selection_phys.intersect(monitor_rect);
        if !intersection.is_positive() {
            continue;
        }

        let captured = monitor.capture_image()?;

        let crop_x = (intersection.min.x - mon_x).max(0.0).round() as u32;
        let crop_y = (intersection.min.y - mon_y).max(0.0).round() as u32;
        let crop_w = intersection.width().round() as u32;
        let crop_h = intersection.height().round() as u32;

        if crop_x + crop_w > captured.width() || crop_y + crop_h > captured.height() {
            continue;
        }

        let cropped = image::imageops::crop_imm(&captured, crop_x, crop_y, crop_w, crop_h)
            .to_image();
        let paste_x = (intersection.min.x - selection_phys.min.x).max(0.0).round() as u32;
        let paste_y = (intersection.min.y - selection_phys.min.y).max(0.0).round() as u32;
        let _ = final_image.copy_from(&cropped, paste_x, paste_y);
    }

    draw_skia_shapes_on_image(&mut final_image, &shapes, selection_phys);

    let ppp = pixels_per_point.max(1.0);
    let pos = eframe::egui::pos2(selection_phys.min.x / ppp, selection_phys.min.y / ppp);

    Ok(DelayCaptureOutput {
        image: final_image,
        pos,
    })
}
