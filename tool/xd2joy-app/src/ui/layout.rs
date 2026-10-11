//! Where things go on the drawings: the keyboard's keys (a full-size ANSI layout), the mouse's
//! buttons, and the Joy-Con 2 buttons. Plain data, so it can be checked by tests.

use xd2joy_proto::hid::{Modifier, MouseButton, WheelDir};
use xd2joy_proto::{Button, Source, StickDir, Target};

/// One cap on the on-screen keyboard: a usage and a width in key units, or a gap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cap {
    Key { usage: u8, label: &'static str, w: f32 },
    Gap(f32),
}

const fn k(usage: u8, label: &'static str) -> Cap {
    Cap::Key { usage, label, w: 1.0 }
}

const fn kw(usage: u8, label: &'static str, w: f32) -> Cap {
    Cap::Key { usage, label, w }
}

/// The main block, six rows of 15 units.
pub const MAIN: [&[Cap]; 6] = [
    &[
        k(0x29, "Esc"),
        Cap::Gap(1.0),
        k(0x3A, "F1"),
        k(0x3B, "F2"),
        k(0x3C, "F3"),
        k(0x3D, "F4"),
        Cap::Gap(0.5),
        k(0x3E, "F5"),
        k(0x3F, "F6"),
        k(0x40, "F7"),
        k(0x41, "F8"),
        Cap::Gap(0.5),
        k(0x42, "F9"),
        k(0x43, "F10"),
        k(0x44, "F11"),
        k(0x45, "F12"),
    ],
    &[
        k(0x35, "`"),
        k(0x1E, "1"),
        k(0x1F, "2"),
        k(0x20, "3"),
        k(0x21, "4"),
        k(0x22, "5"),
        k(0x23, "6"),
        k(0x24, "7"),
        k(0x25, "8"),
        k(0x26, "9"),
        k(0x27, "0"),
        k(0x2D, "-"),
        k(0x2E, "="),
        kw(0x2A, "Backspace", 2.0),
    ],
    &[
        kw(0x2B, "Tab", 1.5),
        k(0x14, "Q"),
        k(0x1A, "W"),
        k(0x08, "E"),
        k(0x15, "R"),
        k(0x17, "T"),
        k(0x1C, "Y"),
        k(0x18, "U"),
        k(0x0C, "I"),
        k(0x12, "O"),
        k(0x13, "P"),
        k(0x2F, "["),
        k(0x30, "]"),
        kw(0x31, "\\", 1.5),
    ],
    &[
        kw(0x39, "Caps", 1.75),
        k(0x04, "A"),
        k(0x16, "S"),
        k(0x07, "D"),
        k(0x09, "F"),
        k(0x0A, "G"),
        k(0x0B, "H"),
        k(0x0D, "J"),
        k(0x0E, "K"),
        k(0x0F, "L"),
        k(0x33, ";"),
        k(0x34, "'"),
        kw(0x28, "Enter", 2.25),
    ],
    &[
        kw(0xE1, "Shift", 2.25),
        k(0x1D, "Z"),
        k(0x1B, "X"),
        k(0x06, "C"),
        k(0x19, "V"),
        k(0x05, "B"),
        k(0x11, "N"),
        k(0x10, "M"),
        k(0x36, ","),
        k(0x37, "."),
        k(0x38, "/"),
        kw(0xE5, "Shift", 2.75),
    ],
    &[
        kw(0xE0, "Ctrl", 1.25),
        kw(0xE3, "Win", 1.25),
        kw(0xE2, "Alt", 1.25),
        kw(0x2C, "Space", 6.25),
        kw(0xE6, "Alt", 1.25),
        kw(0xE7, "Win", 1.25),
        kw(0x65, "Menu", 1.25),
        kw(0xE4, "Ctrl", 1.25),
    ],
];

/// The navigation cluster, three units wide, same rows as `MAIN`.
pub const NAV: [&[Cap]; 6] = [
    &[k(0x46, "PrtSc"), k(0x47, "ScrLk"), k(0x48, "Pause")],
    &[k(0x49, "Ins"), k(0x4A, "Home"), k(0x4B, "PgUp")],
    &[k(0x4C, "Del"), k(0x4D, "End"), k(0x4E, "PgDn")],
    &[],
    &[Cap::Gap(1.0), k(0x52, "↑")],
    &[k(0x50, "←"), k(0x51, "↓"), k(0x4F, "→")],
];

