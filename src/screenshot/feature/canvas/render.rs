use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Ui};

use crate::screenshot::feature::canvas::{
    ANCHOR_SIZE, CanvasState, OVERLAY_ALPHA, phys_rect_to_local, phys_to_local, shape::ShapeRender,
};
use crate::screenshot::feature::screenshot::capture::{ScreenshotState, ScreenshotTool};

/// 渲染画布所有元素
///
/// 画布渲染的总入口函数，按层次顺序渲染：
/// 1. 文本输入框（最顶层的 UI 元素）
/// 2. 遮罩层（选区外的半透明黑色覆盖）
/// 3. 已完成的图形列表（含马赛克特殊处理）
/// 4. 正在绘制中的预览
/// 5. 选中图形的控制点和边框
/// 6. 选区边框（绿色虚线 + 锚点）
pub fn render_canvas_elements(
    ui: &Ui,
    state: &mut ScreenshotState,
    canvas_state: &CanvasState,
    global_offset_phys: Pos2,
    ppp: f32,
    is_hovered: bool,
) {
    let painter = ui.painter();
    let viewport_rect = ui.viewport_rect();

    // 渲染遮罩层
    render_overlay(
        ui,
        painter,
        state,
        global_offset_phys,
        ppp,
        viewport_rect,
        is_hovered,
    );

    let hovered_index = canvas_state.hovered_shape;
    let dragging_index = canvas_state.dragging_shape;
    let selected_index = canvas_state.selected_shape;

    // 遍历所有已完成的图形进行渲染
    for (index, shape) in state.edit.shapes.iter_mut().enumerate() {
        // 视口裁剪：跳过不在可视区域内的图形
        let rect = shape.bounding_rect(global_offset_phys, ppp);
        let mut visible = viewport_rect.intersects(rect);

        // 文本类型使用排版后的精确尺寸做裁剪判定
        if shape.tool == ScreenshotTool::Text {
            let start_local = phys_to_local(shape.start, global_offset_phys, ppp);
            if let Some(galley) = shape.ensure_galley(painter) {
                let text_rect = Rect::from_min_size(start_local, galley.size());
                visible = viewport_rect.intersects(text_rect);
            }
        }

        if !visible {
            continue;
        }

        // 判断高亮状态：悬停/拖拽/选中的图形显示蓝色边框
        // 但正在绘制新图形时不显示高亮（避免视觉干扰）
        let is_highlighted = (Some(index) == hovered_index
            || Some(index) == dragging_index
            || Some(index) == selected_index)
            && state.input.current_shape_start.is_none();

        // 马赛克特殊处理：需要访问原图 captures 进行采样
        if shape.tool == ScreenshotTool::Mosaic {
            if let Some(points) = &shape.points {
                if let Some(ref cache) = shape.cached_mosaic {
                    // 使用缓存纹理渲染（高性能路径）
                    let local_min = phys_to_local(cache.phys_rect.min, global_offset_phys, ppp);
                    let local_max = phys_to_local(cache.phys_rect.max, global_offset_phys, ppp);
                    let local_rect = Rect::from_min_max(local_min, local_max);
                    painter.image(
                        cache.texture.id(),
                        local_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    // 实时渲染马赛克（首帧路径）
                    crate::screenshot::feature::canvas::mosaic::draw_realtime_mosaic(
                        painter,
                        points,
                        shape.stroke_width,
                        global_offset_phys,
                        ppp,
                        state.select.selection,
                        &state.capture.captures,
                    );
                    // 异步生成纹理缓存供下一帧使用
                    if let Some(cache) =
                        crate::screenshot::feature::canvas::mosaic::generate_mosaic_texture(
                            ui.ctx(),
                            points,
                            shape.stroke_width,
                            ppp,
                            state.select.selection,
                            &state.capture.captures,
                        )
                    {
                        shape.cached_mosaic = Some(std::sync::Arc::new(cache));
                    }
                }
            }
            // 马赛克的高亮边框
            if is_highlighted {
                let start_local = phys_to_local(shape.start, global_offset_phys, ppp);
                let end_local = phys_to_local(shape.end, global_offset_phys, ppp);
                let highlight_rect = Rect::from_two_pos(start_local, end_local);
                painter.rect_stroke(
                    highlight_rect.expand(2.0),
                    2.0,
                    Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 150, 255, 100)),
                    StrokeKind::Outside,
                );
            }
        } else {
            // 非马赛克图形使用统一的 render 方法
            shape.render(painter, global_offset_phys, ppp, is_highlighted);
        }
    }

    // 渲染正在绘制中的预览
    crate::screenshot::feature::canvas::draw::render_current_preview(
        painter,
        state,
        global_offset_phys,
        ppp,
        viewport_rect,
    );

    // 绘制选中图形的控制点和选中边框
    if let Some(selected_idx) = selected_index
        && let Some(shape) = state.edit.shapes.get(selected_idx)
        && shape.supports_resize()
    {
        // 蓝色实线选中边框
        let bbox = shape.bounding_rect(global_offset_phys, ppp);
        let selection_border_color = Color32::from_rgb(0, 150, 255);
        painter.rect_stroke(
            bbox.expand(2.0),
            2.0,
            Stroke::new(1.0, selection_border_color),
            StrokeKind::Outside,
        );

        // 白色填充 + 灰色描边的控制点
        let handles = shape.resize_handles(global_offset_phys, ppp);
        let handle_fill = Color32::WHITE;
        let handle_stroke = Stroke::new(1.0, Color32::from_rgb(60, 60, 60));

        for (local_pos, _) in handles {
            let rect = Rect::from_center_size(local_pos, eframe::egui::vec2(10.0, 10.0));
            painter.rect_filled(rect, 0.0, handle_fill);
            painter.rect_stroke(rect, 0.0, handle_stroke, StrokeKind::Inside);
        }
    }

    // 绘制选区边框（绿色虚线 + 锚点）
    render_selection_frame(painter, state, global_offset_phys, ppp, viewport_rect);
}

