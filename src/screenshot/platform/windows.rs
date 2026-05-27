use egui::{Rect, pos2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, ClipCursor, FindWindowA, FindWindowExA, GetCursorPos, GetForegroundWindow,
    GetSystemMetrics, GetWindowRect, GetWindowThreadProcessId, HWND_TOP, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOSIZE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, SW_HIDE, SW_RESTORE,
    WM_SYSCOMMAND,
};
use windows::core::{PCSTR, s};

use super::{ScreenshotPlatform, WindowManager};

/// ComCtl32 子类化 API 的函数签名类型
/// windows 0.62 未直接暴露这些函数，通过 FFI 调用
type SubclassProc = unsafe extern "system" fn(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT;

#[link(name = "ComCtl32")]
unsafe extern "system" {
    fn SetWindowSubclass(
        hwnd: HWND,
        pfnsubclass: Option<SubclassProc>,
        uidsubclass: usize,
        dwrefdata: usize,
    ) -> i32;

    fn RemoveWindowSubclass(
        hwnd: HWND,
        pfnsubclass: Option<SubclassProc>,
        uidsubclass: usize,
    ) -> i32;

    fn DefSubclassProc(hwnd: HWND, umsg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
}

/// 截图模式下用于阻止 Alt 键激活 Windows 系统菜单的子类化 ID
const SCREENSHOT_SUBCLASS_ID: usize = 1;

/// Windows 系统菜单命令 ID（Alt 键触发）
const SC_KEYMENU: usize = 0xF100;

/// 截图窗口子类化回调
/// 拦截 WM_SYSCOMMAND + SC_KEYMENU 消息，阻止 Alt 键激活系统菜单
unsafe extern "system" fn screenshot_alt_suppress_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    if umsg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_KEYMENU {
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, umsg, wparam, lparam) }
}

/// 为截图窗口安装子类化，阻止 Alt 键激活 Windows 系统菜单
///
/// 在截图模式下调用，防止用户按 Alt 键时弹出系统菜单破坏截图覆盖层
pub fn suppress_alt_menu_activation(hwnd_usize: usize) {
    let hwnd = HWND(hwnd_usize as *mut std::ffi::c_void);
    unsafe {
        let _ = RemoveWindowSubclass(
            hwnd,
            Some(screenshot_alt_suppress_proc as SubclassProc),
            SCREENSHOT_SUBCLASS_ID,
        );
        let _ = SetWindowSubclass(
            hwnd,
            Some(screenshot_alt_suppress_proc as SubclassProc),
            SCREENSHOT_SUBCLASS_ID,
            0,
        );
    }
}

/// 移除截图窗口的子类化，恢复 Alt 键的默认系统行为
///
/// 在截图结束后调用，恢复窗口的正常系统菜单行为
pub fn remove_alt_menu_suppression(hwnd_usize: usize) {
    let hwnd = HWND(hwnd_usize as *mut std::ffi::c_void);
    unsafe {
        let _ = RemoveWindowSubclass(
            hwnd,
            Some(screenshot_alt_suppress_proc as SubclassProc),
            SCREENSHOT_SUBCLASS_ID,
        );
    }
}

/// Windows 平台实现
/// 提供窗口管理和截图平台相关的 Windows 特定功能
pub struct WindowsPlatform;

impl WindowsPlatform {
    /// 将 usize 形式的句柄转换为 Windows HWND 类型
    /// 句柄为 0 时返回 None 并记录警告
    fn hwnd_from_usize(&self, hwnd_usize: usize) -> Option<HWND> {
        if hwnd_usize == 0 {
            tracing::warn!("Window handle is unavailable; skipping Windows API call");
            return None;
        }

        HWND(hwnd_usize as *mut std::ffi::c_void).into()
    }
}

impl WindowManager for WindowsPlatform {
    /// 从 eframe 创建上下文获取 Win32 窗口句柄
    fn get_window_handle(&self, cc: &eframe::CreationContext<'_>) -> usize {
        let Ok(window_handle) = cc.window_handle() else {
            tracing::error!("Failed to get window handle");
            return 0;
        };
        let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
            tracing::error!("Unsupported platform raw window handle");
            return 0;
        };

