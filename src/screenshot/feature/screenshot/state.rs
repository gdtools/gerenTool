use eframe::egui::{Color32, Pos2, Rect, TextureHandle};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::Arc;

use crate::screenshot::model::device::MonitorInfo;

/// 重新导出 WindowPrevState，使外部可通过 feature::screenshot::state 路径访问
pub use crate::screenshot::model::state::WindowPrevState;

/// 默认绘图颜色（红色）
const DEFAULT_ACTIVE_COLOR: Color32 = Color32::from_rgb(204, 0, 0);
/// 默认描边宽度
const DEFAULT_STROKE_WIDTH: f32 = 2.0;
/// 默认马赛克笔刷宽度
const DEFAULT_MOSAIC_WIDTH: f32 = 16.0;
/// 撤销/重做历史栈最大条目数
const MAX_HISTORY_ENTRIES: usize = 50;

/// 预设常用绘图颜色（供属性面板快速选择）
pub const PRESET_COLORS: [Color32; 8] = [
    Color32::from_rgb(204, 0, 0),
    Color32::from_rgb(255, 102, 0),
    Color32::from_rgb(255, 204, 0),
    Color32::from_rgb(0, 176, 80),
    Color32::from_rgb(0, 112, 192),
    Color32::from_rgb(112, 48, 160),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(0, 0, 0),
];

/// 预设描边宽度选项
pub const PRESET_WIDTHS: [f32; 4] = [1.0, 2.0, 3.0, 5.0];

/// 属性面板高度（工具栏下方展开的属性设置区域）
pub const PROPERTY_PANEL_HEIGHT: f32 = 36.0;

/// 截图完成后的动作类型
#[derive(PartialEq, Clone, Copy)]
pub enum ScreenshotAction {
    /// 无动作
    None,
    /// 关闭截图（不保存）
    Close,
    /// 保存到文件并关闭
    SaveAndClose,
    /// 另存为（弹出文件对话框，保存后不关闭截图）
    SaveAs,
    /// 复制到剪贴板
    SaveToClipboard,
    /// 置顶贴图（将选区内容钉到屏幕最顶层）
    PinToTop,
    /// 延时截图（关闭覆盖层，等待指定秒数后重新截图）
    DelayCapture(u32),
    /// 开始滚动截图（覆盖层保持，进入滚动捕获模式）
    ScrollCapture,
    /// 停止滚动截图（合并帧，保存到桌面，关闭覆盖层）
    StopScrollCapture,
}

/// 滚动截图阶段
#[derive(Default, Clone)]
pub enum ScrollCapturePhase {
    /// 未启动
    #[default]
    Idle,
    /// 正在滚动捕获中
    /// frames: 已捕获的帧列表（每帧为裁剪后的选区图像）
    /// selection: 选区物理坐标
    Running {
        long_image: image::RgbaImage,
        prev_frame: image::RgbaImage,
        selection: eframe::egui::Rect,
        /// 上次截帧的时间
        last_capture: std::time::Instant,
    },
}

/// 截图绘图工具类型
#[derive(PartialEq, Clone, Copy)]
pub enum ScreenshotTool {
    /// 矩形框
    Rect,
    /// 椭圆
    Circle,
    /// 箭头
    Arrow,
    /// 文本
    Text,
    /// 画笔
    Pen,
    /// 马赛克
    Mosaic,
}

/// 已绘制的形状
///
/// 记录单个绘图元素的完整信息，包括工具类型、坐标、样式和运行时缓存。
/// `cached_galley` 和 `cached_mosaic` 为运行时缓存，不参与历史快照。
#[derive(Clone)]
pub struct DrawnShape {
    /// 使用的工具类型
    pub tool: ScreenshotTool,
    /// 起始坐标（物理像素）
    pub start: Pos2,
    /// 结束坐标（物理像素）
    pub end: Pos2,
    /// 颜色
    pub color: Color32,
    /// 描边宽度
    pub stroke_width: f32,
    /// 文本内容（仅 Text 工具使用）
    pub text: Option<Arc<str>>,
    /// 画笔/马赛克轨迹点（仅 Pen/Mosaic 工具使用）
    pub points: Option<Arc<Vec<Pos2>>>,
    /// 运行时缓存：文本的 egui Galley，避免每帧重排版
    pub cached_galley: Option<Arc<egui::Galley>>,
    /// 运行时缓存：马赛克的纹理，避免每帧采样原图
    pub cached_mosaic: Option<Arc<MosaicCache>>,
}

