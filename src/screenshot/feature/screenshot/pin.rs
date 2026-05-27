use eframe::egui::{
    self, Color32, Context, Frame, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
    ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, WindowLevel,
};
use image::RgbaImage;
use std::collections::HashMap;

/// 单个置顶贴图的运行时状态
#[derive(Clone)]
pub struct PinnedImage {
    /// 贴图纹理名称
    pub texture_name: String,
    /// 原始 RGBA 图像
    pub image: RgbaImage,
    /// 贴图屏幕位置（逻辑坐标）
    pub pos: Pos2,
    /// 当前缩放比例
    pub scale: f32,
    /// 是否请求关闭
    pub should_close: bool,
}

/// 置顶贴图管理器
#[derive(Default)]
pub struct PinnedImageManager {
    /// 当前所有贴图（key = 视口 ID）
    items: HashMap<ViewportId, PinnedImage>,
    /// 自增计数器，用于生成稳定 viewport id
    next_id: u64,
}

impl PinnedImageManager {
    /// 创建管理器
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个新的置顶贴图
    pub fn add_image(&mut self, ctx: &Context, image: RgbaImage, pos: Pos2) {
        let viewport_id = ViewportId::from_hash_of(("pinned_image", self.next_id));
        self.next_id += 1;

        let texture_name = format!("pinned_image_{}", self.next_id);
        self.items.insert(
            viewport_id,
            PinnedImage {
                texture_name,
                image,
                pos,
                scale: 1.0,
                should_close: false,
            },
        );

        ctx.request_repaint();
    }

    /// 渲染所有置顶贴图子视口
    ///
    /// 关键点：
    /// 1. 子视口尺寸必须用"逻辑像素"（egui 的 with_inner_size 使用逻辑像素），
    ///    而 `item.image` 是物理像素，需要除以父视口 ppp 再乘 scale。
    /// 2. 回调内必须使用 CentralPanel 铺满窗口，否则 ui.max_rect 不一定
    ///    覆盖整个客户区，导致 allocate_rect 出的 response 收不到点击/拖拽/滚轮。
    /// 3. 拖动窗口通过 ViewportCommand::StartDrag 交给系统处理，要求左键
    ///    刚按下时立即调用一次。
    pub fn show_viewports(&mut self, ctx: &Context) {
        // 父视口的像素缩放，用于把图像物理像素换算成子视口的逻辑像素
        let parent_ppp = ctx.pixels_per_point().max(1.0);

        let viewport_ids: Vec<ViewportId> = self.items.keys().copied().collect();

        for viewport_id in viewport_ids {
            let Some(item) = self.items.get_mut(&viewport_id) else {
                continue;
            };

            // 从临时数据读取缩放（上一帧滚轮写入的）
            let zoom_key = Id::new(("pinned_zoom", viewport_id));
            ctx.data(|d| {
                if let Some(saved_scale) = d.get_temp::<f32>(zoom_key) {
                    item.scale = saved_scale;
                }
            });

            // 物理像素 → 父视口逻辑像素 → 再乘当前缩放
            let logical_size = Vec2::new(
                item.image.width() as f32 / parent_ppp * item.scale,
                item.image.height() as f32 / parent_ppp * item.scale,
            );

            let builder = ViewportBuilder::default()
                .with_title("Pinned Image")
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_inner_size(logical_size)
                .with_position(item.pos)
                .with_window_level(WindowLevel::AlwaysOnTop)
                // 从任务栏隐藏贴图窗口，避免每张贴图都出现独立的任务栏图标
                .with_taskbar(false);

            let texture_name = item.texture_name.clone();
            let image = item.image.clone();

            ctx.show_viewport_deferred(viewport_id, builder, move |ui, class| {
                if class != ViewportClass::Deferred && class != ViewportClass::EmbeddedWindow {
                    return;
                }

                // 用 CentralPanel::show_inside 铺满整个子视口客户区，确保后续
                // allocate 的响应区域真正覆盖窗口，并能接收点击/拖拽/滚轮/右键事件
                let cctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(Frame::NONE.fill(Color32::TRANSPARENT))
                    .show_inside(ui, |ui| {
                        // 加载贴图纹理（每帧创建开销可接受，纹理由 ctx 内部缓存）
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        );
                        let texture =
                            cctx.load_texture(&texture_name, color_image, Default::default());

                        let full_rect = ui.max_rect();

                        // 分配一个铺满窗口的响应区，开启 click_and_drag 以同时支持
                        // 点击关闭、拖动窗口、右键菜单
                        let response = ui.allocate_rect(full_rect, Sense::click_and_drag());

                        // 绘制贴图本体（铺满整个窗口）
                        ui.painter().image(
                            texture.id(),
                            full_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        // 绘制 1 像素细描边便于辨识贴图边界
                        ui.painter().rect_stroke(
                            full_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_black_alpha(80)),
                            StrokeKind::Inside,
                        );

                        // ---- 拖动窗口：左键按下立即触发 StartDrag ----
                        // 注意：StartDrag 必须在"刚按下"那一帧调用才会被 winit 接受，
                        // 因此使用 drag_started() 而不是 dragged()。
                        if response.drag_started_by(egui::PointerButton::Primary) {
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::StartDrag);
                        }

                        // 关闭辅助闭包：先隐藏窗口再 Close，避免 OS 关闭动画里
                        // 闪一下"上一帧旧尺寸"的图像
                        let request_close = || {
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::Visible(false));
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::Close);
                        };

                        // ---- 双击关闭 ----
                        if response.double_clicked() {
                            request_close();
                        }

                        // ---- Esc 键关闭（仅当鼠标悬停在贴图上或贴图获得焦点时生效）----
                        // 子视口默认收不到全局键盘事件，但若窗口在前台并获得焦点，
                        // egui 会把按键事件路由进来。这里只要本视口收到 Escape 就关闭。
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            request_close();
                        }

                        // ---- 鼠标滚轮缩放（实时）----
                        // 直接从当前 ui 获取输入（CentralPanel 内部）
                        // 包含平滑滚动，以及触控板/Ctrl+滚轮的缩放比例
                        let scroll_y = ui.input(|i| {
                            // egui 0.34 中没有 raw_scroll_delta，可以通过 events 手动获取或直接用 smooth_scroll_delta
                            let events_y: f32 = i.events.iter().map(|e| match e {
                                egui::Event::MouseWheel { delta, .. } => delta.y,
                                _ => 0.0,
                            }).sum();
                            if events_y.abs() > 0.0 {
                                events_y
                            } else {
                                i.smooth_scroll_delta.y
                            }
                        });
                        let zoom_factor = ui.input(|i| i.zoom_delta());

                        if response.hovered() {
                            let mut zoom_delta = 0.0;
                            if scroll_y > 0.0 {
                                zoom_delta = 0.1;
                            } else if scroll_y < 0.0 {
                                zoom_delta = -0.1;
                            } else if zoom_factor > 1.05 {
                                zoom_delta = 0.1;
                            } else if zoom_factor < 0.95 {
                                zoom_delta = -0.1;
                            }

                            if zoom_delta != 0.0 {
                                let current = cctx.data(|d| d.get_temp::<f32>(zoom_key).unwrap_or(1.0));
                                let new_scale = (current + zoom_delta).clamp(0.2, 5.0);
                                cctx.data_mut(|d| d.insert_temp(zoom_key, new_scale));
                                
                                let new_logical_size = Vec2::new(
                                    image.width() as f32 / parent_ppp * new_scale,
                                    image.height() as f32 / parent_ppp * new_scale,
                                );
                                cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::InnerSize(new_logical_size));
                                cctx.request_repaint();
                            }
                        }

