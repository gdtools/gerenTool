//! 系统托盘模块

use eframe::egui;
use std::sync::mpsc::{Receiver, channel};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

#[derive(Debug, Clone, PartialEq)]
pub enum TrayAction {
    Screenshot,
    ShowSettings,
    Quit,
}

pub struct TrayController {
    _tray: TrayIcon,
    rx: Receiver<TrayAction>,
}

impl TrayController {
    pub fn new(ctx: &egui::Context) -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let item_settings = MenuItem::new("设置", true, None);
        let item_quit = MenuItem::new("退出", true, None);
        let id_settings = item_settings.id().0.clone();
        let id_quit = item_quit.id().0.clone();

        menu.append(&item_settings)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&item_quit)?;

        let (tx, rx) = channel::<TrayAction>();

        let tray_tx = tx.clone();
        let tray_ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    let _ = tray_tx.send(TrayAction::Screenshot);
                    tray_ctx.request_repaint();
                }
            }
        }));

        let menu_tx = tx.clone();
        let menu_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let id = event.id().0.as_str();
            if id == id_settings {
                let _ = menu_tx.send(TrayAction::ShowSettings);
                menu_ctx.request_repaint();
            } else if id == id_quit {
                let _ = menu_tx.send(TrayAction::Quit);
                menu_ctx.request_repaint();
            }
        }));

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Settings App")
            .with_icon(load_icon())
            .with_menu_on_left_click(false)
            .build()?;

        Ok(Self { _tray: tray, rx })
    }

    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(action) = self.rx.try_recv() {
            actions.push(action);
        }
        actions
    }
}

fn load_icon() -> Icon {
    const SIZE: u32 = 16;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.push(100);
        rgba.push(149);
        rgba.push(237);
        rgba.push(255);
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("构造托盘图标失败")
}