/// The number pad, four units wide.
pub const NUMPAD: [&[Cap]; 6] = [
    &[],
    &[k(0x53, "Num"), k(0x54, "/"), k(0x55, "*"), k(0x56, "-")],
    &[k(0x5F, "7"), k(0x60, "8"), k(0x61, "9"), k(0x57, "+")],
    &[k(0x5C, "4"), k(0x5D, "5"), k(0x5E, "6")],
    &[k(0x59, "1"), k(0x5A, "2"), k(0x5B, "3"), k(0x58, "Ent")],
    &[kw(0x62, "0", 2.0), k(0x63, ".")],
];

/// The binding source a key cap stands for: modifiers bind through the modifier byte.
pub fn key_source(usage: u8) -> Source {
    match Modifier::from_usage(usage) {
        Some(m) => Source::modifier(m),
        None => Source::Key(usage),
    }
}

/// Is a source held right now, given the live keys, modifiers and mouse buttons?
pub fn is_held(source: Source, keys: &[u8], mods: u8, buttons: u8) -> bool {
    match source {
        Source::Key(u) => keys.contains(&u),
        Source::Modifiers(m) => mods & m != 0,
        Source::Mouse(b) => buttons & b != 0,
        // the wheel is a pulse; there is no held state to show
        Source::Wheel(_) => false,
    }
}

/// A shape on a drawing, in pixels from the drawing's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Corner radius; half the size makes a circle.
    pub r: f32,
}

const fn spot(x: f32, y: f32, w: f32, h: f32, r: f32) -> Spot {
    Spot { x, y, w, h, r }
}

const fn round(x: f32, y: f32, d: f32) -> Spot {
    spot(x, y, d, d, d / 2.)
}

/// The mouse drawing: 150 x 250.
pub const MOUSE_SIZE: (f32, f32) = (150., 250.);

pub fn mouse_spots() -> Vec<(Source, &'static str, Spot)> {
    vec![
        (Source::mouse(MouseButton::Left), "L", spot(12., 10., 48., 92., 24.)),
        (Source::mouse(MouseButton::Right), "R", spot(90., 10., 48., 92., 24.)),
        (Source::Wheel(WheelDir::Up), "▲", spot(63., 26., 24., 22., 6.)),
        (Source::mouse(MouseButton::Middle), "M", spot(63., 50., 24., 18., 6.)),
        (Source::Wheel(WheelDir::Down), "▼", spot(63., 70., 24., 22., 6.)),
        (Source::mouse(MouseButton::Forward), "M5", spot(0., 112., 26., 30., 6.)),
        (Source::mouse(MouseButton::Back), "M4", spot(0., 146., 26., 30., 6.)),
        (Source::mouse(MouseButton::Button6), "M6", spot(124., 112., 26., 26., 6.)),
        (Source::mouse(MouseButton::Button7), "M7", spot(124., 142., 26., 26., 6.)),
        (Source::mouse(MouseButton::Button8), "M8", spot(124., 172., 26., 26., 6.)),
    ]
}

/// A Joy-Con drawing: 170 x 340, the body plus room for the shoulder and grip buttons.
pub const JOYCON_SIZE: (f32, f32) = (170., 340.);
/// The body inside the drawing.
pub const BODY: Spot = spot(26., 34., 118., 290., 0.);

/// The left Joy-Con: rail on the right edge, stick above the D-pad.
pub fn left_spots() -> Vec<(Target, &'static str, Spot)> {
    let b = |x: Button| Target::Button(x);
    let s = |d: StickDir| Target::Stick(d);
    vec![
        (b(Button::Zl), "ZL", spot(26., 0., 70., 16., 8.)),
        (b(Button::L), "L", spot(26., 18., 80., 14., 7.)),
        (b(Button::Minus), "−", round(112., 50., 20.)),
        (s(StickDir::Up), "▲", spot(63., 54., 24., 14., 4.)),
        (s(StickDir::Left), "◀", spot(36., 82., 14., 24., 4.)),
        (b(Button::LeftStickPress), "LS", round(54., 72., 42.)),
        (s(StickDir::Right), "▶", spot(100., 82., 14., 24., 4.)),
        (s(StickDir::Down), "▼", spot(63., 118., 24., 14., 4.)),
        (b(Button::Up), "▲", round(62., 150., 26.)),
        (b(Button::Left), "◀", round(34., 178., 26.)),
        (b(Button::Right), "▶", round(90., 178., 26.)),
        (b(Button::Down), "▼", round(62., 206., 26.)),
        (b(Button::Capture), "◉", spot(88., 248., 22., 22., 5.)),
        (b(Button::LeftSl), "SL", spot(150., 74., 18., 44., 6.)),
        (b(Button::LeftSr), "SR", spot(150., 228., 18., 44., 6.)),
        (b(Button::Gl), "GL", spot(0., 300., 40., 22., 6.)),
    ]
}