impl DrawnShape {
    /// 创建新的绘制形状
    pub fn new(
        tool: ScreenshotTool,
        start: Pos2,
        end: Pos2,
        color: Color32,
        stroke_width: f32,
        text: Option<Arc<str>>,
        points: Option<Arc<Vec<Pos2>>>,
    ) -> Self {
        Self {
            tool,
            start,
            end,
            color,
            stroke_width,
            text,
            points,
            cached_galley: None,
            cached_mosaic: None,
        }
    }

    /// 创建用于历史记录的轻量克隆（不包含运行时缓存）
    fn clone_for_history(&self) -> Self {
        Self {
            tool: self.tool,
            start: self.start,
            end: self.end,
            color: self.color,
            stroke_width: self.stroke_width,
            text: self.text.clone(),
            points: self.points.clone(),
            cached_galley: None,
            cached_mosaic: None,
        }
    }
}

/// 马赛克纹理缓存
#[derive(Clone)]
pub struct MosaicCache {
    /// 纹理句柄
    pub texture: TextureHandle,
    /// 纹理对应的物理坐标范围
    pub phys_rect: Rect,
}

/// 截图状态聚合体
///
/// 包含截图模式下的所有子状态：
/// - `capture`: 屏幕捕获数据
/// - `select`: 选区状态
/// - `drawing`: 绘图工具状态
/// - `edit`: 已绘制形状和撤销/重做历史
/// - `runtime`: 运行时标志
/// - `input`: 当前输入状态
pub struct ScreenshotState {
    pub capture: ScreenshotCaptureState,
    pub select: ScreenshotSelectionState,
    pub drawing: ScreenshotDrawingState,
    pub edit: ScreenshotEditState,
    pub runtime: ScreenshotRuntimeState,
    pub input: ScreenshotInputState,
}

/// 已绘制形状和撤销/重做历史管理
#[derive(Default)]
pub struct ScreenshotEditState {
    /// 当前所有已确认的形状
    pub shapes: Vec<DrawnShape>,
    /// 撤销栈
    pub history: Vec<HistoryEntry>,
    /// 重做栈
    pub redo: Vec<HistoryEntry>,
}

/// 屏幕捕获状态
///
/// 管理多屏幕截图的原始图像、纹理缓存和窗口矩形信息
#[derive(Default)]
pub struct ScreenshotCaptureState {
    /// 已捕获的屏幕快照列表
    pub captures: Vec<CapturedScreen>,
    /// 所有可见窗口的物理矩形（用于智能选区吸附）
    pub window_rects: Vec<Rect>,
    /// 是否正在进行异步捕获
    pub is_capturing: bool,
    /// 异步捕获结果的接收通道
    pub capture_receiver: Option<std::sync::mpsc::Receiver<(Vec<CapturedScreen>, Vec<Rect>)>>,
    /// 屏幕纹理缓存（按显示器名称索引，支持跨次截图复用）
    pub texture_pool: HashMap<String, TextureHandle>,
}

/// 选区状态
#[derive(Default)]
pub struct ScreenshotSelectionState {
    /// 当前选区矩形（物理坐标）
    pub selection: Option<Rect>,
    /// 选区拖拽起始点
    pub drag_start: Option<Pos2>,
    /// 工具栏锚点位置（通常为选区右下角）
    pub toolbar_pos: Option<Pos2>,
    /// 当前鼠标悬停的窗口矩形
    pub hovered_window: Option<Rect>,
}

/// 绘图工具状态
pub struct ScreenshotDrawingState {
    /// 当前选中的工具
    pub current_tool: Option<ScreenshotTool>,
    /// 当前绘图颜色
    pub active_color: Color32,
    /// 描边宽度
    pub stroke_width: f32,
    /// 马赛克笔刷宽度
    pub mosaic_width: f32,
}

impl Default for ScreenshotDrawingState {
    fn default() -> Self {
        Self {
            current_tool: None,
            active_color: DEFAULT_ACTIVE_COLOR,
            stroke_width: DEFAULT_STROKE_WIDTH,
            mosaic_width: DEFAULT_MOSAIC_WIDTH,
        }
    }
}

