use egui::Rect;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// 窗口管理接口
/// 封装窗口句柄获取、显示/隐藏、焦点抢占等平台相关操作
pub trait WindowManager {
    /// 从 eframe 创建上下文获取原生窗口句柄（usize 形式）
    fn get_window_handle(&self, cc: &eframe::CreationContext<'_>) -> usize;

    /// 恢复窗口到屏幕外位置（用于截图前的窗口准备）
    fn show_window_restore_offscreen(&self, hwnd_usize: usize);

    /// 隐藏窗口
    fn show_window_hide(&self, hwnd_usize: usize);

    /// 强制获取窗口焦点（处理跨线程焦点切换）
    fn force_get_focus(&self, hwnd_usize: usize);
}

/// 截图平台接口
/// 封装截图模式下的光标锁定和任务栏检测
pub trait ScreenshotPlatform {
    /// 锁定光标在虚拟屏幕范围内（防止截图时光标移出）
    fn lock_cursor_for_screenshot(&self);

    /// 解除光标锁定
    fn unlock_cursor(&self);

    /// 获取所有任务栏的矩形区域（用于排除窗口检测中的任务栏）
    fn get_taskbar_rects(&self) -> Vec<Rect>;
}

/// 平台统一接口（组合所有平台能力）
pub trait Platform: WindowManager + ScreenshotPlatform {}

/// 自动为同时实现 WindowManager 和 ScreenshotPlatform 的类型实现 Platform
impl<T> Platform for T where T: WindowManager + ScreenshotPlatform {}

/// 获取当前平台的实现
/// 通过条件编译返回对应平台的静态实例
pub fn current_platform() -> &'static dyn Platform {
    #[cfg(target_os = "windows")]
    {
        static PLATFORM: windows::WindowsPlatform = windows::WindowsPlatform;
        &PLATFORM
    }
    #[cfg(target_os = "macos")]
    {
        static PLATFORM: macos::MacosPlatform = macos::MacosPlatform;
        &PLATFORM
    }
    #[cfg(target_os = "linux")]
    {
        static PLATFORM: linux::LinuxPlatform = linux::LinuxPlatform;
        &PLATFORM
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        panic!("Unsupported platform");
    }
}
