use eframe::egui::{Color32, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::sync::Arc;

use crate::screenshot::feature::{
    canvas::{
        GRAB_TOLERANCE_MAX, GRAB_TOLERANCE_MIN, HIT_TEST_RADIUS, MIN_SHAPE_SIZE, ResizeStartState,
        phys_to_local,
    },
    screenshot::capture::{DrawnShape, ScreenshotTool},
};
use crate::screenshot::feature::screenshot::draw::draw_egui_shape;

/// Shape 渲染与交互能力接口
///
/// 为 DrawnShape 提供渲染、命中测试、拖拽移动和 resize 等交互能力。
/// 所有坐标参数均使用本地逻辑坐标系（除非特别标注为物理坐标）。
pub trait ShapeRender {
    /// 计算本地坐标系下的包围盒
    fn bounding_rect(&self, global_offset_phys: Pos2, ppp: f32) -> Rect;

    /// 命中测试：判断给定位置是否命中该形状
    fn hit_test(&self, pos: Pos2, global_offset_phys: Pos2, ppp: f32, painter: &Painter) -> bool;

    /// 渲染形状到画布
    fn render(&self, painter: &Painter, global_offset_phys: Pos2, ppp: f32, is_hovered: bool);

    /// 该形状是否支持 resize 控制点
    fn supports_resize(&self) -> bool {
        false
    }

    /// 应用移动偏移（物理坐标增量）
    fn translate(&mut self, delta: Vec2);

    /// 返回该形状的控制点列表（本地坐标 + 命中半径）
    fn resize_handles(&self, global_offset_phys: Pos2, ppp: f32) -> Vec<(Pos2, f32)>;

    /// 应用 resize：基于基准态、当前鼠标位置、handle 索引，更新 shape
    fn apply_resize(
        &mut self,
        handle: usize,
        current_phys: Pos2,
        start_state: &ResizeStartState,
        selection: Option<Rect>,
    );
}

impl DrawnShape {
    /// 根据笔触宽度计算文本字体大小
    ///
    /// 字体大小与笔触宽度线性相关，基准值 20.0，每单位笔触宽度增加 2.0。
    pub fn text_font_size(stroke_width: f32) -> f32 {
        20.0 + (stroke_width * 2.0)
    }

    /// 获取或创建文本的 Galley 缓存
    ///
    /// Galley 是 egui 的文本排版结果缓存，避免每帧重新排版。
    /// 首次调用时创建并缓存，后续调用直接返回缓存结果。
    pub fn ensure_galley(&mut self, painter: &Painter) -> Option<Arc<Galley>> {
        if let Some(ref g) = self.cached_galley {
            return Some(g.clone());
        }
        let text = self.text.as_ref()?;
        let font_size = Self::text_font_size(self.stroke_width);
        let galley = painter.layout_no_wrap(
            text.to_string(),
            egui::FontId::proportional(font_size),
            self.color,
        );
        self.cached_galley = Some(galley.clone());
        Some(galley)
    }

    /// 使文本排版缓存失效
    ///
    /// 在形状发生移动或 resize 后调用，强制下一帧重新排版。
    pub fn invalidate_galley(&mut self) {
        self.cached_galley = None;
    }

    /// 无缓存的情况下布局文本（用于 hit_test 等只读场景）
    ///
    /// 与 ensure_galley 不同，此方法不会写入缓存，适用于 &self 上下文。
    fn layout_text_galley(&self, painter: &Painter) -> Option<Arc<Galley>> {
        let text = self.text.as_ref()?;
        let font_size = Self::text_font_size(self.stroke_width);
        Some(painter.layout_no_wrap(
            text.to_string(),
            egui::FontId::proportional(font_size),
            self.color,
        ))
    }
}

impl ShapeRender for DrawnShape {
    /// 计算形状的包围盒
    ///
    /// 文本类型特殊处理：优先使用 galley 缓存的真实排版尺寸，
    /// 降级时使用 start/end 计算的矩形尺寸。
    fn bounding_rect(&self, global_offset_phys: Pos2, ppp: f32) -> Rect {
        let start_local = phys_to_local(self.start, global_offset_phys, ppp);

        if self.tool == ScreenshotTool::Text {
            // 文本框：优先使用排版缓存的精确尺寸
            if let Some(galley) = &self.cached_galley {
                return Rect::from_min_size(start_local, galley.size());
            }
            // 降级：无排版缓存时使用 start/end 估算
            let end_local = phys_to_local(self.end, global_offset_phys, ppp);
            let width = (end_local.x - start_local.x).abs();
            let height = (end_local.y - start_local.y).abs();
            return Rect::from_min_size(start_local, eframe::egui::vec2(width, height));
        }

        // 其他工具（Rect, Circle, Arrow 等）使用 start/end 构成的矩形
        let end_local = phys_to_local(self.end, global_offset_phys, ppp);
        Rect::from_two_pos(start_local, end_local)
    }

    /// 命中测试：根据工具类型使用不同的几何判定算法
    ///
    /// - Rect: 边框带状区域（expand + shrink 差集）
    /// - Circle: 椭圆边缘距离判定（极坐标法）
    /// - Arrow: 线段距离判定
    /// - Text: 文本矩形区域判定
    /// - Pen: 折线段逐段距离判定
    /// - Mosaic: 不支持命中测试（由包围盒高亮代替）
    fn hit_test(&self, pos: Pos2, global_offset_phys: Pos2, ppp: f32, painter: &Painter) -> bool {
        let start_local = phys_to_local(self.start, global_offset_phys, ppp);
        let end_local = phys_to_local(self.end, global_offset_phys, ppp);
        let shape_rect = Rect::from_two_pos(start_local, end_local);
        let grab_tolerance =
            (self.stroke_width / ppp).clamp(GRAB_TOLERANCE_MIN, GRAB_TOLERANCE_MAX);

        match self.tool {
            ScreenshotTool::Rect => {
                // 矩形边框命中：扩展区域包含且收缩区域不包含 = 在边框带上
                let expanded = shape_rect.expand(grab_tolerance);
                let shrunk = shape_rect.shrink(grab_tolerance);
                expanded.contains(pos) && (!shrunk.is_positive() || !shrunk.contains(pos))
            }
            ScreenshotTool::Circle => {
                // 椭圆边缘命中：使用极坐标法计算点到椭圆边缘的距离
                let center = shape_rect.center();
                let a = shape_rect.width() / 2.0;
                let b = shape_rect.height() / 2.0;
                let dx = pos.x - center.x;
                let dy = pos.y - center.y;
                let dist = pos.distance(center);

                if dist < 0.1 || a < 0.1 || b < 0.1 {
                    false
                } else {
                    // 计算该角度方向上椭圆的半径
                    let cos_t = dx / dist;
                    let sin_t = dy / dist;
                    let r_theta = (a * b) / ((b * cos_t).powi(2) + (a * sin_t).powi(2)).sqrt();
                    (dist - r_theta).abs() <= grab_tolerance
                }
            }
            ScreenshotTool::Arrow => {
                // 箭头命中：点到线段距离
                dist_to_line_segment(pos, start_local, end_local) <= grab_tolerance
            }
            ScreenshotTool::Text => {
                // 文本命中：文本排版矩形区域（带 4px 容差）
                if let Some(galley) = self.layout_text_galley(painter) {
                    let text_rect = Rect::from_min_size(start_local, galley.size());
                    text_rect.expand(4.0).contains(pos)
                } else {
                    false
                }
            }
            ScreenshotTool::Pen => {
                // 画笔命中：逐段检测点到折线段的距离
                if let Some(points) = &self.points {
                    for i in 0..points.len().saturating_sub(1) {
                        let p1 = phys_to_local(points[i], global_offset_phys, ppp);
                        let p2 = phys_to_local(points[i + 1], global_offset_phys, ppp);
                        if dist_to_line_segment(pos, p1, p2) <= grab_tolerance {
                            return true;
                        }
                    }
                    false
                } else {
                    false
                }
            }
            ScreenshotTool::Mosaic => false,
        }
    }

    /// 渲染形状
    ///
    /// 先绘制悬停高亮边框（蓝色半透明），再根据工具类型分发到对应的渲染逻辑。
    /// 马赛克类型不在此处渲染（由 render.rs 特殊处理，因为需要访问原图采样）。
    fn render(&self, painter: &Painter, global_offset_phys: Pos2, ppp: f32, is_hovered: bool) {
        let start_local = phys_to_local(self.start, global_offset_phys, ppp);
        let end_local = phys_to_local(self.end, global_offset_phys, ppp);
        let rect = Rect::from_two_pos(start_local, end_local);

        // 悬停高亮：蓝色半透明边框
        if is_hovered {
            let highlight_rect = if self.tool == ScreenshotTool::Text {
                if let Some(galley) = self.layout_text_galley(painter) {
                    Rect::from_min_size(start_local, galley.size())
                } else {
                    rect
                }
            } else {
                rect
            };
            painter.rect_stroke(
                highlight_rect.expand(2.0),
                2.0,
                Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 150, 255, 100)),
                StrokeKind::Outside,
            );
        }

        match self.tool {
            ScreenshotTool::Text => {
                // 文本渲染：使用无缓存排版（render 接收 &self，不可变引用）
                if let Some(galley) = self.layout_text_galley(painter) {
                    painter.galley(start_local, galley, self.color);
                }
            }
            ScreenshotTool::Pen => {
                // 画笔渲染：将物理坐标点序列转换为本地坐标后绘制折线
                if let Some(points) = &self.points {
                    let mut local_points = Vec::with_capacity(points.len());
                    for p in points.iter() {
                        local_points.push(phys_to_local(*p, global_offset_phys, ppp));
                    }
                    painter.add(eframe::egui::Shape::line(
                        local_points,
                        Stroke::new(self.stroke_width, self.color),
                    ));
                }
            }
            ScreenshotTool::Mosaic => {
                // 马赛克在 render.rs 中特殊处理，因为需要访问 captures 采样原图
            }
            _ => {
                // Rect, Circle, Arrow 等几何形状使用统一的绘图函数
                draw_egui_shape(
                    painter,
                    self.tool,
                    rect,
                    start_local,
                    end_local,
                    self.stroke_width,
                    self.color,
                );
            }
        }
    }

    /// 判断形状是否支持 resize 控制点
    ///
    /// 矩形、圆形、箭头和文本支持 resize，画笔和马赛克不支持。
    fn supports_resize(&self) -> bool {
        matches!(
            self.tool,
            ScreenshotTool::Rect
                | ScreenshotTool::Circle
                | ScreenshotTool::Arrow
                | ScreenshotTool::Text
        )
    }

    /// 平移形状（物理坐标增量）
    ///
    /// 同时移动 start/end 端点和画笔轨迹点，并使文本缓存失效。
    fn translate(&mut self, delta: Vec2) {
        self.start += delta;
        self.end += delta;
        self.invalidate_galley();
        if let Some(points) = &mut self.points {
            for p in Arc::make_mut(points).iter_mut() {
                *p += delta;
            }
        }
    }

    /// 计算 resize 控制点列表
    ///
    /// 不同工具类型的控制点布局不同：
    /// - Arrow: 仅起点和终点 2 个控制点
    /// - Text: 4 个角的控制点
    /// - Rect/Circle: 8 个控制点（4 角 + 4 边中点）
    fn resize_handles(&self, global_offset_phys: Pos2, ppp: f32) -> Vec<(Pos2, f32)> {
        if !self.supports_resize() {
            return Vec::new();
        }

        let hit_radius = HIT_TEST_RADIUS;

        match self.tool {
            ScreenshotTool::Arrow => {
                // 箭头只有起点和终点两个控制点
                let start_local = phys_to_local(self.start, global_offset_phys, ppp);
                let end_local = phys_to_local(self.end, global_offset_phys, ppp);
                vec![
                    (start_local, hit_radius),
                    (end_local, hit_radius),
                ]
            }
            ScreenshotTool::Text => {
                // 文本工具只保留 4 个角的控制点
                let rect = self.bounding_rect(global_offset_phys, ppp);
                vec![
                    (rect.left_top(), hit_radius),
                    (rect.right_top(), hit_radius),
                    (rect.right_bottom(), hit_radius),
                    (rect.left_bottom(), hit_radius),
                ]
            }
            _ => {
                // Rect, Circle: 8 控制点布局
                //
                // 0 ─── 4 ─── 1
                // │           │
                // 7           5
                // │           │
                // 3 ─── 6 ─── 2
                let rect = self.bounding_rect(global_offset_phys, ppp);
                let center = rect.center();
                vec![
                    (rect.left_top(), hit_radius),
                    (rect.right_top(), hit_radius),
                    (rect.right_bottom(), hit_radius),
                    (rect.left_bottom(), hit_radius),
                    (Pos2::new(center.x, rect.min.y), hit_radius),
                    (Pos2::new(rect.max.x, center.y), hit_radius),
                    (Pos2::new(center.x, rect.max.y), hit_radius),
                    (Pos2::new(rect.min.x, center.y), hit_radius),
                ]
            }
        }
    }

    /// 应用 resize 操作
    ///
    /// 根据控制点索引和当前鼠标位置，计算新的 start/end 端点。
    /// 文本类型的 resize 特殊处理：通过调整 stroke_width 实现等比缩放。
    fn apply_resize(
        &mut self,
        handle: usize,
        current_phys: Pos2,
        start_state: &ResizeStartState,
        selection: Option<Rect>,
    ) {
        let clamped = clamp_pos_to_rect(current_phys, selection.unwrap_or(Rect::EVERYTHING));
        let (new_start, new_end) = resized_endpoints(self.tool, handle, clamped, start_state);

        if !is_valid_resize(self.tool, new_start, new_end) {
            return;
        }

        if self.tool == ScreenshotTool::Text {
            apply_text_resize(self, new_start, new_end);
        } else {
            self.start = new_start;
            self.end = new_end;
        }
    }
}