                        // ---- 右键菜单：另存为 / 关闭 ----
                        let image_for_menu = image.clone();
                        let vid = viewport_id;
                        let ctx_for_menu = cctx.clone();
                        response.context_menu(move |ui| {
                            if ui.button("另存为").clicked() {
                                let file = rfd::FileDialog::new()
                                    .set_file_name("pinned_image.png")
                                    .add_filter("PNG 图片", &["png"])
                                    .save_file();
                                if let Some(path) = file {
                                    let _ = image_for_menu.save(path);
                                }
                                ui.close();
                            }
                            if ui.button("关闭").clicked() {
                                // 与上面 request_close 行为一致：先隐藏再 Close
                                ctx_for_menu
                                    .send_viewport_cmd_to(vid, ViewportCommand::Visible(false));
                                ctx_for_menu.send_viewport_cmd_to(vid, ViewportCommand::Close);
                                ui.close();
                            }
                        });

                        // ---- 处理窗口自身的关闭请求（来自系统/Alt+F4 等） ----
                        if cctx.input(|i| i.viewport().close_requested()) {
                            request_close();
                        }
                    });
            });
        }

        self.gc_closed(ctx);
    }

    /// 清理已关闭的贴图视口
    ///
    /// 使用 `input_for_viewport` 检查每个子视口自身的关闭请求，
    /// 而非主视口的关闭状态（原实现会误判）
    fn gc_closed(&mut self, ctx: &Context) {
        self.items.retain(|viewport_id, item| {
            let close_requested =
                ctx.input_for(*viewport_id, |i| i.viewport().close_requested());
            !close_requested && !item.should_close
        });
    }
}
