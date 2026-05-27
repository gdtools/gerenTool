use eframe::egui::{CursorIcon, Pos2, Rect, Ui};

use crate::screenshot::feature::{
    canvas::{CanvasState, hit_test, shape::ShapeRender},
    screenshot::capture::{DrawnShape, ScreenshotState, ScreenshotTool},
};

/// 检查指针是否悬停在工具栏等 UI 元素上
///
/// 当指针在工具栏矩形范围内时返回 true，
/// 用于阻止画布交互（避免在操作工具栏时触发画布事件）。
pub(super) fn check_hovering_ui(
    ui: &Ui,
    _state: &ScreenshotState,
    toolbar_rect: Option<Rect>,
) -> bool {
    if let Some(pos) = ui.pointer_latest_pos() {
        toolbar_rect.is_some_and(|r| r.contains(pos))
    } else {
        false
    }
}

/// 获取悬停的控制点索引（如果有选中的 shape）
///
/// 遍历选中图形的所有 resize 控制点，返回距离指针最近且在命中半径内的控制点索引。
pub(super) fn get_hovered_handle(
    local_pos: Pos2,
    shape: &DrawnShape,
    global_offset_phys: Pos2,
    ppp: f32,
) -> Option<usize> {
    let handles = shape.resize_handles(global_offset_phys, ppp);
    for (index, (handle_pos, hit_radius)) in handles.iter().enumerate() {
        if local_pos.distance(*handle_pos) <= *hit_radius {
            return Some(index);
        }
    }
    None
}

/// 更新悬停状态
///
/// 在鼠标未按下时执行命中检测，更新 canvas_state.hovered_shape：
/// 1. 优先检查控制点悬停（保持选中状态不变）
/// 2. 其次检查图形 body 悬停
/// 3. 在 UI 元素上或无指针时清除悬停状态
pub(super) fn update_hover_state(
    ui: &Ui,
    state: &ScreenshotState,
    canvas_state: &mut CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    is_hovering_ui: bool,
    is_pointer_down: bool,
) {
    // 鼠标按下时不更新悬停状态（保持拖拽开始时的状态）
    if !is_pointer_down {
        if let Some(pos) = ui.pointer_latest_pos() {
            if !is_hovering_ui {
                // 先检查是否悬停在选中图形的控制点上
                if let Some(selected_idx) = canvas_state.selected_shape
                    && let Some(shape) = state.edit.shapes.get(selected_idx)
                    && shape.supports_resize()
                    && let Some(_handle) = get_hovered_handle(pos, shape, global_offset_phys, ppp)
                {
                    // 悬停在控制点上时不更新 hovered_shape（保持选中高亮）
                    return;
                }

                // 检查图形 body 的命中测试
                canvas_state.hovered_shape = hit_test::get_hovered_shape_index(
                    pos,
                    &state.edit.shapes,
                    global_offset_phys,
                    ppp,
                    ui.painter(),
                );
            } else {
                canvas_state.hovered_shape = None;
            }
        } else {
            canvas_state.hovered_shape = None;
        }
    }
}

/// 更新光标图标
///
/// 根据当前交互上下文设置合适的鼠标光标：
/// - 在 UI 元素上 → Default
/// - 在控制点上 → 对应方向的 Resize 光标
/// - 在图形上 + Alt → Copy（复制拖拽）
/// - 在图形上 / 拖拽选区 → Move
/// - 其他 → Crosshair（十字准星，用于绘制）
pub(super) fn update_cursor(
    ui: &Ui,
    state: &ScreenshotState,
    canvas_state: &CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    is_hovering_ui: bool,
) {
    if is_hovering_ui {
        ui.set_cursor_icon(CursorIcon::Default);
        return;
    }

    // 检查是否悬停在选中图形的控制点上
    if let Some(pos) = ui.pointer_latest_pos()
        && let Some(selected_idx) = canvas_state.selected_shape
        && let Some(shape) = state.edit.shapes.get(selected_idx)
        && shape.supports_resize()
        && let Some(handle) = get_hovered_handle(pos, shape, global_offset_phys, ppp)
    {
        // 根据控制点索引和工具类型设置对应的 resize 光标
        let cursor = match shape.tool {
            ScreenshotTool::Arrow => {
                // 箭头：根据方向判断水平/垂直/对角线 resize
                let dx = (shape.end.x - shape.start.x).abs();
                let dy = (shape.end.y - shape.start.y).abs();
                if dx > dy * 2.0 {
                    CursorIcon::ResizeHorizontal
                } else if dy > dx * 2.0 {
                    CursorIcon::ResizeVertical
                } else {
                    let is_same_direction =
                        (shape.end.x - shape.start.x) * (shape.end.y - shape.start.y) >= 0.0;
                    if is_same_direction {
                        CursorIcon::ResizeNwSe
                    } else {
                        CursorIcon::ResizeNeSw
                    }
                }
            }
            _ => {
                // Rect/Circle/Text: 8 控制点对应的光标方向
                // 0=NW, 1=NE, 2=SE, 3=SW, 4=N, 5=E, 6=S, 7=W
                match handle {
                    0 | 2 => CursorIcon::ResizeNwSe,
                    1 | 3 => CursorIcon::ResizeNeSw,
                    4 | 6 => CursorIcon::ResizeVertical,
                    5 | 7 => CursorIcon::ResizeHorizontal,
                    _ => CursorIcon::Crosshair,
                }
            }
        };
        ui.set_cursor_icon(cursor);
        return;
    }

    let is_moving_state =
        canvas_state.hovered_shape.is_some() || canvas_state.dragging_shape.is_some();

    // 检测是否悬停在选区背景上（无图形时，用于拖拽选区）
    let mut is_hovering_selection_bg = false;
    if let Some(pos) = ui.pointer_latest_pos() {
        let global_phys = global_offset_phys + (pos.to_vec2() * ppp);
        if let Some(sel) = state.select.selection {
            is_hovering_selection_bg = sel.contains(global_phys)
                && canvas_state.hovered_shape.is_none()
                && state.edit.shapes.is_empty();
        }
    }

    let is_alt_down = ui.input(|i| i.modifiers.alt);

    // 综合判断光标类型
    let cursor = if canvas_state.hovered_shape.is_some() && is_alt_down {
        CursorIcon::Copy
    } else if (is_moving_state
        && state.input.current_shape_start.is_none()
        && state.input.current_pen_points.is_empty())
        || canvas_state.dragging_selection
        || (state.drawing.current_tool.is_none() && is_hovering_selection_bg)
    {
        CursorIcon::Move
    } else {
        CursorIcon::Crosshair
    };

    ui.set_cursor_icon(cursor);
}
