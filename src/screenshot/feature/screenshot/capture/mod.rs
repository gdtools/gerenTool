mod actions;
mod capture_impl;

use crate::screenshot::feature::canvas::{self, CanvasState};
use crate::screenshot::feature::screenshot::toolbar::{
    calculate_toolbar_rect, render_toolbar_and_overlays,
};
use crate::screenshot::model::device::DeviceInfo;
use crate::screenshot::model::state::CommonState;
use crate::screenshot::platform::current_platform;
use eframe::egui::{Color32, Context, Pos2, Rect, Ui, ViewportCommand};
use eframe::emath::Vec2;
use egui::WindowLevel;
pub use actions::extract_cropped_image;

/// wgpu 表面最大尺寸限制（物理像素）
const MAX_SURFACE_EXTENT_PHYS: f32 = 8192.0;

// 重新导出 state 模块的类型，方便外部直接使用
pub use crate::screenshot::feature::screenshot::state::{
    CapturedScreen, DrawnShape, ScreenshotAction, ScreenshotState, ScreenshotTool, WindowPrevState,
};

/// 窗口隐藏时的屏幕外位置
fn hidden_window_pos() -> Pos2 {
    Pos2::new(-20000.0, -20000.0)
}

/// 限制视口尺寸不超过 wgpu 表面最大值
fn clamp_viewport_extent(physical_size: Vec2, ppp: f32) -> Vec2 {
    let max_logical_extent = MAX_SURFACE_EXTENT_PHYS / ppp.max(1.0);
    Vec2::new(
        physical_size.x.min(MAX_SURFACE_EXTENT_PHYS) / ppp,
        physical_size.y.min(MAX_SURFACE_EXTENT_PHYS) / ppp,
    )
    .max(Vec2::new(1.0, 1.0))
    .min(Vec2::new(max_logical_extent, max_logical_extent))
}

/// 处理屏幕捕获阶段
///
/// 当尚无捕获数据时，启动异步屏幕捕获流程。
/// 返回 true 表示仍处于捕获阶段（尚未完成）。
fn handle_capture_stage(
    ctx: &Context,
    is_active: &mut bool,
    screenshot_state: &mut ScreenshotState,
) -> bool {
    if !screenshot_state.capture.captures.is_empty() {
        return false;
    }

    // 首次进入捕获阶段：缩小窗口并移到屏幕外，避免遮挡截图
    if !screenshot_state.capture.is_capturing {
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::ZERO));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(hidden_window_pos()));
    }

    let should_exit = capture_impl::handle_capture_process(ctx, screenshot_state);
    if should_exit {
        *is_active = false;
    }

    true
}

/// 配置截图视口窗口
///
/// 将窗口调整为覆盖所有显示器的全屏透明无边框窗口：
/// 1. 计算所有捕获屏幕的联合物理边界
/// 2. 设置窗口位置、尺寸、透明度和层级
/// 3. 验证 DPI 稳定性（尺寸匹配后锁定鼠标）
fn configure_screenshot_viewport(
    ctx: &Context,
    screenshot_state: &mut ScreenshotState,
    hwnd_usize: usize,
) {
    let ppp = ctx.pixels_per_point();

    // 计算所有屏幕的联合物理边界
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for cap in &screenshot_state.capture.captures {
        let info = &cap.screen_info;
        let phys_x = info.x as f32;
        let phys_y = info.y as f32;
        let phys_w = info.width as f32;
        let phys_h = info.height as f32;

        min_x = min_x.min(phys_x);
        min_y = min_y.min(phys_y);
        max_x = max_x.max(phys_x + phys_w);
        max_y = max_y.max(phys_y + phys_h);
    }

    let total_phys_width = (max_x - min_x + 100.0).max(1.0);
    let total_phys_height = (max_y - min_y + 100.0).max(1.0);
    let exact_logical_pos = Pos2::new(min_x / ppp, min_y / ppp);
    let requested_phys_size = Vec2::new(total_phys_width, total_phys_height);
    let exact_logical_size = clamp_viewport_extent(requested_phys_size, ppp);

    // 计算当前窗口的实际物理尺寸
    let viewport = ctx.input(|i| i.viewport().clone());
    let current_phys_size = viewport
        .inner_rect
        .map(|r| r.size() * ppp)
        .unwrap_or_default();

    // 判断是否需要调整（容差 5.0 避免微小的边框差异导致死循环）
    let needs_resize = !screenshot_state.runtime.window_configured
        || (current_phys_size.x - requested_phys_size.x).abs() > 5.0
        || (current_phys_size.y - requested_phys_size.y).abs() > 5.0;

    // 尺寸匹配且 DPI 稳定，锁定鼠标并返回
    if !needs_resize {
        if !screenshot_state.runtime.window_configured {
            current_platform().lock_cursor_for_screenshot();
        }
        return;
    }

    if requested_phys_size.x > MAX_SURFACE_EXTENT_PHYS
        || requested_phys_size.y > MAX_SURFACE_EXTENT_PHYS
    {
        tracing::warn!(
            requested_width = requested_phys_size.x,
            requested_height = requested_phys_size.y,
            clamped_width = exact_logical_size.x * ppp,
            clamped_height = exact_logical_size.y * ppp,
            "Screenshot viewport exceeded wgpu surface extent and was clamped"
        );
    }

    // 配置窗口为全屏透明覆盖层
    ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
    ctx.send_viewport_cmd(ViewportCommand::Transparent(true));
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(exact_logical_size));
    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(exact_logical_pos));
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(exact_logical_size));

    screenshot_state.runtime.window_configured = true;

    // 阻止 Alt 键激活 Windows 系统菜单（否则会破坏截图覆盖层状态）
    #[cfg(windows)]
    crate::screenshot::platform::windows::suppress_alt_menu_activation(hwnd_usize);

    // 强制请求重绘，以便在下一帧检查 DPI 是否发生漂移并再次修正
    ctx.request_repaint();
}

