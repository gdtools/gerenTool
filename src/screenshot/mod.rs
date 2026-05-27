pub mod feature;
pub mod hotkey;
pub mod model;
pub mod platform;

use eframe::egui::Context;
use hotkey::{HotkeyAction, HotkeyManager};
use model::state::{CommonState, WindowState};
use std::sync::{Arc, Mutex};

/// 截图管理器 - 对外暴露的统一入口
///
/// 封装截图功能的所有基础设施：
/// - 全局热键注册和事件处理
/// - 窗口状态管理
/// - 截图激活状态跟踪
///
/// # 使用方式
/// 在应用初始化时创建实例，每帧调用 `update()` 处理热键事件。
/// 当热键触发时，通过返回的 `HotkeyAction` 驱动截图流程。
pub struct ScreenshotManager {
    /// 全局热键管理器
    hotkey_manager: HotkeyManager,
    /// 截图功能是否处于激活状态
    is_active: bool,
}

impl ScreenshotManager {
    /// 创建截图管理器实例
    ///
    /// # 参数
    /// - `ctx`: egui 上下文（用于热键回调中请求重绘）
    /// - `window_state`: 窗口状态（热键回调需要读取/设置窗口可见性）
    pub fn new(ctx: &Context, window_state: Arc<WindowState>) -> Self {
        Self {
            hotkey_manager: HotkeyManager::new(ctx, window_state),
            is_active: false,
        }
    }

    /// 每帧调用，处理热键事件
    ///
    /// 检查是否有待处理的全局热键事件，并返回对应的动作列表。
    /// 调用方应根据返回的动作执行相应逻辑（如进入截图模式）。
    ///
    /// # 返回
    /// 本帧触发的热键动作列表
    pub fn update(&mut self) -> Vec<HotkeyAction> {
        self.hotkey_manager.update(self.is_active)
    }

    /// 查询当前是否处于截图激活状态
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// 设置截图激活状态
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }
}

/// 创建截图模块所需的共享状态
///
/// 便捷函数，一次性创建 WindowState 和 CommonState。
/// 返回 `(window_state, common_state)` 元组。
///
/// # 参数
/// - `hwnd_usize`: 原生窗口句柄
pub fn create_screenshot_state(hwnd_usize: usize) -> (Arc<WindowState>, CommonState) {
    let visible = Arc::new(Mutex::new(true));
    let allow_quit = Arc::new(Mutex::new(false));
    let window_state = WindowState::new(visible, allow_quit, hwnd_usize);
    let common_state = CommonState::new(Arc::clone(&window_state));
    (window_state, common_state)
}
