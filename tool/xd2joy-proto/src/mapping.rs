//! The mapping as a person thinks of it ("W pushes the stick up", "Back presses ZL"), turned into
//! the two boards' blob entries and back.
//!
//! The firmware ORs every entry, so any number of inputs may press one button and one input may
//! press several buttons. The model is therefore a plain list of (source, target) pairs; whether
//! a new binding replaces an old one is the UI's choice, not a rule here. Entries this tool cannot
//! show as a binding (unknown kinds, multi-button masks, kinds stored on the wrong board) are kept
//! as they are and written back, so applying a change never loses them.

use crate::blob::{Colours, ConfigBlob, Entry, Kind, MAX_ENTRIES};
use crate::hid::{self, Modifier, MouseButton, WheelDir};
use crate::joycon::{Button, StickDir, Target};
use crate::{Mode, Side};

/// A physical input on the keyboard (left board) or the mouse (right board).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// A keyboard usage that is not a modifier.
    Key(u8),
    /// A set of modifier bits; the entry fires while **any** of them is held.
    Modifiers(u8),
    /// A set of mouse button bits; fires while any of them is held.
    Mouse(u8),
    Wheel(WheelDir),
}

impl Source {
    pub fn modifier(m: Modifier) -> Source {
        Source::Modifiers(m.bit())
    }

    pub fn mouse(b: MouseButton) -> Source {
        Source::Mouse(b.bit())
    }

    /// The board the input arrives on, which is the board that stores the entry.
    pub fn board(self) -> Side {
        match self {
            Source::Key(_) | Source::Modifiers(_) => Side::Left,
            Source::Mouse(_) | Source::Wheel(_) => Side::Right,
        }
    }

    pub fn label(self) -> String {
        match self {
            Source::Key(u) => hid::key_label(u),
            Source::Modifiers(m) => hid::modifier_mask_label(m),
            Source::Mouse(bits) => {
                let names: Vec<_> = hid::mouse_buttons_in(bits).map(MouseButton::label).collect();
                if names.is_empty() { "no button".to_owned() } else { names.join(" or ") }
            }
            Source::Wheel(d) => d.label().to_owned(),
        }
    }
}

/// One input driving one output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Binding {
    pub source: Source,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindError {
    #[error("{0} is a modifier; bind it as a modifier, not a key")]
    ModifierAsKey(String),
    #[error("that input has no code")]
    Empty,
    #[error("only keyboard keys can push the left stick")]
    StickNeedsKey,
}

impl Binding {
    pub fn new(source: Source, target: Target) -> Result<Binding, BindError> {
        let b = Binding { source, target };
        b.entry()?;
        Ok(b)
    }

    /// The entry on the source's board. Targets on the other half go over the board-to-board
    /// link (the `*Peer` kinds).
    pub fn entry(self) -> Result<Entry, BindError> {
        let own = self.target.half() == self.source.board();
        match (self.source, self.target) {
            (Source::Key(0), _) | (Source::Modifiers(0), _) | (Source::Mouse(0), _) => Err(BindError::Empty),
            (Source::Key(u), _) if hid::MODIFIER_USAGES.contains(&u) => {
                Err(BindError::ModifierAsKey(hid::key_label(u)))
            }
            (Source::Key(u), Target::Stick(d)) => Ok(Entry::new(Kind::Stick, u, d.mask())),
            (_, Target::Stick(_)) => Err(BindError::StickNeedsKey),
            (Source::Key(u), Target::Button(b)) => {
                Ok(Entry::new(if own { Kind::Key } else { Kind::Peer }, u, b.mask()))
            }
            (Source::Modifiers(m), Target::Button(b)) => {
                Ok(Entry::new(if own { Kind::Mod } else { Kind::PeerMod }, m, b.mask()))
            }
            (Source::Mouse(m), Target::Button(b)) => {
                Ok(Entry::new(if own { Kind::Mouse } else { Kind::MousePeer }, m, b.mask()))
            }
            (Source::Wheel(w), Target::Button(b)) => {
                Ok(Entry::new(if own { Kind::Wheel } else { Kind::WheelPeer }, w.code(), b.mask()))
            }
        }
    }

