use crate::screenshot::feature::canvas::mosaic::apply_mosaic_to_cropped_image;
use crate::screenshot::feature::screenshot::capture::{DrawnShape, ScreenshotTool};
use ab_glyph::{FontRef, PxScale};
use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2};
use image::{Rgba, RgbaImage};
use std::sync::LazyLock;

/// 系统字体路径（微软雅黑 TTC）
const SYSTEM_FONT_PATH: &str = r"C:\Windows\Fonts\msyh.ttc";

/// 导出图像时使用的系统字体（延迟加载，仅在实际需要文本渲染时读取磁盘）
static EXPORT_FONT: LazyLock<Option<Vec<u8>>> = LazyLock::new(|| {
    match std::fs::read(SYSTEM_FONT_PATH) {
        Ok(data) => Some(data),
        Err(err) => {
            tracing::error!("Failed to load system font for export: {}", err);
            None
        }
    }
});

/// 渲染 UI 时的实时绘图 (Egui)
///
/// 在截图模式的 UI 渲染阶段调用，使用 Egui 的 Painter 绘制形状预览。
/// 支持矩形、椭圆、箭头三种几何形状，文本/画笔/马赛克由画布模块单独处理。
pub fn draw_egui_shape(
    painter: &Painter,
    tool: ScreenshotTool,
    rect: Rect,
    start: Pos2,
    end: Pos2,
    stroke_width: f32,
    color: Color32,
) {
    match tool {
        ScreenshotTool::Rect => {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(stroke_width, color),
                StrokeKind::Outside,
            );
        }
        ScreenshotTool::Circle => {
            painter.add(Shape::ellipse_stroke(
                rect.center(),
                rect.size() / 2.0,
                Stroke::new(stroke_width, color),
            ));
        }
        ScreenshotTool::Arrow => {
            draw_arrow_egui(painter, start, end, stroke_width, color);
        }
        ScreenshotTool::Text | ScreenshotTool::Pen | ScreenshotTool::Mosaic => {}
    }
}

/// 计算箭头头部两翼的顶点坐标
///
/// 根据箭头方向、终点位置和描边宽度，计算箭头三角形两翼的坐标。
/// 当起点和终点重合（方向为零向量）时返回 None。
fn compute_arrow_head(start: Pos2, end: Pos2, stroke_width: f32) -> Option<[Pos2; 2]> {
    let dir = (end - start).normalized();
    if dir == Vec2::ZERO {
        return None;
    }
    let arrow_size = 12.0 + stroke_width * 2.0;
    let perp = Vec2::new(dir.y, -dir.x) * arrow_size * 0.5;
    Some([end - dir * arrow_size + perp, end - dir * arrow_size - perp])
}

/// 绘制箭头 (Egui)
///
/// 使用三段线段组成箭头：主线 + 箭头两翼
fn draw_arrow_egui(painter: &Painter, start: Pos2, end: Pos2, stroke_width: f32, color: Color32) {
    let stroke = Stroke::new(stroke_width, color);

    // 绘制主线
    painter.line_segment([start, end], stroke);

    let Some([p1, p2]) = compute_arrow_head(start, end, stroke_width) else {
        return;
    };

    // 绘制箭头头部两翼
    painter.line_segment([end, p1], stroke);
    painter.line_segment([end, p2], stroke);
}

/// 绘制箭头 (Tiny-Skia)
///
/// 使用 Tiny-Skia 矢量库绘制高质量抗锯齿箭头，用于导出图像
fn draw_arrow_skia(
    pixmap: &mut tiny_skia::PixmapMut,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    paint: &tiny_skia::Paint,
    stroke: &tiny_skia::Stroke,
) {
    let transform = tiny_skia::Transform::identity();
    let start = pos2(start_x, start_y);
    let end = pos2(end_x, end_y);

    // 主线
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(start_x, start_y);
    pb.line_to(end_x, end_y);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, paint, stroke, transform, None);
    }

    let Some([p1, p2]) = compute_arrow_head(start, end, stroke.width) else {
        return;
    };

    // 箭头左翼
    let mut pb1 = tiny_skia::PathBuilder::new();
    pb1.move_to(end_x, end_y);
    pb1.line_to(p1.x, p1.y);
    if let Some(path1) = pb1.finish() {
        pixmap.stroke_path(&path1, paint, stroke, transform, None);
    }

    // 箭头右翼
    let mut pb2 = tiny_skia::PathBuilder::new();
    pb2.move_to(end_x, end_y);
    pb2.line_to(p2.x, p2.y);
    if let Some(path2) = pb2.finish() {
        pixmap.stroke_path(&path2, paint, stroke, transform, None);
    }
}

