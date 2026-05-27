pub mod drag;
pub mod draw;
pub mod hit_test;
pub mod interaction;
pub mod mosaic;
pub mod render;
pub mod shape;
pub mod text_input;

pub use crate::screenshot::feature::screenshot::SCREENSHOT_BORDER_COLOR;
use crate::screenshot::feature::screenshot::capture::{
    CapturedScreen, DrawnShape, ScreenshotState, ScreenshotTool,
};
use crate::screenshot::model::device::get_screen_phys_rect;
use eframe::egui::{Color32, Id, Pos2, Rect, Ui, Vec2};
use std::sync::Arc;

/// 物理坐标转换为本地逻辑坐标
///
/// 将屏幕物理像素坐标映射到 egui 画布的本地逻辑坐标系，
/// 需要减去全局偏移（多显示器场景下的虚拟桌面起点）再除以缩放因子。
pub fn phys_to_local(pos: Pos2, global_offset_phys: Pos2, ppp: f32) -> Pos2 {
    Pos2::ZERO + ((pos - global_offset_phys) / ppp)
}

/// 物理矩形转换为本地逻辑矩形
///
/// 对矩形的两个顶点分别执行物理→逻辑坐标转换，保持矩形语义不变。
pub fn phys_rect_to_local(rect: Rect, global_offset_phys: Pos2, ppp: f32) -> Rect {
    Rect::from_min_max(
        phys_to_local(rect.min, global_offset_phys, ppp),
        phys_to_local(rect.max, global_offset_phys, ppp),
    )
}

/// 在捕获屏幕列表中查找包含指定物理坐标的屏幕矩形
///
/// 遍历所有捕获的屏幕，返回第一个包含给定坐标的屏幕物理矩形。
/// 此函数使用 state 模块的 CapturedScreen 类型（与 state.capture.captures 一致）。
pub fn find_target_screen_rect(captures: &[CapturedScreen], pos: Pos2) -> Option<Rect> {
    captures.iter().find_map(|cap| {
        let rect = get_screen_phys_rect(&cap.screen_info);
        if rect.contains(pos) {
            Some(rect)
        } else {
            None
        }
    })
}

/// 马赛克块大小（物理像素）
pub const MOSAIC_BLOCK_SIZE: f32 = 15.0;
/// 命中测试半径（本地坐标），用于控制点的命中判定
pub const HIT_TEST_RADIUS: f32 = 15.0;
/// 抓取容差最小值（本地坐标）
pub const GRAB_TOLERANCE_MIN: f32 = 4.0;
/// 抓取容差最大值（本地坐标）
pub const GRAB_TOLERANCE_MAX: f32 = 8.0;
/// 形状最小尺寸（物理像素），低于此值的 resize 操作将被拒绝
pub const MIN_SHAPE_SIZE: f32 = 4.0;
/// 选区/窗口检测框的锚点大小（本地坐标）
pub const ANCHOR_SIZE: f32 = 6.0;
/// 遮罩透明度（0=全透明，255=不透明）
pub const OVERLAY_ALPHA: u8 = 128;

/// Resize 开始时的基准状态
///
/// 记录拖拽控制点开始瞬间的起点和终点，
/// 在 resize 过程中作为不变量参与新坐标的计算。
#[derive(Clone, Copy, Debug)]
pub struct ResizeStartState {
    pub start: Pos2,
    pub end: Pos2,
}

/// 画布运行时状态，在帧间通过 egui temp data 持久化
///
/// 跟踪悬停、选中、拖拽等交互状态。由于 egui 的即时模式特性，
/// 这些状态需要在每帧结束时保存到 UI data 中，下一帧开始时恢复。
#[derive(Default, Clone, Copy, Debug)]
pub struct CanvasState {
    /// 当前鼠标悬停的图形索引
    pub hovered_shape: Option<usize>,
    /// 当前选中的图形索引
    pub selected_shape: Option<usize>,
    /// 正在拖拽移动的图形索引
    pub dragging_shape: Option<usize>,
    /// 是否正在拖拽整个选区
    pub dragging_selection: bool,
    /// 拖拽起始物理坐标（用于计算增量）
    pub drag_start_phys: Option<Pos2>,
    /// 正在拖拽的控制点索引
    pub dragging_handle: Option<usize>,
    /// resize 开始时的形状基准状态
    pub resize_start_state: Option<ResizeStartState>,
}

