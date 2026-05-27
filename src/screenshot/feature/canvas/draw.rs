use eframe::egui::{Painter, Pos2, Rect, Stroke};

use crate::screenshot::feature::canvas::phys_to_local;
use crate::screenshot::feature::screenshot::capture::{ScreenshotState, ScreenshotTool};
use crate::screenshot::feature::screenshot::draw::draw_egui_shape;

/// 渲染正在绘制中的预览（current_shape / current_pen）
///
/// 在用户拖拽绘制过程中实时显示预览效果：
/// - 几何形状（Rect/Circle/Arrow）：显示从起点到当前鼠标位置的形状预览
/// - 画笔/马赛克：显示已记录的轨迹点序列的实时预览
pub fn render_current_preview(
    painter: &Painter,
    state: &ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
    viewport_rect: Rect,
) {
    // 预览几何形状：当有起点和终点时绘制半透明的形状轮廓
    if let (Some(start_phys), Some(end_phys)) = (
        state.input.current_shape_start,
        state.input.current_shape_end,
    ) {
        let start_local = phys_to_local(start_phys, global_offset_phys, ppp);
        let end_local = phys_to_local(end_phys, global_offset_phys, ppp);
        let rect = Rect::from_two_pos(start_local, end_local);

        // 视口裁剪：仅当形状与可视区域相交时才渲染
        if viewport_rect.intersects(rect)
            && let Some(tool) = state.drawing.current_tool
        {
            draw_egui_shape(
                painter,
                tool,
                rect,
                start_local,
                end_local,
                state.drawing.stroke_width,
                state.drawing.active_color,
            );
        }
    }

    // 预览画笔/马赛克轨迹
    if !state.input.current_pen_points.is_empty() {
        if state.drawing.current_tool == Some(ScreenshotTool::Mosaic) {
            // 马赛克预览：实时采样原图并渲染马赛克方块
            crate::screenshot::feature::canvas::mosaic::draw_realtime_mosaic(
                painter,
                &state.input.current_pen_points,
                state.drawing.mosaic_width,
                global_offset_phys,
                ppp,
                state.select.selection,
                &state.capture.captures,
            );
        } else {
            // 画笔预览：将物理坐标点转换为本地坐标后绘制折线
            let mut local_points = Vec::with_capacity(state.input.current_pen_points.len());
            for p in &state.input.current_pen_points {
                local_points.push(phys_to_local(*p, global_offset_phys, ppp));
            }
            let stroke = Stroke::new(state.drawing.stroke_width, state.drawing.active_color);
            painter.add(eframe::egui::Shape::line(local_points, stroke));
        }
    }
}