/// 确定截图完成后的窗口恢复目标状态
fn resolve_effective_prev_state(
    _ctx: &Context,
    _action: ScreenshotAction,
    _screenshot_state: &ScreenshotState,
) -> WindowPrevState {
    // 纯截图工具：截图完成后始终隐藏窗口回到后台
    WindowPrevState::Tray
}

/// 截图完成后恢复窗口状态
///
/// 根据截图前的窗口状态（正常/最小化/托盘），将窗口恢复到正确位置
fn restore_window_after_screenshot(
    ctx: &Context,
    common: &CommonState,
    effective_prev_state: WindowPrevState,
) {
    current_platform().unlock_cursor();

    // 恢复 Alt 键的默认 Windows 系统行为
    #[cfg(windows)]
    crate::screenshot::platform::windows::remove_alt_menu_suppression(
        common.window_state.hwnd_usize,
    );

    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(Vec2::ZERO));

    match effective_prev_state {
        WindowPrevState::Tray | WindowPrevState::Normal => {
            if let Ok(mut visible) = common.window_state.visible.lock() {
                *visible = false;
            }
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(hidden_window_pos()));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::ZERO));
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        WindowPrevState::Minimized => {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    }
}

/// 判断动作是否会结束截图流程
///
/// SaveAs 和 PinToTop 不在此判断为 closing：
/// - SaveAs 由调用方（ScreenshotFeature）延迟到 logic() 阶段处理
/// - PinToTop 由调用方单独处理（创建置顶视口后再以 Close 结束）
fn is_closing_action(action: ScreenshotAction) -> bool {
    matches!(
        action,
        ScreenshotAction::Close | ScreenshotAction::SaveAndClose | ScreenshotAction::SaveToClipboard | ScreenshotAction::SaveAs
    )
}

/// 处理截图完成动作（保存 + 窗口恢复 + 状态清理）
///
/// 仅处理会结束截图流程的动作（Close / SaveAndClose / SaveToClipboard）
fn handle_completion_action(
    ctx: &Context,
    screenshot_state: &mut ScreenshotState,
    common: &CommonState,
    action: ScreenshotAction,
) {
    actions::handle_save_action(action, screenshot_state);

    let effective_prev_state = resolve_effective_prev_state(ctx, action, screenshot_state);
    restore_window_after_screenshot(ctx, common, effective_prev_state);
    *screenshot_state = ScreenshotState::default();
    ctx.request_repaint();
}

/// 截图准备的主入口
///
/// 每帧在 logic() 中调用，依次执行：
/// 1. 屏幕捕获（异步线程，缩小窗口避免遮挡）
/// 2. 视口配置（全屏透明覆盖）
///
/// 返回 true 表示准备完成可以渲染 UI，false 表示仍在准备中
pub fn prepare_screenshot_frame(
    ctx: &Context,
    is_active: &mut bool,
    screenshot_state: &mut ScreenshotState,
    common: &CommonState,
) -> bool {
    if !*is_active {
        return false;
    }

    if handle_capture_stage(ctx, is_active, screenshot_state) {
        return false;
    }

    configure_screenshot_viewport(ctx, screenshot_state, common.window_state.hwnd_usize);

    true
}

/// 截图完成动作的主入口
///
/// 仅处理会结束截图流程的动作（Close / SaveAndClose / SaveToClipboard）。
/// SaveAs 和 PinToTop 由调用方（ScreenshotFeature）自行处理，不应传入此函数。
pub fn finalize_screenshot_action(
    ctx: &Context,
    screenshot_state: &mut ScreenshotState,
    common: &CommonState,
    action: ScreenshotAction,
) {
    if is_closing_action(action) {
        handle_completion_action(ctx, screenshot_state, common, action);
    }
}