/// 运行时状态标志
pub struct ScreenshotRuntimeState {
    /// 视口窗口是否已完成配置（尺寸和位置已稳定）
    pub window_configured: bool,
    /// 截图前的窗口是否已被同步移动到屏幕外
    pub window_hidden_for_capture: bool,
    /// 滚动截图阶段状态
    pub scroll_capture: ScrollCapturePhase,
}

impl ScreenshotRuntimeState {
    fn new(_prev_window_state: WindowPrevState) -> Self {
        Self {
            window_configured: false,
            window_hidden_for_capture: false,
            scroll_capture: ScrollCapturePhase::Idle,
        }
    }
}

/// 当前输入状态（正在进行的交互）
#[derive(Default)]
pub struct ScreenshotInputState {
    /// 当前正在绘制的形状起始点
    pub current_shape_start: Option<Pos2>,
    /// 当前正在绘制的形状结束点
    pub current_shape_end: Option<Pos2>,
    /// 活跃的文本输入：(物理坐标, 文本内容)
    pub active_text_input: Option<(Pos2, String)>,
    /// 当前正在绘制的画笔轨迹点
    pub current_pen_points: Vec<Pos2>,
    /// 选区变更的原始状态（用于撤销）
    pub selection_change_origin: Option<SelectionChangeOrigin>,
}

/// 撤销/重做历史条目
///
/// 使用枚举表示不同类型的可逆操作，每个变体携带足够的信息来执行正向和反向操作
#[derive(Clone)]
pub enum HistoryEntry {
    /// 插入形状（撤销时移除）
    InsertShape {
        index: usize,
        shape: DrawnShape,
    },
    /// 移除形状（撤销时恢复）
    RemoveShape {
        index: usize,
    },
    /// 替换形状（撤销时恢复原状）
    ReplaceShape {
        index: usize,
        shape: DrawnShape,
    },
    /// 恢复选区和所有形状（用于选区变更的批量撤销）
    RestoreSelectionAndShapes {
        selection: Option<Rect>,
        shapes: Vec<DrawnShape>,
    },
}

/// 选区变更的原始状态记录
#[derive(Clone, Copy)]
pub struct SelectionChangeOrigin {
    /// 变更前的选区
    pub previous_selection: Option<Rect>,
}

impl Default for ScreenshotState {
    fn default() -> Self {
        Self::new(WindowPrevState::Normal)
    }
}

impl ScreenshotState {
    /// 创建新的截图状态
    pub fn new(prev_state: WindowPrevState) -> Self {
        Self {
            capture: ScreenshotCaptureState::default(),
            select: ScreenshotSelectionState::default(),
            drawing: ScreenshotDrawingState::default(),
            edit: ScreenshotEditState::default(),
            runtime: ScreenshotRuntimeState::new(prev_state),
            input: ScreenshotInputState::default(),
        }
    }

    /// 记录形状添加操作（撤销时移除该形状）
    pub fn record_shape_added(&mut self, index: usize) {
        self.push_undo_entry(HistoryEntry::RemoveShape { index });
    }

    /// 记录形状移除操作（撤销时恢复该形状）
    pub fn record_shape_removed(&mut self, index: usize, shape: DrawnShape) {
        self.push_undo_entry(HistoryEntry::InsertShape {
            index,
            shape: shape.clone_for_history(),
        });
    }

    /// 记录形状编辑前的状态（撤销时恢复原状）
    pub fn record_shape_before_edit(&mut self, index: usize) {
        let Some(shape) = self.edit.shapes.get(index) else {
            return;
        };

        self.push_undo_entry(HistoryEntry::ReplaceShape {
            index,
            shape: shape.clone_for_history(),
        });
    }

    /// 记录选区变更（撤销时恢复选区和所有形状）
    pub fn record_selection_change(&mut self, previous_selection: Option<Rect>) {
        let shapes = self
            .edit
            .shapes
            .iter()
            .map(DrawnShape::clone_for_history)
            .collect();

        self.push_undo_entry(HistoryEntry::RestoreSelectionAndShapes {
            selection: previous_selection,
            shapes,
        });
    }

    /// 设置选区并同步工具栏位置
    pub fn set_selection(&mut self, selection: Option<Rect>) {
        self.select.selection = selection;
        self.sync_toolbar_to_selection();
    }

    /// 仅更新选区矩形（不同步工具栏位置）
    pub fn update_selection_only(&mut self, selection: Option<Rect>) {
        self.select.selection = selection;
    }

    /// 同步工具栏位置到选区右下角
    pub fn sync_toolbar_to_selection(&mut self) {
        self.select.toolbar_pos = self
            .select
            .selection
            .map(|selection| selection.right_bottom());
    }

