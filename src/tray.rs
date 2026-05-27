//! 系统托盘模块
//!
//! 行为：
//! - 左键单击托盘图标 → 触发截图
//! - 右键单击托盘图标 → 弹出菜单（设置 / 退出）
//!
//! 实现说明：
//! - 使用 tray-icon 0.19 库（基于 muda 菜单）
//! - 托盘必须在有 Win32 消息循环的线程上创建（eframe/winit 主线程满足）
//! - 事件通过 TrayIconEvent::receiver() / MenuEvent::receiver() 异步推送
//!   每帧在 logic() 中 try_recv 消费

use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

/// 托盘菜单产生的高层事件（解析 muda 原生事件后给主程序使用）
#[derive(Debug, Clone, PartialEq)]
pub enum TrayAction {
    /// 触发截图（左键单击）
    Screenshot,
    /// 显示主窗口（设置菜单项）
    ShowSettings,
    /// 退出程序
    Quit,
}

/// 系统托盘控制器
///
/// 持有 TrayIcon 实例（必须保持存活，否则图标会被销毁），
/// 并保存菜单项 ID 以便在事件分发时识别。
pub struct TrayController {
    /// tray-icon 句柄（必须长期持有）
    _tray: TrayIcon,
    /// "设置" 菜单项 ID
    id_settings: String,
    /// "退出" 菜单项 ID
    id_quit: String,
}

impl TrayController {
    /// 创建并显示系统托盘图标
    ///
    /// 失败时返回错误（如初始化 GtkApplication 失败等，Windows 下基本不会失败）
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 构建右键菜单：设置 / 分隔线 / 退出
        let menu = Menu::new();
        let item_settings = MenuItem::new("设置", true, None);
        let item_quit = MenuItem::new("退出", true, None);
        let id_settings = item_settings.id().0.clone();
        let id_quit = item_quit.id().0.clone();

        menu.append(&item_settings)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&item_quit)?;

        // 加载托盘图标：优先使用嵌入的 PNG，失败时退回纯色图标
        let icon = load_icon();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Settings App")
            .with_icon(icon)
            // 重要：必须关闭"左键点击弹出菜单"，否则左键也会弹菜单
            .with_menu_on_left_click(false)
            .build()?;

        Ok(Self {
            _tray: tray,
            id_settings,
            id_quit,
        })
    }

    /// 每帧调用，消费所有待处理的托盘事件，返回对应的高层动作列表
    ///
    /// 同时处理：
    /// - TrayIconEvent：图标本身的鼠标点击事件（左键 → 截图）
    /// - MenuEvent：右键菜单项点击事件
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();

        // 1. 图标点击事件
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            // tray-icon 0.19 的 TrayIconEvent 是一个枚举，左键 Up 时触发截图
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                use tray_icon::{MouseButton, MouseButtonState};
                // 仅在 Left + Up（完整点击）时触发，避免按下时和释放时都触发
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    actions.push(TrayAction::Screenshot);
                }
            }
        }

        // 2. 菜单项点击事件
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id().0.as_str();
            if id == self.id_settings {
                actions.push(TrayAction::ShowSettings);
            } else if id == self.id_quit {
                actions.push(TrayAction::Quit);
            }
        }

        actions
    }
}

/// 加载托盘图标
///
/// 当前实现：生成一个简单的 16x16 纯色图标作为占位。
/// 后续可替换为加载真实 PNG 资源（如 include_bytes!("../assets/tray.png")）。
fn load_icon() -> Icon {
    // 生成 16x16 RGBA 蓝色图标（每像素 4 字节）
    const SIZE: u32 = 16;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.push(100); // R
        rgba.push(149); // G
        rgba.push(237); // B（CornflowerBlue）
        rgba.push(255); // A
    }
    // from_rgba 在数据长度匹配时不会失败
    Icon::from_rgba(rgba, SIZE, SIZE).expect("构造托盘图标失败")
}
