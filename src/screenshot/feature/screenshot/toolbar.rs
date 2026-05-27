use super::capture::{ScreenshotAction, ScreenshotState, ScreenshotTool};
use super::state::{PRESET_COLORS, PRESET_WIDTHS, PROPERTY_PANEL_HEIGHT};
use crate::screenshot::model::device::get_screen_phys_rect;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use egui::UiBuilder;

/// 工具栏总宽度
///
/// 计算依据：
/// 6个工具按钮(192) + 4间距(32) + 分割区(17) + 5个动作按钮(160) + 4间距(32) + 两侧padding(16) = 449
const TOOLBAR_WIDTH: f32 = 465.0;
/// 工具栏总高度（仅按钮行）
const TOOLBAR_HEIGHT: f32 = 48.0;
/// 工具栏与屏幕边缘的间距
const TOOLBAR_SCREEN_PADDING: f32 = 10.0;
/// 工具栏内容区内边距
const TOOLBAR_CONTENT_PADDING: f32 = 8.0;
/// 工具栏按钮之间的间距
const TOOLBAR_ITEM_SPACING: f32 = 8.0;
/// 工具栏按钮尺寸
const TOOLBAR_BUTTON_SIZE: f32 = 32.0;
/// 分割线宽度
const TOOLBAR_DIVIDER_WIDTH: f32 = 1.0;
/// 分割线高度
const TOOLBAR_DIVIDER_HEIGHT: f32 = 16.0;
/// 颜色色块的尺寸
const COLOR_SWATCH_SIZE: f32 = 18.0;

/// 预先计算工具栏应该显示的位置和尺寸
///
/// 工具栏默认位于选区右下角外部，并执行边界防遮挡检测：
/// - 底部溢出时翻转到选区内部
/// - 左右溢出时向屏幕内收缩
///
/// 当选中有绘图工具时，工具栏会额外扩展属性面板区域的高度。
/// 返回 None 当工具栏位置未设置时
pub fn calculate_toolbar_rect(
    state: &ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
) -> Option<Rect> {
    let global_toolbar_pos_phys = state.select.toolbar_pos?;

    let vec_phys = global_toolbar_pos_phys - global_offset_phys;
    let local_pos_logical = Pos2::ZERO + (vec_phys / ppp);

    let toolbar_width = TOOLBAR_WIDTH;
    let padding = TOOLBAR_SCREEN_PADDING;

    // 当有绘图工具选中时，工具栏额外扩展属性面板高度
    let toolbar_height = if state.drawing.current_tool.is_some() {
        TOOLBAR_HEIGHT + PROPERTY_PANEL_HEIGHT
    } else {
        TOOLBAR_HEIGHT
    };

    // 默认位置：选区右下角外部
    let mut target_x = local_pos_logical.x - toolbar_width;
    let mut target_y = local_pos_logical.y + padding;

    // 找到当前工具栏所在的物理显示器边界（用于边界检测）
    let mut current_monitor_rect = None;
    for cap in &state.capture.captures {
        let cap_phys_rect = get_screen_phys_rect(&cap.screen_info);

        // 扩展几个像素以保证边界上的点也能命中
        if cap_phys_rect.expand(5.0).contains(global_toolbar_pos_phys) {
            let min_local = Pos2::ZERO + ((cap_phys_rect.min - global_offset_phys) / ppp);
            let max_local = Pos2::ZERO + ((cap_phys_rect.max - global_offset_phys) / ppp);
            current_monitor_rect = Some(Rect::from_min_max(min_local, max_local));
            break;
        }
    }

    // 边界防遮挡检测
    if let Some(screen_rect) = current_monitor_rect {
        // 底部溢出检测：翻转到选区内部
        if target_y + toolbar_height > screen_rect.max.y {
            target_y = local_pos_logical.y - toolbar_height - padding;

            // 极端情况防御：选区很扁，翻转上去又超出屏幕顶部
            if target_y < screen_rect.min.y {
                target_y = screen_rect.max.y - toolbar_height - padding;
            }
        }

        // 左右溢出检测
        if target_x < screen_rect.min.x {
            target_x = screen_rect.min.x + padding;
        } else if target_x + toolbar_width > screen_rect.max.x {
            target_x = screen_rect.max.x - toolbar_width - padding;
        }
    }

    Some(Rect::from_min_size(
        Pos2::new(target_x, target_y),
        egui::vec2(toolbar_width, toolbar_height),
    ))
}

