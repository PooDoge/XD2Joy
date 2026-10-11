//! USB HID input codes as the firmware sees them: keyboard usages (page 0x07), the modifier byte,
//! and the mouse button byte.

/// Usages the firmware treats as modifiers. They never appear in the key array, so they can only
/// be bound through the modifier byte.
pub const MODIFIER_USAGES: std::ops::RangeInclusive<u8> = 0xE0..=0xE7;

/// One bit of the keyboard report's modifier byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Modifier {
    LeftCtrl,
    LeftShift,
    LeftAlt,
    LeftGui,
    RightCtrl,
    RightShift,
    RightAlt,
    RightGui,
}

impl Modifier {
    pub const ALL: [Modifier; 8] = [
        Modifier::LeftCtrl,
        Modifier::LeftShift,
        Modifier::LeftAlt,
        Modifier::LeftGui,
        Modifier::RightCtrl,
        Modifier::RightShift,
        Modifier::RightAlt,
        Modifier::RightGui,
    ];

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn from_bit(bit: u8) -> Option<Modifier> {
        Modifier::ALL.into_iter().find(|m| m.bit() == bit)
    }

    /// The modifier a usage 0xE0..=0xE7 stands for.
    pub fn from_usage(usage: u8) -> Option<Modifier> {
        MODIFIER_USAGES.contains(&usage).then(|| Modifier::ALL[usize::from(usage - 0xE0)])
    }

    pub fn usage(self) -> u8 {
        0xE0 + self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Modifier::LeftCtrl => "Left Ctrl",
            Modifier::LeftShift => "Left Shift",
            Modifier::LeftAlt => "Left Alt",
            Modifier::LeftGui => "Left Win",
            Modifier::RightCtrl => "Right Ctrl",
            Modifier::RightShift => "Right Shift",
            Modifier::RightAlt => "Right Alt",
            Modifier::RightGui => "Right Win",
        }
    }

    /// Short form for hotkeys, where left and right usually do not matter to the reader.
    pub fn short(self) -> &'static str {
        match self {
            Modifier::LeftCtrl => "Ctrl",
            Modifier::LeftShift => "Shift",
            Modifier::LeftAlt => "Alt",
            Modifier::LeftGui => "Win",
            Modifier::RightCtrl => "Right Ctrl",
            Modifier::RightShift => "Right Shift",
            Modifier::RightAlt => "Right Alt",
            Modifier::RightGui => "Right Win",
        }
    }
}

/// The modifiers in a mask, in bit order.
pub fn modifiers_in(mask: u8) -> impl Iterator<Item = Modifier> {
    Modifier::ALL.into_iter().filter(move |m| mask & m.bit() != 0)
}

/// "Left Ctrl or Left Shift": a modifier entry fires when any of its bits is held.
pub fn modifier_mask_label(mask: u8) -> String {
    let names: Vec<_> = modifiers_in(mask).map(Modifier::label).collect();
    if names.is_empty() { "no modifier".to_owned() } else { names.join(" or ") }
}

/// A keyboard key name for display, for any usage; unnamed ones show their number.
pub fn key_label(usage: u8) -> String {
    match key_name(usage) {
        Some(name) => name.to_owned(),
        None => format!("Key 0x{usage:02X}"),
    }
}

/// The keys a standard full-size keyboard has, by usage.
pub fn key_name(usage: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V",
        "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    const F1_12: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
    const F13_24: [&str; 12] = ["F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24"];
    const NUM_1_9: [&str; 9] = ["Num 1", "Num 2", "Num 3", "Num 4", "Num 5", "Num 6", "Num 7", "Num 8", "Num 9"];
    let i = usize::from(usage);
    Some(match usage {
        0x04..=0x1D => LETTERS[i - 0x04],
        0x1E..=0x27 => DIGITS[i - 0x1E],
        0x28 => "Enter",
        0x29 => "Esc",
        0x2A => "Backspace",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "-",
        0x2E => "=",
        0x2F => "[",
        0x30 => "]",
        0x31 => "\\",
        0x32 => "# (ISO)",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",
        0x39 => "Caps Lock",
        0x3A..=0x45 => F1_12[i - 0x3A],
        0x46 => "Print Screen",
        0x47 => "Scroll Lock",
        0x48 => "Pause",
        0x49 => "Insert",
        0x4A => "Home",
        0x4B => "Page Up",
        0x4C => "Delete",
        0x4D => "End",
        0x4E => "Page Down",
        0x4F => "Right",
        0x50 => "Left",
        0x51 => "Down",
        0x52 => "Up",
        0x53 => "Num Lock",
        0x54 => "Num /",
        0x55 => "Num *",
        0x56 => "Num -",
        0x57 => "Num +",
        0x58 => "Num Enter",
        0x59..=0x61 => NUM_1_9[i - 0x59],
        0x62 => "Num 0",
        0x63 => "Num .",
        0x64 => "\\ (ISO)",
        0x65 => "Menu",
        0x67 => "Num =",
        0x68..=0x73 => F13_24[i - 0x68],
        0xE0..=0xE7 => Modifier::ALL[i - 0xE0].label(),
        _ => return None,
    })
}