    /// 检查是否存在有效（正面积）的选区
    pub fn has_positive_selection(&self) -> bool {
        self.select
            .selection
            .map(|rect| rect.is_positive())
            .unwrap_or(false)
    }

    /// 清除工具栏位置（隐藏工具栏）
    pub fn clear_toolbar(&mut self) {
        self.select.toolbar_pos = None;
    }

    /// 撤销上一步操作
    pub fn undo_last(&mut self) {
        let Some(entry) = self.edit.history.pop() else {
            return;
        };

        let Some(inverse) = self.apply_history_entry(entry) else {
            return;
        };

        Self::push_bounded_entry(&mut self.edit.redo, inverse);
    }

    /// 重做上一步撤销的操作
    pub fn redo_last(&mut self) {
        let Some(entry) = self.edit.redo.pop() else {
            return;
        };

        let Some(inverse) = self.apply_history_entry(entry) else {
            return;
        };

        Self::push_bounded_entry(&mut self.edit.history, inverse);
    }

    /// 应用历史条目并返回其逆操作
    fn apply_history_entry(&mut self, entry: HistoryEntry) -> Option<HistoryEntry> {
        match entry {
            HistoryEntry::InsertShape { index, shape } => {
                if index <= self.edit.shapes.len() {
                    self.edit.shapes.insert(index, shape);
                    Some(HistoryEntry::RemoveShape { index })
                } else {
                    tracing::warn!(
                        "History insert-shape entry was out of bounds: index={}, len={}",
                        index,
                        self.edit.shapes.len()
                    );
                    None
                }
            }
            HistoryEntry::RemoveShape { index } => {
                if index < self.edit.shapes.len() {
                    let removed = self.edit.shapes.remove(index);
                    Some(HistoryEntry::InsertShape {
                        index,
                        shape: removed.clone_for_history(),
                    })
                } else {
                    tracing::warn!(
                        "History remove-shape entry was out of bounds: index={}, len={}",
                        index,
                        self.edit.shapes.len()
                    );
                    None
                }
            }
            HistoryEntry::ReplaceShape { index, shape } => {
                if let Some(target) = self.edit.shapes.get_mut(index) {
                    let inverse = HistoryEntry::ReplaceShape {
                        index,
                        shape: target.clone_for_history(),
                    };
                    *target = shape;
                    Some(inverse)
                } else {
                    tracing::warn!(
                        "History replace-shape entry was out of bounds: index={}, len={}",
                        index,
                        self.edit.shapes.len()
                    );
                    None
                }
            }
            HistoryEntry::RestoreSelectionAndShapes { selection, shapes } => {
                let previous_selection = self.select.selection;
                let previous_shapes = self
                    .edit
                    .shapes
                    .iter()
                    .map(DrawnShape::clone_for_history)
                    .collect();

                self.edit.shapes = shapes;
                self.set_selection(selection);

                Some(HistoryEntry::RestoreSelectionAndShapes {
                    selection: previous_selection,
                    shapes: previous_shapes,
                })
            }
        }
    }

    /// 推入撤销条目（同时清空重做栈）
    fn push_undo_entry(&mut self, entry: HistoryEntry) {
        self.edit.redo.clear();
        Self::push_bounded_entry(&mut self.edit.history, entry);
    }

    /// 推入历史条目并限制栈大小
    fn push_bounded_entry(stack: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
        stack.push(entry);
        if stack.len() > MAX_HISTORY_ENTRIES {
            stack.remove(0);
        }
    }
}

/// 捕获的屏幕快照
#[derive(Clone)]
pub struct CapturedScreen {
    /// 原始 RGBA 图像数据
    pub raw_image: Arc<RgbaImage>,
    /// 对应的显示器信息
    pub screen_info: MonitorInfo,
}

#[cfg(test)]
mod tests {
    use super::{DrawnShape, HistoryEntry, MAX_HISTORY_ENTRIES, ScreenshotState, ScreenshotTool};
    use eframe::egui::{Color32, Pos2, Rect};
    use std::sync::Arc;

    fn make_shape(start_x: f32) -> DrawnShape {
        DrawnShape {
            tool: ScreenshotTool::Rect,
            start: Pos2::new(start_x, 0.0),
            end: Pos2::new(start_x + 10.0, 10.0),
            color: Color32::WHITE,
            stroke_width: 2.0,
            text: None,
            points: None,
            cached_galley: None,
            cached_mosaic: None,
        }
    }

