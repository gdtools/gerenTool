use crate::screenshot::feature::{
    canvas::{
        CanvasState, ResizeStartState, drag,
        shape::{ShapeRender, clamp_pos_to_rect},
    },
    screenshot::capture::{DrawnShape, ScreenshotState, ScreenshotTool},
    screenshot::state::SelectionChangeOrigin,
};
use eframe::egui::{Pos2, Rect, Ui};
use std::sync::Arc;

use super::hover::get_hovered_handle;

/// 拖拽开始处理
///
/// 根据当前上下文决定拖拽操作的类型（按优先级）：
/// 1. 控制点拖拽 → 开始 resize
/// 2. 图形拖拽 → 开始移动（Alt+拖拽 = 复制后移动）
/// 3. 绘制工具 → 开始绘制形状/画笔/马赛克
/// 4. 选区背景拖拽 → 开始移动选区
/// 5. 空白区域拖拽 → 开始创建新选区
pub(super) fn on_drag_start(
    ui: &Ui,
    state: &mut ScreenshotState,
    canvas_state: &mut CanvasState,
    global_phys: Pos2,
    global_offset_phys: Pos2,
    ppp: f32,
    local_pos: Pos2,
) {

    if let Some(selected_idx) = canvas_state.selected_shape
        && let Some(shape) = state.edit.shapes.get(selected_idx)
        && shape.supports_resize()
        && let Some(handle) = get_hovered_handle(local_pos, shape, global_offset_phys, ppp)
    {
        let start = shape.start;
        let end = shape.end;
        // 记录 resize 前的形状状态（用于撤销）
        state.record_shape_before_edit(selected_idx);
        canvas_state.dragging_shape = Some(selected_idx);
        canvas_state.dragging_handle = Some(handle);
        canvas_state.resize_start_state = Some(ResizeStartState { start, end });
        return;
    }

    let interaction_hovered = canvas_state.hovered_shape;
    let is_moving_state =
        canvas_state.hovered_shape.is_some() || canvas_state.dragging_shape.is_some();
    let can_draw = !is_moving_state && !canvas_state.dragging_selection;

    // 检测是否在选区背景上开始拖拽
    let mut is_hovering_selection_bg = false;
    if let Some(sel) = state.select.selection {
        is_hovering_selection_bg = sel.contains(global_phys)
            && canvas_state.hovered_shape.is_none()
            && state.edit.shapes.is_empty();
    }

    if let Some(index) = interaction_hovered {
        // ========== 图形拖拽 ==========
        if ui.input(|i| i.modifiers.alt) {
            // Alt + 拖拽：克隆图形后拖拽副本
            let cloned_shape = state.edit.shapes[index].clone();
            state.edit.shapes.push(cloned_shape);

            let new_index = state.edit.shapes.len() - 1;
            state.record_shape_added(new_index);
            canvas_state.dragging_shape = Some(new_index);
            canvas_state.selected_shape = Some(new_index);
        } else {
            // 普通拖拽：记录拖拽前状态（用于撤销）
            state.record_shape_before_edit(index);
            canvas_state.dragging_shape = Some(index);
            canvas_state.selected_shape = Some(index);
        }

        canvas_state.drag_start_phys = Some(global_phys);
    } else if can_draw && state.drawing.current_tool.is_some() {
        // ========== 绘制操作 ==========
        if let Some(selection) = state.select.selection
            && selection.contains(global_phys)
            && state.drawing.current_tool != Some(ScreenshotTool::Text)
        {
            if state.drawing.current_tool == Some(ScreenshotTool::Pen)
                || state.drawing.current_tool == Some(ScreenshotTool::Mosaic)
            {
                // 画笔/马赛克：开始记录轨迹点
                state.input.current_pen_points.clear();
                state.input.current_pen_points.push(global_phys);
            } else {
                // 几何形状：记录起点
                state.input.current_shape_start = Some(global_phys);
                state.input.current_shape_end = Some(global_phys);
            }
        }
    } else if is_hovering_selection_bg && state.drawing.current_tool.is_none() {
        // ========== 选区拖拽 ==========
        canvas_state.dragging_selection = true;
        canvas_state.drag_start_phys = Some(global_phys);
        state.clear_toolbar();
    } else if can_draw {
        // ========== 创建新选区 ==========
        // 如果已有选区且其中有图形，不允许重新创建选区
        if let Some(sel) = state.select.selection
            && sel.contains(global_phys)
            && !state.edit.shapes.is_empty()
        {
            return;
        }
        state.input.selection_change_origin = Some(SelectionChangeOrigin {
            previous_selection: state.select.selection,
        });
        state.select.drag_start = Some(global_phys);
        state.clear_toolbar();
    }
}