    /// The binding an entry stands for, or `None` if it is not one (a setting, a hotkey, a mask
    /// with several buttons, or an unknown kind).
    pub fn from_entry(board: Side, e: Entry) -> Option<Binding> {
        let kind = e.kind()?;
        if kind.board() != board {
            return None;
        }
        let other = match board {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        let button = |half| Button::from_mask(half, e.mask).map(Target::Button);
        let (source, target) = match kind {
            Kind::Key => (Source::Key(e.code), button(board)?),
            Kind::Peer => (Source::Key(e.code), button(other)?),
            Kind::Mod => (Source::Modifiers(e.code), button(board)?),
            Kind::PeerMod => (Source::Modifiers(e.code), button(other)?),
            Kind::Stick => (Source::Key(e.code), Target::Stick(StickDir::from_mask(e.mask)?)),
            Kind::Mouse => (Source::Mouse(e.code), button(board)?),
            Kind::MousePeer => (Source::Mouse(e.code), button(other)?),
            Kind::Wheel => (Source::Wheel(WheelDir::from_code(e.code)?), button(board)?),
            Kind::WheelPeer => (Source::Wheel(WheelDir::from_code(e.code)?), button(other)?),
            Kind::Mode | Kind::Set => return None,
        };
        // Round-trip check: anything that would not encode back to the same bytes stays raw.
        let b = Binding { source, target };
        (b.entry().ok() == Some(e)).then_some(b)
    }
}

/// What drives one axis of the right stick in stick mode (`stick_src_x/y`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StickSource {
    #[default]
    None,
    MouseX,
    MouseY,
    Wheel,
}

impl StickSource {
    pub const ALL: [StickSource; 4] = [StickSource::None, StickSource::MouseX, StickSource::MouseY, StickSource::Wheel];

    pub fn byte(self) -> u8 {
        match self {
            StickSource::None => 0,
            StickSource::MouseX => 1,
            StickSource::MouseY => 2,
            StickSource::Wheel => 3,
        }
    }

    pub fn from_byte(b: u8) -> Option<StickSource> {
        StickSource::ALL.into_iter().find(|s| s.byte() == b)
    }

    pub fn label(self) -> &'static str {
        match self {
            StickSource::None => "Nothing",
            StickSource::MouseX => "Mouse left/right",
            StickSource::MouseY => "Mouse up/down",
            StickSource::Wheel => "Scroll wheel",
        }
    }
}

/// The right board's settings (`JC_BTN_SET` entries), in the firmware's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    /// Stick-mode sensitivity, 1..=10.
    pub stick_sens: u16,
    /// Pointer gain in percent, 10..=1000.
    pub mouse_gain: u16,
    pub accel: bool,
    /// Stick-mode anti-deadzone, 0..=400 (the web tool shows it divided by 10).
    pub stick_deadzone: u16,
    /// Stick-mode motion speed, 1..=64 (the web tool's slider stops at 20).
    pub stick_speed: u16,
}

pub const STICK_SENS: std::ops::RangeInclusive<u16> = 1..=10;
pub const MOUSE_GAIN: std::ops::RangeInclusive<u16> = 10..=1000;
pub const STICK_DEADZONE: std::ops::RangeInclusive<u16> = 0..=400;
pub const STICK_SPEED: std::ops::RangeInclusive<u16> = 1..=64;

const SET_STICK_SENS: u8 = 1;
const SET_MOUSE_GAIN: u8 = 2;
const SET_ACCEL: u8 = 3;
const SET_STICK_DZ: u8 = 4;
const SET_STICK_SPEED: u8 = 5;

impl Tuning {
    /// What the firmware runs with when the blob has no setting entries at all.
    pub const FIRMWARE_FALLBACK: Tuning =
        Tuning { stick_sens: 5, mouse_gain: 250, accel: false, stick_deadzone: 290, stick_speed: 5 };

    /// The values in the firmware's built-in right blob; the "Default" buttons go back to these.
    pub const DEFAULT: Tuning =
        Tuning { stick_sens: 10, mouse_gain: 420, accel: true, stick_deadzone: 290, stick_speed: 5 };