impl CanvasState {
    const HOVERED_ID: &'static str = "cv_canvas_hovered_shape";
    const SELECTED_ID: &'static str = "cv_canvas_selected_shape";
    const DRAGGING_ID: &'static str = "cv_canvas_dragging_shape";
    const DRAGGING_SEL_ID: &'static str = "cv_canvas_dragging_selection";
    const DRAG_START_ID: &'static str = "cv_canvas_drag_start";
    const DRAGGING_HANDLE_ID: &'static str = "cv_canvas_dragging_handle";
    const RESIZE_START_STATE_ID: &'static str = "cv_canvas_resize_start_state";

    /// 从 egui UI 临时数据中恢复画布状态
    ///
    /// 在每帧开始时调用，从上一帧保存的临时数据中读取所有交互状态。
    pub fn load_from_ui(ui: &Ui) -> Self {
        Self {
            hovered_shape: ui.data(|d| d.get_temp(Id::new(Self::HOVERED_ID))),
            selected_shape: ui.data(|d| d.get_temp(Id::new(Self::SELECTED_ID))),
            dragging_shape: ui.data(|d| d.get_temp(Id::new(Self::DRAGGING_ID))),
            dragging_selection: ui
                .data(|d| d.get_temp(Id::new(Self::DRAGGING_SEL_ID)))
                .unwrap_or(false),
            drag_start_phys: ui.data(|d| d.get_temp(Id::new(Self::DRAG_START_ID))),
            dragging_handle: ui.data(|d| d.get_temp(Id::new(Self::DRAGGING_HANDLE_ID))),
            resize_start_state: ui.data(|d| d.get_temp(Id::new(Self::RESIZE_START_STATE_ID))),
        }
    }

    /// 将画布状态保存到 egui UI 临时数据中
    ///
    /// 在每帧结束时调用，将当前交互状态持久化到 UI data 以供下一帧读取。
    /// Option 为 None 时移除对应的临时数据，避免内存泄漏。
    pub fn save_to_ui(self, ui: &Ui) {
        ui.data_mut(|d| {
            if let Some(v) = self.hovered_shape {
                d.insert_temp(Id::new(Self::HOVERED_ID), v);
            } else {
                d.remove::<usize>(Id::new(Self::HOVERED_ID));
            }

            if let Some(v) = self.selected_shape {
                d.insert_temp(Id::new(Self::SELECTED_ID), v);
            } else {
                d.remove::<usize>(Id::new(Self::SELECTED_ID));
            }

            if let Some(v) = self.dragging_shape {
                d.insert_temp(Id::new(Self::DRAGGING_ID), v);
            } else {
                d.remove::<usize>(Id::new(Self::DRAGGING_ID));
            }

            d.insert_temp(Id::new(Self::DRAGGING_SEL_ID), self.dragging_selection);

            if let Some(v) = self.drag_start_phys {
                d.insert_temp(Id::new(Self::DRAG_START_ID), v);
            } else {
                d.remove::<Pos2>(Id::new(Self::DRAG_START_ID));
            }

            if let Some(v) = self.dragging_handle {
                d.insert_temp(Id::new(Self::DRAGGING_HANDLE_ID), v);
            } else {
                d.remove::<usize>(Id::new(Self::DRAGGING_HANDLE_ID));
            }

            if let Some(v) = self.resize_start_state {
                d.insert_temp(Id::new(Self::RESIZE_START_STATE_ID), v);
            } else {
                d.remove::<ResizeStartState>(Id::new(Self::RESIZE_START_STATE_ID));
            }
        });
    }
}

