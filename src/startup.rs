//! 开机启动管理
//!
//! Windows 下基于 auto-launch（写入 HKCU\Software\Microsoft\Windows\CurrentVersion\Run）实现，
//! 不需要管理员权限，仅影响当前用户。

use auto_launch::AutoLaunch;

/// 注册名（在注册表 Run 中的 ValueName）
const APP_NAME: &str = "SettingsApp";

/// 构造 AutoLaunch 实例（指向当前可执行文件）
fn instance() -> Option<AutoLaunch> {
    let exe = std::env::current_exe().ok()?;
    let exe_str = exe.to_str()?;
    Some(AutoLaunch::new(APP_NAME, exe_str, &[] as &[&str]))
}

/// 查询当前是否已设置开机启动
pub fn is_enabled() -> bool {
    instance()
        .and_then(|a| a.is_enabled().ok())
        .unwrap_or(false)
}

/// 设置开机启动开关
///
/// - `enable = true`：将自身可执行路径写入注册表 Run 项
/// - `enable = false`：从注册表 Run 项删除
pub fn set_enabled(enable: bool) -> Result<(), String> {
    let auto = instance().ok_or_else(|| "获取可执行路径失败".to_string())?;
    if enable {
        auto.enable().map_err(|e| format!("启用开机启动失败: {:?}", e))
    } else {
        auto.disable().map_err(|e| format!("禁用开机启动失败: {:?}", e))
    }
}
