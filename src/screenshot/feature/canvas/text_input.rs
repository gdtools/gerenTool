use eframe::egui::{Color32, Id, Pos2, Rect, Stroke, Ui};

use crate::screenshot::feature::canvas::{commit_text_shape, phys_to_local};
use crate::screenshot::feature::screenshot::capture::{DrawnShape, ScreenshotState};

const TEXT_EDIT_ID: &str = "screenshot_text_edit";

/// 渲染文本输入框
///
/// 当用户选择文本工具并点击画布后，在点击位置显示一个浮动的文本编辑框：
/// 1. 根据选区边界自动调整文本框位置和最大宽度
/// 2. 使用 egui::Area 实现浮动定位
/// 3. 支持 Shift+Enter 换行，Enter 提交
/// 4. 焦点丢失时自动提交文本为 DrawnShape
/// 5. 显示虚线边框指示编辑区域
pub fn render_text_input(
    ui: &mut Ui,
    state: &mut ScreenshotState,
    global_offset_phys: Pos2,
    ppp: f32,
) {
    if let Some((pos_phys, mut text)) = state.input.active_text_input.clone() {
        let font_size = DrawnShape::text_font_size(state.drawing.stroke_width);
        let mut pos_local = phys_to_local(pos_phys, global_offset_phys, ppp);

        // 默认文本框尺寸（逻辑坐标）
        let default_box_width = font_size + 16.0;
        let default_box_height = font_size + 24.0;

        // 根据选区边界调整文本框位置和最大宽度
        let (max_width, clip_rect) = if let Some(sel) = state.select.selection {
            let sel_min_x_local = Pos2::ZERO.x + ((sel.min.x - global_offset_phys.x) / ppp);
            let sel_max_x_local = Pos2::ZERO.x + ((sel.max.x - global_offset_phys.x) / ppp);
            let sel_min_y_local = Pos2::ZERO.y + ((sel.min.y - global_offset_phys.y) / ppp);
            let sel_max_y_local = Pos2::ZERO.y + ((sel.max.y - global_offset_phys.y) / ppp);

            // 右侧空间不够时左移，保留 8px 右边距
            if sel_max_x_local - pos_local.x < default_box_width {
                pos_local.x = (sel_max_x_local - default_box_width).max(sel_min_x_local);
            }

            // 下方空间不够时上移，保留 8px 下边距
            if sel_max_y_local - pos_local.y < default_box_height {
                pos_local.y = (sel_max_y_local - default_box_height - 8.0).max(sel_min_y_local);
            }

            let adjusted_width = (sel_max_x_local - pos_local.x).max(20.0);
            let sel_clip = Rect::from_min_max(
                Pos2::new(sel_min_x_local, sel_min_y_local),
                Pos2::new(sel_max_x_local, sel_max_y_local),
            );
            (adjusted_width, Some(sel_clip))
        } else {
            (1000.0, None)
        };

        // 将调整后的本地坐标转回物理坐标，用于提交和状态更新
        let adjusted_pos_phys = global_offset_phys + (pos_local - Pos2::ZERO) * ppp;

        let area_id = Id::new("screenshot_text_input");
        let text_edit_id = area_id.with(TEXT_EDIT_ID);

        egui::Area::new(area_id)
            .fixed_pos(pos_local)
            .order(egui::Order::Foreground)
            .show(ui, |ui| {
                if let Some(rect) = clip_rect {
                    ui.set_clip_rect(rect);
                }
                let font_id = egui::FontId::proportional(font_size);

                // 预计算文本宽度，用于动态调整编辑框宽度
                let galley =
                    ui.painter()
                        .layout_no_wrap(text.clone(), font_id.clone(), Color32::WHITE);
                let text_width = galley.size().x + 8.0;
                let dynamic_width = text_width.max(10.0).min(max_width);

                let frame = egui::Frame::default()
                    .fill(Color32::from_black_alpha(150))
                    .inner_margin(8.0)
                    .corner_radius(4.0);

                let frame_response = frame.show(ui, |ui| {
                    ui.set_max_width(max_width);

                    // 拦截 Enter 键：只有 Shift+Enter 才允许换行，单独 Enter 用于提交
                    ui.input_mut(|i| {
                        let shift_pressed = i.modifiers.shift;

                        i.events.retain(|event| match event {
                            egui::Event::Key {
                                key: egui::Key::Enter,
                                pressed: true,
                                ..
                            } => shift_pressed,
                            egui::Event::Text(t) if t == "\n" || t == "\r\n" => shift_pressed,
                            _ => true,
                        });
                    });

                    let response = ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .id(text_edit_id)
                            .font(font_id)
                            .text_color(state.drawing.active_color)
                            .frame(egui::Frame::NONE)
                            .desired_rows(1)
                            .desired_width(dynamic_width),
                    );

                    // 首次创建时请求焦点
                    if state.input.active_text_input.is_some()
                        && !response.has_focus()
                        && !response.lost_focus()
                    {
                        response.request_focus();
                    }

                    // 焦点丢失时处理文本提交
                    if response.lost_focus() && !text.trim().is_empty() {
                        state.input.active_text_input = None;
                        commit_text_shape(
                            ui,
                            state,
                            adjusted_pos_phys,
                            text,
                            global_offset_phys,
                            ppp,
                        );
                    } else if response.lost_focus() {
                        // 空文本直接丢弃
                        state.input.active_text_input = None;
                    } else {
                        // 更新文本内容（用户正在输入）
                        state.input.active_text_input = Some((adjusted_pos_phys, text));
                    }
                });

                // 绘制虚线边框（指示编辑区域边界）
                let rect = frame_response.response.rect;
                let stroke = Stroke::new(1.5, super::SCREENSHOT_BORDER_COLOR);
                let painter = ui.painter();
                painter.add(egui::Shape::dashed_line(
                    &[rect.left_top(), rect.right_top()],
                    stroke,
                    5.0,
                    4.0,
                ));
                painter.add(egui::Shape::dashed_line(
                    &[rect.right_top(), rect.right_bottom()],
                    stroke,
                    5.0,
                    4.0,
                ));
                painter.add(egui::Shape::dashed_line(
                    &[rect.right_bottom(), rect.left_bottom()],
                    stroke,
                    5.0,
                    4.0,
                ));
                painter.add(egui::Shape::dashed_line(
                    &[rect.left_bottom(), rect.left_top()],
                    stroke,
                    5.0,
                    4.0,
                ));
            });
    }
}
