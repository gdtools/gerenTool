use crate::screenshot::feature::screenshot::capture::{CapturedScreen, ScreenshotState};
use crate::screenshot::model::device::MonitorInfo;
use crate::screenshot::platform::current_platform;
use eframe::egui::{ColorImage, Context, Pos2, Rect};
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;
use xcap::Monitor;

/// 处理截图捕获过程
///
/// 在后台线程中执行屏幕和窗口捕获，通过 mpsc 通道将结果传回主线程。
/// 主线程每帧调用此函数检查通道，收到结果后创建 GPU 纹理。
///
/// 返回 true 表示应该退出截图模式（通道断开等异常情况）
pub(super) fn handle_capture_process(
    ctx: &Context,
    screenshot_state: &mut ScreenshotState,
) -> bool {
    // 首次调用：启动后台捕获线程
    if !screenshot_state.capture.is_capturing {
        screenshot_state.capture.is_capturing = true;

        ctx.request_repaint();

        let (tx, rx) = channel();
        screenshot_state.capture.capture_receiver = Some(rx);
        let ctx_clone = ctx.clone();

        thread::spawn(move || {
            // 延迟 150ms 确保窗口已缩小/隐藏，避免截到自身
            thread::sleep(Duration::from_millis(150));
            tracing::debug!("Capturing screens and windows in background...");

            // 捕获所有显示器的屏幕图像
            let mut captures = Vec::new();
            if let Ok(monitors) = Monitor::all() {
                for monitor in monitors {
                    let Ok(image) = monitor.capture_image() else {
                        continue;
                    };

                    let info = MonitorInfo {
                        name: monitor.name().unwrap_or_default(),
                        x: monitor.x().unwrap_or(0),
                        y: monitor.y().unwrap_or(0),
                        width: monitor.width().unwrap_or(0),
                        height: monitor.height().unwrap_or(0),
                    };

                    if info.width == 0 || info.height == 0 {
                        continue;
                    }

                    captures.push(CapturedScreen {
                        raw_image: Arc::new(image),
                        screen_info: info,
                    });
                }
            }

            // 捕获所有可见窗口的矩形区域（用于智能选区吸附）
            let mut window_rects = Vec::new();
            if let Ok(windows) = xcap::Window::all() {
                for w in windows {
                    if !w.is_minimized().unwrap_or(true) {
                        let app_name = w.app_name().unwrap_or_default().to_lowercase();
                        // 排除自身窗口
                        if app_name.contains("cloverviewer") || app_name.contains("screenshot") {
                            continue;
                        }

                        let rect = Rect::from_min_size(
                            Pos2::new(w.x().unwrap_or(0) as f32, w.y().unwrap_or(0) as f32),
                            egui::vec2(
                                w.width().unwrap_or(0) as f32,
                                w.height().unwrap_or(0) as f32,
                            ),
                        );
                        if rect.width() > 50.0 && rect.height() > 50.0 {
                            window_rects.push(rect);
                        }
                    }
                }
            }

            // 使用系统 API 捕获任务栏矩形
            let taskbars = current_platform().get_taskbar_rects();
            window_rects.extend(taskbars);

            let _ = tx.send((captures, window_rects));
            ctx_clone.request_repaint();
        });
    }

    // 检查异步捕获结果
    if let Some(rx) = &screenshot_state.capture.capture_receiver {
        match rx.try_recv() {
            Ok((captures, window_rects)) => {
                // 为每个屏幕创建或更新 GPU 纹理
                for cap in &captures {
                    let monitor_name = &cap.screen_info.name;
                    let texture_options = Default::default();
                    let color_image = ColorImage::from_rgba_unmultiplied(
                        [
                            cap.raw_image.width() as usize,
                            cap.raw_image.height() as usize,
                        ],
                        cap.raw_image.as_raw(),
                    );
                    if let Some(texture) =
                        screenshot_state.capture.texture_pool.get_mut(monitor_name)
                    {
                        texture.set(color_image, texture_options);
                    } else {
                        let texture = ctx.load_texture(
                            format!("screenshot_{}", monitor_name),
                            color_image,
                            texture_options,
                        );
                        screenshot_state
                            .capture
                            .texture_pool
                            .insert(monitor_name.clone(), texture);
                    }
                }

                screenshot_state.capture.captures = captures;
                screenshot_state.capture.window_rects = window_rects;
                screenshot_state.capture.is_capturing = false;
                screenshot_state.capture.capture_receiver = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                // 仍在等待，16ms 后再次检查（约 60fps）
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            Err(TryRecvError::Disconnected) => {
                // 通道断开，退出截图模式
                screenshot_state.capture.is_capturing = false;
                screenshot_state.capture.capture_receiver = None;
                return true;
            }
        }
    }
    false
}
