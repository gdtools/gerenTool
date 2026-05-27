mod drag;
mod hover;

use eframe::egui::{Pos2, Rect, Response, Ui};

use crate::screenshot::feature::{
    canvas::{CanvasState, commit_text_shape, shape::ShapeRender},
    screenshot::capture::{ScreenshotAction, ScreenshotState, ScreenshotTool},
};
use crate::screenshot::feature::canvas::find_target_screen_rect;
use drag::{on_drag_start, on_drag_stop, on_dragged};
use hover::{check_hovering_ui, update_cursor, update_hover_state};

/// 处理所有画布交互
///
/// 画布交互的总入口，每帧调用一次，处理以下交互事件：
/// 1. 悬停状态更新（图形命中检测、控制点检测）
/// 2. 光标图标更新（根据上下文显示不同光标）
/// 3. 右键点击退出截图
/// 4. 左键点击（选中图形、选择窗口/屏幕、创建文本框）
/// 5. 拖拽操作（绘制形状、移动图形、调整选区、resize 控制点）
pub fn handle_interaction(
    ui: &mut Ui,
    state: &mut ScreenshotState,
    canvas_state: &mut CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    toolbar_rect: Option<Rect>,
) -> ScreenshotAction {
    // 注册整个画布区域的交互感知（点击和拖拽）
    let response = ui.interact(
        ui.max_rect(),
        ui.id().with("screenshot_background"),
        eframe::egui::Sense::click_and_drag(),
    );

    let is_pointer_down = ui.input(|i| i.pointer.primary_down());
    let is_hovering_ui = check_hovering_ui(ui, state, toolbar_rect);

    // 更新悬停状态（命中检测）
    update_hover_state(
        ui,
        state,
        canvas_state,
        global_offset_phys,
        ppp,
        is_hovering_ui,
        is_pointer_down,
    );

    // 更新光标图标
    update_cursor(
        ui,
        state,
        canvas_state,
        global_offset_phys,
        ppp,
        is_hovering_ui,
    );

    // 右键点击：在满足条件时退出截图
    if response.secondary_clicked()
        && can_exit_screenshot_on_secondary_click(state, canvas_state, is_hovering_ui)
    {
        return ScreenshotAction::Close;
    }

    // 双击选区 → 复制到剪贴板（等效于点击"复制"按钮）
    if response.double_clicked()
        && !is_hovering_ui
        && state.input.active_text_input.is_none()
        && state.has_positive_selection()
    {
        return ScreenshotAction::SaveToClipboard;
    }

    // 左键单击处理（双击时跳过，避免与 double_clicked 冲突）
    if response.clicked() && !response.double_clicked() {
        handle_click(
            ui,
            state,
            canvas_state,
            global_offset_phys,
            ppp,
            toolbar_rect,
            &response,
        );
    }

    // 拖拽事件处理（仅在指针不在工具栏 UI 上时）
    if let Some(press_pos) = response.interact_pointer_pos()
        && !is_hovering_ui
    {
        // 拖拽开始：使用鼠标按下的原始位置计算起始坐标
        if response.drag_started() {
            let start_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(press_pos);
            let start_global_phys = global_offset_phys + (start_pos.to_vec2() * ppp);
            on_drag_start(
                ui,
                state,
                canvas_state,
                start_global_phys,
                global_offset_phys,
                ppp,
                start_pos,
            );
        }
        // 拖拽进行中
        if response.dragged() {
            on_dragged(ui, state, canvas_state, global_offset_phys, ppp, press_pos);
        }
        // 拖拽结束
        if response.drag_stopped() {
            on_drag_stop(state, canvas_state);
        }
    }

    ScreenshotAction::None
}

