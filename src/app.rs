use crate::db;
use crate::menu::{all_menus, MenuItem};
use crate::pages::{
    AppearancePage, GeneralPage, GenericPage, NetworkPage, NotificationsPage, PrivacyPage,
    SettingsPage, StoragePage,
};
use crate::screenshot::feature::screenshot::state::WindowPrevState;
use crate::screenshot::feature::screenshot::{AppMode, ScreenshotFeature, ToastKind, ToastMessage};
use crate::screenshot::hotkey::HotkeyAction;
use crate::screenshot::model::state::CommonState;
use crate::screenshot::{create_screenshot_state, ScreenshotManager};
use crate::tray::{TrayAction, TrayController};
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use egui_toast::{Toast, ToastOptions, Toasts};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// 设置应用主结构体
pub struct SettingsApp {
    /// 当前选中的菜单项 ID
    current_menu_id: String,
    /// 菜单项列表
    menus: Vec<MenuItem>,
    /// 页面状态集合
    pages: HashMap<String, Box<dyn SettingsPage>>,
    /// 运行模式
    mode: AppMode,
    /// 截图管理器（热键 + 激活状态）
    screenshot_manager: Option<ScreenshotManager>,
    /// 截图功能核心
    screenshot_feature: ScreenshotFeature,
    /// 截图共享状态
    screenshot_common: Option<CommonState>,
    /// 系统托盘（持有期间托盘图标可见）
    tray: Option<TrayController>,
    /// 是否已完成初始化（首帧后设置）
    initialized: bool,
    /// 用户主动选择"退出"标志（区分关闭按钮 / 托盘退出菜单）
    user_quit_requested: bool,
    /// 当前已应用的主题（用于避免每帧重复设置）
    current_dark_mode: bool,
    /// Toast 消息接收通道（来自截图功能模块）
    toast_rx: Receiver<ToastMessage>,
}

impl SettingsApp {
    /// 创建新的应用实例
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        // 根据数据库主题配置应用 visuals（"1"=深色，"0"=浅色，默认深色）
        let dark = db::get_sys_config(db::K_THEME).unwrap_or_else(|| "1".to_string()) == "1";
        apply_theme(&cc.egui_ctx, dark);

        let menus = all_menus();
        let current_menu_id = menus.first().map(|m| m.id.to_string()).unwrap_or_default();

        let mut pages: HashMap<String, Box<dyn SettingsPage>> = HashMap::new();
        pages.insert("general".into(), Box::new(GeneralPage::default()));
        pages.insert("appearance".into(), Box::new(AppearancePage::default()));
        pages.insert(
            "notifications".into(),
            Box::new(NotificationsPage::default()),
        );
        pages.insert("privacy".into(), Box::new(PrivacyPage::default()));
        pages.insert("network".into(), Box::new(NetworkPage::default()));
        pages.insert("storage".into(), Box::new(StoragePage::default()));
        for menu in &menus {
            pages.entry(menu.id.to_string()).or_insert_with(|| {
                Box::new(GenericPage {
                    title: menu.label.to_string(),
                })
            });
        }