/// The HID usage for a W3C `KeyboardEvent.code` name ("KeyA", "ArrowUp", "F13"), which is how
/// desktop UI toolkits name physical keys. Used to capture hotkeys from the PC keyboard.
pub fn usage_from_code_name(code: &str) -> Option<u8> {
    if let Some(c) = code.strip_prefix("Key") {
        let b = single_char(c)?;
        return b.is_ascii_uppercase().then(|| 0x04 + (b - b'A'));
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return match single_char(d)? {
            b'0' => Some(0x27),
            b @ b'1'..=b'9' => Some(0x1E + (b - b'1')),
            _ => None,
        };
    }
    if let Some(n) = code.strip_prefix("Numpad")
        && let Some(b) = single_char(n)
    {
        return match b {
            b'0' => Some(0x62),
            b'1'..=b'9' => Some(0x59 + (b - b'1')),
            _ => None,
        };
    }
    if let Some(n) = code.strip_prefix('F')
        && let Ok(n) = n.parse::<u8>()
    {
        return match n {
            1..=12 => Some(0x3A + n - 1),
            13..=24 => Some(0x68 + n - 13),
            _ => None,
        };
    }
    Some(match code {
        "Enter" => 0x28,
        "Escape" => 0x29,
        "Backspace" => 0x2A,
        "Tab" => 0x2B,
        "Space" => 0x2C,
        "Minus" => 0x2D,
        "Equal" => 0x2E,
        "BracketLeft" => 0x2F,
        "BracketRight" => 0x30,
        "Backslash" => 0x31,
        "IntlHash" => 0x32,
        "Semicolon" => 0x33,
        "Quote" => 0x34,
        "Backquote" => 0x35,
        "Comma" => 0x36,
        "Period" => 0x37,
        "Slash" => 0x38,
        "CapsLock" => 0x39,
        "PrintScreen" => 0x46,
        "ScrollLock" => 0x47,
        "Pause" => 0x48,
        "Insert" => 0x49,
        "Home" => 0x4A,
        "PageUp" => 0x4B,
        "Delete" => 0x4C,
        "End" => 0x4D,
        "PageDown" => 0x4E,
        "ArrowRight" => 0x4F,
        "ArrowLeft" => 0x50,
        "ArrowDown" => 0x51,
        "ArrowUp" => 0x52,
        "NumLock" => 0x53,
        "NumpadDivide" => 0x54,
        "NumpadMultiply" => 0x55,
        "NumpadSubtract" => 0x56,
        "NumpadAdd" => 0x57,
        "NumpadEnter" => 0x58,
        "NumpadDecimal" => 0x63,
        "IntlBackslash" => 0x64,
        "ContextMenu" => 0x65,
        "NumpadEqual" => 0x67,
        "ControlLeft" => 0xE0,
        "ShiftLeft" => 0xE1,
        "AltLeft" => 0xE2,
        "MetaLeft" | "OSLeft" | "SuperLeft" => 0xE3,
        "ControlRight" => 0xE4,
        "ShiftRight" => 0xE5,
        "AltRight" => 0xE6,
        "MetaRight" | "OSRight" | "SuperRight" => 0xE7,
        _ => return None,
    })
}

fn single_char(s: &str) -> Option<u8> {
    match s.as_bytes() {
        [b] => Some(*b),
        _ => None,
    }
}

/// One bit of the mouse report's button byte. The firmware reads Button-page usages 1..8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Button6,
    Button7,
    Button8,
}