/// 根据工具类型和控制点索引，计算 resize 后的新端点
fn resized_endpoints(
    tool: ScreenshotTool,
    handle: usize,
    clamped: Pos2,
    start_state: &ResizeStartState,
) -> (Pos2, Pos2) {
    match tool {
        ScreenshotTool::Arrow => resize_arrow(handle, clamped, start_state),
        _ => resize_box_handle(handle, clamped, start_state),
    }
}

/// 箭头 resize：只有起点（handle=0）和终点（handle=1）两个控制点
fn resize_arrow(handle: usize, clamped: Pos2, start_state: &ResizeStartState) -> (Pos2, Pos2) {
    match handle {
        0 => (clamped, start_state.end),
        1 => (start_state.start, clamped),
        _ => (start_state.start, start_state.end),
    }
}

/// 矩形/圆形 resize：8 个控制点的端点计算
///
/// 控制点索引对应关系：
/// 0=NW, 1=NE, 2=SE, 3=SW, 4=N, 5=E, 6=S, 7=W
fn resize_box_handle(handle: usize, clamped: Pos2, start_state: &ResizeStartState) -> (Pos2, Pos2) {
    match handle {
        0 => (clamped, start_state.end),
        1 => (
            Pos2::new(start_state.start.x, clamped.y),
            Pos2::new(clamped.x, start_state.end.y),
        ),
        2 => (start_state.start, clamped),
        3 => (
            Pos2::new(clamped.x, start_state.start.y),
            Pos2::new(start_state.end.x, clamped.y),
        ),
        4 => (Pos2::new(start_state.start.x, clamped.y), start_state.end),
        5 => (start_state.start, Pos2::new(clamped.x, start_state.end.y)),
        6 => (start_state.start, Pos2::new(start_state.end.x, clamped.y)),
        7 => (Pos2::new(clamped.x, start_state.start.y), start_state.end),
        _ => (start_state.start, start_state.end),
    }
}

