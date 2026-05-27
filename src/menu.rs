/// 菜单项定义
/// 要添加新菜单，只需在 `all_menus()` 中追加 `MenuItem` 即可
#[derive(Clone, PartialEq)]
pub struct MenuItem {
    /// Phosphor 图标 unicode（使用 egui_phosphor::regular::* 常量）
    pub icon: &'static str,
    /// 显示文字
    pub label: &'static str,
    /// 唯一标识，用于路由到对应页面
    pub id: &'static str,
}

impl MenuItem {
    pub const fn new(icon: &'static str, label: &'static str, id: &'static str) -> Self {
        Self { icon, label, id }
    }
}

/// =====================================================================
/// 在这里添加/删除菜单项，顺序即为显示顺序
/// =====================================================================
pub fn all_menus() -> Vec<MenuItem> {
    use egui_phosphor::regular as ph;
    vec![
        MenuItem::new(ph::HOUSE,           "常规",       "general"),
        MenuItem::new(ph::PALETTE,         "外观",       "appearance"),
        MenuItem::new(ph::BELL,            "通知",       "notifications"),
        MenuItem::new(ph::SHIELD_CHECK,    "隐私与安全", "privacy"),
        MenuItem::new(ph::WIFI_HIGH,       "网络",       "network"),
        MenuItem::new(ph::HARD_DRIVE,      "存储",       "storage"),
        MenuItem::new(ph::PRINTER,         "打印机",     "printer"),
        MenuItem::new(ph::KEYBOARD,        "键盘与输入", "keyboard"),
        MenuItem::new(ph::MOUSE,           "鼠标",       "mouse"),
        MenuItem::new(ph::SPEAKER_HIGH,    "声音",       "sound"),
        MenuItem::new(ph::MONITOR,         "显示器",     "display"),
        MenuItem::new(ph::TRANSLATE,       "语言与地区", "language"),
        MenuItem::new(ph::CLOCK,           "日期与时间", "datetime"),
        MenuItem::new(ph::USER_CIRCLE,     "账户",       "account"),
        MenuItem::new(ph::QUESTION,        "关于",       "about"),
    ]
}