    /// Applies one setting entry the way `cfg_settings_apply` does: out of range is ignored.
    /// Returns false for an unknown setting id.
    fn apply(&mut self, id: u8, v: u16) -> bool {
        match id {
            // The firmware accepts 1..=512 but the effect stops growing at 10.
            SET_STICK_SENS if (1..=512).contains(&v) => self.stick_sens = v.min(*STICK_SENS.end()),
            SET_MOUSE_GAIN if MOUSE_GAIN.contains(&v) => self.mouse_gain = v,
            SET_ACCEL => self.accel = v != 0,
            SET_STICK_DZ if STICK_DEADZONE.contains(&v) => self.stick_deadzone = v,
            SET_STICK_SPEED if STICK_SPEED.contains(&v) => self.stick_speed = v,
            SET_STICK_SENS | SET_MOUSE_GAIN | SET_STICK_DZ | SET_STICK_SPEED => {}
            _ => return false,
        }
        true
    }

    fn entries(&self) -> [Entry; 5] {
        let clamp = |v: u16, r: &std::ops::RangeInclusive<u16>| v.clamp(*r.start(), *r.end());
        [
            Entry::new(Kind::Set, SET_STICK_SENS, clamp(self.stick_sens, &STICK_SENS)),
            Entry::new(Kind::Set, SET_MOUSE_GAIN, clamp(self.mouse_gain, &MOUSE_GAIN)),
            Entry::new(Kind::Set, SET_ACCEL, u16::from(self.accel)),
            Entry::new(Kind::Set, SET_STICK_DZ, clamp(self.stick_deadzone, &STICK_DEADZONE)),
            Entry::new(Kind::Set, SET_STICK_SPEED, clamp(self.stick_speed, &STICK_SPEED)),
        ]
    }
}

/// A key plus the modifiers that must be held with it. Extra modifiers held at the same time
/// still match (`(mods & want) == want` in the firmware).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub key: u8,
    pub mods: u8,
}

impl Hotkey {
    pub fn label(self) -> String {
        let mut parts: Vec<String> = hid::modifiers_in(self.mods).map(|m| m.short().to_owned()).collect();
        parts.push(hid::key_label(self.key));
        parts.join(" + ")
    }

    /// True if pressing `self` also triggers `other` (same key, and `other` needs no modifier
    /// `self` lacks). The firmware checks this way, so Ctrl+K also fires a plain K hotkey.
    pub fn triggers(self, other: Hotkey) -> bool {
        self.key == other.key && self.mods & other.mods == other.mods
    }

    pub fn clashes(self, other: Hotkey) -> bool {
        self.triggers(other) || other.triggers(self)
    }
}

/// A left-board hotkey that toggles a mode (pressing it again goes back to Joy-Con).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModeHotkey {
    pub mode: Mode,
    pub key: Hotkey,
}

impl ModeHotkey {
    fn entry(self) -> Entry {
        Entry::new(Kind::Mode, self.key.key, u16::from(self.mode.firmware_id()) << 8 | u16::from(self.key.mods))
    }

    fn from_entry(e: Entry) -> Option<ModeHotkey> {
        if e.kind() != Some(Kind::Mode) {
            return None;
        }
        let mode = Mode::from_firmware_id((e.mask >> 8) as u8)?;
        Some(ModeHotkey { mode, key: Hotkey { key: e.code, mods: e.mask as u8 } })
    }
}

/// What a board's blob holds that the mapping does not model; written back unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub stick_gain: u16,
    /// Left board only (the right board's are `Mapping::right_stick`).
    pub stick_src: [u8; 2],
    pub entries: Vec<Entry>,
}