/// 判断右键点击是否可以退出截图
///
/// 只有在以下所有条件都满足时才允许右键退出：
/// - 不在 UI 元素上
/// - 有选区且无图形
/// - 无正在进行的绘制或拖拽操作
fn can_exit_screenshot_on_secondary_click(
    state: &ScreenshotState,
    canvas_state: &CanvasState,
    is_hovering_ui: bool,
) -> bool {
    !is_hovering_ui
        && state.select.selection.is_some()
        && state.edit.shapes.is_empty()
        && state.input.active_text_input.is_none()
        && state.input.current_shape_start.is_none()
        && state.input.current_shape_end.is_none()
        && state.input.current_pen_points.is_empty()
        && state.select.drag_start.is_none()
        && !canvas_state.dragging_selection
        && canvas_state.dragging_shape.is_none()
        && canvas_state.dragging_handle.is_none()
}

/// 处理左键点击事件
///
/// 点击优先级：
/// 1. 点击在控制点上 → 不做操作（由拖拽处理）
/// 2. 点击在图形上 → 选中该图形
/// 3. 无工具时 → 选择窗口或屏幕区域
/// 4. 文本工具 → 创建文本输入框
fn handle_click(
    ui: &Ui,
    state: &mut ScreenshotState,
    canvas_state: &mut CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    toolbar_rect: Option<Rect>,
    response: &Response,
) {
    let is_hovering_ui = check_hovering_ui(ui, state, toolbar_rect);
    if is_hovering_ui {
        return;
    }

    // 检查是否点击在选中图形的控制点上（控制点交互由拖拽处理）
    if let Some(pos) = response.interact_pointer_pos()
        && let Some(selected_idx) = canvas_state.selected_shape
        && let Some(shape) = state.edit.shapes.get(selected_idx)
        && shape.supports_resize()
        && let Some(_handle) = hover::get_hovered_handle(pos, shape, global_offset_phys, ppp)
    {
        return;
    }

    let is_moving_state =
        canvas_state.hovered_shape.is_some() || canvas_state.dragging_shape.is_some();
    let can_draw = !is_moving_state && !canvas_state.dragging_selection;

    // 第一优先级：点击图形选中它（无论当前工具是什么）
    if let Some(hovered_idx) = canvas_state.hovered_shape {
        canvas_state.selected_shape = Some(hovered_idx);
        state.drawing.current_tool = None;
        return;
    }

    // 无工具时：选择窗口或屏幕区域，或取消选中
    if state.drawing.current_tool.is_none() {
        if !is_moving_state {
            canvas_state.selected_shape = None;
        }

        // 已有选区时不重新选择窗口/屏幕（防止单击重置选区）
        if state.select.selection.is_some() {
            return;
        }

        // 优先选择悬停的窗口
        if let Some(hovered) = state.select.hovered_window {
            state.set_selection(Some(hovered));
            return;
        } else if let Some(pointer_pos) = response.interact_pointer_pos() {
            // 其次选择包含点击位置的屏幕
            let global_phys = global_offset_phys + (pointer_pos.to_vec2() * ppp);
            if let Some(cap_phys_rect) =
                find_target_screen_rect(&state.capture.captures, global_phys)
            {
                state.set_selection(Some(cap_phys_rect));
                return;
            }
        }
    }

    // 文本工具点击：在选区内创建文本输入框
    if state.drawing.current_tool == Some(ScreenshotTool::Text)
        && can_draw
        && let Some(pos) = response.interact_pointer_pos()
    {
        let global_phys = global_offset_phys + (pos.to_vec2() * ppp);

        // 点击必须在选区内才允许创建文本框
        if let Some(sel) = state.select.selection
            && !sel.contains(global_phys)
        {
            return;
        }

        // 如果已有正在编辑的文本，先提交它
        if let Some((pos_old, text)) = state.input.active_text_input.take() {
            if !text.trim().is_empty() {
                commit_text_shape(ui, state, pos_old, text, global_offset_phys, ppp);
            }
        } else {
            // 创建新的文本输入框
            state.input.active_text_input = Some((global_phys, String::new()));
        }
    }
}