    #[test]
    fn selection_history_shares_shape_payloads() {
        let shared_text: Arc<str> = Arc::from("hello");
        let shared_points = Arc::new(vec![Pos2::new(1.0, 2.0), Pos2::new(3.0, 4.0)]);
        let mut state = ScreenshotState::default();
        state.edit.shapes.push(DrawnShape {
            tool: ScreenshotTool::Pen,
            start: Pos2::new(0.0, 0.0),
            end: Pos2::new(10.0, 10.0),
            color: Color32::WHITE,
            stroke_width: 2.0,
            text: Some(shared_text.clone()),
            points: Some(shared_points.clone()),
            cached_galley: None,
            cached_mosaic: None,
        });

        state.record_selection_change(None);

        let HistoryEntry::RestoreSelectionAndShapes { shapes, .. } = &state.edit.history[0] else {
            panic!("expected selection restore history entry");
        };

        let snapshot = &shapes[0];
        assert!(Arc::ptr_eq(
            snapshot.text.as_ref().expect("text missing"),
            &shared_text
        ));
        assert!(Arc::ptr_eq(
            snapshot.points.as_ref().expect("points missing"),
            &shared_points
        ));
        assert!(snapshot.cached_galley.is_none());
        assert!(snapshot.cached_mosaic.is_none());
    }

    #[test]
    fn history_is_bounded() {
        let mut state = ScreenshotState::default();

        for index in 0..=MAX_HISTORY_ENTRIES {
            state.record_shape_added(index);
        }

        assert_eq!(state.edit.history.len(), MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn undo_redo_shape_addition_roundtrip() {
        let mut state = ScreenshotState::default();
        state.edit.shapes.push(make_shape(10.0));
        state.record_shape_added(0);

        state.undo_last();
        assert!(state.edit.shapes.is_empty());
        assert!(state.edit.history.is_empty());
        assert_eq!(state.edit.redo.len(), 1);

        state.redo_last();
        assert_eq!(state.edit.shapes.len(), 1);
        assert_eq!(state.edit.shapes[0].start.x, 10.0);
        assert_eq!(state.edit.history.len(), 1);
        assert!(state.edit.redo.is_empty());
    }

    #[test]
    fn undo_redo_shape_edit_roundtrip() {
        let mut state = ScreenshotState::default();
        state.edit.shapes.push(make_shape(10.0));
        state.record_shape_before_edit(0);
        state.edit.shapes[0].start.x = 40.0;
        state.edit.shapes[0].end.x = 60.0;

        state.undo_last();
        assert_eq!(state.edit.shapes[0].start.x, 10.0);
        assert_eq!(state.edit.shapes[0].end.x, 20.0);

        state.redo_last();
        assert_eq!(state.edit.shapes[0].start.x, 40.0);
        assert_eq!(state.edit.shapes[0].end.x, 60.0);
    }

    #[test]
    fn undo_redo_selection_change_roundtrip() {
        let mut state = ScreenshotState::default();
        let original_selection = Some(Rect::from_min_max(
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 100.0),
        ));
        let changed_selection = Some(Rect::from_min_max(
            Pos2::new(10.0, 10.0),
            Pos2::new(80.0, 80.0),
        ));

        state.set_selection(original_selection);
        state.edit.shapes.push(make_shape(5.0));
        state.record_selection_change(original_selection);

        state.set_selection(changed_selection);
        state.edit.shapes.clear();

        state.undo_last();
        assert_eq!(state.select.selection, original_selection);
        assert_eq!(state.edit.shapes.len(), 1);
        assert_eq!(state.edit.shapes[0].start.x, 5.0);

        state.redo_last();
        assert_eq!(state.select.selection, changed_selection);
        assert!(state.edit.shapes.is_empty());
    }

    #[test]
    fn redo_is_cleared_after_new_edit() {
        let mut state = ScreenshotState::default();
        state.edit.shapes.push(make_shape(10.0));
        state.record_shape_added(0);
        state.undo_last();

        state.edit.shapes.push(make_shape(20.0));
        state.record_shape_added(0);

        assert!(state.edit.redo.is_empty());

        state.redo_last();
        assert_eq!(state.edit.shapes.len(), 1);
        assert_eq!(state.edit.shapes[0].start.x, 20.0);
    }
}