/// Both boards' configuration, as one editable thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub bindings: Vec<Binding>,
    /// Right stick X and Y sources in stick mode.
    pub right_stick: [StickSource; 2],
    pub tuning: Tuning,
    pub mode_keys: Vec<ModeHotkey>,
    /// Indexed by `Side::index`.
    pub colours: [Colours; 2],
    pub kept: [Kept; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapError {
    #[error("the {} board would need {count} entries; it has room for 32", side_word(*side))]
    TooMany { side: Side, count: usize },
    #[error("a {} board's config was given for the {} board", side_word(*got), side_word(*want))]
    WrongSide { want: Side, got: Side },
}

fn side_word(s: Side) -> &'static str {
    match s {
        Side::Left => "left",
        Side::Right => "right",
    }
}

impl Mapping {
    /// The built-in mapping of fresh boards.
    pub fn firmware_default() -> Mapping {
        let blobs = [ConfigBlob::firmware_default(Side::Left), ConfigBlob::firmware_default(Side::Right)];
        match Mapping::from_blobs(&blobs[0], &blobs[1]) {
            Ok(m) => m,
            // Both blobs are built for the right sides; tested below.
            Err(_) => Mapping::empty(),
        }
    }

    fn empty() -> Mapping {
        let kept = || Kept { stick_gain: 500, stick_src: [0, 0], entries: Vec::new() };
        Mapping {
            bindings: Vec::new(),
            right_stick: [StickSource::None; 2],
            tuning: Tuning::FIRMWARE_FALLBACK,
            mode_keys: Vec::new(),
            colours: [Colours::FIRMWARE_DEFAULT; 2],
            kept: [kept(), kept()],
        }
    }

    pub fn from_blobs(left: &ConfigBlob, right: &ConfigBlob) -> Result<Mapping, MapError> {
        let mut m = Mapping::empty();
        for (want, blob) in [(Side::Left, left), (Side::Right, right)] {
            if blob.side != want {
                return Err(MapError::WrongSide { want, got: blob.side });
            }
            m.read_board(blob);
        }
        Ok(m)
    }

    /// Replaces one board's part with what its blob says; the other board's part is untouched.
    pub fn read_board(&mut self, blob: &ConfigBlob) {
        let side = blob.side;
        self.bindings.retain(|b| b.source.board() != side);
        let mut kept = Vec::new();
        match side {
            Side::Left => self.mode_keys.clear(),
            Side::Right => {
                self.tuning = Tuning::FIRMWARE_FALLBACK;
                self.right_stick = blob.stick_src.map(|b| StickSource::from_byte(b).unwrap_or_default());
            }
        }
        for &e in &blob.entries {
            if let Some(b) = Binding::from_entry(side, e) {
                self.bindings.push(b);
                continue;
            }
            match (side, e.kind()) {
                (Side::Left, Some(Kind::Mode)) => match ModeHotkey::from_entry(e) {
                    Some(h) => self.mode_keys.push(h),
                    None => kept.push(e),
                },
                // In-range or not, a known setting is folded into `tuning`: the firmware ignores
                // an out-of-range one, and so does the rewritten blob.
                (Side::Right, Some(Kind::Set)) => {
                    if !self.tuning.apply(e.code, e.mask) {
                        kept.push(e);
                    }
                }
                _ => kept.push(e),
            }
        }
        // On the right the sources live in `right_stick` (an unknown byte reads as None, which
        // is also what the firmware does with it); the left board ignores them, so keep them.
        let stick_src = match side {
            Side::Left => blob.stick_src,
            Side::Right => [0, 0],
        };
        self.colours[side.index()] = blob.colours;
        self.kept[side.index()] = Kept { stick_gain: blob.stick_gain, stick_src, entries: kept };
    }

    /// The entries this mapping puts on one board, in write order: bindings, then hotkeys or
    /// settings, then kept entries.
    pub fn entries(&self, side: Side) -> Vec<Entry> {
        let mut out: Vec<Entry> =
            self.bindings.iter().filter(|b| b.source.board() == side).filter_map(|b| b.entry().ok()).collect();
        match side {
            Side::Left => out.extend(self.mode_keys.iter().map(|h| h.entry())),
            Side::Right => out.extend(self.tuning.entries()),
        }
        out.extend(self.kept[side.index()].entries.iter().copied());
        out
    }

    /// How many of the board's 32 entries this mapping uses.
    pub fn entries_used(&self, side: Side) -> usize {
        self.entries(side).len()
    }

