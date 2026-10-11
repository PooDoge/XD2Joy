//! The Joy-Con 2 outputs a binding can drive: buttons on each half and the left stick's four
//! directions.

use crate::Side;

/// Every button the firmware can press, both halves. GL and GR are the grip buttons (only seen by
/// the console in the charging grip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    // left half
    Up,
    Down,
    Left,
    Right,
    L,
    Zl,
    Minus,
    LeftStickPress,
    Capture,
    LeftSl,
    LeftSr,
    Gl,
    // right half
    A,
    B,
    X,
    Y,
    R,
    Zr,
    Plus,
    RightStickPress,
    Home,
    C,
    RightSl,
    RightSr,
    Gr,
}

impl Button {
    pub const ALL: [Button; 25] = [
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
        Button::L,
        Button::Zl,
        Button::Minus,
        Button::LeftStickPress,
        Button::Capture,
        Button::LeftSl,
        Button::LeftSr,
        Button::Gl,
        Button::A,
        Button::B,
        Button::X,
        Button::Y,
        Button::R,
        Button::Zr,
        Button::Plus,
        Button::RightStickPress,
        Button::Home,
        Button::C,
        Button::RightSl,
        Button::RightSr,
        Button::Gr,
    ];

    /// Which Joy-Con half the button is on.
    pub fn half(self) -> Side {
        match self {
            Button::Up
            | Button::Down
            | Button::Left
            | Button::Right
            | Button::L
            | Button::Zl
            | Button::Minus
            | Button::LeftStickPress
            | Button::Capture
            | Button::LeftSl
            | Button::LeftSr
            | Button::Gl => Side::Left,
            _ => Side::Right,
        }
    }

    /// The bit in that half's report (`mask` in a blob entry).
    pub fn mask(self) -> u16 {
        match self {
            Button::Down => 0x0001,
            Button::Right => 0x0002,
            Button::Left => 0x0004,
            Button::Up => 0x0008,
            Button::L => 0x0010,
            Button::Zl => 0x0020,
            Button::Minus => 0x0040,
            Button::LeftStickPress => 0x0080,
            Button::Capture => 0x0100,
            Button::Gl => 0x0400,
            Button::LeftSr => 0x4000,
            Button::LeftSl => 0x8000,
            Button::B => 0x0001,
            Button::A => 0x0002,
            Button::Y => 0x0004,
            Button::X => 0x0008,
            Button::R => 0x0010,
            Button::Zr => 0x0020,
            Button::Plus => 0x0040,
            // RS and GR are not named in the firmware; these bits mirror the left half, as the web
            // tool assumes.
            Button::RightStickPress => 0x0080,
            Button::Home => 0x0100,
            Button::Gr => 0x0400,
            Button::C => 0x1000,
            Button::RightSr => 0x4000,
            Button::RightSl => 0x8000,
        }
    }

    /// The button with exactly this bit on this half; `None` for zero, several bits or an unused
    /// bit.
    pub fn from_mask(half: Side, mask: u16) -> Option<Button> {
        Button::ALL.into_iter().find(|b| b.half() == half && b.mask() == mask)
    }

    pub fn label(self) -> &'static str {
        match self {
            Button::Up => "Up",
            Button::Down => "Down",
            Button::Left => "Left",
            Button::Right => "Right",
            Button::L => "L",
            Button::Zl => "ZL",
            Button::Minus => "Minus",
            Button::LeftStickPress => "Stick press",
            Button::Capture => "Capture",
            Button::LeftSl => "SL",
            Button::LeftSr => "SR",
            Button::Gl => "GL",
            Button::A => "A",
            Button::B => "B",
            Button::X => "X",
            Button::Y => "Y",
            Button::R => "R",
            Button::Zr => "ZR",
            Button::Plus => "Plus",
            Button::RightStickPress => "Stick press",
            Button::Home => "Home",
            Button::C => "C",
            Button::RightSl => "SL",
            Button::RightSr => "SR",
            Button::Gr => "GR",
        }
    }

    /// The name the web tool's preset files use for this button (`L|ZL`, `R|L:UP`, ...).
    pub fn preset_id(self) -> &'static str {
        match self {
            Button::Up => "UP",
            Button::Down => "DOWN",
            Button::Left => "LEFT",
            Button::Right => "RIGHT",
            Button::L => "L",
            Button::Zl => "ZL",
            Button::Minus => "MINUS",
            Button::LeftStickPress => "LS",
            Button::Capture => "CAP",
            Button::LeftSl | Button::RightSl => "SL",
            Button::LeftSr | Button::RightSr => "SR",
            Button::Gl => "GL",
            Button::A => "A",
            Button::B => "B",
            Button::X => "X",
            Button::Y => "Y",
            Button::R => "R",
            Button::Zr => "ZR",
            Button::Plus => "PLUS",
            Button::RightStickPress => "RS",
            Button::Home => "HOME",
            Button::C => "C",
            Button::Gr => "GR",
        }
    }

    pub fn from_preset_id(half: Side, id: &str) -> Option<Button> {
        Button::ALL.into_iter().find(|b| b.half() == half && b.preset_id() == id)
    }
}