/// The right Joy-Con: rail on the left edge, face buttons above the stick.
pub fn right_spots() -> Vec<(Target, &'static str, Spot)> {
    let b = |x: Button| Target::Button(x);
    vec![
        (b(Button::Zr), "ZR", spot(74., 0., 70., 16., 8.)),
        (b(Button::R), "R", spot(64., 18., 80., 14., 7.)),
        (b(Button::Plus), "+", round(38., 50., 20.)),
        (b(Button::X), "X", round(82., 62., 26.)),
        (b(Button::Y), "Y", round(54., 90., 26.)),
        (b(Button::A), "A", round(110., 90., 26.)),
        (b(Button::B), "B", round(82., 118., 26.)),
        (b(Button::RightStickPress), "RS", round(64., 164., 42.)),
        (b(Button::Home), "⌂", round(60., 230., 24.)),
        (b(Button::C), "C", round(90., 230., 24.)),
        (b(Button::RightSl), "SL", spot(2., 228., 18., 44., 6.)),
        (b(Button::RightSr), "SR", spot(2., 74., 18., 44., 6.)),
        (b(Button::Gr), "GR", spot(130., 300., 40., 22., 6.)),
    ]
}

/// The order targets are listed next to each drawing.
pub fn target_order(half: xd2joy_proto::Side) -> Vec<Target> {
    let spots = match half {
        xd2joy_proto::Side::Left => left_spots(),
        xd2joy_proto::Side::Right => right_spots(),
    };
    spots.into_iter().map(|(t, _, _)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use xd2joy_proto::Side;

    fn caps() -> Vec<(u8, f32)> {
        MAIN.iter()
            .chain(NAV.iter())
            .chain(NUMPAD.iter())
            .flat_map(|row| row.iter())
            .filter_map(|c| match c {
                Cap::Key { usage, w, .. } => Some((*usage, *w)),
                Cap::Gap(_) => None,
            })
            .collect()
    }

    #[test]
    fn every_key_appears_once_and_has_a_name() {
        let all = caps();
        let unique: HashSet<u8> = all.iter().map(|(u, _)| *u).collect();
        assert_eq!(unique.len(), all.len(), "a usage is drawn twice");
        for (u, _) in &all {
            assert!(xd2joy_proto::hid::key_name(*u).is_some(), "0x{u:02X}");
        }
        // 104-key ANSI
        assert_eq!(all.len(), 104);
    }

    #[test]
    fn main_rows_are_fifteen_units() {
        for (i, row) in MAIN.iter().enumerate() {
            let w: f32 = row
                .iter()
                .map(|c| match c {
                    Cap::Key { w, .. } => *w,
                    Cap::Gap(g) => *g,
                })
                .sum();
            // the function row is 15 with its gaps; the others are exactly 15
            assert!((w - 15.0).abs() < 0.01 || i == 0 && w <= 15.0, "row {i} is {w}");
        }
    }

    #[test]
    fn every_target_is_drawn_once_on_its_half() {
        let mut seen = HashSet::new();
        for (half, spots) in [(Side::Left, left_spots()), (Side::Right, right_spots())] {
            for (t, _, s) in spots {
                assert_eq!(t.half(), half, "{t:?}");
                assert!(seen.insert(t), "{t:?} twice");
                assert!(s.x + s.w <= JOYCON_SIZE.0 && s.y + s.h <= JOYCON_SIZE.1, "{t:?} outside");
            }
        }
        assert_eq!(seen.len(), 25 + 4);
    }

    #[test]
    fn modifier_caps_bind_the_modifier_byte() {
        assert_eq!(key_source(0xE1), Source::Modifiers(0x02));
        assert_eq!(key_source(0x04), Source::Key(0x04));
        assert!(is_held(Source::Modifiers(0x02), &[], 0x03, 0));
        assert!(!is_held(Source::Key(4), &[5], 0, 0));
    }

    #[test]
    fn mouse_spots_fit() {
        for (_, _, s) in mouse_spots() {
            assert!(s.x + s.w <= MOUSE_SIZE.0 && s.y + s.h <= MOUSE_SIZE.1);
        }
    }
}