/// 绘制截图 UI 的主入口
///
/// 在 CentralPanel 内部调用，渲染完整的截图界面：
/// 1. 屏幕截图纹理
/// 2. 窗口悬停高亮
/// 3. 画布交互和渲染
/// 4. 工具栏
/// 5. 键盘快捷键处理（Ctrl+Z/Y, Enter, Escape）
pub fn draw_screenshot_ui_inside(
    ui: &mut Ui,
    state: &mut ScreenshotState,
    device_info: &DeviceInfo,
) -> ScreenshotAction {
    let mut action = ScreenshotAction::None;
    let ctx = ui.ctx().clone();

    let global_offset_phys =
        Pos2::new(device_info.phys_min_x as f32, device_info.phys_min_y as f32);
    let ppp = ctx.pixels_per_point();

    let painter = ui.painter();

    // 1. 绘制所有屏幕的截图纹理
    for cap in &state.capture.captures {
        if let Some(texture) = state.capture.texture_pool.get(&cap.screen_info.name) {
            let rect = device_info.screen_logical_rect(&cap.screen_info, ppp);

            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    // 2. 窗口悬停检测（无选区且无拖拽时生效）
    state.select.hovered_window = None;
    let is_hovered = ui.rect_contains_pointer(ui.max_rect());

    if is_hovered
        && state.select.selection.is_none()
        && state.select.drag_start.is_none()
        && let Some(pointer_pos) = ui.pointer_latest_pos()
    {
        let global_pointer_phys = global_offset_phys + (pointer_pos.to_vec2() * ppp);

        for rect in &state.capture.window_rects {
            if rect.contains(global_pointer_phys) {
                // 排除全屏窗口（与屏幕尺寸一致的窗口）
                let mut is_fullscreen = false;
                for cap in &state.capture.captures {
                    if (rect.width() - cap.screen_info.width as f32).abs() < 5.0
                        && (rect.height() - cap.screen_info.height as f32).abs() < 5.0
                    {
                        is_fullscreen = true;
                        break;
                    }
                }
                if !is_fullscreen {
                    state.select.hovered_window = Some(*rect);
                }
                break;
            }
        }
    }

    // 3. 画布交互和渲染
    let local_toolbar_rect = calculate_toolbar_rect(state, global_offset_phys, ppp);

    let mut canvas_state = CanvasState::load_from_ui(ui);
    let interaction_action = canvas::interaction::handle_interaction(
        ui,
        state,
        &mut canvas_state,
        global_offset_phys,
        ppp,
        local_toolbar_rect,
    );
    canvas::render::render_canvas_elements(
        ui,
        state,
        &canvas_state,
        global_offset_phys,
        ppp,
        is_hovered,
    );
    canvas::text_input::render_text_input(ui, state, global_offset_phys, ppp);

    // Delete 键：删除当前选中的图形（文本输入状态下不触发）
    if state.input.active_text_input.is_none()
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
        && let Some(selected_idx) = canvas_state.selected_shape
        && selected_idx < state.edit.shapes.len()
    {
        let removed_shape = state.edit.shapes.remove(selected_idx);
        state.record_shape_removed(selected_idx, removed_shape);
        canvas_state.selected_shape = None;
        canvas_state.hovered_shape = None;
        canvas_state.dragging_shape = None;
    }

    canvas_state.save_to_ui(ui);

    if interaction_action != ScreenshotAction::None {
        action = interaction_action;
    }

    // 4. 工具栏渲染
    if let Some(rect) = local_toolbar_rect
        && ui.clip_rect().intersects(rect)
    {
        let toolbar_act = render_toolbar_and_overlays(ui, state, rect);
        if toolbar_act != ScreenshotAction::None {
            action = toolbar_act;
        }
    }

    // 5. 键盘快捷键
    let undo_requested = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z));
    let redo_requested = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Y));

    if undo_requested {
        state.undo_last();
    }

    if redo_requested {
        state.redo_last();
    }

    // Enter 键：复制到剪贴板（需要有有效选区且不在文本输入中）
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let can_save_to_clipboard = state.has_positive_selection();

        if can_save_to_clipboard && state.input.active_text_input.is_none() {
            action = ScreenshotAction::SaveToClipboard;
        }
    }

    // Escape 键：关闭截图
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = ScreenshotAction::Close;
    }

    if matches!(
        action,
        ScreenshotAction::SaveAs
            | ScreenshotAction::SaveAndClose
            | ScreenshotAction::SaveToClipboard
            | ScreenshotAction::PinToTop
    ) {
        canvas::finalize_pending_edits(ui, state, global_offset_phys, ppp);
    }

    ctx.request_repaint();

    action
}
