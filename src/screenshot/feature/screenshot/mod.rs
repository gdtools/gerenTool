use crate::screenshot::feature::screenshot::capture::{
    draw_screenshot_ui_inside, finalize_screenshot_action, prepare_screenshot_frame,
};
use crate::screenshot::model::state::CommonState;
use eframe::egui::{Color32, Context, Frame, Ui};

pub mod capture;
pub mod draw;
pub mod pin;
pub mod state;
pub mod toolbar;

use self::pin::PinnedImageManager;
use self::state::{ScreenshotState, WindowPrevState};
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
}

impl ScreenshotFeature {
    /// 创建截图功能实例
    pub fn new() -> Self {
        Self {
            state: ScreenshotState::default(),
            is_active: false,
            pinned_images: PinnedImageManager::new(),
            pending_pin_close_frames: 0,
        }
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
        Self::new()
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
    /// 3. 处理延迟的"另存为"操作（在非渲染阶段安全弹出文件对话框）
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

        if *mode == AppMode::Screenshot && !self.is_active {
            self.enter_screenshot_mode(WindowPrevState::Normal);
        }

        if *mode != AppMode::Screenshot {
            return;
        }

        if !self.is_active {
            // 若正在等待置顶延迟关闭，不要立即把 mode 切回 Idle
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
        // 注意：置顶贴图渲染已移至 app.rs::ui() 中无条件调用，
        // 不在此处重复调用（否则仅在截图模式下才渲染，置顶窗口会随模式切换而消失）

        if *mode != AppMode::Screenshot || !self.is_active {
            return;
        }

        // 若处于置顶延迟关闭的过渡帧，不再渲染截图 UI（避免出现白底闪烁）
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
            //
            // 关键：不立即调用 finalize_screenshot_action，否则会出现一帧
            // "主截图窗口已重置但贴图视口还未在 OS 层创建出来"的空窗期，
            // 表现为选区位置闪一下白色。改为：
            // 1. 立刻把主截图视口隐藏（Visible(false)），停止它继续绘制
            // 2. 设置 pending_pin_close_frames，由 logic() 倒计时几帧后再做完整恢复
            // 期间贴图视口由 app.rs::ui() 顶层无条件渲染，能稳定显示
            ScreenshotAction::PinToTop => {
                if let Some(image) = capture::extract_cropped_image(&self.state)
                    && let Some(selection) = self.state.select.selection
                {
                    let ppp = ui.ctx().pixels_per_point();
                    let pos = egui::pos2(selection.min.x / ppp, selection.min.y / ppp);
                    self.pinned_images.add_image(ui.ctx(), image, pos);

                    // 立即隐藏主截图视口，阻止下一帧继续绘制白底/旧内容
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Visible(false));

                    // 标记进入"延迟关闭"过渡期；2 帧通常足够贴图视口被 OS 创建并显示
                    self.is_active = false;
                    self.pending_pin_close_frames = 2;
                    ui.ctx().request_repaint();
                }
            }
            // ---- 关闭类动作：标记非活跃 → 执行保存和窗口恢复 ----
            ScreenshotAction::SaveAs
            | ScreenshotAction::Close
            | ScreenshotAction::SaveAndClose
            | ScreenshotAction::SaveToClipboard => {
                self.is_active = false;
                finalize_screenshot_action(ui.ctx(), &mut self.state, common, action);
            }
            // ---- 无动作：跳过 ----
            ScreenshotAction::None => {}
        }

        if !self.is_active {
            *mode = AppMode::Idle;
        }
    }
}