/// 渲染工具栏以及关联的浮层
///
/// 绘制工具栏本体（背景 + 按钮 + 属性面板），并返回用户触发的动作
pub fn render_toolbar_and_overlays(
    ui: &mut Ui,
    state: &mut ScreenshotState,
    toolbar_rect: Rect,
) -> ScreenshotAction {
    let mut action = ScreenshotAction::None;
    let painter = ui.painter().clone();

    let toolbar_action = draw_screenshot_toolbar(ui, &painter, state, toolbar_rect);
    if toolbar_action != ScreenshotAction::None {
        action = toolbar_action;
    }

    action
}

/// 绘制工具栏本体
///
/// 布局结构：
/// 【左侧】绘画工具按钮（矩形、椭圆、箭头、画笔、马赛克、文本）
/// 【中间】视觉分割线
/// 【右侧】行为动作按钮（取消、复制到剪贴板、另存为、保存、置顶）
/// 【下方】属性面板（当有工具选中时显示颜色/线宽选择）
fn draw_screenshot_toolbar(
    ui: &mut Ui,
    painter: &Painter,
    state: &mut ScreenshotState,
    toolbar_rect: Rect,
) -> ScreenshotAction {
    let mut action = ScreenshotAction::None;

    // 绘制背景（白色圆角矩形 + 边框）
    painter.rect_filled(toolbar_rect, 8.0, Color32::WHITE);
    painter.rect_stroke(
        toolbar_rect,
        8.0,
        Stroke::new(1.0, super::SCREENSHOT_BORDER_COLOR),
        StrokeKind::Inside,
    );

    // 内容区域布局
    let content_rect = toolbar_rect.shrink(TOOLBAR_CONTENT_PADDING);

    ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
        ui.vertical(|ui| {
            // ============ 按钮行 ============
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing = Vec2::new(TOOLBAR_ITEM_SPACING, 0.0);
                action = draw_button_row(ui, state);
            });

            // ============ 属性面板行（当有工具选中时展开） ============
            if let Some(tool) = state.drawing.current_tool {
                ui.add_space(4.0);
                draw_property_panel(ui, state, tool);
            }
        });
    });

    action
}

