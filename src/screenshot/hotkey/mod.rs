pub mod parser;

use crate::screenshot::model::state::{WindowPrevState, WindowState};
use crate::screenshot::platform::current_platform;
use eframe::egui::Context;
use egui::ViewportCommand;
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use std::sync::{Arc, mpsc};

/// 截图全局热键的文本表示（硬编码 Alt+S）
pub const SCREENSHOT_HOTKEY: &str = "Alt+S";

/// 热键触发的动作枚举
#[derive(Clone)]
pub enum HotkeyAction {
    /// 触发截图模式，附带窗口恢复所需的状态信息
    SetScreenshotMode { prev_state: WindowPrevState },
}

/// 全局热键管理器
///
/// 负责注册系统级热键（即使窗口不在前台也能捕获），
/// 并将热键事件通过 mpsc 通道传递给主线程处理。
///
/// # 线程安全说明
/// global-hotkey 的事件回调运行在独立线程上，
/// 因此内部使用 mpsc 通道将事件安全传递到 egui 主线程。
pub struct HotkeyManager {
    /// 全局热键管理器实例
    hotkeys_manager: Option<GlobalHotKeyManager>,
    /// 热键事件接收端（主线程消费）
    hotkey_receiver: mpsc::Receiver<(u32, WindowPrevState)>,
    /// 截图热键定义（用于 ID 比对）
    show_hotkey: HotKey,
}

impl HotkeyManager {
    /// 创建全局热键管理器
    ///
    /// 热键硬编码为 Alt+S，用于触发截图模式。
    /// 内部会注册 global-hotkey 事件处理器，当用户按下热键时：
    ///   1. 判断窗口当前状态（最小化/托盘/正常）
    ///   2. 如果窗口不可见则先恢复窗口
    ///   3. 通过 mpsc 通道通知主线程进入截图模式
    pub fn new(ctx: &Context, window_state: Arc<WindowState>) -> Self {
        // 解析热键字符串，失败时使用默认 Alt+S
        let show_hotkey = parser::parse_hotkey_str(SCREENSHOT_HOTKEY)
            .and_then(|p| parsed_to_hotkey(&p))
            .unwrap_or(HotKey::new(Some(Modifiers::ALT), Code::KeyS));

        let (tx, rx) = mpsc::channel();

        let hotkeys_manager = match GlobalHotKeyManager::new() {
            Ok(hotkeys_manager) => {
                // 注册全局热键
                if let Err(e) = hotkeys_manager.register(show_hotkey) {
                    tracing::error!("Failed to register screenshot hotkey: {:?}", e);
                }

                let ctx_clone = ctx.clone();
                GlobalHotKeyEvent::set_event_handler(Some(Box::new(
                    move |event: GlobalHotKeyEvent| {
                        // [重要] global-hotkey 回调运行在独立线程上，
                        // 绝不能在持有 visible Mutex 的同时调用 ctx.input() 或 Win32 API
                        // (如 ShowWindow/SetWindowPos)，否则会与主线程（持有 Context 写锁并
                        // 尝试获取 visible Mutex）产生跨线程死锁。

                        // 1. 先在无锁状态下读取 egui 层面的最小化状态
                        let is_minimized =
                            ctx_clone.input(|i| i.viewport().minimized.unwrap_or(false));

                        // 2. 最小范围持有 visible 锁：读取 + 设置，然后立刻释放
                        let prev_state = {
                            let Ok(mut visible) = window_state.visible.lock() else {
                                return;
                            };
                            let is_visible = *visible;

                            // 根据窗口当前可见性和最小化状态判断之前的状态
                            let state = if !is_visible {
                                WindowPrevState::Tray
                            } else if is_minimized {
                                WindowPrevState::Minimized
                            } else {
                                WindowPrevState::Normal
                            };

                            // 提前标记为可见，释放锁后主线程就能正确读取
                            if state != WindowPrevState::Normal {
                                *visible = true;
                            }

                            state
                            // visible 锁在此处释放
                        };

                        // 3. 锁已释放，安全调用 Win32 API 和 egui viewport commands
                        if prev_state != WindowPrevState::Normal {
                            if prev_state == WindowPrevState::Tray {
                                // 从托盘恢复：先移到屏幕外，再获取焦点
                                let platform = current_platform();
                                platform.show_window_restore_offscreen(window_state.hwnd_usize);
                                platform.force_get_focus(window_state.hwnd_usize);
                                ctx_clone.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            } else {
                                // 从最小化恢复
                                ctx_clone.send_viewport_cmd(ViewportCommand::Minimized(false));
                                ctx_clone.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            }
                        }

                        let _ = tx.send((event.id, prev_state));
                        ctx_clone.request_repaint();
                    },
                )));

                Some(hotkeys_manager)
            }
            Err(err) => {
                tracing::error!("Failed to initialize GlobalHotKeyManager: {:?}", err);
                None
            }
        };

        Self {
            hotkeys_manager,
            hotkey_receiver: rx,
            show_hotkey,
        }
    }

