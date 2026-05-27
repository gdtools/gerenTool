use eframe::egui::{Rect, Vec2};

use crate::screenshot::feature::canvas::shape::ShapeRender;
use crate::screenshot::feature::screenshot::capture::DrawnShape;

/// 移动 shape，受选区边界限制
///
/// 计算图形在选区矩形内可移动的最大偏移量，防止图形被拖出选区范围。
/// 当选区为 None 时不做限制。返回实际应用的偏移量（可能与请求的 delta 不同）。
pub fn move_shape(shape: &mut DrawnShape, delta: Vec2, selection: Option<Rect>) -> Vec2 {
    let mut dx = delta.x;
    let mut dy = delta.y;

    if let Some(sel) = selection {
        let min_x = shape.start.x.min(shape.end.x);
        let max_x = shape.start.x.max(shape.end.x);
        let min_y = shape.start.y.min(shape.end.y);
        let max_y = shape.start.y.max(shape.end.y);

        // 水平方向：确保图形不超出选区左右边界
        if max_x - min_x <= sel.width() {
            if min_x + dx < sel.min.x {
                dx = sel.min.x - min_x;
            }
            if max_x + dx > sel.max.x {
                dx = sel.max.x - max_x;
            }
        }
        // 垂直方向：确保图形不超出选区上下边界
        if max_y - min_y <= sel.height() {
            if min_y + dy < sel.min.y {
                dy = sel.min.y - min_y;
            }
            if max_y + dy > sel.max.y {
                dy = sel.max.y - max_y;
            }
        }
    }

    let clamped = Vec2::new(dx, dy);
    shape.translate(clamped);
    clamped
}

/// 移动选区
///
/// 将整个选区矩形按给定的偏移量平移。
pub fn move_selection(selection: &mut Rect, delta: Vec2) {
    *selection = selection.translate(delta);
}