/// 绘制按钮行（左侧工具 + 分割线 + 右侧动作）
fn draw_button_row(ui: &mut Ui, state: &mut ScreenshotState) -> ScreenshotAction {
    let mut action = ScreenshotAction::None;

    // =========================
    // 【左侧】绘画工具按钮
    // =========================
    let tool_buttons: [(ScreenshotTool, &str, &str); 6] = [
        (
            ScreenshotTool::Rect,
            egui_phosphor::regular::SQUARE,
            "矩形",
        ),
        (
            ScreenshotTool::Circle,
            egui_phosphor::regular::CIRCLE,
            "椭圆",
        ),
        (
            ScreenshotTool::Arrow,
            egui_phosphor::regular::ARROW_UP_RIGHT,
            "箭头",
        ),
        (
            ScreenshotTool::Pen,
            egui_phosphor::regular::PENCIL_SIMPLE,
            "画笔",
        ),
        (
            ScreenshotTool::Mosaic,
            egui_phosphor::regular::GRID_FOUR,
            "马赛克",
        ),
        (
            ScreenshotTool::Text,
            egui_phosphor::regular::TEXT_T,
            "文本",
        ),
    ];
    for (tool, icon, tooltip) in tool_buttons {
        let is_selected = state.drawing.current_tool == Some(tool);
        let resp = draw_tool_button(ui, is_selected, icon, TOOLBAR_BUTTON_SIZE)
            .on_hover_text(tooltip);
        if resp.clicked() {
            state.drawing.current_tool = Some(tool);
        }
    }

    // =========================
    // 【中间】视觉分割线
    // =========================
    ui.add_space(TOOLBAR_ITEM_SPACING);

    let (sep_rect, _) = ui.allocate_exact_size(
        Vec2::new(TOOLBAR_DIVIDER_WIDTH, TOOLBAR_DIVIDER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [sep_rect.center_top(), sep_rect.center_bottom()],
        Stroke::new(1.0, Color32::from_gray(220)),
    );

    ui.add_space(TOOLBAR_ITEM_SPACING);

    // =========================
    // 【右侧】行为动作按钮
    // =========================
    if draw_tool_button(ui, false, egui_phosphor::regular::X, TOOLBAR_BUTTON_SIZE)
        .on_hover_text("关闭")
        .clicked()
    {
        action = ScreenshotAction::Close;
    }

    if draw_tool_button(
        ui,
        false,
        egui_phosphor::regular::CLIPBOARD_TEXT,
        TOOLBAR_BUTTON_SIZE,
    )
    .on_hover_text("复制")
    .clicked()
    {
        action = ScreenshotAction::SaveToClipboard;
    }

    if draw_tool_button(
        ui,
        false,
        egui_phosphor::regular::DOWNLOAD_SIMPLE,
        TOOLBAR_BUTTON_SIZE,
    )
    .on_hover_text("另存为")
    .clicked()
    {
        action = ScreenshotAction::SaveAs;
    }

    if draw_tool_button(
        ui,
        false,
        egui_phosphor::regular::FLOPPY_DISK,
        TOOLBAR_BUTTON_SIZE,
    )
    .on_hover_text("保存")
    .clicked()
    {
        action = ScreenshotAction::SaveAndClose;
    }

    if draw_tool_button(
        ui,
        false,
        egui_phosphor::regular::PUSH_PIN,
        TOOLBAR_BUTTON_SIZE,
    )
    .on_hover_text("置顶")
    .clicked()
    {
        action = ScreenshotAction::PinToTop;
    }

    action
}

/// 绘制属性面板（工具选中时显示在按钮行下方）
///
/// 根据工具类型显示不同的属性控件：
/// - 图形类（Rect/Circle/Arrow/Pen）：颜色选择 + 线宽选择
/// - 文本工具（Text）：颜色选择 + 字号（复用 stroke_width）
/// - 马赛克（Mosaic）：马赛克宽度选择
fn draw_property_panel(ui: &mut Ui, state: &mut ScreenshotState, tool: ScreenshotTool) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing = Vec2::new(6.0, 0.0);

        let is_shape = is_shape_tool(tool);
        let is_text = tool == ScreenshotTool::Text;
        let is_mosaic = tool == ScreenshotTool::Mosaic;

        // ---- 颜色选择（图形类和文本工具都显示） ----
        if is_shape || is_text {
            for &color in &PRESET_COLORS {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::splat(COLOR_SWATCH_SIZE), egui::Sense::click());
                if ui.is_rect_visible(rect) {
                    let is_active = state.drawing.active_color == color;
                    // 绘制颜色色块
                    ui.painter().rect_filled(rect, 3.0, color);
                    if is_active {
                        ui.painter().rect_stroke(
                            rect.expand(1.0),
                            3.0,
                            Stroke::new(2.0, Color32::from_rgb(66, 133, 244)),
                            StrokeKind::Outside,
                        );
                    } else {
                        // 浅色边框便于辨识
                        ui.painter().rect_stroke(
                            rect,
                            3.0,
                            Stroke::new(0.5, Color32::from_gray(180)),
                            StrokeKind::Inside,
                        );
                    }
                }
                if resp.clicked() {
                    state.drawing.active_color = color;
                }
            }
        }

        // ---- 分割线（颜色和宽度之间） ----
        if is_shape || is_text {
            let (sep_rect, _) = ui.allocate_exact_size(
                Vec2::new(1.0, TOOLBAR_DIVIDER_HEIGHT),
                egui::Sense::hover(),
            );
            ui.painter().line_segment(
                [sep_rect.center_top(), sep_rect.center_bottom()],
                Stroke::new(1.0, Color32::from_gray(220)),
            );
            ui.add_space(2.0);
        }

        // ---- 线宽选择（图形类工具） ----
        if is_shape {
            for &w in &PRESET_WIDTHS {
                let is_active = (state.drawing.stroke_width - w).abs() < 0.1;
                let text_color = if is_active {
                    Color32::from_rgb(66, 133, 244)
                } else {
                    ui.visuals().text_color()
                };
                let label = format!("{}px", w as i32);
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(&label).size(12.0).color(text_color),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    state.drawing.stroke_width = w;
                }
            }
        }

        // ---- 字号选择（文本工具，复用 stroke_width 控制字体大小） ----
        if is_text {
            let font_sizes: [(f32, &str); 4] =
                [(1.0, "小"), (2.0, "中"), (3.0, "大"), (5.0, "特大")];
            for &(sw, label) in &font_sizes {
                let is_active = (state.drawing.stroke_width - sw).abs() < 0.1;
                let text_color = if is_active {
                    Color32::from_rgb(66, 133, 244)
                } else {
                    ui.visuals().text_color()
                };
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(label).size(12.0).color(text_color),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    state.drawing.stroke_width = sw;
                }
            }
        }

        // ---- 马赛克宽度选择 ----
        if is_mosaic {
            let mosaic_widths: [f32; 4] = [8.0, 16.0, 24.0, 32.0];
            for &w in &mosaic_widths {
                let is_active = (state.drawing.mosaic_width - w).abs() < 0.5;
                let text_color = if is_active {
                    Color32::from_rgb(66, 133, 244)
                } else {
                    ui.visuals().text_color()
                };
                let label = format!("{}px", w as i32);
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(&label).size(12.0).color(text_color),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    state.drawing.mosaic_width = w;
                }
            }
        }
    });
}