    pub fn to_blob(&self, side: Side) -> Result<ConfigBlob, MapError> {
        let entries = self.entries(side);
        if entries.len() > MAX_ENTRIES {
            return Err(MapError::TooMany { side, count: entries.len() });
        }
        let kept = &self.kept[side.index()];
        let stick_src = match side {
            Side::Left => kept.stick_src,
            Side::Right => self.right_stick.map(StickSource::byte),
        };
        Ok(ConfigBlob { side, stick_gain: kept.stick_gain, stick_src, colours: self.colours[side.index()], entries })
    }

    /// Would both mappings make this board behave the same? This is the "unapplied changes" test.
    /// Entry order is ignored: the firmware ORs entries, and each setting is written once.
    pub fn same_on_board(&self, other: &Mapping, side: Side) -> bool {
        let canon = |m: &Mapping| {
            m.to_blob(side).map(|mut b| {
                b.entries.sort_by_key(|e| (e.kind, e.code, e.mask));
                b
            })
        };
        canon(self) == canon(other)
    }

    /// Adds a binding unless the same pair is already there. Fails, changing nothing, when the
    /// source's board is out of entries.
    pub fn add(&mut self, b: Binding) -> Result<bool, MapError> {
        if self.bindings.contains(&b) {
            return Ok(false);
        }
        let side = b.source.board();
        let count = self.entries_used(side) + 1;
        if count > MAX_ENTRIES {
            return Err(MapError::TooMany { side, count });
        }
        self.bindings.push(b);
        Ok(true)
    }

    /// Binds `source` to `target` and removes the other targets that source had, which is what
    /// "rebind" means in the mapper. Returns the bindings it removed, for undo.
    pub fn rebind(&mut self, b: Binding) -> Result<Vec<Binding>, MapError> {
        let removed: Vec<Binding> =
            self.bindings.iter().copied().filter(|x| x.source == b.source && x.target != b.target).collect();
        let before = self.bindings.clone();
        self.bindings.retain(|x| !removed.contains(x));
        match self.add(b) {
            Ok(_) => Ok(removed),
            Err(e) => {
                self.bindings = before;
                Err(e)
            }
        }
    }

    pub fn remove(&mut self, b: Binding) -> bool {
        let n = self.bindings.len();
        self.bindings.retain(|x| *x != b);
        self.bindings.len() != n
    }