/// 绘制选区或悬浮窗口的遮罩
///
/// 遮罩渲染逻辑（按优先级）：
/// 1. 有选区时：选区外区域覆盖半透明黑色，选区内保持透明
/// 2. 无选区且无绘制操作时：
///    - 悬停在窗口上：该窗口区域保持透明，其余覆盖
///    - 悬停在屏幕上：显示屏幕边界框
///    - 其他情况：全屏覆盖
fn render_overlay(
    ui: &Ui,
    painter: &Painter,
    state: &ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
    viewport_rect: Rect,
    is_hovered: bool,
) {
    let overlay_color = Color32::from_rgba_unmultiplied(0, 0, 0, OVERLAY_ALPHA);

    // 有选区时的遮罩处理
    if let Some(global_sel_phys) = state.select.selection {
        let local_logical_rect = phys_rect_to_local(global_sel_phys, global_offset_phys, ppp);
        let clipped_local_sel = local_logical_rect.intersect(viewport_rect);

        if clipped_local_sel.is_positive() {
            // 选区在可视范围内：绘制四周遮罩 + 尺寸标签
            paint_selection_shade(painter, clipped_local_sel, viewport_rect, overlay_color);

            // 显示选区尺寸标签（宽 x 高）
            if viewport_rect.expand(1.0).contains(local_logical_rect.min) {
                let w = global_sel_phys.width().round() as u32;
                let h = global_sel_phys.height().round() as u32;
                let text = format!("{} x {}", w, h);
                let font_id = eframe::egui::FontId::proportional(12.0);
                let text_color = Color32::WHITE;
                let galley = painter.layout_no_wrap(text, font_id, text_color);
                let padding = eframe::egui::vec2(6.0, 4.0);
                let bg_size = galley.size() + padding * 2.0;

                // 标签默认在选区上方，空间不够时移到选区内
                let mut label_pos =
                    local_logical_rect.min - eframe::egui::vec2(0.0, bg_size.y + 5.0);
                if label_pos.y < viewport_rect.min.y {
                    label_pos = local_logical_rect.min + eframe::egui::vec2(5.0, 5.0);
                }

                let label_rect = Rect::from_min_size(label_pos, bg_size);
                painter.rect_filled(label_rect, 4.0, Color32::from_black_alpha(160));
                painter.galley(label_rect.min + padding, galley, text_color);
            }
        } else {
            // 选区完全不在可视范围内：全屏覆盖
            painter.rect_filled(viewport_rect, 0.0, overlay_color);
        }
        return;
    }

    // 无选区且无绘制操作时的遮罩处理
    if state.input.current_shape_start.is_none() && state.select.drag_start.is_none() {
        if is_hovered {
            if let Some(hover_phys_rect) = state.select.hovered_window {
                // 悬停在窗口上：高亮该窗口
                paint_hover_window_overlay(
                    painter,
                    hover_phys_rect,
                    global_offset_phys,
                    ppp,
                    viewport_rect,
                    overlay_color,
                );
            } else if let Some(pointer_pos) = ui.pointer_latest_pos() {
                // 悬停在屏幕上：显示屏幕边界框
                let global_pointer_phys = global_offset_phys + (pointer_pos.to_vec2() * ppp);
                if let Some(cap_phys_rect) = crate::screenshot::feature::canvas::find_target_screen_rect(
                    &state.capture.captures,
                    global_pointer_phys,
                ) {
                    let local_logical_rect =
                        phys_rect_to_local(cap_phys_rect, global_offset_phys, ppp);
                    paint_style_box(painter, local_logical_rect, 3.0);
                }
            }
        } else {
            // 鼠标不在画布上：全屏覆盖
            painter.rect_filled(viewport_rect, 0.0, overlay_color);
        }
    }
}

/// 绘制选区遮罩（选区外四周的半透明黑色区域）
fn paint_selection_shade(
    painter: &Painter,
    clipped_local_sel: Rect,
    viewport_rect: Rect,
    overlay_color: Color32,
) {
    paint_shade_around_rect(painter, clipped_local_sel, viewport_rect, overlay_color);
}

