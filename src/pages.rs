use egui::Ui;

/// =====================================================================
/// 页面渲染 trait
/// 实现此 trait 即可注册为一个设置页面
/// =====================================================================
pub trait SettingsPage {
    fn render(&mut self, ui: &mut Ui);
}

// ---- 各页面状态 ----

pub struct GeneralPage {
    pub startup_on_login: bool,
    pub language: usize,
    pub update_channel: usize,
}

pub struct AppearancePage {
    pub dark_mode: bool,
    pub accent_color: egui::Color32,
    pub font_size: f32,
    pub sidebar_compact: bool,
}

pub struct NotificationsPage {
    pub enable_all: bool,
    pub sound: bool,
    pub badge: bool,
    pub popup_duration: f32,
}

pub struct PrivacyPage {
    pub send_analytics: bool,
    pub crash_report: bool,
    pub location: bool,
}

pub struct NetworkPage {
    pub proxy_enabled: bool,
    pub proxy_host: String,
    pub proxy_port: String,
    pub timeout: f32,
}

pub struct StoragePage {
    pub cache_limit: f32,
    pub auto_clean: bool,
}

pub struct GenericPage {
    pub title: String,
}

// ---- Default 实现 ----

impl Default for GeneralPage {
    fn default() -> Self {
        Self { startup_on_login: true, language: 0, update_channel: 0 }
    }
}
impl Default for AppearancePage {
    fn default() -> Self {
        Self { dark_mode: true, accent_color: egui::Color32::from_rgb(100, 149, 237), font_size: 14.0, sidebar_compact: false }
    }
}
impl Default for NotificationsPage {
    fn default() -> Self {
        Self { enable_all: true, sound: true, badge: true, popup_duration: 4.0 }
    }
}
impl Default for PrivacyPage {
    fn default() -> Self {
        Self { send_analytics: false, crash_report: true, location: false }
    }
}
impl Default for NetworkPage {
    fn default() -> Self {
        Self { proxy_enabled: false, proxy_host: "127.0.0.1".into(), proxy_port: "7890".into(), timeout: 30.0 }
    }
}
impl Default for StoragePage {
    fn default() -> Self {
        Self { cache_limit: 512.0, auto_clean: true }
    }
}

// ---- render 实现 ----

impl SettingsPage for GeneralPage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "启动行为", |ui| {
            toggle(ui, "随系统启动", &mut self.startup_on_login);
        });
        section(ui, "语言", |ui| {
            egui::ComboBox::from_label("界面语言")
                .selected_text(["简体中文", "English", "日本語"][self.language])
                .show_ui(ui, |ui| {
                    for (i, lang) in ["简体中文", "English", "日本語"].iter().enumerate() {
                        ui.selectable_value(&mut self.language, i, *lang);
                    }
                });
        });
        section(ui, "更新", |ui| {
            egui::ComboBox::from_label("更新渠道")
                .selected_text(["稳定版", "测试版", "每日构建"][self.update_channel])
                .show_ui(ui, |ui| {
                    for (i, ch) in ["稳定版", "测试版", "每日构建"].iter().enumerate() {
                        ui.selectable_value(&mut self.update_channel, i, *ch);
                    }
                });
            if ui.button("检查更新").clicked() {}
        });
    }
}

impl SettingsPage for AppearancePage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "主题", |ui| {
            toggle(ui, "深色模式", &mut self.dark_mode);
            toggle(ui, "紧凑侧边栏", &mut self.sidebar_compact);
        });
        section(ui, "颜色", |ui| {
            ui.horizontal(|ui| {
                ui.label("强调色");
                ui.color_edit_button_srgba(&mut self.accent_color);
            });
        });
        section(ui, "字体", |ui| {
            ui.add(
                egui::Slider::new(&mut self.font_size, 10.0..=24.0)
                    .text("字体大小")
                    .suffix(" px"),
            );
        });
    }
}

impl SettingsPage for NotificationsPage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "总开关", |ui| {
            toggle(ui, "启用通知", &mut self.enable_all);
        });
        ui.add_enabled_ui(self.enable_all, |ui| {
            section(ui, "通知方式", |ui| {
                toggle(ui, "声音提示", &mut self.sound);
                toggle(ui, "角标计数", &mut self.badge);
            });
            section(ui, "弹窗", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.popup_duration, 1.0..=10.0)
                        .text("弹窗持续时间")
                        .suffix(" 秒"),
                );
            });
        });
    }
}

impl SettingsPage for PrivacyPage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "数据收集", |ui| {
            toggle(ui, "发送匿名统计数据", &mut self.send_analytics);
            toggle(ui, "自动上传崩溃报告", &mut self.crash_report);
        });
        section(ui, "位置", |ui| {
            toggle(ui, "允许访问位置信息", &mut self.location);
        });
    }
}

impl SettingsPage for NetworkPage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "代理", |ui| {
            toggle(ui, "启用代理", &mut self.proxy_enabled);
            ui.add_enabled_ui(self.proxy_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("主机");
                    ui.text_edit_singleline(&mut self.proxy_host);
                });
                ui.horizontal(|ui| {
                    ui.label("端口");
                    ui.text_edit_singleline(&mut self.proxy_port);
                });
            });
        });
        section(ui, "连接", |ui| {
            ui.add(
                egui::Slider::new(&mut self.timeout, 5.0..=120.0)
                    .text("超时时间")
                    .suffix(" 秒"),
            );
        });
    }
}

impl SettingsPage for StoragePage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "缓存", |ui| {
            ui.add(
                egui::Slider::new(&mut self.cache_limit, 128.0..=4096.0)
                    .text("缓存上限")
                    .suffix(" MB"),
            );
            toggle(ui, "自动清理过期缓存", &mut self.auto_clean);
            if ui.button("立即清理缓存").clicked() {}
        });
        section(ui, "磁盘占用", |ui| {
            ui.label("应用数据：  42 MB");
            ui.label("日志文件：  8 MB");
            ui.label("缓存文件：  156 MB");
        });
    }
}

impl SettingsPage for GenericPage {
    fn render(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(&self.title);
            ui.add_space(12.0);
            ui.label("该页面暂未实现，欢迎扩展 pages.rs。");
        });
    }
}

// ---- 通用辅助函数 ----

/// 带标题的分组区块
pub fn section(ui: &mut Ui, title: &str, content: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong().size(13.0));
    ui.separator();
    ui.add_space(4.0);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 4))
        .show(ui, content);
    ui.add_space(4.0);
}

/// 行内 toggle（checkbox 风格）
pub fn toggle(ui: &mut Ui, label: &str, value: &mut bool) {
    ui.horizontal(|ui| {
        ui.checkbox(value, label);
    });
}
