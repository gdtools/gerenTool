# Tasks

## Task 1: 升级依赖和项目配置
升级 `Cargo.toml` 中的依赖版本，添加截图功能所需的新依赖，创建 `build.rs`。

- [x] SubTask 1.1: 升级 `Cargo.toml`
  - `edition` 从 `"2021"` 改为 `"2024"`
  - `eframe` 从 `0.27` 升级到 `"0.34.1"`
  - `egui` 从 `"0.27"` 升级到 `"0.34.1"`
  - 保留 `egui-phosphor`（升级到兼容 0.34 的版本）
  - 添加新依赖：image, arboard, xcap, tracing, tracing-subscriber, global-hotkey, raw-window-handle, tiny-skia, imageproc, ab_glyph
  - 添加 Windows 平台依赖

- [x] SubTask 1.2: 创建 `build.rs`
  - 从 MVP 的 `build.rs` 简化：只保留 DLL 拷贝逻辑（`dav1d.dll`）
  - 移除图标编译（`winres`），因为主项目不需要

- [x] SubTask 1.3: 将 `截图MVP不要改动/lib/dav1d.dll` 拷贝到项目根目录的 `lib/` 文件夹

## Task 2: 创建截图模块基础结构
在 `src/screenshot/` 下创建模块骨架和核心类型定义。

- [x] SubTask 2.1: 创建 `src/screenshot/mod.rs`
- [x] SubTask 2.2: 创建 `src/screenshot/model/mod.rs`
- [x] SubTask 2.3: 创建 `src/screenshot/model/state.rs`
- [x] SubTask 2.4: 创建 `src/screenshot/model/device.rs`

## Task 3: 创建平台抽象层
在 `src/screenshot/platform/` 下创建跨平台窗口管理和截图平台接口。

- [x] SubTask 3.1: 创建 `src/screenshot/platform/mod.rs`
- [x] SubTask 3.2: 创建 `src/screenshot/platform/windows.rs`

## Task 4: 创建热键系统
在 `src/screenshot/hotkey/` 下创建全局热键管理。

- [x] SubTask 4.1: 创建 `src/screenshot/hotkey/mod.rs`
- [x] SubTask 4.2: 创建 `src/screenshot/hotkey/parser.rs`
- [x] SubTask 4.3: 创建 `src/screenshot/hotkey/manager.rs`（在 mod.rs 中实现）

## Task 5: 迁移截图核心功能
将 MVP 的截图功能代码迁移到 `src/screenshot/feature/` 下，并适配路径和依赖。

- [x] SubTask 5.1: 创建 `src/screenshot/feature/mod.rs`
- [x] SubTask 5.2: 创建 `src/screenshot/feature/screenshot/mod.rs`
- [x] SubTask 5.3: 创建 `src/screenshot/feature/screenshot/state.rs`
- [x] SubTask 5.4: 创建 `src/screenshot/feature/screenshot/capture/mod.rs`
- [x] SubTask 5.5: 创建 `src/screenshot/feature/screenshot/capture/capture_impl.rs`
- [x] SubTask 5.6: 创建 `src/screenshot/feature/screenshot/capture/actions.rs`
- [x] SubTask 5.7: 创建 `src/screenshot/feature/screenshot/draw.rs`
- [x] SubTask 5.8: 创建画布子模块 `src/screenshot/feature/canvas/`
- [x] SubTask 5.9: 创建 `src/screenshot/feature/screenshot/toolbar.rs`

## Task 6: 适配图标系统（工具栏）
将截图工具栏的图标从自定义几何绘制改为使用 `egui_phosphor` 图标字体。

- [x] SubTask 6.1: 设计 phosphor 图标映射
- [x] SubTask 6.2: 重写工具栏按钮渲染

## Task 7: 集成到主应用
修改主应用的 `main.rs` 和 `app.rs`，集成截图功能。

- [x] SubTask 7.1: 修改 `src/main.rs`
- [x] SubTask 7.2: 修改 `src/app.rs`
- [x] SubTask 7.3: 适配 eframe 0.34 API 变更

## Task 8: 编译验证和修复
确保项目能成功编译。

- [x] SubTask 8.1: 首次编译，收集所有编译错误
- [x] SubTask 8.2: 修复路径引用错误（`crate::` 路径调整）
- [x] SubTask 8.3: 修复 API 兼容性错误（eframe 0.27 → 0.34 的差异）
- [x] SubTask 8.4: 修复 phosphor 图标常量名（确认实际可用的图标）
- [x] SubTask 8.5: 最终编译通过验证

# Task Dependencies
- Task 2 依赖 Task 1（依赖配置完成后才能创建模块）
- Task 3 依赖 Task 2（平台层需要 model 类型定义）
- Task 4 依赖 Task 2（热键系统需要 model 类型定义）
- Task 5 依赖 Task 2、3、4（截图核心功能需要 model、platform、hotkey）
- Task 6 依赖 Task 5（图标适配在工具栏代码迁移时进行）
- Task 7 依赖 Task 5、6（主应用集成需要截图模块完成）
- Task 8 依赖 Task 7（所有代码完成后统一编译验证）
