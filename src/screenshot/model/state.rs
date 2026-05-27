use super::device::DeviceInfo;
use std::sync::{Arc, Mutex};

/// 截图触发前的窗口状态
/// 用于截图结束后恢复窗口到正确的状态
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum WindowPrevState {
    /// 窗口正常显示
    Normal,
    /// 窗口处于最小化状态
    Minimized,
    /// 窗口隐藏在系统托盘
    Tray,
}

/// 窗口状态管理
/// 通过 Arc<Mutex> 在热键回调线程和主线程之间安全共享窗口可见性和关闭许可
pub struct WindowState {
    /// 窗口是否可视（热键线程读取/设置，主线程读取/设置）
    pub visible: Arc<Mutex<bool>>,
    /// 是否允许关闭窗口（截图模式下拦截关闭请求）
    pub allow_quit: Arc<Mutex<bool>>,
    /// 原生窗口句柄（usize 形式，跨平台传递）
    pub hwnd_usize: usize,
}

impl WindowState {
    /// 创建新的窗口状态实例
    ///
    /// # 参数
    /// - `visible`: 窗口可见性的共享引用
    /// - `allow_quit`: 关闭许可的共享引用
    /// - `hwnd_usize`: 原生窗口句柄
    pub fn new(
        visible: Arc<Mutex<bool>>,
        allow_quit: Arc<Mutex<bool>>,
        hwnd_usize: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            visible,
            allow_quit,
            hwnd_usize,
        })
    }
}

/// 各功能模块共享的通用状态
/// 包含窗口管理和设备信息，不包含热键管理器（热键管理由 ScreenshotManager 负责）
pub struct CommonState {
    /// 窗口状态（Arc 包装，支持跨线程共享）
    pub window_state: Arc<WindowState>,
    /// 设备信息（显示器布局等）
    pub device_info: DeviceInfo,
}

impl CommonState {
    /// 创建通用状态实例
    ///
    /// # 参数
    /// - `window_state`: 已初始化的窗口状态
    pub fn new(window_state: Arc<WindowState>) -> Self {
        Self {
            window_state,
            device_info: DeviceInfo::load(),
        }
    }
}