/// 拖拽进行中处理
///
/// 根据当前拖拽类型更新对应状态：
/// - resize 控制点 → 调用 apply_resize 更新形状
/// - 图形拖拽 → 计算增量并移动图形
/// - 选区拖拽 → 平移选区矩形
/// - 画笔/马赛克 → 追加轨迹点
/// - 几何形状绘制 → 更新终点坐标
/// - 选区创建 → 更新选区矩形
pub(super) fn on_dragged(
    ui: &Ui,
    state: &mut ScreenshotState,
    canvas_state: &mut CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    _press_pos: Pos2,
) {
    // 获取当前鼠标的物理坐标
    let current_phys = ui
        .ctx()
        .pointer_latest_pos()
        .map(|pos| global_offset_phys + (pos.to_vec2() * ppp));

    let Some(current_phys) = current_phys else {
        return;
    };

    // resize 控制点拖拽
    if let (Some(shape_idx), Some(handle_idx), Some(start_state)) = (
        canvas_state.dragging_shape,
        canvas_state.dragging_handle,
        canvas_state.resize_start_state,
    ) {
        if let Some(shape) = state.edit.shapes.get_mut(shape_idx) {
            shape.apply_resize(
                handle_idx,
                current_phys,
                &start_state,
                state.select.selection,
            );
        }
    } else if let Some(index) = canvas_state.dragging_shape {
        // 图形移动拖拽
        if let Some(drag_start_phys) = canvas_state.drag_start_phys {
            let delta_phys = current_phys - drag_start_phys;
            if let Some(shape) = state.edit.shapes.get_mut(index) {
                let clamped = drag::move_shape(shape, delta_phys, state.select.selection);
                canvas_state.drag_start_phys = Some(drag_start_phys + clamped);
            }
        }
    } else if canvas_state.dragging_selection {
        // 选区整体拖拽
        if let (Some(drag_start_phys), Some(mut sel)) =
            (canvas_state.drag_start_phys, state.select.selection)
        {
            let delta_phys = current_phys - drag_start_phys;
            drag::move_selection(&mut sel, delta_phys);
            state.update_selection_only(Some(sel));
            canvas_state.drag_start_phys = Some(current_phys);
        }
    } else if (state.drawing.current_tool == Some(ScreenshotTool::Pen)
        || state.drawing.current_tool == Some(ScreenshotTool::Mosaic))
        && !state.input.current_pen_points.is_empty()
    {
        // 画笔/马赛克轨迹记录（最小距离过滤，避免过于密集的点）
        let mut clamped_phys = current_phys;
        if let Some(sel) = state.select.selection {
            clamped_phys = clamp_pos_to_rect(current_phys, sel);
        }
        if let Some(last) = state.input.current_pen_points.last()
            && last.distance(clamped_phys) > 2.0
        {
            state.input.current_pen_points.push(clamped_phys);
        }
    } else if state.input.current_shape_start.is_some() {
        // 几何形状绘制：更新终点（受选区边界约束）
        let mut clamped_phys = current_phys;
        if let Some(sel) = state.select.selection {
            clamped_phys = clamp_pos_to_rect(current_phys, sel);
        }
        state.input.current_shape_end = Some(clamped_phys);
    } else if let Some(drag_start_phys) = state.select.drag_start {
        // 选区创建：根据起点和当前位置构造选区矩形
        let rect = Rect::from_two_pos(drag_start_phys, current_phys);
        if state.select.selection != Some(rect) {
            state.update_selection_only(Some(rect));
        }
    }
}

