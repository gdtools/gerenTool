use crate::screenshot::feature::screenshot::capture::{
    draw_screenshot_ui_inside, finalize_screenshot_action, prepare_screenshot_frame,
};
use crate::screenshot::model::state::CommonState;
use eframe::egui::{Color32, Context, Frame, Rect, Ui};
use egui_toast::{Toast, ToastOptions, Toasts};
use std::sync::mpsc::{Receiver, Sender, channel};


pub mod capture;
pub mod draw;
pub mod pin;
pub mod scroll_capture;
pub mod state;
pub mod toolbar;

use self::pin::PinnedImageManager;
use self::scroll_capture::DelayCaptureOutput;
use self::state::{DrawnShape, ScreenshotState, WindowPrevState};
use crate::screenshot::hotkey::HotkeyAction;

/// 截图功能中通用的边框颜色
pub const SCREENSHOT_BORDER_COLOR: Color32 = Color32::from_gray(200);

/// 应用运行模式
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum AppMode {
    /// 正常设置界面
    Idle,
    /// 截图模式
    Screenshot,
}

/// 截图功能核心结构体
///
/// 管理截图模式的完整生命周期：
/// - 通过热键进入截图模式
/// - 每帧驱动截图准备（屏幕捕获 + 视口配置）
/// - 渲染截图 UI（画布 + 工具栏）
/// - 处理截图完成动作（保存/取消）
pub struct ScreenshotFeature {
    /// 截图状态机
    pub state: ScreenshotState,
    /// 内部状态：是否处于截图模式
    is_active: bool,
    /// 置顶贴图管理器
    pinned_images: PinnedImageManager,
    /// 置顶动作触发后，延迟若干帧再真正关闭截图窗口
    ///
    /// 用途：避免"主截图窗口立即恢复 → 贴图视口尚未创建"中间出现一帧白屏闪烁。
    /// 在此倒计时期间：
    /// - 主截图窗口立刻隐藏（Visible(false)），不再绘制内容
    /// - 贴图子视口持续渲染（在 app.rs::ui() 顶层无条件调用）
    /// - 计数到 0 时再执行完整的窗口恢复流程
    pending_pin_close_frames: u8,
    /// 延时截图：剩余等待秒数（None 表示未启用）
    delay_capture_remaining_secs: Option<u32>,
    /// 延时截图：上次秒数递减的时间戳
    delay_capture_last_tick: Option<std::time::Instant>,
    /// 延时截图：触发时保存的选区矩形（物理坐标）
    delay_capture_selection: Option<Rect>,
    /// 延时截图：触发时保存的已绘制形状列表
    delay_capture_shapes: Vec<DrawnShape>,
    /// 延时截图：触发时保存的像素缩放比（用于计算贴图逻辑坐标）
    delay_capture_ppp: f32,
    /// 延时截图：后台线程结果接收通道（每次触发时新建）
    delay_capture_rx: Option<Receiver<Result<DelayCaptureOutput, String>>>,
    /// 滚动截图：控制器发送通道
    scroll_control_tx: Option<std::sync::mpsc::Sender<scroll_capture::ScrollControlMessage>>,
    /// 滚动截图：结果接收通道
    scroll_result_rx: Option<std::sync::mpsc::Receiver<scroll_capture::ScrollResultMessage>>,
    /// 截图覆盖层内部 Toast 队列（在覆盖层 UI 上直接渲染，用于保存/复制/延时触发提示）
    overlay_toasts: Vec<ToastMessage>,
    /// Toast 消息发送通道（向 app 层推送提示文本，用于延时截图完成等覆盖层已关闭的场景）
    pub toast_tx: Sender<ToastMessage>,
}

/// 向 app 层推送的 Toast 消息
pub struct ToastMessage {
    /// 提示文本
    pub text: String,
    /// 消息类型
    pub kind: ToastKind,
}

/// Toast 消息类型
#[derive(Clone, Copy)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

impl ScreenshotFeature {
    /// 创建截图功能实例，返回 (feature, toast_rx)
    ///
    /// `toast_rx` 由 app 层持有，每帧 drain 并展示 Toast
    pub fn new() -> (Self, Receiver<ToastMessage>) {
        let (toast_tx, toast_rx) = channel::<ToastMessage>();

        let feature = Self {
            state: ScreenshotState::default(),
            is_active: false,
            pinned_images: PinnedImageManager::new(),
            pending_pin_close_frames: 0,
            delay_capture_remaining_secs: None,
            delay_capture_last_tick: None,
            delay_capture_selection: None,
            delay_capture_shapes: Vec::new(),
            delay_capture_ppp: 1.0,
            delay_capture_rx: None,
            scroll_control_tx: None,
            scroll_result_rx: None,
            overlay_toasts: Vec::new(),
            toast_tx,
        };
        (feature, toast_rx)
    }

