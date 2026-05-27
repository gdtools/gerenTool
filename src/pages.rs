use crate::db;
use crate::startup;
use egui::Ui;

/// =====================================================================
/// 页面渲染 trait
/// 实现此 trait 即可注册为一个设置页面
/// =====================================================================
///
/// `render` 返回 `PageOutput`，用于把页面侧的副作用（例如主题需要切换）
/// 反馈给主应用，由主应用统一处理（避免页面直接持有 egui::Context）
pub trait SettingsPage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput;
}

/// 页面渲染输出（副作用通知）
#[derive(Default, Debug, Clone, Copy)]
pub struct PageOutput {
    /// 主题被切换：true=深色，false=浅色；None 表示无变化
    pub theme_changed: Option<bool>,
}

// ---- 各页面状态 ----

/// 常规设置页：开机启动等
pub struct GeneralPage {
    /// 开机启动开关（与系统注册表 + 数据库同步）
    pub startup_on_login: bool,
    /// 语言下拉索引（暂未落地）
    pub language: usize,
    /// 更新渠道下拉索引（暂未落地）
    pub update_channel: usize,
}

/// 外观设置页：主题/字体等
pub struct AppearancePage {
    /// 深色模式开关（与数据库同步）
    pub dark_mode: bool,
    /// 强调色
    pub accent_color: egui::Color32,
    /// 字体大小
    pub font_size: f32,
    /// 紧凑侧边栏
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
        // 启动时从数据库读取实际值（DB 为 "1" 表示启用），同时与系统注册表对齐
        let db_val = db::get_sys_config(db::K_STARTUP).unwrap_or_else(|| "1".to_string());
        let want = db_val == "1";
        let actual = startup::is_enabled();
        // 若 DB 与系统状态不一致，以 DB 为权威源，把系统状态同步过去
        if want != actual {
            if let Err(e) = startup::set_enabled(want) {
                tracing::warn!("初始同步开机启动失败: {}", e);
            }
        }
        Self {
            startup_on_login: want,
            language: 0,
            update_channel: 0,
        }
    }
}

impl Default for AppearancePage {
    fn default() -> Self {
        // 主题：DB "1"=深色, "0"=浅色，默认深色
        let dark = db::get_sys_config(db::K_THEME).unwrap_or_else(|| "1".to_string()) == "1";
        Self {
            dark_mode: dark,
            accent_color: egui::Color32::from_rgb(100, 149, 237),
            font_size: 14.0,
            sidebar_compact: false,
        }
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
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
        let out = PageOutput::default();
        section(ui, "启动行为", |ui| {
            // 用临时变量监听用户操作，仅在状态真正变化时才写库 + 调系统 API
            let mut value = self.startup_on_login;
            let resp = ui.checkbox(&mut value, "开机启动");
            if resp.changed() {
                // 1. 调用系统注册表
                match startup::set_enabled(value) {
                    Ok(_) => {
                        self.startup_on_login = value;
                        // 2. 写入数据库（"1"/"0"）
                        let v = if value { "1" } else { "0" };
                        if let Err(e) = db::set_sys_config(db::K_STARTUP, v) {
                            tracing::warn!("写入 startup 配置失败: {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("切换开机启动失败: {}", e);
                        // 切换失败：保持原值（UI 回滚）
                    }
                }
            }
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
        out
    }
}

impl SettingsPage for AppearancePage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
        let mut out = PageOutput::default();
        section(ui, "主题", |ui| {
            // 单选组：浅色 / 深色
            ui.horizontal(|ui| {
                ui.label("主题模式：");
                let mut light_selected = !self.dark_mode;
                let mut dark_selected = self.dark_mode;
                // 互斥单选实现：点击哪个，另一个自动取消
                if ui.selectable_label(light_selected, "🌞 浅色").clicked() {
                    light_selected = true;
                    dark_selected = false;
                }
                if ui.selectable_label(dark_selected, "🌙 深色").clicked() {
                    light_selected = false;
                    dark_selected = true;
                }
                let new_dark = dark_selected;
                if new_dark != self.dark_mode {
                    self.dark_mode = new_dark;
                    let v = if new_dark { "1" } else { "0" };
                    if let Err(e) = db::set_sys_config(db::K_THEME, v) {
                        tracing::warn!("写入 theme 配置失败: {:?}", e);
                    }
                    out.theme_changed = Some(new_dark);
                }
                // 抑制未读警告
                let _ = light_selected;
            });
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
        out
    }
}

impl SettingsPage for NotificationsPage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
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
        PageOutput::default()
    }
}

impl SettingsPage for PrivacyPage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
        section(ui, "数据收集", |ui| {
            toggle(ui, "发送匿名统计数据", &mut self.send_analytics);
            toggle(ui, "自动上传崩溃报告", &mut self.crash_report);
        });
        section(ui, "位置", |ui| {
            toggle(ui, "允许访问位置信息", &mut self.location);
        });
        PageOutput::default()
    }
}

impl SettingsPage for NetworkPage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
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
        PageOutput::default()
    }
}

impl SettingsPage for StoragePage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
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
        PageOutput::default()
    }
}

impl SettingsPage for GenericPage {
    fn render(&mut self, ui: &mut Ui) -> PageOutput {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(&self.title);
            ui.add_space(12.0);
            ui.label("该页面暂未实现，欢迎扩展 pages.rs。");
        });
        PageOutput::default()
    }
}

// ---- 通用辅助函数 ----

/// 带标题的分组区块
///
/// 渲染一个 `加粗标题 + 分隔线 + 缩进内容` 的模块；
/// 用于让设置项视觉上分组清晰。
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