/// 拖拽结束处理
///
/// 根据拖拽类型完成最终操作：
/// - 图形拖拽 → 清理拖拽状态
/// - 选区拖拽 → 同步工具栏位置
/// - 画笔/马赛克 → 提交为 DrawnShape
/// - 几何形状 → 提交为 DrawnShape（距离过短则丢弃）
/// - 选区创建 → 验证尺寸，过小则取消选区
pub(super) fn on_drag_stop(state: &mut ScreenshotState, canvas_state: &mut CanvasState) {
    if canvas_state.dragging_shape.is_some() {
        // 图形拖拽/resize 结束
        canvas_state.dragging_shape = None;
        canvas_state.drag_start_phys = None;
        canvas_state.dragging_handle = None;
        canvas_state.resize_start_state = None;
    } else if canvas_state.dragging_selection {
        // 选区拖拽结束
        canvas_state.dragging_selection = false;
        canvas_state.drag_start_phys = None;
        state.sync_toolbar_to_selection();
    } else if !state.input.current_pen_points.is_empty() {
        // 画笔/马赛克轨迹提交
        if state.input.current_pen_points.len() > 1 {
            // 计算轨迹的包围盒
            let mut min_pos = state.input.current_pen_points[0];
            let mut max_pos = state.input.current_pen_points[0];
            for p in &state.input.current_pen_points {
                min_pos = min_pos.min(*p);
                max_pos = max_pos.max(*p);
            }

            let Some(tool) = state.drawing.current_tool else {
                return;
            };
            let used_width = if tool == ScreenshotTool::Mosaic {
                state.drawing.mosaic_width
            } else {
                state.drawing.stroke_width
            };
            let points = Arc::new(std::mem::take(&mut state.input.current_pen_points));

            state.edit.shapes.push(DrawnShape::new(
                tool,
                min_pos,
                max_pos,
                state.drawing.active_color,
                used_width,
                None,
                Some(points),
            ));
            state.record_shape_added(state.edit.shapes.len() - 1);
        }
    } else if let Some(start_pos) = state.input.current_shape_start {
        // 几何形状提交（距离过短的丢弃，防止误触）
        let end_pos = state.input.current_shape_end.unwrap_or(start_pos);
        if start_pos.distance(end_pos) > 5.0
            && let Some(tool) = state.drawing.current_tool
        {
            state.edit.shapes.push(DrawnShape::new(
                tool,
                start_pos,
                end_pos,
                state.drawing.active_color,
                state.drawing.stroke_width,
                None,
                None,
            ));
            state.record_shape_added(state.edit.shapes.len() - 1);
        }
        state.input.current_shape_start = None;
        state.input.current_shape_end = None;
    } else if state.select.drag_start.take().is_some()
        && let Some(sel) = state.select.selection
    {
        // 选区创建完成
        let selection_origin = state.input.selection_change_origin.take();
        if sel.width() > 10.0 && sel.height() > 10.0 {
            // 选区尺寸有效：记录选区变更历史
            if let Some(origin) = selection_origin {
                let selection_changed = origin.previous_selection != Some(sel);
                if selection_changed || !state.edit.shapes.is_empty() {
                    state.record_selection_change(origin.previous_selection);
                }
            }

            // 重新选择区域时，清除已有图形
            if !state.edit.shapes.is_empty() {
                state.edit.shapes.clear();
                canvas_state.selected_shape = None;
            }
            state.sync_toolbar_to_selection();
        } else {
            // 选区太小：恢复之前的选区状态
            if let Some(origin) = selection_origin
                && origin.previous_selection.is_some()
            {
                state.record_selection_change(origin.previous_selection);
            }
            state.set_selection(None);
        }
    }
}