/// A direction of the left stick. Keys pushed together add up (W + D = up-right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StickDir {
    Up,
    Down,
    Left,
    Right,
}

impl StickDir {
    pub const ALL: [StickDir; 4] = [StickDir::Up, StickDir::Down, StickDir::Left, StickDir::Right];

    /// The `JC_STICK(dx, dy)` mask: low byte int8 dx, high byte int8 dy, +dy is up.
    pub fn mask(self) -> u16 {
        match self {
            StickDir::Up => 0x0100,
            StickDir::Down => 0xFF00,
            StickDir::Left => 0x00FF,
            StickDir::Right => 0x0001,
        }
    }

    pub fn from_mask(mask: u16) -> Option<StickDir> {
        StickDir::ALL.into_iter().find(|d| d.mask() == mask)
    }

    pub fn label(self) -> &'static str {
        match self {
            StickDir::Up => "Stick up",
            StickDir::Down => "Stick down",
            StickDir::Left => "Stick left",
            StickDir::Right => "Stick right",
        }
    }

    pub fn preset_id(self) -> &'static str {
        match self {
            StickDir::Up => "STICK_UP",
            StickDir::Down => "STICK_DOWN",
            StickDir::Left => "STICK_LEFT",
            StickDir::Right => "STICK_RIGHT",
        }
    }

    pub fn from_preset_id(id: &str) -> Option<StickDir> {
        StickDir::ALL.into_iter().find(|d| d.preset_id() == id)
    }
}

/// Something a binding drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    Button(Button),
    /// Left stick only: the firmware has no key-to-right-stick entry.
    Stick(StickDir),
}

impl Target {
    pub fn half(self) -> Side {
        match self {
            Target::Button(b) => b.half(),
            Target::Stick(_) => Side::Left,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Button(b) => b.label(),
            Target::Stick(d) => d.label(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_are_unique_per_half_and_round_trip() {
        for b in Button::ALL {
            assert_eq!(Button::from_mask(b.half(), b.mask()), Some(b), "{b:?}");
            assert_eq!(Button::from_preset_id(b.half(), b.preset_id()), Some(b), "{b:?}");
            // the link reserves these bits
            assert_eq!(b.mask() & 0x2800, 0, "{b:?}");
        }
        assert_eq!(Button::ALL.iter().filter(|b| b.half() == Side::Left).count(), 12);
        assert_eq!(Button::from_mask(Side::Left, 0x0003), None);
        assert_eq!(Button::from_mask(Side::Left, 0x1000), None);
        assert_eq!(Button::from_mask(Side::Right, 0x1000), Some(Button::C));
    }

    #[test]
    fn stick_masks_are_signed_offsets() {
        for d in StickDir::ALL {
            let m = d.mask();
            let (dx, dy) = ((m & 0xFF) as u8 as i8, (m >> 8) as u8 as i8);
            let want = match d {
                StickDir::Up => (0, 1),
                StickDir::Down => (0, -1),
                StickDir::Left => (-1, 0),
                StickDir::Right => (1, 0),
            };
            assert_eq!((dx, dy), want);
            assert_eq!(StickDir::from_mask(m), Some(d));
            assert_eq!(StickDir::from_preset_id(d.preset_id()), Some(d));
        }
    }
}