/// 导出图片时的抗锯齿高质量绘图（最终合成）
///
/// 在截图保存时调用，使用 Tiny-Skia 渲染几何图形和画笔，
/// 使用 imageproc + ab_glyph 渲染文本，确保导出图像质量高于 UI 实时预览。
///
/// 渲染顺序：
/// 1. 马赛克（直接操作像素）
/// 2. 几何图形和画笔（Tiny-Skia 矢量渲染）
/// 3. 文本（imageproc 光栅化）
pub fn draw_skia_shapes_on_image(
    final_image: &mut RgbaImage,
    shapes: &[DrawnShape],
    selection_phys: Rect,
) {
    let final_width = final_image.width();
    let final_height = final_image.height();

    // 先渲染马赛克（直接操作像素，不受矢量渲染影响）
    for shape in shapes.iter().filter(|s| s.tool == ScreenshotTool::Mosaic) {
        if let Some(points) = &shape.points {
            apply_mosaic_to_cropped_image(final_image, points, shape.stroke_width, selection_phys);
        }
    }

    // 使用 Tiny-Skia 渲染几何图形和画笔
    if let Some(mut pixmap) =
        tiny_skia::PixmapMut::from_bytes(final_image, final_width, final_height)
    {
        for shape in shapes {
            if shape.tool == ScreenshotTool::Text || shape.tool == ScreenshotTool::Mosaic {
                continue;
            }

            // 将物理坐标转换为相对于选区起点的局部坐标
            let start_x = shape.start.x - selection_phys.min.x;
            let start_y = shape.start.y - selection_phys.min.y;
            let end_x = shape.end.x - selection_phys.min.x;
            let end_y = shape.end.y - selection_phys.min.y;

            let x0 = start_x.min(end_x);
            let y0 = start_y.min(end_y);
            let width = (start_x - end_x).abs();
            let height = (start_y - end_y).abs();

            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(
                shape.color.r(),
                shape.color.g(),
                shape.color.b(),
                shape.color.a(),
            );
            paint.anti_alias = true;

            let stroke = tiny_skia::Stroke {
                width: shape.stroke_width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            };
            let transform = tiny_skia::Transform::identity();

            match shape.tool {
                ScreenshotTool::Rect => {
                    if width <= 0.0 || height <= 0.0 {
                        continue;
                    }
                    if let Some(rect) = tiny_skia::Rect::from_xywh(x0, y0, width, height) {
                        let path = tiny_skia::PathBuilder::from_rect(rect);
                        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
                ScreenshotTool::Circle => {
                    if width <= 0.0 || height <= 0.0 {
                        continue;
                    }
                    if let Some(rect) = tiny_skia::Rect::from_xywh(x0, y0, width, height)
                        && let Some(path) = tiny_skia::PathBuilder::from_oval(rect)
                    {
                        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
                ScreenshotTool::Arrow => {
                    draw_arrow_skia(&mut pixmap, start_x, start_y, end_x, end_y, &paint, &stroke);
                }
                ScreenshotTool::Pen => {
                    if let Some(points) = &shape.points
                        && points.len() > 1
                    {
                        let mut pb = tiny_skia::PathBuilder::new();
                        pb.move_to(
                            points[0].x - selection_phys.min.x,
                            points[0].y - selection_phys.min.y,
                        );

                        for p in points.iter().skip(1) {
                            pb.line_to(p.x - selection_phys.min.x, p.y - selection_phys.min.y);
                        }
                        if let Some(path) = pb.finish() {
                            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 使用 imageproc 渲染顶层文本
    let font_data = EXPORT_FONT.as_ref();

    let font_ref = font_data.and_then(|data| {
        FontRef::try_from_slice_and_index(data.as_slice(), 0)
            .or_else(|_| FontRef::try_from_slice(data.as_slice()))
            .ok()
    });

    if font_ref.is_none()
        && shapes
            .iter()
            .any(|shape| shape.tool == ScreenshotTool::Text)
    {
        tracing::warn!(
            "Skipping text rendering during screenshot export because the system font is unavailable"
        );
    }

    for shape in shapes {
        if shape.tool == ScreenshotTool::Text
            && let Some(ref text) = shape.text
            && let Some(ref font) = font_ref
        {
            let start_x = shape.start.x - selection_phys.min.x;
            let start_y = shape.start.y - selection_phys.min.y;

            // 字体大小基准计算（乘以 1.5 模拟一般屏幕缩放系数，保持与 UI 视觉一致）
            let font_size = (20.0 + (shape.stroke_width * 2.0)) * 1.5;
            let scale = PxScale::from(font_size);

            // 行高计算（基础高度 + 固定行距补偿）
            let line_height = font_size + 6.0;

            let text_color = Rgba([
                shape.color.r(),
                shape.color.g(),
                shape.color.b(),
                shape.color.a(),
            ]);

            let mut current_y = start_y;

            // 逐行渲染文本（UI 层已将换行固化为 \n）
            for line in text.split('\n') {
                // 过滤可能残留的 Windows 回车符
                let clean_line = line.trim_end_matches('\r');
                imageproc::drawing::draw_text_mut(
                    final_image,
                    text_color,
                    start_x as i32,
                    current_y as i32,
                    scale,
                    font,
                    clean_line,
                );
                current_y += line_height;
            }
        }
    }
}
