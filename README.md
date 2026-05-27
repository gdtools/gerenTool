# settings_app — 功能设置框架

基于 **Rust + egui 0.27 + egui-phosphor** 的桌面设置面板框架。

## 快速启动

```bash
cd settings_app
cargo run
```

窗口默认 800×600，可拖动边角自由调整大小。

---

## 项目结构

```
src/
├── main.rs      # 入口，字体注册
├── app.rs       # 主 App：左右布局、菜单渲染
├── menu.rs      # 菜单项定义（★ 添加菜单在这里）
└── pages.rs     # 各页面内容（★ 添加控件在这里）
```

---

## 如何添加新菜单

打开 `src/menu.rs`，在 `all_menus()` 中追加一行：

```rust
MenuItem::new(ph::GEAR_SIX, "高级设置", "advanced"),
```

三个参数：`(Phosphor图标常量, 显示文字, 唯一id)`

> 所有图标常量见 [egui-phosphor 文档](https://docs.rs/egui-phosphor)，
> 例如 `ph::HOUSE`、`ph::BELL`、`ph::GEAR_SIX` 等。

---

## 如何添加新页面内容

1. **在 `pages.rs` 定义页面状态结构体**

```rust
pub struct AdvancedPage {
    pub debug_mode: bool,
    pub log_level: usize,
}

impl Default for AdvancedPage {
    fn default() -> Self {
        Self { debug_mode: false, log_level: 1 }
    }
}
```

2. **实现 `SettingsPage` trait**

```rust
impl SettingsPage for AdvancedPage {
    fn render(&mut self, ui: &mut Ui) {
        section(ui, "调试", |ui| {
            toggle(ui, "启用调试模式", &mut self.debug_mode);
            egui::ComboBox::from_label("日志级别")
                .selected_text(["Error", "Warn", "Info", "Debug"][self.log_level])
                .show_ui(ui, |ui| {
                    for (i, lv) in ["Error", "Warn", "Info", "Debug"].iter().enumerate() {
                        ui.selectable_value(&mut self.log_level, i, *lv);
                    }
                });
        });
    }
}
```

3. **在 `app.rs` 中注册页面**

```rust
// SettingsApp 结构体中添加字段
advanced: AdvancedPage,

// update() 的 match 中添加分支
"advanced" => self.advanced.render(ui),
```

---

## 辅助函数

| 函数 | 说明 |
|------|------|
| `section(ui, "标题", \|ui\| { ... })` | 带标题分组区块 |
| `toggle(ui, "标签", &mut bool)` | 开关复选框 |

其余控件直接使用 egui 原生组件：
`Slider`、`ComboBox`、`TextEdit`、`Button`、`color_edit_button_srgba` 等。

---

## 依赖版本

```toml
eframe       = "0.27"
egui         = "0.27"
egui-phosphor = { version = "0.5", features = ["regular"] }
```
