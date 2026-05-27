# 提取截图功能到 settings_app 框架

## Why
`截图MVP不要改动` 文件夹中包含一个完整的截图工具（CloverViewer），需要将其截图功能提取出来，集成到 `settings_app` 框架中。截图功能通过全局热键 `Alt+S` 触发，不添加到菜单中。所有截图相关代码集中在 `src/screenshot/` 文件夹内。

## What Changes
- **升级 eframe/egui 版本**：从 0.27 升级到 0.34，以兼容截图代码使用的 API（`StrokeKind`、`UiBuilder`、`let chains` 等）
- **升级 Rust edition**：从 2021 升级到 2024，以支持 `let chains` 语法（截图代码大量使用）
- **添加新依赖**：`image`、`arboard`、`xcap`、`global-hotkey`、`raw-window-handle`、`tiny-skia`、`imageproc`、`ab_glyph`、`tracing`、`tracing-subscriber`、`windows`（Windows 平台 API）
- **创建 `src/screenshot/` 模块**：包含截图功能的全部代码，自包含子模块结构
- **创建平台抽象层**：`screenshot/platform/` 处理窗口管理和截图平台差异
- **创建热键系统**：`screenshot/hotkey/` 处理全局热键注册和事件分发
- **适配图标系统**：工具栏图标从 MVP 的自定义几何绘制改为使用主框架的 `egui_phosphor` 图标字体
- **适配字体系统**：截图文本渲染使用主框架的系统字体（msyh.ttc），不再内嵌字体文件
- **修改主应用**：添加 `AppMode` 状态机，在截图模式下将窗口控制权交给截图模块
- **添加 `build.rs`**：处理 `dav1d.dll` 的拷贝（xcap 依赖）

## Impact
- Affected code:
  - `Cargo.toml` - 依赖升级和新增
  - `src/main.rs` - 添加 screenshot 模块声明、初始化热键
  - `src/app.rs` - 添加 AppMode 状态机、截图模式下的 logic/ui 委托
  - `build.rs` - 新建，处理 DLL 拷贝
  - `src/screenshot/` - 新建，截图功能全部代码

## ADDED Requirements

### Requirement: 截图功能模块（screenshot）
系统 SHALL 提供一个自包含的截图功能模块，代码全部位于 `src/screenshot/` 目录下，通过全局热键 `Alt+S` 触发。

#### Scenario: 热键触发截图
- **WHEN** 用户在任何时候按下 `Alt+S`
- **THEN** 应用隐藏设置窗口，捕获所有屏幕，显示全屏透明截图覆盖层

#### Scenario: 区域选择
- **WHEN** 截图覆盖层显示后，用户拖拽鼠标
- **THEN** 显示绿色选区框，支持窗口自动检测和全屏选择

#### Scenario: 图像标注
- **WHEN** 用户选择区域后，使用工具栏中的绘图工具（矩形、圆形、箭头、画笔、马赛克、文本）
- **THEN** 在选区内绘制对应的标注图形，支持撤销/重做（Ctrl+Z/Ctrl+Y）

#### Scenario: 保存截图
- **WHEN** 用户点击保存到剪贴板按钮或按 Enter
- **THEN** 将选区内容（含标注）复制到系统剪贴板
- **WHEN** 用户点击保存按钮
- **THEN** 将截图保存为 PNG 文件到桌面

#### Scenario: 退出截图
- **WHEN** 用户按 Escape 或点击取消按钮
- **THEN** 退出截图模式，恢复设置窗口

### Requirement: 平台抽象层
系统 SHALL 提供平台抽象层（`screenshot/platform/`），封装窗口管理和截图相关的平台差异。

#### Scenario: Windows 平台支持
- **WHEN** 在 Windows 平台运行
- **THEN** 使用 Win32 API 实现窗口隐藏/恢复、光标锁定、任务栏矩形检测、Alt 菜单抑制

### Requirement: 全局热键系统
系统 SHALL 提供全局热键管理器（`screenshot/hotkey/`），在应用不处于前台时也能捕获热键。

#### Scenario: 热键注册
- **WHEN** 应用启动
- **THEN** 注册 `Alt+S` 为全局热键，通过 mpsc 通道将事件传递到主线程

### Requirement: 图标适配
截图工具栏 SHALL 使用主框架的 `egui_phosphor` 图标字体替代 MVP 中的自定义几何图标绘制。

#### Scenario: 工具栏图标显示
- **WHEN** 截图工具栏渲染
- **THEN** 使用 phosphor 图标字体显示各工具按钮（矩形、圆形、箭头、画笔、马赛克、文本、取消、保存、剪贴板）

### Requirement: 字体适配
截图功能的文本渲染（导出图片时）SHALL 使用系统字体路径加载微软雅黑，而非内嵌字体文件。

#### Scenario: 文本导出
- **WHEN** 截图包含文本标注并导出
- **THEN** 从 `C:\Windows\Fonts\msyh.ttc` 加载字体进行渲染

## MODIFIED Requirements

### Requirement: 主应用状态管理
在原有 `SettingsApp` 基础上添加 `AppMode` 枚举（`Settings` / `Screenshot`），截图模式下将 `logic()` 和 `ui()` 委托给截图模块处理。

### Requirement: 主应用初始化
`SettingsApp::new()` 需要额外初始化截图模块和热键管理器，获取窗口句柄（hwnd）传递给截图模块。

## REMOVED Requirements

### Requirement: MVP 内嵌字体资源
**Reason**: 主框架使用系统字体加载方式，不需要内嵌字体文件
**Migration**: 截图导出时的字体加载改为从系统路径读取 msyh.ttc

### Requirement: MVP 自定义几何图标
**Reason**: 主框架使用 egui_phosphor 图标字体系统
**Migration**: 工具栏按钮改用 phosphor 图标字体渲染，移除 `icons.rs` 中的几何绘制代码

### Requirement: MVP 内存调试模块
**Reason**: `memory_debug.rs` 是 MVP 专用的调试工具，不属于截图功能核心
**Migration**: 不迁移，截图代码中的 `report_memory` 调用直接移除

### Requirement: MVP 日志模块
**Reason**: 主框架可自行决定是否使用 tracing，截图模块直接使用 `tracing::` 宏即可
**Migration**: 保留 tracing 依赖，但不迁移 `logging.rs` 的初始化逻辑（由主应用决定）
