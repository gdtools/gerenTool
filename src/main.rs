// Windows 子系统：避免后台运行时弹出黑色控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod db;
mod menu;
mod pages;
mod screenshot;
mod startup;
mod tray;

use app::SettingsApp;
use eframe::NativeOptions;

fn main() -> eframe::Result<()> {
    // 初始化日志（debug 默认 INFO，release 默认 WARN）
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    // 初始化数据库（建表 + 插入默认配置）
    if let Err(e) = db::init() {
        tracing::error!("数据库初始化失败: {:?}", e);
    }

    let options = NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Settings App",
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
    )
}
