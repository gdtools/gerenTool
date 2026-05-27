use crate::menu::{MenuItem, all_menus};
use crate::pages::{
    AppearancePage, GeneralPage, GenericPage, NetworkPage, NotificationsPage, PrivacyPage,
    SettingsPage, StoragePage,
};
use crate::screenshot::feature::screenshot::AppMode;
use crate::screenshot::feature::screenshot::ScreenshotFeature;
use crate::screenshot::model::state::CommonState;
use crate::screenshot::{ScreenshotManager, create_screenshot_state};
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::HashMap;
use std::sync::Arc;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

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
    /// 是否已完成初始化（首帧后设置）
    initialized: bool,
}

impl SettingsApp {
    /// 创建新的应用实例
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        let menus = all_menus();
        let current_menu_id = menus.first().map(|m| m.id.to_string()).unwrap_or_default();

        let mut pages: HashMap<String, Box<dyn SettingsPage>> = HashMap::new();
        pages.insert("general".into(), Box::new(GeneralPage::default()));
        pages.insert("appearance".into(), Box::new(AppearancePage::default()));
        pages.insert("notifications".into(), Box::new(NotificationsPage::default()));
        pages.insert("privacy".into(), Box::new(PrivacyPage::default()));
        pages.insert("network".into(), Box::new(NetworkPage::default()));
        pages.insert("storage".into(), Box::new(StoragePage::default()));
        for menu in &menus {
            pages
                .entry(menu.id.to_string())
                .or_insert_with(|| Box::new(GenericPage { title: menu.label.to_string() }));
        }

        let mut hwnd_usize = 0;
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                hwnd_usize = h.hwnd.get() as usize;
            }
        }

        let (window_state, common_state) = create_screenshot_state(hwnd_usize);

        Self {
            current_menu_id,
            menus,
            pages,
            mode: AppMode::Idle,
            screenshot_manager: None,
            screenshot_feature: ScreenshotFeature::new(),
            screenshot_common: Some(common_state),
            initialized: false,
        }
    }

    /// 延迟初始化截图模块（首帧时获取窗口句柄）
    fn ensure_initialized(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        if let Some(common_state) = &self.screenshot_common {
            let screenshot_manager = ScreenshotManager::new(ctx, Arc::clone(&common_state.window_state));
            self.screenshot_manager = Some(screenshot_manager);
        }
    }
}

/// 配置中文字体和图标字体（使用系统字体 + phosphor 图标字体）
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // 尝试从系统路径加载微软雅黑
    let font_path = "C:\\Windows\\Fonts\\msyh.ttc";
    if let Ok(font_data) = std::fs::read(font_path) {
        let font_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());
        fonts.font_data.insert(
            "msyh".to_owned(),
            Arc::new(FontData::from_static(font_data)),
        );

        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "msyh".to_owned());

        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("msyh".to_owned());
    }

    // 注册 phosphor 图标字体（将字体数据插入并添加到 Proportional 回退链）
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // 为工具栏显式创建 phosphor-regular 命名字体系列
    // toolbar.rs 中通过 FontFamily::Name("phosphor-regular") 引用此字体
    fonts.families.insert(
        FontFamily::Name("phosphor-regular".into()),
        vec!["phosphor".to_owned()],
    );

    ctx.set_fonts(fonts);
}

impl eframe::App for SettingsApp {
    /// 每帧逻辑更新（在 UI 渲染之前调用）
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_initialized(ctx);

        // 处理窗口关闭事件
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested {
            if self.mode == AppMode::Screenshot {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if let Some(ref common) = self.screenshot_common {
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
        // 置顶贴图必须在每帧无条件渲染（无论当前模式），
        // 否则截图关闭后 mode=Idle，贴图窗口将不再被维护而变成孤儿窗口
        self.screenshot_feature.show_pinned_viewports(ui.ctx());

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
                    let response = ui.add(
                        egui::Button::selectable(is_selected, format!("{} {}", menu.icon, menu.label)),
                    );
                    if response.clicked() {
                        new_selected_id = menu.id.to_string();
                    }
                }
            });

        self.current_menu_id = new_selected_id;

        // 渲染中央面板
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(page) = self.pages.get_mut(&self.current_menu_id) {
                page.render(ui);
            }
        });
    }
}