/// 验证 resize 后的尺寸是否有效
///
/// 文本类型要求最小宽度 10.0（保证至少能显示一个字符），
/// 其他类型要求宽高都不小于 MIN_SHAPE_SIZE。
fn is_valid_resize(tool: ScreenshotTool, new_start: Pos2, new_end: Pos2) -> bool {
    let width = (new_end.x - new_start.x).abs();
    let height = (new_end.y - new_start.y).abs();

    if tool == ScreenshotTool::Text {
        width >= 10.0 && height >= MIN_SHAPE_SIZE
    } else {
        width >= MIN_SHAPE_SIZE && height >= MIN_SHAPE_SIZE
    }
}

/// 文本 resize 的特殊处理：通过调整 stroke_width 实现等比缩放
///
/// 文本的"大小"由 stroke_width 控制（字体大小 = 20 + stroke_width * 2），
/// resize 时根据宽度变化比例反推新的 stroke_width，然后按实际比例调整 end 坐标。
fn apply_text_resize(shape: &mut DrawnShape, new_start: Pos2, new_end: Pos2) {
    let prev_w = (shape.end.x - shape.start.x).abs();
    let prev_h = (shape.end.y - shape.start.y).abs();

    if prev_w > 1.0 {
        // 计算宽度变化比例，反推新的 stroke_width
        let new_w = (new_end.x - new_start.x).abs();
        let ratio = new_w / prev_w;
        let stroke_width_before = shape.stroke_width;
        let mut stroke_width_after = ratio * (10.0 + stroke_width_before) - 10.0;
        stroke_width_after = stroke_width_after.clamp(1.0, 48.0);
        shape.stroke_width = stroke_width_after;

        // 使用实际比例（考虑 stroke_width 的离散化误差）计算新的 end 坐标
        let actual_ratio = (10.0 + stroke_width_after) / (10.0 + stroke_width_before);
        let actual_new_w = prev_w * actual_ratio;
        let actual_new_h = prev_h * actual_ratio;
        let sign_x = (new_end.x - new_start.x).signum();
        let sign_y = (new_end.y - new_start.y).signum();

        shape.start = new_start;
        shape.end = Pos2::new(
            new_start.x + actual_new_w * sign_x,
            new_start.y + actual_new_h * sign_y,
        );
    } else {
        shape.start = new_start;
        shape.end = new_end;
    }

    shape.invalidate_galley();
}

/// 计算点到线段的最短距离
///
/// 使用向量投影法：将点投影到线段所在直线上，
/// 参数 t 限制在 [0,1] 范围内，然后计算点到投影点的距离。
pub fn dist_to_line_segment(p: Pos2, v: Pos2, w: Pos2) -> f32 {
    let l2 = v.distance_sq(w);
    if l2 == 0.0 {
        return p.distance(v);
    }
    let t = ((p.x - v.x) * (w.x - v.x) + (p.y - v.y) * (w.y - v.y)) / l2;
    let t = t.clamp(0.0, 1.0);
    let projection = Pos2::new(v.x + t * (w.x - v.x), v.y + t * (w.y - v.y));
    p.distance(projection)
}

/// 将位置限制在矩形内
///
/// 对 x 和 y 分量分别做 clamp，确保返回的位置不超出矩形边界。
pub fn clamp_pos_to_rect(pos: Pos2, rect: Rect) -> Pos2 {
    Pos2::new(
        pos.x.clamp(rect.min.x, rect.max.x),
        pos.y.clamp(rect.min.y, rect.max.y),
    )
}