        handle.hwnd.get() as usize
    }

    /// 恢复窗口到屏幕外位置（-20000, -20000）
    /// 用于截图准备阶段，先恢复窗口但不让用户看到
    fn show_window_restore_offscreen(&self, hwnd_usize: usize) {
        let Some(window_handle) = self.hwnd_from_usize(hwnd_usize) else {
            return;
        };
        unsafe {
            let _ = SetWindowPos(
                window_handle,
                HWND_TOP.into(),
                -20000,
                -20000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            let _ = ShowWindow(window_handle, SW_RESTORE);
        }
    }

    /// 隐藏窗口
    fn show_window_hide(&self, hwnd_usize: usize) {
        let Some(window_handle) = self.hwnd_from_usize(hwnd_usize) else {
            return;
        };
        unsafe {
            let _ = ShowWindow(window_handle, SW_HIDE);
        }
    }

    /// 强制获取窗口焦点
    /// 处理 Windows 跨线程焦点切换限制：通过 AttachThreadInput 临时关联线程
    fn force_get_focus(&self, hwnd_usize: usize) {
        let Some(window_handle) = self.hwnd_from_usize(hwnd_usize) else {
            return;
        };
        unsafe {
            let fg_hwnd = GetForegroundWindow();

            if fg_hwnd == window_handle {
                return;
            }

            let fg_thread = GetWindowThreadProcessId(fg_hwnd, None);
            let current_thread = GetCurrentThreadId();

            // 跨线程时需要临时关联输入线程，否则 SetForegroundWindow 会被系统拒绝
            if fg_thread != current_thread && fg_thread != 0 {
                let _ = AttachThreadInput(current_thread, fg_thread, true);
                if let Err(e) = BringWindowToTop(window_handle) {
                    tracing::error!("BringWindowToTop failed: {:?}", e);
                }
                let _ = SetForegroundWindow(window_handle);
                let _ = AttachThreadInput(current_thread, fg_thread, false);
            } else {
                if let Err(e) = BringWindowToTop(window_handle) {
                    tracing::error!("BringWindowToTop failed: {:?}", e);
                }
                let _ = SetForegroundWindow(window_handle);
            }
        }
    }
}

impl ScreenshotPlatform for WindowsPlatform {
    /// 锁定光标在虚拟屏幕范围内
    /// 限制光标 Y 轴底部为当前显示器底部减 2 像素，防止误触任务栏
    fn lock_cursor_for_screenshot(&self) {
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let mut pt = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut pt);

            // 获取光标所在显示器信息，用于精确计算底部限制
            let hmonitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);

            let mut monitor_info: MONITORINFO = std::mem::zeroed();
            monitor_info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            let _ = GetMonitorInfoW(hmonitor, &mut monitor_info);

            let bottom_limit = if monitor_info.rcMonitor.bottom > 0 {
                monitor_info.rcMonitor.bottom - 2
            } else {
                vy + vh - 5
            };

            let rect = RECT {
                left: vx,
                top: vy,
                right: vx + vw,
                bottom: bottom_limit,
            };

            if let Err(err) = ClipCursor(Some(&rect as *const RECT)) {
                tracing::warn!("Failed to lock cursor for screenshot: {:?}", err);
            }
        }
    }

    /// 解除光标锁定，恢复正常鼠标移动范围
    fn unlock_cursor(&self) {
        unsafe {
            if let Err(err) = ClipCursor(None) {
                tracing::warn!("Failed to unlock cursor: {:?}", err);
            }
        }
    }

    /// 获取所有任务栏的矩形区域
    /// 查找主任务栏（Shell_TrayWnd）和所有副任务栏（Shell_SecondaryTrayWnd）
    fn get_taskbar_rects(&self) -> Vec<Rect> {
        let mut rects = Vec::new();
        unsafe {
            // 闭包：从窗口句柄获取矩形并添加到结果列表
            let mut push_rect_from_hwnd = |hwnd: HWND| {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    rects.push(Rect::from_min_max(
                        pos2(rect.left as f32, rect.top as f32),
                        pos2(rect.right as f32, rect.bottom as f32),
                    ));
                }
            };

            // 查找主任务栏
            if let Ok(hwnd_main) = FindWindowA(s!("Shell_TrayWnd"), PCSTR::null())
                && !hwnd_main.0.is_null()
            {
                push_rect_from_hwnd(hwnd_main);
            }

            // 遍历查找所有副任务栏（多显示器场景）
            let mut current_hwnd = HWND::default();
            loop {
                match FindWindowExA(
                    HWND::default().into(),
                    current_hwnd.into(),
                    s!("Shell_SecondaryTrayWnd"),
                    PCSTR::null(),
                ) {
                    Ok(hwnd) if !hwnd.0.is_null() => {
                        push_rect_from_hwnd(hwnd);
                        current_hwnd = hwnd;
                    }
                    _ => break,
                }
            }
        }
        rects
    }
}
