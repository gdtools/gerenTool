use eframe::egui::{Painter, Pos2};

use crate::screenshot::feature::canvas::shape::ShapeRender;
use crate::screenshot::feature::screenshot::capture::DrawnShape;

/// 获取命中的 shape 索引（倒序遍历，后绘制的优先）
///
/// 从图形列表末尾向前遍历，第一个通过 hit_test 的图形即为命中目标。
/// 这确保了后绘制的（视觉上在顶层的）图形优先被选中。
pub fn get_hovered_shape_index(
    pos: Pos2,
    shapes: &[DrawnShape],
    global_offset_phys: Pos2,
    ppp: f32,
    painter: &Painter,
) -> Option<usize> {
    shapes.iter().enumerate().rev().find_map(|(index, shape)| {
        if shape.hit_test(pos, global_offset_phys, ppp, painter) {
            Some(index)
        } else {
            None
        }
    })
}