    /// 每帧调用，检查并消费待处理的全局热键事件
    ///
    /// 返回本帧触发的热键动作列表，由调用方执行相应逻辑。
    /// 使用 `is_active` 参数防止在截图模式激活时重复触发。
    pub fn update(&mut self, is_active: bool) -> Vec<HotkeyAction> {
        let mut actions = Vec::new();

        let Some(_hotkeys_manager) = self.hotkeys_manager.as_ref() else {
            return actions;
        };

        // 用于防止一帧内处理多次重复按键
        let mut screenshot_triggered_this_frame = false;

        // 处理接收到的热键事件
        while let Ok((id, prev_state)) = self.hotkey_receiver.try_recv() {
            // 通过 ID 对比来判断是哪个键被按下了
            if id == self.show_hotkey.id() {
                tracing::debug!("截图热键触发");
                // 只有在不是截图模式，且本帧未触发的情况下，才接受事件
                if !is_active && !screenshot_triggered_this_frame {
                    actions.push(HotkeyAction::SetScreenshotMode { prev_state });
                    screenshot_triggered_this_frame = true;
                }
            }
        }

        actions
    }
}

/// 将 ParsedHotkey 转换为 global_hotkey::HotKey
///
/// 将解析后的修饰键和按键名映射为 global-hotkey 库所需的 HotKey 类型
fn parsed_to_hotkey(parsed: &parser::ParsedHotkey) -> Option<HotKey> {
    use global_hotkey::hotkey::Modifiers as GModifiers;
    let code = parser::parsed_key_to_code(&parsed.key_name)?;
    let mut modifiers = GModifiers::empty();
    if parsed.ctrl {
        modifiers.insert(GModifiers::CONTROL);
    }
    if parsed.alt {
        modifiers.insert(GModifiers::ALT);
    }
    if parsed.shift {
        modifiers.insert(GModifiers::SHIFT);
    }
    if parsed.cmd {
        modifiers.insert(GModifiers::SUPER);
    }
    Some(HotKey::new(Some(modifiers), code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_hotkey_is_alt_s() {
        let p = parser::parse_hotkey_str(SCREENSHOT_HOTKEY).unwrap();
        assert!(p.alt);
        assert!(!p.ctrl);
        assert!(!p.shift);
        assert!(!p.cmd);
        assert_eq!(p.key_name, "S");
        assert!(parsed_to_hotkey(&p).is_some());
    }

    #[test]
    fn parsed_to_hotkey_accepts_modifier_combinations() {
        let p = parser::parse_hotkey_str("Ctrl+Alt+S").unwrap();
        assert!(parsed_to_hotkey(&p).is_some());

        let p = parser::parse_hotkey_str("Cmd+Shift+F12").unwrap();
        assert!(parsed_to_hotkey(&p).is_some());
    }

    #[test]
    fn parsed_to_hotkey_rejects_unknown_key_names() {
        let p = parser::parse_hotkey_str("Ctrl+NoSuchKey").unwrap();
        assert!(parsed_to_hotkey(&p).is_none());

        assert!(parser::parse_hotkey_str("").is_none());
    }
}