impl MouseButton {
    pub const ALL: [MouseButton; 8] = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
        MouseButton::Button6,
        MouseButton::Button7,
        MouseButton::Button8,
    ];

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn from_bit(bit: u8) -> Option<MouseButton> {
        MouseButton::ALL.into_iter().find(|b| b.bit() == bit)
    }

    pub fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left click",
            MouseButton::Right => "Right click",
            MouseButton::Middle => "Middle click",
            MouseButton::Back => "Back (button 4)",
            MouseButton::Forward => "Forward (button 5)",
            MouseButton::Button6 => "Button 6",
            MouseButton::Button7 => "Button 7",
            MouseButton::Button8 => "Button 8",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            MouseButton::Left => "LMB",
            MouseButton::Right => "RMB",
            MouseButton::Middle => "MMB",
            MouseButton::Back => "M4",
            MouseButton::Forward => "M5",
            MouseButton::Button6 => "M6",
            MouseButton::Button7 => "M7",
            MouseButton::Button8 => "M8",
        }
    }
}

pub fn mouse_buttons_in(mask: u8) -> impl Iterator<Item = MouseButton> {
    MouseButton::ALL.into_iter().filter(move |b| mask & b.bit() != 0)
}

/// The wheel turned one way; the firmware pulses the target for 90 ms per notch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WheelDir {
    Up,
    Down,
}

impl WheelDir {
    /// The `code` of a wheel entry.
    pub fn code(self) -> u8 {
        match self {
            WheelDir::Up => 1,
            WheelDir::Down => 2,
        }
    }

    pub fn from_code(code: u8) -> Option<WheelDir> {
        match code {
            1 => Some(WheelDir::Up),
            2 => Some(WheelDir::Down),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WheelDir::Up => "Scroll up",
            WheelDir::Down => "Scroll down",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bits_and_usages_line_up() {
        for (i, m) in Modifier::ALL.into_iter().enumerate() {
            assert_eq!(m.bit(), 1 << i);
            assert_eq!(Modifier::from_bit(m.bit()), Some(m));
            assert_eq!(Modifier::from_usage(m.usage()), Some(m));
        }
        assert_eq!(Modifier::from_usage(0xE8), None);
        assert_eq!(modifier_mask_label(0x11), "Left Ctrl or Right Ctrl");
    }

    #[test]
    fn key_names_match_the_hid_tables() {
        assert_eq!(key_name(0x04), Some("A"));
        assert_eq!(key_name(0x1D), Some("Z"));
        assert_eq!(key_name(0x1E), Some("1"));
        assert_eq!(key_name(0x27), Some("0"));
        assert_eq!(key_name(0x45), Some("F12"));
        assert_eq!(key_name(0x68), Some("F13"));
        assert_eq!(key_name(0x73), Some("F24"));
        assert_eq!(key_name(0x59), Some("Num 1"));
        assert_eq!(key_name(0x61), Some("Num 9"));
        assert_eq!(key_label(0x99), "Key 0x99");
    }

    #[test]
    fn code_names_map_to_usages() {
        assert_eq!(usage_from_code_name("KeyA"), Some(0x04));
        assert_eq!(usage_from_code_name("KeyZ"), Some(0x1D));
        assert_eq!(usage_from_code_name("Digit1"), Some(0x1E));
        assert_eq!(usage_from_code_name("Digit0"), Some(0x27));
        assert_eq!(usage_from_code_name("Numpad0"), Some(0x62));
        assert_eq!(usage_from_code_name("Numpad9"), Some(0x61));
        assert_eq!(usage_from_code_name("NumpadEnter"), Some(0x58));
        assert_eq!(usage_from_code_name("F1"), Some(0x3A));
        assert_eq!(usage_from_code_name("F12"), Some(0x45));
        assert_eq!(usage_from_code_name("F13"), Some(0x68));
        assert_eq!(usage_from_code_name("F24"), Some(0x73));
        assert_eq!(usage_from_code_name("F25"), None);
        assert_eq!(usage_from_code_name("Fn"), None);
        assert_eq!(usage_from_code_name("Keya"), None);
        assert_eq!(usage_from_code_name("ShiftRight"), Some(0xE5));
        // every named code lands on a named key
        for code in ["Escape", "ArrowUp", "Backquote", "IntlBackslash", "ContextMenu", "PrintScreen"] {
            let usage = usage_from_code_name(code).expect("known code");
            assert!(key_name(usage).is_some(), "{code}");
        }
    }

    #[test]
    fn mouse_bits() {
        assert_eq!(MouseButton::Back.bit(), 0x08);
        assert_eq!(MouseButton::Forward.bit(), 0x10);
        assert_eq!(MouseButton::from_bit(0x80), Some(MouseButton::Button8));
        assert_eq!(MouseButton::from_bit(0x03), None);
        assert_eq!(mouse_buttons_in(0x05).collect::<Vec<_>>(), vec![MouseButton::Left, MouseButton::Middle]);
    }
}