        let mut hwnd_usize = 0;
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                hwnd_usize = h.hwnd.get() as usize;
            }
        }

        let (_window_state, common_state) = create_screenshot_state(hwnd_usize);

        // 创建托盘（失败时仅记录错误，程序继续运行）
        let tray = match TrayController::new(&cc.egui_ctx) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::error!("创建系统托盘失败: {:?}", e);
                None
            }
        };

        let (screenshot_feature, toast_rx) = ScreenshotFeature::new();

        Self {
            current_menu_id,
            menus,
            pages,
            mode: AppMode::Idle,
            screenshot_manager: None,
            screenshot_feature,
            screenshot_common: Some(common_state),
            tray,
            initialized: false,
            user_quit_requested: false,
            current_dark_mode: dark,
            toast_rx,
        }
    }

    /// 延迟初始化截图模块（首帧时获取窗口句柄）
    fn ensure_initialized(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        if let Some(common_state) = &self.screenshot_common {
            let screenshot_manager =
                ScreenshotManager::new(ctx, Arc::clone(&common_state.window_state));
            self.screenshot_manager = Some(screenshot_manager);
        }
    }

    /// 处理托盘事件
    ///
    /// - Screenshot：等价于按下截图热键（伪造 HotkeyAction）
    /// - ShowSettings：显示主窗口并置顶
    /// - Quit：标记用户主动退出，发起 Close 流程
    fn handle_tray_actions(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };
        let actions = tray.poll();
        for action in actions {
            match action {
                TrayAction::Screenshot => {
                    // 模拟热键路径：先确保窗口处于已知状态再进入截图模式
                    // 直接使用 WindowPrevState::Tray 让截图模块识别"原本最小化/隐藏"
                    if self.mode != AppMode::Screenshot {
                        if let Some(new_mode) =
                            self.screenshot_feature
                                .handle_hotkey(HotkeyAction::SetScreenshotMode {
                                    prev_state: WindowPrevState::Tray,
                                })
                        {
                            self.mode = new_mode;
                            if let Some(m) = &mut self.screenshot_manager {
                                m.set_active(true);
                            }
                        }
                    }
                }
                TrayAction::ShowSettings => {
                    let window_size = egui::vec2(800.0, 600.0);
                    let min_size = egui::vec2(400.0, 300.0);
                    let pos = ctx.input(|i| {
                        i.viewport()
                            .monitor_size
                            .map(|size| {
                                egui::pos2(
                                    (size.x - window_size.x) / 2.0,
                                    (size.y - window_size.y) / 2.0,
                                )
                            })
                            .unwrap_or_else(|| egui::pos2(100.0, 100.0))
                    });

                    if let Some(common) = &self.screenshot_common {
                        if let Ok(mut visible) = common.window_state.visible.lock() {
                            *visible = true;
                        }
                    }

                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::Normal,
                    ));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(window_size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
                TrayAction::Quit => {
                    // 标记允许真正退出
                    self.user_quit_requested = true;
                    if let Some(common) = &self.screenshot_common {
                        if let Ok(mut allow) = common.window_state.allow_quit.lock() {
                            *allow = true;
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

/// 配置中文字体和图标字体（使用系统字体 + phosphor 图标字体）
///
/// 内存优化要点：
/// 1. 不再使用 `Box::leak` 把整份 msyh.ttc 永久驻留在堆上(~20MB)。
///    改用 `FontData::from_owned(Vec<u8>)`：所有权转移给 egui 内部，
///    egui 自己用 `Arc<Vec<u8>>` 管理，最终在 ctx 释放时被回收。
/// 2. 优先使用 `msyh.ttc`(微软雅黑标准字重)。
///    若失败则尝试 `msyhl.ttc`(Light) 或 `simsun.ttc`(宋体)。
///    选用最小的可用 CJK 字体可显著降低运行时驻留内存。
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // 候选字体列表，按"体积优先 + 视觉妥协"顺序排列
    // - msyhl.ttc: 微软雅黑 Light，体积更小（约 14MB）
    // - msyh.ttc:  微软雅黑 Regular（约 20MB）
    // - simsun.ttc: 宋体备用
    const FONT_CANDIDATES: &[&str] = &[
        "C:\\Windows\\Fonts\\msyhl.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];

    for path in FONT_CANDIDATES {
        if let Ok(font_data) = std::fs::read(path) {
            // 关键：from_owned 让 egui 拿走所有权(Arc 管理)，
            // 而不是 Box::leak 永久泄漏 'static 引用
            fonts
                .font_data
                .insert("cjk".to_owned(), Arc::new(FontData::from_owned(font_data)));

            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());

            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("cjk".to_owned());
            break;
        }
    }

    // 注册 phosphor 图标字体
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // 工具栏使用的命名字体系列
    fonts.families.insert(
        FontFamily::Name("phosphor-regular".into()),
        vec!["phosphor".to_owned()],
    );

    ctx.set_fonts(fonts);
}

/// 根据 dark 标志应用 egui 视觉主题
///
/// - dark=true  → 使用内置 dark 主题
/// - dark=false → 使用内置 light 主题
fn apply_theme(ctx: &egui::Context, dark: bool) {
    if dark {
        ctx.set_visuals(egui::Visuals::dark());
    } else {
        ctx.set_visuals(egui::Visuals::light());
    }
}

impl eframe::App for SettingsApp {
    /// 每帧逻辑更新（在 UI 渲染之前调用）
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_initialized(ctx);

        // 处理托盘事件（必须每帧 poll，事件来自后台线程）
        self.handle_tray_actions(ctx);

        // 处理窗口关闭事件：除非用户主动选择退出，否则一律最小化到托盘
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested {
            if self.mode == AppMode::Screenshot {
                // 截图模式下拦截关闭
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if !self.user_quit_requested {
                // 非主动退出 → 取消关闭，隐藏窗口到托盘
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                // 同步内部 visible 状态，让热键模块知道窗口已隐藏（用于从托盘恢复）
                if let Some(common) = &self.screenshot_common {
                    if let Ok(mut v) = common.window_state.visible.lock() {
                        *v = false;
                    }
                }

                // 内存优化：窗口隐藏到托盘时，触发 egui CacheStorage 的 LRU 清扫，
                // 释放本帧未被访问的字形/Galley/形状缓存。
                // egui 0.34 的 update() 会遍历所有 cache 并调用其 update()，
                // 内部实现是"丢弃上一帧未命中的条目"。
                ctx.memory_mut(|mem| {
                    mem.caches.update();
                });
            } else if let Some(common) = &self.screenshot_common {
                // 用户主动退出：放行
                if let Ok(mut allow) = common.window_state.allow_quit.lock() {
                    *allow = true;
                }
            }
        }

        // 处理全局热键事件
        if let Some(ref mut manager) = self.screenshot_manager {
            let hotkey_actions = manager.update();
            for action in hotkey_actions {
                if let Some(new_mode) = self.screenshot_feature.handle_hotkey(action) {
                    self.mode = new_mode;
                    manager.set_active(true);
                }
            }
        }

        // 截图模式下的逻辑驱动
        if let Some(ref mut common) = self.screenshot_common {
            self.screenshot_feature.logic(ctx, common, &mut self.mode);
        }

        // 同步截图激活状态
        if let Some(ref mut manager) = self.screenshot_manager {
            manager.set_active(self.screenshot_feature.is_active());
        }
    }

    /// 每帧 UI 渲染
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 置顶贴图必须在每帧无条件渲染
        self.screenshot_feature.show_pinned_viewports(ui.ctx());

        // 全局 Toast：从通道 drain 所有待显示消息，统一在右下角展示
        let mut toasts = Toasts::new()
            .anchor(egui::Align2::RIGHT_BOTTOM, (-16.0, -16.0))
            .direction(egui::Direction::BottomUp)
            .order(egui::Order::Tooltip);

        while let Ok(msg) = self.toast_rx.try_recv() {
            let kind = match msg.kind {
                ToastKind::Success => egui_toast::ToastKind::Success,
                ToastKind::Error => egui_toast::ToastKind::Error,
                ToastKind::Info => egui_toast::ToastKind::Info,
            };
            toasts.add(Toast {
                text: msg.text.into(),
                kind,
                options: ToastOptions::default()
                    .duration_in_seconds(3.0)
                    .show_progress(true)
                    .show_icon(true),
                ..Default::default()
            });
        }

        toasts.show(ui);

        if self.mode == AppMode::Screenshot {
            if let Some(ref mut common) = self.screenshot_common {
                self.screenshot_feature.ui(ui, common, &mut self.mode);
            }
            return;
        }

        // 设置模式：渲染侧边栏菜单
        let selected_id = self.current_menu_id.clone();
        let mut new_selected_id = selected_id.clone();

        egui::Panel::left("menu_panel")
            .default_size(200.0)
            .show_inside(ui, |ui| {
                ui.add_space(16.0);
                ui.heading("设置");
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                for menu in &self.menus {
                    let is_selected = menu.id == selected_id;
                    let response = ui.add(egui::Button::selectable(
                        is_selected,
                        format!("{} {}", menu.icon, menu.label),
                    ));
                    if response.clicked() {
                        new_selected_id = menu.id.to_string();
                    }
                }
            });

        self.current_menu_id = new_selected_id;

        // 渲染中央面板
        let mut pending_theme: Option<bool> = None;
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(page) = self.pages.get_mut(&self.current_menu_id) {
                let out = page.render(ui);
                if let Some(dark) = out.theme_changed {
                    pending_theme = Some(dark);
                }
            }
        });

        // 应用主题变更（在所有 UI 渲染完成后再设置 visuals，避免本帧 UI 不一致）
        if let Some(dark) = pending_theme {
            if dark != self.current_dark_mode {
                self.current_dark_mode = dark;
                apply_theme(ui.ctx(), dark);
                ui.ctx().request_repaint();
            }
        }
    }
}