/// 绘制工具栏按钮（使用 phosphor 图标字体）
///
/// 支持三种视觉状态：
/// - 默认：透明背景
/// - 悬停：浅灰背景
/// - 选中：蓝色边框 + 浅蓝背景
fn draw_tool_button(
    ui: &mut Ui,
    is_selected: bool,
    icon: &str,
    size: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();

        // 背景色：选中 > 悬停 > 透明
        let bg_color = if is_selected {
            Color32::from_rgba_premultiplied(66, 133, 244, 40)
        } else if response.hovered() {
            visuals.widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };

        // 图标颜色：选中蓝色 > 正常文字色
        let icon_color = if is_selected {
            Color32::from_rgb(66, 133, 244)
        } else {
            visuals.text_color()
        };

        // 绘制背景
        ui.painter().rect_filled(rect, 4.0, bg_color);

        // 选中状态绘制边框
        if is_selected {
            ui.painter().rect_stroke(
                rect,
                4.0,
                Stroke::new(1.5, Color32::from_rgb(66, 133, 244)),
                StrokeKind::Inside,
            );
        }

        // 使用 phosphor 图标字体渲染图标字符
        let font_id = egui::FontId::new(
            16.0,
            egui::FontFamily::Name("phosphor-regular".into()),
        );
        let galley = ui
            .painter()
            .layout_no_wrap(icon.to_string(), font_id, icon_color);
        let icon_pos = Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(icon_pos, galley, icon_color);
    }

    response
}

/// 判断是否为图形类工具（Rect/Circle/Arrow/Pen）
fn is_shape_tool(tool: ScreenshotTool) -> bool {
    matches!(
        tool,
        ScreenshotTool::Rect | ScreenshotTool::Circle | ScreenshotTool::Arrow | ScreenshotTool::Pen
    )
}
