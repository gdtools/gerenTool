# Checklist

## 项目配置
- [x] `Cargo.toml` 中 edition 已升级为 `"2024"`
- [x] `eframe` 和 `egui` 已升级到 `"0.34.1"`
- [x] 所有截图相关依赖已添加（image, arboard, xcap, global-hotkey, raw-window-handle, tiny-skia, imageproc, ab_glyph, tracing, tracing-subscriber, windows）
- [x] `egui-phosphor` 版本兼容 eframe 0.34（0.12）
- [x] `build.rs` 已创建并处理 DLL 拷贝（如需要）
- [x] `lib/dav1d.dll` 已拷贝到项目目录

## 截图模块结构
- [x] `src/screenshot/mod.rs` 存在且声明了所有子模块
- [x] `ScreenshotManager` 结构体已实现，提供 `new`、`update`、`logic`、`ui` 方法
- [x] `src/screenshot/model/` 包含 `device.rs` 和 `state.rs`
- [x] `src/screenshot/platform/` 包含平台抽象 trait 和 Windows 实现
- [x] `src/screenshot/hotkey/` 包含热键解析器和管理器
- [x] `src/screenshot/feature/` 包含完整的截图功能代码

## 截图功能完整性
- [ ] 屏幕捕获功能正常（xcap 多屏幕截图）
- [ ] 窗口矩形检测功能正常（xcap::Window + 任务栏检测）
- [ ] 选区拖拽功能正常（创建、调整、移动选区）
- [ ] 窗口悬浮高亮功能正常
- [ ] 全屏选择功能正常（点击空白区域选择整个屏幕）
- [ ] 绘图工具功能正常：矩形、圆形、箭头、画笔、马赛克、文本
- [ ] 撤销/重做功能正常（Ctrl+Z / Ctrl+Y）
- [ ] 图形选中、拖拽、resize 功能正常
- [ ] 保存到剪贴板功能正常（Enter 键或工具栏按钮）
- [ ] 保存到桌面功能正常（保存按钮）
- [ ] 退出截图功能正常（Escape 键或取消按钮）

## 图标和字体适配
- [x] 工具栏按钮使用 `egui_phosphor` 图标字体渲染
- [ ] 所有工具栏图标正确显示（矩形、圆形、箭头、画笔、马赛克、文本、取消、保存、剪贴板）
- [x] 截图导出时的文本渲染使用系统字体（msyh.ttc）而非内嵌字体
- [x] 工具栏按钮保留 hover 背景、选中边框、tooltip 交互

## 热键系统
- [x] `Alt+S` 全局热键在应用启动时注册
- [ ] 热键在应用不处于前台时仍能触发
- [x] 截图模式下重复按 `Alt+S` 不会触发新的截图

## 主应用集成
- [x] `AppMode` 枚举已添加到 `app.rs`
- [x] 设置模式下正常显示菜单和页面
- [x] 截图模式下设置 UI 完全隐藏
- [ ] 截图完成后设置窗口正确恢复
- [x] 窗口关闭事件在截图模式下被正确拦截
- [x] 截图功能不添加到菜单中

## 代码质量
- [x] 所有截图代码位于 `src/screenshot/` 目录下
- [x] 所有 `crate::` 路径引用正确（无残留的 MVP 路径）
- [x] 无 `memory_debug` 调试代码残留
- [x] 项目能通过 `cargo build` 编译成功
- [x] 项目能通过 `cargo test` 测试（14 个测试全部通过）