    pub fn sources_of(&self, target: Target) -> impl Iterator<Item = Source> + '_ {
        self.bindings.iter().filter(move |b| b.target == target).map(|b| b.source)
    }

    pub fn targets_of(&self, source: Source) -> impl Iterator<Item = Target> + '_ {
        self.bindings.iter().filter(move |b| b.source == source).map(|b| b.target)
    }

    /// Is any input bound to HOME? The right board wakes a sleeping console when HOME is held, so
    /// a mapping without one cannot wake it.
    pub fn has_home(&self) -> bool {
        self.sources_of(Target::Button(Button::Home)).next().is_some()
    }

    pub fn mode_key(&self, mode: Mode) -> Option<Hotkey> {
        self.mode_keys.iter().find(|h| h.mode == mode).map(|h| h.key)
    }

    /// Sets or clears the hotkey of one mode, keeping at most one per mode.
    pub fn set_mode_key(&mut self, mode: Mode, key: Option<Hotkey>) {
        self.mode_keys.retain(|h| h.mode != mode);
        if let Some(key) = key {
            self.mode_keys.push(ModeHotkey { mode, key });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Mapping {
        Mapping::firmware_default()
    }

    #[test]
    fn firmware_defaults_decode_fully_and_write_back_exactly() {
        let m = defaults();
        // 27 left entries = 25 bindings + 2 mode hotkeys; right = 2 bindings + 5 settings.
        assert_eq!(m.bindings.iter().filter(|b| b.source.board() == Side::Left).count(), 25);
        assert_eq!(m.bindings.iter().filter(|b| b.source.board() == Side::Right).count(), 2);
        assert!(m.kept.iter().all(|k| k.entries.is_empty()));
        assert_eq!(m.tuning, Tuning::DEFAULT);
        assert_eq!(m.right_stick, [StickSource::None, StickSource::Wheel]);
        assert_eq!(m.mode_key(Mode::Keyboard), Some(Hotkey { key: 0x0E, mods: 0x01 }));
        assert_eq!(m.mode_key(Mode::Stick), Some(Hotkey { key: 0x0D, mods: 0x01 }));
        for side in Side::BOTH {
            assert_eq!(m.to_blob(side), Ok(ConfigBlob::firmware_default(side)), "{side:?}");
        }
        assert_eq!(m.entries_used(Side::Left), 27);
        assert!(m.has_home());
    }

    #[test]
    fn crossmaps_use_the_link_kinds() {
        let w = Source::Key(0x1A);
        let peer = Binding::new(w, Target::Button(Button::A)).and_then(Binding::entry);
        assert_eq!(peer, Ok(Entry::new(Kind::Peer, 0x1A, 0x0002)));
        let back = Source::mouse(MouseButton::Back);
        let mpeer = Binding::new(back, Target::Button(Button::Zl)).and_then(Binding::entry);
        assert_eq!(mpeer, Ok(Entry::new(Kind::MousePeer, 0x08, 0x0020)));
        let wheel = Binding::new(Source::Wheel(WheelDir::Down), Target::Button(Button::Up)).and_then(Binding::entry);
        assert_eq!(wheel, Ok(Entry::new(Kind::WheelPeer, 2, 0x0008)));
        let shift = Binding::new(Source::modifier(Modifier::LeftShift), Target::Button(Button::B));
        assert_eq!(shift.and_then(Binding::entry), Ok(Entry::new(Kind::PeerMod, 0x02, 0x0001)));
    }

    #[test]
    fn invalid_bindings_are_refused() {
        assert_eq!(
            Binding::new(Source::Key(0xE1), Target::Button(Button::L)),
            Err(BindError::ModifierAsKey("Left Shift".into()))
        );
        assert_eq!(
            Binding::new(Source::mouse(MouseButton::Left), Target::Stick(StickDir::Up)),
            Err(BindError::StickNeedsKey)
        );
        assert_eq!(Binding::new(Source::Key(0), Target::Button(Button::L)), Err(BindError::Empty));
    }

    #[test]
    fn stacking_both_ways_is_kept() {
        let mut m = defaults();
        let space = Source::Key(0x2C);
        // Space already presses B; add A too, and a mouse button on B as well.
        assert_eq!(m.add(Binding { source: space, target: Target::Button(Button::A) }), Ok(true));
        assert_eq!(
            m.add(Binding { source: Source::mouse(MouseButton::Back), target: Target::Button(Button::B) }),
            Ok(true)
        );
        let blobs = [m.to_blob(Side::Left), m.to_blob(Side::Right)];
        let [Ok(l), Ok(r)] = blobs else { panic!("encodes: {blobs:?}") };
        let back = Mapping::from_blobs(&l, &r).expect("decodes");
        assert_eq!(back.targets_of(space).count(), 2);
        assert_eq!(back.sources_of(Target::Button(Button::B)).count(), 2);
        assert!(Side::BOTH.iter().all(|&s| back.same_on_board(&m, s)));
    }

    #[test]
    fn rebind_moves_and_reports_what_it_removed() {
        let mut m = defaults();
        let space = Source::Key(0x2C);
        let removed = m.rebind(Binding { source: space, target: Target::Button(Button::Y) });
        assert_eq!(removed, Ok(vec![Binding { source: space, target: Target::Button(Button::B) }]));
        assert_eq!(m.targets_of(space).collect::<Vec<_>>(), vec![Target::Button(Button::Y)]);
    }

    #[test]
    fn the_budget_is_enforced_before_apply() {
        let mut m = defaults();
        // 27 used on the left: 5 more fit, the 6th does not.
        for (i, key) in (0x59..0x5E).enumerate() {
            assert_eq!(m.add(Binding { source: Source::Key(key), target: Target::Button(Button::L) }), Ok(true), "{i}");
        }
        let over = m.add(Binding { source: Source::Key(0x5E), target: Target::Button(Button::L) });
        assert_eq!(over, Err(MapError::TooMany { side: Side::Left, count: 33 }));
        assert_eq!(m.entries_used(Side::Left), 32);
        // the right board is unaffected
        assert!(m.to_blob(Side::Right).is_ok());
    }

    #[test]
    fn unknown_and_odd_entries_survive_a_round_trip() {
        let mut left = ConfigBlob::firmware_default(Side::Left);
        let odd = [
            Entry { code: 1, kind: 99, mask: 2 },  // unknown kind
            Entry::new(Kind::Key, 0x04, 0x0003),   // two buttons at once
            Entry::new(Kind::Mouse, 0x01, 0x0010), // a right-board kind on the left
            Entry::new(Kind::Stick, 0x04, 0x0101), // diagonal
            Entry::new(Kind::Mode, 0x0A, 3 << 8),  // mode the firmware ignores
        ];
        left.entries.extend(odd);
        left.stick_src = [7, 9];
        let mut right = ConfigBlob::firmware_default(Side::Right);
        right.entries.push(Entry::new(Kind::Set, 77, 5)); // unknown setting
        right.entries.push(Entry::new(Kind::Wheel, 3, 0x0001)); // tilt, not in protocol 1
        let m = Mapping::from_blobs(&left, &right).expect("decodes");
        assert_eq!(m.kept[0].entries, odd);
        assert_eq!(m.kept[1].entries.len(), 2);
        assert_eq!(m.to_blob(Side::Left), Ok(left));
        let r = m.to_blob(Side::Right).expect("encodes");
        assert_eq!(&r.entries[r.entries.len() - 2..], &right.entries[right.entries.len() - 2..]);
    }

    #[test]
    fn settings_follow_the_firmware_rules() {
        let mut right = ConfigBlob::firmware_default(Side::Right);
        right.entries.retain(|e| e.kind() != Some(Kind::Set));
        let m = Mapping::from_blobs(&ConfigBlob::firmware_default(Side::Left), &right).expect("decodes");
        assert_eq!(m.tuning, Tuning::FIRMWARE_FALLBACK);

        // out of range is ignored, last one wins, sensitivity is capped at 10
        right.entries.push(Entry::new(Kind::Set, 2, 5000));
        right.entries.push(Entry::new(Kind::Set, 2, 300));
        right.entries.push(Entry::new(Kind::Set, 2, 9));
        right.entries.push(Entry::new(Kind::Set, 1, 200));
        let m = Mapping::from_blobs(&ConfigBlob::firmware_default(Side::Left), &right).expect("decodes");
        assert_eq!(m.tuning.mouse_gain, 300);
        assert_eq!(m.tuning.stick_sens, 10);
        assert!(m.kept[1].entries.is_empty());
    }

    #[test]
    fn reading_one_board_leaves_the_other_alone() {
        let mut m = defaults();
        m.tuning.mouse_gain = 600;
        let mut left = ConfigBlob::firmware_default(Side::Left);
        left.entries.truncate(3);
        m.read_board(&left);
        assert_eq!(m.tuning.mouse_gain, 600);
        assert_eq!(m.bindings.iter().filter(|b| b.source.board() == Side::Left).count(), 3);
        assert!(m.mode_keys.is_empty());
    }

    #[test]
    fn wrong_side_is_an_error() {
        let l = ConfigBlob::firmware_default(Side::Left);
        assert_eq!(Mapping::from_blobs(&l, &l), Err(MapError::WrongSide { want: Side::Right, got: Side::Left }));
    }

    #[test]
    fn hotkeys_match_like_the_firmware() {
        let k = Hotkey { key: 0x0E, mods: 0 };
        let ctrl_k = Hotkey { key: 0x0E, mods: 0x01 };
        assert!(ctrl_k.triggers(k));
        assert!(!k.triggers(ctrl_k));
        assert!(k.clashes(ctrl_k));
        assert!(!ctrl_k.clashes(Hotkey { key: 0x0E, mods: 0x10 }));
        assert_eq!(ctrl_k.label(), "Ctrl + K");
    }
}