    /// 进入截图模式，重置所有状态
    pub fn enter_screenshot_mode(&mut self, prev_state: WindowPrevState) {
        self.state = ScreenshotState::new(prev_state);
        self.is_active = true;
    }

    /// 查询当前是否处于截图激活状态
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// 渲染所有置顶贴图子视口
    ///
    /// 必须在每帧无条件调用（无论当前是否为截图模式），
    /// 否则截图关闭后 mode 切回 Idle，贴图窗口将不再被维护和渲染。
    pub fn show_pinned_viewports(&mut self, ctx: &Context) {
        self.pinned_images.show_viewports(ctx);
    }
}

impl Default for ScreenshotFeature {
    fn default() -> Self {
        Self::new().0
    }
}

impl ScreenshotFeature {
    /// 处理热键事件，返回需要切换到的目标模式
    ///
    /// 当收到截图热键动作时，进入截图模式并返回 `AppMode::Screenshot`
    pub fn handle_hotkey(&mut self, action: HotkeyAction) -> Option<AppMode> {
        match action {
            HotkeyAction::SetScreenshotMode { prev_state } => {
                self.enter_screenshot_mode(prev_state);
                Some(AppMode::Screenshot)
            }
        }
    }
}

impl ScreenshotFeature {
    /// 每帧逻辑更新：驱动截图准备流程
    ///
    /// 在截图模式下依次执行：
    /// 1. 屏幕捕获（异步线程）
    /// 2. 视口配置（窗口全屏覆盖）
    ///
    /// 当截图流程结束（主动取消或完成），将模式切回 Idle
    pub fn logic(&mut self, ctx: &Context, common: &mut CommonState, mode: &mut AppMode) {
        // 处理置顶后的延迟关闭：每帧递减，归零时执行真正的窗口恢复
        // 期间持续请求重绘，保证倒计时能稳定推进
        if self.pending_pin_close_frames > 0 {
            self.pending_pin_close_frames -= 1;
            ctx.request_repaint();
            if self.pending_pin_close_frames == 0 {
                use crate::screenshot::feature::screenshot::state::ScreenshotAction;
                finalize_screenshot_action(ctx, &mut self.state, common, ScreenshotAction::Close);
                *mode = AppMode::Idle;
                return;
            }
        }

        // 处理延时截图倒计时：每秒递减，归零时启动后台截屏线程
        if let Some(remaining) = self.delay_capture_remaining_secs {
            let now = std::time::Instant::now();
            let last_tick = self.delay_capture_last_tick.get_or_insert(now);
            if now.duration_since(*last_tick).as_secs() >= 1 {
                *last_tick = now;
                if remaining <= 1 {
                    self.delay_capture_remaining_secs = None;
                    self.delay_capture_last_tick = None;
                    let selection = self.delay_capture_selection.take();
                    let shapes = std::mem::take(&mut self.delay_capture_shapes);
                    let ppp = self.delay_capture_ppp;
                    let (tx, rx) = channel::<Result<DelayCaptureOutput, String>>();
                    self.delay_capture_rx = Some(rx);
                    scroll_capture::start_delay_capture(selection, shapes, ppp, tx);
                } else {
                    self.delay_capture_remaining_secs = Some(remaining - 1);
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }

        // 检查延时截图后台线程是否已完成，完成后创建置顶贴图
        if let Some(rx) = &self.delay_capture_rx {
            match rx.try_recv() {
                Ok(Ok(output)) => {
                    self.pinned_images.add_image(ctx, output.image, output.pos);
                    let _ = self.toast_tx.send(ToastMessage {
                        text: "延时截图已置顶".to_string(),
                        kind: ToastKind::Success,
                    });
                    self.delay_capture_rx = None;
                    ctx.request_repaint();
                }
                Ok(Err(e)) => {
                    tracing::error!("延时截图失败: {}", e);
                    let _ = self.toast_tx.send(ToastMessage {
                        text: format!("延时截图失败: {}", e),
                        kind: ToastKind::Error,
                    });
                    self.delay_capture_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.delay_capture_rx = None;
                }
            }
        }

        // 检查滚动截图后台线程是否已完成，完成后保存并提示
        if let Some(rx) = &self.scroll_result_rx {
            match rx.try_recv() {
                Ok(scroll_capture::ScrollResultMessage::Success(path)) => {
                    let filename = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let _ = self.toast_tx.send(ToastMessage {
                        text: format!("滚动长图已保存到桌面: {}", filename),
                        kind: ToastKind::Success,
                    });
                    self.scroll_result_rx = None;
                    self.scroll_control_tx = None;
                    self.state.runtime.scroll_capture = crate::screenshot::feature::screenshot::state::ScrollCapturePhase::Idle;
                    self.is_active = false;
                    use crate::screenshot::feature::screenshot::state::ScreenshotAction;
                    finalize_screenshot_action(ctx, &mut self.state, common, ScreenshotAction::Close);
                    *mode = AppMode::Idle;
                    ctx.request_repaint();
                }
                Ok(scroll_capture::ScrollResultMessage::Error(e)) => {
                    tracing::error!("滚动截图失败: {}", e);
                    let _ = self.toast_tx.send(ToastMessage {
                        text: format!("滚动截图失败: {}", e),
                        kind: ToastKind::Error,
                    });
                    self.scroll_result_rx = None;
                    self.scroll_control_tx = None;
                    self.state.runtime.scroll_capture = crate::screenshot::feature::screenshot::state::ScrollCapturePhase::Idle;
                    self.is_active = false;
                    use crate::screenshot::feature::screenshot::state::ScreenshotAction;
                    finalize_screenshot_action(ctx, &mut self.state, common, ScreenshotAction::Close);
                    *mode = AppMode::Idle;
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.scroll_result_rx = None;
                    self.scroll_control_tx = None;
                }
            }
        }

        if *mode == AppMode::Screenshot && !self.is_active {
            self.enter_screenshot_mode(WindowPrevState::Normal);
        }

        if *mode != AppMode::Screenshot {
            return;
        }

        if !self.is_active {
            if self.pending_pin_close_frames == 0 {
                *mode = AppMode::Idle;
            }
            return;
        }

        // 驱动截图准备流程（截屏捕获 + 窗口配置）
        if !prepare_screenshot_frame(ctx, &mut self.is_active, &mut self.state, common)
            && !self.is_active
        {
            *mode = AppMode::Idle;
        }
    }

    /// 每帧 UI 渲染：绘制截图界面
    ///
    /// 在截图准备完成后，渲染完整的截图 UI：
    /// - 屏幕截图纹理
    /// - 画布交互和绘制元素
    /// - 工具栏
    ///
    /// 当用户执行完成动作（保存/取消）后，清理资源并切回 Idle 模式
    pub fn ui(&mut self, ui: &mut Ui, common: &mut CommonState, mode: &mut AppMode) {
        if *mode != AppMode::Screenshot || !self.is_active {
            return;
        }

        if self.pending_pin_close_frames > 0 {
            return;
        }

        if self.state.capture.captures.is_empty() || !self.state.runtime.window_configured {
            return;
        }

        let action = egui::CentralPanel::default()
            .frame(Frame::NONE.fill(egui::Color32::TRANSPARENT))
            .show_inside(ui, |ui| {
                draw_screenshot_ui_inside(ui, &mut self.state, &common.device_info)
            })
            .inner;

        use crate::screenshot::feature::screenshot::state::ScreenshotAction;

        match action {
            // ---- 置顶：提取裁剪图像 → 创建置顶视口 → 延迟关闭截图 ----
            ScreenshotAction::PinToTop => {
                if let Some(image) = capture::extract_cropped_image(&self.state)
                    && let Some(selection) = self.state.select.selection
                {
                    let ppp = ui.ctx().pixels_per_point();
                    let pos = egui::pos2(selection.min.x / ppp, selection.min.y / ppp);
                    self.pinned_images.add_image(ui.ctx(), image, pos);
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.is_active = false;
                    self.pending_pin_close_frames = 2;
                    ui.ctx().request_repaint();
                }
            }
            // ---- 延时截图：保存选区和形状，关闭覆盖层，启动倒计时 ----
            ScreenshotAction::DelayCapture(secs) => {
                self.delay_capture_selection = self.state.select.selection;
                self.delay_capture_shapes =
                    self.state.edit.shapes.iter().map(|s| s.clone()).collect();
                self.delay_capture_ppp = ui.ctx().pixels_per_point();
                self.delay_capture_remaining_secs = Some(secs);
                self.delay_capture_last_tick = Some(std::time::Instant::now());
                // 在覆盖层关闭前先渲染一帧 Toast 提示
                self.overlay_toasts.push(ToastMessage {
                    text: format!("{} 秒后截图", secs),
                    kind: ToastKind::Info,
                });
                self.is_active = false;
                finalize_screenshot_action(
                    ui.ctx(),
                    &mut self.state,
                    common,
                    ScreenshotAction::Close,
                );
            }
            // ---- 滚动截图：开始滚动截图后台流程，保持覆盖层全屏展示 ----
            ScreenshotAction::ScrollCapture => {
                if let Some(selection) = self.state.select.selection {
                    let (control_tx, control_rx) = std::sync::mpsc::channel();
                    let (result_tx, result_rx) = std::sync::mpsc::channel();

                    self.scroll_control_tx = Some(control_tx);
                    self.scroll_result_rx = Some(result_rx);

                    scroll_capture::start_scroll_capture_thread(common.window_state.hwnd_usize, selection, control_rx, result_tx);

                    // 设置状态为运行中
                    self.state.runtime.scroll_capture = crate::screenshot::feature::screenshot::state::ScrollCapturePhase::Running {
                        long_image: image::RgbaImage::new(1, 1),
                        prev_frame: image::RgbaImage::new(1, 1),
                        selection,
                        last_capture: std::time::Instant::now(),
                    };

                    self.overlay_toasts.push(ToastMessage {
                        text: "滚动截图已启动，移出选区自动暂停".to_string(),
                        kind: ToastKind::Info,
                    });
                    ui.ctx().request_repaint();
                }
            }
            // ---- 停止滚动截图：向后台线程发送停止命令 ----
            ScreenshotAction::StopScrollCapture => {
                if let Some(tx) = &self.scroll_control_tx {
                    let _ = tx.send(scroll_capture::ScrollControlMessage::Stop);
                }
                self.state.runtime.scroll_capture = crate::screenshot::feature::screenshot::state::ScrollCapturePhase::Idle;
            }
            // ---- 关闭类动作：标记非活跃 → 执行保存和窗口恢复 ----
            ScreenshotAction::SaveAndClose => {
                self.overlay_toasts.push(ToastMessage {
                    text: "已保存到桌面".to_string(),
                    kind: ToastKind::Success,
                });
                self.is_active = false;
                finalize_screenshot_action(ui.ctx(), &mut self.state, common, action);
            }
            ScreenshotAction::SaveToClipboard => {
                self.overlay_toasts.push(ToastMessage {
                    text: "已复制到剪贴板".to_string(),
                    kind: ToastKind::Success,
                });
                self.is_active = false;
                finalize_screenshot_action(ui.ctx(), &mut self.state, common, action);
            }
            ScreenshotAction::SaveAs => {
                self.overlay_toasts.push(ToastMessage {
                    text: "已另存为文件".to_string(),
                    kind: ToastKind::Success,
                });
                self.is_active = false;
                finalize_screenshot_action(ui.ctx(), &mut self.state, common, action);
            }
            ScreenshotAction::Close => {
                self.is_active = false;
                finalize_screenshot_action(ui.ctx(), &mut self.state, common, action);
            }
            // ---- 无动作：跳过 ----
            ScreenshotAction::None => {}
        }

        // 在截图覆盖层上渲染内部 Toast（本帧入队，本帧渲染，存入 egui data 持续显示）
        {
            let mut toasts = Toasts::new()
                .anchor(egui::Align2::RIGHT_BOTTOM, (-16.0, -16.0))
                .direction(egui::Direction::BottomUp)
                .order(egui::Order::Tooltip);
            for msg in self.overlay_toasts.drain(..) {
                let kind = match msg.kind {
                    ToastKind::Success => egui_toast::ToastKind::Success,
                    ToastKind::Error => egui_toast::ToastKind::Error,
                    ToastKind::Info => egui_toast::ToastKind::Info,
                };
                toasts.add(Toast {
                    text: msg.text.into(),
                    kind,
                    options: ToastOptions::default()
                        .duration_in_seconds(2.5)
                        .show_progress(true)
                        .show_icon(true),
                    ..Default::default()
                });
            }
            toasts.show(ui);
        }

        if !self.is_active {
            *mode = AppMode::Idle;
        }
    }
}