/// 提交文本输入为一个 DrawnShape
///
/// 将当前正在编辑的文本固化为一个正式的图形对象：
/// 1. 根据选区边界计算文本最大宽度
/// 2. 使用 egui 排版引擎获取实际文本布局
/// 3. 将排版结果"烘焙"为带换行符的纯文本
/// 4. 创建 DrawnShape 并加入图形列表，同时记录撤销历史
pub fn commit_text_shape(
    ui: &Ui,
    state: &mut ScreenshotState,
    pos: Pos2,
    text: String,
    global_offset_phys: Pos2,
    ppp: f32,
) {
    let font_size = DrawnShape::text_font_size(state.drawing.stroke_width);

    // 计算文本最大宽度：如果有选区，限制在选区右边界内；否则给一个足够大的值
    let max_width_logical = if let Some(sel) = state.select.selection {
        let sel_max_x_local = Pos2::ZERO.x + ((sel.max.x - global_offset_phys.x) / ppp);
        let start_local_x = Pos2::ZERO.x + ((pos.x - global_offset_phys.x) / ppp);
        (sel_max_x_local - start_local_x - 16.0).max(20.0)
    } else {
        1000.0
    };

    // 使用 egui 排版引擎计算文本的实际布局
    let galley = ui.painter().layout(
        text.clone(),
        eframe::egui::FontId::proportional(font_size),
        Color32::WHITE,
        max_width_logical,
    );

    // 将排版结果"烘焙"为纯文本：去除行尾空白，保留换行结构
    let mut baked_text = String::new();
    let rows_len = galley.rows.len();
    for (i, row) in galley.rows.iter().enumerate() {
        let mut row_str = String::new();
        for glyph in &row.glyphs {
            row_str.push(glyph.chr);
        }
        baked_text.push_str(row_str.trim_end_matches(&['\r', '\n'][..]));
        if i < rows_len - 1 {
            baked_text.push('\n');
        }
    }

    // 计算文本形状的物理坐标范围
    let start_pos_phys = pos + Vec2::new(8.0 * ppp, 8.0 * ppp);
    let text_width_phys = galley.size().x * ppp;
    let end_pos = start_pos_phys + Vec2::new(text_width_phys, 0.0);

    // 创建文本图形并记录到撤销历史
    state.edit.shapes.push(DrawnShape::new(
        ScreenshotTool::Text,
        start_pos_phys,
        end_pos,
        state.drawing.active_color,
        state.drawing.stroke_width,
        Some(Arc::<str>::from(baked_text)),
        None,
    ));
    state.record_shape_added(state.edit.shapes.len() - 1);
}

pub fn finalize_pending_edits(
    ui: &Ui,
    state: &mut ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
) {
    if let Some((pos, text)) = state.input.active_text_input.take() {
        if !text.trim().is_empty() {
            commit_text_shape(ui, state, pos, text, global_offset_phys, ppp);
        }
    }

    if !state.input.current_pen_points.is_empty() {
        if state.input.current_pen_points.len() > 1 {
            let mut min_pos = state.input.current_pen_points[0];
            let mut max_pos = state.input.current_pen_points[0];
            for p in &state.input.current_pen_points {
                min_pos = min_pos.min(*p);
                max_pos = max_pos.max(*p);
            }

            let tool = state.drawing.current_tool.unwrap_or(ScreenshotTool::Pen);
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
        } else {
            state.input.current_pen_points.clear();
        }
    }

    if let Some(start_pos) = state.input.current_shape_start {
        let end_pos = state.input.current_shape_end.unwrap_or(start_pos);
        if start_pos.distance(end_pos) > 5.0
            && let Some(tool) = state.drawing.current_tool
            && tool != ScreenshotTool::Text
            && tool != ScreenshotTool::Pen
            && tool != ScreenshotTool::Mosaic
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
    }
}
