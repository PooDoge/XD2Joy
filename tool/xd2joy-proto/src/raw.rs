//! Decoding `[RAW]` reports so the app can show which keys and buttons are held and let a person
//! bind by pressing them.
//!
//! This is best-effort, like the web tool's capture: the board prints a report only when it
//! changes, shows at most 32 bytes, stops printing in keyboard mode and while wired input is
//! active, and the app does not see the device's report map. So the layouts are guessed from the
//! length the same way the firmware's own fallback does: keyboards of 8 (boot) or 7 bytes, mice
//! of 4-8 bytes with the buttons in byte 0. Clicking a key on the on-screen drawing always works.

use std::collections::BTreeMap;

use crate::Side;
use crate::hid::{MODIFIER_USAGES, WheelDir};
use crate::reply::RawReport;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyboardState {
    pub mods: u8,
    /// Usages held, without zeros and the "rollover" error code 1.
    pub keys: Vec<u8>,
}

/// Mouse state from one report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseState {
    pub buttons: u8,
    pub dx: i16,
    pub dy: i16,
    pub wheel: i8,
}

pub fn decode_keyboard(r: &RawReport) -> Option<KeyboardState> {
    let start = match (r.len, r.bytes.len()) {
        (8, 8) => 2,
        (7, 7) => 1,
        _ => return None,
    };
    let keys = r.bytes[start..].iter().copied().filter(|&k| k > 1 && !MODIFIER_USAGES.contains(&k)).collect();
    Some(KeyboardState { mods: r.bytes[0], keys })
}

pub fn decode_mouse(r: &RawReport) -> Option<MouseState> {
    let d = &r.bytes;
    if d.len() != r.len {
        return None;
    }
    let i16_at = |o: usize| i16::from_le_bytes([d[o], d[o + 1]]);
    let (dx, dy, wheel) = match r.len {
        // 12-bit packed X and Y
        7 => {
            let x = u16::from(d[2]) | (u16::from(d[3] & 0x0F) << 8);
            let y = u16::from(d[3] >> 4) | (u16::from(d[4]) << 4);
            let s12 = |v: u16| ((v << 4) as i16) >> 4;
            (s12(x), s12(y), d[5] as i8)
        }
        6 | 8 => (i16_at(1), i16_at(3), d[5] as i8),
        4 | 5 => (i16::from(d[1] as i8), i16::from(d[2] as i8), d[3] as i8),
        _ => return None,
    };
    Some(MouseState { buttons: d[0], dx, dy, wheel })
}

/// A press seen while capturing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    Key(u8),
    /// Modifier bits that went down; the UI waits briefly so Ctrl+Shift can land as one.
    Modifiers(u8),
    /// The lowest mouse button bit that went down.
    Mouse(u8),
    Wheel(WheelDir),
}

/// The latest report per handle for both boards, which is the held state because reports are
/// printed only on change.
#[derive(Debug, Clone, Default)]
pub struct LiveInput {
    keyboards: BTreeMap<u16, KeyboardState>,
    mice: BTreeMap<u16, MouseState>,
    /// The left board's reports that are neither layout (media keys, vendor reports): the "this
    /// key sent no usable code" case.
    unreadable: u32,
}

impl LiveInput {
    /// Takes a report from `side` and returns the new presses it carries, oldest first.
    pub fn feed(&mut self, side: Side, r: &RawReport) -> Vec<Pressed> {
        match side {
            Side::Left => {
                let Some(now) = decode_keyboard(r) else {
                    if r.bytes.iter().any(|&b| b != 0) {
                        self.unreadable += 1;
                    }
                    return Vec::new();
                };
                let before = self.keyboards.insert(r.handle, now.clone()).unwrap_or_default();
                let mut out: Vec<Pressed> =
                    now.keys.iter().filter(|k| !before.keys.contains(k)).map(|&k| Pressed::Key(k)).collect();
                let new_mods = now.mods & !before.mods;
                if new_mods != 0 {
                    out.push(Pressed::Modifiers(new_mods));
                }
                out
            }
            Side::Right => {
                let Some(now) = decode_mouse(r) else { return Vec::new() };
                let before = self.mice.insert(r.handle, now).unwrap_or_default();
                let new_buttons = now.buttons & !before.buttons;
                if new_buttons != 0 {
                    return vec![Pressed::Mouse(new_buttons & new_buttons.wrapping_neg())];
                }
                match now.wheel {
                    w if w > 0 && now.buttons == 0 => vec![Pressed::Wheel(WheelDir::Up)],
                    w if w < 0 && now.buttons == 0 => vec![Pressed::Wheel(WheelDir::Down)],
                    _ => Vec::new(),
                }
            }
        }
    }