/// 绘制悬停窗口的遮罩效果
///
/// 在悬停窗口区域保持透明，四周覆盖遮罩，并绘制绿色边框 + 锚点。
fn paint_hover_window_overlay(
    painter: &Painter,
    hover_phys_rect: Rect,
    global_offset_phys: Pos2,
    ppp: f32,
    viewport_rect: Rect,
    overlay_color: Color32,
) {
    let local_logical_rect = phys_rect_to_local(hover_phys_rect, global_offset_phys, ppp);
    let clipped_local_sel = local_logical_rect.intersect(viewport_rect);

    if clipped_local_sel.is_positive() {
        paint_shade_around_rect(painter, clipped_local_sel, viewport_rect, overlay_color);
        paint_style_box(painter, clipped_local_sel, 2.0);
    }
}

/// 在高亮矩形四周绘制遮罩（上/下/左/右四个矩形区域）
fn paint_shade_around_rect(
    painter: &Painter,
    highlight_rect: Rect,
    viewport_rect: Rect,
    overlay_color: Color32,
) {
    for shade_rect in shade_rects_around(highlight_rect, viewport_rect) {
        if shade_rect.is_positive() {
            painter.rect_filled(shade_rect, 0.0, overlay_color);
        }
    }
}

/// 计算高亮矩形四周的四个遮罩矩形
///
/// 将视口分为上、下、左、右四个区域，中间留出高亮矩形：
/// ```text
/// ┌─────────────────────┐
/// │        top          │
/// ├────┬──────────┬─────┤
/// │left│ highlight│right│
/// ├────┴──────────┴─────┤
/// │       bottom        │
/// └─────────────────────┘
/// ```
fn shade_rects_around(highlight_rect: Rect, viewport_rect: Rect) -> [Rect; 4] {
    let top = Rect::from_min_max(
        viewport_rect.min,
        Pos2::new(viewport_rect.max.x, highlight_rect.min.y),
    );
    let bottom = Rect::from_min_max(
        Pos2::new(viewport_rect.min.x, highlight_rect.max.y),
        viewport_rect.max,
    );
    let left = Rect::from_min_max(
        Pos2::new(viewport_rect.min.x, highlight_rect.min.y),
        Pos2::new(highlight_rect.min.x, highlight_rect.max.y),
    );
    let right = Rect::from_min_max(
        Pos2::new(highlight_rect.max.x, highlight_rect.min.y),
        Pos2::new(viewport_rect.max.x, highlight_rect.max.y),
    );

    [top, bottom, left, right]
}

/// 绘制绿色检测框（边框 + 8 个锚点）
///
/// 用于选区边框和窗口悬停框的视觉样式：
/// - 绿色实线边框
/// - 8 个绿色填充锚点（4 角 + 4 边中点）
fn paint_style_box(painter: &Painter, rect: Rect, line_width: f32) {
    let anchor_size = ANCHOR_SIZE;
    let green = Color32::from_rgb(0, 255, 0);
    let main_stroke = Stroke::new(line_width, green);
    let anchor_stroke = Stroke::new(1.0, green);
    let anchor_fill = green;

    painter.rect_stroke(rect, 0.0, main_stroke, StrokeKind::Inside);

    // 只在矩形足够大时显示锚点（避免锚点重叠）
    if rect.width() > anchor_size * 3.0 && rect.height() > anchor_size * 3.0 {
        let inset = anchor_size / 2.0;
        let min = rect.min + eframe::egui::vec2(inset, inset);
        let max = rect.max - eframe::egui::vec2(inset, inset);
        let center = rect.center();

        // 8 个锚点位置：4 角 + 4 边中点
        let anchors = [
            min,
            Pos2::new(max.x, min.y),
            max,
            Pos2::new(min.x, max.y),
            Pos2::new(center.x, min.y),
            Pos2::new(center.x, max.y),
            Pos2::new(min.x, center.y),
            Pos2::new(max.x, center.y),
        ];

        for anchor_pos in anchors {
            let anchor_rect =
                Rect::from_center_size(anchor_pos, eframe::egui::vec2(anchor_size, anchor_size));
            painter.rect_filled(anchor_rect, 0.0, anchor_fill);
            painter.rect_stroke(anchor_rect, 0.0, anchor_stroke, StrokeKind::Inside);
        }
    }
}

/// 绘制选区边框
///
/// 当选区存在时，在选区边界绘制绿色检测框（细线版，line_width=1.0）。
fn render_selection_frame(
    painter: &Painter,
    state: &ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
    viewport_rect: Rect,
) {
    let Some(global_sel_phys) = state.select.selection else {
        return;
    };

    let local_logical_rect = phys_rect_to_local(global_sel_phys, global_offset_phys, ppp);
    let clipped_local_sel = local_logical_rect.intersect(viewport_rect);

    if clipped_local_sel.is_positive() {
        paint_style_box(painter, clipped_local_sel, 1.0);
    }
}
