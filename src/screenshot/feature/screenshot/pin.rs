use eframe::egui::{
    self, Color32, Context, Frame, Id, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Vec2,
    ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, WindowLevel,
};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::Arc;

/// 投影阴影层定义：(偏移像素, alpha 值)
/// 从近到远排列，越远偏移越大、透明度越高（alpha 越低）
const SHADOW_LAYERS: [(f32, u8); 5] = [
    (1.0, 30),
    (2.0, 22),
    (3.0, 16),
    (4.0, 10),
    (5.0, 5),
];

/// 阴影最大扩展像素，视口需要为此预留空间
const SHADOW_SPREAD: f32 = SHADOW_LAYERS[SHADOW_LAYERS.len() - 1].0;

/// 单个置顶贴图的运行时状态
///
/// 内存优化关键：
/// - `image` 使用 `Arc<RgbaImage>`，避免每帧 clone 整张图像(4K 图 ≈ 30MB)
/// - `texture` 只在首帧上传一次到 GPU，后续帧直接复用 TextureHandle，
///   避免每帧重新 `ColorImage::from_rgba_unmultiplied + load_texture`
pub struct PinnedImage {
    /// 贴图纹理名称
    pub texture_name: String,
    /// 原始 RGBA 图像（Arc 共享，避免按值 clone）
    pub image: Arc<RgbaImage>,
    /// 已上传的 GPU 纹理（首帧创建后复用）
    pub texture: Option<TextureHandle>,
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
                image: Arc::new(image),
                texture: None,
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
    /// 1. 子视口尺寸必须用"逻辑像素"。
    /// 2. 回调内必须使用 CentralPanel 铺满窗口。
    /// 3. 拖动窗口通过 ViewportCommand::StartDrag。
    /// 4. 纹理在首帧 lazy 上传一次后缓存到 `PinnedImage.texture`，
    ///    后续帧直接复用句柄；图像数据用 `Arc` 在主线程和回调闭包之间共享，
    ///    避免每次进入此函数都 clone 整张大图。
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

            // 物理像素 → 父视口逻辑像素 → 再乘当前缩放 → 加上阴影扩展空间
            let logical_size = Vec2::new(
                item.image.width() as f32 / parent_ppp * item.scale + SHADOW_SPREAD,
                item.image.height() as f32 / parent_ppp * item.scale + SHADOW_SPREAD,
            );

            // ---- 首帧上传纹理：只创建一次，后续帧直接复用 ----
            // 早期实现是每帧 ColorImage::from_rgba_unmultiplied + load_texture，
            // 会在 CPU 端反复分配/复制几十 MB 的像素数据；此处改为 lazy 一次性
            if item.texture.is_none() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [item.image.width() as usize, item.image.height() as usize],
                    item.image.as_raw(),
                );
                item.texture =
                    Some(ctx.load_texture(&item.texture_name, color_image, Default::default()));
            }

            let builder = ViewportBuilder::default()
                .with_title("Pinned Image")
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_inner_size(logical_size)
                .with_position(item.pos)
                .with_window_level(WindowLevel::AlwaysOnTop)
                // 从任务栏隐藏贴图窗口
                .with_taskbar(false);

            // 共享数据进入闭包：
            // - image_arc: Arc 引用，按引用计数共享 RgbaImage(右键另存为用)
            // - texture:   TextureHandle 内部也是 Arc，clone 仅是引用计数+1
            // - 这里不会再 clone 像素数据
            let image_arc: Arc<RgbaImage> = Arc::clone(&item.image);
            let texture = item.texture.clone().expect("texture initialized above");
            let img_w = item.image.width();
            let img_h = item.image.height();

            ctx.show_viewport_deferred(viewport_id, builder, move |ui, class| {
                if class != ViewportClass::Deferred && class != ViewportClass::EmbeddedWindow {
                    return;
                }

                let cctx = ui.ctx().clone();
                egui::CentralPanel::default()
                    .frame(Frame::NONE.fill(Color32::TRANSPARENT))
                    .show_inside(ui, |ui| {
                        let full_rect = ui.max_rect();

                        // 贴图本体区域：从左上角开始，右下留出阴影扩展空间
                        let image_rect = Rect::from_min_max(
                            full_rect.min,
                            Pos2::new(
                                full_rect.max.x - SHADOW_SPREAD,
                                full_rect.max.y - SHADOW_SPREAD,
                            ),
                        );

                        // 绘制投影阴影层（在贴图本体下方）
                        for &(offset, alpha) in &SHADOW_LAYERS {
                            let shadow_rect = image_rect.translate(Vec2::new(offset, offset));
                            ui.painter().rect_filled(
                                shadow_rect,
                                0.0,
                                Color32::from_black_alpha(alpha),
                            );
                        }

                        // 分配响应区
                        let response = ui.allocate_rect(image_rect, Sense::click_and_drag());

                        // 绘制贴图本体（复用预先创建的纹理）
                        ui.painter().image(
                            texture.id(),
                            image_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        // 描边
                        ui.painter().rect_stroke(
                            image_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_black_alpha(80)),
                            StrokeKind::Inside,
                        );

                        // ---- 拖动窗口 ----
                        if response.drag_started_by(egui::PointerButton::Primary) {
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::StartDrag);
                        }

                        // 关闭闭包
                        let request_close = || {
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::Visible(false));
                            cctx.send_viewport_cmd_to(viewport_id, ViewportCommand::Close);
                        };

                        // ---- 双击关闭 ----
                        if response.double_clicked() {
                            request_close();
                        }

                        // ---- Esc 键关闭 ----
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            request_close();
                        }

                        // ---- 鼠标滚轮缩放 ----
                        let scroll_y = ui.input(|i| {
                            let events_y: f32 = i
                                .events
                                .iter()
                                .map(|e| match e {
                                    egui::Event::MouseWheel { delta, .. } => delta.y,
                                    _ => 0.0,
                                })
                                .sum();
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
                                let current =
                                    cctx.data(|d| d.get_temp::<f32>(zoom_key).unwrap_or(1.0));
                                let new_scale = (current + zoom_delta).clamp(0.2, 5.0);
                                cctx.data_mut(|d| d.insert_temp(zoom_key, new_scale));

                                let new_logical_size = Vec2::new(
                                    img_w as f32 / parent_ppp * new_scale + SHADOW_SPREAD,
                                    img_h as f32 / parent_ppp * new_scale + SHADOW_SPREAD,
                                );
                                cctx.send_viewport_cmd_to(
                                    viewport_id,
                                    ViewportCommand::InnerSize(new_logical_size),
                                );
                                cctx.request_repaint();
                            }
                        }

                        // ---- 右键菜单：另存为 / 关闭 ----
                        // image_arc 已是 Arc，进一步 clone 仅是引用计数+1
                        let image_for_menu = Arc::clone(&image_arc);
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
                                ctx_for_menu
                                    .send_viewport_cmd_to(vid, ViewportCommand::Visible(false));
                                ctx_for_menu.send_viewport_cmd_to(vid, ViewportCommand::Close);
                                ui.close();
                            }
                        });

                        // ---- 系统关闭请求 ----
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
    /// 关闭一个贴图意味着：丢弃 PinnedImage（其 image Arc 与 texture 都会被 drop，
    /// 进而触发 ctx.tex_manager 卸载 GPU 纹理），显著降低长期持有的内存占用。
    fn gc_closed(&mut self, ctx: &Context) {
        self.items.retain(|viewport_id, item| {
            let close_requested = ctx.input_for(*viewport_id, |i| i.viewport().close_requested());
            !close_requested && !item.should_close
        });
    }
}