    pub fn held_keys(&self) -> impl Iterator<Item = u8> + '_ {
        self.keyboards.values().flat_map(|k| k.keys.iter().copied())
    }

    pub fn held_mods(&self) -> u8 {
        self.keyboards.values().fold(0, |m, k| m | k.mods)
    }

    pub fn held_buttons(&self) -> u8 {
        self.mice.values().fold(0, |m, s| m | s.buttons)
    }

    /// The most recent mouse motion, for the live readout.
    pub fn last_motion(&self) -> Option<MouseState> {
        self.mice.values().next().copied()
    }

    pub fn unreadable_reports(&self) -> u32 {
        self.unreadable
    }

    /// Forget one board's state (it disconnected or went into keyboard mode, where reports stop
    /// and the last one would look held forever).
    pub fn clear(&mut self, side: Side) {
        match side {
            Side::Left => self.keyboards.clear(),
            Side::Right => self.mice.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(handle: u16, bytes: &[u8]) -> RawReport {
        RawReport { handle, len: bytes.len(), bytes: bytes.to_vec() }
    }

    #[test]
    fn boot_and_short_keyboards() {
        assert_eq!(
            decode_keyboard(&raw(1, &[0x02, 0, 0x1A, 0x04, 0, 0, 0, 0])),
            Some(KeyboardState { mods: 2, keys: vec![0x1A, 0x04] })
        );
        assert_eq!(
            decode_keyboard(&raw(1, &[0, 0x1A, 0, 0, 0, 0, 0])),
            Some(KeyboardState { mods: 0, keys: vec![0x1A] })
        );
        assert_eq!(decode_keyboard(&raw(1, &[0, 0x1A])), None);
        // truncated print (real length longer than what was shown)
        assert_eq!(decode_keyboard(&RawReport { handle: 1, len: 8, bytes: vec![0; 4] }), None);
    }

    #[test]
    fn mouse_layouts() {
        assert_eq!(
            decode_mouse(&raw(1, &[0x01, 0xFE, 0x05, 0x01, 0])),
            Some(MouseState { buttons: 1, dx: -2, dy: 5, wheel: 1 })
        );
        assert_eq!(
            decode_mouse(&raw(1, &[0x08, 0x10, 0x00, 0xF0, 0xFF, 0xFF, 0, 0])),
            Some(MouseState { buttons: 8, dx: 16, dy: -16, wheel: -1 })
        );
        // 12-bit packed: x = 0xFFF (-1), y = 0x002
        assert_eq!(
            decode_mouse(&raw(1, &[0x00, 0x00, 0xFF, 0x2F, 0x00, 0x01, 0x00])),
            Some(MouseState { buttons: 0, dx: -1, dy: 2, wheel: 1 })
        );
        assert_eq!(decode_mouse(&raw(1, &[0, 0, 0])), None);
    }

    #[test]
    fn presses_are_the_changes() {
        let mut live = LiveInput::default();
        assert_eq!(live.feed(Side::Left, &raw(1, &[0, 0, 0x1A, 0, 0, 0, 0, 0])), vec![Pressed::Key(0x1A)]);
        // still held: nothing new; then D joins
        assert_eq!(live.feed(Side::Left, &raw(1, &[0, 0, 0x1A, 0x07, 0, 0, 0, 0])), vec![Pressed::Key(0x07)]);
        assert_eq!(live.feed(Side::Left, &raw(1, &[0x01, 0, 0x1A, 0x07, 0, 0, 0, 0])), vec![Pressed::Modifiers(1)]);
        assert_eq!(live.held_keys().collect::<Vec<_>>(), vec![0x1A, 0x07]);
        assert_eq!(live.held_mods(), 1);
        // a media-key report is counted as unreadable
        assert!(live.feed(Side::Left, &raw(2, &[0xE9, 0x00])).is_empty());
        assert_eq!(live.unreadable_reports(), 1);

        assert_eq!(live.feed(Side::Right, &raw(3, &[0x18, 0, 0, 0])), vec![Pressed::Mouse(0x08)]);
        assert_eq!(live.held_buttons(), 0x18);
        assert!(live.feed(Side::Right, &raw(3, &[0x18, 0, 0, 1])).is_empty());
        assert_eq!(live.feed(Side::Right, &raw(3, &[0, 0, 0, 0xFF])), vec![Pressed::Wheel(WheelDir::Down)]);
        live.clear(Side::Left);
        assert_eq!(live.held_keys().count(), 0);
    }
}
