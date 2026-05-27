use global_hotkey::hotkey::Code;

/// 解析后的热键中间表示
/// 将 "Ctrl+Alt+S" 这样的字符串解析为结构化的修饰键 + 按键名
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    pub key_name: String,
}

/// 将配置热键字符串解析为 ParsedHotkey
///
/// 支持的格式：`"Ctrl+Alt+S"`, `"Cmd+Shift+F12"`, `"Super+X"` 等
/// 修饰键支持：Ctrl, Alt, Shift, Cmd/Super
/// 最后一个非修饰键部分作为按键名
///
/// # 返回
/// 解析成功返回 `Some(ParsedHotkey)`，空字符串或无效格式返回 `None`
pub fn parse_hotkey_str(hotkey_str: &str) -> Option<ParsedHotkey> {
    if hotkey_str.is_empty() {
        return None;
    }
    let parts: Vec<&str> = hotkey_str.split('+').collect();

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut cmd = false;
    let mut key_name = None;

    for part in parts {
        match part {
            "Ctrl" => ctrl = true,
            "Alt" => alt = true,
            "Shift" => shift = true,
            "Cmd" | "Super" => cmd = true,
            name => {
                // 如果出现第二个非修饰键部分，视为无效
                if key_name.is_some() {
                    return None;
                }
                key_name = Some(name.to_string());
            }
        }
    }

    key_name.map(|k| ParsedHotkey {
        ctrl,
        alt,
        shift,
        cmd,
        key_name: k,
    })
}

/// 将按键名字符串映射为 `global_hotkey::hotkey::Code`
///
/// 支持字母键（A-Z）、数字键（Num0-Num9）、功能键（F1-F12）
/// 以及常用特殊键（Space, Enter, Escape, Tab, Backspace）
pub fn parsed_key_to_code(key_name: &str) -> Option<Code> {
    match key_name {
        "A" => Some(Code::KeyA),
        "B" => Some(Code::KeyB),
        "C" => Some(Code::KeyC),
        "D" => Some(Code::KeyD),
        "E" => Some(Code::KeyE),
        "F" => Some(Code::KeyF),
        "G" => Some(Code::KeyG),
        "H" => Some(Code::KeyH),
        "I" => Some(Code::KeyI),
        "J" => Some(Code::KeyJ),
        "K" => Some(Code::KeyK),
        "L" => Some(Code::KeyL),
        "M" => Some(Code::KeyM),
        "N" => Some(Code::KeyN),
        "O" => Some(Code::KeyO),
        "P" => Some(Code::KeyP),
        "Q" => Some(Code::KeyQ),
        "R" => Some(Code::KeyR),
        "S" => Some(Code::KeyS),
        "T" => Some(Code::KeyT),
        "U" => Some(Code::KeyU),
        "V" => Some(Code::KeyV),
        "W" => Some(Code::KeyW),
        "X" => Some(Code::KeyX),
        "Y" => Some(Code::KeyY),
        "Z" => Some(Code::KeyZ),
        "Num0" => Some(Code::Digit0),
        "Num1" => Some(Code::Digit1),
        "Num2" => Some(Code::Digit2),
        "Num3" => Some(Code::Digit3),
        "Num4" => Some(Code::Digit4),
        "Num5" => Some(Code::Digit5),
        "Num6" => Some(Code::Digit6),
        "Num7" => Some(Code::Digit7),
        "Num8" => Some(Code::Digit8),
        "Num9" => Some(Code::Digit9),
        "F1" => Some(Code::F1),
        "F2" => Some(Code::F2),
        "F3" => Some(Code::F3),
        "F4" => Some(Code::F4),
        "F5" => Some(Code::F5),
        "F6" => Some(Code::F6),
        "F7" => Some(Code::F7),
        "F8" => Some(Code::F8),
        "F9" => Some(Code::F9),
        "F10" => Some(Code::F10),
        "F11" => Some(Code::F11),
        "F12" => Some(Code::F12),
        "Space" => Some(Code::Space),
        "Enter" => Some(Code::Enter),
        "Escape" => Some(Code::Escape),
        "Tab" => Some(Code::Tab),
        "Backspace" => Some(Code::Backspace),
        _ => {
            tracing::debug!("Unknown key name for Code: {}", key_name);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_combinations() {
        let h = parse_hotkey_str("Ctrl+Alt+S").unwrap();
        assert!(h.ctrl);
        assert!(h.alt);
        assert!(!h.shift);
        assert!(!h.cmd);
        assert_eq!(h.key_name, "S");

        let h = parse_hotkey_str("Cmd+Shift+F12").unwrap();
        assert!(!h.ctrl);
        assert!(!h.alt);
        assert!(h.shift);
        assert!(h.cmd);
        assert_eq!(h.key_name, "F12");
    }

    #[test]
    fn parse_super_as_cmd() {
        let h = parse_hotkey_str("Super+X").unwrap();
        assert!(h.cmd);
        assert_eq!(h.key_name, "X");
    }

    #[test]
    fn parse_single_key() {
        let h = parse_hotkey_str("F5").unwrap();
        assert!(!h.ctrl && !h.alt && !h.shift && !h.cmd);
        assert_eq!(h.key_name, "F5");
    }

    #[test]
    fn reject_empty() {
        assert!(parse_hotkey_str("").is_none());
    }

    #[test]
    fn key_to_code_maps() {
        assert_eq!(parsed_key_to_code("A"), Some(Code::KeyA));
        assert_eq!(parsed_key_to_code("F12"), Some(Code::F12));
        assert_eq!(parsed_key_to_code("Space"), Some(Code::Space));
        assert_eq!(parsed_key_to_code("Num0"), Some(Code::Digit0));
        assert!(parsed_key_to_code("UnknownKey").is_none());
    }
}
